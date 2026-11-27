using System;
using System.Collections;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
using System.Threading;
using LanGame.SaveControl;

// Compile as DedicatedServer.exe in an external test directory. Only the game
// boundary is synthetic; the production admission and task publisher run intact.
namespace Barotrauma
{
    public static class GameMain
    {
        public static GameServer Server;
        public static GameSession GameSession;
        public static Thread MainThread { get; set; }
    }
    public sealed class GameServer { public bool GameStarted { get; set; } }
    public sealed class GameSession
    {
        public bool IsRunning { get; set; }
        public Campaign Campaign { get; set; }
        public CampaignDataPath DataPath { get; set; }
        public object SubmarineInfo { get; set; }
    }
    public sealed class Campaign { public object PendingSubmarineSwitch { get; set; } }
    public sealed class CampaignDataPath { public string SavePath = "fixed-native-campaign.save"; }
    public static class SaveUtil
    {
        public static int Calls;
        public static CampaignDataPath Path;
        public static int FailureMode;
        public static Action<CampaignDataPath> AfterSave;
        public static void SaveGame(CampaignDataPath path, bool loading)
        {
            if (loading) throw new Exception("Wrong native save mode");
            Calls++; Path = path;
            if (FailureMode == 1) { try { throw new System.IO.IOException("native swallowed write failure"); } catch (System.IO.IOException) { } }
            if (FailureMode == 2) throw new System.IO.IOException("native escaped write failure");
            if (FailureMode == 3)
            {
                var worker = new Thread(delegate() { try { throw new System.IO.IOException("unrelated worker"); } catch (System.IO.IOException) { } });
                worker.Start(); worker.Join();
            }
            if (AfterSave != null) AfterSave(path);
        }
    }
    public static class CrossThread
    {
        public delegate void TaskDelegate();
        public sealed class Task
        {
            public TaskDelegate Deleg;
            public ManualResetEvent Mre = new ManualResetEvent(false);
            public bool Done;
            public Task(TaskDelegate action) { Deleg = action; }
        }
    }
}
internal static class BarotraumaControlTests
{
    private const BindingFlags Flags = BindingFlags.NonPublic | BindingFlags.Static;
    private static readonly Type Hook = typeof(StartupHook);
    private static void Set(string name, object value) { Hook.GetField(name, Flags).SetValue(null, value); }
    private static Func<SaveReceipt> Prepare()
    {
        return (Func<SaveReceipt>)Hook.GetMethod("Prepare", Flags).Invoke(null, null);
    }
    private static bool Rejected(string issue)
    {
        int before = Barotrauma.SaveUtil.Calls;
        try { Prepare(); return false; }
        catch (TargetInvocationException error)
        {
            return error.InnerException.Message == issue && Barotrauma.SaveUtil.Calls == before;
        }
    }
    private static void ExpectFailure(Func<SaveReceipt> invoke, string expected)
    {
        try { invoke(); }
        catch (SaveFailure failure) { if (failure.Code == expected) return; throw; }
        throw new Exception("Expected " + expected);
    }
    private static void CheckCompanionCheckpoint()
    {
        var session = Barotrauma.GameMain.GameSession;
        string savePath = Path.Combine(Path.GetDirectoryName(Assembly.GetExecutingAssembly().Location), "campaign.save");
        session.DataPath.SavePath = savePath;
        Set("requirePrimaryWrite", true);
        ExpectFailure(Prepare(), "native_outcome_unknown");
        Barotrauma.SaveUtil.AfterSave = delegate { File.WriteAllText(savePath, "checkpoint"); };
        if (Prepare()() != SaveReceipt.Completed) throw new Exception("New primary was not confirmed");

        // A silent return, an unrelated write, and an empty primary give no confirmation.
        Barotrauma.SaveUtil.AfterSave = delegate { File.WriteAllText(savePath + ".bk0", "backup"); };
        ExpectFailure(Prepare(), "native_outcome_unknown");
        Barotrauma.SaveUtil.AfterSave = delegate { File.WriteAllText(savePath, ""); };
        ExpectFailure(Prepare(), "native_outcome_unknown");
        Barotrauma.SaveUtil.AfterSave = delegate { File.WriteAllText(savePath, "new checkpoint"); };
        if (Prepare()() != SaveReceipt.Completed) throw new Exception("Changed primary was not confirmed");
        File.SetLastWriteTimeUtc(savePath, new DateTime(2001, 1, 1, 0, 0, 0, DateTimeKind.Utc));
        if (Prepare()() != SaveReceipt.Completed) throw new Exception("Same-length rewrite was not confirmed");

        int calls = Barotrauma.SaveUtil.Calls;
        var invoke = Prepare();
        session.DataPath.SavePath = savePath + ".other";
        ExpectFailure(invoke, "world_context_changed");
        session.DataPath.SavePath = savePath;
        invoke = Prepare();
        session.Campaign.PendingSubmarineSwitch = new object();
        ExpectFailure(invoke, "world_context_changed");
        session.Campaign.PendingSubmarineSwitch = null;
        if (Barotrauma.SaveUtil.Calls != calls) throw new Exception("Changed campaign was written");

        // Native SaveGame may normalize the data path object for this same primary.
        Barotrauma.SaveUtil.AfterSave = delegate {
            File.WriteAllText(savePath, "normalized path");
            session.DataPath = new Barotrauma.CampaignDataPath { SavePath = savePath };
        };
        if (Prepare()() != SaveReceipt.Completed) throw new Exception("Same native path was rejected");
        Barotrauma.SaveUtil.AfterSave = delegate {
            File.WriteAllText(savePath, "changed session");
            Barotrauma.GameMain.GameSession = new Barotrauma.GameSession();
        };
        ExpectFailure(Prepare(), "native_outcome_unknown");
        Barotrauma.SaveUtil.AfterSave = null;
    }
    public static int Main()
    {
        Set("main", typeof(Barotrauma.GameMain));
        Barotrauma.GameMain.MainThread = Thread.CurrentThread;
        if (!Rejected("campaign_not_ready")) return 1;
        Barotrauma.GameMain.Server = new Barotrauma.GameServer();
        Barotrauma.GameMain.GameSession = new Barotrauma.GameSession();
        if (!Rejected("campaign_not_ready")) return 2;
        Barotrauma.GameMain.GameSession.Campaign = new Barotrauma.Campaign();
        Barotrauma.GameMain.GameSession.DataPath = new Barotrauma.CampaignDataPath();
        Barotrauma.GameMain.GameSession.SubmarineInfo = new object();
        Barotrauma.GameMain.Server.GameStarted = true;
        if (!Rejected("campaign_round_running")) return 3;
        Barotrauma.GameMain.Server.GameStarted = false;
        Barotrauma.GameMain.GameSession.IsRunning = true;
        if (!Rejected("campaign_round_running")) return 4;
        Barotrauma.GameMain.GameSession.IsRunning = false;
        Barotrauma.GameMain.GameSession.Campaign.PendingSubmarineSwitch = new object();
        if (!Rejected("campaign_submarine_switch_pending")) return 5;
        Barotrauma.GameMain.GameSession.Campaign.PendingSubmarineSwitch = null;
        Func<SaveReceipt> invoke = Prepare();
        if (invoke() != SaveReceipt.Completed || Barotrauma.SaveUtil.Calls != 1
            || !Object.ReferenceEquals(Barotrauma.SaveUtil.Path, Barotrauma.GameMain.GameSession.DataPath)) return 6;
        invoke = Prepare();
        Barotrauma.GameMain.Server.GameStarted = true;
        try { invoke(); return 7; } catch (LanGame.SaveControl.SaveFailure) { }
        if (Barotrauma.SaveUtil.Calls != 1) return 8;
        Barotrauma.GameMain.Server.GameStarted = false;
        invoke = Prepare(); Barotrauma.GameMain.GameSession = new Barotrauma.GameSession();
        try { invoke(); return 9; } catch (LanGame.SaveControl.SaveFailure) { }
        if (Barotrauma.SaveUtil.Calls != 1) return 10;
        Barotrauma.GameMain.GameSession = new Barotrauma.GameSession {
            Campaign = new Barotrauma.Campaign(), DataPath = new Barotrauma.CampaignDataPath(), SubmarineInfo = new object()
        };
        Barotrauma.SaveUtil.FailureMode = 1;
        try { Prepare()(); return 15; } catch (SaveFailure error) { if (error.Code != "native_outcome_unknown") return 16; }
        Barotrauma.SaveUtil.FailureMode = 2;
        try { Prepare()(); return 17; } catch (TargetInvocationException error) { if (!(error.InnerException is System.IO.IOException)) return 18; }
        Barotrauma.SaveUtil.FailureMode = 3;
        if (Prepare()() != SaveReceipt.Completed) return 19;
        Barotrauma.SaveUtil.FailureMode = 0;
        if (Prepare()() != SaveReceipt.Completed) return 20;
        CheckCompanionCheckpoint();

        var queue = new List<Barotrauma.CrossThread.Task>();
        Set("queue", queue); Set("taskType", typeof(Barotrauma.CrossThread.Task));
        Set("taskConstructor", typeof(Barotrauma.CrossThread.Task).GetConstructor(new Type[] {typeof(Barotrauma.CrossThread.TaskDelegate)}));
        Set("taskDone", typeof(Barotrauma.CrossThread.Task).GetField("Done"));
        Set("taskEvent", typeof(Barotrauma.CrossThread.Task).GetField("Mre"));
        Set("tick", new Barotrauma.CrossThread.TaskDelegate(delegate { }));
        MethodInfo enqueue = Hook.GetMethod("Enqueue", Flags);
        enqueue.Invoke(null, new object[] {null});
        enqueue.Invoke(null, new object[] {null});
        if (queue.Count != 1) return 11;
        var first = queue[0];
        lock (queue) { first.Deleg(); first.Mre.Set(); first.Done = true; queue.Clear(); }
        enqueue.Invoke(null, new object[] {null});
        if (queue.Count != 1) return 12;
        try { first.Mre.WaitOne(0); return 13; } catch (ObjectDisposedException) { }
        // A busy owner tick must not accumulate Timer callbacks waiting on it.
        using (var owns = new ManualResetEvent(false))
        using (var release = new ManualResetEvent(false))
        {
            var owner = new Thread(delegate() { lock (queue) { owns.Set(); release.WaitOne(); } });
            owner.Start(); owns.WaitOne();
            var publisher = new Thread(delegate() { enqueue.Invoke(null, new object[] {null}); });
            publisher.Start(); bool completed = publisher.Join(1000);
            release.Set(); owner.Join(); publisher.Join();
            if (!completed || queue.Count != 1) return 14;
        }
        queue[0].Mre.Dispose();
        TextWriter original = Console.Error;
        using (var diagnostic = new StringWriter())
        {
            Console.SetError(diagnostic);
            try
            {
                var other = new Thread(delegate() { Hook.GetMethod("Tick", Flags).Invoke(null, null); });
                other.Start(); other.Join();
            }
            finally { Console.SetError(original); }
            if (!diagnostic.ToString().Contains("wrong_owner_thread")) return 21;
        }
        Console.WriteLine("PASS Barotrauma production policy: campaign and path guards, fixed-target primary creation and rewrite, unchanged/empty primary rejection, native failure observation, bounded owner queue");
        return 0;
    }
}
