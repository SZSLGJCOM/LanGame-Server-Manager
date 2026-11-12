# ARK server tools native extension

`LgsmArkTools.cpp` is LanGame project code, governed by the repository license.
`asa_version.cpp` adapts the MIT licensed ArkServerApi loader and preserves its
attribution in `THIRD_PARTY_NOTICES.txt`. These community APIs are maintained by
ArkServerApi; they are not a Studio Wildcard API distribution.

The build downloads fixed archives, verifies SHA-256 before extracting selected
headers/import libraries, and compiles the three DLLs into Cargo's build output.
The SDK source and game symbols are not bundled into the desktop application.
No downloaded build script is executed. Framework binaries are downloaded by
the separately reviewed instance installer, rather than embedded in this code.

| Input | Fixed source | Archive SHA-256 |
| --- | --- | --- |
| ASE SDK 3.56 | [AseApi 9459cb3941c2e62d4144cbb8294fe6c4c772d564](https://github.com/ArkServerApi/AseApi/tree/9459cb3941c2e62d4144cbb8294fe6c4c772d564) | `500cc0e0d46e75e3c67374a81914fe318ee1f88db8bca0c291f2e8a72ede3def` |
| ASA SDK 2.11 | [AsaApi bd376a0422291b0b1bf15a0b7868e7e275605da9](https://github.com/ArkServerApi/AsaApi/tree/bd376a0422291b0b1bf15a0b7868e7e275605da9) | `e179b1de7037ea48e69531e4aac75e02126fa63a8054cc32236f818a20ea95bc` |
| ASA import library | [AsaApi 2.11 release, AsaApi_2.11.zip](https://github.com/ArkServerApi/AsaApi/releases/tag/2.11) | `64f5f1165ef73f3439bba3794e77e379489167cc385c0ac5c4717ca60ad3e88a` |
| ASA version proxy export stubs | [AsaApiLoader 4e04dc8982e1473920278f199638b29dc511d539](https://github.com/ArkServerApi/AsaApiLoader/tree/4e04dc8982e1473920278f199638b29dc511d539) | `dd3c6ac17d2bdc9d072ca01c8e86f9451e6de909c2bf901bb5b2c63cb939e833` |

The exact archive URLs are in `scripts/build_ark_tools.ps1`. ASA's public
`UnrealString.h` includes `fmt/format.h`, while its logger includes bundled
fmt 4.1.0. The script exposes that same bundled version at the expected include
path; it does not introduce another fmt version. SDK third-party header notices
remain intact in the build cache. The notices shipped with the plugin include
the incorporated JSON, spdlog, and fmt copyrights and licenses.

ASA's UE 5.8.3 support starts with the 2.1 SDK and framework. That SDK resolves
`APrimalWorldSettings` through `AsaApi::ResolveClass` and updates the native
`SpawnDino` and `TameDino` signatures; rebuilding is required for plugins compiled
against 2.03. The current 2.11 SDK also corrects the UE 5.8 object-array layouts,
so this bundle pins the 2.11 SDK and framework together and requires API 2.11
in its plugin metadata. The import library, executable-bound cache format and
generation selection are unchanged from 2.1. Intact managed 2.03 and 2.1 installs
remain recognizable for explicit upgrades; unknown or modified files are
still protected by ownership and byte-hash checks.

Build prerequisites are Windows x64, Visual Studio C++ Build Tools with MSVC
C++20 and a Windows 10 or later SDK. The script discovers these through
`vswhere` and the Windows Kits registry. Release users do not need a compiler.
The build emits `LgsmArkTools-ase.dll`, `LgsmArkTools-asa.dll`, and
`asa-version.dll`. The installer selects an edition and deploys the plugin as
`ArkApi/Plugins/LgsmArkTools/LgsmArkTools.dll` with its metadata and notices.

The tool page keeps installation detection separate from runtime connection checks.
Installation detection verifies the recorded ownership and file hashes, so an
intact plugin from a previous manager build remains installed after reentry.
An explicit check/update prepares the current bundle and, for ASA, symbols for
the selected server executable. Tool protocol availability is checked separately
by the RCON handshake after the server starts.
Removal requires the entire instance to be stopped, and validates the recorded ownership
and file hashes before changing files. It removes the LanGame plugin; when other
plugins are present, their shared API framework remains. Otherwise the unchanged
framework files installed by LanGame are removed too. Saved worlds, creatures,
unknown files and framework caches are retained. The ownership record remains
as installation provenance so a later install can reuse the retained directories.
A bounded removal journal preserves originals through interruption or rollback;
an unfinished transaction prevents server startup until recovery completes.

The protocol uses authenticated RCON commands `LgsmArkTools.Status`, `Spawn`,
`Inspect`, and `Players`, with a 32-character hexadecimal request ID and JSON replies
prefixed by `LGSM_ARK_TOOLS `. Spawn calls the native world-coordinate API,
verifies the returned dino ID through `FindDinoWithID`, and reads the actual
class, base level, team, tame state, and position. Taming requires a matching
online player controller and skips bonus tame levels. Short class names resolve
only loaded classes; unloaded creatures require their full `/Game/..._C` path.
Mutation receipts remain bounded to 512 per plugin lifetime; a full table
rejects additional mutations instead of evicting an ID and risking a duplicate
spawn. Restarting the server clears this in-memory table.

`Players` is a read-only world-tick snapshot of authenticated player controllers,
not saved or sleeping characters. The in-game `LinkedPlayerID` is kept separate
from Steam/EOS account strings. Each player record keeps the survivor's `name`
separate from `accountName`, the platform name from the SDK's `GetSteamName`.
Both names use explicit UTF-8 conversion (ASA `ToStringUTF8`, ASE `ToString`),
then JSON Unicode escapes preserve them through the engines' RCON conversions.
The host can match `accountId` to native `ListPlayers` rows to restore names
without changing online membership, account identities, or action bindings.
Characters without a current position have null
XYZ values. Collection is limited to 512 players and 400 KiB of JSON; replies
use UTF-8-safe RCON fragments of at most 8192 bytes. Reads do not consume mutation
receipts, install extensions or retry mutations.

The host binds map reads and all map tools to the selected process key and its
own `run_id`, not the cluster's primary run. Its registered RCON port must belong
to that process identity. Snapshot map names must match the configured native
map; switching maps or restarting a secondary map invalidates old requests.

Map projections follow the pinned SDKs' `Ark/ArkApiUtils.h::FVectorToCoords`:
ASE uses native longitude/latitude scale and origin; the stock Island alone
receives the legacy defaults for missing fields. ASA uses the sole native
`FMapData` origin rectangle, including the game's default minimap-data class.
Missing, invalid, or multiple-layer metadata falls back to world XY coordinates,
with Z retained as height. No guessed per-map dimensions are shipped.

The inspected 2026-10-25 dedicated packages (ASE build 21241282 and ASA build
26068782) strip the tested map textures' platform pixel data. The UI therefore
loads reviewed ARK Wiki map artwork automatically by edition and native map
name. `apps/desktop/src/data/ark-map-artwork.json` records the image filename,
decoded dimensions, full-image GPS bounds, verification date, and pinned Wiki
Data:Maps revision for each entry. The 2026-10-25 catalog contains 10 ASE and
9 ASA entries. ASE uses topographic maps where calibrated sources exist;
ASA uses the corresponding in-game minimap artwork. Cross-edition and unknown
map names never select an approximate substitute.

The bounds describe the complete image in longitude (X) and latitude (Y),
including any border, rather than a pixel crop. They follow DataMaps' explicit
background `at` bounds or its default GPS 0–100 coordinate system. These image
bounds never change the native player projection or command coordinates.
Fjordur and Genesis Part 1 remain grids because the player protocol cannot
identify their overlapping realms/biomes. Aquatica has no reviewed calibration.
Gen2 uses its single full-map plane; interiors and caves are not separate layers.
An explicitly unavailable native projection always uses world XY without artwork.

Image bytes are fetched on demand through the existing bounded media cache;
only catalog image paths are admitted by `crates/app-network/official-sources.json`.
The reviewed images are published in versioned GitHub/GitCode resource directories,
with source notices; they are not embedded in the frontend or installers. Each
image links to its Wiki file page for attribution and applicable terms; see
`THIRD_PARTY_NOTICES.md` and `docs/ark-map-artwork-distribution.md`.
Cached artwork is reusable after reopening. Missing artwork, download failures,
or changed dimensions leave the coordinate grid usable, with manual refresh
available even when the server is stopped. Map changes discard obsolete loads.

The 19 published GitHub/GitCode mirror pairs are registered as image-only
`exactResources` with a frozen SHA-256. Chinese UI requests prefer the domestic
origin and English UI requests prefer the international origin. Mirror aliases
share one cache identity containing that digest; a language change does not
invalidate fresh cached bytes. The original Wiki image remains a separate final
source because its CDN can re-encode image bytes. Mirror downloads must pass the
native digest check before caching and cannot bypass that check through a direct
browser request. Every decoded image must also match the reviewed dimensions
before it is accepted; invalid candidates advance to the next source.

Published artwork explicitly enables `cacheImmutable` in the exact resource
policy. This is application asset storage: GitCode sends `no-store` even for
these public frozen files, so successful hash-verified downloads are retained
in the existing bounded LRU until eviction or a digest change. The opt-in requires
an image digest and partitions its identity from ordinary HTTP-cache entries.
Original response headers are preserved; other media still honor HTTP cache rules.

`scripts/prepare_ark_map_artwork.py` prepares the original images and source
notices for review. `docs/ark-map-artwork-distribution.md` describes the package
and proposed hosting paths. Mirror URLs are enabled only after publication and
anonymous readback verification; preparing a package does not publish it.

Optional local PNG overrides remain scoped to edition and native map name.
Imported art must cover the full GPS 0–100 rectangle without borders. Removing
an override restores automatic artwork; an override never determines command
coordinates. The default workflow requires no manual image import.

Verification covers both pinned SDK compilations, bounded protocol parsing,
fragmented RCON responses, process/run admission, browser map/tool flows,
catalog/cache-policy parity, image failure and map-switch handling, and actual
Wiki downloads surviving a native cache reopen. Browser artwork checks use
controlled player snapshots, with downloaded Wiki JPEGs for visual evidence.
These checks do not replace a real connected-player test on each game build.

Explicit ASA extension preparation obtains an executable-hash-matched offset
cache. Reviewed images in [`symbol-caches.json`](symbol-caches.json) have a fixed
ZIP size/SHA-256 and a published GitCode mirror: Chinese requests try that mirror
first; international requests try the three maintainer CDNs before the mirror.
Unknown executable hashes retain the three upstream sources without inventing
a mirror address. Every source for a reviewed image must match its pinned ZIP;
all archives still pass bounded ZIP/member and offset/bitfield-map validation
before a complete generation is atomically selected. Existing caches remain
available if preparation fails. Requests contain the executable's SHA-256,
not its contents or local path. The [publication record](symbols-publication-2026-11-12.json)
binds the current build 26564816 cache to complete anonymous mirror readback.

Ordinary startup does not download symbols. After program updates, the manager
refreshes its owned ASA proxy and configuration through the existing installation
transaction, including a framework retained after plugin removal. It preserves
plugin bytes, removed-plugin state, saves and caches. Modified or unowned files
are not replaced. The proxy keeps system Version.dll forwarding available and
initializes the optional framework only with a locally verified, executable-bound
cache and automatic downloads disabled. It holds the selected files while the
framework reads them. Missing or incompatible cache data disables the extension;
it does not turn startup into a symbol download wait.

AsaApi 2.11 passed an isolated native run on Steam build 26477280 on
2026-11-09: installation, plugin handshake, GM commands, map snapshot, three
wild-creature spawn/readbacks, and a normal save/stop/restart with the same native
creature IDs. The [dated acceptance record](../arksurvivalascended/reference-fixtures/2026-11-09-managed_native_creature_persistence_asa_2_11_build_26477280.json)
binds the exact build and installed hashes to those results. This run used a new
world; it does not establish existing-world migration or client connectivity.

A further isolated run on 2026-11-12 passed on Steam build 26564816; both native
server logs directly report ARK version 97.21. The existing pinned AsaApi 2.11
SDK and framework passed production executable-bound symbol preparation,
handshake, GM responses, map snapshot, and three wild-creature spawn/readbacks.
After a primary-world save request and normal stop, a new managed run read back
the same Dodo, Rex and Enigmasaur native IDs, classes, levels, wild states and
teams without replaying Spawn. Both stops exited 0 and owned cleanup passed.
The [new dated acceptance record](../arksurvivalascended/reference-fixtures/2026-11-12-managed_native_creature_persistence_asa_2_11_build_26564816.json)
binds the package, frozen sources, frontend commands, test binary and raw-log
hashes. Initial startup included Windows runtime preparation/download and a
non-elevated firewall timeout warning; individual delays are not fully
attributed. This fresh-world result does not establish external connectivity,
live-player taming, populated-world migration or gameplay defaults. The earlier
build-specific acceptance and failed preparation records remain historical.
