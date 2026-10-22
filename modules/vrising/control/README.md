# Native save extension

Startup prepares `BepInEx/plugins/LanGame.VRising.Save.dll` through the existing
reviewed `BepInEx-BepInExPack_V_Rising` 1.733.2 installation and ownership flow.
The managed entry is compiled against that loader's bundled .NET 6 SDK. Unknown
server builds disable only the manager-owned save plugin before native startup;
modified or foreign files remain conflicts. User Mods and autosave settings are
preserved.

Verified package: Steam app 1829350, build 25169871,
1.1.15.0-r101082-b2. The IL2CPP compatibility tuple remains:

- `GameAssembly.dll`: `ff366577fb0c707c42b8bde242304653aaa37f6ff0665ab9520150ceb692aed1`.
- `VRisingServer_Data/il2cpp_data/Metadata/global-metadata.dat`:
  `58d3cea9b9b546ad3d734ba2a7296ac3c9820491eec7bb108a03cfbcb1065f85`.

A Harmony postfix on `ProjectM.ServerBootstrapSystem.OnUpdate()` processes the
closed IPC protocol on the game thread. The adapter requires an initialized
server and live world, obtains that server's `GetServerRuntimeSettings()`, and
uses its existing persistence systems. `SerializePersistenceSystemV2.IsSaving`
and `TriggerPersistenceSaveSystem.IsReadyToSave(settings)` reject busy saves.
The world pointer and sequence must still match at invocation.

`TriggerAutoSave(ServerRuntimeSettings): bool` is invoked exactly once. The
native method builds its ordinary autosave name and forwards the bool returned
by `TriggerSave`; autosave scheduling happens outside this entry. A true return
means **requested**, not completed or durable. False is a native rejection.
An exception after dispatch remains unknown and is never automatically resent.
The extension does not patch the serializer, finalizer, or writer callbacks,
read their internal offsets, or infer completion from nearby file changes.
See the shared [protocol and boundary](../../corekeeper/control/README.md).

The settings type belongs to `ProjectM.Shared.dll`; the server initialization
getter and settings accessor are instance members. `Test-Bindings.ps1` checks
the actual generated interop without invoking native code. `Test-Control.ps1`
compiles the production adapter and protocol with native-boundary doubles to
check acceptance, rejection, readiness, world identity, and replay behavior.
These component tests do not establish real-world persistence. Acceptance must
capture the native save before shutdown, then restore that snapshot and verify
its contents in a fresh process.

The isolated exact-save test opts into one read-only diagnostic of the native
runtime autosave interval and variance. It excludes the periodic-save window
using those applied values and the current process identity, then requires
fresh native write-completion and reload logs. Missing diagnostic fields fail
acceptance without changing the real save operation or operator settings.

An upgrade publishes the new managed entry before retiring the old owned native
observer DLL and its license. Interrupted publication keeps the ownership intent
for recovery; missing, changed, or foreign files are never adopted or overwritten.
The reviewed loader remains installed for other operator plugins.

The Windows Framework compiler can emit CS1701 because the reviewed BepInEx SDK
references netstandard 2.0 while its bundled .NET 6 runtime supplies netstandard
2.1. This reference unification warning remains visible; other warnings fail
preparation.

Official contracts:

- [Server RCON instructions](https://github.com/StunlockStudios/vrising-dedicated-server-instructions/blob/master/1.1.x-pc/INSTRUCTIONS.md#rcon) expose no save command.
- [BepInEx IL2CPP BasePlugin](https://github.com/BepInEx/BepInEx/blob/master/Runtimes/Unity/BepInEx.Unity.IL2CPP/BasePlugin.cs) defines the loader lifecycle.
- Reviewed loader archive identity and file hashes remain in
  `apps/desktop/src-tauri/assets/mod-loaders/vrising.json`.

Immediate Stop sends the native console interrupt, which invokes the game's
`OnApplicationQuit` and `SaveOnExit` path. It does not depend on RCON being
enabled. The native RCON `shutdown` command schedules positive integer minutes;
zero is rejected and cannot implement immediate Stop. RCON commands use one
matching response, as in the officially recommended
[mcrcon client](https://github.com/Tiiffi/mcrcon/blob/master/mcrcon.c).
V Rising rejects Source response-value markers. Native `error:` replies remain
command failures, and neither a response nor a delivered console interrupt
substitutes for observing the complete owned process tree exit.
