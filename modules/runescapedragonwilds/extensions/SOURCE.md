# Dragonwilds 名单扩展来源与验证

`LgsmPlayerQuery` 是 LanGame 独立实现的只读 Lua 扩展。它通过当前世界的网络连接和玩家状态读取名单，不提供管理员命令、踢人、封禁或游戏规则修改，不从访问记录、存档和加入、退出日志拼接当前成员。

## 加载器来源

UE4SS 核心固定为 `3.0.1-1125-g527a483b`，源码 commit 为 `527a483b63b4dd0104fe1ca1a3934a06b87fcfb2`。固定资产来自官方发布页，其 SHA-256 已与 GitHub Release API `digest` 核对：

| 文件 | SHA-256 |
| --- | --- |
| `UE4SS_v3.0.1-1125-g527a483b.zip` | `4f9762f812329a640c8cfa14444c2bb97ecc213b8320bd4c5433383c3eef48f7` |
| `ue4ss/UE4SS.dll` | `91d5444f41d19d0bb502f0fbdc15d76421e20359e898caa39e41e134c8f87958` |

[`experimental-latest`](https://github.com/UE4SS-RE/RE-UE4SS/releases/tag/experimental-latest) 是可移动 tag，不能作为固定版本标识；本目录使用上表中的版本和哈希，不自动升级。固定源码的 [README](https://github.com/UE4SS-RE/RE-UE4SS/blob/527a483b63b4dd0104fe1ca1a3934a06b87fcfb2/README.md)覆盖 UE 4.7–5.8，同时明确各游戏仍需验证；这不能单独证明某个专服可用。

## 专服代理

Dragonwilds 专服使用 `version.dll` 加载代理。[作者发布页的 2026-06-24 记录](https://www.nexusmods.com/runescapedragonwilds/mods/4?tab=posts)指出，专服更新后不再加载默认 `dwmapi.dll`，替代代理可以加载 UE4SS；上传者随后提供了可选文件。本目录没有下载、使用或分发该 Nexus 二进制。

本目录使用官方固定源码的 [proxy_generator](https://github.com/UE4SS-RE/RE-UE4SS/blob/527a483b63b4dd0104fe1ca1a3934a06b87fcfb2/UE4SS/proxy_generator/main.cpp) 和 [代理 CMake 目标](https://github.com/UE4SS-RE/RE-UE4SS/blob/527a483b63b4dd0104fe1ca1a3934a06b87fcfb2/UE4SS/proxy_generator/proxy/CMakeLists.txt)。生成器读取本机已验证签名的 `System32/version.dll` 导出表，生成 C++、汇编和 DEF 文件，再编译成独立代理。代理从 Windows 系统目录加载原 DLL 并转发 API，随后加载同目录下的 `ue4ss/UE4SS.dll`。

[build-proxy.ps1](build-proxy.ps1) 和 [proxy-build/CMakeLists.txt](proxy-build/CMakeLists.txt) 仅提供最小构建入口。32 个必要源文件及固定下载地址在 [proxy-sources.json](proxy-sources.json)；下载后验证 SHA-256 和 Git blob，不修改上游源码，也不编译 UE4SS 核心。依赖为 UE4SS 的 Constructs、File、Helpers、String，以及上游固定的 `fmt 11.2.0`。构建没有 Cargo 步骤，不安装系统工具或更改全局设置。

已验证代理的 17 个导出名称与序号均与 Windows 原 DLL 一致，包括两个系统转发导出。代理使用静态 C++ 运行库，只导入 `KERNEL32.dll`、`USER32.dll` 和 `SHELL32.dll`。不同构建可能因 PE 时间戳或工具链而产生不同哈希；部署应记录实际生成文件及本次构建清单，而不是要求所有机器生成相同二进制。

## 许可边界

UE4SS、代理生成器及所用第一方依赖采用 [MIT 许可证](https://github.com/UE4SS-RE/RE-UE4SS/blob/527a483b63b4dd0104fe1ca1a3934a06b87fcfb2/LICENSE)。`fmt 11.2.0` 采用其 [MIT 许可证](https://github.com/fmtlib/fmt/blob/11.2.0/LICENSE)。构建输出附带两份许可证；复用或分发时保留。仓库不保存这些上游源文件、Windows 系统 DLL 或生成的代理 DLL。

RSDWTools/Dev Kit 仅用于发现可观察的游戏 API 线索。本实现不复制其模块、界面、配置或派生脚本，也不将其许可解释为可重新分发整个工具包。

## 已确认的实服事实

2026-09-08，在隔离的 Steam 专服 build `24574222`、UE `5.6.1` 上验证：

- 官方 `1125` 核心经自建 `version.dll` 成功加载，日志确认 commit `527a483b`。
- Lua 在游戏线程运行，能读取正式地图 `L_World` 的 `GameNetDriver`、`GameState` 和玩家数组。
- 当前网络驱动为 `RedpointEOSNetDriver`；驱动回指当前世界，`ServerConnection` 无效，空服 `ClientConnections` 与 `PlayerArray` 均为 0。
- 本目录构建脚本生成的代理加载成功，生产 Lua 返回与当前请求匹配的 `complete=true`、`current_players=0`、`players=[]`。
- Lua 构造测试在隔离 UE4SS VM 中通过，覆盖连接集合、就绪条件、字段校验及失败边界。
- 原生探针通过正式 `spawn_launch_plan` 启动 `RSDragonwildsServer.exe`，验证当前进程身份后，连续以两个不同请求标识经文件 IPC 和 Rust 严格解析器取得完整空名单；停止受控进程树后，读取器拒绝旧 PID。
- 重启后 `boot_id` 与先前录制的 fixture 不同，新响应包含新的请求标识和时间戳。

生产采集只接受正式 `L_World`、`DominionGameStateBase` 与 `DominionGameMode`，要求当前唯一的 `RedpointEOSNetDriver/GameNetDriver` 和世界回指一致。连接、控制器和完整 `PlayerArray` 必须双向对应；世界切换、无效关联或不完整枚举不能生成空名单。

`DominionPlayerController:IsPlayerReady` 的实服反射标志为 `0x54020401`，只有一个 `BoolProperty` 类型的返回值。[Epic 的 EFunctionFlags 定义](https://dev.epicgames.com/documentation/en-us/unreal-engine/API/Runtime/CoreUObject/EFunctionFlags)对应 Native、Public、BlueprintCallable、BlueprintPure、Const 和 Final。生产脚本在游戏线程核对函数签名后调用这个只读 getter，只接受 `true`。它表达游戏就绪，不能解释为独立证明 EOS 认证通过。

名称取自当前 `PlayerState.PlayerNamePrivate`，`PlayerState.PlayerId` 仅作为数字会话标识，不当作稳定账号；玩家行只读。`DisplayNameComponent` 的姓名注册数组不作为在线证据。

这些事实证明专服启动、生产采集、原生读取、重启和停服边界已贯通。真实多人连接、加入、退出和重连尚未验收。完整机器可读证据见 [evidence.json](evidence.json)。
