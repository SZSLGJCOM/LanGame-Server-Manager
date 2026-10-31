# Valheim native control

This Windows server plugin runs a closed local file protocol on Unity's main
thread. It requires the reviewed BepInEx loader. Loading the plugin changes the
server runtime; startup automatically prepares the loader and control before
creating the game process. It is not a vanilla server
console or RCON endpoint. It adds no network RPC or client plugin requirement.
Client compatibility must still be verified with the target server build.

## Enabling and updates

Starting the instance prepares an independent server program and automatically
installs/configures the reviewed loader when it is missing. Complete compatible
installations are reused without a download. The optional stopped-instance
account-management preparation performs the same setup ahead of startup. On
normal startup, the manager holds the existing instance and program locks,
verifies the installed loader and known native assembly profile, then installs
or refreshes the owned management DLL to the current bundled payload.
The installed DLL hash must match its ownership record's previous payload or a
recognized pending update; recognized interrupted updates can be recovered.
Updating LanGame therefore does not require manually enabling account management
again. Setup does not write world rules or saves. Externally changed
and unknown files are preserved and reported instead of being overwritten.

A remote loader download failure leaves native startup available only when the
loader is absent or a complete owned installation has Doorstop explicitly
disabled. The owned management/save plugin is then retired. Unknown ownership,
incomplete injection, path conflicts and failed cleanup still block startup.

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

Native reflection is limited to explicitly checked members and three
complete three-assembly SHA-256 profiles:

- Build 26266951 (1.0.21): `648f3f8ed7b44c3d3cd612acd782743d9e03af7e60b7fe72d5827523e95505fd`
- Build 25909431: `ef8c8d3a1f45694ef7a701e8e2a25bf8a3874c951a2639c81dfe614ce3aa5fa3`
- Local integration fixture build 25730807: `0dfc7e81436f822121148efed859ba1e661d58b9484bd45e3b6f33b6dd86bfad`

Build 26266951 requires `assembly_utils.dll`
`9658619e34e456b88639f6cc6e7d0fd186432f63bf1033e3e04e98e8fac2be9e`
and `Splatform.dll`
`1924e4c348616fbd754a8f688d3dc65cde5a741cfef57e80474fcdd0121013b5`.
For the two earlier builds, the corresponding `assembly_utils.dll` hashes are
`0d66bd029bf753e2721304c775126b34af329bfcd6e632d8d38edaa96b5cf1a5` (25909431) and
`63e114e7cfdcc38e4f4391c18b91f774290274d2b9200e6d85d757cbe3673e18` (25730807).
Both require `Splatform.dll`
`8953c24a2f2af7ae257cc64fb36dde9d102c871c29b4cdd97d0c3c001de5bd5a`.
Mixed or unknown tuples are rejected.

All three profiles were inspected statically. The fixed contract uses public
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

Build 25909431 was also verified on Windows through the normal desktop BAT
entry with an unmodified Steam client. One connected native peer retained the
same authoritative platform identity and session throughout six live admin and
permitted-list changes. Native lists and the online admin flag confirmed each
addition and removal; all six rendered/native roster files were restored to
their original hashes. Existing settings and version-42 saved rules were
preserved. Normal startup also upgraded the previously enabled owned plugin
without invoking account-management preparation again. A normal client departure
returned a complete zero-player native snapshot with no remaining player entries.

On 2026-10-30, official SteamCMD validation installed build 26266951 (1.0.21,
network 41). All three hashes changed, including `Splatform.dll`; admission uses
complete tuples so an old/new assembly mixture is still rejected. Against build
25909431, full `ZNet`, `World`, `SyncedList`, peer/socket, `PlatformUserID`, `Game`,
`FileWriter` and `ZDOMan` method/field/property captures are identical, including
IL, exception handlers and local types. `SaveSystem.EndSave` and its committed
counter retain the reviewed semantics. File-list/cache helpers changed separately.

Before changing either production admission gate, an independent probe in an
isolated official package invoked the reviewed native disk check and asynchronous
save API. It observed one save start, the original worker completion and committed
generation 0 to 1. The five-file world snapshot was captured and hash-checked
before shutdown, then restored to a fresh run. The native loader read the same
world UID and generation 1 with world format 42. Both phases exited normally
through the engine shutdown path with code zero; no forced cleanup was used.
The fixed BepInExPack 5.4.2351 remained unchanged and all twenty immutable loader
files were verified. This new zero-player world does not establish live client,
crossplay, public A2S or existing-player world compatibility. The retained evidence
is [the native build fixture](../reference-fixtures/2026-10-30-native_build_26266951.json).

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
Operations are `query`, `reload`, `kick`, and `save`; only kick accepts the required
`session_key` and `host_id`. Unknown/duplicate fields, trailing JSON, invalid
targets and unauthorized requests cannot reach native operations.

`response.json` is atomically published with the same nonce, process id and
operation, status `ok` or `error`, optional nonempty error code, `players`,
`access`, and fixed capabilities `query/reload/kick/save`. Players carry
`session_key/name/host_id/platform_id/admin`; access contains
`admin/banned/permitted` arrays. Host ids are private execution bindings and must
not be exposed as guessed account identifiers. Responses are at most 64 KiB;
players are bounded to 255 and access lists to 4096 entries of at most 1024
Unicode characters each. These are native read facts, preserved without applying
future-write normalization. The total response limit remains 64 KiB.

Polling is at most five times per second with one pending mutation. The last 64
completed nonces cache their exact request byte fingerprint and result; a changed
payload reusing a nonce is rejected. The caller must preserve its original nonce
when recovering an uncertain operation within that window. Publication retries
last at most five seconds while native operation completion continues to advance.
Permission errors or an unsafe response path deactivate only the controller and
emit one generic diagnostic, leaving the server running.
Atomic replacement of an otherwise identical request is detected from its file
identity/write time and republishes the cached result without native reexecution.

The BCL harness uses synthetic peers, access files and gated save workers. It covers malformed
and unauthorized requests, nonce replay/conflicts, pending kick success/timeout,
current peer identity, exact overload/host binding, native reload authority,
file/response limits, read pins, and directory-junction rejection.

## Immediate world save

`save` has no target, path or command argument. It requires the extension prepared
automatically by startup; submitting it does not install a loader or alter the instance
program. The owner verifies a dedicated, enabled server, an active world, native
ZoneSystem/DungeonDB save readiness, an existing native `Game.instance`, no native
load error and no active save. It calls the same native disk-space check used by
autosaving: `EnoughDiskSpaceAvailable(out exitGamePopupShown, false, null)`.
A false result or an unexpected exit-prompt result rejects the operation before
native saving starts. It then calls `ZNet.Save(false, true, false)` on Unity's
owner thread: `sync=false` keeps the save asynchronous,
`saveOtherPlayerProfiles=true` notifies connected clients to save their profiles,
and `waitForNextFrame=false` creates the worker before the call returns.
The native `Save` retains its session, load-error, zone and dungeon guards.
The asynchronous
native save worker is polled with zero-time `Join(0)`; the controller never waits
for that thread on Unity's owner.

The client-facing `SaveWorldAndPlayerProfiles` / `RPC_Save` entry is unsuitable
for the dedicated process. Its `HardSaveBlock` implements the Nintendo client save
interval by dereferencing `PlatformManager.DistributionPlatform` during the
first 60 seconds. In the isolated Windows dedicated build 25730807, that call
raised a native `NullReferenceException` after the server had opened its Steam
endpoint. The server/autosave path above preserves the real disk and save-state
checks without invoking client-platform UI or manufacturing a native singleton.
All three complete pinned tuples are admitted for this dedicated save route. Read-only
IL comparison on 2026-10-21 confirmed identical disk checks, save dispatch, worker
completion and committed-generation logic, including exception handlers and local
types. The native world worker's only instruction changes in build 25909431 write
world format 42 instead of 41; the game owns that serialization. Save admission
remains separate from player-control admission so another build requires its own
save-path review. Reflection binding and static comparison do not replace a real
dedicated-server save test.

Read-only IL inspection of builds 25730807, 25909431 and 26266951 confirms that
`SaveSystem.EndSave(source, success)` updates private `s_saveNumber` only when
`success` is true, after the chunk/DB2/FWL/OK native commit path. In contrast,
`GetSaveNumber()` returns the proposed generation while saving and cannot confirm
success. The controller binds the request to the actual `m_saveThread`, the
current world and exactly one `WorldSaveStarted` observation, then checks worker
completion and a committed generation increment of exactly one. Another save,
world/server replacement or removed observer makes the outcome unknown.
`WorldSaveFinished`, a stopped thread or a log message alone never proves success.

Only a confirmed save returns `status: "ok"` with the additional closed field
`"world_save":{"previous_save_number":7,"committed_save_number":8,"evidence":"native_commit"}`.
The receipt contains no file path or world name. The Rust consumer verifies the
generation transition, evidence, nonce, process identity and operation. Failed
native commits return `save_failed`; a request that does not start returns an
explicit rejection. Observation loss or the 120-second native observation limit
returns `save_outcome_unknown`, with no receipt. The Rust worker waits at most
125 seconds, never republishes the mutation, and retains the instance mutation
guard and storage-context lease even when its awaiting caller is cancelled.
Unknown outcomes must be checked before another manual attempt.

Unknown native failures include bounded exception type/method metadata in the
existing error field and local plugin log. No exception message, file path,
request content or token is included. A failed logging target cannot replace
the original failure or cause the request to execute again.

The synthetic harness covers successful/failed workers, native disk-space
refusal, missing game readiness, the absent dedicated client-platform dependency,
rejection/readiness,
unrelated saves, world/observer replacement, counter wrap, authentication,
serialization, nonce replay and bounded unknown outcomes. The two earlier native
profiles also passed a binding-only reflection probe. The 31 boundary tests pass
when the plugin is built against the actual 26266951 and reviewed loader/Unity
references. Synthetic and binding-only checks do not replace the independent real
save/reload evidence above, client integration or live filesystem-failure acceptance.

The [2026-10-31 managed backend acceptance](../reference-fixtures/2026-10-31-managed_native_lifecycle_build_26266951.json)
completed four native lifecycle cycles with the adapted manager and normal exits 0.
The first confirmed manual save produced a five-file checkpoint before shutdown;
the next process loaded that exact world/generation without a second save. Backup
restore, interrupted archive recovery and library-only uninstall preserved the
isolated instance. Two later restored-world checks each preceded a distinct
confirmed save. These results cover the disposable world, not populated old-world
migration, multiplayer, crossplay or power-loss durability.

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
