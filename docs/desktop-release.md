# Desktop release preparation

The repository defaults to local builds: the update feed is empty, update checks are disabled, and ordinary NSIS builds do not require an updater signing key. Release preparation is local and does not publish a GitHub Release, upload assets, create a tag, change repository visibility, or enable a publishing workflow.

The supported release target is Windows x86_64 with a stable `major.minor.patch` version. Keep `Cargo.toml` (`workspace.package.version`), `apps/desktop/package.json`, and `apps/desktop/src-tauri/tauri.conf.json` aligned. The desktop crate inherits the workspace version. Change the corresponding lockfile package version when bumping a version.

## WebView2 delivery

All NSIS builds use Tauri's `offlineInstaller` mode with `silent: true`. The same English/Chinese installer embeds Microsoft's x64 Evergreen Standalone Installer. A first installation can install a missing WebView2 runtime without downloading it; an existing runtime is reused. This increases the setup size (Tauri documents approximately 127 MB; the actual size changes with Microsoft's runtime). Evergreen remains serviced by Microsoft when connectivity is available.

The build machine still needs network access. The pinned Tauri CLI 2.11.4 resolves Microsoft's [x64 download endpoint](https://go.microsoft.com/fwlink/?linkid=2124701), requires its HEAD redirect to end under `https://msedge.sf.dl.delivery.mp.microsoft.com/filestreamingservice/files/`, and caches the payload by its resolved identifier. Resolution still occurs for a cached payload. There is no third-party runtime mirror or language-dependent installer source. See the [pinned bundler implementation](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.11.4/crates/tauri-bundler/src/bundle/windows/util.rs), [Tauri installation modes](https://v2.tauri.app/distribute/windows-installer/#webview2-installation-options), and [Microsoft runtime distribution](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution).

That bundler download path does not verify the runtime's Authenticode signature or compare it against a pinned digest. Before distributing an installer, inspect its actual embedded runtime without executing it, require `Get-AuthenticodeSignature` to report `Valid` with a Microsoft publisher, and record its SHA-256 with the installer acceptance evidence. The setup's own publisher signature and updater `.sig` are separate checks. Configuration tests and build receipts do not replace this payload inspection or a clean-machine installation test.

## Prepare a configuration without building

From the repository root, choose a new directory outside the repository:

```powershell
$releaseOutput = 'C:\ReleaseStaging\LanGame-0.0.1-plan'
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build_desktop_update_artifacts.ps1 `
    -OutputRoot $releaseOutput
```

The default mode, also available as `-DryRun`, needs no private key and makes no network requests. It writes only:

- `tauri.release.conf.json`: a Tauri merge configuration for signed NSIS artifacts, carrying the offline WebView2 mode and keeping update checks disabled by default.
- `desktop-release-plan.json`: the requested configuration, original Tauri `source_artifact_name`, public `artifact_name`, and future versioned download URL. `requested_updates_enabled` and `requested_webview_install_mode` are intentions, not evidence about an executable. `artifact_build_configuration` remains `not-inspected`.

Use `-EnableGitHubUpdates` only when preparing a build that should check GitHub for updates:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build_desktop_update_artifacts.ps1 `
    -OutputRoot 'C:\ReleaseStaging\LanGame-0.0.1-update-plan' -EnableGitHubUpdates
```

That local merge configuration points to `https://github.com/SZSLGJCOM/LanGame-Server-Manager/releases/latest/download/latest.json` and sets `VITE_LANGAME_DESKTOP_UPDATES_ENABLED=true` in the frontend build command. The checked-in default configuration remains disabled. A private repository or unpublished release does not provide an anonymous public update feed; do not embed GitHub tokens in the app or URLs.

## Build signed installers on a portable Windows build host

Install the locked project prerequisites and Tauri CLI version described in the [README](../README.en.md). Supply `TAURI_SIGNING_PRIVATE_KEY` and, if required, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` through the release environment. The private key must match the existing updater public key; never place private keys or passwords in this repository, release assets, or command logs. Preserve the existing key for installed clients.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build_desktop_update_artifacts.ps1 `
    -OutputRoot 'C:\ReleaseStaging\LanGame-0.0.1-signed' `
    -BuildPortable -EnableGitHubUpdates -NotesFile 'C:\ReleaseStaging\release-notes.txt'
```

`-BuildPortable` is explicit. It runs Tauri from `apps/desktop`, pins `x86_64-pc-windows-msvc` and NSIS, forwards Cargo `--locked`, applies the generated merge configuration, and restores the caller's frontend environment afterward. Both the exact current-version installer and its `.exe.sig` must be freshly generated. Omitting `-EnableGitHubUpdates` produces a signed build whose update checks remain disabled.

`-BuildPortable` refuses managed build hosts. Follow that environment's approved build entry instead of bypassing its resource controls.

## Export an existing signed installer

Use a new output directory and an existing, repository-external NSIS bundle directory:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build_desktop_update_artifacts.ps1 `
    -OutputRoot 'C:\ReleaseStaging\LanGame-0.0.1-export' `
    -ArtifactRoot 'C:\ReleaseStaging\approved-nsis-bundle' `
    -NotesFile 'C:\ReleaseStaging\release-notes.txt'
```

The export selects only the Tauri inputs `LanGame Server Manager_<version>_x64-setup.exe` and the matching `.exe.sig`. It copies their bytes unchanged to `LanGame.Server.Manager_<version>_x64-setup.exe` and `.exe.sig`, then generates `latest.json`, `SHA256SUMS`, and `desktop-update-artifacts.json` using those public names. Other versions and legacy `.nsis.zip` files are not selected. Existing output files are never overwritten. Use a new output directory for each attempt.

GitHub can [rename uploaded asset filenames](https://docs.github.com/en/rest/releases/assets#upload-a-release-asset). Public export names replace spaces with dots and reject characters outside ASCII letters, digits, dots, underscores and hyphens. For version `0.0.1`, the manifest therefore uses `https://github.com/SZSLGJCOM/LanGame-Server-Manager/releases/download/v0.0.1/LanGame.Server.Manager_0.0.1_x64-setup.exe`. Upload the exported names unchanged and compare GitHub's returned asset names and download URLs with the manifest before publishing. Renaming the exported files does not alter the installer or signature bytes; retain the original build receipt for their source identity.

Exporting does not rebuild or inspect the executable's embedded update configuration or WebView2 payload. Even with `-EnableGitHubUpdates`, the generated configuration cannot change an imported installer. Preserve its original build evidence and verify the installed app's update behavior separately.

The manifest generator rejects invalid versions, unsafe/non-HTTPS URLs, incorrect filenames, empty or malformed signatures, and signature key IDs that disagree with the configured public key. It compares copied files with their source hashes and checks that the exported signature equals the manifest signature; a concurrent input change fails the export and removes only files created by that export. `SHA256SUMS` covers the installer, signature, and manifest.

These checks validate format, key identity, and copy consistency. They are **not cryptographic signature verification**, proof that the private key is available, or proof that the installer contains the expected version. Tauri verifies update signatures before installation. Windows Authenticode signing is a separate requirement; a Tauri `.sig` is not a Windows publisher certificate. No valid private key or Windows signing certificate is generated by these scripts.

## Update notes and delivery

Write the user-facing changes in a UTF-8 text file and pass it with `-NotesFile` when building or exporting. The script copies this text into `latest.json.notes`; Tauri exposes it as the update body and the prompt displays it as plain text with line breaks. Empty notes are omitted from the prompt. Use the same text for the GitHub Release description: the current preparation scripts do not create Releases or synchronize their descriptions. Editing only the GitHub page does not update the in-app notes.

No custom push server is needed for this feed. An enabled desktop build checks the static GitHub manifest once after initialization and then every four hours while no update is already available, downloading or installing. A higher version triggers the prompt; dismissing a version suppresses repeated prompts during the session. This is polling, not immediate server push. An already-pending version remains selected until installation or another explicit check/restart, so editing its notes does not interrupt the current prompt.

Release the incremented version's installer, signature and matching manifest together when publication is authorized. Uploading only an installer does not notify clients. Binary distribution does not require publishing application source: a separate public distribution repository can host the feed, but using one requires changing the release script's feed/asset URLs and the frontend's versioned download-page URL together. The current scripts do not make that separation or add private-repository authentication.

## Installer and update acceptance

The installer and uninstaller guard against a running desktop or runtime. Exit LGSM through its coordinated shutdown action before manually upgrading or uninstalling; closing the window to the tray does not stop the application. The guard must not force-kill an active game server. Application updates download before coordinating runtime shutdown and installation.

An enabled build automatically prompts when a new version is found. Download opens that version's official GitHub Release page; Update in app starts the existing updater after the prompt explains saving, stopping and restarting. There is no header button or second confirmation screen. Dismissing the prompt does not install anything or cause repeated prompts for the same version during ordinary background checks.

### Data locations and uninstallation

The application installation directory is independent of its data location. On first use without existing settings or a database, LGSM tries writable local fixed non-system drives in descending order of available space and creates `LanGame` at the selected drive root. If no other drive is usable, it tries the Windows system drive. If none is usable, initialization reports an error. The selected location is saved and is not recalculated when free space changes. An unavailable saved location is reported instead of silently selecting another drive.

New installations use `LanGame/server-files` for game programs, `LanGame/instances` for instances and `LanGame/cmd/steamcmd` for SteamCMD. Server path settings, the database, logs, knowledge and media caches reside under `LanGame/app-data/ServerManager/<local-user-directory-id>`, in `settings.json`, `db`, `logs`, `knowledge` and `cache` respectively. Windows keeps the small location record at `%LOCALAPPDATA%\LanGame\ServerManager\storage-location.json`. Existing backend settings or databases in the previous user data directory remain there without automatic migration, including their existing local runtime and configured game directories.

WebView's Windows profile, browser storage and browser cache are separate from these directories and remain under `%LOCALAPPDATA%\cn.langame.servermanager`. Uninstallation removes the application but preserves game data, server settings, the backend `settings.json`, databases, previously used data directories and the location record, so reinstallation can find them. The optional interface-data cleanup targets the application-identifier/WebView directories and resets interface settings stored in localStorage, including language, theme and model-service configuration. It does not erase the retained server data or backend files.

The disposable NSIS guard fixture checks six install/uninstall process scenarios without using real server data:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/tests/test_desktop_installer_guard.ps1 `
    -NsisRoot 'C:\BuildTools\NSIS'
```

`-NsisRoot` must contain `makensis.exe` and the Tauri NSIS plugins expected by the test. `-CompileOnly` verifies compilation only; omit it to execute all six install/uninstall scenarios. Run the full fixture on a supported Windows test machine whose application-control policy permits the test executables. Compilation alone does not establish runtime behavior.

Before public distribution, use disposable data on a supported Windows test machine to verify:

1. On a disposable Windows image without WebView2, disconnect networking and verify clean installation and desktop launch in both installer languages, plus Windows uninstall registration. Also verify that an image with WebView2 reuses its existing runtime. Do not remove the developer workstation's runtime to simulate this test.
2. Manual upgrade/uninstall while LGSM or its runtime is running: the operation waits or refuses safely without killing processes. Retry after coordinated shutdown.
3. Upgrade and uninstall with the application stopped, including retained configuration, instances, saves, archives, and backups; reinstall and read back retained data.
4. An older signed build discovers a higher version, displays notes, downloads, verifies the real signature, stops servers safely, installs, and restarts with data intact.
5. Interrupted download, unavailable feed, incorrect signatures, and installation failure report actionable errors; an unsuccessful download does not stop running servers.
6. The final installer has the intended publisher signature and certificate chain. Verify both a trusted update and a tampered update through the actual Tauri updater.

## When publication is separately authorized

Create the corresponding stable `v<version>` tag and release with release notes only after the private acceptance work is complete. Attach the exported installer, `.exe.sig`, `latest.json`, and `SHA256SUMS`. Keep the plan and preparation receipt local; they are operational records. The manifest uses the exact versioned asset URL, while clients read the stable `releases/latest/download/latest.json` endpoint. A draft or prerelease is not the stable latest-release channel. Never replace an already-distributed installer under the same version.

A build distributed while checks were disabled needs one manual installation of an update-enabled build. Uploading an installer alone cannot enable updates in those older binaries. Source publication and repository visibility remain a separate decision from preparing installer assets.

References: [Tauri updater and static JSON](https://v2.tauri.app/plugin/updater/), [Windows code signing](https://v2.tauri.app/distribute/sign/windows/), [GitHub Releases](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases).

## 简体中文操作摘要

默认执行脚本只在仓库外生成本地计划与合并配置，不构建、不联网、不上传、不创建 Release、不公开源码。默认不需要更新私钥、不开启更新源。`-SignUpdates` 生成更新签名；`-EnableGitHubUpdates` 同时启用 GitHub 更新源并要求匹配的签名私钥。`-BuildPortable` 用于普通 Windows 构建机，受管环境应遵循其指定构建入口。

中英文安装包统一内置微软官方 x64 Evergreen 离线安装程序（`offlineInstaller`），首次安装缺少 WebView2 时无需临时下载，已安装的运行时会直接复用。构建机仍须连接微软官方地址解析并获取运行时，以及 Tauri 官方地址获取打包工具；运行时后续由微软正常更新。发布前须检查真实内嵌载荷的微软 Authenticode 签名与 SHA-256，并在无 WebView2 的可丢弃 Windows 测试镜像断网验证中英文安装与启动；配置测试及源码回执不能替代这些验收。

每版更新说明用 UTF-8 文本通过 `-NotesFile` 写入 `latest.json.notes`，软件按纯文本和换行显示。GitHub Release 正文与软件内说明目前不自动同步，应使用同一份文案。无需自建推送接口：启用更新的客户端初始化后检查一次，之后在没有待更新版本、下载或安装任务时每四小时检查；这不是实时推送。新版本须连同安装包、签名和清单一起发布，仅上传安装包不会触发通知。保持源码私有并公开二进制时，可另用公开发行仓库，但须同步修改更新源、资产下载和前往下载页面三处地址，当前脚本不会自动拆分。

`-ArtifactRoot` 可导出已有当前版本 `.exe` 与 `.exe.sig`，生成 `latest.json`、`SHA256SUMS` 和回执，但不能修改或证明导入包内置的更新开关。Tauri 输入仍使用含空格的原始名称，公开附件统一使用 `LanGame.Server.Manager_<version>_x64-setup.exe` 及 `.exe.sig`，文件字节不变；清单与校验表使用相同的点号名称，发布前核对 GitHub 实际返回的附件名和下载地址。签名格式、key ID、复制一致性校验不等于密码学验签；正式私钥匹配、Windows 发布者证书、安装升级卸载及数据保留仍须以真实安装包验收。手动安装或卸载前应从 LGSM 安全退出，不能只关闭到托盘。`-CompileOnly` 只验证 NSIS 夹具编译；六个安装/卸载场景须在允许该夹具执行的受支持 Windows 测试机上完整运行，才能验证守卫的运行行为。

### 数据位置与卸载

程序安装目录与数据目录独立。没有已有设置或数据库时，首次使用按可用空间从大到小尝试可写的本地固定非系统盘，自动创建选中盘根的 `LanGame`。没有可用的其他盘时尝试 Windows 系统盘；均不可用时明确报错。选择会保存，之后不随剩余空间变化重新选盘；已选位置不可用时提示恢复该位置，不静默另建一套数据。

新用户的游戏程序位于 `LanGame/server-files`，实例位于 `LanGame/instances`，SteamCMD 位于 `LanGame/cmd/steamcmd`。服务器路径设置、数据库、日志、知识库和媒体缓存集中在 `LanGame/app-data/ServerManager/<本机用户的目录ID>`，分别对应 `settings.json`、`db`、`logs`、`knowledge` 和 `cache`。Windows 将小型位置记录保存在 `%LOCALAPPDATA%\LanGame\ServerManager\storage-location.json`。已有用户的后端设置或数据库继续使用原用户数据目录，包括原本地 runtime 和已配置的游戏目录，不自动迁移。

WebView 的 Windows 配置、浏览器存储和浏览器缓存另行位于 `%LOCALAPPDATA%\cn.langame.servermanager`。卸载移除应用文件，保留游戏数据、服务器设置、后端 `settings.json`、数据库、旧数据目录和位置记录，供重装后继续读取。可选的界面数据清理针对应用标识/WebView 目录，会重置 localStorage 中的语言、主题、模型服务配置等界面设置，不会删除保留的服务器数据或后端文件。
