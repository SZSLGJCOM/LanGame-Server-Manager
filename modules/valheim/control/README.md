# Valheim native control

This Windows server plugin runs a closed local file protocol on Unity's main
thread. It requires the reviewed BepInEx loader. Loading the plugin changes the
server runtime and must be enabled explicitly; it is not a vanilla server
console or RCON endpoint. It adds no network RPC or client plugin requirement.
Client compatibility must still be verified with the target server build.

## Build

`scripts/build_valheim_control.ps1 -OutDir <absolute-build-directory> -RunTests`
uses the Windows .NET Framework compiler and BCL only. Its minimal public ABI
declarations generate **compiler references** under `reference/`. Deploy only
`LanGame.Valheim.Control.dll`; never deploy either reference DLL or the test
executable. No game DLL is vendored. For ABI verification, the helper optionally
accepts all four explicit real references: `-BepInExAssembly`,
`-UnityCoreAssembly`, `-UnityEngineAssembly`, and `-NetStandardAssembly`. A managed
Cargo `OUT_DIR` is supported. Output must remain outside the repository, on a
local volume, with no reparse ancestors; volume roots are rejected.

The checked ABI is BepInEx `5.4.23.5` and UnityEngine.CoreModule `0.0.0.0`, both
with no public key. Only six external members are emitted:

| Type | Member | Signature |
| --- | --- | --- |
| BepInEx.BepInPlugin | public constructor | `(string, string, string)` |
| BepInEx.BaseUnityPlugin | protected constructor | `()` |
| BepInEx.BaseUnityPlugin | protected Logger getter | `ManualLogSource` |
| BepInEx.Logging.ManualLogSource | public LogInfo | `void(object)` |
| BepInEx.Logging.ManualLogSource | public LogError | `void(object)` |
| UnityEngine.Time | public static realtimeSinceStartup getter | `float` |

Both the real-reference and compiler-reference outputs were compared for all
emitted type references and these member signatures. This verifies compilation
ABI; it does not replace an isolated server/client integration test.

## Native contract

Native reflection is limited to explicitly checked members and two
three-assembly SHA-256 profiles:

- Build 25909431: `ef8c8d3a1f45694ef7a701e8e2a25bf8a3874c951a2639c81dfe614ce3aa5fa3`
- Local integration fixture build 25730807: `0dfc7e81436f822121148efed859ba1e661d58b9484bd45e3b6f33b6dd86bfad`

The corresponding `assembly_utils.dll` hashes are
`0d66bd029bf753e2721304c775126b34af329bfcd6e632d8d38edaa96b5cf1a5` (25909431) and
`63e114e7cfdcc38e4f4391c18b91f774290274d2b9200e6d85d757cbe3673e18` (25730807).
Both require `Splatform.dll`
`8953c24a2f2af7ae257cc64fb36dde9d102c871c29b4cdd97d0c3c001de5bd5a`.
Mixed or unknown tuples are rejected.

Both profiles were inspected statically. The fixed contract uses public
`ZNet.instance`, `IsServer()`, `GetPeers()`, `ZNetPeer.IsReady()`, peer uid/name/socket,
`ISocket.IsConnected()/GetHostName()`, `ZNet.IsAdmin(string)`, private
`InternalKick(ZNetPeer)` and `SendAdminList()`, and the three private `SyncedList`
fields with private `Load()` and public `GetList()`.

On 2026-10-16, the isolated Windows build 25730807 (Valheim 1.0.17, network 40)
loaded the actual plugin under the reviewed loader and returned a correlated
zero-player query. Synthetic complete platform IDs were added to admin/permitted,
read back through native reload, then removed and confirmed empty. Ctrl+C produced
native shutdown and exit code zero. All twenty immutable loader files and the
three native assembly hashes remained unchanged. This smoke does not verify an
online client's identity/admin flag, client compatibility, or build 25909431.

For direct Steam sockets, the native bare host is passed to
`Splatform.PlatformUserID("Steam", host)`. PlayFab sockets already return a full
native platform identity and use `PlatformUserID(host)`, including cross-platform
Steam identities. No platform is inferred from a character name or nearby log.
Session keys belong to current peer objects and expire on uid/host changes,
disconnects, or server replacement. Kick requires both a current session and a
unique exact host, then waits up to ten seconds for that peer to disappear.

Before controller-initiated `GetList`, `IsAdmin`, `Load`, or kick, all three native
access files must be local regular files of at most 512 KiB. Their verified
`m_fileLocation.m_path` and `m_fileSource` fields select the files; request data
cannot select paths. File and ancestor handles reject reparse points and remain
pinned against writes/replacement through the native operation. This admission
does not constrain reads independently initiated by the game or other plugins.
Reload resets only the verified admin/permitted timestamp caches, calls those
two native parsers and the admin broadcast, and returns the resulting authority;
it never calls Save or actively reloads banned entries. The banned response array
copies the already loaded private native cache because `GetList` would itself
trigger `CheckLoad`. The game's own `UpdateBanList` runs every five seconds and
native `CheckLoad` checks at ten-second intervals. A stored ban can therefore
apply through normal native checks; restart is a guaranteed application point,
not a strict promise that the native game defers changes until restart.
The caller must compare returned lists with its requested persistent state.

The manager's running configuration protection remains in force. Ordinary
configuration materialization does not refresh native files while a server is
running. Account actions use `materialize_valheim_player_roster` instead: under
the instance settings lease, it publishes only the selected admin or permitted
roster to its rendered configuration file and the active save root, verifies
both copies, and rolls back a failed publication. It does not rewrite the ban
roster, settings document, or world data. Native reload and privilege readback
follow publication; a saved change alone is never reported as live success.

## Protocol and bounds

Without `LGSM_VALHEIM_CONTROL_ROOT`, the controller remains inactive. The existing
root must be an absolute local plain directory with no reparse ancestors. The
manager writes `session.json` before launch: exactly
`{"protocol":1,"token":"<redacted>"}`, at most 4 KiB. The redacted value must be
a freshly generated 32-character lowercase hexadecimal nonce.
The plugin always reads it through the pinned root. A legacy
`LGSM_VALHEIM_CONTROL_TOKEN` environment value is optional and, if present, must
match that file. The launch plan needs only the root environment variable. The
manager supplies a fresh file token for each run; the plugin never returns or
logs it.

`request.json` is one UTF-8 object of at most 16 KiB with required `protocol:1`,
`nonce` (32 lowercase hexadecimal GUID characters), `token`, and `op`.
Operations are `query`, `reload`, and `kick`; only kick accepts the required
`session_key` and `host_id`. Unknown/duplicate fields, trailing JSON, invalid
targets and unauthorized requests cannot reach native operations.

`response.json` is atomically published with the same nonce, process id and
operation, status `ok` or `error`, optional nonempty error code, `players`,
`access`, and fixed capabilities `query/reload/kick`. Players carry
`session_key/name/host_id/platform_id/admin`; access contains
`admin/banned/permitted` arrays. Host ids are private execution bindings and must
not be exposed as guessed account identifiers. Responses are at most 64 KiB;
players are bounded to 255 and access lists to 4096 entries of at most 1024
Unicode characters each. These are native read facts, preserved without applying
future-write normalization. The total response limit remains 64 KiB.

Polling is at most five times per second with one pending kick. The last 64
completed nonces cache their exact request byte fingerprint and result; a changed
payload reusing a nonce is rejected. The caller must preserve its original nonce
when recovering an uncertain operation within that window. Publication retries
last at most five seconds while native kick completion continues to advance.
Permission errors or an unsafe response path deactivate only the controller and
emit one generic diagnostic, leaving the server running.
Atomic replacement of an otherwise identical request is detected from its file
identity/write time and republishes the cached result without native reexecution.

The BCL harness uses synthetic peers and access files only. It covers malformed
and unauthorized requests, nonce replay/conflicts, pending kick success/timeout,
current peer identity, exact overload/host binding, native reload authority,
file/response limits, read pins, and directory-junction rejection.

## Isolated smoke test

`Invoke-SmokeTest.ps1` never starts a game, installs a loader, or writes access
list contents. `-Mode Prepare -FixtureRoot <private-work/vh>` verifies the old
25730807 tuple and creates a fresh isolated IPC root, session file, save directory
and manifest. It outputs only the manifest path. The caller installs the reviewed
loader and plugin separately, then starts that fixture executable with the
manifest's `control_root` as `LGSM_VALHEIM_CONTROL_ROOT` and `savedir` as `-savedir`.

After startup, use `-Mode Query` or `-Mode VerifyReload` with the same `-FixtureRoot`,
`-SessionManifest` and actual `-ExpectedPid`. The helper verifies the process path
and run start time, sends the closed request, and checks nonce/PID/op within ten
seconds. VerifyReload also compares the admin and permitted arrays exactly with the
persisted native lines, preserving case and excluding only empty and `//` lines.
Any synthetic list edits and process lifecycle remain under the caller's control.
An old-build smoke test does not establish newest-build or vanilla-client success.
