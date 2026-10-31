using System;
using System.Collections;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Security.Cryptography;

namespace Lgsm.Valheim.Control
{
    // All methods are called by the Unity owner, never a worker or file callback.
    // No request can select a reflection target, file path or arbitrary command.
    internal sealed class ValheimNativeControl : IValheimControl, IValheimWorldSave, IDisposable
    {
        internal const string GameSha256 = "ef8c8d3a1f45694ef7a701e8e2a25bf8a3874c951a2639c81dfe614ce3aa5fa3";
        internal const string LocalProofGameSha256 = "0dfc7e81436f822121148efed859ba1e661d58b9484bd45e3b6f33b6dd86bfad";
        internal const string UtilitiesSha256 = "0d66bd029bf753e2721304c775126b34af329bfcd6e632d8d38edaa96b5cf1a5";
        internal const string LocalProofUtilitiesSha256 = "63e114e7cfdcc38e4f4391c18b91f774290274d2b9200e6d85d757cbe3673e18";
        internal const string PlatformSha256 = "8953c24a2f2af7ae257cc64fb36dde9d102c871c29b4cdd97d0c3c001de5bd5a";
        internal const string Build26266951GameSha256 = "648f3f8ed7b44c3d3cd612acd782743d9e03af7e60b7fe72d5827523e95505fd";
        internal const string Build26266951UtilitiesSha256 = "9658619e34e456b88639f6cc6e7d0fd186432f63bf1033e3e04e98e8fac2be9e";
        internal const string Build26266951PlatformSha256 = "1924e4c348616fbd754a8f688d3dc65cde5a741cfef57e80474fcdd0121013b5";
        private const BindingFlags PublicInstance = BindingFlags.Public | BindingFlags.Instance;
        private const BindingFlags PrivateInstance = BindingFlags.NonPublic | BindingFlags.Instance;
        private sealed class ReferenceIdentity : IEqualityComparer<object>
        {
            public new bool Equals(object left, object right) { return Object.ReferenceEquals(left, right); }
            public int GetHashCode(object value) { return RuntimeHelpers.GetHashCode(value); }
        }
        private sealed class Session
        {
            internal long Uid;
            internal string Host, Key;
        }
        private readonly Dictionary<object, Session> sessions = new Dictionary<object, Session>(new ReferenceIdentity());
        private PropertyInfo instance;
        private MethodInfo isServer, getPeers, isReady, connected, getHost, isAdmin, kick, sendAdmin;
        private MethodInfo load, getList;
        private FieldInfo playerName, uid, socket, lastLoadDate;
        private FieldInfo fileLocation, filePath, fileSource, cachedList;
        private FieldInfo[] access;
        private ConstructorInfo steamId, platformId;
        private PropertyInfo validPlatformId;
        private object currentServer;
        private ValheimWorldSave worldSave;
        private bool worldSaveProfileSupported;

        private static MethodInfo Method(Type type, string name, BindingFlags flags, Type result, params Type[] arguments)
        {
            MethodInfo method = type.GetMethod(name, flags, null, arguments, null);
            if (Object.ReferenceEquals(method, null) || method.ReturnType != result)
                throw new ControlFailure("unsupported_native_contract");
            return method;
        }
        private static FieldInfo Field(Type type, string name, BindingFlags flags, Type expected)
        {
            FieldInfo field = type.GetField(name, flags);
            if (Object.ReferenceEquals(field, null) || (expected != null && field.FieldType != expected))
                throw new ControlFailure("unsupported_native_contract");
            return field;
        }
        private static Assembly Loaded(string name)
        {
            foreach (Assembly assembly in AppDomain.CurrentDomain.GetAssemblies())
                if (assembly.GetName().Name == name) return assembly;
            throw new ControlFailure("native_not_loaded");
        }

        internal static bool SupportedProfile(string game, string utilities, string platform)
        {
            return (platform == PlatformSha256 && ((game == GameSha256 && utilities == UtilitiesSha256) ||
                (game == LocalProofGameSha256 && utilities == LocalProofUtilitiesSha256))) ||
                (game == Build26266951GameSha256 && utilities == Build26266951UtilitiesSha256 &&
                    platform == Build26266951PlatformSha256);
        }

        internal static bool SupportedWorldSaveProfile(string game, string utilities, string platform)
        {
            // Save admission is reviewed separately from player control. Build
            // 26266951 retains the reviewed disk check/commit contract and passed
            // native saving plus pre-stop snapshot reload (world format 42).
            return (platform == PlatformSha256 && ((game == GameSha256 && utilities == UtilitiesSha256) ||
                (game == LocalProofGameSha256 && utilities == LocalProofUtilitiesSha256))) ||
                (game == Build26266951GameSha256 && utilities == Build26266951UtilitiesSha256 &&
                    platform == Build26266951PlatformSha256);
        }

        private static string Digest(Assembly assembly)
        {
            using (FileStream stream = File.OpenRead(assembly.Location))
            using (SHA256 hash = SHA256.Create())
            {
                if (stream.Length > 64 * 1024 * 1024) throw new ControlFailure("unsupported_game_build");
                return BitConverter.ToString(hash.ComputeHash(stream)).Replace("-", "").ToLowerInvariant();
            }
        }

        private void Bind()
        {
            if (instance != null) return;
            Assembly game = Loaded("assembly_valheim");
            Assembly platform = Loaded("Splatform");
            string gameHash = Digest(game), utilitiesHash = Digest(Loaded("assembly_utils")), platformHash = Digest(platform);
            if (!SupportedProfile(gameHash, utilitiesHash, platformHash))
                throw new ControlFailure("unsupported_game_build");
            worldSaveProfileSupported = SupportedWorldSaveProfile(gameHash, utilitiesHash, platformHash);
            BindTypes(game.GetType("ZNet", true), game.GetType("ZNetPeer", true),
                platform.GetType("Splatform.PlatformUserID", true));
        }

        private void BindTypes(Type znet, Type peer, Type platform)
        {
            PropertyInfo singleton = znet.GetProperty("instance", BindingFlags.Public | BindingFlags.Static);
            if (singleton == null || singleton.PropertyType != znet)
                throw new ControlFailure("unsupported_native_contract");
            isServer = Method(znet, "IsServer", PublicInstance, typeof(bool));
            getPeers = Method(znet, "GetPeers", PublicInstance, typeof(List<>).MakeGenericType(peer));
            isReady = Method(peer, "IsReady", PublicInstance, typeof(bool));
            playerName = Field(peer, "m_playerName", PublicInstance, typeof(string));
            uid = Field(peer, "m_uid", PublicInstance, typeof(long));
            socket = Field(peer, "m_socket", PublicInstance, null);
            connected = Method(socket.FieldType, "IsConnected", PublicInstance, typeof(bool));
            getHost = Method(socket.FieldType, "GetHostName", PublicInstance, typeof(string));
            isAdmin = Method(znet, "IsAdmin", PublicInstance, typeof(bool), typeof(string));
            kick = Method(znet, "InternalKick", PrivateInstance, typeof(void), peer);
            sendAdmin = Method(znet, "SendAdminList", PrivateInstance, typeof(void));
            access = new[] {
                Field(znet, "m_adminList", PrivateInstance, null),
                Field(znet, "m_bannedList", PrivateInstance, null),
                Field(znet, "m_permittedList", PrivateInstance, null)
            };
            Type list = access[0].FieldType;
            if (access[1].FieldType != list || access[2].FieldType != list)
                throw new ControlFailure("unsupported_native_contract");
            load = Method(list, "Load", PrivateInstance, typeof(void));
            getList = Method(list, "GetList", PublicInstance, typeof(List<string>));
            cachedList = Field(list, "m_list", PrivateInstance, typeof(List<string>));
            lastLoadDate = Field(list, "m_lastLoadDate", PrivateInstance, typeof(DateTime));
            fileLocation = Field(list, "m_fileLocation", PrivateInstance, null);
            filePath = Field(fileLocation.FieldType, "m_path", PublicInstance, typeof(string));
            fileSource = Field(fileLocation.FieldType, "m_fileSource", PublicInstance, null);
            Type sourceType = fileSource.FieldType;
            if (!sourceType.IsEnum || Enum.GetUnderlyingType(sourceType) != typeof(int) ||
                Enum.GetName(sourceType, 2) != "Local") throw new ControlFailure("unsupported_native_contract");
            steamId = platform.GetConstructor(new[] { typeof(string), typeof(string) });
            platformId = platform.GetConstructor(new[] { typeof(string) });
            validPlatformId = platform.GetProperty("IsValid", PublicInstance);
            if (steamId == null || platformId == null || validPlatformId == null || validPlatformId.PropertyType != typeof(bool))
                throw new ControlFailure("unsupported_native_contract");
            instance = singleton;
        }

#if CONTROL_TESTS
        internal static ValheimNativeControl ForTests(Type znet, Type peer, Type platform)
        {
            ValheimNativeControl result = new ValheimNativeControl();
            result.BindTypes(znet, peer, platform);
            return result;
        }
#endif

        private object Server()
        {
            Bind();
            object server = instance.GetValue(null, null);
            if (server == null || !(bool)isServer.Invoke(server, null))
            {
                sessions.Clear();
                currentServer = null;
                throw new ControlFailure("server_not_ready");
            }
            if (!Object.ReferenceEquals(currentServer, server)) sessions.Clear();
            currentServer = server;
            return server;
        }

        private string PlatformId(object networkSocket, string host)
        {
            object id;
            if (networkSocket.GetType().Name == "ZSteamSocket")
                id = steamId.Invoke(new object[] { "Steam", host });
            else if (networkSocket.GetType().Name == "ZPlayFabSocket")
                id = platformId.Invoke(new object[] { host });
            else throw new ControlFailure("unsupported_socket");
            if (!(bool)validPlatformId.GetValue(id, null))
                throw new ControlFailure("invalid_native_identity");
            return id.ToString();
        }

        private List<object> Peers(object server)
        {
            List<object> result = new List<object>();
            int scanned = 0;
            foreach (object peer in (IEnumerable)getPeers.Invoke(server, null))
            {
                if (++scanned > 255) throw new ControlFailure("native_player_limit");
                object networkSocket = socket.GetValue(peer);
                if ((bool)isReady.Invoke(peer, null) && networkSocket != null &&
                    (bool)connected.Invoke(networkSocket, null)) result.Add(peer);
            }
            return result;
        }

        private static string[] CopyAccess(object values)
        {
            List<string> result = new List<string>();
            foreach (object entry in (IEnumerable)values)
            {
                string value = entry as string;
                if (result.Count >= 4096 || !ControlProtocol.BoundedText(value, 1024))
                    throw new ControlFailure("native_access_limit");
                result.Add(value);
            }
            return result.ToArray();
        }

        private ControlSnapshot Capture(object server)
        {
            List<ControlPlayer> players = new List<ControlPlayer>();
            HashSet<object> seen = new HashSet<object>(new ReferenceIdentity());
            foreach (object peer in Peers(server))
            {
                string name = (string)playerName.GetValue(peer);
                string host = (string)getHost.Invoke(socket.GetValue(peer), null);
                long peerUid = (long)uid.GetValue(peer);
                if (!ControlProtocol.SafeText(name, 256, true) || !ControlProtocol.SafeText(host, 256, false))
                    throw new ControlFailure("invalid_native_player");
                Session session;
                if (!sessions.TryGetValue(peer, out session) || session.Uid != peerUid || session.Host != host)
                {
                    session = new Session { Uid = peerUid, Host = host, Key = Guid.NewGuid().ToString("N") };
                    sessions[peer] = session;
                }
                seen.Add(peer);
                players.Add(new ControlPlayer {
                    SessionKey = session.Key, Name = name, HostId = host,
                    PlatformId = PlatformId(socket.GetValue(peer), host),
                    Admin = (bool)isAdmin.Invoke(server, new object[] { host })
                });
            }
            List<object> removed = new List<object>();
            foreach (object peer in sessions.Keys) if (!seen.Contains(peer)) removed.Add(peer);
            foreach (object peer in removed) sessions.Remove(peer);
            string[][] rosters = new string[3][];
            for (int i = 0; i < access.Length; i++)
            {
                object list = access[i].GetValue(server);
                // Bans are not actively reloaded by this controller. Native
                // GetList itself calls CheckLoad, so copy its loaded cache.
                rosters[i] = CopyAccess(i == 1 ? cachedList.GetValue(list) : getList.Invoke(list, null));
            }
            return new ControlSnapshot {
                Players = players.ToArray(),
                Access = new ControlAccess { Admin = rosters[0], Banned = rosters[1], Permitted = rosters[2] }
            };
        }

        private NativeAccessPins PinAccess(object server)
        {
            string[] paths = new string[access.Length];
            for (int i = 0; i < access.Length; i++)
            {
                object location = fileLocation.GetValue(access[i].GetValue(server));
                if (Convert.ToInt32(fileSource.GetValue(location)) != 2)
                    throw new ControlFailure("unsupported_native_access_source");
                paths[i] = (string)filePath.GetValue(location);
            }
            return new NativeAccessPins(paths);
        }

        public ControlSnapshot Query()
        {
            object server = Server();
            using (PinAccess(server)) return Capture(server);
        }

        public ControlSnapshot Reload()
        {
            object server = Server();
            using (PinAccess(server))
            {
                foreach (FieldInfo field in access)
                {
                    if (field == access[1]) continue;
                    object list = field.GetValue(server);
                    // Native Load skips files whose timestamp did not advance.
                    // Reset only its verified cache marker, then run the native
                    // parser; no native Save or caller-supplied path is used.
                    lastLoadDate.SetValue(list, DateTime.MinValue);
                    load.Invoke(list, null);
                }
                sendAdmin.Invoke(server, null);
                return Capture(server);
            }
        }

        public void Kick(string sessionKey, string hostId)
        {
            object server = Server();
            using (PinAccess(server))
            {
                Capture(server);
                object target = null;
                int hosts = 0;
                foreach (object peer in Peers(server))
                {
                    string host = (string)getHost.Invoke(socket.GetValue(peer), null);
                    if (host != hostId) continue;
                    hosts++;
                    Session session;
                    if (sessions.TryGetValue(peer, out session) && session.Key == sessionKey) target = peer;
                }
                if (hosts != 1 || target == null) throw new ControlFailure("stale_or_ambiguous_target");
                kick.Invoke(server, new[] { target });
            }
        }

        public void BeginSave()
        {
            object server = Server();
            if (!worldSaveProfileSupported) throw new ControlFailure("unsupported_native_save_contract");
            if (worldSave == null)
            {
                Assembly game = instance.PropertyType.Assembly;
                worldSave = new ValheimWorldSave(instance.PropertyType, game.GetType("SaveSystem", true),
                    game.GetType("ZoneSystem", true), game.GetType("DungeonDB", true), game.GetType("Game", true));
            }
            worldSave.Begin(server);
        }

        public WorldSaveReceipt PollSave() { return worldSave.Poll(Server()); }
        public void EndSaveObservation() { if (worldSave != null) worldSave.Dispose(); }
        public void Dispose() { EndSaveObservation(); }
    }
}
