using System;
using System.IO;
using System.Reflection;
using LanGame.SaveControl;

// Uses the production initializer's verifier and real, lawfully acquired DLLs.
// The caller supplies read-only packages and a fresh external output root.
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
        if (args.Length < 2) throw new ArgumentException("Expected package roots and an external output root");
        int count = args.Length - 1;
        string mixed = Path.Combine(args[count], "mixed");
        Directory.CreateDirectory(mixed);
        string server = Path.Combine(mixed, "DedicatedServer.dll"), core = Path.Combine(mixed, "BarotraumaCore.dll");
        for (int i = 0; i < count; i++)
        {
            Verify(args[i], true);
            File.Copy(Path.Combine(args[i], "DedicatedServer.dll"), server, true);
            for (int j = 0; j < count; j++)
            {
                if (i == j) continue;
                File.Copy(Path.Combine(args[j], "BarotraumaCore.dll"), core, true);
                Verify(mixed, false);
            }
            File.WriteAllText(core, "unknown core");
            Verify(mixed, false);
            File.Copy(Path.Combine(args[i], "BarotraumaCore.dll"), core, true);
            File.WriteAllText(server, "unknown server");
            Verify(mixed, false);
        }
        Console.WriteLine("PASS Barotrauma real-package admission: {0} complete versions, {1} mixed pairs rejected, {2} unknown server/core combinations rejected",
            count, count * (count - 1), 2 * count);
        return 0;
    }
}
