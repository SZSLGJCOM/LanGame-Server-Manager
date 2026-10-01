using System;
using System.Collections.Generic;
using System.Reflection;
using System.Threading;

internal static class ForestWorldReadinessTests
{
    public static bool DedicatedHost, FinishGameLoad, IsSuspended;
    private static readonly List<string> lines = new List<string>();
    public static void Log(object value) { lines.Add((string)value); }

    private static void Set(ForestNativeControl bridge, string name, object value)
    {
        typeof(ForestNativeControl).GetField(name, BindingFlags.Instance | BindingFlags.NonPublic).SetValue(bridge, value);
    }

    private static void Expect(params string[] expected)
    {
        if (lines.Count != expected.Length)
        {
            Console.Error.WriteLine("World readiness transition count mismatch: expected=" + expected.Length + " actual=" + lines.Count);
            throw new Exception("World readiness transition count mismatch");
        }
        for (int index = 0; index < expected.Length; index++)
            if (lines[index] != "[LanGame native control] " + expected[index]) throw new Exception("World readiness transition mismatch");
    }

    internal static void Verify()
    {
        ForestNativeControl bridge = new ForestNativeControl();
        try
        {
            foreach (string name in new string[] { "peer", "scene", "level" }) Set(bridge, name, typeof(ForestWorldReadinessTests));
            foreach (string name in new string[] { "log", "logError" }) Set(bridge, name, typeof(ForestWorldReadinessTests).GetMethod("Log"));
            Set(bridge, "inputReader", Thread.CurrentThread);
            DedicatedHost = true; FinishGameLoad = false; IsSuspended = false;
            bridge.MoveNext(); Expect(); // The native early Running line occurs in this state.
            FinishGameLoad = true; IsSuspended = true;
            bridge.MoveNext(); Expect();
            DedicatedHost = false; IsSuspended = false;
            bridge.MoveNext(); Expect();
            DedicatedHost = true;
            bridge.MoveNext(); bridge.MoveNext();
            Expect("world_ready=True");
            IsSuspended = true;
            bridge.MoveNext(); bridge.MoveNext();
            Expect("world_ready=True", "world_ready=False");
            IsSuspended = false;
            bridge.MoveNext();
            Expect("world_ready=True", "world_ready=False", "world_ready=True");
            FinishGameLoad = false;
            bridge.MoveNext();
            Expect("world_ready=True", "world_ready=False", "world_ready=True", "world_ready=False");
            Console.WriteLine("The Forest world readiness: main-thread idle observation, all native gates, single transition and revocation cases passed");
        }
        finally
        {
            ((ManualResetEvent)typeof(ForestNativeControl).GetField("inputCancellation", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(bridge)).Close();
        }
    }
}
