using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Runtime.Serialization;
using System.Runtime.Serialization.Json;
using System.Security.Cryptography;
using System.Text;
using System.Xml;

namespace Lgsm.Valheim.Control
{
    [DataContract]
    internal sealed class ControlRequest
    {
        [DataMember(Name = "protocol", IsRequired = true)] public int Protocol { get; set; }
        [DataMember(Name = "nonce", IsRequired = true)] public string Nonce { get; set; }
        [DataMember(Name = "token", IsRequired = true)] public string Token { get; set; }
        [DataMember(Name = "op", IsRequired = true)] public string Op { get; set; }
        [DataMember(Name = "session_key", EmitDefaultValue = false)] public string SessionKey { get; set; }
        [DataMember(Name = "host_id", EmitDefaultValue = false)] public string HostId { get; set; }
        internal string Fingerprint;
    }

    [DataContract]
    internal sealed class ControlSession
    {
        [DataMember(Name = "protocol", IsRequired = true)] public int Protocol { get; set; }
        [DataMember(Name = "token", IsRequired = true)] public string Token { get; set; }
    }

    [DataContract]
    internal sealed class ControlPlayer
    {
        [DataMember(Name = "session_key")] public string SessionKey;
        [DataMember(Name = "name")] public string Name;
        [DataMember(Name = "host_id")] public string HostId;
        [DataMember(Name = "platform_id")] public string PlatformId;
        [DataMember(Name = "admin")] public bool Admin;
    }

    [DataContract]
    internal sealed class ControlAccess
    {
        [DataMember(Name = "admin")] public string[] Admin = new string[0];
        [DataMember(Name = "banned")] public string[] Banned = new string[0];
        [DataMember(Name = "permitted")] public string[] Permitted = new string[0];
    }

    internal sealed class ControlSnapshot
    {
        internal ControlPlayer[] Players = new ControlPlayer[0];
        internal ControlAccess Access = new ControlAccess();
        internal WorldSaveReceipt WorldSave;
    }

    [DataContract]
    internal sealed class ControlResponse
    {
        [DataMember(Name = "protocol")] public int Protocol = 1;
        [DataMember(Name = "nonce")] public string Nonce;
        [DataMember(Name = "pid")] public int Pid;
        [DataMember(Name = "op")] public string Op;
        [DataMember(Name = "status")] public string Status;
        [DataMember(Name = "error", EmitDefaultValue = false)] public string Error;
        [DataMember(Name = "players")] public ControlPlayer[] Players = new ControlPlayer[0];
        [DataMember(Name = "access")] public ControlAccess Access = new ControlAccess();
        [DataMember(Name = "world_save", EmitDefaultValue = false)] public WorldSaveReceipt WorldSave;
        [DataMember(Name = "capabilities")] public string[] Capabilities = new[] { "query", "reload", "kick", "save" };
    }

    internal sealed class ControlFailure : Exception
    {
        internal readonly string Code;
        internal ControlFailure(string code) : base(code) { Code = code; }
    }

    internal interface IValheimControl
    {
        ControlSnapshot Query();
        ControlSnapshot Reload();
        void Kick(string sessionKey, string hostId);
    }

    internal sealed class ControlPublication
    {
        internal ControlResponse Response { get; private set; }
        private double deadline;
        internal void Offer(ControlResponse response, double now)
        {
            if (response == null) return;
            if (Response == null || Response.Nonce != response.Nonce) deadline = now + 5.0;
            Response = response;
        }
        internal bool Expired(double now) { return Response != null && now >= deadline; }
        internal void Clear() { Response = null; }
    }

    internal static class ControlProtocol
    {
        internal const int MaxRequestBytes = 16 * 1024;
        internal const int MaxResponseBytes = 64 * 1024;
        internal const int MaxSessionBytes = 4 * 1024;

        internal static bool IsNonce(string value)
        {
            if (value == null || value.Length != 32) return false;
            bool nonzero = false;
            foreach (char c in value)
            {
                if (!((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f'))) return false;
                if (c != '0') nonzero = true;
            }
            return nonzero;
        }

        internal static bool IsToken(string value)
        {
            if (value == null || value.Length < 32 || value.Length > 128) return false;
            foreach (char c in value) if (c < 33 || c > 126) return false;
            return true;
        }

        internal static bool SameToken(string left, string right)
        {
            if (left == null || right == null) return false;
            int difference = left.Length ^ right.Length;
            int count = Math.Max(left.Length, right.Length);
            for (int i = 0; i < count; i++)
                difference |= (i < left.Length ? left[i] : 0) ^ (i < right.Length ? right[i] : 0);
            return difference == 0;
        }

        internal static bool SafeText(string value, int limit, bool spaces)
        {
            if (String.IsNullOrWhiteSpace(value) || !BoundedText(value, limit)) return false;
            if (!spaces) foreach (char c in value) if (Char.IsWhiteSpace(c)) return false;
            return true;
        }

        internal static bool BoundedText(string value, int limit)
        {
            if (value == null) return false;
            int count = 0;
            for (int i = 0; i < value.Length; i++)
            {
                char c = value[i];
                if (Char.IsControl(c)) return false;
                if (Char.IsHighSurrogate(c))
                {
                    if (++i >= value.Length || !Char.IsLowSurrogate(value[i])) return false;
                }
                else if (Char.IsLowSurrogate(c)) return false;
                if (++count > limit) return false;
            }
            return true;
        }

        private static bool JsonSpace(char value)
        {
            return value == ' ' || value == '\t' || value == '\r' || value == '\n';
        }

        private static void ValidateFrame(string text)
        {
            int start = 0;
            while (start < text.Length && JsonSpace(text[start])) start++;
            if (start >= text.Length || text[start] != '{') throw new SerializationException();
            bool quoted = false, escaped = false;
            for (int i = start + 1; i < text.Length; i++)
            {
                char value = text[i];
                if (quoted)
                {
                    if (escaped) escaped = false;
                    else if (value == '\\') escaped = true;
                    else if (value == '"') quoted = false;
                    continue;
                }
                if (value == '"') quoted = true;
                else if (value == '{' || value == '[' || value == ']') throw new SerializationException();
                else if (value == '}')
                {
                    for (int remaining = i + 1; remaining < text.Length; remaining++)
                        if (!JsonSpace(text[remaining])) throw new SerializationException();
                    return;
                }
            }
            throw new SerializationException();
        }

        private static T ReadEnvelope<T>(byte[] bytes, int maximum, string errorCode, params string[] allowed)
        {
            if (bytes == null || bytes.Length == 0 || bytes.Length > maximum)
                throw new ControlFailure(errorCode);
            try
            {
                // The native serializer ignores unknown members. Inspect the
                // bounded envelope first so duplicate/extra fields cannot
                // acquire meaning when the closed protocol evolves.
                // Framework JSON readers stop at the first root and may
                // buffer trailing bytes, so verify the single flat frame.
                ValidateFrame(new UTF8Encoding(false, true).GetString(bytes));
                XmlDictionaryReaderQuotas quotas = new XmlDictionaryReaderQuotas {
                    MaxDepth = 4, MaxArrayLength = 128, MaxStringContentLength = maximum,
                    MaxBytesPerRead = 4096, MaxNameTableCharCount = 1024
                };
                using (XmlDictionaryReader reader = JsonReaderWriterFactory.CreateJsonReader(bytes, quotas))
                {
                    reader.MoveToContent();
                    if (reader.LocalName != "root" || reader.GetAttribute("type") != "object")
                        throw new SerializationException();
                    HashSet<string> fields = new HashSet<string>(StringComparer.Ordinal);
                    HashSet<string> permitted = new HashSet<string>(allowed, StringComparer.Ordinal);
                    while (reader.Read())
                    {
                        if (reader.NodeType != XmlNodeType.Element) continue;
                        string field = reader.LocalName;
                        if (reader.Depth != 1 || !permitted.Contains(field) ||
                            !fields.Add(field) || reader.GetAttribute("type") != (field == "protocol" ? "number" : "string"))
                            throw new SerializationException();
                    }
                }
                using (MemoryStream stream = new MemoryStream(bytes, false))
                {
                    DataContractJsonSerializer serializer = new DataContractJsonSerializer(
                        typeof(T), null, 128, false, null, false);
                    T result = (T)serializer.ReadObject(stream);
                    if (stream.Position != stream.Length) throw new SerializationException();
                    return result;
                }
            }
            catch (Exception error)
            {
                if (!(error is SerializationException || error is FormatException ||
                    error is XmlException || error is InvalidCastException || error is DecoderFallbackException))
                    throw;
                throw new ControlFailure(errorCode);
            }
        }

        internal static string DecodeSession(byte[] bytes)
        {
            ControlSession session = ReadEnvelope<ControlSession>(bytes, MaxSessionBytes, "invalid_session", "protocol", "token");
            if (session == null || session.Protocol != 1 || !IsNonce(session.Token)) throw new ControlFailure("invalid_session");
            return session.Token;
        }

        internal static ControlRequest Decode(byte[] bytes)
        {
            if (bytes == null || bytes.Length == 0 || bytes.Length > MaxRequestBytes)
                throw new ControlFailure("request_limit");
            ControlRequest request = ReadEnvelope<ControlRequest>(bytes, MaxRequestBytes, "invalid_request",
                "protocol", "nonce", "token", "op", "session_key", "host_id");
            if (request == null || !IsNonce(request.Nonce)) throw new ControlFailure("invalid_nonce");
            using (SHA256 hash = SHA256.Create())
                request.Fingerprint = Convert.ToBase64String(hash.ComputeHash(bytes));
            return request;
        }

        internal static string Validate(ControlRequest request, string token)
        {
            if (request.Protocol != 1) return "unsupported_protocol";
            if (!IsToken(request.Token) || !SameToken(request.Token, token)) return "unauthorized";
            if (request.Op != "query" && request.Op != "reload" && request.Op != "kick" && request.Op != "save")
                return "unsupported_operation";
            if (request.Op == "kick")
                return IsNonce(request.SessionKey) && SafeText(request.HostId, 256, false)
                    ? null : "invalid_target";
            return String.IsNullOrEmpty(request.SessionKey) && String.IsNullOrEmpty(request.HostId)
                ? null : "unexpected_target";
        }

        internal static byte[] Encode(ControlResponse response)
        {
            using (MemoryStream stream = new MemoryStream())
            {
                DataContractJsonSerializer serializer = new DataContractJsonSerializer(
                    typeof(ControlResponse), null, 16384, false, null, false);
                serializer.WriteObject(stream, response);
                if (stream.Length > MaxResponseBytes) throw new ControlFailure("response_limit");
                return stream.ToArray();
            }
        }
    }

    // The Unity owner calls this serial state machine. Only one mutation can be
    // pending; completed nonces retain their exact byte fingerprint and result.
    internal sealed class ControlLoop : IDisposable
    {
        private sealed class Completed
        {
            internal string Fingerprint;
            internal ControlResponse Response;
        }
        private readonly IValheimControl native;
        private readonly string token;
        private readonly int pid;
        private readonly Action<string> diagnostic;
        private readonly Dictionary<string, Completed> completed = new Dictionary<string, Completed>();
        private readonly Queue<string> order = new Queue<string>();
        private ControlRequest pending;
        private double pendingUntil;
        internal bool HasPending { get { return pending != null; } }

        internal ControlLoop(IValheimControl native, string token, int pid, Action<string> diagnostic = null)
        {
            if (!ControlProtocol.IsToken(token)) throw new ControlFailure("invalid_bootstrap");
            this.native = native;
            this.token = token;
            this.pid = pid;
            this.diagnostic = diagnostic;
        }

        internal ControlResponse Submit(byte[] bytes, double now)
        {
            ControlRequest request = ControlProtocol.Decode(bytes);
            string invalid = ControlProtocol.Validate(request, token);
            if (invalid != null) return Response(request, invalid, null);
            Completed old;
            if (completed.TryGetValue(request.Nonce, out old))
                return old.Fingerprint == request.Fingerprint ? old.Response : Response(request, "nonce_conflict", null);
            if (pending != null)
                return pending.Nonce == request.Nonce && pending.Fingerprint == request.Fingerprint
                    ? null : Response(request, pending.Nonce == request.Nonce ? "nonce_conflict" : "busy", null);
            try
            {
                if (request.Op == "save")
                {
                    IValheimWorldSave saving = native as IValheimWorldSave;
                    if (saving == null) throw new ControlFailure("unsupported_native_save_contract");
                    saving.BeginSave();
                    pending = request;
                    pendingUntil = now + 120.0;
                    return null;
                }
                if (request.Op == "kick")
                {
                    native.Kick(request.SessionKey, request.HostId);
                    pending = request;
                    pendingUntil = now + 10.0;
                    return null;
                }
                return Finish(request, null, request.Op == "reload" ? native.Reload() : native.Query());
            }
            catch (Exception error)
            {
                string loggingFailure = ReportFailure(request.Op, "begin", error);
                return Finish(request, FailureCode(error, loggingFailure), null);
            }
        }

        internal ControlResponse Tick(double now)
        {
            if (pending == null) return null;
            ControlRequest request = pending;
            try
            {
                if (request.Op == "save")
                {
                    IValheimWorldSave saving = (IValheimWorldSave)native;
                    WorldSaveReceipt receipt = saving.PollSave();
                    if (receipt == null && now < pendingUntil) return null;
                    pending = null;
                    saving.EndSaveObservation();
                    return Finish(request, receipt == null ? "save_outcome_unknown" : null,
                        receipt == null ? null : new ControlSnapshot { WorldSave = receipt });
                }
                ControlSnapshot snapshot = native.Query();
                bool present = false;
                foreach (ControlPlayer player in snapshot.Players)
                    if (player.SessionKey == request.SessionKey && player.HostId == request.HostId) present = true;
                if (present && now < pendingUntil) return null;
                pending = null;
                return Finish(request, present ? "kick_timeout" : null, snapshot);
            }
            catch (Exception error)
            {
                string loggingFailure = ReportFailure(request.Op, "observe", error);
                pending = null;
                if (request.Op == "save") ((IValheimWorldSave)native).EndSaveObservation();
                string code = FailureCode(error, loggingFailure);
                return Finish(request, request.Op == "save" && code != "save_failed"
                    ? "save_outcome_unknown" : code, null);
            }
        }

        public void Dispose()
        {
            IDisposable owned = native as IDisposable;
            if (owned != null) owned.Dispose();
            pending = null;
        }

        private static string FailureCode(Exception error, string loggingFailure)
        {
            ControlFailure known = error as ControlFailure;
            if (known != null) return known.Code;
            StringBuilder detail = new StringBuilder("native_error");
            AppendFailureMetadata(detail, error, 2);
            if (loggingFailure != null) detail.Append(" log_failure=").Append(loggingFailure);
            // Error text already has a closed 512-scalar consumer limit. Native
            // metadata is diagnostic only; a failure still has no save receipt.
            return detail.Length > 512 ? detail.ToString(0, 512) : detail.ToString();
        }

        private static void AppendFailureMetadata(StringBuilder detail, Exception error, int frameLimit)
        {
            List<Exception> causes = new List<Exception>();
            for (int cause = 0; error != null && cause < 4; cause++, error = error.InnerException) causes.Add(error);
            // Put the native cause first so the bounded IPC error retains it
            // even when reflection wrappers have long implementation names.
            for (int cause = causes.Count - 1; cause >= 0; cause--)
            {
                error = causes[cause];
                detail.Append(" type=").Append(error.GetType().FullName);
                StackFrame[] frames;
                try { frames = new StackTrace(error, false).GetFrames(); }
                catch (Exception unavailable)
                {
                    detail.Append(" stack_unavailable=").Append(unavailable.GetType().FullName);
                    continue;
                }
                if (frames == null) continue;
                for (int index = 0; index < frames.Length && index < frameLimit; index++)
                {
                    System.Reflection.MethodBase method = frames[index].GetMethod();
                    if (method == null) continue;
                    detail.Append(" at=").Append(method.DeclaringType == null ? "" : method.DeclaringType.FullName)
                        .Append('.').Append(method.Name);
                }
            }
        }

        private string ReportFailure(string operation, string phase, Exception error)
        {
            if (diagnostic == null || error is ControlFailure) return null;
            // Native exception messages may contain paths or request data.
            // Record only bounded type/method identities, never the message,
            // request envelope, bootstrap token, or a full user stack trace.
            StringBuilder detail = new StringBuilder("LGSM_VALHEIM_CONTROL_FAILURE op=" + operation + " phase=" + phase);
            AppendFailureMetadata(detail, error, 4);
            try { diagnostic(detail.Length > 4096 ? detail.ToString(0, 4096) : detail.ToString()); }
            catch (Exception loggingFailure)
            {
                // A failed diagnostic sink must not replace the native failure
                // or prevent publishing and caching its one-shot response.
                return loggingFailure.GetType().FullName;
            }
            return null;
        }

        private ControlResponse Response(ControlRequest request, string error, ControlSnapshot snapshot)
        {
            return new ControlResponse {
                Nonce = request.Nonce, Pid = pid,
                Op = request.Op == "query" || request.Op == "reload" || request.Op == "kick" || request.Op == "save" ? request.Op : "",
                Status = error == null ? "ok" : "error", Error = error,
                Players = snapshot == null ? new ControlPlayer[0] : snapshot.Players,
                Access = snapshot == null ? new ControlAccess() : snapshot.Access,
                WorldSave = snapshot == null ? null : snapshot.WorldSave
            };
        }

        private ControlResponse Finish(ControlRequest request, string error, ControlSnapshot snapshot)
        {
            ControlResponse response = Response(request, error, snapshot);
            try { ControlProtocol.Encode(response); }
            catch (ControlFailure) { response = Response(request, "response_limit", null); }
            while (order.Count >= 64) completed.Remove(order.Dequeue());
            completed.Add(request.Nonce, new Completed { Fingerprint = request.Fingerprint, Response = response });
            order.Enqueue(request.Nonce);
            return response;
        }
    }
}
