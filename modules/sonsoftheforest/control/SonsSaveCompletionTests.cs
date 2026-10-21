using System;
using System.IO;
using LanGame.SaveControl;
using LanGame.Sons;
using Sons.Save;

internal static class SonsSaveCompletionTests
{
    private sealed class RecursiveDiagnosticException : IOException
    {
        public override string Message { get { throw new RecursiveDiagnosticException(); } }
    }
    private static int passed;
    private static void Check(bool value, string name)
    { if (!value) throw new Exception(name); passed++; }
    private static void Unknown(NativeSaveCompletion.Capture capture, string name)
    {
        try { capture.Receipt(); throw new Exception("False completion: " + name); }
        catch (SaveFailure error) { Check(error.Code == "native_outcome_unknown", name); }
    }
    private static void Main(string[] args)
    {
        string root = Path.Combine(args[0], "completion-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root);
        SaveGameManager.Temporary = Path.Combine(root, "native-temp.zip");
        File.WriteAllBytes(SaveGameManager.Temporary, new byte[] {1, 2, 3});
        if (args.Length == 2 && args[1] == "successful-native-cleanup-failure")
        {
            using (NativeSaveCompletion observer = NativeSaveCompletion.Install())
            {
                SaveGameManager.FailUnregisterAfter = true;
                bool returned = false;
                try
                {
                    NativeSaveCompletion.Invoke(observer.Begin(), delegate {
                        SaveGameManager.SaveToSlot(root); return SaveReceipt.Completed;
                    });
                    returned = true;
                }
                catch (AggregateException failure)
                {
                    Check(failure.InnerExceptions.Count == 1 && File.Exists(Path.Combine(root, "SaveData.zip")),
                        "native save completed but observer cleanup failed");
                }
                Check(!returned, "cleanup failure prevents a completed receipt after successful native save");
            }
            Console.WriteLine("Sons successful-native cleanup tests: " + passed + " passed; native game not started");
            return;
        }
        using (NativeSaveCompletion observer = NativeSaveCompletion.Install())
        {
            using (NativeSaveCompletion.Capture capture = observer.Begin())
                Unknown(capture, "void no-op cannot complete this request");
            using (NativeSaveCompletion.Capture capture = observer.Begin())
            {
                SaveGameManager.SaveToSlot(root);
                Check(capture.Receipt() == SaveReceipt.Completed, "owned before and full-world after callbacks confirm this invocation");
            }
            Check(SaveGameManager.BeforeCallbacks.Count == 0 && SaveGameManager.AfterCallbacks.Count == 0,
                "successful request unregisters both callbacks");
            Type injector = NativeBinding.Type("Il2CppInterop.Runtime", "Il2CppInterop.Runtime.Injection.ClassInjector");
            Check((int)injector.GetField("Registrations").GetValue(null) == 1,
                "ordinary emitted receiver is injected once across all requests");
            SaveGameManager.SaveMode = 2;
            using (NativeSaveCompletion.Capture capture = observer.Begin())
            {
                try { SaveGameManager.SaveToSlot(root); throw new Exception("native copy failure was swallowed"); }
                catch (System.IO.IOException) { }
                Unknown(capture, "native copy failure propagates and never reaches after callback");
            }
            SaveGameManager.SaveMode = 0;
            foreach (int mode in new[] {1, 2, 3, 4})
            {
                using (NativeSaveCompletion.Capture capture = observer.Begin())
                {
                    if (mode == 1) SaveGameManager.AfterSave(false);
                    if (mode == 2) { SaveGameManager.BeforeSave(); SaveGameManager.AfterSave(true); }
                    if (mode == 3) { SaveGameManager.BeforeSave(); SaveGameManager.BeforeSave(); SaveGameManager.AfterSave(false); }
                    if (mode == 4) { SaveGameManager.BeforeSave(); SaveGameManager.AfterSave(false); SaveGameManager.AfterSave(false); }
                    Unknown(capture, "out-of-order, player-only or nested callbacks mode " + mode);
                }
            }
            using (NativeSaveCompletion.Capture capture = observer.Begin())
            {
                var worker = new System.Threading.Thread(delegate() { SaveGameManager.BeforeSave(); SaveGameManager.AfterSave(false); });
                worker.Start(); worker.Join();
                Unknown(capture, "another thread cannot complete this invocation");
            }
            SaveGameManager.FailRegisterAfter = true;
            try { observer.Begin(); throw new Exception("registration failure admitted"); }
            catch (System.Reflection.TargetInvocationException) { passed++; }
            finally { SaveGameManager.FailRegisterAfter = false; }
            Check(SaveGameManager.BeforeCallbacks.Count == 0 && SaveGameManager.AfterCallbacks.Count == 0,
                "partial registration failure cleans earlier registration");
            using (NativeSaveCompletion.Capture capture = observer.Begin())
            {
                SaveGameManager.SaveToSlot(root);
                Check(capture.Receipt() == SaveReceipt.Completed, "clean registry remains reusable");
            }
        }
        Check(SaveGameManager.BeforeCallbacks.Count == 0 && SaveGameManager.AfterCallbacks.Count == 0,
            "native teardown leaves no test callbacks");
        string ipc = Path.Combine(root, "runtime", ".langame-world-save");
        Directory.CreateDirectory(ipc);
        string run = new string('a', 32), token = new string('b', 32), nonce = new string('c', 32);
        File.WriteAllText(Path.Combine(ipc, "session.txt"), "LGSM_SAVE_1\nmodule=sonsoftheforest\nrun=" + run + "\ntoken=" + token + "\n");
        var diagnostics = new System.Collections.Generic.List<string>();
        string previous = Environment.GetEnvironmentVariable("LANGAME_NATIVE_SAVE_FAILURE_ACCEPTANCE");
        try
        {
            Environment.SetEnvironmentVariable("LANGAME_NATIVE_SAVE_FAILURE_ACCEPTANCE", "true");
            using (FileStream locked = File.Open(Path.Combine(root, "SaveData.zip"), FileMode.Open, FileAccess.Read, FileShare.Read))
            using (NativeSaveCompletion observer = NativeSaveCompletion.Install())
            using (SaveLoop loop = new SaveLoop(ipc, "sonsoftheforest", new SaveAdapter(delegate {
                return delegate {
                    return NativeSaveCompletion.Invoke(observer.Begin(), delegate {
                        try { SaveGameManager.SaveToSlot(root); return SaveReceipt.Completed; }
                        catch (IOException failure) { throw new AggregateException(new System.Reflection.TargetInvocationException(new AggregateException(failure))); }
                    });
                };
            }), diagnostics.Add))
            {
                string response = loop.Process("LGSM_SAVE_1\nrun=" + run + "\nnonce=" + nonce + "\ntoken=" + token + "\nop=save\nexpires=" + (SaveLoop.Now() + 5000) + "\n");
                Check(response.Contains("status=unknown\nissue=native_outcome_unknown"), "actual locked-copy exception stays unknown");
                Check(diagnostics.Exists(delegate(string line) { return line.Contains("nonce=" + nonce) && line.Contains("IOException") && line.Contains("<fixture>/SaveData.zip"); }), "native failure diagnostic is nonce correlated and fixture relative");
                Check(diagnostics.Exists(delegate(string line) { return line.Contains("detail=at SaveGameManager.WriteJsonZip"); }), "diagnostic includes only admitted native write frame");
                Check(diagnostics.Exists(delegate(string line) { return line.Contains("exception_type=System.IO.IOException") && line.Contains("hresult=80070020"); }), "bounded traversal reaches wrapped native IO HRESULT");
                Check(diagnostics.Exists(delegate(string line) { return line.Contains("nodes=4"); }), "aggregate and reflection chain is traversed without duplication");
                Check(diagnostics.TrueForAll(delegate(string line) { return !line.Contains(token) && !line.Contains(root); }), "diagnostics disclose no IPC token or fixture absolute path");
            }
            using (FileStream locked = File.Open(Path.Combine(root, "SaveData.zip"), FileMode.Open, FileAccess.Read, FileShare.Read))
            using (NativeSaveCompletion observer = NativeSaveCompletion.Install())
            using (SaveLoop loop = new SaveLoop(ipc, "sonsoftheforest", new SaveAdapter(delegate {
                return delegate { return NativeSaveCompletion.Invoke(observer.Begin(), delegate {
                    SaveGameManager.SaveToSlot(root); return SaveReceipt.Completed;
                }); };
            }), diagnostics.Add))
            {
                diagnostics.Clear(); SaveGameManager.SaveMode = 5;
                string nextNonce = new string('e', 32);
                string response = loop.Process("LGSM_SAVE_1\nrun=" + run + "\nnonce=" + nextNonce + "\ntoken=" + token + "\nop=save\nexpires=" + (SaveLoop.Now() + 5000) + "\n");
                Check(response.Contains("status=unknown"), "swallowed native write failure cannot complete");
                Check(diagnostics.Exists(delegate(string line) { return line.Contains("nonce=" + nextNonce) && line.Contains("exception_type=Il2CppInterop.Runtime.Il2CppException"); }),
                    "same invocation first-chance exception survives loader swallowing");
                Check(diagnostics.Exists(delegate(string line) { return line.Contains("callback_before=1 callback_after=0") && line.Contains("callback_owner_thread=true"); }),
                    "failed receipt reports exact owner callback counts");
                Check(diagnostics.Exists(delegate(string line) { return line.Contains("IOException") && line.Contains("<fixture>/SaveData.zip"); }),
                    "captured swallowed write failure retains exact fixture path");
                SaveGameManager.SaveMode = 0;
            }
            foreach (bool workerOnly in new[] {true, false})
            using (NativeSaveCompletion observer = NativeSaveCompletion.Install())
            using (SaveLoop loop = new SaveLoop(ipc, "sonsoftheforest", new SaveAdapter(delegate {
                return delegate { return NativeSaveCompletion.Invoke(observer.Begin(), delegate {
                    SaveGameManager.BeforeSave();
                    if (workerOnly)
                    {
                        var worker = new System.Threading.Thread(delegate() {
                            try { throw new Il2CppInterop.Runtime.Il2CppException("IOException " + Path.Combine(root, "SaveData.zip") + "\n at Sons.Save.SaveGameManager.WriteJsonZip"); }
                            catch (Il2CppInterop.Runtime.Il2CppException) { }
                        });
                        worker.Start(); worker.Join();
                    }
                    else { try { throw new RecursiveDiagnosticException(); } catch (RecursiveDiagnosticException) { } }
                    return SaveReceipt.Completed;
                }); };
            }), diagnostics.Add))
            {
                diagnostics.Clear();
                string nextNonce = new string(workerOnly ? 'f' : '1', 32);
                string response = loop.Process("LGSM_SAVE_1\nrun=" + run + "\nnonce=" + nextNonce + "\ntoken=" + token + "\nop=save\nexpires=" + (SaveLoop.Now() + 5000) + "\n");
                Check(response.Contains("status=unknown"), "worker or recursive diagnostic cannot manufacture completion");
                if (workerOnly)
                    Check(!diagnostics.Exists(delegate(string line) { return line.Contains("exception_type=Il2CppInterop.Runtime.Il2CppException"); }),
                        "another thread's native exception cannot provide this invocation's fault evidence");
                else
                    Check(diagnostics.Exists(delegate(string line) { return line.Contains("diagnostic_unavailable=true"); }),
                        "recursive exception inspection is bounded, contained and observable");
            }
            using (FileStream locked = File.Open(Path.Combine(root, "SaveData.zip"), FileMode.Open, FileAccess.Read, FileShare.Read))
            using (SaveLoop loop = new SaveLoop(ipc, "sonsoftheforest", new SaveAdapter(delegate {
                return delegate { SaveGameManager.SaveToSlot(root); return SaveReceipt.Completed; };
            }), delegate(string line) { throw new IOException("test diagnostic sink failure"); }))
            {
                string response = loop.Process("LGSM_SAVE_1\nrun=" + run + "\nnonce=" + new string('d',32) + "\ntoken=" + token + "\nop=save\nexpires=" + (SaveLoop.Now() + 5000) + "\n");
                Check(response.Contains("status=unknown") && loop.FailureDiagnosticUnavailable, "diagnostic sink failure preserves unknown and is observable");
            }
        }
        finally { Environment.SetEnvironmentVariable("LANGAME_NATIVE_SAVE_FAILURE_ACCEPTANCE", previous); }
        using (NativeSaveCompletion observer = NativeSaveCompletion.Install())
        {
            NativeSaveCompletion.Capture capture = observer.Begin();
            SaveGameManager.FailUnregisterAfter = true;
            IOException primary = new IOException("original native write failure");
            try
            {
                NativeSaveCompletion.Invoke(capture, delegate { throw primary; });
                throw new Exception("cleanup failure hidden");
            }
            catch (AggregateException failure)
            {
                Check(Object.ReferenceEquals(failure.InnerExceptions[0], primary) && failure.InnerExceptions[1] is AggregateException,
                    "original native write failure and cleanup failure are both preserved");
            }
            SaveGameManager.BeforeSave(); SaveGameManager.AfterSave(false);
            try { observer.Begin(); throw new Exception("failed registry accepted another subscription"); }
            catch (SaveFailure error) { Check(error.Code == "api_unavailable", "failed unregister is inert and prevents subscription leaks"); }
            using (NativeSaveCompletion replacement = NativeSaveCompletion.Install())
            {
                try { replacement.Begin(); throw new Exception("replacement observer accepted failed native registry"); }
                catch (SaveFailure error) { Check(error.Code == "api_unavailable", "failed native registry remains poisoned across observer replacement"); }
            }
            SaveGameManager.FailUnregisterAfter = false;
            SaveGameManager.BeforeCallbacks.Clear(); SaveGameManager.AfterCallbacks.Clear();
        }
        Console.WriteLine("Sons completion callback tests: " + passed + " passed; native game not started");
    }
}
