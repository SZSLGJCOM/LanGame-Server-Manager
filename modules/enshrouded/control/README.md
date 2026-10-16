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

The supported executable SHA256 is
`4de2acdd3b18e562f129bc488a2daea2da16b00341497304423e1e18c49a83ba`.
Both the on-disk digest and loaded PE/entry bytes must match. Unknown builds retain
dump forwarding and do not install control hooks. No game binary is distributed.

## Identity and native calls

The transparent update observer enters the original `updatePlayers` at RVA
`0x7b33e0` after restoring its original registers, flags, stack and return address.
The native helper at RVA `0x9f54d0` has verified inputs RCX=output, RDX=session
manager, R8D=full session handle, and AL=0 for success. Its two original
`updatePlayers` return sites are RVAs `0x7b3aaf` and `0x7b489e`. The second observer
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

`accountAction` RVA `0x79ba20` has verified RCX=Server and RDX=0x18-byte event inputs
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
