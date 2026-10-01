<p align="center">
  <a href="https://langame.cn/products/langame-server-manager/"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/logo-dark.svg"><img src="assets/logo.svg" alt="LanGame Server Manager" width="420" align="middle"></picture></a>
  &nbsp;&nbsp;&nbsp;&nbsp;
  <a href="https://langame.cn/"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/entrogenesis-dark.svg"><img src="assets/entrogenesis.svg" alt="熵灵硅界 · ENTROGENESIS" width="230" align="middle"></picture></a>
</p>

# LanGame Server Manager

[简体中文](README.md) | [English](README.en.md)

[Website](https://langame.cn/) · [Product page](https://langame.cn/products/langame-server-manager/) · [Hosting guides](https://langame.cn/hosting/) · [WeChat](#follow-us-on-wechat)

[LanGame Server Manager](https://langame.cn/products/langame-server-manager/) is a **Windows** game server manager. Use it on a Windows PC at home or in a machine room, or on a Windows Server host, to install dedicated servers, edit native settings, run multiple instances, and handle logs and backups from one desktop app.

Hosting yourself usually means SteamCMD, config files, and a pile of native console windows. This app keeps day-to-day operations in one place: servers run in the background without covering the desktop, and you can reopen the app from the tray. Server files, settings, and saves are normally stored on the computer running the manager.

<p align="center">
  <img src="assets/lgsm-system-49eec728.webp" alt="System page: host resources and sample instances" width="920">
</p>

<p align="center"><em>Current interface with sample data.</em></p>

## Main features

**32** game server integrations are available. See the [product page](https://langame.cn/products/langame-server-manager/) for the full list.

- Install and update dedicated servers
- Edit native per-game settings
- Manage multiple instances, ports, saves, and power controls
- Host monitoring
- Backup and restore
- In-app console, with minimize-to-tray

The LAN AI assistant supports your own model service (BYOK) and can help with logs and troubleshooting. It sends messages and required diagnostic context to the service you configure. See the [privacy notice](PRIVACY.md#english).

## Download and install

Download the Windows x64 installer from [GitHub Releases](https://github.com/SZSLGJCOM/LanGame-Server-Manager/releases). The installer offers Simplified Chinese and includes the WebView2 runtime.

Before manual installation or removal, choose **Exit** from the system tray and wait for servers to stop. Uninstalling the manager preserves server files and settings.

## Requirements

Runs on **Windows 10 / 11** and **Windows Server**, with Simplified Chinese and English interfaces. Each game has its own hardware, storage, and network requirements. Public connections still need suitable ports and firewall rules. Native LAN, and overlay LANs such as Radmin VPN and Hamachi, can bind to the detected adapter. Some settings take effect only after a server restart.

## Use and permissions

- Noncommercial use only. Paid game servers, business use and commercial services require separate written permission.
- Private changes and contributions to the official project are permitted. Independent modified releases are not.
- Public official distributions may be shared free of charge, complete and unchanged, with their license and notices.

[License](LICENSE) · [Brand rules](NOTICE) · [Contribution guide](CONTRIBUTING.md#english) · [Contributor agreement](CONTRIBUTOR_AGREEMENT.md#english) · [Permission requests](https://github.com/SZSLGJCOM/LanGame-Server-Manager/issues/new?template=contact_request.yml)

## Guides and feedback

- Server guides (Chinese): [Palworld](https://langame.cn/articles/langame-palworld-server-guide/), [Minecraft](https://langame.cn/articles/langame-minecraft-server-guide/), [Don't Starve Together](https://langame.cn/articles/langame-dst-server-guide/), and [more games](https://langame.cn/hosting/)
- [Multiplayer and networking](https://langame.cn/networking/)
- [Issues](https://github.com/SZSLGJCOM/LanGame-Server-Manager/issues) for suggestions and feedback. Keep credentials, private server details, and player information out of public posts

## LanGame product family

Server Manager focuses on Windows hosting and server operation. Related products in the [LanGame](https://langame.cn/products/) family include:

- [LanGame OS Orbit](https://langame.cn/products/langame-os-orbit/): an AI game terminal for PC players, with a game library, classic emulators, LINK multiplayer, and SOFA streaming
- [LanGame OS Stellar](https://langame.cn/products/langame-os-stellar/): an AI venue management platform for esports spaces, coordinating Orbit terminals, game distribution, and seat operations
- [LanGame OS Lunet](https://langame.cn/products/langame-os-lunet/): the SOFA streaming core on Android, for streaming from Orbit on the same network

Hosts can install and start servers with Server Manager; players on the same network can discover and join supported services from Orbit.

## Follow us on WeChat

Scan with WeChat to follow **熵灵硅界 (ENTROGENESIS)**.

<a href="assets/wechat-official-account.jpg"><img src="assets/wechat-official-account.jpg" alt="QR code for the 熵灵硅界 (ENTROGENESIS) WeChat Official Account" width="215" height="215"></a>

## Run from source

Requires Git, Rust `1.98.1` (MSVC), Node.js `^22.22.2` or `>=24.15.0`, npm `>=11.16.0`, and Python 3.11 or later. Node.js 24 LTS is recommended. See the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for C++ Build Tools and WebView2.

<details>
<summary>Build, run, and package commands</summary>

Install dependencies and check the workspace from the repository root:

```powershell
npm ci --prefix apps/desktop --strict-allow-scripts
npm --prefix apps/desktop run build
cargo check --workspace --locked
```

Start the frontend development server:

```powershell
npm --prefix apps/desktop run dev
```

Start the desktop process in a second terminal:

```powershell
cargo run --locked --manifest-path apps/desktop/src-tauri/Cargo.toml --bin langame-desktop
```

Build a local installer (network update checks are disabled by default; no release signing key is required):

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

The first build downloads dependencies and the WebView2 installer. See the [release guide](docs/desktop-release.md) for signed distributions.

</details>

## About this repository

This repository contains the LanGame Server Manager source, build tools, and [user documentation](docs/README.md#english), under the [LanGame Source-Available License 1.0](LICENSE). See the [third-party notices](THIRD_PARTY_NOTICES.md).
