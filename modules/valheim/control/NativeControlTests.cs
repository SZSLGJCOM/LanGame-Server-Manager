using System;
using System.Collections.Generic;
using System.IO;

namespace Lgsm.Valheim.Control
{
    // These fake types exercise the exact reflection contract without loading
    // licensed game code, Unity, the loader, or any server process.
    internal abstract class FakeSocket
    {
        internal string Host;
        internal bool Connected = true;
        public bool IsConnected() { return Connected; }
        public string GetHostName() { return Host; }
    }
    internal sealed class ZSteamSocket : FakeSocket { }
    internal sealed class ZPlayFabSocket : FakeSocket { }
    internal sealed class FakePeer
    {
        public long m_uid = 1;
        public string m_playerName = "Raven";
        public FakeSocket m_socket;
        public bool IsReady() { return m_uid != 0; }
    }
    internal sealed class FakePlatformUserID
    {
        private readonly string value;
        public FakePlatformUserID(string platform, string user) { value = platform + "_" + user; }
        public FakePlatformUserID(string raw) { value = raw; }
        public bool IsValid { get { return value != null && value.IndexOf('_') > 0 && !value.EndsWith("_"); } }
        public override string ToString() { return value; }
    }
    internal sealed class FakeSyncedList
    {
        private readonly FakeLocation m_fileLocation = new FakeLocation(FakeAccessFiles.NewFile());
        private DateTime m_lastLoadDate = DateTime.UtcNow;
        private List<string> m_list = new List<string>();
        internal List<string> Next = new List<string>();
        internal int Loads, Reads;
        public List<string> GetList() { Reads++; return m_list; }
        public bool Contains(string value) { return m_list.Contains(value); }
        internal void SeedCache(params string[] values) { m_list = new List<string>(values); }
        private void Load()
        {
            ControlTests.Assert(m_lastLoadDate == DateTime.MinValue, "Native timestamp cache not invalidated");
            ControlTests.Assert(File.Exists(m_fileLocation.m_path), "Native fixture file missing");
            m_list = new List<string>(Next);
            m_lastLoadDate = DateTime.UtcNow;
            Loads++;
        }
        internal string Path { get { return m_fileLocation.m_path; } }
    }
    internal enum FakeFileSource { Auto = 1, Local = 2, Cloud = 4, Legacy = 8 }
    internal struct FakeLocation
    {
        public readonly string m_path;
        public readonly FakeFileSource m_fileSource;
        internal FakeLocation(string path) { m_path = path; m_fileSource = FakeFileSource.Local; }
    }
    internal static class FakeAccessFiles
    {
        internal static string Root;
        internal static readonly List<string> Created = new List<string>();
        internal static string NewFile()
        {
            string path = System.IO.Path.Combine(Root, Guid.NewGuid().ToString("N") + ".txt");
            File.WriteAllText(path, "// synthetic native access list\n");
            Created.Add(path);
            return path;
        }
    }
    internal sealed class FakeZNet
    {
        private readonly FakeSyncedList m_adminList = new FakeSyncedList();
        private readonly FakeSyncedList m_bannedList = new FakeSyncedList();
        private readonly FakeSyncedList m_permittedList = new FakeSyncedList();
        internal readonly List<FakePeer> Peers = new List<FakePeer>();
        internal bool Server = true;
        internal int Sends, Kicks;
        internal FakePeer Kicked;
        public static FakeZNet instance { get; set; }
        public bool IsServer() { return Server; }
        public List<FakePeer> GetPeers() { return Peers; }
        public bool IsAdmin(string host) { return m_adminList.Contains("Steam_" + host); }
        private void InternalKick(FakePeer peer) { Kicks++; Kicked = peer; }
        private void InternalKick(string value) { throw new Exception("Ambiguous string overload invoked"); }
        private void SendAdminList() { Sends++; }
        internal FakeSyncedList[] Lists { get { return new[] { m_adminList, m_bannedList, m_permittedList }; } }
    }
    internal static class NativeControlTests
    {
        private static FakePeer Peer(string host)
        {
            return new FakePeer { m_socket = new ZSteamSocket { Host = host } };
        }
        private static ValheimNativeControl Native(params FakePeer[] peers)
        {
            FakeZNet.instance = new FakeZNet();
            FakeZNet.instance.Peers.AddRange(peers);
            return ValheimNativeControl.ForTests(typeof(FakeZNet), typeof(FakePeer), typeof(FakePlatformUserID));
        }
        internal static void Run(string output)
        {
            FakeAccessFiles.Root = System.IO.Path.Combine(output, "native_" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(FakeAccessFiles.Root);
            try { Cases(); }
            finally
            {
                foreach (string path in FakeAccessFiles.Created) if (File.Exists(path)) File.Delete(path);
                FakeAccessFiles.Created.Clear();
                Directory.Delete(FakeAccessFiles.Root);
            }
        }
        private static void Cases()
        {
            ControlTests.Test("native profiles reject mixed or unknown assembly fingerprints", delegate {
                ControlTests.Assert(ValheimNativeControl.SupportedProfile(ValheimNativeControl.GameSha256,
                    ValheimNativeControl.UtilitiesSha256, ValheimNativeControl.PlatformSha256), "Current profile rejected");
                ControlTests.Assert(ValheimNativeControl.SupportedProfile(ValheimNativeControl.LocalProofGameSha256,
                    ValheimNativeControl.LocalProofUtilitiesSha256, ValheimNativeControl.PlatformSha256), "Fixture profile rejected");
                ControlTests.Assert(!ValheimNativeControl.SupportedProfile(ValheimNativeControl.GameSha256,
                    ValheimNativeControl.LocalProofUtilitiesSha256, ValheimNativeControl.PlatformSha256), "Mixed native profile accepted");
                ControlTests.Assert(!ValheimNativeControl.SupportedProfile(ValheimNativeControl.GameSha256,
                    ValheimNativeControl.UtilitiesSha256, new string('0', 64)), "Unknown platform ABI accepted");
                ControlTests.Assert(!ValheimNativeControl.SupportedProfile(new string('f', 64),
                    ValheimNativeControl.UtilitiesSha256, ValheimNativeControl.PlatformSha256), "Unknown game ABI accepted");
            });
            ControlTests.Test("same-object session and current native platform identity", delegate {
                FakePeer peer = Peer("76561198000000001");
                ValheimNativeControl native = Native(peer);
                ControlPlayer before = native.Query().Players[0];
                peer.m_playerName = "Raven Storm";
                ControlPlayer after = native.Query().Players[0];
                ControlTests.Assert(before.SessionKey == after.SessionKey && ControlProtocol.IsNonce(before.SessionKey),
                    "Same native peer lost its session identity");
                ControlTests.Assert(after.Name == "Raven Storm" && after.HostId == "76561198000000001" &&
                    after.PlatformId == "Steam_76561198000000001", "Identity not read from current peer socket");
                FakePeer cross = new FakePeer { m_socket = new ZPlayFabSocket { Host = "Xbox_synthetic-player" } };
                FakeZNet.instance.Peers.Add(cross);
                ControlPlayer[] rows = native.Query().Players;
                ControlTests.Assert(rows[1].PlatformId == "Xbox_synthetic-player" &&
                    rows[1].HostId == rows[1].PlatformId, "PlayFab platform was guessed or prefixed");
                cross.m_socket.Host = "Steam_76561198000000003";
                rows = native.Query().Players;
                ControlTests.Assert(rows[1].HostId == "Steam_76561198000000003" &&
                    rows[1].PlatformId == rows[1].HostId, "Crossplay Steam identity was prefixed or guessed");
            });
            ControlTests.Test("uid host disconnect and server changes expire sessions", delegate {
                FakePeer peer = Peer("76561198000000001");
                ValheimNativeControl native = Native(peer);
                string previous = native.Query().Players[0].SessionKey;
                peer.m_uid++;
                string changed = native.Query().Players[0].SessionKey;
                ControlTests.Assert(changed != previous, "Changed uid reused session");
                previous = changed;
                peer.m_socket.Host = "76561198000000002";
                changed = native.Query().Players[0].SessionKey;
                ControlTests.Assert(changed != previous, "Changed socket host reused session");
                previous = changed;
                peer.m_socket.Connected = false;
                ControlTests.Assert(native.Query().Players.Length == 0, "Disconnected peer exposed");
                peer.m_socket.Connected = true;
                changed = native.Query().Players[0].SessionKey;
                ControlTests.Assert(changed != previous, "Disconnected then rejoined peer reused session");
                previous = changed;
                FakeZNet.instance = new FakeZNet();
                FakeZNet.instance.Peers.Add(peer);
                ControlTests.Assert(native.Query().Players[0].SessionKey != previous, "New server reused old session");
                FakeZNet.instance.Server = false;
                ControlTests.Fails("server_not_ready", delegate { native.Query(); });
            });
            ControlTests.Test("kick requires unique host and exact current session", delegate {
                FakePeer peer = Peer("76561198000000001");
                ValheimNativeControl native = Native(peer);
                string session = native.Query().Players[0].SessionKey;
                ControlTests.Fails("stale_or_ambiguous_target", delegate {
                    native.Kick(Guid.NewGuid().ToString("N"), "76561198000000001");
                });
                ControlTests.Fails("stale_or_ambiguous_target", delegate { native.Kick(session, "76561198000000002"); });
                FakeZNet.instance.Peers.Add(Peer("76561198000000001"));
                ControlTests.Fails("stale_or_ambiguous_target", delegate { native.Kick(session, "76561198000000001"); });
                FakeZNet.instance.Peers.RemoveAt(1);
                ControlTests.Assert(FakeZNet.instance.Kicks == 0, "Rejected target invoked native kick");
                native.Kick(session, "76561198000000001");
                ControlTests.Assert(FakeZNet.instance.Kicks == 1 && Object.ReferenceEquals(FakeZNet.instance.Kicked, peer),
                    "Exact native peer overload not used");
            });
            ControlTests.Test("reload updates admin and permitted while preserving native ban cache", delegate {
                ValheimNativeControl native = Native(Peer("76561198000000001"));
                FakeSyncedList[] lists = FakeZNet.instance.Lists;
                lists[0].Next.Add("Steam_76561198000000001");
                lists[1].SeedCache("Raven Existing");
                lists[1].Next.Add("Raven Pending");
                lists[2].Next.Add("Xbox_synthetic-player");
                ControlTests.Assert(!native.Query().Players[0].Admin, "Admin inferred before native reload");
                ControlSnapshot result = native.Reload();
                ControlTests.Assert(result.Players[0].Admin && result.Access.Admin[0] == "Steam_76561198000000001" &&
                    result.Access.Banned[0] == "Raven Existing" && result.Access.Permitted[0] == "Xbox_synthetic-player",
                    "Reload returned guessed instead of native authority");
                ControlTests.Assert(lists[0].Loads == 1 && lists[1].Loads == 0 && lists[1].Reads == 0 && lists[2].Loads == 1 &&
                    FakeZNet.instance.Sends == 1, "Reload actively changed bans or omitted admin broadcast");
            });
            ControlTests.Test("unready peers and native collection limits", delegate {
                FakePeer notReady = Peer("76561198000000001");
                notReady.m_uid = 0;
                ValheimNativeControl native = Native(notReady);
                ControlTests.Assert(native.Query().Players.Length == 0, "Unready peer exposed");
                for (int i = 0; i < 255; i++) FakeZNet.instance.Peers.Add(notReady);
                ControlTests.Fails("native_player_limit", delegate { native.Query(); });
                native = Native();
                for (int i = 0; i < 4097; i++) FakeZNet.instance.Lists[0].Next.Add("synthetic-" + i);
                ControlTests.Fails("native_access_limit", delegate { native.Reload(); });
            });
            ControlTests.Test("native access facts preserve historical text without write normalization", delegate {
                ValheimNativeControl native = Native();
                string historical = "Raven " + new string('x', 300);
                FakeZNet.instance.Lists[1].SeedCache(" ", historical);
                string[] entries = native.Query().Access.Banned;
                ControlTests.Assert(entries[0] == " " && entries[1] == historical,
                    "Historical native facts were changed by the write codec");
            });
            ControlTests.Test("native access read admission precedes reload and kick", delegate {
                ValheimNativeControl native = Native(Peer("76561198000000001"));
                string session = native.Query().Players[0].SessionKey;
                string path = FakeZNet.instance.Lists[1].Path;
                using (FileStream stream = new FileStream(path, FileMode.Open, FileAccess.Write))
                    stream.SetLength(NativeAccessPins.MaximumFileBytes + 1);
                ControlTests.Fails("native_access_file_limit", delegate { native.Query(); });
                ControlTests.Fails("native_access_file_limit", delegate { native.Reload(); });
                ControlTests.Fails("native_access_file_limit", delegate { native.Kick(session, "76561198000000001"); });
                ControlTests.Assert(FakeZNet.instance.Kicks == 0 && FakeZNet.instance.Lists[0].Loads == 0,
                    "Oversized access file reached native execution");
                File.Delete(path);
                ControlTests.Fails("unsafe_control_path", delegate { native.Reload(); });
            });
            ControlTests.Test("native access pins block writes and replacement", delegate {
                ValheimNativeControl native = Native();
                ControlTests.Assert(native.Query().Players.Length == 0, "Empty native roster invalid");
                string[] paths = new[] { FakeZNet.instance.Lists[0].Path, FakeZNet.instance.Lists[1].Path,
                    FakeZNet.instance.Lists[2].Path };
                using (new NativeAccessPins(paths))
                {
                    bool writeBlocked = false, moveBlocked = false;
                    try { File.WriteAllText(paths[0], "changed"); } catch (IOException) { writeBlocked = true; }
                    try { File.Move(paths[1], paths[1] + ".moved"); } catch (IOException) { moveBlocked = true; }
                    ControlTests.Assert(writeBlocked && moveBlocked, "Native file pin allowed write or replacement");
                    ControlTests.Assert(File.ReadAllText(paths[0]).StartsWith("// synthetic"), "Read pin blocked native parser");
                }
            });
        }
    }
}
