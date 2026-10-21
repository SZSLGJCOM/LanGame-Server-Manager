using System;
using System.Collections.Generic;
using System.Threading;

namespace Lgsm.Valheim.Control
{
    // Model the independently inspected native contract: the worker publishes
    // s_saveNumber only after EndSave(true). No game or world file is loaded.
    internal static class FakeSaveSystem
    {
        private static uint s_saveNumber;
        private static bool s_saving;
        internal static uint Number { get { return s_saveNumber; } }
        internal static bool Saving { get { return s_saving; } }
        internal static void Reset(uint number) { s_saveNumber = number; s_saving = false; }
        internal static void Start() { s_saving = true; }
        internal static void Finish(bool success) { if (success) s_saveNumber++; s_saving = false; }
    }
    internal sealed class FakeSaveWorld { }
    internal sealed class FakeSaveGame
    {
        public static FakeSaveGame instance { get; set; }
    }
    internal sealed class FakeSaveZone
    {
        public static FakeSaveZone instance { get; set; }
        internal bool Skip;
        public bool SkipSaving() { return Skip; }
    }
    internal sealed class FakeSaveDungeon
    {
        public static FakeSaveDungeon instance { get; set; }
        public bool SkipSaving() { return false; }
    }
    internal sealed class FakeSaveZNet : IDisposable
    {
        private Thread m_saveThread;
        public static bool m_loadError;
        public static Action WorldSaveStarted;
        internal static FakeSaveWorld World;
        internal bool Reject, Commit = true, DiskSpace = true;
        internal int Calls, DiskChecks, ClientRateChecks, ClientRequests;
        internal readonly ManualResetEvent Release = new ManualResetEvent(false);
        public bool enabled { get { return true; } }
        public bool IsDedicated() { return true; }
        public bool IsSaving() { return m_saveThread != null; }
        public bool HardSaveBlock()
        {
            ClientRateChecks++;
            throw new NullReferenceException("Dedicated server has no client distribution platform");
        }
        public void SaveWorldAndPlayerProfiles() { ClientRequests++; HardSaveBlock(); }
        public bool EnoughDiskSpaceAvailable(out bool exitGamePopupShown, bool exitGamePrompt, Action<bool> onDecisionMade)
        {
            DiskChecks++;
            ControlTests.Assert(!exitGamePrompt && onDecisionMade == null, "Save requested a client exit prompt or callback");
            exitGamePopupShown = false;
            return DiskSpace;
        }
        public static FakeSaveWorld GetWorldIfIsHost() { return World; }
        public void Save(bool sync, bool saveOtherPlayerProfiles, bool waitForNextFrame)
        {
            ControlTests.Assert(!sync && saveOtherPlayerProfiles && !waitForNextFrame,
                "Dedicated save changed async, client-profile notification or worker-start semantics");
            Calls++;
            if (Reject) return;
            if (WorldSaveStarted != null) WorldSaveStarted();
            FakeSaveSystem.Start();
            m_saveThread = new Thread(delegate() {
                Release.WaitOne();
                FakeSaveSystem.Finish(Commit);
            });
            m_saveThread.Start();
        }
        internal void Finish()
        {
            Release.Set();
            if (m_saveThread != null)
                ControlTests.Assert(m_saveThread.Join(3000), "Synthetic save worker did not terminate");
        }
        public void Dispose() { Finish(); Release.Dispose(); }
    }
    internal static class WorldSaveTests
    {
        private static ValheimWorldSave Adapter()
        {
            return new ValheimWorldSave(typeof(FakeSaveZNet), typeof(FakeSaveSystem),
                typeof(FakeSaveZone), typeof(FakeSaveDungeon), typeof(FakeSaveGame));
        }
        private static void Reset(uint number)
        {
            FakeSaveSystem.Reset(number);
            FakeSaveZNet.World = new FakeSaveWorld();
            FakeSaveZNet.WorldSaveStarted = null;
            FakeSaveZNet.m_loadError = false;
            FakeSaveZone.instance = new FakeSaveZone();
            FakeSaveDungeon.instance = new FakeSaveDungeon();
            FakeSaveGame.instance = new FakeSaveGame();
        }

        private sealed class SavingControl : IValheimControl, IValheimWorldSave
        {
            internal int Starts, Ends;
            internal WorldSaveReceipt Receipt;
            internal bool PollFails;
            internal Exception BeginError;
            public ControlSnapshot Query() { throw new Exception("Saving must not query access lists"); }
            public ControlSnapshot Reload() { throw new Exception("Unexpected reload"); }
            public void Kick(string session, string host) { throw new Exception("Unexpected kick"); }
            public void BeginSave() { Starts++; if (BeginError != null) throw BeginError; }
            public WorldSaveReceipt PollSave()
            {
                if (PollFails) throw new ControlFailure("server_not_ready");
                return Receipt;
            }
            public void EndSaveObservation() { Ends++; }
        }

        internal static void Run()
        {
            ControlTests.Test("native save failures retain bounded diagnostics without request or exception text", delegate {
                string sensitive = "private_native_exception_detail_" + Guid.NewGuid().ToString("N");
                SavingControl native = new SavingControl { BeginError = new InvalidOperationException(sensitive) };
                List<string> diagnostics = new List<string>();
                using (ControlLoop loop = new ControlLoop(native, ControlTests.Token, 123, diagnostics.Add))
                {
                    byte[] request = ControlTests.Request("save");
                    ControlResponse response = loop.Submit(request, 0);
                    ControlTests.Assert(response.Error.StartsWith("native_error type=System.InvalidOperationException") &&
                        response.Error.Contains("SavingControl.BeginSave") &&
                        ControlProtocol.SafeText(response.Error, 512, true) &&
                        !response.Error.Contains(sensitive) && !response.Error.Contains(ControlTests.Token) &&
                        response.WorldSave == null && diagnostics.Count == 1 &&
                        diagnostics[0].Contains("op=save phase=begin") &&
                        diagnostics[0].Contains("System.InvalidOperationException") &&
                        diagnostics[0].Contains("SavingControl.BeginSave") &&
                        !diagnostics[0].Contains(sensitive) && !diagnostics[0].Contains(ControlTests.Token),
                        "Native failure lost diagnostics or exposed private data");
                    loop.Submit(request, 1);
                    ControlTests.Assert(native.Starts == 1 && diagnostics.Count == 1,
                        "Cached native failure was dispatched or logged again");
                }
            });
            ControlTests.Test("diagnostic sink failure preserves and caches the original native failure", delegate {
                SavingControl native = new SavingControl { BeginError = new InvalidOperationException(ControlTests.Token) };
                using (ControlLoop loop = new ControlLoop(native, ControlTests.Token, 123,
                    delegate(string detail) { throw new System.IO.IOException("private log path " + ControlTests.Token); }))
                {
                    byte[] request = ControlTests.Request("save");
                    ControlResponse response = loop.Submit(request, 0);
                    ControlTests.Assert(response.Error.StartsWith("native_error type=System.InvalidOperationException") &&
                        response.Error.Contains("log_failure=System.IO.IOException") && response.WorldSave == null &&
                        !response.Error.Contains(ControlTests.Token) && ControlProtocol.SafeText(response.Error, 512, true) &&
                        Object.ReferenceEquals(response, loop.Submit(request, 1)) && native.Starts == 1,
                        "Diagnostic sink replaced the native failure, leaked content, or enabled a retry");
                }
            });
            ControlTests.Test("native reflection failure reports its cause within the existing error bound", delegate {
                string sensitive = "native_private_detail_" + Guid.NewGuid().ToString("N");
                Exception wrapped = new InvalidOperationException(sensitive);
                for (int count = 0; count < 3; count++) wrapped = new System.Reflection.TargetInvocationException(wrapped);
                SavingControl native = new SavingControl { BeginError = wrapped };
                using (ControlLoop loop = new ControlLoop(native, ControlTests.Token, 123))
                {
                    ControlResponse response = loop.Submit(ControlTests.Request("save"), 0);
                    ControlTests.Assert(response.Error.StartsWith("native_error type=System.InvalidOperationException") &&
                        response.Error.Contains("type=System.Reflection.TargetInvocationException") &&
                        ControlProtocol.SafeText(response.Error, 512, true) && !response.Error.Contains(sensitive) &&
                        response.WorldSave == null && native.Starts == 1,
                        "Nested reflection failure escaped the bounded private diagnostic contract");
                }
            });
            ControlTests.Test("save completion requires its worker and committed generation", delegate {
                Reset(7);
                using (FakeSaveZNet server = new FakeSaveZNet())
                using (ValheimWorldSave adapter = Adapter())
                {
                    adapter.Begin(server);
                    ControlTests.Assert(adapter.Poll(server) == null && FakeSaveSystem.Saving,
                        "A running save worker was reported complete");
                    ControlTests.Fails("save_busy", delegate { adapter.Begin(server); });
                    server.Finish();
                    WorldSaveReceipt result = adapter.Poll(server);
                    ControlTests.Assert(result.Previous == 7 && result.Committed == 8 &&
                        result.Evidence == "native_commit" && FakeSaveZNet.WorldSaveStarted == null &&
                        server.ClientRateChecks == 0 && server.ClientRequests == 0 && server.DiskChecks == 1,
                        "Native commit receipt or observation cleanup missing");
                }
            });
            ControlTests.Test("failed worker is not success even after thread completion", delegate {
                Reset(10);
                using (FakeSaveZNet server = new FakeSaveZNet { Commit = false })
                using (ValheimWorldSave adapter = Adapter())
                {
                    adapter.Begin(server);
                    server.Finish();
                    ControlTests.Fails("save_failed", delegate { adapter.Poll(server); });
                    ControlTests.Assert(FakeSaveSystem.Number == 10 && FakeSaveZNet.WorldSaveStarted == null,
                        "Failure changed native commit or retained observer");
                }
            });
            ControlTests.Test("native rejection and not-ready states cannot report a save", delegate {
                Reset(0);
                using (FakeSaveZNet server = new FakeSaveZNet())
                using (ValheimWorldSave adapter = Adapter())
                {
                    FakeSaveZone.instance.Skip = true;
                    ControlTests.Fails("save_not_ready", delegate { adapter.Begin(server); });
                    FakeSaveZone.instance.Skip = false;
                    FakeSaveGame.instance = null;
                    ControlTests.Fails("save_not_ready", delegate { adapter.Begin(server); });
                    FakeSaveGame.instance = new FakeSaveGame();
                    server.DiskSpace = false;
                    ControlTests.Fails("save_disk_space_rejected", delegate { adapter.Begin(server); });
                    ControlTests.Assert(server.Calls == 0 && server.DiskChecks == 1,
                        "Readiness or native disk-space refusal dispatched a save");
                    server.DiskSpace = true;
                    server.Reject = true;
                    ControlTests.Fails("save_not_started", delegate { adapter.Begin(server); });
                    ControlTests.Assert(server.Calls == 1 && FakeSaveZNet.WorldSaveStarted == null,
                        "Native rejection was retried or retained observation");
                }
            });
            ControlTests.Test("a later save cannot be mistaken for this failed save", delegate {
                Reset(4);
                using (FakeSaveZNet server = new FakeSaveZNet { Commit = false })
                using (ValheimWorldSave adapter = Adapter())
                {
                    adapter.Begin(server);
                    server.Finish();
                    FakeSaveZNet.WorldSaveStarted();
                    FakeSaveSystem.Finish(true);
                    ControlTests.Fails("save_outcome_unknown", delegate { adapter.Poll(server); });
                }
            });
            ControlTests.Test("world replacement or observer removal invalidates completion", delegate {
                foreach (bool remove in new[] { false, true })
                {
                    Reset(2);
                    using (FakeSaveZNet server = new FakeSaveZNet())
                    using (ValheimWorldSave adapter = Adapter())
                    {
                        adapter.Begin(server);
                        server.Finish();
                        if (remove) FakeSaveZNet.WorldSaveStarted = null;
                        else FakeSaveZNet.World = new FakeSaveWorld();
                        ControlTests.Fails("save_outcome_unknown", delegate { adapter.Poll(server); });
                    }
                }
            });
            ControlTests.Test("native commit counter wrap follows uint semantics", delegate {
                Reset(UInt32.MaxValue);
                using (FakeSaveZNet server = new FakeSaveZNet())
                using (ValheimWorldSave adapter = Adapter())
                {
                    adapter.Begin(server);
                    server.Finish();
                    ControlTests.Assert(adapter.Poll(server).Committed == 0, "Wrapped native commit rejected");
                }
            });
            ControlTests.Test("authenticated closed save is serialized and replay-safe", delegate {
                SavingControl native = new SavingControl();
                using (ControlLoop loop = new ControlLoop(native, ControlTests.Token, 123))
                {
                    byte[] request = ControlTests.Request("save");
                    ControlTests.Assert(loop.Submit(request, 0) == null && native.Starts == 1,
                        "Submission claimed immediate save success");
                    ControlTests.Assert(loop.Submit(request, 1) == null && native.Starts == 1,
                        "Pending save was dispatched twice");
                    ControlTests.Assert(loop.Submit(ControlTests.Request("query"), 1).Error == "busy",
                        "Concurrent operation bypassed pending save");
                    ControlTests.Assert(loop.Tick(2) == null, "Missing commit was reported complete");
                    native.Receipt = new WorldSaveReceipt { Previous = 10, Committed = 11 };
                    ControlResponse result = loop.Tick(3);
                    ControlTests.Assert(result.Status == "ok" && result.WorldSave.Committed == 11 &&
                        Object.ReferenceEquals(result, loop.Submit(request, 4)) && native.Starts == 1,
                        "Correlated save receipt or replay protection missing");
                }
            });
            ControlTests.Test("save authentication targets and timeout preserve mutation boundaries", delegate {
                SavingControl native = new SavingControl();
                using (ControlLoop loop = new ControlLoop(native, ControlTests.Token, 123))
                {
                    string nonce = Guid.NewGuid().ToString("N");
                    ControlTests.Assert(loop.Submit(ControlTests.Request("save", nonce, null, null,
                        new string('z', 32)), 0).Error == "unauthorized", "Unauthorized save accepted");
                    ControlTests.Assert(loop.Submit(ControlTests.Request("save", nonce, "target", null,
                        ControlTests.Token), 0).Error == "unexpected_target", "Save accepted target data");
                    ControlTests.Assert(native.Starts == 0, "Rejected save reached native code");
                    byte[] request = ControlTests.Request("save");
                    loop.Submit(request, 0);
                    ControlTests.Assert(loop.Tick(119.9) == null, "Save timed out early");
                    ControlResponse result = loop.Tick(120);
                    ControlTests.Assert(result.Error == "save_outcome_unknown" && result.WorldSave == null &&
                        native.Ends == 1 && Object.ReferenceEquals(result, loop.Submit(request, 121)) &&
                        native.Starts == 1, "Save timeout claimed success or redispatched");
                }
            });
            ControlTests.Test("loss of server observation after submission is an unknown outcome", delegate {
                SavingControl native = new SavingControl { PollFails = true };
                using (ControlLoop loop = new ControlLoop(native, ControlTests.Token, 123))
                {
                    loop.Submit(ControlTests.Request("save"), 0);
                    ControlResponse result = loop.Tick(1);
                    ControlTests.Assert(result.Error == "save_outcome_unknown" && result.WorldSave == null &&
                        native.Starts == 1 && native.Ends == 1, "Lost observation was reported as a preflight rejection");
                }
            });
        }
    }
}
