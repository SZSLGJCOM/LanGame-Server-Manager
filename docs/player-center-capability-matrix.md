# 玩家中心能力矩阵 / Player Center capability matrix

当前 32 款游戏中，31 款已声明在线名单采集器，其中部分接口需要额外配置、扩展或公开玩家名称。本表记录模块声明的采集方式和操作归属，不表示已对每款游戏执行真实在线服务器验收。契约测试根据模块清单、Schema、模拟投影和响应样本校验 32 款游戏；协议解析及传输边界由 Rust 测试验证。

## 展示与操作规则

- `structured`：提供实时快照，并可对后端绑定的目标执行清单中明确声明的行操作。
- `read_only`：提供实时名称或身份信息，行本身没有管理动作。名称不等同于可用于管理的账号 ID。
- `manual_id`：尚无可靠的实时名单解析器，保留需要手工输入目标的既有管理操作。
- `unsupported`：尚无可用的实时名单采集器和手工玩家操作；仍可显示有来源的在线人数。
- 访问名单独立于在线名单。管理员、白名单、封禁记录均不用于生成在线玩家。
- 左侧选项卡只切换查看的名单；右侧共用一份模块支持的操作列表，未选择目标或目标不适用时按钮禁用。名单条目不会仅因保存了账号就获得在线动作权限，踢出等动作仍须匹配当前有效在线快照。
- 手工操作可与实时列表共存；列表触发动作、已绑定的行操作、访问名单消费的动作和广播不会重复出现在手工操作区。`pending_adapter` 下的运行管理动作不开放。

| Module / 模块 | Online mode / 在线模式 | Manual ID actions / 手工 ID 动作 | Access lists / 访问名单 | Evidence / 证据 |
| --- | --- | --- | --- | --- |
| `abioticfactor` | read_only | — | `admin` | `codec:a2s_players` |
| `arksurvivalascended` | structured | `unban_player`, `allow_no_check`, `disallow_no_check` | `admin`, `allow`, `priority` | `codec:ark_list_players` |
| `arksurvivalevolved` | structured | `unban_player` | `admin`, `allow`, `priority` | `codec:ark_list_players` |
| `astroneer` | read_only | — | — | `codec:astroneer_players` |
| `barotrauma` | read_only | — | `admin` | `codec:barotrauma_players` |
| `conanexiles` | structured | `unban_player` | — | `codec:conan_list_players` |
| `corekeeper` | read_only | — | `admin`, `block` | `codec:a2s_players` |
| `dontstarve` | structured | — | `admin`, `allow`, `block` | `codec:dst_client_table_v1` |
| `enshrouded` | read_only | — | `block` | `codec:a2s_players` |
| `humanitz` | read_only | `kick_player` | `admin`, `block`, `priority` | `codec:humanitz_players` |
| `minecraft` | read_only | `kick_player`, `ban_player`, `unban_player`, `op_player`, `deop_player`, `whitelist_add`, `whitelist_remove` | `admin`, `allow`, `block` | `codec:minecraft_players` |
| `necesse` | read_only | `check_permission`, `set_permission`, `ban_player`, `unban_player` | `admin` | `codec:necesse_players` |
| `nightingale` | read_only | — | — | `codec:nightingale_players` |
| `palworld` | structured | `unban_player` | — | `codec:palworld_players` |
| `projectzomboid` | structured | `ban_steamid`, `unban_user`, `set_access_level`, `remove_from_whitelist` | — | `codec:zomboid_players` |
| `returntomoria` | read_only | — | — | `codec:return_to_moria_players` |
| `rimworld` | manual_id | `kick_user`, `ban_user`, `unban_user`, `op_player`, `deop_player` | — | `unavailable:runtime_actions` |
| `romestead` | read_only | — | — | `codec:romestead_players` |
| `runescapedragonwilds` | read_only | — | — | `codec:dragonwilds_players` |
| `rust` | structured | — | `admin`, `block`, `priority` | `codec:rust_player_list` |
| `satisfactory` | read_only | — | — | `codec:satisfactory_frm_players` |
| `scum` | read_only | — | `admin` | `codec:scum_players` |
| `sevendaystodie` | structured | — | `admin`, `allow`, `block` | `codec:seven_days_players` |
| `sonsoftheforest` | read_only | — | `admin` | `codec:a2s_players` |
| `soulmask` | read_only | `kick_player`, `ban_player`, `unban_player` | — | `codec:soulmask_players` |
| `squad` | structured | `kick_player_id`, `ban_player_id` | `admin`, `priority` | `codec:squad_list_players` |
| `terraria` | read_only | `kick_player`, `ban_player` | `block` | `codec:terraria_players` |
| `theforest` | read_only | — | — | `codec:a2s_players` |
| `unturned` | read_only | `add_admin`, `remove_admin`, `kick_player`, `ban_player`, `unban_player`, `permit_player`, `unpermit_player` | `admin` | `codec:a2s_players` |
| `valheim` | read_only | — | `admin`, `allow`, `block` | `codec:a2s_players` |
| `vrising` | read_only | — | `admin`, `block` | `codec:a2s_players` |
| `windrose` | read_only | — | — | `codec:windrose_players` |

## 采集路径与限制

- **饥荒联机版**：Master 分片生成带请求标识的结构化名单；保留原有 Klei ID 和踢出能力。
- **Rust、ARK ASA/ASE、Conan Exiles、Project Zomboid、Squad**：分别解析服务器直接响应，校验当前协议的身份字段和可绑定动作。ARK 的 Steam、Epic 和 EOS 标识按实际格式区分；Conan 使用其内部 UserID，不能把全部 ID 都标成 Steam ID。
- **7 Days to Die**：Telnet `listplayers` 提供名称、平台账号和跨平台账号；管理目标使用服务器 `InternalId` 对应的跨平台账号，缺失时才使用平台账号。已识别的 Steam / EOS 账号支持从在线行踢出和立即封禁 10 年；不会用可复用的实体 ID 执行动作。即时封禁后从本实例原生 `serveradmin.xml` 核验目标记录，再保存到管理器黑名单并刷新左侧名单；保留原生到期时间的时分秒，不把发送命令等同于保存成功。管理员、白名单和持久黑名单从左侧切换，在线选中玩家后可直接填写名单操作所需参数。黑名单中的解除封禁同时移除持久条目，并在服务器运行时发送离线账号解封命令；其他未声明即时同步的名单变更仍在下次启动生效。契约依据本机安装包静态核验，协议样本为合成数据，不代表完成了真实多人服务器验收。
- **Minecraft Java**：RCON `list uuids` 提供名称和 UUID。格式依据本机安装的 Mojang 26.2 服务端；行只读。
- **Palworld**：REST API 返回名称、平台用户 ID 和延迟。当前运行的完整名单支持踢出和封禁，动作绑定响应中的 `userId`；解封通过手动用户 ID 操作执行。广播、保存和停服同样使用 REST API，成功必须收到 HTTP 200。新建实例默认启用 REST API 并生成管理员密码；已有实例需启用 REST API。官方已弃用 RCON，LanGame 不再通过它管理 Palworld。[官方 REST API](https://docs.palworldgame.com/category/rest-api/)
- **Nightingale**：HTTP `/status` 提供人数和名称；503 表示尚未就绪，不当成空名单。当前接口不提供可绑定管理动作的稳定身份。
- **HumanitZ**：原生 RCON 响应不能按标准 Source RCON 的请求 ID 和空命令结束帧读取。专用传输使用 `info` 的人数与 `Players:` 名称段判断完成，名称行只读；没有完整边界则失败。已测试 build 23914958 的默认 EOS 会话模式不回复 A2S，隔离副本改用 Steam 会话后可回复；生产保留原生联机方式，名单与人数使用认证后的完整 RCON 响应。默认关闭的 RCON 需要配置并启用，缺失或失败时人数保持未知。管理员、预留名额和封禁名单均使用包含 `|` 的完整 NetID，并保留原始大小写；将 `ReserveSlots` 设为 `MaxPlayers` 可通过 `F_ReservedSlots.txt` 限制准入。原生游戏已停用独立的 `F_MVPAccess.txt` 名单，管理器保留已有文件及设置数据，移除对应编辑入口。
- **Necesse、Romestead、Terraria**：发送只读命令后，从当前日志位置读取新增输出；人数、记录格式及完整行必须吻合。采集期间串行发送控制台命令，历史日志不进入本次快照。此类没有请求标识的控制台名单只读；Terraria 支持安装包内的本地化人数结束语。
- **Astroneer**：原生 TCP 控制台执行 `DSListPlayers`，只取 `inGame=true` 的玩家。需要非空控制台密码及独立 TCP 端口；空密码时配置模板关闭接口。GUID 按 Astroneer 身份显示，不标成 Steam ID。
- **Soulmask**：通过原生 Echo TCP 管理口执行 `lp`，按页码读取所有页面，并使用本轮随机未知命令的响应确认每页结束；最后 `dc` 仅关闭管理连接。Steam ID 来自在线表，列表只读，既有手工管理保留。
- **Barotrauma**：实例独立运行目录和 Windows ConPTY 提供真正的控制台输入输出；`clientlist` 带本轮请求标识，必须同时匹配回显及名单起止标记。仅显示原始客户端描述和延迟，不把含角色信息的描述猜成账号或为交互式管理命令伪造完成状态。
- **Return to Moria**：`native_console` 采集器通过独立 Windows 辅助进程附着受管服务器控制台，执行[官方只读 `players` 命令](https://northbeachgames.freshdesk.com/support/solutions/articles/154000217148-commands-while-the-server-is-running)。辅助进程校验 PID、创建时间和可执行文件身份，并以请求的起止标记限定响应；人数、记录数和结束标记必须一致。保留原始玩家标签，不把括号内容猜成账号 ID，行无管理动作。需要 `console_enabled=true` 并重启服务器，配置不完整时提供配置入口。实服版本与验收范围见下方“证据与验收边界”。
- **Satisfactory**：安装兼容的 [Ficsit Remote Monitoring](https://github.com/porisius/FicsitRemoteMonitoring/blob/main/docs/modules/ROOT/pages/dedicatedserver.adoc) 服务端扩展后，如果已开启 FRM HTTP 自动启动，管理器从本实例 `GameUserSettings.ini` 读取端口，经本机回环地址调用 `GET /api/getPlayer`；否则调用游戏 HTTPS API 的 `frm/getPlayer` 扩展。只取 `Online=true`，Actor ID 不当作账号。两个通道均不发送管理员令牌；缺少扩展、世界未就绪、鉴权失败或响应错误有独立状态。
- **Windrose**：安装已验证的 UE4SS 加载器及专用配置后，启动准备部署独立的 `LgsmPlayerQuery` 只读脚本。脚本在游戏线程读取当前世界的 `GameNetDriver.ClientConnections`，逐项关联控制器、玩家状态并与完整 `PlayerArray` 对照；登录未完成、世界切换、无效身份或调度器未响应均不能生成完整名单。桌面通过实例运行目录中的随机请求标识交换读取结果；`PlayerId` 仅作会话标识，行只读。安装要求与版本证据见 `modules/windrose/extensions/`。Windrose+ 的缓存状态、角色回退、存档及邀请码不进入此数据源。
- **RuneScape: Dragonwilds**：通过独立的 `LgsmPlayerQuery` 读取正式 `L_World` 的当前连接，逐项核对 Dominion 控制器、玩家状态、完整 `PlayerArray` 及游戏的只读 `IsPlayerReady` 检查。使用官方 UE4SS 固定源码自建的 `version.dll` 专服代理，无须复制 Nexus 或 Dev Kit 工具包。只有完整且匹配本轮请求的响应才能成为实时名单；`PlayerId` 仅作会话标识，行只读。安装和构建步骤见 `modules/runescapedragonwilds/extensions/README.md`；原生 A2S 与管理动作仍未接入。
- **SCUM**：通过独立 `LgsmPlayerQuery` 扩展读取正式 `The_Island` 世界的当前连接，并与完整玩家状态数组交叉核对。官方 UE4SS 加载器需按 `modules/scum/extensions/README.md` 安装；未就绪或扩展不可用时显示错误。名称和会话 ID 只读，不作为 Steam64 或管理目标。原生 A2S 在已测试版本中仍无响应。
- **Core Keeper、Enshrouded、Sons of the Forest、Unturned、Valheim、V Rising、The Forest、Abiotic Factor**：在已声明的查询端口尝试 A2S_PLAYER。协议只保证名称、分数和在线时长，不保证账号 ID，也不保证每款游戏、平台或服务器设置都会返回名字。返回匿名行、仅人数、拒绝或超时时明确展示对应状态，保留可确认的人数。特别是启用跨平台后，查询行为可能不同；不能把“已接入查询”写成“每服必有名单”。压缩分包暂不支持，失败时不生成空名单。

## 尚不能建立实时名单的游戏

| 游戏 | 已确认的边界 / 尚缺证据 |
| --- | --- |
| RimWorld Together | 本机 26.6.23.1 的 `list` 返回连接数和 IP；`deeplist` 枚举历史用户数据。[登录源码](https://github.com/RimWorld-Together/Rimworld-Together/blob/26.6.23.1/Source/Server/PacketManagers/PM_Login.cs)已将用户名绑定到连接，因此技术上可自行补结构化查询；但用户名绑定早于全部登录检查，必须建立认证完成状态并处理断线并发。该版本的 [CC BY-NC-ND 4.0 许可](https://github.com/RimWorld-Together/Rimworld-Together/blob/26.6.23.1/LICENSE)未授予分享修改版的权利，配套改版分发需另行解决许可；不能将其描述为只能等待上游提供技术接口。既有手工用户名管理保留。模块已适配 26.8.31.1：该版本取消原生白名单，现用服务器密码准入；历史名单与设置保留，原白名单实例必须设置密码后才能启动。新版配置依据标签源码和程序集核对，不将旧版在线探针视为新版运行验证。 |

## 证据与验收边界

2026-09-08 的隔离实服探针已确认 Astroneer build 24411584 的认证及零人 JSON、Soulmask build 25117179 的零人表头及多页控制台响应、The Forest build 3488796 和 Abiotic Factor build 24343458 的 A2S 零人响应。Barotrauma 1.13.4.0 已通过生产启动路径的 ConPTY 实服测试：连续两次请求标识不同的零人查询完整，退出码为 0。Moria build 21872765 和 Windrose build 24913903 均通过生产启动路径及实际采集器的双请求空服验证，测试进程树已回收；Windrose 还验证了停服后拒绝旧进程身份及响应。Dragonwilds build 24574222 已通过独立 UE4SS 专服代理、生产 Lua 和实际 Rust 采集器的双请求空服验证，重启产生不同运行标识，停服后旧进程及响应被拒绝。Satisfactory build 24656085、SML 3.12.0、FRM 1.5.3 已通过实际采集器的双请求空服验证：从实例原生配置读取 HTTP 端口，返回不同快照标识的完整空名单，世界就绪并正常停服。SCUM build 24973389 已通过独立只读扩展的双请求空服验证及 28 项 Lua 构造测试。**这些实服结果不覆盖真实多人、重名或中文名。** 多人 FRM 样本仍依据上游源码构造。

响应样本位于 `apps/desktop/src-tauri/test-data/live-players/`。各 `SOURCE.md` 或 `evidence.json` 记录官方文档、上游源码或本机安装包版本；多人样本是依据这些来源构造的协议测试数据，**不是实际在线玩家抓包**；实服空结果在对应来源文件中单独标识。A2S 使用本地 UDP fixture 验证挑战、分包、匿名行及异常响应。

名单刷新检查当前运行实例、进程和配置；传输受整体时间、帧数、字节数及行数限制。鉴权失败、配置缺失、响应不完整、名称未公开和未接入适配器分别呈现。失败时旧记录标为上次在线并移除可执行动作；不把失败显示成“当前无人在线”。

Source RCON、WebSocket 和 Telnet 的完整命令交换限时 8 秒，HumanitZ 为 12 秒；HTTP、UDP 及日志采集分别有独立的总预算。UI 的响应预览长度不再截断内部名单解析所需的响应。

新增或调整游戏能力时，同步修改模块契约、消费者、测试和本表。真实服务器验收仍应覆盖零人、多人、重名、中文名、服务器重启及配置变化；未执行的真实服务器场景不得写成已通过。
