using System;
using System.IO;
using System.Reflection;
using LanGame.SaveControl;

// Uses the production initializer's verifier and real, lawfully acquired DLLs.
// The caller supplies two read-only packages and a fresh external output root.
internal static class NativeBuildTests
{
    private static void Verify(string root, bool admitted)
    {
        MethodInfo verify = typeof(StartupHook).GetMethod("VerifyNativeBuild", BindingFlags.NonPublic | BindingFlags.Static);
        try
        {
            verify.Invoke(null, new object[] { root });
            if (!admitted) throw new Exception("An unreviewed assembly pair was admitted");
        }
        catch (TargetInvocationException error)
        {
            SaveFailure failure = error.InnerException as SaveFailure;
            if (admitted || failure == null || failure.Code != "unsupported_native_build") throw;
        }
    }
    public static int Main(string[] args)
    {
        try { return Run(args); }
        catch (Exception error)
        {
            Exception cause = error.InnerException ?? error;
            SaveFailure failure = cause as SaveFailure;
            Console.Error.WriteLine("Native admission failed: " + cause.GetType().FullName
                + (failure == null ? "" : " / " + failure.Code));
            return 1;
        }
    }
    private static int Run(string[] args)
    {
        if (args.Length != 3) throw new ArgumentException("Expected two package roots and an external output root");
        Verify(args[0], true);
        Verify(args[1], true);
        for (int direction = 0; direction < 2; direction++)
        {
            string mixed = Path.Combine(args[2], "mixed-" + direction);
            Directory.CreateDirectory(mixed);
            File.Copy(Path.Combine(args[direction], "DedicatedServer.dll"), Path.Combine(mixed, "DedicatedServer.dll"), false);
            File.Copy(Path.Combine(args[1 - direction], "BarotraumaCore.dll"), Path.Combine(mixed, "BarotraumaCore.dll"), false);
            Verify(mixed, false);
        }
        string changed = Path.Combine(args[2], "unknown");
        Directory.CreateDirectory(changed);
        File.WriteAllText(Path.Combine(changed, "DedicatedServer.dll"), "unreviewed native program");
        File.Copy(Path.Combine(args[0], "BarotraumaCore.dll"), Path.Combine(changed, "BarotraumaCore.dll"), false);
        Verify(changed, false);
        Console.WriteLine("PASS Barotrauma real-package admission: both complete versions, both mixed pairs, unknown program rejected; no game started");
        return 0;
    }
}
