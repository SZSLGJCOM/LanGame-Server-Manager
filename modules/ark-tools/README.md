# Fixed ARK tool dependencies

[`dependencies.json`](dependencies.json) is the authoritative runtime download
inventory for ASE ArkApi 3.56 and ASA AsaApi 2.11. Each archive has a fixed byte
length, SHA-256, official upstream URL, domestic mirror URL and upstream SDK
license at an immutable source commit. The runtime and publisher share the
32 MiB per-archive limit. Updating a package requires reviewing its identity,
license inputs and installer compatibility together.

The published mirror uses the existing `SZSLGJCOM/LanGame-Server-Manager-Releases`
repository and the dedicated
`ark-tools-dependencies-ase-3.56-asa-2.11` release. This release remains `pre`;
it does not change the desktop release's status or update pointer. A URL in the
inventory is not evidence that an attachment has been published: the publisher's
successful receipt requires a complete anonymous download and matching hash.

The [2026-11-09 publication record](publication-2026-11-09.json) confirms all seven
attachments passed full anonymous download and SHA-256 checks on the publisher
and a separate Windows host. Both ZIP archives also passed CRC checks; the ASA
framework DLL matches the installer's pinned hash.

## Prepare and publish

Run from the repository root with Python 3.11 or later and an artifact directory
outside the repository. Preparation downloads only public upstream resources;
it does not install, load or execute the downloaded binaries.

```powershell
$artifactDirectory = Join-Path $env:TEMP 'lgsm-ark-dependencies-reviewed'
python -B -E -s scripts/publish_ark_tools_dependencies.py --work-dir $artifactDirectory
```

The default is a dry run. Review `prepared.json`, both unchanged ZIP archives,
`ASE-LICENSE.txt`, `ASA-LICENSE.txt`, `THIRD_PARTY_NOTICES.txt`, the frozen copy
of `dependencies.json` and `SHA256SUMS`. The checksum file covers the other six
attachments. Preparation refuses to overwrite conflicting files, including
earlier preparation evidence. When notice inputs change, preserve the previous
directory and prepare in another one; copies of already hash-verified ZIPs may
be used to avoid downloading them again.

An authorized publishing process provides `GITCODE_RELEASE_TOKEN` only through
its child environment, then invokes the same script with `--execute` and the
same reviewed directory. Do not put credentials in command arguments or files.
The existing desktop mirror's `Transport` supplies authenticated API requests,
signed uploads and anonymous downloads. GitCode is direct by default; GitHub
uses the process's existing approved proxy. This script does not alter proxy
settings, credentials, desktop release jobs or their state.

```powershell
python -B -E -s scripts/publish_ark_tools_dependencies.py --work-dir $artifactDirectory --execute
```

Use one publisher for this dedicated release. Each attempt records its intent
and fresh readback files under `attempt-*`. All existing same-name attachments
must pass complete anonymous size/hash verification before any missing
attachment is appended. There is no delete, replace, promote or retry-write
operation. Only successful readback produces `receipt.json` and a `verified`
`publish-state.json`.

After interruption, retain and reuse the same work directory. `publish-state.json`
records a pending release creation or upload before it is attempted. On resume,
the publisher first queries remote state and verifies any matching attachment.
If a previous mutation is still unresolved, it stops without repeating it.
A stale `publisher.lock` requires checking the recorded PID and pending state
before removing that task-owned lock; elapsed time alone does not resolve an
upload. A new work directory must not be used to bypass unresolved state.

## Attribution and verification

The original archives contain no full license/notice documents. Their SDK MIT
licenses do not relicense every included binary. The generated notices preserve
the fixed upstream project licenses, Permissions and loader notices, the
OpenSSL 3.2.1 license, and the licenses of fmt, Poco, Detours, zlib and minizip
from the fixed ASA source's declared vcpkg baseline. Existing incorporated
header notices remain separately identified. Microsoft DIA/C++ components
retain Microsoft's applicable terms and official redistribution references.
Source declarations and binary version observations are distinguished from
reproducible-build evidence.

Focused tests use isolated fixture archives and a simulated remote boundary:

```powershell
python -B -E -s -m unittest scripts.tests.test_publish_ark_tools_dependencies
```

These tests cover manifest rejection, unchanged ZIP preparation, checksum
inventory, conflicting attachments, immutable local evidence, full readback,
lost responses and unresolved mutation recovery. Actual publication additionally
requires the live anonymous readback recorded in the receipt.

## Executable-bound ASA symbols

[`symbol-caches.json`](symbol-caches.json) separately pins reviewed runtime
symbol ZIPs. The current entry is for Steam build 26564816, executable SHA-256
`79b0ce800a05e254f0a591967acea9ae0fcd0c03becf75ab04182e2c3cd0ab02`.
Its original 23,155,786-byte archive contains 704,423 offset entries, 13,055
bitfield entries and the upstream symbol-name text. No game executable, content,
world or credentials are included. The [dated publication record](symbols-publication-2026-11-12.json)
records all five attachments passing full anonymous size/hash verification.

Chinese requests prefer the reviewed GitCode cache. International requests
prefer the three existing maintainer CDNs, then GitCode. Known images enforce
the same reviewed archive size/hash on every route; unreviewed images retain
the upstream-only lookup. All routes retain executable binding, ZIP/map bounds,
atomic generation selection and existing-cache preservation. Ordinary startup
and status inspection still do not download symbols.

Prepare an individual reviewed cache outside the repository:

```powershell
$symbolWork = Join-Path $env:TEMP 'langame-asa-symbols-26564816'
python -B -E -s scripts/publish_ark_symbol_cache.py --work-dir $symbolWork --executable-sha256 79b0ce800a05e254f0a591967acea9ae0fcd0c03becf75ab04182e2c3cd0ab02
```

Review `prepared.json`, the unchanged ZIP, `symbols.json`, `SHA256SUMS`,
`ASA-SDK-LICENSE.txt` and `THIRD_PARTY_NOTICES.txt`. The SDK notice preserves its
MIT attribution without relicensing the proprietary game or every derived
symbol name. Authorized publication uses the existing child-only
`GITCODE_RELEASE_TOKEN` entry and the same command with `--execute`. It reuses
the dependency publisher's immutable attachments, pending-write recovery and
anonymous full readback. The content-addressed symbols release stays `pre`;
it does not promote or update the desktop application. A new reviewed ZIP
requires a new content-addressed tag, never replacing an existing attachment.

`scripts.tests.test_publish_ark_symbol_cache` checks identity, map/ZIP rejection
and the shared publication path. The ignored Rust test
`asa_reviewed_symbol_mirror_prepares_owned_copy` additionally accepts
`LANGAME_ARK_SYMBOLS_NATIVE_EXE`, copies that executable read-only to its own
fixture and downloads the reviewed ZIP through GitCode, publishes a local cache
generation and reopens it for verification. It does not publish a remote release,
start a server or edit an existing cache; a fallback does not pass mirror verification.
