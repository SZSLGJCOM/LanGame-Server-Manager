using System;
using System.IO;
using LanGame.SaveControl;

internal static class ProtocolTests
{
    private static int passed;
    private static string root;
    private const string Run = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    private static readonly string Token = Guid.NewGuid().ToString("N");
    private static void Check(bool value, string name)
    { if (!value) throw new Exception("FAIL: " + name); passed++; }
    private static string Request(string nonce, string token, string op, long expires)
    { return "LGSM_SAVE_1\nrun=" + Run + "\nnonce=" + nonce + "\ntoken=" + token + "\nop=" + op + "\nexpires=" + expires + "\n"; }
    private static string Request(string nonce)
    { return Request(nonce, Token, "save", SaveLoop.Now() + 5000); }
    private static string Nonce() { return Guid.NewGuid().ToString("N"); }
    private static void Invalid(Action action, string name)
    { bool rejected = false; try { action(); } catch (SaveFailure) { rejected = true; } Check(rejected, name); }
    private static int Main()
    {
        try { RunChecks(); return 0; }
        catch(Exception error)
        {
            Console.Error.WriteLine("Protocol failure after " + passed + " checks: " + error.GetType().FullName);
            Console.Error.WriteLine(error.Message);
            return 1;
        }
    }
    private static void RunChecks()
    {
        root = Path.Combine(Path.GetTempPath(), "langame-save-protocol-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root);
        File.WriteAllText(Path.Combine(root,"session.txt"),"LGSM_SAVE_1\nmodule=corekeeper\nrun="+Run+"\ntoken="+Token+"\n",new System.Text.UTF8Encoding(false));
        try
        {
            int invokes = 0;
            using (SaveLoop loop = new SaveLoop(root,"corekeeper",delegate { return delegate {
                Check(File.ReadAllText(Path.Combine(root,"response.txt")).Contains("status=dispatching\n"), "dispatch marker precedes native call");
                invokes++; return true;
            }; }))
            {
                string request = Request(Nonce()); string response = loop.Process(request);
                Check(response.Contains("status=dispatched\nissue=none\n"),"native acceptance is dispatched, never completed");
                Check(loop.Process(request)==response && invokes==1,"same nonce replays without native resubmission");
                Check(loop.Process(request.Replace("op=save", "op=other")).Contains("unsupported_operation"),"operation is closed");
                Check(loop.Process(request.Replace("expires=", "expires=1")).Contains("nonce_conflict"),"same nonce changed payload is rejected");
                Check(loop.Process(Request(Nonce(),new string('c',32),"save",SaveLoop.Now()+5000)).Contains("unauthorized"),"bad credential cannot invoke");
                Check(loop.Process(Request(Nonce(),Token,"save",SaveLoop.Now()-1)).Contains("request_expired"),"expired request cannot invoke");
                Check(loop.Process(Request(Nonce(),Token,"save",SaveLoop.Now()+20000)).Contains("request_expired"),"unbounded deadline cannot invoke");
                Check(invokes==1,"all rejected requests remain nonmutating");
                Invalid(delegate {loop.Process(request+"op=save\n");},"duplicate field rejected");
                Invalid(delegate {loop.Process(request.Replace("op=save","op=save\r"));},"noncanonical CR rejected");
                Invalid(delegate {loop.Process(request.TrimEnd('\n'));},"incomplete frame rejected");
            }
            using (SaveLoop busy = new SaveLoop(root,"corekeeper",delegate {throw new SaveFailure("save_busy");}))
                Check(busy.Process(Request(Nonce())).Contains("status=rejected\nissue=save_busy\n"),"native preflight rejects busy save");
            using (SaveLoop rejected = new SaveLoop(root,"corekeeper",delegate {return delegate {return false;};}))
                Check(rejected.Process(Request(Nonce())).Contains("status=rejected\nissue=native_rejected\n"),"native false is rejected");
            using (SaveLoop unknown = new SaveLoop(root,"corekeeper",delegate {return delegate {throw new IOException("synthetic disk failure");};}))
            {
                string request=Request(Nonce()), first=unknown.Process(request);
                Check(first.Contains("status=unknown\nissue=native_outcome_unknown\n"),"native exception is unknown, never swallowed");
                Check(unknown.Process(request)==first,"unknown outcome is cached without retry");
            }
            using (SaveLoop late = new SaveLoop(root,"corekeeper",delegate {return delegate {throw new SaveFailure("world_not_ready");};}))
            {
                string lateReply = late.Process(Request(Nonce()));
                Check(lateReply.Contains("status=unknown"),"post-admission exception stays unknown: " + lateReply);
            }
            string completedRequest = Request(Nonce()), completedReply;
            using (SaveLoop completed = new SaveLoop(root,"corekeeper",new SaveAdapter(delegate {return delegate {
                invokes++; return SaveReceipt.Completed;
            };})))
            {
                int before = invokes;
                completedReply = completed.Process(completedRequest);
                Check(completedReply.Contains("status=completed\nissue=none\n"),"explicit native completion has a distinct receipt");
                Check(completed.Process(completedRequest) == completedReply && invokes == before + 1,
                    "completed request replay cannot save twice");
            }
            File.WriteAllText(Path.Combine(root,"response.txt"),completedReply,new System.Text.UTF8Encoding(false));
            using (SaveLoop reloaded = new SaveLoop(root,"corekeeper",delegate {throw new Exception("cannot repeat a completed save");}))
                Check(reloaded.Process(completedRequest) == completedReply,"completed receipt survives control reload");
            using (SaveLoop invalid = new SaveLoop(root,"corekeeper",new SaveAdapter(delegate {return delegate {return (SaveReceipt)99;};})))
                Check(invalid.Process(Request(Nonce())).Contains("status=unknown"),"unknown adapter result cannot become completion");
            int started = 0, polls = 0;
            bool tracked = false, observeFailure = false;
            SaveReceipt observation = SaveReceipt.Pending;
            SaveAdapter asynchronous = new SaveAdapter(delegate { return delegate {
                started++; tracked = true; return SaveReceipt.Pending;
            }; }, delegate { polls++; if(observeFailure) throw new IOException("native observer failed"); return observation; },
                delegate { tracked = false; });
            using (SaveLoop pending = new SaveLoop(root,"corekeeper",asynchronous))
            {
                string request = Request(Nonce());
                Check(pending.Process(request).Contains("status=dispatching"),"asynchronous native job remains executing");
                Check(pending.Process(request).Contains("status=dispatching") && started == 1,"pending nonce never resubmits");
                Check(pending.Process(Request(Nonce())).Contains("issue=save_busy") && started == 1,"one native operation owns the channel");
                pending.ObservePending(0);
                Check(polls == 1 && tracked,"pending observer keeps ownership");
                observation = SaveReceipt.Completed; pending.ObservePending(Int64.MaxValue);
                Check(File.ReadAllText(Path.Combine(root,"response.txt")).Contains("status=completed") && !tracked,
                    "already observed native completion takes priority over expiry and releases observation");
                Check(pending.Process(request).Contains("status=completed") && started == 1,"async completion updates replay cache");
                observation = SaveReceipt.Pending; request = Request(Nonce()); pending.Process(request);
                pending.ObservePending(Int64.MaxValue);
                Check(pending.Process(request).Contains("status=unknown") && !tracked && started == 2,"completion timeout is bounded unknown without native retry");
                observation = SaveReceipt.Completed; pending.ObservePending(0);
                Check(pending.Process(request).Contains("status=unknown"),"late completion cannot rewrite a timed-out result");
                observation = SaveReceipt.Pending; request = Request(Nonce()); pending.Process(request);
                observeFailure = true; pending.ObservePending(0);
                Check(pending.Process(request).Contains("status=unknown") && !tracked,"observer failure releases ownership and remains unknown");
                observeFailure = false;
                pending.Process(Request(Nonce()));
            }
            Check(!tracked,"disposing the channel abandons observation without replaying the native operation");
            observation = SaveReceipt.Dispatched;
            using (SaveLoop queued = new SaveLoop(root,"corekeeper",asynchronous))
            {
                string request = Request(Nonce());
                queued.Process(request); int submissions = started;
                queued.ObservePending(0);
                Check(queued.Process(request).Contains("status=dispatched\nissue=none\n") &&
                    !tracked && started == submissions,
                    "asynchronous native submission releases ownership without claiming disk completion or replaying");
            }
            string reloadRequest=Request(Nonce());
            using (SaveLoop before = new SaveLoop(root,"corekeeper",delegate {return delegate {return true;};}))
                before.Process(reloadRequest); // Persists dispatch marker, simulates lost final reply.
            using (SaveLoop after = new SaveLoop(root,"corekeeper",delegate {throw new Exception("must never invoke after reload");}))
                Check(after.Process(reloadRequest).Contains("status=unknown"),"durable dispatch marker prevents redispatch after plugin reload");
            Check(!SaveLoop.Nonce(new string('0',32))&&!SaveLoop.Nonce(new string('A',32)),"nonce encoding is canonical");
            using (SaveFiles files=new SaveFiles(root))
            {
                Invalid(delegate {files.Read("user-save.json",false);},"arbitrary read leaf rejected");
                Invalid(delegate {files.Publish("user-save.json","data");},"arbitrary write leaf rejected");
                Check(files.Read("request.txt",true)==null,"missing request is inactive");
            }
            {
                const string module = "vrising";
                File.Delete(Path.Combine(root,"response.txt"));
                File.WriteAllText(Path.Combine(root,"session.txt"),"LGSM_SAVE_1\nmodule="+module+"\nrun="+Run+"\ntoken="+Token+"\n",new System.Text.UTF8Encoding(false));
                const string issue = "vrising_write_failed";
                int failedInvokes = 0;
                string failedRequest = Request(Nonce()), failedReply;
                using (SaveLoop failure = new SaveLoop(root,module,new SaveAdapter(delegate { return delegate {
                    failedInvokes++; return SaveReceipt.Pending;
                }; }, delegate {return SaveReceipt.Failed;}, delegate {}, delegate {return issue;})))
                {
                    Check(failure.Process(failedRequest).Contains("status=dispatching"),"write failure is observed after owned admission");
                    failure.ObservePending(0);
                    failedReply = File.ReadAllText(Path.Combine(root,"response.txt"));
                    Check(failedReply.Contains("status=failed") && failedReply.Contains("issue="+issue),"native write failure retains a specific terminal result");
                    Check(!failedReply.Contains("status=completed") && failure.Process(failedRequest)==failedReply && failedInvokes==1,"failed operation never borrows success or redispatches");
                }
                using (SaveLoop replay = new SaveLoop(root,module,delegate {throw new Exception("must not repeat failed save");}))
                    Check(replay.Process(failedRequest)==failedReply,"failed receipt survives control reload");
                using (SaveLoop wrong = new SaveLoop(root,module,new SaveAdapter(delegate {return delegate {return SaveReceipt.Failed;};},null,null,
                    delegate {return "unsupported_failure";})))
                    Check(wrong.Process(Request(Nonce())).Contains("status=unknown"),"an unsupported failure code cannot certify an outcome");
            }
            foreach (string module in new string[] { "corekeeper", "barotrauma", "sonsoftheforest" })
                Check(!SaveLoop.ValidFailureIssue(module, "vrising_write_failed"), "native writer failure belongs to its own module");
            Console.WriteLine("PASS: "+passed+" managed save protocol checks");
        }
        finally
        {
            // Only this freshly created fixture directory, never a game path.
            Directory.Delete(root,true);
        }
    }
}
