using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.Serialization.Json;
using System.Text;

namespace Lgsm.Valheim.Control
{
    internal static class ControlTests
    {
        internal static readonly string Token = Guid.NewGuid().ToString("N");
        private static int passed;
        internal static void Assert(bool value, string message)
        {
            if (!value) throw new Exception(message);
        }
        internal static void Test(string name, Action test)
        {
            test();
            passed++;
            Console.WriteLine("PASS " + name);
        }
        internal static void Fails(string code, Action action)
        {
            try { action(); }
            catch (ControlFailure failure) { Assert(failure.Code == code, "Expected " + code + "; got " + failure.Code); return; }
            throw new Exception("Expected failure " + code);
        }
        internal static byte[] Request(string op, string nonce, string session, string host, string token)
        {
            using (MemoryStream stream = new MemoryStream())
            {
                new DataContractJsonSerializer(typeof(ControlRequest)).WriteObject(stream,
                    new ControlRequest { Protocol = 1, Nonce = nonce, Op = op, Token = token,
                        SessionKey = session, HostId = host });
                return stream.ToArray();
            }
        }
        internal static byte[] Request(string op)
        {
            return Request(op, Guid.NewGuid().ToString("N"), null, null, Token);
        }
        private sealed class FakeControl : IValheimControl
        {
            internal int Queries, Reloads, Kicks;
            internal ControlSnapshot Snapshot = new ControlSnapshot();
            public ControlSnapshot Query() { Queries++; return Snapshot; }
            public ControlSnapshot Reload() { Reloads++; return Snapshot; }
            public void Kick(string session, string host) { Kicks++; }
        }
        private static FakeControl PlayerControl(string session)
        {
            return new FakeControl { Snapshot = new ControlSnapshot { Players = new[] {
                new ControlPlayer { SessionKey = session, Name = "Raven", HostId = "76561198000000001",
                    PlatformId = "Steam_76561198000000001" }
            } } };
        }
        private static void ProtocolTests()
        {
            Test("session bootstrap is closed bounded and exact", delegate {
                string token = new string('a', 32);
                string valid = "{\"protocol\":1,\"token\":\"" + token + "\"}";
                Assert(ControlProtocol.DecodeSession(Encoding.UTF8.GetBytes(valid)) == token, "Valid session rejected");
                foreach (string malformed in new[] { valid + "{}", valid.Replace(":1", ":2"),
                    valid.Replace(token, token.ToUpperInvariant()), valid.Replace(token, "short"),
                    valid.Replace(token, new string('0', 32)),
                    valid.TrimEnd('}') + ",\"token\":\"" + token + "\"}",
                    valid.TrimEnd('}') + ",\"extra\":\"value\"}" })
                    Fails("invalid_session", delegate { ControlProtocol.DecodeSession(Encoding.UTF8.GetBytes(malformed)); });
                Fails("invalid_session", delegate { ControlProtocol.DecodeSession(new byte[4097]); });
            });
            Test("closed bounded UTF-8 envelope", delegate {
                byte[] valid = Request("query");
                ControlRequest parsed = ControlProtocol.Decode(valid);
                Assert(ControlProtocol.Validate(parsed, Token) == null, "Valid query rejected");
                Fails("request_limit", delegate { ControlProtocol.Decode(new byte[16385]); });
                Fails("invalid_request", delegate { ControlProtocol.Decode(new byte[] { 0xc3, 0x28 }); });
                string json = Encoding.UTF8.GetString(valid);
                int malformedCase = 0;
                foreach (string input in new[] { json + "{}", json + " trailing", "[]",
                    json.TrimEnd('}') + ",\"op\":\"kick\"}",
                    json.TrimEnd('}') + ",\"unknown\":1}",
                    json.Replace("\"protocol\":1", "\"protocol\":\"1\"") })
                {
                    malformedCase++;
                    try { Fails("invalid_request", delegate { ControlProtocol.Decode(Encoding.UTF8.GetBytes(input)); }); }
                    catch (Exception error) { throw new Exception("Malformed envelope case " + malformedCase, error); }
                }
            });
            Test("authentication and operation checks precede native calls", delegate {
                FakeControl native = new FakeControl();
                ControlLoop loop = new ControlLoop(native, Token, 123);
                ControlResponse unauthorized = loop.Submit(Request("query", Guid.NewGuid().ToString("N"), null, null,
                    "wrong_synthetic_token_0123456789abcdef"), 0);
                Assert(unauthorized.Error == "unauthorized", "Token check missing");
                Assert(loop.Submit(Request("execute"), 0).Error == "unsupported_operation", "Unknown operation allowed");
                Assert(loop.Submit(Request("kick"), 0).Error == "invalid_target", "Missing kick identity allowed");
                Assert(native.Queries == 0 && native.Reloads == 0 && native.Kicks == 0, "Unauthorized native call");
                string encoded = Encoding.UTF8.GetString(ControlProtocol.Encode(unauthorized));
                Assert(!encoded.Contains(Token) && !encoded.Contains("\"token\""), "Secret echoed");
                Assert(encoded.Contains("\"capabilities\"") && encoded.Contains("\"access\"") &&
                    encoded.Contains("\"players\""), "Error omitted closed response fields");
            });
            Test("query cache and changed nonce payload", delegate {
                FakeControl native = new FakeControl();
                ControlLoop loop = new ControlLoop(native, Token, 123);
                string nonce = Guid.NewGuid().ToString("N");
                byte[] bytes = Request("query", nonce, null, null, Token);
                ControlResponse result = loop.Submit(bytes, 0);
                Assert(result.Status == "ok" && result.Pid == 123 && result.Nonce == nonce, "Query envelope wrong");
                Assert(Object.ReferenceEquals(result, loop.Submit(bytes, 1)) && native.Queries == 1, "Query replay executed again");
                Assert(loop.Submit(Request("reload", nonce, null, null, Token), 1).Error == "nonce_conflict" &&
                    native.Reloads == 0, "Changed payload accepted");
                Assert(!Encoding.UTF8.GetString(ControlProtocol.Encode(result)).Contains("\"error\""), "Ok error should be omitted");
            });
            Test("kick waits for disappearance and is replay safe", delegate {
                string session = Guid.NewGuid().ToString("N");
                FakeControl native = PlayerControl(session);
                ControlLoop loop = new ControlLoop(native, Token, 123);
                byte[] request = Request("kick", Guid.NewGuid().ToString("N"), session, "76561198000000001", Token);
                Assert(loop.Submit(request, 0) == null && native.Kicks == 1, "Kick claimed immediate success");
                Assert(loop.Submit(request, 1) == null && native.Kicks == 1, "Pending kick replay executed twice");
                Assert(loop.Submit(Request("query"), 1).Error == "busy", "Second operation entered pending state");
                Assert(loop.Tick(2) == null, "Connected peer was reported kicked");
                native.Snapshot.Players = new ControlPlayer[0];
                ControlResponse done = loop.Tick(3);
                Assert(done.Status == "ok" && !loop.HasPending, "Absent peer did not finish kick");
                Assert(Object.ReferenceEquals(done, loop.Submit(request, 4)) && native.Kicks == 1, "Completed kick replay executed");
            });
            Test("kick timeout reports failure", delegate {
                string session = Guid.NewGuid().ToString("N");
                FakeControl native = PlayerControl(session);
                ControlLoop loop = new ControlLoop(native, Token, 123);
                loop.Submit(Request("kick", Guid.NewGuid().ToString("N"), session, "76561198000000001", Token), 5);
                Assert(loop.Tick(14.9) == null, "Kick timed out early");
                Assert(loop.Tick(15).Error == "kick_timeout" && !loop.HasPending, "Timeout was not explicit");
            });
            Test("response bound and Unicode scalar validation", delegate {
                Assert(ControlProtocol.SafeText("Raven \ud83d\udc26", 7, true), "Valid Unicode rejected");
                Assert(!ControlProtocol.SafeText("\ud800", 256, true) &&
                    !ControlProtocol.SafeText("line\nbreak", 256, true), "Unsafe text accepted");
                Fails("response_limit", delegate {
                    ControlProtocol.Encode(new ControlResponse { Nonce = Guid.NewGuid().ToString("N"),
                        Error = new string('a', 65537) });
                });
            });
            Test("publication deadline cannot be extended by same nonce", delegate {
                ControlPublication publication = new ControlPublication();
                ControlResponse response = new ControlResponse { Nonce = Guid.NewGuid().ToString("N") };
                publication.Offer(response, 0);
                publication.Offer(response, 4);
                ControlTests.Assert(!publication.Expired(4.9) && publication.Expired(5), "Publication retry is unbounded");
                publication.Clear();
                ControlTests.Assert(!publication.Expired(6) && publication.Response == null, "Publication did not clear");
            });
        }
        private static void FileTests(string output)
        {
            string fixture = Path.Combine(output, "ipc_" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(fixture);
            try
            {
                Test("fixed file boundary and atomic response", delegate {
                    Fails("invalid_control_root", delegate { using (new ControlFiles("relative")) { } });
                    using (ControlFiles files = new ControlFiles(fixture))
                    {
                        Fails("session_unavailable", delegate { files.ReadSessionToken(); });
                        string sessionPath = Path.Combine(fixture, "session.json");
                        string sessionCredential = new string('a', 32);
                        File.WriteAllText(sessionPath, "{\"protocol\":1,\"token\":\"" + sessionCredential + "\"}",
                            new UTF8Encoding(false));
                        Assert(files.ReadSessionToken() == sessionCredential, "session bootstrap read-back");
                        File.WriteAllBytes(sessionPath, new byte[4097]);
                        Fails("invalid_session", delegate { files.ReadSessionToken(); });
                        File.Delete(sessionPath);
                        Assert(files.ReadRequest() == null, "Missing request not empty");
                        byte[] body = Request("query");
                        string path = Path.Combine(fixture, "request.json");
                        File.WriteAllBytes(path, body);
                        Assert(Convert.ToBase64String(files.ReadRequest()) == Convert.ToBase64String(body), "Request changed");
                        string version = files.LastRequestVersion;
                        string retry = Path.Combine(fixture, "request_retry.tmp");
                        File.WriteAllBytes(retry, body);
                        File.Replace(retry, path, null);
                        files.ReadRequest();
                        Assert(files.LastRequestVersion != version, "Byte-identical atomic retry was not observable");
                        File.WriteAllBytes(path, new byte[16385]);
                        Fails("request_limit", delegate { files.ReadRequest(); });
                        File.Delete(path);
                        Directory.CreateDirectory(path);
                        Fails("request_unavailable", delegate { files.ReadRequest(); });
                        Directory.Delete(path);
                        ControlResponse response = new ControlResponse { Nonce = Guid.NewGuid().ToString("N"),
                            Pid = 123, Op = "query", Status = "ok" };
                        files.Publish(response);
                        response.Nonce = Guid.NewGuid().ToString("N");
                        files.Publish(response);
                        string result = File.ReadAllText(Path.Combine(fixture, "response.json"));
                        Assert(result.Contains(response.Nonce) && !result.Contains(Token), "Atomic response stale or secret exposed");
                        Assert(Directory.GetFiles(fixture, "*.tmp").Length == 0, "Owned publication scratch leaked");
                        string responsePath = Path.Combine(fixture, "response.json");
                        File.SetAttributes(responsePath, FileAttributes.ReadOnly);
                        try
                        {
                            bool refused = false;
                            try { files.Publish(response); } catch (UnauthorizedAccessException) { refused = true; }
                            Assert(refused, "Read-only response was overwritten");
                        }
                        finally { File.SetAttributes(responsePath, FileAttributes.Normal); }
                        bool pinned = false;
                        try { Directory.Move(fixture, fixture + "_moved"); }
                        catch (IOException) { pinned = true; }
                        Assert(pinned, "Pinned control directory could be replaced");
                    }
                });
            }
            finally
            {
                // Only files/directories created by this harness are removed.
                string request = Path.Combine(fixture, "request.json");
                string response = Path.Combine(fixture, "response.json");
                string session = Path.Combine(fixture, "session.json");
                if (File.Exists(request)) File.Delete(request);
                if (Directory.Exists(request)) Directory.Delete(request);
                if (File.Exists(response)) File.Delete(response);
                if (File.Exists(session)) File.Delete(session);
                if (Directory.Exists(fixture)) Directory.Delete(fixture);
            }
        }
        private static int Main(string[] arguments)
        {
            try
            {
                Assert(arguments.Length == 2 && ControlFiles.LocalAbsolute(arguments[0]) &&
                    ControlFiles.LocalAbsolute(arguments[1]), "Absolute owned fixtures required");
                ProtocolTests();
                NativeControlTests.Run(arguments[0]);
                FileTests(arguments[0]);
                Test("reparse root and native access ancestor rejected", delegate {
                    Fails("unsafe_control_path", delegate { using (new ControlFiles(arguments[1])) { } });
                    Fails("unsafe_control_path", delegate {
                        using (new NativeAccessPins(new[] { Path.Combine(arguments[1], "synthetic.txt") })) { }
                    });
                });
                Console.WriteLine("Valheim control boundary tests: " + passed + " passed");
                return 0;
            }
            catch (Exception error) { Console.Error.WriteLine(error); return 1; }
        }
    }
}
