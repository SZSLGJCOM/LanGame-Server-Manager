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
    private static bool requirePrimaryWrite;
    private const string CompanionServer = "8b39b613abc8de1f53248f085d36b9646af79e6a1f7028e3491354d6d7a82a02";
    private static readonly string[,] SupportedBuilds = {
        { CompanionServer, "e04362c36714bb5e64d610eb05d94b29168829a5f52da600b09b303836ce53cb" },
        { "d058bbde56286699aa3ffcdd95e87d4d1e430b184ecc3c4007ea442ee9518a46",
          "48d637d505a2af5136feaafa871e5057e1f280a56f502d08e52f372816efe606" },
        { "18278624996b543a2595a0ea9c0c4c14881d511ac307e1a69028d6579e67ad3c",
          "72acd77784b83a0e09584262ab1eea9ee40395b95f3a2e3ec3974da0bc0151b3" }
    };
    private const BindingFlags Static = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static;
    private const BindingFlags Instance = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance;

    public static void Initialize()
    {
        try
        {
            string root = Path.GetDirectoryName(Assembly.GetExecutingAssembly().Location);
            string runtime = Path.GetDirectoryName(root);
            requirePrimaryWrite = VerifyNativeBuild(runtime) == CompanionServer;
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
    private static string VerifyNativeBuild(string runtime)
    {
        for (int i = 0; i < SupportedBuilds.GetLength(0); i++)
        {
            try { NativeBinding.VerifyFile(Path.Combine(runtime, "DedicatedServer.dll"), SupportedBuilds[i, 0]); }
            catch (SaveFailure failure)
            {
                if (failure.Code != "unsupported_native_build") throw;
                continue;
            }
            NativeBinding.VerifyFile(Path.Combine(runtime, "BarotraumaCore.dll"), SupportedBuilds[i, 1]);
            return SupportedBuilds[i, 0];
        }
        throw new SaveFailure("unsupported_native_build");
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
            object currentPath = CurrentCheckpoint(server, session, campaign, submarine, destination);
            if (currentPath == null)
                throw new SaveFailure("world_context_changed");
            FileInfo before = requirePrimaryWrite ? ReadPrimary(destination) : null;
            // The game can swallow disk errors. Observe them within this call
            // on its owner thread, including exceptions during stream disposal.
            bool failed;
            using (NativeSaveExceptions observer = new NativeSaveExceptions())
            {
                method.Invoke(null, new object[] {currentPath, false});
                failed = observer.Failed;
            }
            if (failed || CurrentCheckpoint(server, session, campaign, submarine, destination) == null)
                throw new SaveFailure("native_outcome_unknown");
            if (requirePrimaryWrite)
            {
                FileInfo after = ReadPrimary(destination);
                if (after == null || after.Length == 0 || (before != null
                    && before.Length == after.Length && before.LastWriteTimeUtc == after.LastWriteTimeUtc))
                    throw new SaveFailure("native_outcome_unknown");
            }
            return SaveReceipt.Completed;
        };
    }

    private static object CurrentCheckpoint(object server, object session, object campaign, object submarine, string destination)
    {
        Type type = session.GetType();
        if (!Object.ReferenceEquals(Member(main, null, "Server", Static), server)
            || !Object.ReferenceEquals(Member(main, null, "GameSession", Static), session)
            || !Object.ReferenceEquals(Member(type, session, "Campaign", Instance), campaign)
            || !Object.ReferenceEquals(Member(type, session, "SubmarineInfo", Instance), submarine)
            || Member(campaign.GetType(), campaign, "PendingSubmarineSwitch", Instance) != null
            || Boolean(server.GetType(), server, "GameStarted") || Boolean(type, session, "IsRunning")) return null;
        object path = Member(type, session, "DataPath", Instance);
        return path != null && String.Equals(destination,
            Member(path.GetType(), path, "SavePath", Instance) as string, StringComparison.Ordinal) ? path : null;
    }

    private static FileInfo ReadPrimary(string destination)
    {
        try
        {
            if ((File.GetAttributes(destination) & (FileAttributes.Directory | FileAttributes.ReparsePoint)) != 0)
                throw new SaveFailure("native_outcome_unknown");
            // Keep the cached metadata without holding a handle across the save.
            var file = new FileInfo(destination);
            file.Refresh();
            return file;
        }
        catch (FileNotFoundException) { return null; }
        catch (DirectoryNotFoundException) { return null; }
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
