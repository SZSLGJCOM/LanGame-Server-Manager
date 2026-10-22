# Desktop release preparation

Local builds disable update checks and do not require an updater signing key. The release script prepares or builds artifacts; publishing is a separate step after acceptance.

The supported release target is Windows x86_64 with a stable `major.minor.patch` version. Keep `Cargo.toml` (`workspace.package.version`), `apps/desktop/package.json`, and `apps/desktop/src-tauri/tauri.conf.json` aligned. The desktop crate inherits the workspace version. Change the corresponding lockfile package version when bumping a version. Release builds statically link the Visual C++ runtime and use the UCRT provided by supported Windows systems, so a separate Visual C++ Redistributable is not required.

## Automated pre-release regression

Use a successful complete-tier result for the exact release candidate before packaging. An automatic push run that already selected and passed the full frontend, Rust and native checks provides that result; retain its run URL and acceptance artifacts instead of dispatching the same checks again. Otherwise run the shared complete tier locally or dispatch the GitHub `CI` workflow with `tier: full`. Reuse requires the same clean source revision, locked dependencies and verification rules; a partial run or later source change does not qualify. The daily regression invokes the same checks; it is not an independent certification of a later commit.

```powershell
python -B scripts/verify_project.py --tier full --output 'C:\Verification\LanGame-release-candidate'
```

The complete tier includes all frontend and Rust tests, synthetic Windows process lifecycle checks and an isolated Tauri/WebView2 acceptance host. Supply `--nsis-root` to run the disposable installer guard and language fixtures. Managed hosts use the explicit wrapper, external frontend and receipted native-executable options documented in [Development](development.md#改动与验证). Evidence reports stay outside the repository. Automated success does not certify the signed installer, a real upgrade, preserved saves or a live game-server start: complete the clean-machine acceptance below with the actual package before publication.

## WebView2 delivery

The default NSIS installer uses Tauri's `embedBootstrapper` mode with `silent: true`. It embeds Microsoft's small Evergreen bootstrapper and reuses an installed WebView2 runtime. When WebView2 is missing, the bootstrapper downloads and installs it from Microsoft; this requires internet access. The same behavior applies to both installer languages. In-app updates use this smaller package.

Pass `-OfflineInstaller` to produce the separate `x64-offline-setup.exe` with `offlineInstaller` and `silent: true`. It embeds Microsoft's full x64 Evergreen Standalone Installer, allowing a missing runtime to be installed without networking. Both packages install the same application and reuse an existing runtime. The offline package is larger and does not contain game server downloads. Evergreen remains serviced by Microsoft when connectivity is available.

The build machine needs Microsoft's official downloads and Tauri's packaging tools. The pinned Tauri CLI 2.12.3 acquires the embedded bootstrapper from [Microsoft's bootstrapper endpoint](https://go.microsoft.com/fwlink/p/?LinkId=2124703) and caches it. On a user's machine, that bootstrapper obtains the runtime through Microsoft's delivery service; this repository does not pin its internal runtime URL. For the offline package, the bundler resolves Microsoft's [x64 download endpoint](https://go.microsoft.com/fwlink/?linkid=2124701), requires its HEAD redirect under `https://msedge.sf.dl.delivery.mp.microsoft.com/filestreamingservice/files/`, and caches by the resolved identifier; resolution still occurs for a cached offline payload. There is no third-party mirror or language-dependent source. See the [pinned bundler implementation](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.3/crates/tauri-bundler/src/bundle/windows/util.rs), [Tauri installation modes](https://v2.tauri.app/distribute/windows-installer/#webview2-installation-options), and [Microsoft runtime distribution](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution).

Those bundler downloads do not verify Authenticode or compare a pinned digest. Before distribution, inspect the actual embedded bootstrapper or standalone installer without executing it, require `Get-AuthenticodeSignature` to report `Valid` with a Microsoft publisher, and record its SHA-256. The setup's own publisher signature and updater `.sig` are separate checks. Configuration tests and build receipts do not replace this payload inspection or clean-machine acceptance.

## Prepare a configuration without building

From the repository root, choose a new directory outside the repository:

```powershell
$releaseOutput = 'C:\ReleaseStaging\LanGame-0.0.1-plan'
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build_desktop_update_artifacts.ps1 `
    -OutputRoot $releaseOutput
```

The default mode, also available as `-DryRun`, needs no private key and makes no network requests. It writes only:

- `tauri.release.conf.json`: a Tauri merge configuration for signed NSIS artifacts, carrying the selected WebView2 mode and keeping update checks disabled by default.
- `desktop-release-plan.json`: the requested configuration, original Tauri `source_artifact_name`, public `artifact_name`, and future versioned download URL. `requested_updates_enabled` and `requested_webview_install_mode` are intentions, not evidence about an executable. `artifact_build_configuration` remains `not-inspected`.

Use `-EnableGitHubUpdates` only when preparing a build that should check GitHub for updates:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build_desktop_update_artifacts.ps1 `
    -OutputRoot 'C:\ReleaseStaging\LanGame-0.0.1-update-plan' -EnableGitHubUpdates
```

That merge configuration points to `https://github.com/SZSLGJCOM/LanGame-Server-Manager/releases/latest/download/latest.json` and sets `VITE_LANGAME_DESKTOP_UPDATES_ENABLED=true` in the frontend build command. The checked-in default remains disabled. Update feeds and artifacts must be anonymously accessible; credentials never belong in the app or URLs.

Use `-EnableRegionalUpdates` to prepare regional release candidates. Publish these candidates only after the [regional metadata service](../deploy/desktop-updates/README.md) has been deployed and verified. It enables updates with `https://langame.cn/updates/server-manager/latest.json` first and the independent GitHub feed second. It works with both small and offline installers. Clients also try the two built-in public proxy feeds (`gh-proxy.org` and `ghfast.top`) when earlier checks fail; each source has a 12-second check budget and the complete check has a 60-second deadline. Local builds with no configured endpoint remain disabled.

The regional endpoint returns only a small manifest: China-allocated addresses select the verified GitCode attachment, and other addresses select GitHub. Address allocation is a routing approximation, not proof of physical location. The website does not relay installation files. Downloads retain the selected version and signature while trying the selected attachment, its exact GitHub counterpart and the two public proxies. A source transferring less than 1 MiB in 30 seconds gives way to the next source; a second pass tolerates slow progress but still rejects a 30-second stall. The entire download remains bounded to 30 minutes. Each attempt resets progress; installation and runtime shutdown start only after Tauri verifies the complete installer signature. Public proxies are external services whose policies and availability must be reviewed; they are fallbacks, not an availability guarantee.

The updater requires the trusted signature comment to contain the advertised version. Do not disable `requireSignedVersion` for a mirror: an old, validly signed binary must not be relabeled as a newer update. The pinned Tauri bundler writes the version into its signed comment; exported artifacts retain those signature bytes unchanged.

Automatic updates reject small installers exceeding 256 MiB; oversized bodies or advertised lengths cancel that source before installation. This is a response-byte rejection threshold, not a hard process-memory limit: the plugin cancels at its next asynchronous yield and may allocate the current chunk and extra buffer capacity first. The mirror publisher enforces the same small-package limit. Offline companions remain manual downloads. The release workstation checks stable GitHub Releases every five minutes and verifies all seven attachments before publishing the GitCode pointer. It must be on and logged in, with its configured GitHub proxy available. The GitHub mirror workflow remains disabled to keep one writer. See the [service guide](../deploy/desktop-updates/README.md) for credential storage, routing and recovery. Complete the first live synchronization and metadata-service acceptance before publishing regional builds.

## Build signed installers on a portable Windows build host

Install the locked project prerequisites and Tauri CLI version described in the [README](../README.en.md). Supply `TAURI_SIGNING_PRIVATE_KEY` and, if required, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` through the release environment. The private key must match the existing updater public key; never place private keys or passwords in this repository, release assets, or command logs. Preserve the existing key for installed clients.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build_desktop_update_artifacts.ps1 `
    -OutputRoot 'C:\ReleaseStaging\LanGame-0.0.1-signed' `
    -BuildPortable -EnableGitHubUpdates -NotesFile 'C:\ReleaseStaging\release-notes.txt'
```

`-BuildPortable` is explicit. It runs Tauri from `apps/desktop`, pins `x86_64-pc-windows-msvc` and NSIS, forwards Cargo `--locked`, applies the generated merge configuration, and restores the caller's frontend and static-runtime environment settings afterward. Both the exact current-version installer and its `.exe.sig` must be freshly generated. Omitting both `-EnableGitHubUpdates` and `-EnableRegionalUpdates` produces a signed build whose update checks remain disabled.

`-BuildPortable` refuses managed build hosts. Follow that environment's approved build entry instead of bypassing its resource controls.

Build the offline companion from the same source version with `-OfflineInstaller`, a separate new `-OutputRoot`, the same signing key, and the same `-EnableGitHubUpdates` setting. This flag changes WebView2 delivery and export names, not the installed application's update channel. An update-enabled offline installation subsequently updates through the small package. Managed builds use `-BuildManaged -SignUpdates` (or `-EnableGitHubUpdates`) and retain a separate immutable configuration receipt for each package; the wrapper can reuse its compilation cache. Do not reuse a receipt for a different bundle configuration or repackage a released target directory outside its managed session.

`build.windows.staticVCRuntime=true` in `tauri.conf.json` is the release CRT policy. The release entry copies this explicit boolean into the captured merge configuration and temporarily clears Tauri's deprecated `STATIC_VCRUNTIME` environment override, restoring the caller's environment on success or failure. New managed receipts bind their CRT setting to that hashed configuration; this records the build input, not proof of the binary's imports.

The managed build entry checks static-runtime evidence in the build receipt and reads the exported main executable's PE imports; the portable entry checks both `langame-desktop.exe` and `install_catalog.exe`. Before publication, also check both executables from the actual NSIS payload or installation, since the managed receipt exports only the main executable:

```powershell
python -B scripts/verify_desktop_runtime_dependencies.py `
    'C:\ReleaseStaging\installed\langame-desktop.exe' `
    'C:\ReleaseStaging\installed\install_catalog.exe'
```

This rejects separate Visual C++ runtime dependencies in normal and delayed imports without executing either file; Windows system UCRT imports remain permitted.

## Export an existing signed installer

Use a new output directory and an existing, repository-external NSIS bundle directory:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build_desktop_update_artifacts.ps1 `
    -OutputRoot 'C:\ReleaseStaging\LanGame-0.0.1-export' `
    -ArtifactRoot 'C:\ReleaseStaging\approved-nsis-bundle' `
    -NotesFile 'C:\ReleaseStaging\release-notes.txt'
```

The export selects only the Tauri inputs `LanGame Server Manager_<version>_x64-setup.exe` and the matching `.exe.sig`. It copies their bytes unchanged to `LanGame.Server.Manager_<version>_x64-setup.exe` and `.exe.sig`, then generates `latest.json`, `SHA256SUMS`, and `desktop-update-artifacts.json` using those public names. Other versions and legacy `.nsis.zip` files are not selected. Existing output files are never overwritten. Use a new output directory for each attempt.

For an already verified offline build, add `-OfflineInstaller`. Its original Tauri input names are unchanged, but export names become `LanGame.Server.Manager_<version>_x64-offline-setup.exe` and `.exe.sig`. The signed export writes `SHA256SUMS.offline` and the local `desktop-offline-artifacts.json` inventory; it never writes `latest.json` or the small package's `SHA256SUMS`. Keep the inventories and build receipts local. The flag cannot turn an imported small installer into an offline installer: check the source receipt and extracted WebView2 payload before export. Update notes belong to the small package's manifest and are not exported for the offline companion.

GitHub can [rename uploaded asset filenames](https://docs.github.com/en/rest/releases/assets#upload-a-release-asset). Public export names replace spaces with dots and reject characters outside ASCII letters, digits, dots, underscores and hyphens. For version `0.0.1`, the manifest therefore uses `https://github.com/SZSLGJCOM/LanGame-Server-Manager/releases/download/v0.0.1/LanGame.Server.Manager_0.0.1_x64-setup.exe`. Upload the exported names unchanged and compare GitHub's returned asset names and download URLs with the manifest before publishing. Renaming the exported files does not alter the installer or signature bytes; retain the original build receipt for their source identity.

Exporting does not rebuild or inspect the executable's embedded update configuration or WebView2 payload. Even with `-EnableGitHubUpdates`, the generated configuration cannot change an imported installer. Preserve its original build evidence and verify the installed app's update behavior separately.

The manifest generator rejects invalid versions, unsafe/non-HTTPS URLs, incorrect filenames, empty or malformed signatures, and signature key IDs that disagree with the configured public key. It compares copied files with their source hashes and checks that the exported signature equals the manifest signature; a concurrent input change fails the export and removes only files created by that export. `SHA256SUMS` covers the installer, signature, and manifest.

These checks validate format, key identity, and copy consistency. They are **not cryptographic signature verification**, proof that the private key is available, or proof that the installer contains the expected version. Tauri verifies update signatures before installation. Windows Authenticode signing is separate and optional for the application's installer; a Tauri `.sig` is not a Windows publisher certificate. The existing release channel uses updater-signed installers without an application publisher certificate. No valid private key or Windows signing certificate is generated by these scripts. Embedded Microsoft runtime installers must still have valid Microsoft signatures.

## Update notes and delivery

Write the user-facing changes in a UTF-8 text file and pass it with `-NotesFile` when building or exporting. The script copies this text into `latest.json.notes`; Tauri exposes it as the update body and the prompt displays it as plain text with line breaks. Empty notes are omitted from the prompt. Use the same text for the GitHub Release description: the current preparation scripts do not create Releases or synchronize their descriptions. Editing only the GitHub page does not update the in-app notes.

An enabled desktop build checks its configured manifest sources once after initialization and then every four hours while no update is already available, downloading or installing. A higher version triggers the prompt; dismissing a version suppresses repeated prompts during the session. This is polling, not immediate server push. An already-pending version remains selected until installation or another explicit check/restart, so editing its notes does not interrupt the current prompt. A valid response reporting no update does not prevent checking independent feeds, because a mirror may still be synchronizing.

Publish the incremented version's installer, signature and matching manifest together. The release description and `latest.json.notes` use the same release notes. The feed, artifact URLs and frontend download-page URL must refer to the same repository and version.

## Installer and update acceptance

The installer and uninstaller guard against a running desktop or runtime. Exit LGSM through its coordinated shutdown action before manually upgrading or uninstalling; closing the window to the tray does not stop the application. The guard must not force-kill an active game server. Application updates download before coordinating runtime shutdown and installation.

During an application update, the same ten-second handoff budget also waits for an existing executable to become writable. The probe never truncates or changes the file; a persistent lock or permission error stops installation before replacement.

Passive updates reuse the saved installer language, or use the system language when none was saved. They never open a language selection dialog; interactive manual installation still offers the language selector. The installer guard test entry also runs eight isolated language-initialization cases, including the old updater's `/P /UPDATE /R /ARGS` invocation.

An enabled build automatically prompts when a new version is found. Download opens that version's official GitHub Release page; Update in app starts the existing updater after the prompt explains saving, stopping and restarting. There is no header button or second confirmation screen. Dismissing the prompt does not install anything or cause repeated prompts for the same version during ordinary background checks.

### Data locations and uninstallation

The application installation directory is independent of its data location. On first use without existing settings or a database, LGSM tries writable local fixed non-system drives in descending order of available space, then the Windows system drive. A missing or empty `LanGame` directory at a candidate drive root is initialized. A nonempty directory can be reused only with a valid LGSM marker bound to that actual directory and a complete expected layout; unrecognized directories are skipped without adopting their contents. If every automatic candidate is unavailable, first startup lets you select another local parent directory for `LanGame` or an empty `LanGame` directory, subject to the same checks, or cancel startup. The selected location is saved and is not recalculated when free space changes. An unavailable saved location must be restored; it does not offer selection of another drive.

New installations use `LanGame/server-files` for game programs, `LanGame/instances` for instances and `LanGame/cmd/steamcmd` for SteamCMD. Server path settings, the database, logs, knowledge and media caches reside under `LanGame/app-data/ServerManager/<local-user-directory-id>`, in `settings.json`, `db`, `logs`, `knowledge` and `cache` respectively; each Windows account has a separate database. Windows keeps the small location record at `%LOCALAPPDATA%\LanGame\ServerManager\storage-location.json`. Already registered locations do not require the new directory marker. Existing backend settings or databases in the previous user data directory remain there without automatic migration, including their existing local runtime and configured game directories.

WebView's Windows profile, browser storage and browser cache are separate from these directories and remain under `%LOCALAPPDATA%\cn.langame.servermanager`. Uninstallation removes the application but preserves game data, server settings, the backend `settings.json`, databases, previously used data directories and the location record, so reinstallation can find them. The optional interface-data cleanup targets the application-identifier/WebView directories and resets interface settings stored in localStorage, including language, theme and model-service configuration. It does not erase the retained server data or backend files.

The publisher is Entro Genesis. Installers retain the previous LanGame Team installation directory and language by copying missing settings before Tauri initializes its pages. Existing Entro Genesis settings and an explicit `/D` installation directory take precedence. Only a successful installation retires the old known values, and records for another installation path are preserved. The product name, application identifier, data locations and update signing identity remain stable.

Tauri's in-app update replaces the uninstaller's resource list without removing files retired by an earlier package. `apps/desktop/src-tauri/installer/retired-resources.nsh` records those exact paths and original SHA256 values, including distributed line endings. Installation and uninstallation remove only matching files; edited files, additional files and directory links are preserved. When retiring another bundled resource, add its original identity to this list and exercise it through the same installer fixture. Never replace this with recursive removal of the modules directory.

The disposable NSIS guard fixture exercises process guards, installer languages and retired-resource cleanup without using real server data:

```powershell
pwsh -NoProfile -File scripts/tests/test_desktop_installer_guard.ps1 `
    -NsisRoot 'C:\BuildTools\NSIS'
```

`-NsisRoot` must contain `makensis.exe` and the Tauri NSIS plugins expected by the test. `-CompileOnly` verifies compilation only; omit it to execute the install/uninstall scenarios, including modified-file preservation and a failed deletion followed by retry. Run the full fixture on a supported Windows test machine whose application-control policy permits the test executables. Compilation alone does not establish runtime behavior.

The Windows native CI job prepares the Tauri-pinned NSIS tools with `scripts/prepare_desktop_installer_tools.ps1`, verifies the downloaded assets, and runs this same fixture entry point.

Before public distribution, use disposable data on a supported Windows test machine to verify:

1. On a disposable Windows image without WebView2, verify small-package installation with Microsoft connectivity, and verify a blocked runtime download reports failure instead of claiming a usable installation. Disconnect networking and verify the offline package installs and launches in both installer languages, plus Windows uninstall registration. Verify both packages reuse an existing runtime without downloading one. Do not remove the developer workstation's runtime to simulate these tests.
2. Manual upgrade/uninstall while LGSM or its runtime is running: the operation waits or refuses safely without killing processes. Retry after coordinated shutdown.
3. Upgrade and uninstall with the application stopped, including retained configuration, instances, saves, archives, and backups; reinstall and read back retained data.
4. An older signed build discovers a higher version, displays notes, downloads, verifies the real signature, stops servers safely, installs, and restarts with data intact.
5. Interrupted download, unavailable feed, incorrect signatures, and installation failure report actionable errors; an unsuccessful download does not stop running servers.
6. Record the final installer's Windows signature status. If application publisher signing is configured, verify its intended publisher and certificate chain. Verify both a trusted update and a tampered update through the actual Tauri updater regardless of Windows publisher signing.

## Publish a verified release

After CI and package acceptance pass for the release candidate, create the stable `v<version>` tag and release. Upload exactly seven public assets:

- Small installer and its `.exe.sig`.
- Offline installer and its `.exe.sig`.
- `latest.json`, `SHA256SUMS`, and `SHA256SUMS.offline`.

Keep plans, inventories and build receipts local. Verify both updater signatures against the established public key and compare uploaded bytes with the checksum inventories. Only the small installer belongs in `latest.json`; the offline companion is a manual download. Clients consume signed NSIS executables directly through the stable `releases/latest/download/latest.json` feed, whose manifest names an exact versioned asset URL. Drafts and prereleases do not update the stable channel. Never replace a distributed installer under the same version.

A build distributed with update checks disabled needs one manual installation of an update-enabled build.

References: [Tauri updater and static JSON](https://v2.tauri.app/plugin/updater/), [Windows code signing](https://v2.tauri.app/distribute/sign/windows/), [GitHub Releases](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases).

## 简体中文操作摘要

完整参数与验收要求见上文；以下按实际发布顺序执行：

1. 将 Cargo、npm、Tauri 及对应锁文件版本统一为待发布版本。保留精确候选提交的完整 CI 结果；同一候选已通过完整检查时无需重复运行。
2. 在仓库外的新目录准备 UTF-8 更新说明。普通 Windows 主机使用 `-BuildPortable`；受管主机使用 `-BuildManaged`，签名和更新选项沿用受管发行入口。私钥通过发行环境传入，保留现有 updater 公钥。
3. 小包使用 `embedBootstrapper`，缺 WebView2 时从微软联网安装。用独立输出目录加 `-OfflineInstaller` 构建内嵌完整 WebView2 的离线包；两包版本、更新通道与签名密钥一致。
4. `-EnableGitHubUpdates` 启用 GitHub 更新；`-EnableRegionalUpdates` 同时启用官网地区清单。地区服务、GitCode 附件与匿名读取须先通过[部署验收](../deploy/desktop-updates/README.md)。
5. 检查实际安装包、两份 updater 签名、微软 WebView2 载荷签名和两个应用可执行文件的运行库依赖。在隔离 Windows 环境执行安装、升级、卸载、数据保留、真实开服和旧版在线更新验收。编译夹具或配置回执不能替代实际安装。
6. 创建正式 `v<version>` Release，上传两包、两份 `.exe.sig`、`latest.json`、`SHA256SUMS` 和 `SHA256SUMS.offline`。核对上传后文件名、版本、哈希与匿名下载；本地计划和回执不上传。
7. 核实本机镜像发布器的该版本记录为 `succeeded`，再核对官网地区清单和 GitCode 的完整版本指针。GitHub 镜像工作流保持禁用，确保只有一个发布者。

客户端初始化后检查一次，空闲时每四小时检查；关闭某版本提示后，本次会话不重复提示。更新下载并验签完成后才协调停止后台及游戏进程。手动安装或卸载前使用应用的安全退出，关闭到托盘不等于退出。

安装目录与游戏数据独立；卸载保留服务器配置、数据库、实例、存档、归档、备份和已保存的数据位置。可选界面数据清理仅重置 WebView/localStorage 设置。具体默认路径及旧数据兼容规则见 [Data locations and uninstallation](#data-locations-and-uninstallation)。
