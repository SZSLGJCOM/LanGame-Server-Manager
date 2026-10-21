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

## 实体持久化（2026-10-19）

本地 Steam app `3792580`、build `25247308` 的 `SCUMServer.exe` SHA-256 为 `4ca11449c4c8d3a243c797c719f66e33192eefad53a9c64cca9aac925c9e39ad`。独立的 [实体持久化控制](../control/README.md) 复用经过固定版本验证的 UE4SS；玩家查询脚本仍只读，不调用保存、管理或任意命令。

本次只读 PE 元数据、反汇编与独立空服证据确认：

- `/Script/SCUM.ConZPlayerController.SaveState` 的 native thunk RVA `0x26f8f50` 跳转至 `0x1a61780`，保存控制器对应 profile 与个人字段，不能当作全世界接口。
- `UEntitySystem::AutoSave` 位于 RVA `0x1b94040`，ABI 为 `void(this, bool force)`。正常 tick 的调用点 `0x1bcda4b` 传 false；退出路径 `0x1ba9ac1` 传 true 后再单独设置退出标志。true 分支对全部已注册实体执行持久化检查，取消正常 tick 的时间预算；函数自身不关服。
- RVA `0x22ff660` 的 `DbSaveGlobalData` 实际属于 RadiationManager，SQL 更新 `global_radiation_data`。不能因日志名字而把它当作全部世界的数据库保存。
- 真实专服中当前唯一 EntitySystem 的 outer 是当前 ConZGameInstance，原生 GetWorld 与 GameNetDriver 绑定正式 The_Island 世界。EntitySystem 有 authority、loading、save busy、shutdown 状态；控制桥逐项拒绝不满足条件的实例。
- 专服的 `GameState.bReplicatedHasBegunPlay` 仍为 false。保存控制检查固定构建原生 `AActor::ActorHasBegunPlay` 状态：当前 GameMode 与 GameState 的 `+0x5c` 高两位必须为 `0x80`。原生 `AActor::EndPlay` RVA `0x3ec3aa0` 检查此值并在拆除前立即清除。

这只提供“持久化实体”，其他子系统继续原生自动持久化；既不是完整世界快照，也不是备份。加载器、可执行文件、签名或所属世界变化即拒绝，不将任意新构建地址当作兼容 API。原生 void 返回只代表调用已返回，界面显示保存请求已发送，不声称写盘完成。构建、反例测试和真实调用范围见控制目录说明。

原生程序清单请求的是 `highestAvailable`，不是 `requireAdministrator`。该固定构建在独立世界以 `RunAsInvoker` 子进程环境成功启动，进程和启动 shell 的 TokenElevation 均为 0；世界运行无需新增特权。LanGame 对 SCUM 使用相同的进程环境，保留操作系统文件和进程权限，不设置全局环境、注册表或防火墙。
