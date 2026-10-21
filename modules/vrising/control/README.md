# Native save extension

Startup automatically prepares the extension before creating the game process.
Preparation uses the existing reviewed
`BepInEx-BepInExPack_V_Rising` 1.733.2 installation/ownership workflow, compiles the
fixed source against that exact loader's bundled .NET 6 SDK assemblies, and
installs `BepInEx/plugins/LanGame.VRising.Save.dll`. A missing reviewed loader is
installed and configured automatically in the instance's independent runtime;
later startups verify and refresh owned files. Unknown or externally changed
files are preserved and reported as conflicts. No game configuration or autosave
interval changes.

Verified package: Steam app 1829350, build 25169871,
1.1.15.0-r101082-b2. `GameAssembly.dll` SHA-256:
`ff366577fb0c707c42b8bde242304653aaa37f6ff0665ab9520150ceb692aed1`.
`VRisingServer_Data/il2cpp_data/Metadata/global-metadata.dat` SHA-256:
`58d3cea9b9b546ad3d734ba2a7296ac3c9820491eec7bb108a03cfbcb1065f85`.

A Harmony postfix on the exact native
`ProjectM.ServerBootstrapSystem.OnUpdate()` runs the closed IPC protocol on the
main thread. It requires `ServerIsInitialized`, the current server world,
`GetServerRuntimeSettings()`, its existing
`ProjectM.TriggerPersistenceSaveSystem`, and native
`IsReadyToSave(ServerRuntimeSettings): bool`. It calls
`TriggerAutoSave(ServerRuntimeSettings): bool` exactly once. Inspection of this
native method confirmed that it constructs the next native autosave name and
calls `TriggerSave` with `SaveReason.AutoSave`; the autosave interval check lives
outside that method. This saves the current world through its ordinary native
persistence path, without caller-specified filenames or server shutdown.

The runtime settings type is `ProjectM.ServerRuntimeSettings` from
`ProjectM.Shared.dll`, as confirmed by the actual generated interop. It is not
defined in `ProjectM.dll`. The current `ServerIsInitialized` getter and
`GetServerRuntimeSettings` are instance members. The optional
`Test-Bindings.ps1` uses .NET SDK `System.Reflection.MetadataLoadContext` to resolve
the production assembly/type literals and verify all reflected members,
inherited world property, exact non-ref settings parameters and system lookup
generic constraints against the generated files without invoking native code.

The completion adapter is now a candidate native observer. `TriggerAutoSave`
returning true starts `status=dispatching`; it never reports success by itself.
The payload also supplies `LgsmVRisingSaveControl.dll` and its MinHook license in
the same plugin directory. The managed source's `LGSM_VRISING_NATIVE_SHA256`
marker must be bound to the bundled helper digest before compilation. The
plugin pins that exact ordinary file while loading it by absolute path with
system-only dependency search. The helper independently hashes and pins the
actual loaded `GameAssembly.dll` and checks the complete Windows function ranges
of the save trigger, serializer, finalizer, publication, writer, consumer and
native autosave entry against its pinned file. Unexpected patches fail closed.

Three native MinHook wrappers preserve the original native calls and exceptions:

- `TriggerPersistenceSaveSystem.TriggerSave`, token `06002c40`, RVA `7f3d030`,
  captures the actual native name only during this IPC operation, on its owner
  thread, for its exact trigger and the reviewed `TriggerAutoSave` callsite
  (`7f3cfc0`). The name must match the reviewed native
  `AutoSave_<decimal counter>.save` grammar: the pinned format literal at
  global `dbd5a70` is `{0}{1}.save` and its prefix is `AutoSave_`.
  Native bool rejection never becomes completion.
- `SerializePersistenceSystemV2.TriggerSave`, token `06002b8f`, RVA `7f25570`,
  binds the same serializer to that native event's exact reason, name and gzip
  flag. The serializer must have no existing disk-write dependency before
  admission. Another serializer generation invalidates the receipt.
- `FinalizeWritersJob.Execute`, token `06002bb5`, RVA `7f33970`, matches the
  native result container and exact native target leaf. It calls the original
  job once and copies the terminal byte only after the original writes it,
  while still executing on that job's owner thread. It never reads a
  `NativeReference` through `IsCompleted` polling or calls `Complete` itself.

For this reviewed build, `Execute` calls `MoveCopyAndFinalizeSave` at `7f3399f`,
stores the returned result through its `Result` pointer at `7f339a4`–`7f339ab`,
and returns. `MoveCopyAndFinalizeSave` first examines every
`WriterJobsSuccessful` entry; any non-success writer result bypasses success.
It performs native publication/required-file and written-data validation,
counts serialized chunks from the native job data, and returns `Success=3`
only after the final chunk-count check at `7f340d9`–`7f340f4` and cleanup.
Native results `4`–`10` represent actual write/publication/validation failures
and map to `status=failed, issue=vrising_write_failed`; `0`–`2`, missing hooks,
changed identities and ambiguity never map to success. Header, world, system
and ECS chunk writers are covered by this native aggregate. This is the game's
completed-write contract, not an additional filesystem crash-durability claim.

The native consumer calls `JobHandle.Complete` at `7f2a3cf` before clearing
`_WriteToDiskDependency` at `7f2a3de`/`7f2a3e5`. A later request's `begin`
requires that dependency to be absent. The previous job's wrapper must have
returned before `Complete` can return, so rotating/reused autosave names and
the serializer's reused result container cannot leave an older wrapper in
flight when a new request begins. A still-queued earlier save event is also
rejected by the original trigger's query guard before native acceptance.

The managed observer rechecks the server, native world pointer, live world
sequence number and existing trigger/serializer identity before accepting a
receipt. A monotonic helper ticket binds asynchronous native completion to the
one pending SaveLoop request; timeout/unload abandons that ticket, so a late
completion cannot acknowledge a later request. Unknown results are not resent.
See the shared [protocol and boundary](../../corekeeper/control/README.md).
Failure diagnostics use closed stage/reason labels only, including trigger
identity, caller, exact native name grammar, event identity, finalizer and
result-address failures. They do not print paths, payloads, native pointers,
IPC tokens or game-supplied exception text, and never change a failed
verification into success.

The low-level wrappers do not pass the original through the IL2CPP Harmony
native-to-managed exception bridge. Each original function's relocated prefix
has pinned Windows unwind metadata, checked at every trampoline PC before
enabling any hook. Original native C++ exceptions pass through the C++ wrapper
without a catch; observer code does not replace native outcomes. All hook code,
metadata and library mappings remain pinned until process exit. Unload ends
the IPC owner while leaving forwarding-only native hooks in place.

The Windows Framework compiler emits CS1701 because this reviewed BepInEx SDK
references netstandard 2.0 while its own official bundled .NET 6 runtime supplies
netstandard 2.1. This known reference unification warning remains visible; all
other compiler warnings fail preparation. Compilation uses the actual reviewed
release assemblies.

On 2026-10-19 an isolated ordinary program copy, the reviewed loader and fresh
task-owned `-persistentDataPath` loaded this actual plugin and reached native
server startup completion. That acceptance run first caught the incorrect
runtime-settings assembly: one preflight was definitively rejected before any
native save. The metadata binding test reproduced that failure, then passed all
12 checks after the one-line assembly fix. The corrected production DLL was
recompiled and tested on another fresh data root. Exactly one new request at
31.8 seconds returned `status=dispatched`, `issue=none`, while the server remained
running. The game endpoint was loopback; only the explicitly identified read-only
Steam Query endpoint used all interfaces, with server listing and RCON disabled.
All processes belonging to that copied program path were stopped in cleanup.
This verifies the earlier loader entry, main-thread reflection, native readiness
and `TriggerAutoSave` bool acceptance. It does not verify the new completion
candidate. No user save or existing credential was used.

On 2026-10-20, the new candidate passed 107 synthetic identity/state boundary
checks and 16 actual MinHook/prologue/unwind/C++ exception propagation checks
using synthetic native probe functions. The actual reviewed `GameAssembly.dll`
and `baselib.dll` were then copied to an isolated component directory: loading
the image, matching its complete function ranges and installing all three
native observers passed without initializing IL2CPP or invoking game methods.
The actual generated interop passed 21 metadata binding checks; the managed
entry compiled against the reviewed loader SDK. These are component checks,
not full-server or fault-injection acceptance. The subsequent full-server result
is recorded below.

The first complete-server attempt reached the actual save path but rejected its
request identity immediately: the observer's old filename filter rejected the
native `.save` extension. The same failure was reproduced before repair by
changing the component fixture to the actual `AutoSave_42.save` name; its native
admission assertion failed. The fixed grammar accepts exactly the reviewed
prefix, decimal counter and `.save` suffix while rejecting missing/extra
extensions, other prefixes, path separators and nonnumeric counters. The
107-check run above includes this repair and closed failure-reason checks.
The repaired candidate then passed the manager's real isolated local acceptance
on 2026-10-20: automatic startup preparation, a request-correlated completed save,
normal shutdown, a fresh process loading the same world, a second completed save,
and normal shutdown and fixture cleanup. Both saves used the shared production
Maintenance/card action. The evidence is `vrising-150624-831751/run.json` under
the local `lgsm-save-verification-followup-20261020` evidence directory:
`formal_action_receipts=2`, `fresh_restart_ready=true`, `cleanup_passed=true`,
and test exit code `0`. The frozen manager test binary SHA-256 is
`17b512cc785146b61f5e4de8ca3effcf28b78d192a7b189c8d26b8cb90b62a9c`.
This was an empty, task-owned server; it does not establish player-state
coverage, power-loss durability or full-server fault-injection behavior.

Developer checks: `scripts/build_vrising_save_control.ps1 -OutDir <ordinary
external directory> -Test` compiles the native helper and runs the two component
executables. `tests/native_image_probe.py` accepts an isolated directory holding
ordinary copies of the reviewed image and baselib, plus the compiled helper;
it never invokes a game method. MinHook v1.3.4 is reused from the existing
reviewed vendor directory; its complete source manifest is checked before
building, with no download or end-user compiler requirement.

Official contracts:

- [Server RCON instructions](https://github.com/StunlockStudios/vrising-dedicated-server-instructions/blob/master/1.1.x-pc/INSTRUCTIONS.md#rcon) do not expose a save command.
- [BepInEx IL2CPP BasePlugin](https://github.com/BepInEx/BepInEx/blob/master/Runtimes/Unity/BepInEx.Unity.IL2CPP/BasePlugin.cs) provides the plugin lifecycle.
- The reviewed loader archive and complete file hashes remain in the existing
  `apps/desktop/src-tauri/assets/mod-loaders/vrising.json` authority.
