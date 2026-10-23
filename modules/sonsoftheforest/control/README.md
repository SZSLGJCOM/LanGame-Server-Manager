# Native dedicated-server save extension

Startup preparation, before creating the game process, downloads the fixed official
[RedLoader 0.8.6 release](https://github.com/ToniMacaroni/RedLoader/releases/tag/0.8.6),
checks its complete archive and file hashes in `redloader.json`, and installs its
official Windows dedicated-server layout. The reviewed archive is 36,770,854
bytes, SHA-256
`1429bee2fe6a5b1b82bb06a447762b09e9faf077138966fbfe65c687123fd4eb`.
No installer or downloaded script is executed. Existing changed loader files,
proxy DLLs or configuration are preserved and reported as conflicts. The fixed
loader is installed and configured automatically on first startup; later starts
verify it and refresh owned extension files.

A failed remote download can skip manual save after the installer verifies all
remaining loader files, retires only the owned unchanged `version.dll` from an
incomplete installation, and removes the owned save plugin. It retains the
loader receipt for recovery. Unowned or modified proxies, local path failures
and unsuccessful cleanup block startup without deleting operator files.

The fixed C# source is compiled using the Windows Framework compiler and this
release's bundled .NET 6 SDK assemblies. It installs `Mods/LanGame.Sons.Save.dll`
and `Mods/LanGame.Sons.Save/manifest.json`; the same manifest is embedded as
`LanGame.Sons.manifest.json`. Loader startup/manifest contracts are in the official
[dedicated-server instructions](https://github.com/ToniMacaroni/RedLoader#dedicated-server-installation),
[SonsMod callbacks](https://github.com/ToniMacaroni/RedLoader/blob/0.8.6/SonsSdk/SonsMod.cs)
and [ManifestReader](https://github.com/ToniMacaroni/RedLoader/blob/0.8.6/SonsSdk/ManifestReader.cs).

Verified package: Steam app 2465200, build 20228410.
`GameAssembly.dll` SHA-256:
`deadbef72344c5d19e2572e607f0c1ecbd0b85200ec9b98d5939753cde5929a2`.
`SonsOfTheForestDS_Data/il2cpp_data/Metadata/global-metadata.dat` SHA-256:
`d9df15ce6470003dcffe1149a2d11625fa6b3feb7db509bcd65e3b268bfe448c`.

The loader's `OnInitializeMod` initializes the control. The SDK's title-scene
`OnSdkInitialized` callback does not run in a dedicated server: its server path
calls `InitSystems` directly rather than the title-scene initialization coroutine
([fixed loader source](https://github.com/ToniMacaroni/RedLoader/blob/0.8.6/SonsSdk/Private/MainInitializer.cs)).
The `SonsSdk.Attributes.IOnUpdateReceiver.OnUpdate` callback runs the shared closed
IPC protocol on the main thread. It requires exactly one active native
`Sons.Gameplay.DedicatedServerAutosave` from `Sons.Multiplayer`, validates its
native `IsReadyToSave(): bool`, then invokes public static
`SaveImmediate(): void` once. No user-specified slot, filename, native object or
code is accepted. Its void return alone cannot confirm completion. The
`SonsSaveCompletion.cs` observer registers the game's official before-save
and after-save callbacks only around this request's synchronous invocation.
Exactly one before/after pair on the request's owner thread must complete for a
full-world save. The after callback's bool means `savePlayerOnly`, not success;
a player-only or no-op call cannot acknowledge the request. Registration,
unregistration and duplicate-callback failures remain unknown. Failed callback
cleanup makes the subscription inert and prevents further subscriptions.

The native registry stores a weak target and a reflected method; the loader's
implicit managed-delegate conversion exposes `Action.Invoke` with a different
reference-holder target. Actual native unregistration rejected that pairing.
The observer therefore creates one field-free receiver type with ordinary public
instance methods, using `Reflection.Emit` after the generated wrappers are
loaded. The pinned runtime's official `ClassInjector.RegisterTypeInIl2Cpp(Type)`
registers that completed type. Native `Delegate.CreateDelegate` binds its native
`GetMethod` results to the receiver; target pointer, declaring type, method name,
return type and parameter types are checked before registry admission. The
receiver and its two delegates remain strongly held and are reused for the
process. Public bridge methods only forward to the current thread's capture.
This follows the fixed runtime's [class-injection contract](https://github.com/BepInEx/Il2CppInterop/blob/8663e8997a9e5cef0c8f8b9f7b082cdeb3304e89/Documentation/Class-Injection.md),
without hand-written native pointers or an extra generated-wrapper dependency
in the pre-start compiler. First startup still performs the whole setup.
Execution and cleanup failures are both retained when both occur; cleanup must
succeed before a completion receipt can be returned.

The pinned native `SaveGameManager.Save` invokes its after-save callback only
after `WriteJsonZip` and the remaining synchronous save work. `WriteJsonZip`
closes the serialized temporary ZIP, executes the native backup boundary, copies
that ZIP over the primary `SaveData.zip`, and deletes the temporary file. Its
failure cleanup rethrows through native `Save`. The admitted loader also patches
`Save`; its native-to-managed trampoline can log and swallow that exception
before `SaveImmediate` returns. The native after callback is never reached, so
the missing callback pair reports unknown even when the outer call returned.
Only this exact native callback chain emits `status=completed`; no file timestamp,
adjacent log or void return substitutes for it. Actual locked-primary acceptance
checks this failure contract separately. No completion detour is installed on
file copying, serialization or job finalization: the reviewed IL2CPP Harmony
trampoline catches original exceptions and could change native failure behavior.

The completion receipt means the native synchronous full-world save finished.
It does not make the game's overwrite operation atomic or certify durable
storage. Thirty-four callback-boundary cases cover no-op, completion, copy failure,
callback order, player-only saves, nested callbacks, another thread, registration
compensation, one-time receiver injection, native reflected-method pairing,
failed-cleanup containment across observer replacement, preservation of the
original write failure, loader-swallowed write exceptions, owner-thread
isolation, recursive diagnostic containment and locked-copy failure diagnostics. Those doubles verify the observer,
not actual IL2CPP behavior; real acceptance is recorded separately.

`#DSL Dedicated server loaded.` and a live A2S reply do not prove that the native
autosave component currently accepts saving. Startup therefore runs the same
read-only `Prepare` validation on the main thread at most once per second. It
never invokes the returned save delegate. The first accepted preflight logs
`LGSM_SONS_SAVE_READY` once in `_Redloader/Latest.log` and permanently ends this
startup observation. `world_not_ready` continues observation; another API or
world-ambiguity failure logs `LGSM_SONS_SAVE_UNAVAILABLE` once and ends it. The
observer also stops after ten minutes, so a failed or permanently unready world
cannot cause endless object scans. The smoke waits for this actual preflight
marker as well as the normal world/query readiness. Every real manual save still
discovers and validates the current owner and readiness again before dispatch;
a startup marker never bypasses those checks.

The fixed native binary was inspected independently of its generated managed
thunks. `Sons.Multiplayer.dll`'s IL2CPP code-generation module maps native token
`0x06000039` to `IsReadyToSave` at `GameAssembly.dll` RVA `0x28effe0`.
That implementation requires `LoadScene.IsMainSceneLoadComplete` and either a
world loaded from a save or the native initial post-cutscene save queue. Its
cached loaded-world check calls `GameSetupManager.GameLoadedFromSave` at RVA
`0x2818130`; `LoadScene.IsMainSceneLoadComplete` at RVA `0x28c8440` reads the same
native loading flag. The readiness gate has no autosave interval, player-count
or empty-server pause check. Native `Update` checks the timer only after this
gate. Native `SaveImmediate` at RVA `0x28f03b0` calls the same gate and returns
without saving when it is false, so accepting that void no-op would give a false
invocation receipt. A fresh empty world must reach its own native initialization
conditions; a loaded-server log alone does not establish them.

An opt-in native acceptance can load the existing checkpoint created by this
task: both `LANGAME_NATIVE_SONS_WORLD_ZIP` and
`LANGAME_NATIVE_SONS_WORLD_ZIP_SHA256` must be supplied. Before the initial start,
the test validates ordinary pinned paths, the exact archive hash, ZIP entry and
expanded-size limits and CRC, then exclusively copies that archive into a fresh
fixture's slot 1. It requires the fixture's own `SaveMode=Continue` and
`SaveSlot=1`; it copies no configuration, password or native state flag. This
case reports `source=task_checkpoint` and does not claim fresh world generation.
The restart loads the same fixture world and never seeds it again.

`SonsSaveReadinessTests.cs` compiles alongside the plugin and shared protocol
using the same reviewed RedLoader/.NET references. Its five boundary cases
verify that observation never invokes saving, polls at most once per second,
reports readiness once, stops on terminal native failures, and stops at its
budget. These tests do not start Unity or replace a native server acceptance run.

See the shared [protocol and ownership boundary](../../corekeeper/control/README.md).
On 2026-10-19 an isolated ordinary program copy with fresh task-owned
`-userdatapath`, a random private password, `LanOnly=true`, and a 3,600-second
autosave interval completed the production native invocation. The exact generated
`Sons.Multiplayer.dll` signatures were verified, the actual Mod initialized, and
one authenticated request received a matching run/nonce/PID `dispatched` receipt
after current-world discovery and `IsReadyToSave()`. The server remained running.
The slot's `SaveData.zip` then updated to 127,540 bytes with a new native save
marker, and RedLoader logged the game's serialization at the same time. This
observed save is integration evidence, not a durable per-request completion API;
the production result remains unconfirmed as described above.
The 39-entry archive passed ZIP CRC validation. A subsequent `SaveMode=Continue`
restart, without another manual save request, deserialized that slot, activated
the world, and retained its exact `GameId` in the game's next startup save.

The later production acceptance run beginning at 20:40 on 2026-10-19 verified
automatic loader/extension preparation, native preflight readiness, one formal
manual-save receipt, a normal stop, a new managed run, readiness and a second
formal receipt, followed by owned-process cleanup. This run explicitly loaded
the prior task-owned checkpoint (SHA-256
`82f585727e31d2e3ac640ae89787c95ef2b5608b50ccd58a1091dee8f64620c6`);
it does not prove fresh empty-world generation. Both receipts remained pending
write confirmation, and the restart check did not compare saved world contents.

Both stops exposed the defect corrected below: after the manual invocation logged
`Autosave complete`, the game's `Sigterm: Emergency Shutdown` performed another
save in the same second. `BackupPreviousSave` then reported an `IOException`
because its second-resolution backup ZIP name already existed. The fixed native
binary confirms the order: complete and dispose the new temporary ZIP, attempt a
directory backup, copy the temporary ZIP over `SaveData.zip` with overwrite
enabled, then delete the temporary ZIP. `BackupPreviousSave` catches and logs its
own exception and returns; a backup-name collision therefore does **not** prevent
the subsequent primary-save overwrite. The failed exclusive creation does not
overwrite the colliding backup ZIP, although normal native backup rotation runs
before creation and can remove older backups. The latest primary contents before
this overwrite may consequently lack their own independent recovery backup.

The game logged another autosave completion and the primary `SaveData.zip`
remained present. The subsequent restart reached readiness after the first
occurrence. This is not evidence of observed corruption, nor does it establish
the final stopped archive's integrity: native overwrite uses `File.Copy`, not an
atomic replacement, and the void save API does not certify storage success.
The module's stop action sends only `CTRL_C`; the extension adds no shutdown save.
Its readiness gate has no busy-save or cooldown check. A manual-request check of
the current second's backup name cannot protect later native automatic/shutdown
saves, and the native timestamp is chosen only after ZIP serialization.

For the exact native package admitted above, the extension now installs one
owned Harmony prefix on the generated
`Sons.Save.SaveGameManager.BackupPreviousSave(string, bool, bool): void`.
The existing reviewed RedLoader SDK supplies Harmony 2.10.1; no additional
loader or download is needed. Startup refuses an existing patch on that method,
verifies its own installed prefix, and removes only its own patch when the mod
is unloaded. Initialization failure cannot create the manual-save IPC loop.
`LGSM_SONS_BACKUP_PATCH_READY` and `LGSM_SONS_BACKUP_PATCH_REMOVED` identify its
lifecycle. Normal native manual, automatic and shutdown saves reach the same
backup boundary. The separate `markCorrupted=true` recovery operation retains
its original native implementation and naming behavior.

Normal backup names keep the native slot prefix, real local second, and `.zip`
suffix, then append a four-character lowercase hexadecimal ordinal directly to
the time: `<slot>_backup_yyyyMMddHHmmss0000.zip`. They are only four characters
longer than native names. The ordinal is not a fabricated future timestamp.
The exact native `DeleteOldBackups` and `SaveHasAvailableBackup` implementations
select `Path.GetFileName(...).StartsWith(<slot>_backup)`, sort strings, and choose
the last candidate for recovery. They do not parse the timestamp. Tests verify
same-second native and extended names sort together in invariant, Chinese and
English cultures. The ZIP keeps the native enclosing slot-directory entry, so
native recovery can find `<slot>/SaveData.zip` and the accompanying files.

The backup directory and source-directory ancestors stay pinned against
redirection. Source files are ordinary, pinned read handles. One complete ZIP is
written into an exclusively created `.lgsm-backup-*.partial` sibling, outside
the native recovery prefix. After disposing the ZIP, the same exclusive output
stream is flushed and its length/SHA-256 recorded. The completed file is
published using a same-directory, no-overwrite move. A directory scan chooses
the next ordinal; ownership is established only by that atomic move. Only
Windows `ERROR_FILE_EXISTS`/`ERROR_ALREADY_EXISTS` allocates another ordinal for
the same completed ZIP, with at most 4,096 candidates and no ordinal wrap.
There is no native-save replay or repeated ZIP serialization. Other I/O failures
are reported immediately.

Before retention, the published ordinary file is pinned, checked against that
original length/SHA-256, and kept open without write/delete sharing until
retention finishes. A replaced, changed or redirected publication cannot delete
older recovery points. Only then are older recognized native/extension backups
removed to retain three; unknown external backup names are preserved. The newly
published recovery point is always retained, even if the system clock moved
backward. Failed serialization/publication leaves previous backups untouched;
failed retention preserves the complete newly published archive. Cleanup deletes
only the pinned ordinary file leaf, and preserves both errors if cleanup also
fails. `LGSM_SONS_BACKUP_FAILED issue=backup_io_failure hresult=...` reports a
failed backup. As in the admitted native catch-and-log branch, native primary
saving continues afterward. The existing pending manual receipt remains honest:
neither successful backup publication nor the native void save API alone
certifies primary-save completion or atomic replacement. Completion is now
observed separately through the native callback chain above.

`SonsBackupTests.cs` runs real filesystem/ZIP operations and the actual reviewed
Harmony implementation; only the external `Sons.Save` method is a boundary
double. Its 24 cases cover same-second preservation, exclusive ordinal allocation
and exhaustion, native sort compatibility, three-backup retention, unknown-file
preservation, read/retention failures, clock rollback, publication hash checks,
write/delete exclusion, long local file-leaf pinning, independent concurrent publishers, patch conflicts,
native-call dispatch, failure handling, concurrent callbacks and unpatching.
The original five readiness cases also pass. These are boundary regressions,
not a native game acceptance claim. From PowerShell 7 at the repository root:

```powershell
& modules/sonsoftheforest/control/tests/Run.ps1 -LoaderRoot 'D:\path\to\prepared\private-server' -OutputRoot 'D:\path\to\existing\test-output'
```

The runner verifies the loader reference hashes and uses its bundled .NET 6.0.7
in a private output layout with an already installed local `dotnet` host. It
does not install a global runtime or start Unity. `-NativeSecondResolutionName`
builds a clearly identified differential naming baseline: the unchanged real
filesystem/ZIP case fails on its second same-second exclusive publication.
This baseline is not presented as an old production executable.
`-ConvertedDelegateBaseline` substitutes only the prior implicit conversion;
its first registry cleanup fails the reflected target/method check. It leaves
the observer, emitted receiver, filesystem and all other tests unchanged.

On 2026-10-20 the corrected extension completed two actual production start/save/
normal-stop cycles with a fresh managed restart in between. Each cycle issued
one formal authenticated save request, then immediately followed the ordinary
stop workflow without an added delay. Both normal stops returned native exit
code `-1`, with `crash_flag=false` and no remaining owned process. The fixture
loaded only the prior task-owned checkpoint identified above; it had no players
and does not claim fresh empty-world generation.

Read-only checks before the formal request and after each stop verified the
primary ZIP CRC, every retained outer backup and nested primary ZIP CRC, the
native slot-directory layout, and the same loaded native `GameId`. Each cycle
retained the exact pre-request primary bytes in a recovery ZIP, created two new
recovery points, and retained three total backups. In the restart cycle, the
manual save and native `Sigterm: Emergency Shutdown` both completed at local
09:15:50. Their distinct recovery ZIPs ended in
`_backup_202610200915500000.zip` and `_backup_202610200915500001.zip`; both passed
the nested archive checks. No backup failure or `IOException` was logged.
This directly exercises the original same-second collision condition.

The shutdown save still used the installed backup prefix and published its
ordinal-named ZIP, establishing that the patch was active through that final
native save. The normal native exit did not record a patch-removal marker, so
the test does not assert a fully observed ordering of unload callbacks. Explicit
mod disposal/unpatching is covered separately by the Harmony boundary test.
Independent OS checks found zero processes under the fixture program root and
no remaining listeners on UDP 8766/27016/9700 or TCP on those ports. These checks
establish archive integrity and world-identity readback for this fixture; they
do not certify per-request durable writes or real player-progress recovery.

The native game endpoint used `127.0.0.1`; query and BlobSync endpoints bind to
`0.0.0.0` despite the loopback `IpAddress` setting. No firewall, global setting,
existing world, or user credential was changed. All processes belonging to the
copied program path were stopped. Changed native packages continue to fail closed
until their contract is verified.

The ignored fixture test may explicitly enable
`LANGAME_NATIVE_SAVE_FAILURE_ACCEPTANCE=true`. Only within this explicit fixture
scenario, an invoke-scoped CLR `FirstChanceException` listener retains at most
one matching native IOException before the loader may swallow it. The listener
requires the current capture and owner thread, limits inspected text, prevents
recursive inspection and unregisters in `finally` before receipt/cleanup work.
It performs no I/O and changes no completion condition. Only failed invocations
attach this evidence and fixed callback counts to the existing diagnostic graph.
Sons failures already past admission emit bounded, nonce/run/PID-correlated native IOException and write
frames through the existing mod logger; fixture paths are replaced with
`<fixture>`, and IPC tokens are excluded. A failing diagnostic sink preserves
the original unknown outcome. This opt-in changes no completion condition and
introduces no native detour. The test locks only its own primary file against
write/delete sharing, verifies the resulting native write failure and unchanged
primary hash, releases the lock, and then uses a fresh request for ordinary
positive acceptance. An independent completed-save preflight first checks the
real receiver's callback pair; it does not count toward either of the two formal
save/restart cycles. This fault evidence is separate from completed saves.

The completion-specific production acceptance also passed on 2026-10-20 with a
new complete private copy of the admitted native package. First startup installed
and compiled the extension and generated the loader wrappers automatically. A
separate preflight completed, then the locked-primary request reported unknown
with exactly one before callback and no after callback. Its same-invocation
first-chance native IOException identified the exact primary; the locked bytes
were unchanged. After releasing the lock, both formal requests completed across
a fresh managed restart, and both ordinary stops finished with native exit `-1`
and `crash_flag=false`. Each stopped cycle passed primary/outer/nested CRC,
native world-identity, exact pre-request-primary recovery and three-backup
retention checks. The initial cycle retained three distinct recovery points
from the same second, ending in `202610201136240000.zip`, `0001.zip` and `0002.zip`.
The archive log check holds the same ordinary read handle across the formal save
and stop, verifies its captured prefix hash, and rejects any later IOException
or backup failure; the deliberate earlier fault is outside this window. Final
cleanup completed and independent OS checks found no copied server process or
remaining game/query/blob listener. This confirms native completion and the
tested recovery archives, without asserting power-loss durability or player
client progress.
