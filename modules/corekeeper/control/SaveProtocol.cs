using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Reflection;
using System.Security.Cryptography;
using System.Text;

namespace LanGame.SaveControl
{
    // Completion is an explicit adapter receipt, never inferred from a void
    // reflection return, a nearby log line, or an unrelated file change.
    internal enum SaveReceipt { Rejected, Dispatched, Completed, Pending, Failed }
    internal sealed class SaveAdapter
    {
        internal readonly Func<Func<SaveReceipt>> Prepare;
        internal readonly Func<SaveReceipt> Observe;
        internal readonly Action Abandon;
        internal readonly Func<string> UnconfirmedIssue;
        internal SaveAdapter(Func<Func<SaveReceipt>> prepare) : this(prepare, null, null) { }
        internal SaveAdapter(Func<Func<SaveReceipt>> prepare, Func<SaveReceipt> observe, Action abandon)
            : this(prepare, observe, abandon, null) { }
        internal SaveAdapter(Func<Func<SaveReceipt>> prepare, Func<SaveReceipt> observe, Action abandon, Func<string> unconfirmedIssue)
        { Prepare = prepare; Observe = observe; Abandon = abandon; UnconfirmedIssue = unconfirmedIssue; }
    }
    internal sealed class SaveFailure : Exception
    {
        internal readonly string Code;
        internal SaveFailure(string code) : base(code) { Code = code; }
    }

    internal static class NativeBinding
    {
        internal static Type Type(string assembly, string name)
        {
            return System.Type.GetType(name + ", " + assembly, true);
        }
        internal static MethodInfo Method(Type type, string name, BindingFlags flags, Type result, params Type[] args)
        {
            MethodInfo method = type.GetMethod(name, flags, null, args, null);
            if (method == null || method.ReturnType != result) throw new SaveFailure("api_unavailable");
            return method;
        }
        internal static void VerifyFile(string path, string expected)
        {
            using (FileStream file = SaveFiles.OpenPinnedFile(path, 512L * 1024 * 1024))
            using (SHA256 sha = SHA256.Create())
                if (BitConverter.ToString(sha.ComputeHash(file)).Replace("-", "").ToLowerInvariant() != expected)
                    throw new SaveFailure("unsupported_native_build");
        }
    }

    internal sealed class SaveLoop : IDisposable
    {
        internal const string Header = "LGSM_SAVE_1";
        private readonly SaveFiles files;
        private readonly string module, run, token;
        private readonly Func<Func<SaveReceipt>> prepare;
        private readonly SaveAdapter adapter;
        private readonly Action<string> diagnostic;
        private readonly string diagnosticRoot;
        internal bool FailureDiagnosticUnavailable { get; private set; }
        private readonly Dictionary<string, KeyValuePair<string, string>> history = new Dictionary<string, KeyValuePair<string, string>>();
        private readonly Queue<string> order = new Queue<string>();
        private readonly Stopwatch poll = Stopwatch.StartNew();
        private long nextPoll;
        private string last;
        private string pendingNonce, pendingRequest;
        private long pendingSince;
        internal const long CompletionBudgetMilliseconds = 120000;

        internal SaveLoop(string root, string module, Func<Func<bool>> prepare)
            : this(root, module, new SaveAdapter(delegate {
                Func<bool> invoke = prepare();
                return delegate { return invoke() ? SaveReceipt.Dispatched : SaveReceipt.Rejected; };
            })) { }

        internal SaveLoop(string root, string module, SaveAdapter adapter, Action<string> diagnostic = null)
        {
            this.module = module;
            this.diagnostic = diagnostic;
            this.diagnosticRoot = Path.GetDirectoryName(Path.GetDirectoryName(root));
            this.adapter = adapter;
            this.prepare = adapter.Prepare;
            files = new SaveFiles(root);
            try
            {
                string[] session = Lines(files.Read("session.txt", false), 4);
                if (session[0] != Header || Value(session[1], "module") != module) throw new SaveFailure("invalid_session");
                run = Value(session[2], "run"); token = Value(session[3], "token");
                if (!Nonce(run) || !Nonce(token)) throw new SaveFailure("invalid_session");
            }
            catch { files.Dispose(); throw; }
        }

        internal static bool Nonce(string value)
        {
            if (value == null || value.Length != 32 || value == new string('0', 32)) return false;
            foreach (char c in value) if (!((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f'))) return false;
            return true;
        }
        internal static string[] Lines(string value, int count)
        {
            if (value == null || value.Length > 2048 || !value.EndsWith("\n", StringComparison.Ordinal))
                throw new SaveFailure("invalid_frame");
            foreach (char c in value) if (c != '\n' && (c < 32 || c > 126)) throw new SaveFailure("invalid_frame");
            string[] lines = value.Substring(0, value.Length - 1).Split('\n');
            if (lines.Length != count) throw new SaveFailure("invalid_frame");
            return lines;
        }
        internal static string Value(string line, string key)
        {
            string prefix = key + "=";
            if (!line.StartsWith(prefix, StringComparison.Ordinal)) throw new SaveFailure("invalid_frame");
            return line.Substring(prefix.Length);
        }
        private static bool Same(string left, string right)
        {
            int diff = left.Length ^ right.Length;
            for (int i = 0; i < Math.Max(left.Length, right.Length); i++)
                diff |= (i < left.Length ? left[i] : 0) ^ (i < right.Length ? right[i] : 0);
            return diff == 0;
        }
        internal static long Now() { return (DateTime.UtcNow.Ticks - 621355968000000000L) / 10000; }
        private string Reply(string nonce, string status, string issue)
        {
            return Header + "\nmodule=" + module + "\nrun=" + run + "\nnonce=" + nonce +
                "\npid=" + System.Diagnostics.Process.GetCurrentProcess().Id.ToString(CultureInfo.InvariantCulture) +
                "\nstatus=" + status + "\nissue=" + issue + "\n";
        }

        private string Previous(string nonce)
        {
            // A loader reload must not repeat a still-admissible frame after
            // losing its in-memory nonce cache. The persisted dispatch marker
            // is deliberately conservative if the previous callback vanished.
            string previous = files.Read("response.txt", true);
            if (previous == null) return null;
            string[] fields = Lines(previous, 7);
            string oldNonce = Value(fields[3], "nonce");
            if (fields[0] != Header || Value(fields[1], "module") != module || !Nonce(oldNonce))
                throw new SaveFailure("invalid_frame");
            if (oldNonce != nonce) return null;
            if (Value(fields[2], "run") != run || Value(fields[4], "pid") !=
                System.Diagnostics.Process.GetCurrentProcess().Id.ToString(CultureInfo.InvariantCulture))
                throw new SaveFailure("invalid_frame");
            string status = Value(fields[5], "status"), issue = Value(fields[6], "issue");
            if ((status == "completed" && issue == "none") ||
                (status == "dispatched" && ValidUnconfirmedIssue(issue)) || status == "rejected" ||
                (status == "unknown" && issue == "native_outcome_unknown") ||
                (status == "failed" && ValidFailureIssue(module, issue))) return previous;
            if (status == "dispatching" && issue == "none") return Reply(nonce, "unknown", "native_outcome_unknown");
            throw new SaveFailure("invalid_frame");
        }

        internal static bool ValidUnconfirmedIssue(string issue)
        {
            return issue == "none";
        }
        internal static bool ValidFailureIssue(string module, string issue)
        {
            return module == "vrising" && issue == "vrising_write_failed";
        }
        private string FailedReply(string nonce)
        {
            string issue = adapter.UnconfirmedIssue == null ? null : adapter.UnconfirmedIssue();
            if (!ValidFailureIssue(module, issue)) throw new SaveFailure("native_outcome_unknown");
            return Reply(nonce, "failed", issue);
        }

        internal string Process(string request)
        {
            string[] lines = Lines(request, 6);
            string requestedRun = Value(lines[1], "run"), nonce = Value(lines[2], "nonce");
            string suppliedToken = Value(lines[3], "token"), op = Value(lines[4], "op");
            string expiresText = Value(lines[5], "expires");
            long expires;
            if (lines[0] != Header || !Nonce(requestedRun) || !Nonce(nonce) || !Nonce(suppliedToken) ||
                !Int64.TryParse(expiresText, NumberStyles.None, CultureInfo.InvariantCulture, out expires) ||
                expires.ToString(CultureInfo.InvariantCulture) != expiresText) throw new SaveFailure("invalid_frame");
            if (requestedRun != run || !Same(suppliedToken, token)) return Reply(nonce, "rejected", "unauthorized");
            if (op != "save") return Reply(nonce, "rejected", "unsupported_operation");
            KeyValuePair<string, string> old;
            if (history.TryGetValue(nonce, out old)) return old.Key == request ? old.Value : Reply(nonce, "rejected", "nonce_conflict");
            string recovered = Previous(nonce);
            if (recovered != null) return recovered;
            if (pendingNonce != null) return Reply(nonce, "rejected", "save_busy");
            string response;
            bool marked = false;
            try
            {
                if (expires <= Now() || expires > Now() + 10000) throw new SaveFailure("request_expired");
                Func<SaveReceipt> invoke = prepare();
                // This marker precedes the only native call. A lost reply or a
                // caller timeout after this point is never a no-op claim.
                files.Publish("response.txt", Reply(nonce, "dispatching", "none"));
                marked = true;
                if (expires <= Now()) response = Reply(nonce, "rejected", "request_expired");
                else
                {
                    SaveReceipt receipt = invoke();
                    if (receipt == SaveReceipt.Completed) response = Reply(nonce, "completed", "none");
                    else if (receipt == SaveReceipt.Dispatched)
                    {
                        string issue = adapter.UnconfirmedIssue == null ? "none" : adapter.UnconfirmedIssue();
                        if (!ValidUnconfirmedIssue(issue)) throw new SaveFailure("native_outcome_unknown");
                        response = Reply(nonce, "dispatched", issue);
                    }
                    else if (receipt == SaveReceipt.Failed)
                        response = FailedReply(nonce);
                    else if (receipt == SaveReceipt.Rejected) response = Reply(nonce, "rejected", "native_rejected");
                    else if (receipt == SaveReceipt.Pending && adapter.Observe != null)
                    {
                        pendingNonce = nonce; pendingRequest = request; pendingSince = poll.ElapsedMilliseconds;
                        response = Reply(nonce, "dispatching", "none");
                    }
                    else throw new SaveFailure("native_outcome_unknown");
                }
            }
            catch (Exception error)
            {
                SaveFailure known = error as SaveFailure;
                response = Reply(nonce, marked ? "unknown" : "rejected", marked ? "native_outcome_unknown" :
                    known == null ? "api_unavailable" : known.Code);
                if (marked && module == "sonsoftheforest" && diagnostic != null &&
                    Environment.GetEnvironmentVariable("LANGAME_NATIVE_SAVE_FAILURE_ACCEPTANCE") == "true")
                {
                    try { ReportNativeFailure(nonce, error); }
                    catch (Exception) { FailureDiagnosticUnavailable = true; }
                    // Diagnostics cannot change or replace the original unknown.
                    // A failed sink is observable to tests and cannot pass the
                    // host's correlated native-failure evidence requirement.
                }
            }
            if (pendingNonce == null && adapter.Abandon != null)
            {
                try { adapter.Abandon(); }
                catch { response = Reply(nonce, marked ? "unknown" : "rejected", marked ? "native_outcome_unknown" : "api_unavailable"); }
            }
            while (order.Count >= 64) history.Remove(order.Dequeue());
            history.Add(nonce, new KeyValuePair<string, string>(request, response));
            order.Enqueue(nonce);
            return response;
        }

        private void ReportNativeFailure(string nonce, Exception error)
        {
            string prefix = "LGSM_SONS_NATIVE_WRITE_FAILURE run=" + run + " nonce=" + nonce +
                " pid=" + System.Diagnostics.Process.GetCurrentProcess().Id.ToString(CultureInfo.InvariantCulture);
            string fixture = diagnosticRoot.Replace('\\', '/');
            if (fixture.StartsWith("//?/", StringComparison.Ordinal)) fixture = fixture.Substring(4);
            Queue<Exception> pending = new Queue<Exception>();
            HashSet<Exception> seen = new HashSet<Exception>();
            pending.Enqueue(error);
            int emitted = 0;
            string[] methods = { "SaveGameManager.WriteJsonZip", "System.IO.File.Copy",
                "SaveGameManager.RegisterBeforeSaveCallback", "SaveGameManager.RegisterAfterSaveCallback",
                "SaveGameManager.UnregisterBeforeSaveCallback", "SaveGameManager.UnregisterAfterSaveCallback",
                "NativeSaveCompletion.Convert", "NativeSaveCompletion.Begin", "Capture.Dispose",
                "NativeBinding.Method", "DelegateSupport.ConvertDelegate",
                "NativeCallbackReceiver.Create", "NativeCallbackReceiver.Bind", "NativeCallbackReceiver.Verify" };
            while (pending.Count != 0 && seen.Count < 32)
            {
                Exception node = pending.Dequeue();
                if (node == null || !seen.Add(node)) continue;
                diagnostic(prefix + " node=" + seen.Count.ToString(CultureInfo.InvariantCulture) +
                    " exception_type=" + node.GetType().FullName + " hresult=" + node.HResult.ToString("X8", CultureInfo.InvariantCulture));
                Exception ownedWrite = node.Data["sons_native_write_exception"] as Exception;
                if (ownedWrite != null && pending.Count < 32) pending.Enqueue(ownedWrite);
                int[] counts = node.Data["sons_callback_counts"] as int[];
                if (counts != null && counts.Length == 6 && emitted < 24)
                {
                    diagnostic(prefix + " callback_before=" + counts[0].ToString(CultureInfo.InvariantCulture) +
                        " callback_after=" + counts[1].ToString(CultureInfo.InvariantCulture) +
                        " callback_invalid=" + (counts[2] == 1 ? "true" : "false") +
                        " callback_current=" + (counts[3] == 1 ? "true" : "false") +
                        " callback_owner_thread=" + (counts[4] == 1 ? "true" : "false") +
                        " callback_closed=" + (counts[5] == 1 ? "true" : "false")); emitted++;
                }
                if (node.Data["sons_native_diagnostic_unavailable"] is bool &&
                    (bool)node.Data["sons_native_diagnostic_unavailable"] && emitted < 24)
                { diagnostic(prefix + " diagnostic_unavailable=true"); emitted++; }
                // Do not call AggregateException.ToString/Flatten: those expand
                // the entire graph. Bound our own traversal and each node's text.
                AggregateException aggregate = node as AggregateException;
                if (aggregate != null)
                    for (int index = 0; index < aggregate.InnerExceptions.Count && pending.Count < 32; index++)
                        pending.Enqueue(aggregate.InnerExceptions[index]);
                if (node.InnerException != null && pending.Count < 32) pending.Enqueue(node.InnerException);
                string message = node.Message ?? "", stack = node.StackTrace ?? "";
                if (message.Length > 16384) message = message.Substring(0, 16384);
                if (stack.Length > 16384) stack = stack.Substring(0, 16384);
                string[] lines = (message + "\n" + stack).Replace('\\', '/').Split('\n');
                for (int index = 0; index < lines.Length && index < 128 && emitted < 24; index++)
                {
                    string line = lines[index].Trim();
                    if (line.Length > 1024) continue;
                    int start = line.IndexOf(fixture, StringComparison.OrdinalIgnoreCase);
                    if ((node is IOException || line.IndexOf("IOException", StringComparison.Ordinal) >= 0) &&
                        line.IndexOf("SaveData.zip", StringComparison.OrdinalIgnoreCase) >= 0 && start >= 0)
                    {
                        line = line.Substring(0, start) + "<fixture>" + line.Substring(start + fixture.Length);
                        diagnostic(prefix + " detail=IOException " + line.Replace(token, "<token>")); emitted++;
                    }
                    else if ((line.IndexOf("Il2CppSystem.Action", StringComparison.Ordinal) >= 0 ||
                        line.IndexOf("Il2CppInterop.", StringComparison.Ordinal) >= 0 ||
                        line.IndexOf("SaveGameManager.Register", StringComparison.Ordinal) >= 0 ||
                        line.IndexOf("SaveGameManager.Unregister", StringComparison.Ordinal) >= 0) &&
                        !line.Contains("/") && !line.StartsWith("at ", StringComparison.Ordinal))
                    { diagnostic(prefix + " message=" + line.Replace(token, "<token>")); emitted++; }
                    foreach (string method in methods)
                        if (line.IndexOf(method, StringComparison.Ordinal) >= 0 && emitted < 24)
                        { diagnostic(prefix + " detail=at " + method); emitted++; }
                    if ((line == "Object reference not set to an instance of an object." ||
                        line == "Collection was modified; enumeration operation may not execute." ||
                        line == "Parameter count mismatch.") && emitted < 24)
                    { diagnostic(prefix + " message=" + line); emitted++; }
                }
            }
            diagnostic(prefix + " details=" + emitted.ToString(CultureInfo.InvariantCulture) +
                " nodes=" + seen.Count.ToString(CultureInfo.InvariantCulture));
        }

        internal void Update()
        {
            if (poll.ElapsedMilliseconds < nextPoll) return;
            nextPoll = poll.ElapsedMilliseconds + 200;
            ObservePending(poll.ElapsedMilliseconds);
            string request = files.Read("request.txt", true);
            if (request == null || request == last) return;
            string response = Process(request);
            files.Publish("response.txt", response);
            last = request;
        }
        internal void ObservePending(long elapsedMilliseconds)
        {
            if (pendingNonce != null)
            {
                string response = null;
                try
                {
                    SaveReceipt observed = adapter.Observe();
                    if (observed == SaveReceipt.Completed) response = Reply(pendingNonce, "completed", "none");
                    else if (observed == SaveReceipt.Dispatched)
                    {
                        string issue = adapter.UnconfirmedIssue == null ? "none" : adapter.UnconfirmedIssue();
                        if (!ValidUnconfirmedIssue(issue)) throw new SaveFailure("native_outcome_unknown");
                        response = Reply(pendingNonce, "dispatched", issue);
                    }
                    else if (observed == SaveReceipt.Failed) response = FailedReply(pendingNonce);
                    else if (observed != SaveReceipt.Pending || elapsedMilliseconds - pendingSince >= CompletionBudgetMilliseconds)
                        response = Reply(pendingNonce, "unknown", "native_outcome_unknown");
                }
                catch { response = Reply(pendingNonce, "unknown", "native_outcome_unknown"); }
                if (response != null)
                {
                    string nonce = pendingNonce, request = pendingRequest;
                    try { EndObservation(); }
                    catch { response = Reply(nonce, "unknown", "native_outcome_unknown"); }
                    history[nonce] = new KeyValuePair<string, string>(request, response);
                    files.Publish("response.txt", response);
                }
            }
        }
        private void EndObservation()
        {
            pendingNonce = null; pendingRequest = null;
            if (adapter.Abandon != null) adapter.Abandon();
        }
        public void Dispose() { try { EndObservation(); } finally { files.Dispose(); } }
    }
}
