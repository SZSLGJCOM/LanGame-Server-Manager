# Mod 管理 / Mod management

## 简体中文

ARK SE 和 ASA 默认共用 Mod 配置。在「我的 Mod」选中一个 Mod，右侧参数栏的「生效地图」支持同时勾选多张地图，只改变当前 Mod；其他 Mod 继续继承共用默认。启用范围与参数保存范围分别设置，修改两张地图的参数不会改变启用状态。恢复共用启用设置保留已保存参数；恢复共用参数只清除当前 Mod 的参数覆盖。商店和清单导入修改共用默认。地图之间的实际 Mod 或参数不同时显示跨服兼容提示，允许继续保存。

ASA 的 CurseForge Mod 使用游戏内置更新流程，无需用户申请 API Key。手动启动、重启及自动启动先由 ASA 进程初始化服务端 Mod：官方 SDK 每次查询当前服务端适配版本，比较已有包的文件 ID，下载或修复不匹配的内容。管理器等待当前地图、当前进程启动时间之后的每个启用或被动加载项目的最终 `Mod valid` 记录，并核实该进程持有游戏端口，才完成启动；单地图绑定 `0.0.0.0` 也执行核验。多地图依次初始化，避免同时写同一个原生缓存。更新失败、证据缺失、超时或取消都会停止本次启动，既有缓存不单凭文件存在通过。原生准备阶段最多 15 分钟，完成后进入普通监听端口预算；停止按钮可取消等待。LanGameCMD 显示检查和最终校验进度。保留 Mod 配置、存档及原生缓存，由 ASA 负责包的更新。作者的 `-dev` 测试版本按原配置核验，不改成公开版。

这个流程在 ASA 进程初始化期间、服务器进入运行状态前完成更新。游戏进程创建前的独立 CurseForge 下载尚未接入；普通第三方 API Key 的 ASA 服务端包权限不能直接假定。其他游戏的 CurseForge 本地包导入也不等同于自动更新。版本选择与缓存核验依据见 [CurseForge 官方服务端流程](https://docs.curseforge.com/docs/game-integration/unreal/multiplayer-and-servers/)。当前完成日志格式核对了 [CFCore SDK 1.47.0 官方发布包](https://github.com/curseforge-sdks/cfcore-sdk-ue/releases/tag/1.47.0)。若自定义日志参数屏蔽 `LogCFCore` 的普通日志，管理器无法确认完成，需恢复日志后重试。

`Error querying server mods` / `serverUnreachable` 表示本次服务端版本查询未成功，不能据此认定缓存损坏或需要用户 API Key。失败提示会检查 SDK 安装记录、官方模块指纹、文件长度和当前内容，并区分缓存完整、无法证明完整及未检查。在已验证的 Windows ASA 94.29 / CFCore 1.47.0 中，版本查询连接失败可能导致游戏进程在 Mod 缓存完整时仍自行退出，无法加载 Mod 或进入可加入状态。管理器无法用放宽检查代替游戏原生初始化，也不会移除 Mod 参数启动无 Mod 服务器。SDK 未提供已核实的离线加载入口，飞升的更新失败缓存开服仍受此原生限制。SDK 由官方接口返回下载域名，没有已接入的国内／国际 Mod 镜像开关。

ASA 默认不附加代理参数，通过服务器主机自己的网络访问官方 CurseForge 服务。主机网络及用户自行使用的代理由用户维护，LanGame 不安装、探测或自动配置代理。服务可达性取决于服务器主机的实际网络环境。

启动失败提示区分版本查询、安装和最终验证，按原生日志中明确的错误类别或 HTTP 状态显示中英文说明，并保留实例、地图、本次原生日志位置、缓存核验结果及进程退出信息。完整缓存不代表最新版，也不保证原生进程能离线加载；不再将网络报错统一解释为缓存损坏或“必须最新版”。`serverUnreachable` 本身不能区分 DNS、TLS、连接或超时。下载中的重试或差量下载回落完整包不等于最终失败，不提前中断游戏原生恢复流程。缓存诊断有独立 10 秒上限且可取消；缺少官方指纹等完整性证据会如实提示无法确认，不声称损坏。

隔离实机验收用例 `asa_curseforge_cached_native_start_stop_and_failure` 要求独立、已脱敏的 ASA 程序副本与缓存，不接受存档或 SDK 用户上下文；它通过正式启动入口验证缓存启动、地图就绪、正常停服及不可用项目阻止启动。可选环境变量 `LANGAME_ASA_CF_HTTP_PROXY=127.0.0.1:PORT` 只接受有效回环端口，其他任意测试启动参数仍会清空。测试代理须提前提供，测试不安装代理、不修改系统代理或 DNS；仅创建并清理本隔离实例的临时防火墙规则，需要管理员权限。原生 HTTP 调试日志可能包含请求凭据或完整下载 URL，仅保留时间、项目／文件 ID、错误分类和校验结果，不导出原始 HTTP 日志。

进入实例的 **Mod → 我的 Mod** 管理该实例中的文件。Nexus Mods、Thunderstore、CurseForge 等来源的本地包导入后使用同一套文件管理操作；来源网站下载到电脑的文件仍需先导入实例。非 Steam 创意工坊的 **商店** 提供独立的文件拖入安装、来源网站入口及适用的链接／ID 添加，不显示实例已有的 Mod 列表；导入结果在 **我的 Mod** 中管理。支持本地导入的 Steam 创意工坊游戏仍可直接拖入 **我的 Mod** 安装。游戏原生管理的 Workshop、CurseForge ID 和启用列表沿用各游戏已有的操作，不删除共享下载缓存。

非生物因素、七日杀、人类 Z 和雾锁王国的 Nexus 商店提供官方网站入口。请从官网手动下载 Mod，再将文件或目录拖入商店安装；导入后在 **我的 Mod** 中管理。安装沿用各游戏的本地导入、文件归属和回滚保护。请按游戏及作者说明准备匹配的加载器、依赖和服务端包布局；项目页面本身不能证明文件适用于服务器。不支持的压缩格式需先在本地解压，再导入适用的文件或目录。Nexus 本地包不参与在线版本追踪或启动自动更新。

- **纳入管理**：为此前已放入 Mod 目录的包建立文件归属记录。只有可识别的 Mod 载荷或包标记才能纳管；官方文件、加载器、配置目录和无关文件不能作为普通 Mod 接管。
- **禁用 / 启用**：停止服务器后，把包移出游戏的加载目录，或者恢复到原位置。同名位置已有其他内容时拒绝覆盖。禁用的包继续显示在“我的 Mod”中。
- **删除**：先检查依赖，再移出加载目录。只清理归属记录中内容未变的非配置文件；配置、存档、新增文件、用户修改过的文件保留在恢复位置，可从操作结果打开。其他包及加载器不会随依赖一起删除。

重新导入或在线更新也会保留既有配置。新包试图覆盖配置时会拒绝整批更新；新包省略的配置继续保留。

启动、重启及自动启动会在服务器程序更新后、游戏进程创建前检查当前实例实际加载的 Steam Workshop Mod。完整缓存也执行原生 SteamCMD 版本检查与校验，有更新时下载并部署。更新失败时，若旧版载荷能通过官方文件清单哈希或先前成功校验生成的逐文件 SHA-256 凭据确认完整，就恢复旧缓存并检查实际部署文件，使用已有版本继续启动。后台任务、启动结果及界面警告说明失败原因、缓存使用及尚未确认最新版；没有完整可验证缓存时阻止启动。取消、生命周期锁或保护规则失败仍停止操作，不以回退绕过保护。已有启用列表、Mod 设置及不相关实例的缓存不变。实际部署目录中的原有配置和用户新增文件有独立保护：新包不变时保留原目录；需要整目录替换时先尝试回到已核验的旧包。用户改过的随包配置、下载期间发生的目录变化、清单外的未知可加载文件或不能证明完整性的包会拒绝自动替换，保留文件并提示冲突。Palworld 已启用但目录缺失的 Mod 只从本实例自己的下载记录恢复身份，不猜测共享缓存中同名包的归属。

Valheim、V Rising 和 Core Keeper 的受管 Thunderstore 在线包也会在启动、重启及自动启动前检查。安装时记录用户直接请求的根包、依赖身份和版本策略：`follow_latest` 在下次启动查询根包的当前版本；`pinned` 保持明确选定的根包版本，依赖按该版本声明解析，不自动升到最新版。已安装的版本号本身不代表用户选择了固定版本。禁用和删除的包不作为启动更新根包；启动检查要求现有加载器、启用包及依赖完整，不自动替换加载器。Core Keeper 只接纳当前原生 `ModManifest.json` 包，不把旧 BepInEx 包作为原生 Mod 安装。

Thunderstore 的版本查询或网络下载未完成且尚未发布新包时，会再次核验当前文件、依赖、加载器及归属记录，确认完整后使用现有内容继续启动，并提示版本尚未确认。缺失、损坏或不匹配的现有载荷和依赖、配置冲突、归属记录变化、发布失败及取消仍阻止启动。缺少原始根包或版本策略的旧 Thunderstore 在线记录不会由已安装版本反推：当前内容核验通过后保留使用，并提示“最新版未检查”，需在 Mod 页明确来源和策略。仅本地导入的文件没有可信在线身份，不会因文件名或 `manifest.json` 自动绑定同名网络项目。

服务端下载也复用同一登录控制台，详见[服务端安装与程序隔离](server-configuration.md#实例隔离)。Workshop 默认匿名下载。匿名下载明确遇到账号权限问题时，界面自动转到当前实例的 **LanGameCMD → SteamCMD**，说明原因并等待登录；普通失败或网络错误不会触发。输入 Steam 账号名或 `login STEAM_ACCOUNT_NAME`，立即开始原生登录，由 Steam 的实际返回决定下一步，无需预先选择验证方式。只有 Steam 要求密码时才显示隐藏密码输入，登录缓存有效时可直接继续。无需二次验证时直接完成登录；邮箱验证时显示隐藏的邮箱验证码输入；手机验证时显示当前五位令牌码输入，也可在手机应用中确认。原生 SteamCMD 无法在等待手机确认时直接接收令牌码，提交令牌后会在同一操作内完整回收旧登录进程，再启动一次令牌登录；不会无限重启或静默改成手机确认。登录分别显示等待运行、准备 SteamCMD、连接 Steam、验证账号和确认登录缓存的实际阶段。密码仅在本次原生登录工作线程内保留到认证确认或操作结束；密码和验证码隐藏显示、提交后立即清空，不进入进程启动参数、临时脚本、游戏命令历史或实例日志。登录成功后自动返回 Mod 并继续原下载一次；账号仍无内容权限时保留错误，不重复登录。匿名、已登录账号和切换账号操作统一位于 LanGameCMD 的 SteamCMD 控制台。账号选择和认证状态只在当前界面会话内保留，切换实例或退出工作区会取消尚未完成的登录及等待中的下载。LAN 远程连接需在服务器主机的桌面应用中完成登录。

已验证的受管 SteamCMD 自更新后，原生文件变化会使旧就绪记录过期。Mod 下载、服务端下载和账号登录会在同一运行时锁下自动执行一次限时 `+quit` 复核，核验成功才刷新记录并继续。首次缺少或尚未验证的 SteamCMD 仍需到系统页安装或检查；就绪错误本身不触发账号登录。

受管 SteamCMD 的下载、交互登录及登录缓存复核复用已核验的本地更新清单，避免重复访问引导器更新站点。官方多源用于 SteamCMD 安装与更新；账号认证连接和 Workshop 文件传输仍由原生 SteamCMD 处理，不经过管理器的国内外 HTTP 源选择。各类来源及边界见[官方网络来源](official-network-sources.md)。

管理器不保存密码或验证码；SteamCMD 自身在本机的 `config/config.vdf` 中维护登录令牌，以便后续仅用账号名登录。[Valve 登录令牌说明](https://partner.steamgames.com/doc/sdk/uploading)。每次账号下载都重新核实实际登录身份；令牌失效时提示重新登录，不会自动改用匿名下载。账号认证和游戏内容访问权限分别由 Steam 判断，登录成功本身不能保证任意游戏或 Mod 可下载。

幻兽帕鲁的 Workshop 下载若明确报告 `Missing decryption key`，表示 Steam 未向本次会话提供内容解密密钥；普通 `Failure` 本身不能确定这一原因。可使用有访问权限的账号登录 SteamCMD 后重试。也可在 Steam 中订阅并完成下载 Mod 及其依赖，再到 Steam 库的 `steamapps/workshop/content/1623730`，切到实例的 **Mod → 我的 Mod**，将对应 Mod 文件夹直接拖入下方 Mod 列表。列表只在拖入时显示接收反馈。按作者要求准备依赖，导入后检查实际加载日志。

SteamCMD 也可能在外层显示 `No Connection`，但内容日志记录获取下载清单时被 Steam 拒绝（`Failed to get manifest request code, 'Access Denied'`）。只有本次调用新增的拒绝记录与当前请求的 Workshop 条目及清单对应时，管理器才进入账号登录重试流程；历史记录、其他条目的拒绝和普通网络失败不会触发。登录后的内容访问仍由 Steam 决定。

下载过程中，管理器独立收集本次调用的关键失败及条目、清单证据，不依赖原始输出末尾 40 行。即使 SteamCMD 退出码为 0 或没有控制台失败行，当前应用的明确权限拒绝、缺少解密密钥或已对应的清单拒绝仍会参与结果判断。证据不足时保留原始诊断，不把普通网络或本地文件访问失败当成账号权限问题。

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

CurseForge 是分发来源，不是所有游戏共用的加载器。ASA 已在游戏里集成官方 SDK，管理器只驱动原生流程；其他游戏需分别选择匹配的游戏版本、加载器、服务端文件及依赖，并保留 CurseForge 项目／文件身份，才能自动更新。当前本地导入包不具有这些在线身份。链接或项目 ID 解析只确定项目，不代表已下载普通包、建立文件版本跟踪或获得其他游戏的 API 授权。普通第三方 API 接入还需要[获批的应用凭证](https://support.curseforge.com/support/solutions/articles/9000208346-about-the-curseforge-api-and-how-to-apply-for-a-key)，且遵守作者的[第三方分发选择](https://support.curseforge.com/support/solutions/articles/9000207877)；开源并不自动获得接口或下载权限。

文件来源相同不代表加载方式相同。当前 12 个文件型工作流提供通用纳管；以下来源或运行环境仍有明确边界。此表说明功能覆盖，不能代替具体游戏、加载器和 Mod 版本的实际启动验收。

| 启动更新来源 / 游戏 | 在线身份和策略 | 当前启动行为 |
| --- | --- | --- |
| Steam Workshop / ARK SE、Palworld、Project Zomboid、Don't Starve Together、Squad、Conan Exiles、Barotrauma、Unturned、Soulmask、Terraria | 当前实例实际启用／加载的 Workshop ID；Terraria 仅限 tModLoader | 10 个服务端工作流在进程创建前用 SteamCMD 检查更新和校验；失败仅在完整旧载荷及实际部署均可核验时使用缓存并告警。RimWorld Together 声明的 Workshop 内容供客户端使用，不在此服务端更新范围内。 |
| CurseForge / ASA | 游戏原生项目 ID、地图启用／被动列表与官方服务端适配文件 | 由本次 ASA 进程查询、下载及最终校验，无需用户 API Key；原生联网失败可能使游戏退出，完整缓存不构成离线开服承诺。 |
| Thunderstore / Valheim、V Rising、Core Keeper | 官方包身份、根包／依赖角色、`follow_latest` 或 `pinned` 意图随安装事务保存 | 3 个工作流启动前按保存的根包策略查询；同版本且依赖声明不变时复核缓存，所需新包通过现有包事务发布。旧在线记录意图未知时核验当前内容并提示未检查最新版。 |
| 本地文件 / Nexus、非 ASA CurseForge、Modrinth、mod.io、作者网站等 | 本地归属、文件哈希及可识别的依赖信息；不据此猜测在线项目／版本策略 | 保留本地管理；未接入自动追踪这些来源。Minecraft 当前正式安装入口为 Vanilla，不能通过导入 JAR 获得已验证的 Mod 加载器。 |

| 来源 / 游戏 | 当前可用范围 | 尚未接入的部分 |
| --- | --- | --- |
| CurseForge / Windrose | 兼容的本地 `.pak` / `.ucas` / `.utoc` 和 ZIP 文件纳管 | 官方 API 授权、在线项目／文件身份、游戏版本与专服文件布局选择及依赖更新。不同格式可能有独立安装要求，不能把全部项目 ZIP 泛装到 `~mods`。[官方安装说明](https://blog.curseforge.com/how-to-install-mods-for-windrose-the-easiest-way/) |
| CurseForge / Minecraft | 与外部准备的兼容运行环境匹配的本地 JAR 纳管 | 官方 API 授权、加载器／游戏版本绑定、服务端适配文件与依赖选择、在线身份及启动更新。现有正式模块只安装 Vanilla，`supports_mods = false`。 |
| mod.io / Core Keeper | 可导入兼容的原生包目录，每包目录包含 `ModManifest.json` | mod.io 在线下载和依赖解析；根目录直接包含清单的 ZIP 需先整理为独立包目录。[原生目录说明](https://github.com/CoreKeeperMods/Core-Keeper-Docs/blob/main/playing-with-mods/installing-mods/on-dedicated-servers.md) |
| Modrinth / Minecraft | 匹配已自行准备的运行环境的本地 JAR 纳管 | Fabric、Forge、NeoForge、Quilt 运行环境与版本绑定；自动 Modrinth 安装目前拒绝执行。Paper/Hangar/Spigot 的 `plugins` 流程也未接入。[Fabric](https://fabricmc.net/use/server/)、[Paper](https://docs.papermc.io/paper/adding-plugins/) |
| Nexus / Abiotic Factor、7 Days to Die、HumanitZ、Enshrouded | 官方网站入口；手动下载后导入并管理兼容本地包 | 应用内下载、在线版本追踪、依赖自动安装和启动更新；需按游戏及作者说明准备匹配的加载器和服务端包布局。 |
| uMod/Oxide / Rust | 外部准备 Oxide 后，`oxide/plugins` 中本地 `.cs` 纳管 | 在线安装、加载器验证；Carbon 的独立目录和 `.cszip` 包不在此流程内。[uMod](https://umod.org/documentation/plugins/installation)、[Carbon](https://carbonmod.gg/devs/features/zip-script-packages) |
| SOTF Mods / Sons of the Forest | 外部准备 RedLoader 后，`Mods` 中本地 DLL 和独立目录纳管 | RedLoader 安装、跨 `Mods` / `Libs` 的包布局与依赖、网站下载。[安装说明](https://sotf-mods.com/install#dedicated-servers) |
| Necesse | 本地 JAR 和独立包目录纳管 | 在线下载及运行依赖识别。[Mod 说明](https://necessewiki.com/Modding) |
| FICSIT / Satisfactory | 查看目录与库存，使用 SMM 或 ficsit-cli 管理 | 通用导入、启停和删除暂时拒绝执行。`.smod` 不能原样复制；还缺 SML、服务器平台、`.uplugin` 目录和 `GameFeature` 路由的统一适配。[包布局](https://docs.ficsit.app/satisfactory-modding/latest/ManualInstallDirections.html) |
| AML / Astroneer | 查看目录与库存，使用 AstroModLoader Classic 管理 | 通用导入、启停和删除暂时拒绝执行。每次清单变化都须重新生成 `999-AstroModIntegrator_P.pak`，只移动原始 `.pak` 无法证明禁用生效。[整合说明](https://astroneermodding.readthedocs.io/en/latest/guides/faq.html#can-i-use-mods-on-dedicated-servers) |
| Rocket / OpenMod / Unturned | 现有 Steam Workshop 资产流程 | 独立插件运行环境、插件目录和包管理。[Rocket](https://docs.smartlydressedgames.com/en/stable/servers/rocket.html)、[OpenMod](https://docs.smartlydressedgames.com/en/stable/servers/openmod.html) |
| TShock / Terraria | 现有原生和 tModLoader 流程 | TShock 入口、`ServerPlugins` 及独立配置 / 数据库。[TShock](https://github.com/Pryaxis/TShock/wiki) |

GitHub、作者网站和 ModDB 可以作为文件来源；使用当前支持格式的本地导入不等于接入了其网站 API，也不支持任意跨游戏根目录的安装包。

## English

ASA's CurseForge Mods use the game's native updater without an operator-provided API key. Manual starts, restarts and automatic starts let the ASA process initialize server Mods: the official SDK queries the current server-compatible revision, compares installed file IDs, and downloads or repairs mismatched content. Startup requires a final `Mod valid` record for every enabled or passive project in this map's native log after the owned process's creation time, plus its owned game listener. Single-map servers bound to `0.0.0.0` also validate. Maps initialize sequentially to avoid concurrent writers to the native cache. Failed, unconfirmed, timed-out or cancelled initialization stops this start; cache existence cannot establish success. Native preparation has a fixed 15-minute limit followed by the ordinary listener budget; Stop can cancel the wait. LanGameCMD reports checks and final validation progress. ASA retains ownership of package updates, with Mod settings, saves and native caches preserved by the manager. Author `-dev` revisions retain their requested testing behavior.

Updates run during ASA process initialization, before the manager completes startup. Independent CurseForge downloads before process creation are not integrated, and ordinary third-party keys cannot be assumed to grant ASA server-file access. Local CurseForge imports for other games do not provide automatic updates. See the [official server flow](https://docs.curseforge.com/docs/game-integration/unreal/multiplayer-and-servers/) and the [CFCore SDK 1.47.0 release](https://github.com/curseforge-sdks/cfcore-sdk-ue/releases/tag/1.47.0) checked for completion-log semantics. Custom logging that suppresses ordinary `LogCFCore` records prevents confirmation; restore logging before retrying.

`Error querying server mods` / `serverUnreachable` identifies a failed server-version query, not cache damage or a missing operator API key. Diagnostics inspect SDK records, published module fingerprints, content size and the current files, distinguishing verified caches, unverified caches and checks not performed. In the verified Windows ASA 94.29 / CFCore 1.47.0 combination, a failed query connection can cause the native process to exit without loading Mods or advertising, even with complete cached content. Relaxing a manager gate cannot replace native initialization, and LanGame retains the requested Mod arguments. No verified offline-loading entry point was found in the SDK; ASA cache fallback remains constrained by the game. The official service selects the download domain; no regional native Mod mirror selector is integrated.

ASA adds no proxy argument by default and accesses official CurseForge services through the server host's own network. Users maintain host networking and any proxy they choose; LanGame does not install, discover or automatically configure one. Service availability depends on the server host's network environment.

Startup errors distinguish queries, installation and final validation using recognized native categories or HTTP status, with instance, map, log, cache verification and process-exit context. A complete cache does not establish freshness or that ASA can load it offline. Network failure is not labeled cache corruption or a requirement to use the newest revision. Cache diagnostics have a separate ten-second bound and cancellation; missing integrity metadata is reported as unverifiable. Intermediate retries and delta-to-full download recovery remain nonterminal.

The isolated native test `asa_curseforge_cached_native_start_stop_and_failure` requires a separate sanitized ASA package and cache, and refuses saves or SDK user context. It uses the production start entry to verify cached starts, native map readiness, normal stops and unavailable-project rejection. Optional `LANGAME_ASA_CF_HTTP_PROXY=127.0.0.1:PORT` accepts only a valid loopback port; arbitrary fixture launch flags remain cleared. Supply the test proxy beforehand; the test neither installs one nor modifies the system proxy or DNS. It creates and removes only this fixture's temporary firewall rules and requires administrator access. Native HTTP diagnostics can contain credentials or full download URLs: retain only timestamps, project/file IDs, error classifications and validation results, never raw HTTP logs.

ARK SE and ASA inherit shared Mod defaults initially. Select a Mod in **Mods → My Mods** and choose multiple **Enabled maps** in its right-hand configuration pane. This changes only that Mod; other Mods keep inheriting shared defaults. Parameter save targets are independent of enablement. Restoring shared enablement retains saved options; restoring shared options removes only this Mod's parameter overrides. Store and manifest imports update shared defaults. Differences among enabled maps produce a transfer-compatibility warning without preventing saving. ASA drafts remain separate by selected maps, Mod, and INI file.

Open **Mods → My Mods** for the instance. Imported local packages from Nexus Mods, Thunderstore, CurseForge and other sources share the file management controls. Files downloaded in a browser must first be imported. Non-Steam Workshop **Store** pages provide a separate file-drop installation area, source website access and supported link/ID entry without showing the instance's Mod inventory; manage imported results in **My Mods**. Steam Workshop games with local-import support still accept file drops directly into **My Mods**. Game-managed Workshop and CurseForge IDs retain their existing enablement workflows; removing an instance entry does not purge shared download caches.

The Nexus Store for Abiotic Factor, 7 Days to Die, HumanitZ and Enshrouded provides access to the official website. Download Mods there manually, then drop supported files or folders into the Store to install them and manage the results in **My Mods**. Installation follows each game's existing local-import, file-ownership and rollback rules. Prepare matching loaders, dependencies and server-compatible package layouts according to the game and author; a project page alone does not establish server compatibility. Extract unsupported archives locally before importing supported files or folders. Nexus local packages are not tracked online or updated automatically at startup.

- **Manage files** records ownership for an identifiable existing Mod package. Official files, loaders, configuration directories and unrelated files are protected.
- **Disable / enable** requires a stopped server and moves a package outside its loading directory or restores it. Conflicting destinations are preserved. Disabled packages remain in My Mods.
- **Delete** checks dependencies and removes the package from the loading directory. Only unchanged recorded non-configuration files are deleted. Configuration, saves, additional files and modified files remain in the recovery location shown in the result. Dependencies and loaders are never removed automatically.

Reimports and online updates also retain existing configuration. Conflicting defaults reject the entire update; configuration omitted from the new package is retained.

Starts, restarts and automatic starts check the instance's loaded Steam Workshop Mods after server program updates and before game process creation. Complete caches still receive native SteamCMD version checks and validation. On update failure, a complete old payload proven by official manifest file hashes or a prior successful-validation SHA-256 receipt is restored and its actual deployment checked before continuing with the cached revision. Jobs, returned start results and UI warnings retain the failure classification, log and unconfirmed freshness. Missing, damaged or unverifiable required caches reject startup. Cancellation, lifecycle lease and protection failures still stop the operation. Mod enablement, settings and unrelated instance caches remain unchanged. Original configuration and additional operator files in the actual deployment have separate protection: an unchanged native package keeps the existing directory; a required whole-directory replacement first attempts verified old-package recovery. Modified configuration supplied by the package, deployment changes during downloads, unknown loadable files outside the package manifest, and unverifiable package contents refuse automatic replacement while preserving files and reporting the conflict. Missing enabled Palworld packages recover their identity only from this instance's own download records, never from matching names in shared caches.

Managed online Thunderstore packages for Valheim, V Rising and Core Keeper are also checked before starts, restarts and automatic starts. Installation records explicitly requested roots, dependency identities and version intentions: `follow_latest` queries the root's current version on subsequent startup; `pinned` keeps the selected root revision and resolves its declared dependencies rather than upgrading the root. An installed version number alone does not establish a user pin. Disabled and deleted packages are not startup roots. Startup requires an intact existing loader, enabled payloads and dependencies; it does not replace loaders automatically. Core Keeper accepts native `ModManifest.json` packages and rejects old BepInEx packages for this runtime.

If version queries or network downloads cannot complete before publication, startup rechecks the actual files, dependencies, loader and ownership record. Verified existing content can continue with a warning that freshness is unconfirmed. Missing, damaged or incompatible existing payloads or dependencies, configuration conflicts, changed ownership, publication failures and cancellation still stop startup. Older online Thunderstore records lacking original root or version intent retain their verified content with a “latest version not checked” warning until the source and strategy are explicitly confirmed in Mods. Local filenames and `manifest.json` cannot establish a matching online project identity.

Server downloads share the same login console; see [server installation and program isolation](server-configuration.md#instance-isolation). Workshop downloads use anonymous login by default. A confirmed account-access failure opens **LanGameCMD → SteamCMD** for the current instance and explains why sign-in is required. Generic failures and network errors do not trigger login. Enter the Steam account name or `login STEAM_ACCOUNT_NAME`. Native login starts immediately, and the actual Steam response determines the next step without a verification-method choice. A hidden password prompt appears only if Steam requests it; a valid cached login can continue directly. Accounts without a second-factor challenge finish directly. Email challenges open hidden email-code input; mobile challenges open current five-character token input and can also be approved in the mobile app. Native SteamCMD cannot receive a token code while blocked on mobile approval. Submitting a token fully reaps that attempt and starts one token-login attempt within the same operation, without an unbounded restart loop or silent fallback to mobile approval. Login reports the actual queue, SteamCMD preparation, Steam connection, account authentication and cached-login verification stages separately. The password remains only inside the native login worker until authentication is confirmed or the operation ends. Passwords and codes are masked, cleared immediately on submission, and excluded from process arguments, temporary scripts, game command history and instance logs. Successful authentication returns to Mods and continues the original download once. If the account still lacks access, the error remains without another login loop. Anonymous, authenticated-account and switch-account controls are in the SteamCMD console. Selection and authentication remain local to the current interface session. Changing instances or leaving the workspace cancels unfinished login and its waiting download. LAN connections require login in the desktop app on the server host.

A previously verified managed SteamCMD can update its native files and invalidate its old readiness record. Mod downloads, server downloads and account login automatically run one bounded `+quit` verification under the same runtime lock. Only successful verification refreshes the record and continues the operation. A missing or never-verified SteamCMD still requires installation or inspection in System; a readiness error alone does not request account login.

Managed SteamCMD downloads, interactive login and cached-login verification reuse the verified local update manifest to avoid repeated bootstrap update requests. Official source alternatives cover SteamCMD installation and updates. Account authentication and Workshop file transfer remain native SteamCMD operations outside the manager's regional HTTP source selection. See [official network sources](official-network-sources.md) for the delivery boundaries.

The manager does not save passwords or Guard codes. SteamCMD maintains its own local login token in `config/config.vdf`, allowing subsequent login with just the account name. See [Valve's login-token guidance](https://partner.steamgames.com/doc/sdk/uploading). Account downloads verify the actual login identity each time. An expired token requires login again without switching to anonymous. Steam decides authentication and content access separately; successful login alone does not establish access to every game or Mod.

When a Palworld Workshop download explicitly reports `Missing decryption key`, Steam did not provide the content key to that session; a generic `Failure` does not establish this cause. Sign in to SteamCMD with an account that has access and retry. Alternatively, subscribe to and finish downloading the Mod and its dependencies in Steam. Open **Mods → My Mods** for the instance, then drag the corresponding Mod folders from `steamapps/workshop/content/1623730` in the Steam library directly into the Mod list. Drop feedback appears only while dragging. Prepare dependencies as required by the author and inspect actual loading logs after import.

SteamCMD can also report `No Connection` while its content log records that Steam denied the download manifest request (`Failed to get manifest request code, 'Access Denied'`). Account login recovery starts only when a denial appended during this invocation matches the requested Workshop item and its manifest. Historical records, denials for other items, and ordinary network failures do not trigger login. Steam still determines content access after authentication.

During the download, the manager retains this invocation's decisive failures and item/manifest evidence independently of the final 40 output lines. Explicit permission failures, missing decryption keys for the current application and correlated manifest denials still affect the result when SteamCMD exits with code 0 or omits a console failure row. Insufficient evidence keeps the original diagnostic without treating ordinary network or local file-access failures as account permission problems.

Declared dependencies are checked by identity and version. Enabling requires matching enabled dependencies with intact recorded payloads. Disabling blocks enabled dependents; deletion blocks all managed dependents. Handle the named dependents before their requirements.

Package metadata cannot establish every runtime requirement of arbitrary third-party code. Packages without a complete declaration are labelled accordingly; consult the author for loader, client and compatibility requirements. The standard local Thunderstore manifest has no publishing-team identity, so it cannot establish the full online package identity. See the [official package format](https://wiki.thunderstore.io/mods/creating-a-package).

Some Mono file APIs used by BepInEx still have Windows path-length limits. Keep instance program paths short. Server readiness alone does not establish Mod loading: inspect a fresh `BepInEx/LogOutput.log`, or root-level `preloader_*.log` files when preloading fails. See the [loader troubleshooting guide](https://docs.bepinex.dev/articles/user_guide/troubleshooting.html#extremely-long-paths-with-non-ascii-characters).

Operations own the instance and program-directory leases. Durable intent precedes file moves; subsequent mutations and startup recover interrupted work. Inventory reads reject pending work without changing files. Conflicts or changed records stop recovery while preserving evidence. Keep recovery data with the instance program directory.

### Isolated native acceptance

The Windows opt-in test `isolated_valheim_thunderstore_file_mod_lifecycle` uses the four environment variables and portable command above. Supply a read-only installed Valheim source, the official pinned Server_devcommands 1.115.0 ZIP and an existing evidence directory, all outside the repository. The test refuses elevation and does not change firewall rules. It copies its own program, instance and database, selects independent ports, and performs five production start/stop cycles for online installation, disabling, re-enabling, deletion and local reimport. Fresh logs establish actual readiness, plugin loading and configuration initialization. Dependency refusals use a metadata-only consumer rather than another executable plugin. Whole-source and protected-file SHA checks, logs and a JSON report are retained outside the disposable fixture; fixture removal requires confirmed shutdown.

### Additional ecosystems and runtime boundaries

CurseForge supplies content, not one universal game loader. ASA integrates the official SDK inside the game; other adapters must select compatible game/loader versions, server files and dependencies, and retain CurseForge project/file identities. Current local imports do not have those online identities. Resolving a link or project ID does not download ordinary packages, establish file-version tracking or grant API access for other games. Ordinary third-party API access requires [approved application credentials](https://support.curseforge.com/support/solutions/articles/9000208346-about-the-curseforge-api-and-how-to-apply-for-a-key) and honors [author distribution choices](https://support.curseforge.com/support/solutions/articles/9000207877); open-source status alone does not grant API or download access.

Twelve file workflows currently provide generic management. Local file support does not establish compatibility with every loader or distribution layout. This matrix describes implemented startup checks, not native loading acceptance for every game, loader and Mod revision.

| Startup provider / games | Online identity and strategy | Current startup behavior |
| --- | --- | --- |
| Steam Workshop / ARK SE, Palworld, Project Zomboid, Don't Starve Together, Squad, Conan Exiles, Barotrauma, Unturned, Soulmask, Terraria | Workshop IDs currently enabled or loaded by this instance; Terraria only with tModLoader | Ten server workflows check and validate through SteamCMD before process creation. Failed updates use cache only after proving both the old payload and actual deployment, with a warning. RimWorld Together's declared Workshop content is client-side and outside this server update scope. |
| CurseForge / ASA | Native project IDs, per-map enabled/passive lists and official server-compatible files | The current ASA process queries, downloads and validates without an operator API key. Native network failure can exit the game; complete caches do not guarantee offline starts. |
| Thunderstore / Valheim, V Rising, Core Keeper | Official package identity, root/dependency role and `follow_latest` or `pinned` intent saved with installation | Three workflows query using recorded root intentions. Unchanged versions and dependency declarations reuse reverified content; required new packages use the existing package transaction. Unknown intentions in older online records produce an unchecked-freshness warning after current-content verification. |
| Local files / Nexus, non-ASA CurseForge, Modrinth, mod.io and author sites | Local ownership, file hashes and recognizable dependency information; no inferred online project or update strategy | Local management remains available; these sources are not automatically tracked. Minecraft's formal installer currently supplies Vanilla, and importing a JAR does not establish a verified Mod loader. |

| Provider / game | Available behavior | Integration still required |
| --- | --- | --- |
| CurseForge / Windrose | Management of compatible local `.pak`, `.ucas`, `.utoc` and ZIP content | Official API authorization, project/file identity, game-version and dedicated-server layouts, and dependency updates. Different formats need project-specific installation, so not every ZIP can be installed into `~mods`. [Official guide](https://blog.curseforge.com/how-to-install-mods-for-windrose-the-easiest-way/) |
| CurseForge / Minecraft | Local JAR management for a compatible externally prepared runtime | Official API authorization, verified loader/game versions, server-compatible files, dependencies, online identity and startup updates. The current formal module installs only Vanilla with `supports_mods = false`. |
| Modrinth / Minecraft | Local JAR management for an externally prepared runtime | Loader/version binding; automatic Modrinth installation is rejected. Paper/Hangar/Spigot plugin integration is also absent. |
| Nexus / Abiotic Factor, 7 Days to Die, HumanitZ, Enshrouded | Official website access; import and manage compatible packages after downloading them manually | In-app downloads, online version tracking, automatic dependency installation and startup updates; prepare matching loaders and server-compatible package layouts according to the game and author. |
| mod.io / Core Keeper | Import compatible native directories containing `ModManifest.json` | Online downloads and dependency resolution through mod.io; this differs from the integrated Thunderstore route. |

Rust Carbon, Unturned Rocket/OpenMod and Terraria TShock still require dedicated integration. Existing local routes cover Oxide scripts, RedLoader Mod directories and Necesse JARs after their runtimes are prepared externally. GitHub, author sites and ModDB are file sources rather than integrated download providers.

Astroneer and Satisfactory expose inventory and directory access while generic imports and mutations are blocked. Astroneer must regenerate its integrated package after every Mod-list change through [AstroModLoader Classic](https://astroneermodding.readthedocs.io/en/latest/guides/faq.html#can-i-use-mods-on-dedicated-servers). Satisfactory needs SMM/ficsit-cli and the correct server-compatible `.uplugin` / `GameFeature` [package layout](https://docs.ficsit.app/satisfactory-modding/latest/ManualInstallDirections.html). Moving original files alone cannot establish correct enablement for these workflows.
