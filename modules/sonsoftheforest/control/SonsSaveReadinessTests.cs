using System;
using System.Collections.Generic;
using LanGame.SaveControl;

namespace LanGame.Sons
{
    // Native boundary doubles only; these tests never load Unity or save a game.
    internal static class SonsSaveReadinessTests
    {
        private static void Assert(bool value, string message)
        {
            if (!value) throw new Exception(message);
        }

        private static void Main()
        {
            int checks = 0, saves = 0;
            bool ready = false;
            List<string> logs = new List<string>();
            StartupSaveReadiness observer = new StartupSaveReadiness(delegate {
                checks++;
                if (!ready) throw new SaveFailure("world_not_ready");
                return delegate { saves++; throw new Exception("Observer must never invoke saving"); };
            }, logs.Add);
            observer.Tick(0);
            observer.Tick(999);
            observer.Tick(1000);
            Assert(checks == 2 && logs.Count == 0 && saves == 0, "Readiness polling is not limited to one second");
            ready = true;
            observer.Tick(1999);
            observer.Tick(2000);
            observer.Tick(3000);
            observer.Tick(StartupSaveReadiness.BudgetMilliseconds + 1);
            Assert(checks == 3 && saves == 0 && logs.Count == 1 && logs[0] == StartupSaveReadiness.ReadyMarker,
                "Ready observation saved, repeated its marker, or kept scanning");

            foreach (string issue in new[] { "api_unavailable", "world_ambiguous", "unknown_exception" })
            {
                checks = 0;
                logs.Clear();
                observer = new StartupSaveReadiness(delegate {
                    checks++;
                    if (issue == "unknown_exception") throw new InvalidOperationException("private native detail");
                    throw new SaveFailure(issue);
                }, logs.Add);
                observer.Tick(0);
                observer.Tick(1000);
                string expected = issue == "unknown_exception" ? "api_unavailable" : issue;
                Assert(checks == 1 && logs.Count == 1 &&
                    logs[0] == "LGSM_SONS_SAVE_UNAVAILABLE issue=" + expected,
                    "A terminal native error was ignored, exposed private text, or kept scanning");
            }

            checks = 0;
            logs.Clear();
            observer = new StartupSaveReadiness(delegate { checks++; throw new SaveFailure("world_not_ready"); }, logs.Add);
            observer.Tick(0);
            observer.Tick(StartupSaveReadiness.BudgetMilliseconds);
            observer.Tick(StartupSaveReadiness.BudgetMilliseconds + 1000);
            Assert(checks == 1 && logs.Count == 1 &&
                logs[0] == "LGSM_SONS_SAVE_UNAVAILABLE issue=startup_readiness_timeout",
                "Startup observation exceeded its budget or failed to report expiry");
            Console.WriteLine("Sons startup readiness boundary tests: 5 passed; native game not started");
        }
    }
}
