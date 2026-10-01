using System;
using System.Collections;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Threading;

// Loaded by the server's native DllModLoader; enabled only for LanGame-owned stdin.
// Its only assembly reference is the platform BCL; game APIs are resolved at entry.
public static class LanGameTheForestControlEntry
{
    private static bool loaded;
    public static void __LanGameControl()
    {
        if (loaded || Environment.GetEnvironmentVariable("LANGAME_THEFOREST_STDIN_BRIDGE") != "1") return;
        loaded = true;
        new ForestNativeControl().Start();
    }
}

internal sealed class ForestNativeControl : IEnumerator
{
    private readonly object gate = new object();
    private readonly Queue<string> commands = new Queue<string>();
    private volatile bool stopping;
    private string inputError;
    private bool reportedInputError;
    private bool publishedWorldReady, reportedWorldError;
    private readonly ManualResetEvent inputCancellation = new ManualResetEvent(false);
    private Thread inputReader;
    private string commandStage;
    private object host;
    private MethodInfo shutdown, startCoroutine, log, logError, save, savePath;
    private Type level, peer, scene;
    [DllImport("kernel32.dll", SetLastError=true)] private static extern IntPtr GetStdHandle(int n);
    [DllImport("kernel32.dll", SetLastError=true)] private static extern uint GetFileType(IntPtr h);
    [DllImport("kernel32.dll")] private static extern IntPtr GetCurrentProcess();
    [DllImport("kernel32.dll", SetLastError=true)] private static extern bool DuplicateHandle(IntPtr p, IntPtr h, IntPtr target, out IntPtr copy, uint access, bool inherit, uint options);
    [DllImport("kernel32.dll", SetLastError=true)] private static extern bool CloseHandle(IntPtr handle);
    private const BindingFlags Static = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static;

    internal void Start()
    {
        Type debug = Type.GetType("UnityEngine.Debug, UnityEngine", true);
        log = debug.GetMethod("Log", new Type[] { typeof(object) });
        logError = debug.GetMethod("LogError", new Type[] { typeof(object) });
        try
        {
            Type unityObject = Type.GetType("UnityEngine.Object, UnityEngine", true);
            Type mono = Type.GetType("UnityEngine.MonoBehaviour, UnityEngine", true);
            Type component = Type.GetType("UnityEngine.Component, UnityEngine", true);
            Type loader = Type.GetType("TheForest.Tools.DllModLoader, Assembly-CSharp", true);
            Type admin = Type.GetType("CoopAdminCommand, Assembly-CSharp", true);
            level = Type.GetType("LevelSerializer, Assembly-CSharp", true);
            peer = Type.GetType("CoopPeerStarter, Assembly-CSharp", true);
            scene = Type.GetType("TheForest.Utils.Scene, Assembly-CSharp", true);
            save = Type.GetType("SteamDSConfig, Assembly-CSharp", true).GetMethod("SaveGame", Static, null, Type.EmptyTypes, null);
            savePath = Type.GetType("TheForest.Utils.SaveSlotUtils, Assembly-CSharp", true).GetMethod("GetLocalSlotPath", Static, null, Type.EmptyTypes, null);
            shutdown = admin.GetMethod("ShutDownRoutine", Static, null, new Type[] { typeof(bool) }, null);
            // Unity's bundled Mono predates the reflection equality operators in
            // the workstation BCL. ReferenceEquals keeps this entry loadable there.
            if (Object.ReferenceEquals(shutdown, null) || !Object.ReferenceEquals(shutdown.ReturnType, typeof(IEnumerator)) || Object.ReferenceEquals(save, null) || Object.ReferenceEquals(savePath, null)) throw new MissingMethodException();
            host = unityObject.GetMethod("FindObjectOfType", new Type[] { typeof(Type) }).Invoke(null, new object[] { loader });
            if (host == null) throw new InvalidOperationException();
            object owner = component.GetProperty("gameObject").GetValue(host, null);
            unityObject.GetMethod("DontDestroyOnLoad", new Type[] { unityObject }).Invoke(null, new object[] { owner });
            startCoroutine = mono.GetMethod("StartCoroutine", new Type[] { typeof(IEnumerator) });
            IntPtr stdin = GetStdHandle(-10);
            if (GetFileType(stdin) != 3) { Error("unavailable: stdin is not the owned pipe"); return; }
            IntPtr copied;
            if (!DuplicateHandle(GetCurrentProcess(), stdin, GetCurrentProcess(), out copied, 0, false, 2)) { Error("unavailable: input handle duplication failed"); return; }
            ForestOwnedInputStream stream = null;
            try
            {
                stream = new ForestOwnedInputStream(copied, inputCancellation);
                // Unity logs coroutine JIT failures without propagating them.
                // Validate the idle first step before claiming this bridge ready.
                MoveNext();
                startCoroutine.Invoke(host, new object[] { this });
                inputReader = new Thread(delegate() { ReadInput(stream); });
                inputReader.IsBackground = true;
                inputReader.Name = "LanGame TheForest input";
                inputReader.Start();
            }
            catch
            {
                stopping = true;
                inputCancellation.Set();
                if (stream != null) stream.Dispose(); else CloseHandle(copied);
                throw;
            }
            Info("ready; commands: help, status, save, shutdown");
        }
        catch (Exception error) { Error("unavailable: " + Cause(error)); }
    }

    private void ReadInput(Stream stream)
    {
        try
        {
            using (stream)
            {
                while (!stopping)
                {
                    string command = ForestControlProtocol.Read(stream);
                    if (command == null) { if (!stopping) SetInputError("input pipe closed"); return; }
                    if (command.Length == 0) continue;
                    // Mono in this server has Enter(object), but not the newer
                    // Enter(object, ref bool) generated by workstation `lock`.
                    Monitor.Enter(gate);
                    try
                    {
                        while (commands.Count != 0 && !stopping) Monitor.Wait(gate);
                        if (stopping) return;
                        commands.Enqueue(command);
                    }
                    finally { Monitor.Exit(gate); }
                }
            }
        }
        catch (Exception error) { SetInputError(error.GetType().Name); }
    }

    private void SetInputError(string error)
    {
        Monitor.Enter(gate);
        try { inputError = error; }
        finally { Monitor.Exit(gate); }
    }

    public object Current { get { return null; } }
    public void Reset() { throw new NotSupportedException(); }
    public bool MoveNext()
    {
        string command = null, pendingError = null;
        Monitor.Enter(gate);
        try
        {
            if (commands.Count != 0) { command = commands.Dequeue(); Monitor.PulseAll(gate); }
            if (!reportedInputError && inputError != null) { reportedInputError = true; pendingError = inputError; }
        }
        finally { Monitor.Exit(gate); }
        if (pendingError != null) Error(pendingError);
        if (stopping) return false;
        try
        {
            // The native Running banner precedes FinishGameLoad. Observe the
            // actual save gates even on idle frames, never by elapsed time.
            commandStage = "world_readiness";
            bool ready = (bool)Member(peer, "DedicatedHost") && (bool)Member(scene, "FinishGameLoad");
            bool suspended = (bool)Member(level, "IsSuspended");
            PublishWorldReadiness(ready && !suspended && inputReader != null && inputReader.IsAlive && !reportedInputError);
            reportedWorldError = false;
            if (command == null) return true;
            commandStage = "command";
            if (command == "help") { Info("help status save shutdown"); return true; }
            if (command == "status") { Info("world_ready=" + ready + " serialization_suspended=" + suspended); return true; }
            if (command != "save" && command != "shutdown") { Error("unsupported command"); return true; }
            if (!ready || suspended) { Error("command refused: world is not ready for a synchronous save"); return true; }
            long savedBytes = SaveNativeGame();
            Info("native_save_completed bytes=" + savedBytes);
            if (command == "shutdown")
            {
                // Unity's Mono shutdown must not inherit a managed thread
                // blocked in an uninterruptible synchronous stdin read.
                commandStage = "stop_input";
                PublishWorldReadiness(false);
                stopping = true;
                inputCancellation.Set();
                Monitor.Enter(gate);
                try { Monitor.PulseAll(gate); }
                finally { Monitor.Exit(gate); }
                if (inputReader == null || !inputReader.Join(1000)) throw new IOException();
                inputCancellation.Close();
                Info("input_reader_stopped");
                commandStage = "shutdown";
                IEnumerator sequence = (IEnumerator)shutdown.Invoke(null, new object[] { false });
                Info("native_shutdown_requested");
                startCoroutine.Invoke(host, new object[] { sequence });
            }
        }
        catch (Exception error)
        {
            if (commandStage == "world_readiness")
            {
                PublishWorldReadiness(false);
                if (!reportedWorldError) Error("world readiness unavailable: " + Cause(error));
                reportedWorldError = true;
            }
            else Error("command failed: " + commandStage + " " + Cause(error));
        }
        return !stopping;
    }

    private void PublishWorldReadiness(bool ready)
    {
        if (publishedWorldReady == ready) return;
        publishedWorldReady = ready;
        Info("world_ready=" + ready);
    }

    private string CurrentSavePath()
    {
        return Path.GetFullPath(Path.Combine((string)savePath.Invoke(null, null), (string)Member(level, "PlayerName") + "__RESUME__"));
    }

    private long SaveNativeGame()
    {
        commandStage = "resolve_before";
        string path = CurrentSavePath();
        commandStage = "pin_before";
        return ForestNativeCheckpoint.Save(path, delegate()
        {
            commandStage = "invoke";
            save.Invoke(null, null);
            commandStage = "resolve_after";
            if (!String.Equals(path, CurrentSavePath(), StringComparison.OrdinalIgnoreCase)) throw new IOException();
            commandStage = "identity";
        });
    }

    private static object Member(Type type, string name)
    {
        PropertyInfo property = type.GetProperty(name, Static);
        return !Object.ReferenceEquals(property, null) ? property.GetValue(null, null) : type.GetField(name, Static).GetValue(null);
    }
    private static string Cause(Exception error)
    {
        Exception cause = error.InnerException ?? error;
        ForestCheckpointFailure checkpoint = cause as ForestCheckpointFailure;
        return checkpoint != null ? checkpoint.Code : cause.GetType().Name;
    }
    private void Info(string text) { log.Invoke(null, new object[] { "[LanGame native control] " + text }); }
    private void Error(string text) { logError.Invoke(null, new object[] { "[LanGame native control] " + text }); }
}
