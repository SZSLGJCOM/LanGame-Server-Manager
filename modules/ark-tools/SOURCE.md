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
| ASA SDK 2.03 | [AsaApi 86bd3b21d1e940fe837af6511f945944977bb9ca](https://github.com/ArkServerApi/AsaApi/tree/86bd3b21d1e940fe837af6511f945944977bb9ca) | `01b21af8a98265b273bc1fb3b264f2bfdff0e603a1f6bb7cc1616fbf0d27ee30` |
| ASA import library | [AsaApi 2.03 release, AsaApi_2.03.zip](https://github.com/ArkServerApi/AsaApi/releases/tag/2.03) | `ac72fb29436198ac062cd273e1c496b1ef4e6ffddeec08243d11d9b35e8b8ae3` |
| ASA version proxy export stubs | [AsaApiLoader 4e04dc8982e1473920278f199638b29dc511d539](https://github.com/ArkServerApi/AsaApiLoader/tree/4e04dc8982e1473920278f199638b29dc511d539) | `dd3c6ac17d2bdc9d072ca01c8e86f9451e6de909c2bf901bb5b2c63cb939e833` |

The exact archive URLs are in `scripts/build_ark_tools.ps1`. ASA's public
`UnrealString.h` includes `fmt/format.h`, while its logger includes bundled
fmt 4.1.0. The script exposes that same bundled version at the expected include
path; it does not introduce another fmt version. SDK third-party header notices
remain intact in the build cache. The notices shipped with the plugin include
the incorporated JSON, spdlog, and fmt copyrights and licenses.

Build prerequisites are Windows x64, Visual Studio C++ Build Tools with MSVC
C++20 and a Windows 10 or later SDK. The script discovers these through
`vswhere` and the Windows Kits registry. Release users do not need a compiler.
The build emits `LgsmArkTools-ase.dll`, `LgsmArkTools-asa.dll`, and
`asa-version.dll`. The installer selects an edition and deploys the plugin as
`ArkApi/Plugins/LgsmArkTools/LgsmArkTools.dll` with its metadata and notices.

The protocol uses authenticated RCON commands `LgsmArkTools.Status`, `Spawn`,
and `Inspect`, with a 32-character hexadecimal request ID and JSON replies
prefixed by `LGSM_ARK_TOOLS `. Spawn calls the native world-coordinate API,
verifies the returned dino ID through `FindDinoWithID`, and reads the actual
class, base level, team, tame state, and position. Taming requires a matching
online player controller and skips bonus tame levels. Short class names resolve
only loaded classes; unloaded creatures require their full `/Game/..._C` path.
Mutation receipts remain bounded to 512 per plugin lifetime; a full table
rejects additional mutations instead of evicting an ID and risking a duplicate
spawn. Restarting the server clears this in-memory table.

ASA framework startup obtains an executable-hash-matched offset cache from its
maintainer CDN. That network dependency is separate from compilation and must
be disclosed by the install flow. The proxy forwards the system Version.dll
exports and initializes the adjacent framework without replacing the original
game executable. It checks long-path buffer bounds before loading either DLL.
