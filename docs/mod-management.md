# Mod 管理 / Mod management

## 简体中文

进入实例的 **Mod → 我的 Mod** 管理该实例中的文件。Nexus Mods、Thunderstore、CurseForge 等来源的本地包导入后使用同一套文件管理操作；来源网站下载到电脑的文件仍需先导入实例。游戏原生管理的 Workshop、CurseForge ID 和启用列表沿用各游戏已有的操作，不删除共享下载缓存。

- **纳入管理**：为此前已放入 Mod 目录的包建立文件归属记录。只有可识别的 Mod 载荷或包标记才能纳管；官方文件、加载器、配置目录和无关文件不能作为普通 Mod 接管。
- **禁用 / 启用**：停止服务器后，把包移出游戏的加载目录，或者恢复到原位置。同名位置已有其他内容时拒绝覆盖。禁用的包继续显示在“我的 Mod”中。
- **删除**：先检查依赖，再移出加载目录。只清理归属记录中内容未变的非配置文件；配置、存档、新增文件、用户修改过的文件保留在恢复位置，可从操作结果打开。其他包及加载器不会随依赖一起删除。

重新导入或在线更新也会保留既有配置。新包试图覆盖配置时会拒绝整批更新；新包省略的配置继续保留。

服务端下载也复用同一登录控制台，详见[服务端安装与程序隔离](server-configuration.md#实例隔离)。Workshop 默认匿名下载。匿名下载明确遇到账号权限问题时，界面自动转到当前实例的 **LanGameCMD → SteamCMD**，说明原因并等待登录；普通失败或网络错误不会触发。输入 Steam 账号名或 `login STEAM_ACCOUNT_NAME`，立即开始原生登录，由 Steam 的实际返回决定下一步，无需预先选择验证方式。只有 Steam 要求密码时才显示隐藏密码输入，登录缓存有效时可直接继续。无需二次验证时直接完成登录；邮箱验证时显示隐藏的邮箱验证码输入；手机验证时显示当前五位令牌码输入，也可在手机应用中确认。原生 SteamCMD 无法在等待手机确认时直接接收令牌码，提交令牌后会在同一操作内完整回收旧登录进程，再启动一次令牌登录；不会无限重启或静默改成手机确认。登录分别显示等待运行、准备 SteamCMD、连接 Steam、验证账号和确认登录缓存的实际阶段。密码仅在本次原生登录工作线程内保留到认证确认或操作结束；密码和验证码隐藏显示、提交后立即清空，不进入进程启动参数、临时脚本、游戏命令历史或实例日志。登录成功后自动返回 Mod 并继续原下载一次；账号仍无内容权限时保留错误，不重复登录。匿名、已登录账号和切换账号操作统一位于 LanGameCMD 的 SteamCMD 控制台。账号选择和认证状态只在当前界面会话内保留，切换实例或退出工作区会取消尚未完成的登录及等待中的下载。LAN 远程连接需在服务器主机的桌面应用中完成登录。

已验证的受管 SteamCMD 自更新后，原生文件变化会使旧就绪记录过期。Mod 下载、服务端下载和账号登录会在同一运行时锁下自动执行一次限时 `+quit` 复核，核验成功才刷新记录并继续。首次缺少或尚未验证的 SteamCMD 仍需到系统页安装或检查；就绪错误本身不触发账号登录。

受管 SteamCMD 的下载、交互登录及登录缓存复核复用已核验的本地更新清单，避免重复访问引导器更新站点。官方多源用于 SteamCMD 安装与更新；账号认证连接和 Workshop 文件传输仍由原生 SteamCMD 处理，不经过管理器的国内外 HTTP 源选择。各类来源及边界见[官方网络来源](official-network-sources.md)。

管理器不保存密码或验证码；SteamCMD 自身在本机的 `config/config.vdf` 中维护登录令牌，以便后续仅用账号名登录。[Valve 登录令牌说明](https://partner.steamgames.com/doc/sdk/uploading)。每次账号下载都重新核实实际登录身份；令牌失效时提示重新登录，不会自动改用匿名下载。账号认证和游戏内容访问权限分别由 Steam 判断，登录成功本身不能保证任意游戏或 Mod 可下载。

幻兽帕鲁的 Workshop 下载若明确报告 `Missing decryption key`，表示 Steam 未向本次会话提供内容解密密钥；普通 `Failure` 本身不能确定这一原因。可使用有访问权限的账号登录 SteamCMD 后重试。也可在 Steam 中订阅并完成下载 Mod 及其依赖，再到 Steam 库的 `steamapps/workshop/content/1623730`，切到实例的 **Mod → 我的 Mod**，将对应 Mod 文件夹直接拖入下方 Mod 列表。列表只在拖入时显示接收反馈。按作者要求准备依赖，导入后检查实际加载日志。

已声明的依赖按包身份和版本检查。启用时要求所需包已启用、版本匹配且记录的载荷完整；禁用时阻止仍被已启用包依赖的操作，删除时阻止仍被其他受管包依赖的操作。先处理提示中的使用方，再操作其依赖。

依赖声明来自包或来源元数据，不能证明任意第三方代码的全部运行要求。未提供完整声明的包会显示“未提供完整依赖声明”，需要结合作者说明检查加载器、客户端要求及运行兼容性。Thunderstore 的标准声明包含版本和依赖，但不包含发布团队身份，不能仅凭本地 `manifest.json` 推断完整的在线包身份。[包格式](https://wiki.thunderstore.io/mods/creating-a-package)。

BepInEx 所用的部分 Mono 文件接口仍受 Windows 长路径限制影响；实例程序目录应保持简短。服务器就绪本身不证明 Mod 已加载，应检查新的 `BepInEx/LogOutput.log`，预加载失败时检查程序根目录的 `preloader_*.log`。[加载器路径排障](https://docs.bepinex.dev/articles/user_guide/troubleshooting.html#extremely-long-paths-with-non-ascii-characters)。

管理操作持有实例和程序目录的变更锁。文件移动前持久化操作记录；中断后在下一次变更或启动前恢复，库存读取遇到未完成操作时停止显示并提示恢复。出现文件冲突或记录变化时停止恢复并保留证据，不覆盖新文件。恢复目录属于实例数据，应随实例程序目录保留，不应手工清理。

### 隔离原生验收

Windows 的显式验收测试 `isolated_valheim_thunderstore_file_mod_lifecycle` 使用只读的已安装 Valheim 源包，新建独立程序副本、实例、数据库和端口；拒绝管理员运行，不修改防火墙。先准备源包、官方 [Server_devcommands 1.115.0 ZIP](https://thunderstore.io/c/valheim/p/JereKuusela/Server_devcommands/1.115.0/) 和已存在的证据目录，全部位于仓库之外：

```powershell
$env:LGSM_FILE_MODS_NATIVE = 'true'
$env:LGSM_FILE_MODS_SOURCE_PACKAGE = 'C:\GamePackages\Valheim'
$env:LGSM_FILE_MODS_LOCAL_ARCHIVE = 'C:\ModPackages\JereKuusela-Server_devcommands-1.115.0.zip'
$env:LGSM_FILE_MODS_EVIDENCE_ROOT = 'C:\ModEvidence'
cargo test --locked -p langame-desktop --bin langame-desktop isolated_valheim_thunderstore_file_mod_lifecycle -- --ignored --test-threads=1 --nocapture
```

测试通过正式命令执行在线安装、禁用、重新启用、删除和本地重导入的五轮启动，核对新运行身份、实际就绪、插件加载及配置生成。依赖保护使用不含可执行文件的元数据使用方；不据此宣称另一个第三方依赖插件的玩法已验证。源包全树、配置、加载器和保留文件均核对 SHA；每轮日志及 JSON 报告写入新的证据子目录。已确认停服后才清理测试副本。

### 其他来源与运行环境边界

文件来源相同不代表加载方式相同。当前 12 个文件型工作流提供通用纳管；以下来源或运行环境仍有明确边界。此表说明功能覆盖，不能代替具体游戏、加载器和 Mod 版本的实际启动验收。

| 来源 / 游戏 | 当前可用范围 | 尚未接入的部分 |
| --- | --- | --- |
| mod.io / Core Keeper | 可导入兼容的原生包目录，每包目录包含 `ModManifest.json` | mod.io 在线下载和依赖解析；根目录直接包含清单的 ZIP 需先整理为独立包目录。[原生目录说明](https://github.com/CoreKeeperMods/Core-Keeper-Docs/blob/main/playing-with-mods/installing-mods/on-dedicated-servers.md) |
| Modrinth / Minecraft | 匹配已自行准备的运行环境的本地 JAR 纳管 | Fabric、Forge、NeoForge、Quilt 运行环境与版本绑定；自动 Modrinth 安装目前拒绝执行。Paper/Hangar/Spigot 的 `plugins` 流程也未接入。[Fabric](https://fabricmc.net/use/server/)、[Paper](https://docs.papermc.io/paper/adding-plugins/) |
| uMod/Oxide / Rust | 外部准备 Oxide 后，`oxide/plugins` 中本地 `.cs` 纳管 | 在线安装、加载器验证；Carbon 的独立目录和 `.cszip` 包不在此流程内。[uMod](https://umod.org/documentation/plugins/installation)、[Carbon](https://carbonmod.gg/devs/features/zip-script-packages) |
| SOTF Mods / Sons of the Forest | 外部准备 RedLoader 后，`Mods` 中本地 DLL 和独立目录纳管 | RedLoader 安装、跨 `Mods` / `Libs` 的包布局与依赖、网站下载。[安装说明](https://sotf-mods.com/install#dedicated-servers) |
| Necesse | 本地 JAR 和独立包目录纳管 | 在线下载及运行依赖识别。[Mod 说明](https://necessewiki.com/Modding) |
| FICSIT / Satisfactory | 查看目录与库存，使用 SMM 或 ficsit-cli 管理 | 通用导入、启停和删除暂时拒绝执行。`.smod` 不能原样复制；还缺 SML、服务器平台、`.uplugin` 目录和 `GameFeature` 路由的统一适配。[包布局](https://docs.ficsit.app/satisfactory-modding/latest/ManualInstallDirections.html) |
| AML / Astroneer | 查看目录与库存，使用 AstroModLoader Classic 管理 | 通用导入、启停和删除暂时拒绝执行。每次清单变化都须重新生成 `999-AstroModIntegrator_P.pak`，只移动原始 `.pak` 无法证明禁用生效。[整合说明](https://astroneermodding.readthedocs.io/en/latest/guides/faq.html#can-i-use-mods-on-dedicated-servers) |
| Rocket / OpenMod / Unturned | 现有 Steam Workshop 资产流程 | 独立插件运行环境、插件目录和包管理。[Rocket](https://docs.smartlydressedgames.com/en/stable/servers/rocket.html)、[OpenMod](https://docs.smartlydressedgames.com/en/stable/servers/openmod.html) |
| TShock / Terraria | 现有原生和 tModLoader 流程 | TShock 入口、`ServerPlugins` 及独立配置 / 数据库。[TShock](https://github.com/Pryaxis/TShock/wiki) |

GitHub、作者网站和 ModDB 可以作为文件来源；使用当前支持格式的本地导入不等于接入了其网站 API，也不支持任意跨游戏根目录的安装包。

## English

Open **Mods → My Mods** for the instance. Imported local packages from Nexus Mods, Thunderstore, CurseForge and other sources share the file management controls. Files downloaded in a browser must first be imported. Game-managed Workshop and CurseForge IDs retain their existing enablement workflows; removing an instance entry does not purge shared download caches.

- **Manage files** records ownership for an identifiable existing Mod package. Official files, loaders, configuration directories and unrelated files are protected.
- **Disable / enable** requires a stopped server and moves a package outside its loading directory or restores it. Conflicting destinations are preserved. Disabled packages remain in My Mods.
- **Delete** checks dependencies and removes the package from the loading directory. Only unchanged recorded non-configuration files are deleted. Configuration, saves, additional files and modified files remain in the recovery location shown in the result. Dependencies and loaders are never removed automatically.

Reimports and online updates also retain existing configuration. Conflicting defaults reject the entire update; configuration omitted from the new package is retained.

Server downloads share the same login console; see [server installation and program isolation](server-configuration.md#instance-isolation). Workshop downloads use anonymous login by default. A confirmed account-access failure opens **LanGameCMD → SteamCMD** for the current instance and explains why sign-in is required. Generic failures and network errors do not trigger login. Enter the Steam account name or `login STEAM_ACCOUNT_NAME`. Native login starts immediately, and the actual Steam response determines the next step without a verification-method choice. A hidden password prompt appears only if Steam requests it; a valid cached login can continue directly. Accounts without a second-factor challenge finish directly. Email challenges open hidden email-code input; mobile challenges open current five-character token input and can also be approved in the mobile app. Native SteamCMD cannot receive a token code while blocked on mobile approval. Submitting a token fully reaps that attempt and starts one token-login attempt within the same operation, without an unbounded restart loop or silent fallback to mobile approval. Login reports the actual queue, SteamCMD preparation, Steam connection, account authentication and cached-login verification stages separately. The password remains only inside the native login worker until authentication is confirmed or the operation ends. Passwords and codes are masked, cleared immediately on submission, and excluded from process arguments, temporary scripts, game command history and instance logs. Successful authentication returns to Mods and continues the original download once. If the account still lacks access, the error remains without another login loop. Anonymous, authenticated-account and switch-account controls are in the SteamCMD console. Selection and authentication remain local to the current interface session. Changing instances or leaving the workspace cancels unfinished login and its waiting download. LAN connections require login in the desktop app on the server host.

A previously verified managed SteamCMD can update its native files and invalidate its old readiness record. Mod downloads, server downloads and account login automatically run one bounded `+quit` verification under the same runtime lock. Only successful verification refreshes the record and continues the operation. A missing or never-verified SteamCMD still requires installation or inspection in System; a readiness error alone does not request account login.

Managed SteamCMD downloads, interactive login and cached-login verification reuse the verified local update manifest to avoid repeated bootstrap update requests. Official source alternatives cover SteamCMD installation and updates. Account authentication and Workshop file transfer remain native SteamCMD operations outside the manager's regional HTTP source selection. See [official network sources](official-network-sources.md) for the delivery boundaries.

The manager does not save passwords or Guard codes. SteamCMD maintains its own local login token in `config/config.vdf`, allowing subsequent login with just the account name. See [Valve's login-token guidance](https://partner.steamgames.com/doc/sdk/uploading). Account downloads verify the actual login identity each time. An expired token requires login again without switching to anonymous. Steam decides authentication and content access separately; successful login alone does not establish access to every game or Mod.

When a Palworld Workshop download explicitly reports `Missing decryption key`, Steam did not provide the content key to that session; a generic `Failure` does not establish this cause. Sign in to SteamCMD with an account that has access and retry. Alternatively, subscribe to and finish downloading the Mod and its dependencies in Steam. Open **Mods → My Mods** for the instance, then drag the corresponding Mod folders from `steamapps/workshop/content/1623730` in the Steam library directly into the Mod list. Drop feedback appears only while dragging. Prepare dependencies as required by the author and inspect actual loading logs after import.

Declared dependencies are checked by identity and version. Enabling requires matching enabled dependencies with intact recorded payloads. Disabling blocks enabled dependents; deletion blocks all managed dependents. Handle the named dependents before their requirements.

Package metadata cannot establish every runtime requirement of arbitrary third-party code. Packages without a complete declaration are labelled accordingly; consult the author for loader, client and compatibility requirements. The standard local Thunderstore manifest has no publishing-team identity, so it cannot establish the full online package identity. See the [official package format](https://wiki.thunderstore.io/mods/creating-a-package).

Some Mono file APIs used by BepInEx still have Windows path-length limits. Keep instance program paths short. Server readiness alone does not establish Mod loading: inspect a fresh `BepInEx/LogOutput.log`, or root-level `preloader_*.log` files when preloading fails. See the [loader troubleshooting guide](https://docs.bepinex.dev/articles/user_guide/troubleshooting.html#extremely-long-paths-with-non-ascii-characters).

Operations own the instance and program-directory leases. Durable intent precedes file moves; subsequent mutations and startup recover interrupted work. Inventory reads reject pending work without changing files. Conflicts or changed records stop recovery while preserving evidence. Keep recovery data with the instance program directory.

### Isolated native acceptance

The Windows opt-in test `isolated_valheim_thunderstore_file_mod_lifecycle` uses the four environment variables and portable command above. Supply a read-only installed Valheim source, the official pinned Server_devcommands 1.115.0 ZIP and an existing evidence directory, all outside the repository. The test refuses elevation and does not change firewall rules. It copies its own program, instance and database, selects independent ports, and performs five production start/stop cycles for online installation, disabling, re-enabling, deletion and local reimport. Fresh logs establish actual readiness, plugin loading and configuration initialization. Dependency refusals use a metadata-only consumer rather than another executable plugin. Whole-source and protected-file SHA checks, logs and a JSON report are retained outside the disposable fixture; fixture removal requires confirmed shutdown.

### Additional ecosystems and runtime boundaries

Twelve file workflows currently provide generic management. Local file support does not establish compatibility with every loader or distribution layout. Core Keeper's mod.io, Minecraft loader/version binding and Paper plugins, Rust Carbon, Unturned Rocket/OpenMod and Terraria TShock still require dedicated integration. Existing local routes cover Oxide scripts, RedLoader Mod directories and Necesse JARs after their runtimes are prepared externally. GitHub, author sites and ModDB are file sources rather than integrated download providers.

Astroneer and Satisfactory expose inventory and directory access while generic imports and mutations are blocked. Astroneer must regenerate its integrated package after every Mod-list change through [AstroModLoader Classic](https://astroneermodding.readthedocs.io/en/latest/guides/faq.html#can-i-use-mods-on-dedicated-servers). Satisfactory needs SMM/ficsit-cli and the correct server-compatible `.uplugin` / `GameFeature` [package layout](https://docs.ficsit.app/satisfactory-modding/latest/ManualInstallDirections.html). Moving original files alone cannot establish correct enablement for these workflows.
