# Enshrouded server control

This independently implemented Windows x64 bridge is installed as `dbghelp.dll`
beside the owned dedicated-server executable. `MiniDumpWriteDump` always forwards
to the Windows system DLL. Control starts only when the adjacent plain directory
`.langame-enshrouded-control` contains a plain `enable` file whose entire contents
are `LGSM_ENSHROUDED_CONTROL_1\n` (LF, no BOM). The Rust installer owns the runtime
marker, instance binding, payload integrity and stopped-process installation.
Files or directories with reparse attributes are rejected. The enable file is an
explicit local opt-in, not a password; an operator able to replace the DLL already
has process code execution authority.

The compiled profiles cover these exact official executable SHA256 digests:

| Steam public build | Executable SHA256 | Admitted scope |
| --- | --- | --- |
| 25967645 | `4de2acdd3b18e562f129bc488a2daea2da16b00341497304423e1e18c49a83ba` | Player control and world saving; earlier acceptance below |
| 25988414 | `b627639ce4a3f8c940fe90afa1d36ed59db3ff9ced24e935890eb7c99b04d5bf` | Player control; immediate world saving remains unsupported |
| 26083076 | `d7baa768eb00aa895d9f5a1cd8747ccf66858c7b542f50983092b2e3261588ee` | Player-control ABI rechecked statically; current world-save profile below |

The release profile inventory is [supported-builds.json](supported-builds.json).
Adding a compiled profile is not publication or real-client acceptance: the
independent release tool also requires the final DLL's moderation test receipt.
Both the on-disk digest and loaded PE/entry bytes must match. Unknown builds retain
dump forwarding and do not install control hooks. No game binary is distributed.

## Independent updates and maintenance

LanGame maintains this bridge in this repository. Steam does not provide this
extension or guarantee the internal functions it uses. A game hotfix can move
those functions even when their behavior remains the same. Each supported profile
therefore pins the executable digest, PE identity and native entry bytes; new
builds require inspection and validation before they can control players.

Before starting an Enshrouded instance, the manager hashes its actual executable
and selects only locally verified exact-build support from the signed cache or
built-in payload. Extension preparation is optional: a failed check or install is
logged, unrelated loaders are preserved, and normal game startup continues.
Hashing, signature checks and file work run off the async executor.

Each normal start may also launch one bounded background check of the published
signed feed. Startup never waits for the network or maintenance work. Concurrent
starts do not queue extra checks; after an attempt ends, a later normal start may
check again. Downloads only update the application cache. They cannot replace a
live runtime DLL, restart a server, change game configuration or update the game.
A downloaded compatible extension is prepared at the next normal stop/start.
An unknown executable disables native player-management actions and shows
the reason beside the buttons; the server remains available for normal gameplay.
Immediate saving also requires an admitted save profile. When preparation has
already identified an unsupported executable, saving reports the incompatible
game version before opening absent control files, and sends no request. A
player-control-only profile reports that saving is unsupported separately.

The independent trust root is [update-public-key.pub](update-public-key.pub),
separate from the desktop updater's key. Both the manifest and the DLL use standard
four-line minisign `ED` signatures (Ed25519 over Blake2b-512), including a signed
purpose comment. Signatures, exact executable membership, payload length/digest
and a monotonic revision are checked before caching/installing; installed signed
evidence is verified again when native control is opened. Public download URLs
are derived from the fixed repository and signed version, never supplied as
arbitrary URLs by metadata.

- Moving feed: `https://github.com/SZSLGJCOM/LanGame-Server-Manager/releases/download/enshrouded-control/manifest.json`
- Feed signature: the same URL with `.minisig` appended.
- Immutable payload: `.../releases/download/enshrouded-control-v<version>/Lgsm.Enshrouded.Control_<version>_windows-x86_64.dll`, plus `.minisig`.

These GitHub releases are marked prerelease and `make_latest=false` so the desktop
application's stable update feed remains independent. The extension's own feed
is its production channel despite that GitHub display label. The versioned
payload and signatures are never overwritten. The small moving manifest/signature
pair cannot be replaced atomically through GitHub's asset API; clients reject
mixed pairs during publication and retain verified support.

The manifest contract is deliberately bounded:

```json
{
  "schema": 1,
  "module_id": "enshrouded",
  "protocol": 1,
  "target": "windows-x86_64",
  "version": "1.0.0",
  "revision": 1,
  "profiles": [
    {"game_build_id": "25988414", "executable_sha256": "b627639ce4a3f8c940fe90afa1d36ed59db3ff9ced24e935890eb7c99b04d5bf"}
  ],
  "dll": {
    "asset_name": "Lgsm.Enshrouded.Control_1.0.0_windows-x86_64.dll",
    "sha256": "<final-DLL-sha256>",
    "size": 123456
  }
}
```

The example is explanatory metadata, not a signed release. Unknown fields are
rejected. Every release increments both the strict `major.minor.patch` version
and the positive revision; previously verified profiles must be retained. The
DLL limit is 1 MiB. The exact signed comments are
`lgsm-enshrouded-control-manifest:1` and
`lgsm-enshrouded-control-dll:<version>`.

### Detecting game updates

[track-enshrouded-build.yml](../../../.github/workflows/track-enshrouded-build.yml)
is a manual maintainer diagnostic (`workflow_dispatch`). It has no schedule,
client event submission or automatic publication. Run it when investigating a
reported failure. An unpublished feed is not evidence of compatibility: the
diagnostic requires a valid signed feed and reports an error if it is missing.
The workflow downloads Valve's
[official SteamCMD](https://developer.valvesoftware.com/wiki/SteamCMD), refreshes
anonymous app metadata for app `2278520`, reads `depots/branches/public/buildid`,
then compares it with the cryptographically verified published feed. A new build
opens one compatibility issue, deduplicated across open and closed issues. The
job retains a small detection report. Metadata/network/signature errors fail the
job instead of claiming that the game is supported.

Player-management compatibility is maintained manually in this repository when
a failure is reported; keeping every game build current is not a prerequisite
for opening or playing on a server. Detection does not alter profiles, sign or
release code. The maintainer must
obtain the official binary in isolation, inspect the native ABI/entry points,
run the C++/Rust tests, and verify real kick, ban and unban on the target build.
Keeping a signed updater cannot make an unknown native ABI safe automatically.

### Build, sign and publish a verified extension

The independent private key is a PKCS8 Ed25519 file in the publisher's protected
local directory. It must never be committed, uploaded, printed, or embedded in
the app. Preserve a secure backup of the established key; generating a different
root would require a desktop update before existing clients could trust it.
`scripts/enshrouded_control_package.cjs init` is a one-time provisioning command,
not a release step. Create and restrict its repository-external parent directory
to the publisher account before using it. No private key is needed for ordinary
development or verification.

Build into a new external directory with `build_enshrouded_control.ps1 -RunTests`.
After the actual moderation acceptance, save a private local receipt:

```json
{
  "schema": 1,
  "dll_sha256": "<final-DLL-sha256>",
  "profiles": "<the exact supported-builds.json array, not this placeholder>",
  "verified_actions": ["kick", "ban", "unban"],
  "evidence": "<local native and real-client acceptance evidence location>"
}
```

The receipt binds the exact compiled payload/profile inventory to the maintainer's
real-action evidence. It is not distributed with the public artifacts and does
not prove those actions automatically. Keep test account names, addresses and
credentials out of public release notes.

From the repository root, choose an unused external staging directory:

```powershell
$extensionStaging = 'C:\ReleaseStaging\enshrouded-control-1.0.0'
$env:LGSM_ENSHROUDED_SIGNING_KEY_PATH = Join-Path $env:LOCALAPPDATA 'LanGame\Publisher\EnshroudedControl\signing-key.pem'
node scripts/enshrouded_control_package.cjs package `
  --dll 'C:\ReleaseStaging\enshrouded-native\dbghelp.dll' `
  --profiles (Join-Path (Get-Location) 'modules\enshrouded\control\supported-builds.json') `
  --acceptance 'C:\ReleaseStaging\enshrouded-moderation-acceptance.json' `
  --version 1.0.0 --revision 1 --private-key $env:LGSM_ENSHROUDED_SIGNING_KEY_PATH --out $extensionStaging
python -B scripts/publish_enshrouded_control.py --directory $extensionStaging
```

The first command signs and independently verifies the package; existing staging
directories are refused. The second is a local verification/dry run and makes
no network requests. After the complete tested source is committed and available
on GitHub, publish it using that exact commit:

```powershell
python -B scripts/publish_enshrouded_control.py --directory $extensionStaging `
  --source-commit '<exact-40-character-source-commit>' --execute
```

For the first release, publish and verify this feed together with desktop 0.1.3.
Later manual fixes can ship as small signed extensions or with a desktop release.
A source push alone does not distribute a compiled fix to installed clients.
No periodic tracking job or automatic adaptation/publishing service is needed.

The publisher uses `GITHUB_TOKEN`/`GH_TOKEN`, or Git's configured credential
helper, without printing credentials. It verifies local signatures/checksums,
rejects revision rollback or mutation of existing versioned files, uploads and
reads back all immutable assets, publishes that version, and only then advances
the small signed feed. The immutable release includes the DLL/signature,
manifest/signature, `LICENSE.txt`, `LICENSE.minhook.txt` and `SHA256SUMS`; both
licenses are copied unchanged from source and linked from the release notes.
It rejects unexpected feed assets and concurrent feed
advancement. A damaged/incomplete existing feed fails closed for maintainer
repair unless it belongs to the current publisher's recorded interrupted update.
Before changing the feed, it atomically saves a publication intent beside the
staging directory, binding the original signed pair, exact target hashes/version/
revision and feed release ID. Repeating the same publication command recovers
missing, mixed or completed target files only when every remote file is either
the original bytes or the verified target bytes. It revalidates the immutable
target before recovery, rejects unrelated/newer content, and removes the intent
after successful feed verification. Keep that intent and staging directory until
publication succeeds. This first implementation uses GitHub directly; it does not provide a
regional extension mirror or an availability guarantee.

Run the maintenance regressions without private keys:

```powershell
node --test scripts/tests/enshrouded_control_package.test.cjs
python -B -m unittest scripts.tests.test_track_enshrouded_build scripts.tests.test_publish_enshrouded_control
```

The signed fixtures under `apps/desktop/src-tauri/test-data/enshrouded-extension`
use a discarded one-time test key and inert text payload. Their public key is
independent of production and cannot authorize a real extension.

## Identity and native calls

The native addresses differ between the two verified executable profiles:

| Build | `updatePlayers` | identity helper | `accountAction` | helper caller A | helper caller B |
| --- | --- | --- | --- | --- | --- |
| 25967645 | `0x7b33e0` | `0x9f54d0` | `0x79ba20` | `0x7b3aaf` | `0x7b489e` |
| 25988414 | `0x7b34c0` | `0x9f5bc0` | `0x79bb00` | `0x7b3b8f` | `0x7b497e` |

The transparent update observer enters the original `updatePlayers` after
restoring its original registers, flags, stack and return address. The native
helper has verified inputs RCX=output, RDX=session manager, R8D=full session handle,
and AL=0 for success. Its two original `updatePlayers` return sites are pinned
per profile. The second observer
calls the relocated original helper with these three inputs, then copies logical
identity fields and name bytes from the native caller's own deep-copy output.
It preserves native return registers/flags and ordinary unwind behavior. It never
retains native name pointers, reads the active snapshot tables itself, or installs
a publication hook.

At each update entry the bridge compares all 16 current server slots with its
private cache. Nonzero handles require phase byte 3, the complete full handle,
the same name, Steam platform byte 3, nonzero native account hash/platform ID,
fresh native observation within 1,000 ms, and no duplicate identity. Other occupied
phases and missing/unknown identities make `complete=false`; an all-empty slot
table is a complete empty observation. Phase 3 describes the native connected
state used here, and does not assert that a character has entered the world.
The Rust consumer independently verifies the platform ID to native hash mapping.

Every invalidation retires a bridge generation. A new observed identity gets a
new nonzero generation, including a reused full handle after an observed departure
or transition. Server/manager or owner-thread changes permanently disable authority
for that process. Native immunity is the server-slot permission byte AND **1**;
The native copied projection flag at +0x148 equal to 1 also sets immunity (2 does
not); both independent native guards are represented. No permission bits or
native immunity guards are modified.

`accountAction` has verified RCX=Server and RDX=0x18-byte event inputs
(native account hash at +8, action byte at +0x14: 0 kick, 1 ban, 2 unban). Only the
existing update owner thread calls it. Its own fresh session/hash and immunity
checks remain intact. Returning from this call means **dispatched** only. The
Rust caller confirms departure and the canonical native configuration; absence of
confirmation is an unknown/partial result and must not cause automatic redispatch.

## Protocol 1

All files are confined to `.langame-enshrouded-control`. Outputs are bounded to
64 KiB and written using plain `*.tmp` files and atomic replacement. After a crash,
the owning installer may retire the three plain owned `players.json.tmp`,
`response.json.tmp` and `status.json.tmp` files while the process is stopped. An
unexpected existing temporary file causes fail-closed behavior.

`players.json` has exactly these fields:

```json
{"protocol":1,"pid":123,"creation_time":"456","run_id":"0123456789abcdef0123456789abcdef","sequence":"10","tick_ms":"10000","complete":true,"issue":"none","players":[{"session_handle":64,"generation":"1","account_id":"7105925790711809417","platform":3,"platform_id":"76561197960265729","name":"Synthetic player","moderation_immune":false}]}
```

The example uses the independently verified synthetic platform/hash pair; it is not a real player account.
`creation_time` is the raw Windows creation FILETIME u64. `run_id` is 16 bytes
generated with `BCryptGenRandom`, serialized as 32 lowercase hex characters.
`sequence`, `tick_ms`, `generation`, `account_id` and `platform_id` are decimal
u64 strings without leading zeroes. `tick_ms` uses `GetTickCount64`. Sequence
increases at each native update, and the worker publishes the latest observation
every 100 ms while updates arrive. `issue` is `none` on a complete snapshot and a
bounded ASCII reason otherwise. The observer never performs file I/O.

`request.txt` is at most 1,024 ASCII bytes, LF-only, with this exact field order,
exactly one trailing LF and no extra/unknown/duplicate fields:

```text
LGSM_ENSHROUDED_CONTROL_1
request_id=0123456789abcdef0123456789abcdef
run_id=fedcba9876543210fedcba9876543210
action=ban
snapshot_sequence=10
session_handle=64
generation=1
account_id=7105925790711809417
expires_at_tick=15000
```

IDs are exactly 32 lowercase hex characters. Action is `kick`, `ban` or `unban`.
Numbers are unsigned decimal with overflow checking and no leading zeroes.
The runtime must bind requests to the selected run and original snapshot tick,
and publish them atomically. Online actions require a nonzero handle/generation,
a nonzero sequence no newer than current, a complete fresh current snapshot,
the same current handle/generation/hash, and no immunity. Offline unban requires
handle, generation and sequence all zero, a nonzero account hash, and a fresh
live bridge observation. Expired requests or expiry over 15 seconds in the future
are rejected. The Rust caller further limits expiry to the selected tick +15s
and current tick +5s.

There is one outstanding queue slot and 16 history slots. Pending/dispatching
records are retained; completed records are reusable only after their request
expiry. When all slots are occupied, new requests are rejected rather than
evicting an unexpired nonce. Run identity and bounded expiry are checked before
reserving history. Identical IDs reuse the recorded response; changed payloads
with the same ID are rejected. Busy queues reject rather than overwrite.
Stopping retires pending requests. Only the native owner callback dispatches.

`response.json` has exactly:

```json
{"protocol":1,"pid":123,"creation_time":"456","run_id":"0123456789abcdef0123456789abcdef","request_id":"fedcba9876543210fedcba9876543210","action":"ban","account_id":"7105925790711809417","status":"dispatched","issue":"none"}
```

Moderation status is `dispatched` or `rejected`; immediate world saves may also
return `completed` after the exact native persistence operation succeeds. Rejection
includes a bounded ASCII `issue`.
`status.json` is separate diagnostic metadata and includes the pinned executable
digest. It carries no player names, platform IDs or account hashes. Players and
responses are private protocol data, not audit/log records.

## Build and verification

```powershell
.\scripts\build_enshrouded_control.ps1 -OutDir C:\absolute\external\build -RunTests
```

The MSVC x64 Build Tools are required. All objects, executables and `dbghelp.dll`
go to `OutDir`, which must be outside the source repository. Original bridge
sources use `/W4 /WX`; untouched upstream MinHook is built with `/W4` and retains
its existing warnings. The tests exercise the original ABI via real MinHook
trampolines, flags/return values, virtual unwind points, strict protocol parsing,
cache generations, stale/changed targets and immunity. Synthetic tests do not
prove actual player moderation; isolated native verification is a separate step.

Once enabled, the module and all installed hooks/trampolines stay pinned until
process exit. `EnshroudedControlStop` stops sampling/worker and joins the worker;
it does not unload installed hooks. The owning runtime removes/replaces the
payload only after the owned process actually exits.

Each actual MinHook trampoline is checked against its fixed 5/6-byte relocated
prefix and 14-byte absolute handoff. A private read-only V2 unwind record describes
only the relocated PUSH/ALLOC effects, without falsely declaring that handoff an
epilogue. V1 incorrectly treats this FF25 as an already-restored tail epilogue;
simply increasing the prologue size does not repair that behavior. V2 encoding
is cross-checked against the [LLVM emitter](https://llvm.org/doxygen/MCWin64EH_8cpp_source.html).
Before enabling either hook, every byte offset of both real trampolines is looked
up with `RtlLookupFunctionEntry` (exact registered table/base required) and tested
with `RtlVirtualUnwind` against synthetic caller RIP/RSP/RBX/RBP. An unsupported
unwinder or any registration/protection/lookup failure disables control while the
original server continues. Registered tables and metadata remain until process
exit, including failed setup after a successful registration.

## Third-party source

Only [official MinHook v1.3.4](https://github.com/TsudaKageyu/minhook/tree/c3fcafdc10146beb5919319d0683e44e3c30d537)
is vendored, at commit `c3fcafdc10146beb5919319d0683e44e3c30d537`. Its original
[LICENSE.txt](vendor/minhook/LICENSE.txt) includes the MinHook/HDE BSD notices;
[SOURCE-MANIFEST.json](vendor/minhook/SOURCE-MANIFEST.json) records upstream Git
blob IDs and SHA256 hashes. It contains no downloaded precompiled DLL. The bridge
and native layout handling were independently implemented from verified binary
facts; no unlicensed third-party connector implementation was incorporated.

## Immediate world saving (2026-10-20)

Read-only inspection of the pinned build `25967645` executable
(`4de2acdd3b18e562f129bc488a2daea2da16b00341497304423e1e18c49a83ba`)
identifies `ForceSave`, `keen::ecs::ForceSave` and
`keen::ds::ecs::ForceSave` as ECS type metadata. These strings do not establish a
callable force-save function or a server command.

Further inspection identified the engine's actual save-request entry at RVA
`0x7ada80`, with ABI `void(Server*)`. Its ordinary autosave caller is `0x7b14df`;
the same entry is used on a player's departure (`0x7b294a`). The separate shutdown
path also calls this helper, but the helper itself neither disconnects players
nor stops a world. It preserves native save admission and queues native
serialization; the bridge never writes native save flags or copies live files.
The existing verified `updatePlayers` observer supplies its owning `Server*` and
owner thread. The exact executable hash and all 32 entry bytes must match before
this save entry becomes available. Build `25988414` retains its separately
verified moderation support but has no admitted save profile yet.

### Build 26083076 compatibility (2026-10-21)

The current executable is not admitted by adding its hash alone. Its PE timestamp
is `1792507668` and image size is `31264768`, rather than the earlier `31256576`.
The inspected entry RVAs are `updatePlayers=0x7b5800`, `localCopy=0x9f7600`,
`accountAction=0x79de40`, and `saveWorld=0x7afea0`; the two copied-identity return
sites are `0x7b5ecf` and `0x7b6cbe`. The save completion windows moved to
`0x7bb58c` and `0x7b97fe`. The latter has a changed RIP-relative displacement;
its complete new 32-byte signature is pinned separately, not masked at runtime.

An offline Capstone 5.0.7 comparison checked all 1,261 update instructions,
400 account-action instructions, 36 copy instructions, 21 save-entry instructions,
1,057 persistence-state-machine instructions and 129 result instructions against
build 25967645. Only direct-call/RIP relocation and the explicitly compared switch
table RVAs were normalized; internal branches and both switch target offsets
also match. The bridge's Server/save-context/World bindings and flags/phase/
request-sequence offsets remain unchanged. A deeper engine persistence callee
does have shifted internal fields; the bridge never reads that opaque object and
continues to call the game's own implementation.
The copied-identity record, full-handle enumeration and native identity-hash
assignment were also compared and retain their existing layouts.

`native_profile_tests.exe <official-executable-path>` constructs an inert virtual
PE image without executing game code. Both 25967645 and 26083076 pass their exact
identity and native-entry checks. Altering the PE size or a moderation entry
rejects the profile; altering the save, commit or result window disables saving.
The ordinary unknown test executable is rejected. The MSVC `-RunTests` suite also
retains the observer ABI, trampoline unwind, protocol, generation and request
history tests. These checks do not constitute real-client moderation acceptance
on the new build or publication of a signed extension release.

The current production DLL also passed two isolated native save cycles on this
build. Each current `save_world` request was published once, returned a matching
`completed/none` receipt and changed the native `world0.ksd` digest while the game
kept running. Both native Ctrl+C shutdowns exited with code 0. A third fresh
process loaded the exact 42,833-byte snapshot produced by the second manual save
from a separate task-only save directory, reached native `Run`, and retained the
same digest through loading without receiving a save request; it also exited
normally with code 0. No user configuration, existing instance or user save was
used or changed. This checks native persistence and exact-byte recovery for the
newly generated zero-player world; it does not establish player-content recovery,
power-loss durability, real-client moderation or deployment to an existing host.

`world-save.json` is a separate protocol-1 snapshot containing `pid`, decimal
string `creation_time`, `run_id`, decimal strings `sequence`, `tick_ms` and
`generation`, boolean `ready`, and ASCII `issue`. The opaque generation retires
whenever the native Server/save context/World binding changes or unloads.
Readiness requires a current World, native saving enabled and initialized, no
pending save and no shutdown. A save request uses the existing bounded request
envelope with `action=save_world`, `snapshot_sequence` and `generation` from this
fresh snapshot, and `session_handle=0`/`account_id=0`. Moderation validation cannot
authorize this action. Native owner-thread validation binds it to the same run,
current world generation and readiness before invoking the native helper once.
The shared bounded history makes identical request IDs replay their original
receipt; a timeout must never automatically resend.

The native helper's void return establishes admission only. A save now retains
its single-flight request and exact Server/context/World generation plus the
native context's request sequence (`+0x80`) until native completion. Immediately
after the one call, the admitted state must be phase 1 / flags 7. Native
serialization and persistence remain in the engine (`0x7b7330` leading to
`0x7b80d0`); phase 5 calls the native persistence completion check (`0x730820`)
at `0x7b9180`, and only its successful result advances to phase 6 and then idle.
The success branch at `0x7b745b` clears pending to phase 0 / flags 6. Native
terminal failure clears pending while retaining error bit 8, so it cannot
produce a completed receipt. The executable hash and loaded 32-byte windows at
the request, commit-check and result branches must all match.

The observer must actually see that same operation at phase 6 / flags 7 before
accepting its return to phase 0 / flags 6. Merely observing idle after admission
could also represent an unseen reset and stays unconfirmed. A skipped phase-6
sample is not inferred from timing or file changes. Only this full sequence yields
`status=completed, issue=none` and a confirmed desktop result. Native error bit 8
is an error; a changed context, generation, sequence, shutdown, invalid state or
120-second observation timeout remains unconfirmed. The bridge never repeats
the save. A dispatched receipt, a void return, a newer file timestamp or an
adjacent log line never establishes completion. This follows the native
persistence contract; it makes no extra power-loss durability guarantee.

Historical isolated acceptance on 2026-10-19 (before the completion-receipt observer)
used ordinary copied program files and a newly generated
private `world0.ksd`, without copying user configuration or saves. The production
DLL received one `save_world` IPC request on build `25967645`; native flags moved
from 6 to 7 and native save phase from 0 to 1, then returned to idle phase 0/flags
6. The current 42,750-byte world changed SHA256 and modification time while the
server remained running. A subsequent fresh process deserialized that exact
saved file and reached `Run` without another save request. This is actual native
save/restart evidence, distinct from the synthetic protocol and generation tests.

The 2026-10-20 completion observer passed the MSVC control test suite, including
admission versus completion, phases 1-6, terminal error, changed request sequence,
changed generation, a missed phase-6 observation, reset after commit and deadline
cases. A newly built production acceptance executable then ran two isolated
native cycles against the read-only local package: ordinary Start installed the
extension automatically, each formal `save_world` action returned confirmed only
after the native phase-6 boundary, and both normal Stops exited with code 0.
The second cycle used a fresh managed process and passed native readiness again.
Independent process and port checks found both native processes, the fixture,
the test process and its console host gone after cleanup. This verifies the new
native completion path; it does not add player-content or power-loss claims.
