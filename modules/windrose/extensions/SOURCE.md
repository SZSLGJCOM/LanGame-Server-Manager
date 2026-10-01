# Windrose 玩家读取桥证据

`LgsmPlayerQuery` 是 LanGame 的只读 Lua 扩展。它不安装 Windrose+，不生成 PAK，不调整游戏规则，不提供踢人、封禁或任意命令入口。UE4SS 是独立安装前置，不随此目录分发二进制。

## 已核实的运行组合

2026-09-08 在独立服务器副本和可重建世界中验证：

- Steam app `4129620`，build `24913903`，游戏版本 `0.10.0.9.32-22d39a16`。
- 进程映像：`R5/Binaries/Win64/WindroseServer-Win64-Shipping.exe`。
- UE4SS 官方资产 `UE4SS_v3.0.1-1125-g527a483b.zip`；下载 SHA-256 与 GitHub Release API 的 `digest` 一致，见 `evidence.json`。
- 普通权限启动成功；未提权，未运行 Windrose+ 安装、Dashboard 或 PAK 脚本。
- `LgsmPlayerQuery/enabled.txt` 独立加载成功，无须修改已有 `mods.txt`。
- 本目录精简 `UE4SS-settings.ini` 已实服重启验证。`EngineTick` 持续回调；`ProcessEvent` 在空服启动后停止触发，不能作为轮询请求的调度方案。

官方加载器来源：[UE4SS Release](https://github.com/UE4SS-RE/RE-UE4SS/releases/tag/experimental)中的[固定版本资产](https://github.com/UE4SS-RE/RE-UE4SS/releases/download/experimental/UE4SS_v3.0.1-1125-g527a483b.zip)。安装须同时核对本证据记录的资产名和哈希。UE4SS 采用 [MIT 许可证](https://github.com/UE4SS-RE/RE-UE4SS/blob/527a483b/LICENSE)，分发或安装时保留原资产的许可证。

## 采集依据

实服在游戏线程中确认存在唯一 `R5NetDriver`，`NetDriverName = GameNetDriver`，`ServerConnection` 无效，且 `driver.World.NetDriver` 反向指向自身。该世界存在 `R5GameState`、`R5GameMode`，`bReplicatedHasBegunPlay = true`。空服 `ClientConnections` 与 `GameState.PlayerArray` 均为长度 0 的真实 `TArray`。

生产源只枚举该 `ClientConnections`，再验证连接、控制器和状态对象之间的双向关系，并与 `PlayerArray` 完整交叉核对。不完整认证、世界切换、无效身份、重复 ID、机器人、已失效状态、分屏子连接或反射异常均返回 `complete = false`。名称只取 `PlayerState.PlayerNamePrivate`；会话 ID 只取 `PlayerState.PlayerId`，不解释为 Steam ID。

连接、控制器和状态属性在当前专服的真实反射元数据中存在；**真实多人加入、离开和重连尚未验收**。这些分支已使用明确标注的 Lua mock 测试，不能把测试对象当作实服玩家样本。

`GameSession.MaxPlayers` 在探针中是引擎默认值 1000，实际配置为 4，故响应不返回该字段，由宿主保留其已知容量。

## 请求与生命周期

由宿主创建游戏根目录中的 `langame_player_query`。请求写入 `request.json`：

```json
{"request_id":"0123456789abcdef0123456789abcdef","requested_at":1788831512}
```

响应 `response.json` 包含 `protocol`、`request_id`、`boot_id`、`timestamp`、`complete`、固定 `source = net_driver_client_connections`、`current_players` 和 `players`。失败响应还包含固定错误代码，名单为空。每条玩家只有 `name` 和数字字符串 `session_id`。

Lua 每 250 ms 读取一次受限请求，只接受 10 秒内的 32 位十六进制请求 ID；始终最多保留一个待执行游戏线程闭包。游戏线程仅读取 UObject 并生成纯 Lua 数据，异步回调负责文件读写。响应先写临时文件再替换；宿主须容忍替换瞬间缺失，只接受当前请求 ID，结合登记进程的 PID 与创建时间判断来源。

加载期间可能尚无响应。重启后旧文件仍存在，但新的 `boot_id` 和新请求 ID 可区分本次运行。停止后响应文件保留且不再更新，宿主必须优先检查进程状态，不能把磁盘上最后一帧当在线名单。真实空服跨重启请求已验证；延迟回调、过期请求、队列上限和失败后恢复另有 mock 覆盖。

固定 UE4SS 源码的 [LuaRaw 锁配置](https://github.com/UE4SS-RE/RE-UE4SS/blob/527a483b/deps/first/LuaRaw/include/luaconf.h)及 [Windows 临界区实现](https://github.com/UE4SS-RE/RE-UE4SS/blob/527a483b/deps/first/LuaRaw/src/luauser.c)串行化 Lua VM 执行；C 调用可能释放锁，因此生产代码在反射结束后二次检查请求代次，文件刷新捕获本次响应，避免清掉较新的结果。

## 上游方案与排除依据

[Windrose+ 固定查询源码](https://github.com/humangenome/WindrosePlus/blob/757d935068a0fc8ac8678b534ddb3ac4244902af/WindrosePlus/Scripts/modules/query.lua)用于发现读取入口，没有复制进本扩展。它有角色回退、占位名称和降级缓存；[主入口](https://github.com/humangenome/WindrosePlus/blob/757d935068a0fc8ac8678b534ddb3ac4244902af/WindrosePlus/Scripts/main.lua)仅用 Pawn/ping 判断连接，缺少直接连接证明。其默认禁用游戏线程调度后还会直接异步访问 UObject。因此本适配器不读取该 `server_status.json`。

对不适用的 FProperty 调用 `GetPropertyClass` 会导致 native access violation，已由隔离元数据诊断确认；Lua `pcall` 无法捕获该类错误。生产源不做属性泛型强转或未知函数探测，只读已验证类型上的属性，并强制 `IsInGameThread()`。

## 验证入口

`LgsmPlayerQuery/tests.lua` 为纯 Lua 5.4+ 行为测试，运行命令：

```text
lua modules/windrose/extensions/LgsmPlayerQuery/tests.lua modules/windrose/extensions/LgsmPlayerQuery/Scripts/main.lua
```

Lua 构造测试已在隔离 UE4SS Lua VM 中通过，覆盖双玩家、Unicode/引号转义、完整性、重复身份、断开关联、未认证连接、超时后的迟到回调、队列上限及后续请求恢复。

桌面 Rust 实服测试 `isolated_windrose_bootstrap_reads_two_current_empty_snapshots_and_rejects_stopped_process` 已通过：使用生产 `spawn_launch_plan` 启动 `WindroseServer.exe`，从登记的启动进程身份调用实际文件交换与解析器，连续取得两次请求标识匹配的完整空名单；停止自有进程树后，旧进程身份被拒绝。该测试不覆盖真实多人。

已有加载器的配置不能直接用本目录示例覆盖。应先核对其版本、必要调度配置和现有模组要求；不支持的加载器或游戏版本须返回扩展不可用。在已验证的游戏构建中，原生 UDP 即使配置回环地址仍监听所有接口，TCP 监听回环地址。隔离探针使用独立随机密码，不公开邀请码，并对所有游戏进程设置明确的所有者和有界清理。
