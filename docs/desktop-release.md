# Desktop release preparation

The repository defaults to local builds: the update feed is empty, update checks are disabled, and ordinary NSIS builds do not require an updater signing key. Release preparation is local and does not publish a GitHub Release, upload assets, create a tag, change repository visibility, or enable a publishing workflow.

The supported release target is Windows x86_64 with a stable `major.minor.patch` version. Keep `Cargo.toml` (`workspace.package.version`), `apps/desktop/package.json`, and `apps/desktop/src-tauri/tauri.conf.json` aligned. The desktop crate inherits the workspace version. Change the corresponding lockfile package version when bumping a version. Release builds statically link the Visual C++ runtime and use the UCRT provided by supported Windows systems, so a separate Visual C++ Redistributable is not required.

## WebView2 delivery

The default NSIS installer uses Tauri's `embedBootstrapper` mode with `silent: true`. It embeds Microsoft's small Evergreen bootstrapper and reuses an installed WebView2 runtime. When WebView2 is missing, the bootstrapper downloads and installs it from Microsoft; this requires internet access. The same behavior applies to both installer languages. In-app updates use this smaller package.

Pass `-OfflineInstaller` to produce the separate `x64-offline-setup.exe` with `offlineInstaller` and `silent: true`. It embeds Microsoft's full x64 Evergreen Standalone Installer, allowing a missing runtime to be installed without networking. Both packages install the same application and reuse an existing runtime. The offline package is larger and does not contain game server downloads. Evergreen remains serviced by Microsoft when connectivity is available.

The build machine needs Microsoft's official downloads and Tauri's packaging tools. The pinned Tauri CLI 2.12.1 acquires the embedded bootstrapper from [Microsoft's bootstrapper endpoint](https://go.microsoft.com/fwlink/p/?LinkId=2124703) and caches it. On a user's machine, that bootstrapper obtains the runtime through Microsoft's delivery service; this repository does not pin its internal runtime URL. For the offline package, the bundler resolves Microsoft's [x64 download endpoint](https://go.microsoft.com/fwlink/?linkid=2124701), requires its HEAD redirect under `https://msedge.sf.dl.delivery.mp.microsoft.com/filestreamingservice/files/`, and caches by the resolved identifier; resolution still occurs for a cached offline payload. There is no third-party mirror or language-dependent source. See the [pinned bundler implementation](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.1/crates/tauri-bundler/src/bundle/windows/util.rs), [Tauri installation modes](https://v2.tauri.app/distribute/windows-installer/#webview2-installation-options), and [Microsoft runtime distribution](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution).

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

That local merge configuration points to `https://github.com/SZSLGJCOM/LanGame-Server-Manager/releases/latest/download/latest.json` and sets `VITE_LANGAME_DESKTOP_UPDATES_ENABLED=true` in the frontend build command. The checked-in default configuration remains disabled. A private repository or unpublished release does not provide an anonymous public update feed; do not embed GitHub tokens in the app or URLs.

Use `-EnableRegionalUpdates` after the [regional metadata service](../deploy/desktop-updates/README.md) has been deployed and verified. It enables updates with `https://langame.cn/updates/server-manager/latest.json` first and the independent GitHub feed second. It works with both small and offline installers. Clients also try the two built-in public proxy feeds (`gh-proxy.org` and `ghfast.top`) when earlier checks fail; each source has a 12-second check budget and the complete check has a 60-second deadline. Local builds with no configured endpoint remain disabled.

The regional endpoint returns only a small manifest: China-allocated addresses select the verified GitCode attachment, and other addresses select GitHub. Address allocation is a routing approximation, not proof of physical location. The website does not relay installation files. Downloads retain the selected version and signature while trying the selected attachment, its exact GitHub counterpart and the two public proxies. A source transferring less than 1 MiB in 30 seconds gives way to the next source; a second pass tolerates slow progress but still rejects a 30-second stall. The entire download remains bounded to 30 minutes. Each attempt resets progress; installation and runtime shutdown start only after Tauri verifies the complete installer signature. Public proxies are external services whose policies and availability must be reviewed; they are fallbacks, not an availability guarantee.

The updater requires the trusted signature comment to contain the advertised version. Do not disable `requireSignedVersion` for a mirror: an old, validly signed binary must not be relabeled as a newer update. The pinned Tauri bundler writes the version into its signed comment; exported artifacts retain those signature bytes unchanged.

Automatic updates reject small installers exceeding 256 MiB; oversized bodies or advertised lengths cancel that source before installation. This is a response-byte rejection threshold, not a hard process-memory limit: the plugin cancels at its next asynchronous yield and may allocate the current chunk and extra buffer capacity first. The mirror publisher enforces the same small-package limit. Offline companions remain manual downloads. The GitCode synchronization workflow runs on stable GitHub Release publication after `GITCODE_RELEASE_TOKEN` is configured; it verifies all seven attachments before publishing the regional pointer. Complete the first live synchronization and metadata-service acceptance before enabling regional builds.

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

These checks validate format, key identity, and copy consistency. They are **not cryptographic signature verification**, proof that the private key is available, or proof that the installer contains the expected version. Tauri verifies update signatures before installation. Windows Authenticode signing is a separate requirement; a Tauri `.sig` is not a Windows publisher certificate. No valid private key or Windows signing certificate is generated by these scripts.

## Update notes and delivery

Write the user-facing changes in a UTF-8 text file and pass it with `-NotesFile` when building or exporting. The script copies this text into `latest.json.notes`; Tauri exposes it as the update body and the prompt displays it as plain text with line breaks. Empty notes are omitted from the prompt. Use the same text for the GitHub Release description: the current preparation scripts do not create Releases or synchronize their descriptions. Editing only the GitHub page does not update the in-app notes.

An enabled desktop build checks its configured manifest sources once after initialization and then every four hours while no update is already available, downloading or installing. A higher version triggers the prompt; dismissing a version suppresses repeated prompts during the session. This is polling, not immediate server push. An already-pending version remains selected until installation or another explicit check/restart, so editing its notes does not interrupt the current prompt. A valid response reporting no update does not prevent checking independent feeds, because a mirror may still be synchronizing.

Release the incremented version's installer, signature and matching manifest together when publication is authorized. Uploading only an installer does not notify clients. Binary distribution does not require publishing application source: a separate public distribution repository can host the feed, but using one requires changing the release script's feed/asset URLs and the frontend's versioned download-page URL together. The current scripts do not make that separation or add private-repository authentication.

## Installer and update acceptance

The installer and uninstaller guard against a running desktop or runtime. Exit LGSM through its coordinated shutdown action before manually upgrading or uninstalling; closing the window to the tray does not stop the application. The guard must not force-kill an active game server. Application updates download before coordinating runtime shutdown and installation.

Passive updates reuse the saved installer language, or use the system language when none was saved. They never open a language selection dialog; interactive manual installation still offers the language selector. The installer guard test entry also runs eight isolated language-initialization cases, including the old updater's `/P /UPDATE /R /ARGS` invocation.

An enabled build automatically prompts when a new version is found. Download opens that version's official GitHub Release page; Update in app starts the existing updater after the prompt explains saving, stopping and restarting. There is no header button or second confirmation screen. Dismissing the prompt does not install anything or cause repeated prompts for the same version during ordinary background checks.

### Data locations and uninstallation

The application installation directory is independent of its data location. On first use without existing settings or a database, LGSM tries writable local fixed non-system drives in descending order of available space, then the Windows system drive. A missing or empty `LanGame` directory at a candidate drive root is initialized. A nonempty directory can be reused only with a valid LGSM marker bound to that actual directory and a complete expected layout; unrecognized directories are skipped without adopting their contents. If every automatic candidate is unavailable, first startup lets you select another local parent directory for `LanGame` or an empty `LanGame` directory, subject to the same checks, or cancel startup. The selected location is saved and is not recalculated when free space changes. An unavailable saved location must be restored; it does not offer selection of another drive.

New installations use `LanGame/server-files` for game programs, `LanGame/instances` for instances and `LanGame/cmd/steamcmd` for SteamCMD. Server path settings, the database, logs, knowledge and media caches reside under `LanGame/app-data/ServerManager/<local-user-directory-id>`, in `settings.json`, `db`, `logs`, `knowledge` and `cache` respectively; each Windows account has a separate database. Windows keeps the small location record at `%LOCALAPPDATA%\LanGame\ServerManager\storage-location.json`. Already registered locations do not require the new directory marker. Existing backend settings or databases in the previous user data directory remain there without automatic migration, including their existing local runtime and configured game directories.

WebView's Windows profile, browser storage and browser cache are separate from these directories and remain under `%LOCALAPPDATA%\cn.langame.servermanager`. Uninstallation removes the application but preserves game data, server settings, the backend `settings.json`, databases, previously used data directories and the location record, so reinstallation can find them. The optional interface-data cleanup targets the application-identifier/WebView directories and resets interface settings stored in localStorage, including language, theme and model-service configuration. It does not erase the retained server data or backend files.

The disposable NSIS guard fixture checks six install/uninstall process scenarios without using real server data:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/tests/test_desktop_installer_guard.ps1 `
    -NsisRoot 'C:\BuildTools\NSIS'
```

`-NsisRoot` must contain `makensis.exe` and the Tauri NSIS plugins expected by the test. `-CompileOnly` verifies compilation only; omit it to execute all six install/uninstall scenarios. Run the full fixture on a supported Windows test machine whose application-control policy permits the test executables. Compilation alone does not establish runtime behavior.

Before public distribution, use disposable data on a supported Windows test machine to verify:

1. On a disposable Windows image without WebView2, verify small-package installation with Microsoft connectivity, and verify a blocked runtime download reports failure instead of claiming a usable installation. Disconnect networking and verify the offline package installs and launches in both installer languages, plus Windows uninstall registration. Verify both packages reuse an existing runtime without downloading one. Do not remove the developer workstation's runtime to simulate these tests.
2. Manual upgrade/uninstall while LGSM or its runtime is running: the operation waits or refuses safely without killing processes. Retry after coordinated shutdown.
3. Upgrade and uninstall with the application stopped, including retained configuration, instances, saves, archives, and backups; reinstall and read back retained data.
4. An older signed build discovers a higher version, displays notes, downloads, verifies the real signature, stops servers safely, installs, and restarts with data intact.
5. Interrupted download, unavailable feed, incorrect signatures, and installation failure report actionable errors; an unsuccessful download does not stop running servers.
6. The final installer has the intended publisher signature and certificate chain. Verify both a trusted update and a tampered update through the actual Tauri updater.

## When publication is separately authorized

Create the corresponding stable `v<version>` tag and release with release notes only after the private acceptance work is complete. Attach the small installer, its `.exe.sig`, `latest.json`, and `SHA256SUMS`, plus the offline installer, its `.exe.sig`, and `SHA256SUMS.offline`. Keep plans, inventories and build receipts local. Confirm both signatures match the established public key and all uploaded bytes match their respective checksum inventory. Only the small installer belongs in `latest.json`; retain the offline companion as a manual download. The manifest uses the exact versioned asset URL, while clients read the stable `releases/latest/download/latest.json` endpoint. A draft or prerelease is not the stable latest-release channel. Never replace an already-distributed installer under the same version.

A build distributed while checks were disabled needs one manual installation of an update-enabled build. Uploading an installer alone cannot enable updates in those older binaries. Source publication and repository visibility remain a separate decision from preparing installer assets.

References: [Tauri updater and static JSON](https://v2.tauri.app/plugin/updater/), [Windows code signing](https://v2.tauri.app/distribute/sign/windows/), [GitHub Releases](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases).

## 简体中文操作摘要

默认执行脚本只在仓库外生成本地计划与合并配置，不构建、不联网、不上传、不创建 Release、不公开源码。默认不需要更新私钥、不开启更新源。`-SignUpdates` 生成更新签名；`-EnableGitHubUpdates` 同时启用 GitHub 更新源并要求匹配的签名私钥。`-BuildPortable` 用于普通 Windows 构建机，受管环境应遵循其指定构建入口。

部署并验证[地区更新清单服务](../deploy/desktop-updates/README.md)后，可用 `-EnableRegionalUpdates` 构建小包或离线包。客户端依次检查官网清单、独立 GitHub 清单及内置的 `gh-proxy.org`、`ghfast.top` 公共代理；单源检查最多 12 秒，整体最多 60 秒。某个镜像返回“无更新”时仍检查其他源，避免同步延迟阻挡新版本。本地空更新源配置保持禁用。

官网只返回小型更新清单，按 APNIC 中国地址分配记录选择已验证的 GitCode 附件，其他地址选择 GitHub，安装包不经过官网服务器。地址分配记录只是地区判断依据，不保证实际物理位置。下载始终保持选定版本和签名，依次尝试清单附件、同版本 GitHub 附件及两个公共代理；30 秒不足 1 MiB 时切换下一源，第二轮允许慢速传输但拒绝连续 30 秒无进展，整体最多 30 分钟。每次切源重置进度，完整安装包经 Tauri 验签通过后才停止服务器并安装。公共代理政策和可用性须独立核实，不能作为可用性承诺。

发布同步工作流在 GitHub 正式 Release 发布后运行，将两包、两份签名、两份校验表和唯一 `latest.json` 同步至 GitCode。只有七份附件完成匿名下载、哈希及安装包密码学验签后，才提交国内清单指针；失败保留上一份有效指针。首次启用须配置仓库 Secret `GITCODE_RELEASE_TOKEN` 并完成真实同步验证，配置步骤见服务指南。客户端要求签名的可信注释包含实际版本，不得为镜像关闭 `requireSignedVersion`，以防旧包被标成新版本。

自动更新小包拒绝阈值为 256 MiB，同步发布器与客户端同时执行该限制；响应长度声明或实际流量超限时请求取消当前源。插件在下次异步让出时才执行取消，当前数据块和缓冲区预留容量可能额外占用内存，因此这不是进程内存峰值的硬上限。离线包仍供手动下载。

中英文默认小包使用 `embedBootstrapper`，内嵌微软引导器，已有 WebView2 时直接复用，缺少时须联网从微软安装。构建机从 `https://go.microsoft.com/fwlink/p/?LinkId=2124703` 获取引导器；用户端运行时下载由该微软程序处理，仓库不固定其内部下载地址。无法连接微软或需要断网安装时，使用显式 `-OfflineInstaller` 构建的独立离线包，内置完整 x64 Evergreen 安装程序。两包使用相同版本和签名密钥，各自保留独立配置回执；离线包不含游戏服务端文件，启用更新后仍通过小包更新。构建机仍须连接微软官方分发路径和 Tauri 官方工具地址，不使用第三方镜像。发布前检查两包实际内嵌微软载荷的 Authenticode 签名与 SHA-256，分别验证缺运行时的联网安装、联网失败、离线包断网安装及已有运行时复用；配置测试及源码回执不能替代真实验收。

每版更新说明用 UTF-8 文本通过 `-NotesFile` 写入 `latest.json.notes`，软件按纯文本和换行显示。GitHub Release 正文与软件内说明目前不自动同步，应使用同一份文案。启用更新的客户端初始化后检查一次，之后在没有待更新版本、下载或安装任务时每四小时检查；这不是实时推送。新版本须连同安装包、签名和清单一起发布，仅上传安装包不会触发通知。保持源码私有并公开二进制时，可另用公开发行仓库，但须同步修改更新源、资产下载和前往下载页面三处地址，当前脚本不会自动拆分。

在线升级复用已保存的安装器语言；未保存时使用系统语言，不弹语言选择框。手动交互安装仍显示语言选择框。安装器守卫测试入口同时运行八个隔离的语言初始化场景，覆盖旧版更新器的 `/P /UPDATE /R /ARGS` 参数组合。

`-ArtifactRoot` 可导出已有当前版本 `.exe` 与 `.exe.sig`，生成 `latest.json`、`SHA256SUMS` 和回执，但不能修改或证明导入包内置的更新开关。Tauri 输入仍使用含空格的原始名称，公开附件统一使用 `LanGame.Server.Manager_<version>_x64-setup.exe` 及 `.exe.sig`，文件字节不变；清单与校验表使用相同的点号名称，发布前核对 GitHub 实际返回的附件名和下载地址。签名格式、key ID、复制一致性校验不等于密码学验签；正式私钥匹配、Windows 发布者证书、安装升级卸载及数据保留仍须以真实安装包验收。手动安装或卸载前应从 LGSM 安全退出，不能只关闭到托盘。`-CompileOnly` 只验证 NSIS 夹具编译；六个安装/卸载场景须在允许该夹具执行的受支持 Windows 测试机上完整运行，才能验证守卫的运行行为。

离线导出另加 `-OfflineInstaller` 并使用新的输出目录，公开文件名为 `LanGame.Server.Manager_<version>_x64-offline-setup.exe` 及 `.exe.sig`，校验表为 `SHA256SUMS.offline`，本地清单为 `desktop-offline-artifacts.json`，不生成 `latest.json`，不改写小包校验表。发布时附带两包、两份签名、两份校验表及小包的唯一 `latest.json`，本地计划与回执不上传。导出选项不能把已有小包变成离线包，须核对原构建回执和实际载荷；更新说明仅写入小包清单。

### 数据位置与卸载

程序安装目录与数据目录独立。没有已有设置或数据库时，首次使用按可用空间从大到小尝试可写的本地固定非系统盘，最后尝试 Windows 系统盘。盘根 `LanGame` 不存在或为空时初始化；非空目录只有具备绑定实际目录的有效本程序标识和完整预期布局时才复用，陌生目录会跳过，不接管其中内容。全部自动候选不可用时，首次启动可以选择另一个本地父目录或空的 `LanGame` 目录，仍须通过同样检查，也可以取消启动。选择会保存，之后不随剩余空间变化重新选盘；已保存位置异常时只能恢复该位置，不提供换盘选项。

新用户的游戏程序位于 `LanGame/server-files`，实例位于 `LanGame/instances`，SteamCMD 位于 `LanGame/cmd/steamcmd`。服务器路径设置、数据库、日志、知识库和媒体缓存集中在 `LanGame/app-data/ServerManager/<本机用户的目录ID>`，分别对应 `settings.json`、`db`、`logs`、`knowledge` 和 `cache`，各 Windows 账号的数据库独立。Windows 将小型位置记录保存在 `%LOCALAPPDATA%\LanGame\ServerManager\storage-location.json`。已有登记位置不要求补加新目录标识；已有用户的后端设置或数据库继续使用原用户数据目录，包括原本地 runtime 和已配置的游戏目录，不自动迁移。

WebView 的 Windows 配置、浏览器存储和浏览器缓存另行位于 `%LOCALAPPDATA%\cn.langame.servermanager`。卸载移除应用文件，保留游戏数据、服务器设置、后端 `settings.json`、数据库、旧数据目录和位置记录，供重装后继续读取。可选的界面数据清理针对应用标识/WebView 目录，会重置 localStorage 中的语言、主题、模型服务配置等界面设置，不会删除保留的服务器数据或后端文件。
