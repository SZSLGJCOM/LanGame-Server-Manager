using System;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
using System.Threading;

internal static class ForestSaveRequestTests
{
    public static bool DedicatedHost = true, FinishGameLoad = true, IsSuspended;
    public static string PlayerName = "request-test";
    private static string root;
    private static int saves;
    private static bool fail;
    private static readonly List<string> lines = new List<string>();
    public static void Log(object value) { lines.Add((string)value); }
    public static string GetLocalSlotPath() { return root; }
    public static void SaveGame()
    {
        saves++;
        if (fail) throw new IOException();
        string path = Path.Combine(root, PlayerName + "__RESUME__");
        if (File.Exists(path + "prev")) File.Delete(path + "prev");
        if (File.Exists(path)) File.Move(path, path + "prev");
        File.WriteAllText(path, "synchronous checkpoint " + saves);
    }
    private static void Set(ForestNativeControl bridge, string name, object value)
    {
        typeof(ForestNativeControl).GetField(name, BindingFlags.Instance | BindingFlags.NonPublic).SetValue(bridge, value);
    }
    private static string Nonce(long now, int serial) { return now.ToString("x16") + serial.ToString("x16"); }
    private static void Expect(bool condition) { if (!condition) throw new Exception("Correlated save request contract failed"); }
    private static void Send(ForestNativeControl bridge, string command)
    {
        ((Queue<string>)typeof(ForestNativeControl).GetField("commands", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(bridge)).Enqueue(command);
        bridge.MoveNext();
    }
    internal static void Verify()
    {
        uint before = unchecked((uint)Environment.TickCount);
        long now = ForestSaveRequests.NowMilliseconds();
        uint after = unchecked((uint)Environment.TickCount);
        Expect(now >= 0 && unchecked((uint)now - before) <= unchecked(after - before));
        string nonce = Nonce(now, 1);
        foreach (string command in new string[] { "save", "save " + nonce + " ", " SAVE " + nonce, "save " + nonce.ToUpperInvariant(), "save " + nonce + "x", "save " + nonce.Substring(1) })
            Expect(ForestSaveRequests.Nonce(command) == null);
        Expect(ForestSaveRequests.Nonce("save " + nonce) == nonce);
        ForestSaveRequests requests = new ForestSaveRequests();
        Expect(requests.Admit(nonce, now) == null);
        Expect(requests.Admit(nonce, now) == "replayed");
        Expect(requests.Admit(Nonce(now - 10001, 2), now) == "expired");
        Expect(requests.Admit(Nonce(now + 2001, 3), now) == "expired");
        Expect(requests.Admit("ffffffffffffffff0000000000000000", now) == "expired");
        for (int at = 2; at <= 1024; at++) Expect(requests.Admit(Nonce(now, at), now) == null);
        Expect(requests.Admit(Nonce(now, 1025), now) == "request_capacity");
        Expect(requests.Admit(Nonce(now + 10001, 1025), now + 10001) == null);
        Expect(requests.Admit(nonce, now + 10001) == "expired");

        root = Path.Combine(Path.GetTempPath(), "lgsm-forest-request-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root);
        ForestNativeControl bridge = new ForestNativeControl();
        try
        {
            foreach (string name in new string[] { "peer", "scene", "level" }) Set(bridge, name, typeof(ForestSaveRequestTests));
            foreach (string name in new string[] { "log", "logError" }) Set(bridge, name, typeof(ForestSaveRequestTests).GetMethod("Log"));
            Set(bridge, "save", typeof(ForestSaveRequestTests).GetMethod("SaveGame"));
            Set(bridge, "savePath", typeof(ForestSaveRequestTests).GetMethod("GetLocalSlotPath"));
            Set(bridge, "inputReader", Thread.CurrentThread);
            Send(bridge, "save " + nonce);
            Expect(saves == 1 && lines[lines.Count - 1] == "[LanGame native control] " + ForestSaveRequests.Completed(nonce, new FileInfo(Path.Combine(root, PlayerName + "__RESUME__")).Length));
            Send(bridge, "save " + nonce);
            Expect(saves == 1 && lines[lines.Count - 1].EndsWith("nonce=" + nonce + " code=replayed", StringComparison.Ordinal));
            IsSuspended = true;
            string refused = Nonce(now, 2001);
            Send(bridge, "save " + refused);
            Expect(saves == 1 && lines[lines.Count - 1].EndsWith("nonce=" + refused + " code=world_not_ready", StringComparison.Ordinal));
            IsSuspended = false; fail = true;
            string failed = Nonce(now, 2002);
            Send(bridge, "save " + failed);
            Expect(saves == 2 && lines[lines.Count - 1].EndsWith("nonce=" + failed + " code=invoke_IOException", StringComparison.Ordinal));
            fail = false;
            Send(bridge, "save");
            Expect(saves == 3 && lines[lines.Count - 1].StartsWith("[LanGame native control] native_save_completed bytes=", StringComparison.Ordinal));
            Console.WriteLine("The Forest correlated save: strict framing, expiration, bounded replay, native checkpoint result, readiness and exception cases passed");
        }
        finally
        {
            ((ManualResetEvent)typeof(ForestNativeControl).GetField("inputCancellation", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(bridge)).Close();
            Directory.Delete(root, true);
        }
    }
}
