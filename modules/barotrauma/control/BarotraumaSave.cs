using System;
using System.Collections;
using System.IO;
using System.Reflection;
using System.Runtime.ExceptionServices;
using System.Threading;
using LanGame.SaveControl;

// The .NET host invokes this exact type before DedicatedServer.Main. Only the
// native CrossThread task queue crosses threads; game state is touched by its owner tick.
public static class StartupHook
{
    private static SaveLoop control;
    private static Timer timer;
    private static IList queue;
    private static Type taskType;
    private static ConstructorInfo taskConstructor;
    private static Delegate tick;
    private static object pendingTask;
    private static FieldInfo taskDone, taskEvent;
    private static Type main;
    private static int stopped;
    private const BindingFlags Static = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static;
    private const BindingFlags Instance = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance;

    public static void Initialize()
    {
        try
        {
            string root = Path.GetDirectoryName(Assembly.GetExecutingAssembly().Location);
            string runtime = Path.GetDirectoryName(root);
            NativeBinding.VerifyFile(Path.Combine(runtime, "DedicatedServer.dll"),
                "18278624996b543a2595a0ea9c0c4c14881d511ac307e1a69028d6579e67ad3c");
            NativeBinding.VerifyFile(Path.Combine(runtime, "BarotraumaCore.dll"),
                "72acd77784b83a0e09584262ab1eea9ee40395b95f3a2e3ec3974da0bc0151b3");
            main = NativeBinding.Type("DedicatedServer", "Barotrauma.GameMain");
            Type crossThread = NativeBinding.Type("DedicatedServer", "Barotrauma.CrossThread");
            taskType = crossThread.GetNestedType("Task", BindingFlags.NonPublic);
            Type taskDelegate = crossThread.GetNestedType("TaskDelegate", BindingFlags.Public);
            FieldInfo tasks = crossThread.GetField("enqueuedTasks", Static);
            if (taskType == null || taskDelegate == null || tasks == null) throw new SaveFailure("api_unavailable");
            queue = tasks.GetValue(null) as IList;
            taskConstructor = taskType.GetConstructor(Instance, null, new Type[] {taskDelegate}, null);
            taskDone = taskType.GetField("Done", Instance); taskEvent = taskType.GetField("Mre", Instance);
            MethodInfo tickMethod = typeof(StartupHook).GetMethod("Tick", Static);
            if (queue == null || taskConstructor == null || tickMethod == null || taskDone == null
                || taskDone.FieldType != typeof(bool) || taskEvent == null || taskEvent.FieldType != typeof(ManualResetEvent))
                throw new SaveFailure("api_unavailable");
            tick = Delegate.CreateDelegate(taskDelegate, tickMethod);
            control = new SaveLoop(root, "barotrauma", new SaveAdapter(Prepare));
            AppDomain.CurrentDomain.ProcessExit += delegate { Stop(); };
            timer = new Timer(Enqueue, null, 200, 200);
        }
        catch (Exception error) { Fail(error); }
    }
    private static object Member(Type type, object owner, string name, BindingFlags flags)
    {
        FieldInfo field = type.GetField(name, flags);
        if (field != null) return field.GetValue(owner);
        PropertyInfo property = type.GetProperty(name, flags);
        if (property == null || !property.CanRead || property.GetIndexParameters().Length != 0)
            throw new SaveFailure("api_unavailable");
        return property.GetValue(owner, null);
    }
    private static bool Boolean(Type type, object owner, string name)
    {
        object value = Member(type, owner, name, Instance);
        if (!(value is bool)) throw new SaveFailure("api_unavailable");
        return (bool)value;
    }
    private static void Enqueue(object unused)
    {
        if (Volatile.Read(ref stopped) != 0) return;
        try
        {
            // ProcessTasks holds this same lock while a save runs. A periodic
            // timer must skip that busy tick rather than accumulate blocked
            // ThreadPool callbacks behind a slow native disk operation.
            if (!Monitor.TryEnter(queue)) return;
            try
            {
                if (pendingTask != null)
                {
                    if (!(bool)taskDone.GetValue(pendingTask)) return;
                    ((ManualResetEvent)taskEvent.GetValue(pendingTask)).Dispose();
                    pendingTask = null;
                }
                if (queue.Count >= 1024 || Member(main, null, "MainThread", Static) == null) return;
                pendingTask = taskConstructor.Invoke(new object[] {tick});
                queue.Add(pendingTask);
            }
            finally { Monitor.Exit(queue); }
        }
        catch (Exception error) { Fail(error); }
    }
    private static void Tick()
    {
        if (Volatile.Read(ref stopped) != 0) return;
        try
        {
            if (!Object.ReferenceEquals(Member(main, null, "MainThread", Static), Thread.CurrentThread))
                throw new SaveFailure("wrong_owner_thread");
            control.Update();
        }
        catch (Exception error) { Fail(error); }
    }
    private static Func<SaveReceipt> Prepare()
    {
        object server = Member(main, null, "Server", Static);
        object session = Member(main, null, "GameSession", Static);
        if (server == null || session == null) throw new SaveFailure("campaign_not_ready");
        Type sessionType = session.GetType();
        // SaveGame serializes a campaign checkpoint. Ending a live round or
        // calling SavePlayers would mutate gameplay/inventories, so both remain
        // under the game's normal round lifecycle, outside this save action.
        if (Boolean(server.GetType(), server, "GameStarted") || Boolean(sessionType, session, "IsRunning"))
            throw new SaveFailure("campaign_round_running");
        object campaign = Member(sessionType, session, "Campaign", Instance);
        if (campaign == null) throw new SaveFailure("campaign_not_ready");
        if (Member(campaign.GetType(), campaign, "PendingSubmarineSwitch", Instance) != null)
            throw new SaveFailure("campaign_submarine_switch_pending");
        object path = Member(sessionType, session, "DataPath", Instance);
        object submarine = Member(sessionType, session, "SubmarineInfo", Instance);
        if (path == null || submarine == null) throw new SaveFailure("campaign_not_ready");
        string destination = Member(path.GetType(), path, "SavePath", Instance) as string;
        if (String.IsNullOrEmpty(destination)) throw new SaveFailure("campaign_not_ready");
        Type save = NativeBinding.Type("DedicatedServer", "Barotrauma.SaveUtil");
        MethodInfo method = NativeBinding.Method(save, "SaveGame", Static, typeof(void), path.GetType(), typeof(bool));
        return delegate
        {
            // The same owner callback invokes this immediately after admission.
            if (!Object.ReferenceEquals(Member(main, null, "Server", Static), server)
                || !Object.ReferenceEquals(Member(main, null, "GameSession", Static), session)
                || !Object.ReferenceEquals(Member(sessionType, session, "Campaign", Instance), campaign)
                || Boolean(server.GetType(), server, "GameStarted") || Boolean(sessionType, session, "IsRunning"))
                throw new SaveFailure("world_context_changed");
            // In the two hash-pinned native assemblies, every SaveGame early
            // return is an exception handler. The successful path serializes
            // GameSession and submarines, then CompressDirectory disposes both
            // GZipStream and output stream. Observe exceptions *before* native
            // catches swallow them; a normal void return alone proves nothing.
            bool failed;
            using (NativeSaveExceptions observer = new NativeSaveExceptions())
            {
                method.Invoke(null, new object[] {path, false});
                failed = observer.Failed;
            }
            if (failed) throw new SaveFailure("native_outcome_unknown");
            object currentPath = Member(sessionType, session, "DataPath", Instance);
            if (!Object.ReferenceEquals(Member(main, null, "GameSession", Static), session)
                || !Object.ReferenceEquals(Member(main, null, "Server", Static), server)
                || !Object.ReferenceEquals(Member(sessionType, session, "Campaign", Instance), campaign)
                || currentPath == null || !String.Equals(destination,
                    Member(currentPath.GetType(), currentPath, "SavePath", Instance) as string, StringComparison.Ordinal))
                throw new SaveFailure("native_outcome_unknown");
            return SaveReceipt.Completed;
        };
    }

    private sealed class NativeSaveExceptions : IDisposable
    {
        private readonly int owner = Thread.CurrentThread.ManagedThreadId;
        internal bool Failed;
        internal NativeSaveExceptions() { AppDomain.CurrentDomain.FirstChanceException += Observe; }
        private void Observe(object sender, FirstChanceExceptionEventArgs error)
        {
            // No exception objects, messages or game contents leave this scope.
            // Unrelated worker-thread failures cannot acknowledge/reject it.
            if (Thread.CurrentThread.ManagedThreadId == owner) Failed = true;
        }
        public void Dispose() { AppDomain.CurrentDomain.FirstChanceException -= Observe; }
    }
    private static void Fail(Exception error)
    {
        SaveFailure known = error as SaveFailure;
        Stop();
        Console.Error.WriteLine("[LanGame save] Barotrauma control stopped: " + (known == null ? "api_unavailable" : known.Code));
    }
    private static void Stop()
    {
        if (Interlocked.Exchange(ref stopped, 1) != 0) return;
        Timer owned = timer; timer = null;
        if (owned != null) owned.Dispose();
        SaveLoop loop = control; control = null;
        if (loop != null) loop.Dispose();
    }
}
