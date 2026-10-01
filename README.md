# LanGame Server Manager

[English](README.md) | [简体中文](README.zh-CN.md)

LanGame Server Manager is a Windows desktop application for installing, configuring, and managing dedicated game servers. Server files, settings, logs, and backups stay on your machine.

If you enable a remote AI provider, selected conversation and diagnostic context is sent to that provider. See the [assistant data-handling guide](docs/assistant.md#english) for the included data and redaction boundaries.

Manage a co-op world or several game servers from one workspace, with native game settings and separate files and ports for each instance.

## Main features

- Install and update dedicated-server packages through each supported game module.
- Create instances with separate ports, native configuration and saves, sharing program files only where the game supports it.
- Start, stop, inspect, and back up servers from one desktop workspace.
- Inspect and operate ARK clusters, with stopped-group snapshots and recoverable restoration.
- Set optional CPU and committed-memory limits for each instance's complete process group.
- Close the Windows window to the system tray while servers keep running; restore the interface or safely stop the runtime from the tray.
- Query players and run the administrator actions supported by each game.
- Use **LAN**, the optional [AI assistant](docs/assistant.md#english), with a configured local or remote model provider.
- Publish bounded, read-only running-server discovery on the local network through the [LanGame LAN Directory Protocol v2](docs/lan-directory-protocol-v2.md).

## Supported games and platform

The [`modules/`](modules/) catalog contains 32 dedicated-server integrations, including Palworld, Minecraft, Valheim, ARK: Survival Ascended, ARK: Survival Evolved, Don't Starve Together, Rust, 7 Days to Die, and Project Zomboid.

Available settings, queries, and administration actions depend on the game and server version. See the [server configuration guide](docs/server-configuration.md#english) and [game integration documentation](docs/README.md#game-integrations) for the modeled settings and their sources.

Windows desktop is the supported runtime; NSIS is the configured installer format. The interface is available in English and Simplified Chinese.

## Requirements

The application runs locally and does not require a LanGame account or hosted control plane. LAN is optional; model-provider requirements depend on the selected service.

Installations and server instances are managed separately. When no installed or recoverable program source remains after uninstall, install server files before creating another instance. The first verified instance can use an unused downloaded installation in place, without moving or duplicating its program files. Additional independent instances copy verified program files and keep their own data; Minecraft vanilla also supports explicitly shared programs. Creating a server always prepares a clean original program with new configuration and saves. If existing files cannot be verified, creation reuses verified bytes in a separate library, downloads missing official files and continues automatically. This repaired library remains available after all instances are deleted; independent instances use their own copies so running them does not modify the retained source. Existing installations, modifications, instances and archives remain intact. Preparation is cancellable; interrupted acquisitions retain their registered files, and a later attempt reuses only verifiable files.

Supported SteamCMD and Minecraft Java installations check for updates before each instance start by default. Updates target the instance's actual program directory and complete before configuration is rendered and the server is launched. In instance maintenance, choose **Keep current version** to disable automatic updates and protect the program from library or maintenance updates until the policy is changed; a shared program is protected when any referencing instance is pinned. Another shared Minecraft instance can start when a read-only check confirms the installed release and Java runtime are current. Failed checks or required updates blocked by running instances or archive dependencies stop startup with a reason. Whole-package replacement sources remain manual.

Creation verifies the selected program files, reusing verified file identities where supported to avoid rereading unchanged contents. After the last instance is permanently deleted, its original library can be used in place again when no instance or archive reserves it, its program files remain intact, and no previous settings, saves or Mods remain. Empty directories and deleted shipped defaults do not force another program copy. Unknown or modified files are preserved and creation uses an independent verified copy instead. Libraries explicitly retained as repair sources keep that role. An intact, verified local source does not require a download or the SteamCMD runtime; an unrelated game's download can continue while creation proceeds.

Deleting an instance removes its owned data and any instance-owned program copy, while a user-retained installation stays visible in the library. Unknown files and external saves are preserved. A data archive includes personal files and modifications but depends on its exact program source; updates and uninstall cannot invalidate that dependency. A complete archive retains the program bytes when a reliable reconstruction source is unavailable. Existing archives retain their recovery contracts.

When an inactive instance's entire managed folder is deleted outside the app, refreshing the server list removes its remaining registration without touching libraries or external saves. Missing instance-owned program files leave the instance available for explicit deletion; unavailable storage and running instances are retained.

Library uninstall requires that game's instances to be stopped. It removes unused library installations and their registered variants, leaving instance-owned program copies intact. A library still used by a first-instance reference, shared instance, or dependent archive is retained with a reason; uninstall does not detach those references automatically.

Instances can run the same game or different games together. Their files and managed ports are separate, while CPU, memory, disk and network capacity still belong to the same host. This is not a security container. See the [configuration guide](docs/server-configuration.md#english) for port groups, ARK shared-transfer directories, resource limits, and explicit runtime shutdown.

Each game has its own hardware, storage, network, and dedicated-server requirements. Check the game's documentation before installing a server. This repository does not redistribute dedicated-server binaries, proprietary game files, or unapproved third-party media. Game names and trademarks belong to their respective owners.

## Run from source

Install the following prerequisites:

- Git.
- Rust `1.98.1` with the MSVC target. `rust-toolchain.toml` pins the repository toolchain.
- Node.js `^22.22.2` or `>=24.15.0` with npm `>=11.16.0`. Node.js 24 LTS is recommended and used by CI.
- Microsoft C++ Build Tools and Microsoft Edge WebView2; see the official [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).
- Python 3.11 or later for repository verification scripts.

From the repository root, install the frontend dependencies and verify the Rust workspace without changing the lock files:

```powershell
npm ci --prefix apps/desktop --strict-allow-scripts
npm --prefix apps/desktop run build
cargo check --workspace --locked
```

Start the frontend development server:

```powershell
npm --prefix apps/desktop run dev
```

In a second terminal, start the desktop process:

```powershell
cargo run --locked --manifest-path apps/desktop/src-tauri/Cargo.toml --bin langame-desktop
```

To build a local NSIS installer without release credentials, use the pinned Tauri CLI. The default configuration disables update signing and network update checks. The `--locked` argument is forwarded to the underlying Cargo build.

```powershell
cargo install tauri-cli --version "=2.11.4" --locked
Push-Location apps/desktop
try {
    cargo tauri build -- --locked
    if ($LASTEXITCODE -ne 0) { throw "Tauri build failed." }
}
finally {
    Pop-Location
}
```

The installer supports English and Simplified Chinese, installs for the current user, and registers a Windows uninstall entry. It embeds Microsoft's official WebView2 offline installer, so a missing runtime needs no download during installation; existing runtimes are reused. Building the setup still requires access to Microsoft's runtime download service. Before manual installation or removal, choose **Exit** from the system tray and wait for servers to stop. Removing the application preserves server files and settings; the optional interface-data cleanup is not a complete data erasure.

Maintainers can prepare a local release plan with [`scripts/build_desktop_update_artifacts.ps1`](scripts/build_desktop_update_artifacts.ps1) and an explicit repository-external `-OutputRoot`. Its default mode requires no signing key and does not build, upload or publish. On the managed workstation, `-BuildManaged` builds a local installer with a source snapshot and artifact receipt; it keeps update checks disabled and needs no updater key by default. Signed builds and GitHub update checks are explicit options; see the [desktop release guide](docs/desktop-release.md). Keep private keys and passwords outside the repository. Finding a new version opens a prompt with direct choices to visit its download page or update in the app; checks remain disabled until an enabled build is deliberately prepared.

The first build downloads Rust and npm dependencies. Dedicated-server packages are acquired only when an operator starts the corresponding game workflow.

## Documentation

- [Documentation index](docs/README.md): server configuration, game integrations, and protocols.
- [Privacy and data](PRIVACY.md#english): local storage, AI recipients, LAN visibility, external services, and deletion limits.
- [Contributing guide](CONTRIBUTING.md): issue reports, pull requests, and documentation contributions.
- [Development guide](docs/development.md#english): architecture, generated files, checks, and source distribution.
- [Third-party notices](THIRD_PARTY_NOTICES.md): dependency licenses and documentation provenance.

## Support

Use the repository's issue forms for reproducible defects, scoped feature proposals, and Windows dedicated-server requests. Before opening an issue or pull request, remove credentials, private addresses, player data, proprietary files, and unlicensed media.

For problems inside a game server or mod, consult its publisher's documentation and support channels. If the problem concerns installation, generated configuration, or process management by this application, include the game module and reproduction steps in a bug report.

Report vulnerabilities through the private process in [SECURITY.md](SECURITY.md).

## Contributing

Code, documentation, translations, and reproducible game-integration evidence are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) and [LanGame Contributor Agreement 1.0](CONTRIBUTOR_AGREEMENT.md#english) before proposing a change, and follow the [Code of Conduct](CODE_OF_CONDUCT.md). Contributors keep their copyright and expressly authorize official commercial use within this Project; confirmation is required before inclusion.

## License

Project-authored source code uses the [LanGame Source-Available License 1.0](LICENSE).

| Use | Permission |
| --- | --- |
| Run a free personal or community game server; study or privately modify the code | Permitted for noncommercial purposes; private changes need not be published |
| Submit improvements to the official project | Permitted through patches and contribution forks; see [CONTRIBUTING.md](CONTRIBUTING.md) |
| Share an official, already public distribution unchanged and in full | Permitted free of charge with its license and notices |
| Independently release a modified version, whether free, paid, open-source, or closed-source | Not permitted by this license |
| Use for a business, commercial hosting, paid deployment, or a game server selling access, memberships, items, or privileges | Requires separate written authorization, even if revenue only covers costs |

For commercial use or other permissions, use the [contact request form](.github/ISSUE_TEMPLATE/contact_request.yml).

[Brand rules](NOTICE) · [Privacy notice](PRIVACY.md#english) · [Third-party notices](THIRD_PARTY_NOTICES.md) · [Asset licenses](THIRD_PARTY_ASSETS/NOTICE)
