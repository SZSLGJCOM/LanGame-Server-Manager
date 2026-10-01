# SCUM 名单扩展来源与验证

本目录 Lua 由 LanGame 独立编写，遵循仓库 [LanGame Source-Available License 1.0](../../../LICENSE)，仅许可非商业使用和私下修改；商业用途须另获授权，不能自行发布修改版。没有采用 SCUM-RCON 的闭源插件、许可服务或二进制。

## 运行组合

2026-09-08 在独立副本中验证 Steam app `3792580`、build `24973389`、Unreal Engine `4.27.2`（SCUM release `1.3.3`，CL `145413`）。静态内容复用已安装游戏，配置和世界为隔离数据，没有复制原服存档，也没有真实客户端加入。

加载器为官方 UE4SS `3.0.1-1125-g527a483b`，SCUM 可直接使用其中的 `dwmapi.dll`。来源为 [UE4SS 官方发布页](https://github.com/UE4SS-RE/RE-UE4SS/releases/tag/experimental)，源码为 [527a483b](https://github.com/UE4SS-RE/RE-UE4SS/tree/527a483b63b4dd0104fe1ca1a3934a06b87fcfb2)，采用 [MIT 许可证](https://github.com/UE4SS-RE/RE-UE4SS/blob/527a483b63b4dd0104fe1ca1a3934a06b87fcfb2/LICENSE)。二进制不进入仓库。

| 文件 | SHA-256 |
| --- | --- |
| `UE4SS_v3.0.1-1125-g527a483b.zip` | `4f9762f812329a640c8cfa14444c2bb97ecc213b8320bd4c5433383c3eef48f7` |
| `ue4ss/UE4SS.dll` | `91d5444f41d19d0bb502f0fbdc15d76421e20359e898caa39e41e134c8f87958` |

## 采集边界

真实反射确认 `IpNetDriver` 的 `GameNetDriver` 绑定正式 `The_Island` 世界、`ConZGameState` 与 `ConZGameMode`。过渡地图使用引擎默认状态，必须拒绝。当前构建的 `bReplicatedHasBegunPlay` 在正常空服中仍为 false，不能作为 SCUM 就绪条件。

生产脚本强制在 `EngineTick` 游戏线程中读取 `ClientConnections` 和 `PlayerArray`，完整核对连接、控制器、玩家状态的双向关系，要求有效 `_userProfile`。该字段来自真实反射；其含义不等同于独立验证 Steam 认证。玩家名称取 `PlayerNamePrivate`，数字 `PlayerId` 只作本次运行的会话标识。

响应必须完整且对应当前请求。宿主校验登记进程的 PID、创建时间、可执行文件布局、请求标识、响应时效、数量和唯一会话 ID。文件大小、名单数量、请求时间及异步队列均有上限；未就绪、迟到回调或失效进程不得发布旧名单。游戏线程只读取对象，文件 I/O 在异步轮询中完成。

## 验证入口

```text
lua modules/scum/extensions/LgsmPlayerQuery/tests.lua modules/scum/extensions/LgsmPlayerQuery/Scripts/main.lua
```

Lua 构造测试覆盖完整空服、双玩家、Unicode、重复身份、关联失效、过渡世界、未完成加载、时限和迟到回调。真实服务器已连续返回两个不同请求的完整零人快照。多人分支属于构造测试，不是实服玩家样本。

既有实例也已通过 LanGame 生产启动链路验收：原生世界就绪，连续三个不同请求返回完整、未过期的零人名单，随后通过管理器停止。提权启动登记按配置中的完整可执行文件路径匹配 `SCUMServer.exe`，排除先出现的控制台进程和加载器创建的崩溃报告助手；超过等待期限时清理启动进程并报告失败。

Rust 原生启动探针为 `isolated_scum_reads_current_empty_snapshots_and_rejects_stopped_process`，仅对通过 `LGSM_SCUM_PROBE_ROOT` 指定、带隔离标记及私有密码的专服副本运行。探针使用生产启动和读取路径，检查两次完整响应及停服后拒绝旧进程；默认测试不启动真实游戏。
