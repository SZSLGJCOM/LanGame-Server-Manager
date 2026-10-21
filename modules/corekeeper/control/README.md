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

This local extension requires `skipSafetyChecks` and
`accessesExtraAssemblies` for pinned, authenticated local IPC and reflection.
These permissions apply only to this Mod. Harmony patching is disabled; no
native method body, writer thread, filesystem call or other Mod is changed.

Verified packages: Steam app 1963720, builds 25537714 and 25825323. Each build
has a complete fingerprint tuple for `Pug.Other.dll`, `Pug.ECS.Components.dll`,
`Unity.Entities.dll`, the SDK, Unity CoreModule and netstandard. Mixed tuples are
rejected. The native save APIs and their IL were compared across both packages;
metadata tokens are not an integration dependency.

On the game's `IMod.Update` thread, the extension validates the current ECS
manager, loaded server world, shutdown state and `SaveSystem.SaveIsPending`.
A native entity query also requires a world object eligible for serialization.
A new empty world can report loaded before creating any such objects; writing
only its metadata would fail the native loader's `ObjectDataSerializedCD`
requirement on restart. That state is rejected as `world_not_ready` before
queueing or writing. The extension does not resume simulation to create objects.
It calls `ECSManager.QueueNewServerSave()` once, preserving its Boolean result.
This is the game's ordinary automatic-save queue. `SaveSystem.Save()` is not
used because it disables player and inventory systems for shutdown saving.

An empty server pauses simulation. While the request is pending, the extension
flushes the queued event and advances `SerializeWorldRunSystem`, the native chunk
preparation normally performed by simulation. It then advances the
serialization-group / BeginSimulation ECB / serialization-group sequence used
by `SaveSystem.SaveDone`, every 200 ms with
the protocol's two-minute deadline. It never advances the simulation clock,
queues a second save or waits synchronously for the serialization worker.
World changes, expiry and disposal release the pending operation without retry.

`SerializeWorldSystem.LastWorldWrite` changes after the game submits serialized
bytes to its own filesystem queue. Advancement ends only after that changes,
the native serialization state is `Idle`, and no save trigger remains. This
also drains a request queued while an earlier automatic save is in progress. The
receipt is `dispatched`, displayed as a requested save, never `completed`.
Serialization errors leave the request unconfirmed. The extension does not
infer disk success from a timestamp, log line or another task's write, and does
not replace the game's serialization, filesystem or error handling.

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
with a monotonic two-minute deadline. Submission or completion updates that same cached nonce;
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

Native acceptance uses both admitted server packages: an uninitialized empty
world rejects without writing, and an initialized world's manual snapshot is
copied before shutdown and loaded in a separate process. Normal shutdown saves
are not used as evidence that the preceding manual save succeeded.

Build the production extension against the admitted package with
`Build-Control.ps1 -Module corekeeper -RuntimeRoot <runtime> -OutputDirectory <external-output>`.
