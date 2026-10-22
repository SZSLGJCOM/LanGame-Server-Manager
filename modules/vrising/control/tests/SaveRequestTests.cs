using System;
using System.IO;
using System.Reflection;
using LanGame.SaveControl;
using LanGame.VRising;

namespace BepInEx
{
    public sealed class BepInPlugin : Attribute { public BepInPlugin(string id, string name, string version) { } }
    public sealed class TestLog { public void LogError(string value) { } }
}
namespace BepInEx.Unity.IL2CPP
{
    public abstract class BasePlugin
    {
        public readonly BepInEx.TestLog Log = new BepInEx.TestLog();
        public abstract void Load();
        public virtual bool Unload() { return true; }
    }
}
namespace HarmonyLib
{
    public sealed class HarmonyMethod { public HarmonyMethod(MethodInfo method) { } }
    public sealed class Harmony
    {
        public Harmony(string id) { }
        public void Patch(MethodInfo method, HarmonyMethod postfix) { }
        public void UnpatchSelf() { }
    }
}
internal static class SaveRequestTests
{
    private static int checks;
    private const string Run = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    private static readonly string Token = Guid.NewGuid().ToString("N");
    private static void Check(bool value, string name)
    { if (!value) throw new Exception(name); checks++; }
    private static string Request()
    {
        return "LGSM_SAVE_1\nrun=" + Run + "\nnonce=" + Guid.NewGuid().ToString("N") +
            "\ntoken=" + Token + "\nop=save\nexpires=" + (SaveLoop.Now() + 9000) + "\n";
    }
    private static Func<SaveReceipt> Prepare()
    {
        try { return (Func<SaveReceipt>)typeof(SaveControl).GetMethod("Prepare", BindingFlags.NonPublic | BindingFlags.Static).Invoke(null, null); }
        catch (TargetInvocationException error) { throw error.InnerException; }
    }
    private static int Main()
    {
        try { RunTests(); return 0; }
        catch (Exception error) { Console.Error.WriteLine("FAIL: " + error.Message); return 1; }
    }
    private static void CheckOptionalTiming(object settings)
    {
        string[] names = { "LANGAME_NATIVE_EXACT_SAVE_READBACK", "LANGAME_NATIVE_MANUAL_SAVE", "LANGAME_NATIVE_MANUAL_SAVE_RESTART" };
        string[] previous = Array.ConvertAll(names, Environment.GetEnvironmentVariable);
        TextWriter output = Console.Out;
        var captured = new StringWriter();
        MethodInfo observe = typeof(SaveControl).GetMethod("ObserveNativeSaveTiming", BindingFlags.NonPublic | BindingFlags.Static);
        try
        {
            Console.SetOut(captured);
            foreach (string name in names) Environment.SetEnvironmentVariable(name, null);
            Check(!(bool)observe.Invoke(null, new[] { settings }) && captured.ToString() == "", "Ordinary saves must emit no acceptance diagnostics");
            foreach (string name in names) Environment.SetEnvironmentVariable(name, "true");
            Check((bool)observe.Invoke(null, new[] { settings }) && captured.ToString().Contains("interval_seconds=3600 variance_seconds=60"),
                "Acceptance must observe the actual native settings");
            captured.GetStringBuilder().Length = 0;
            Check(!(bool)observe.Invoke(null, new[] { new object() }) && captured.ToString() == "",
                "Missing diagnostic fields must not throw or fabricate evidence");
        }
        finally
        {
            Console.SetOut(output);
            for (int index = 0; index < names.Length; index++) Environment.SetEnvironmentVariable(names[index], previous[index]);
        }
    }
    private static void RunTests()
    {
        string root = Path.Combine(Path.GetTempPath(), "langame-vrising-request-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root);
        try
        {
            File.WriteAllText(Path.Combine(root, "session.txt"), "LGSM_SAVE_1\nmodule=vrising\nrun=" + Run + "\ntoken=" + Token + "\n");
            var world = new ProjectM.World { IsCreated = true, SequenceNumber = 1 };
            var server = new ProjectM.ServerBootstrapSystem { ServerIsInitialized = true, World = world };
            CheckOptionalTiming(server.Settings);
            typeof(SaveControl).GetField("bootstrap", BindingFlags.NonPublic | BindingFlags.Static).SetValue(null, server);
            using (SaveLoop loop = new SaveLoop(root, "vrising", new SaveAdapter(Prepare)))
            {
                string request = Request();
                string result = loop.Process(request);
                Check(result.Contains("status=dispatched\nissue=none\n") && world.Saves.Calls == 1,
                    "Native bool acceptance must dispatch once without a completion observer: " + result);
                Check(Object.ReferenceEquals(world.Saves.ReceivedSettings, server.Settings), "Native runtime settings must be passed unchanged");
                Check(loop.Process(request) == result && world.Saves.Calls == 1, "A repeated nonce must not save twice");
                world.Saves.Accepted = false;
                Check(loop.Process(Request()).Contains("status=rejected\nissue=native_rejected\n") && world.Saves.Calls == 2,
                    "Native false must remain a rejection");
                world.Saves.Ready = false;
                Check(loop.Process(Request()).Contains("issue=save_busy\n") && world.Saves.Calls == 2,
                    "Native readiness must reject before invocation");
                world.Saves.Ready = true; world.Serializer.IsSaving = true;
                Check(loop.Process(Request()).Contains("issue=save_busy\n") && world.Saves.Calls == 2,
                    "An active native serializer must not admit another save");
                world.Serializer.IsSaving = false; server.ServerIsInitialized = false;
                Check(loop.Process(Request()).Contains("issue=world_not_ready\n") && world.Saves.Calls == 2,
                    "An uninitialized server must not invoke saving");
                server.ServerIsInitialized = true; world.Saves.ThrowOnSave = true;
                request = Request(); result = loop.Process(request);
                Check(result.Contains("status=unknown\nissue=native_outcome_unknown\n") && world.Saves.Calls == 3,
                    "An exception after dispatch must remain unknown: calls=" + world.Saves.Calls + " " + result);
                Check(loop.Process(request) == result && world.Saves.Calls == 3, "An unknown outcome must not be resent");
                world.Saves.ThrowOnSave = false;
                Func<SaveReceipt> invoke = Prepare();
                world.SequenceNumber++;
                bool changed = false;
                try { invoke(); } catch (SaveFailure error) { changed = error.Code == "target_changed"; }
                Check(changed && world.Saves.Calls == 3, "A changed world generation must not be saved");
            }
            Console.WriteLine("PASS: " + checks + " V Rising request checks; synthetic native boundary, no game started");
        }
        finally
        {
            typeof(SaveControl).GetField("bootstrap", BindingFlags.NonPublic | BindingFlags.Static).SetValue(null, null);
            Directory.Delete(root, true);
        }
    }
}
