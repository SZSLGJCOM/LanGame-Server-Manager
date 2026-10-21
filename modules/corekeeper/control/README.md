# Native current-world save control

Starting the instance automatically prepares its save extension before creating
the game process. This detaches a shared program into the instance's private
runtime, compiles the fixed repository source with the Windows Framework
compiler and the verified installed SDK, and installs or refreshes the owned
native Mod.

Core Keeper uses its bundled `PugMod.IMod` loader; no BepInEx or external loader is
needed. The native package's `Manager.EarlyInit` adds `PugMod.SideLoader` unless
`-safemode` is present. `SideLoader.Init` reads
`Application.streamingAssetsPath/Mods`; there is no `-enablemods` requirement in
the inspected build. The extension is installed as
`CoreKeeperServer_Data/StreamingAssets/Mods/LanGameSaveControl/`, with
`ModManifest.json` and `LanGame.CoreKeeper.Save.dll`.

This particular local extension requires `skipSafetyChecks` and
`accessesExtraAssemblies`: the official SDK restrictions otherwise exclude
`System.IO`, `System.Diagnostics`, `System.Runtime.InteropServices`, reflection,
and native assembly access. They are needed for pinned, authenticated local IPC
and exact native binding. This permission applies to this Mod only. The Mod
uses the package's fingerprinted Harmony for its closed save observers; no
loader-wide safety option or other Mod is changed. See the official
[scripting restrictions](https://github.com/Pugstorm/CoreKeeperModDocs/blob/main/modding-documentation/creating-mods/creating-a-mod/creating-mods-via-scripts-and-prefabs.md#scripting-restrictions).
The inspected `PugMod.Loader.LoadDll` selects `ScriptSecurityMode` from that
individual Mod's metadata.

Verified package: Steam app 1963720, build 25537714. The current
`CoreKeeperServer_Data/Managed/Pug.Other.dll` SHA-256 is
`042b6db4b62a34c5354b19e478302b723b3c89a4e6c6912ffd0ce91c78311938`.
The installer also pins the SDK, Unity CoreModule and netstandard hashes.

The main-thread `IMod.Update` validates `Manager.ecs.WorldsAreLoaded`, the current
server world, and `SaveSystem.SaveIsPending`, then creates exactly one native
`SerializeWorldSystem.SerializeWorld` component entity. The admitted
`SaveSystem.QueueNewSave` does only this creation through a deferred ECB; direct
native creation gives the request a real Entity Index+Version immediately. It
creates no caller-named save, backup or shutdown.

The observer binds that exact entity in the serializer's trigger query to one
serialization generation. Only that serializer's `WriteCompressedWorldData`
payload array, size and world-file descriptor can bind a native `WriteRequest`.
The standalone writer can swallow errors and return buffers after failures, so
neither void return nor `returnQueue` is completion. Its admitted IL flushes and
closes the temporary stream before `File.Replace` or `File.Move`. The observer
records only normal returns from those two original publication calls, then
requires that same native request to reach its end before the pool can reuse
the payload. Path, platform and exact published bytes must also match. Native
calls and exception handling are retained; observers never throw into the game.

The Mod constructs its observers during native loader construction, before
`IMod.Init`. Admission additionally requires evidence that this filesystem's
actual writer thread entered the transformed loop. Installing a patch after a
long-lived writer frame has started cannot manufacture that evidence. Changed
native IL, conflicting patches, absent observations, target changes, failed
publication, expiry and disposal cannot emit a completed receipt. The owner
thread rechecks the current world and storage target before acknowledging the
same pending nonce. While its owned request is pending, it advances the same
serialization-group / BeginSimulation ECB / serialization-group sequence used
by native `SaveSystem.SaveDone`. This drains that work on the owner thread even
when an empty server has timescale zero; it does not advance the simulation
clock or enqueue another save. A busy or blocked serializer still cannot emit
completion until the owned writer actually publishes its payload.

For this dedicated-server snapshot, required world metadata is serialized into
the same world payload. `WriteWorldInfo(int)` is the server's explicit rejected
client-file path; independent world-generation-parameter initialization is not
part of `WriteSerializedDataToFile`. This acknowledgement concerns the current
server snapshot, not client-side saves or storage durability after power loss.

## Shared protocol and installer boundary

`SaveProtocol.cs` and `SaveFiles.cs` also compile into the V Rising and Sons of the
Forest extensions. Source is embedded in the desktop application; callers cannot
supply C# source, arbitrary native objects, reflection targets or filenames.
The only operation is `save`. All native calls run on the game's main thread.

Each startup creates a fresh run nonce and credential in
`.langame-world-save/session.txt`, injected through the module-specific process
environment. Requests have a fixed ASCII schema, a unique nonce and a five-second
native admission deadline. The manager verifies the current primary executable,
program ownership and Windows process creation identity, pins ordinary files and
ancestors, and publishes once. The peer publishes a `dispatching` marker before
the native call, then `dispatched`, an explicitly confirmed `completed`, a
definitive preflight rejection, or `unknown`.
The last 64 nonce results prevent duplicate execution of replayed frames.

A timeout, process change, failed reply publication or exception after admission
is an unknown outcome. It is never retried automatically. A `dispatched` result
is a pending receipt, not disk completion. The actual blocking worker retains
both the instance mutation guard and storage lease if its caller is cancelled.
Only an adapter's explicit `SaveReceipt.Completed` can emit a completed frame;
bool acceptance continues to emit dispatched. Both terminal receipts are cached
by nonce and recovered from the persisted response after a loader reload.
An asynchronous adapter can return `Pending` and observe its own native
operation on subsequent owner-thread ticks. The loop owns at most one such
request, retains its nonce and cached executing receipt, and bounds observation
with a monotonic two-minute deadline. Completion updates that same cached nonce;
failure, expiry or disposal releases observer ownership without native replay.
An already available native completion is recorded before expiring an otherwise
unfinished observation; the manager's independent 125-second total deadline
still bounds the caller's wait.

The `.langame-world-save.json` receipt records source and installed payload hashes
plus pending publication identities. Unknown or changed plugin files are
preserved. Refresh only repairs or replaces verified owned files; first-time
installation uses exclusive creation. User save directories are never edited by
the installer.

Database status alone does not authorize preparation. Windows process enumeration
also checks the selected module's fixed executable against the canonical private
runtime path before any loader/plugin publication and session refresh. An
unmanaged process using that path blocks those writes. Before publishing a save,
the same check requires exactly the current managed PID; a second process sharing
the program and IPC directory is rejected. Other instances using different
canonical program paths do not block this instance.

Run the isolated protocol checks with PowerShell:

```powershell
& modules/corekeeper/control/Test-Control.ps1
```

These checks exercise real pinned file IPC with synthetic native callbacks. They
do not start a server or prove native disk completion. Runtime compatibility must
be checked again when native package hashes change.

`Test-Completion.ps1` compiles the production observer and its boundary tests
with the Windows Framework compiler. Supply `-RuntimeRoot`, a fresh external
`-OutputDirectory` and a modern `.NET` `-RuntimeConfiguration` for reading the
native netstandard 2.1 SDK IL. The tests cover request/generation isolation,
failure paths, worker ownership, original exception preservation and exact
rewrites of the admitted native writer/publication IL. They do not replace a
real loader startup, locked-primary failure, save and restart acceptance run.

On 2026-10-19 the earlier admission-only extension in an isolated copy of the verified server, with a fresh task-owned
`-datapath`, loaded this exact Mod through the native loader. The native log
reported `LanGameSaveControl` and `LanGame.CoreKeeper.Save.dll` with
`safetyCheck=False`. A single request after 65.4 seconds returned
`status=dispatched`, `issue=none`, while that isolated server remained running.
Its owned copied-path processes were then stopped by the test's final cleanup.
This historical run verifies real loader entry and native queue acceptance. It
does not verify the subsequently added completion observer or restart/readback.
