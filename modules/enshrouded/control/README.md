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

| Steam public build | Executable SHA256 |
| --- | --- |
| 25967645 | `4de2acdd3b18e562f129bc488a2daea2da16b00341497304423e1e18c49a83ba` |
| 25988414 | `b627639ce4a3f8c940fe90afa1d36ed59db3ff9ced24e935890eb7c99b04d5bf` |

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

Before starting an Enshrouded instance, the manager hashes that instance's actual
server executable and checks the independent signed compatibility feed. It can
install a matching extension while the owned server is stopped, then start the
server. It never replaces a DLL in a running process. Network failures can fall
back to previously verified exact-build support. An unknown executable remains
read-only, with the reason shown in the player controls. Starting an instance is
the automatic update trigger; already running servers receive their extension
update at their next normal stop/start.

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
runs every four hours only when the repository Actions variable
`ENABLE_ENSHROUDED_COMPATIBILITY_TRACKER` is set to `true`; an absent or
disabled variable leaves scheduled jobs skipped. Manual `workflow_dispatch`
remains available. Keep the variable unset until the signed production feed is
published with desktop 0.1.3, then enable it as part of that release. An unpublished
feed is not evidence of compatibility; a manual run still requires a valid signed
feed and reports an error if it is missing. The workflow downloads Valve's
[official SteamCMD](https://developer.valvesoftware.com/wiki/SteamCMD), refreshes
anonymous app metadata for app `2278520`, reads `depots/branches/public/buildid`,
then compares it with the cryptographically verified published feed. A new build
opens one compatibility issue, deduplicated across open and closed issues. The
job retains a small detection report. Metadata/network/signature errors fail the
job instead of claiming that the game is supported.

Detection does not alter profiles, sign or release code. The maintainer must
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
Then set repository **Settings → Secrets and variables → Actions → Variables →
`ENABLE_ENSHROUDED_COMPATIBILITY_TRACKER`** to `true` to activate scheduled
tracking. Pushing this workflow alone does not enable its scheduled job.

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

Status is `dispatched` or `rejected`; rejection includes a bounded ASCII `issue`.
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
