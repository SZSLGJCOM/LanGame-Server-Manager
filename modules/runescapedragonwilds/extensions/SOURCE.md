# Dragonwilds 名单扩展来源与验证

`LgsmPlayerQuery` 是 LanGame 独立实现的只读 Lua 扩展。它通过当前世界的网络连接和玩家状态读取名单，不提供管理员命令、踢人、封禁或游戏规则修改，不从访问记录、存档和加入、退出日志拼接当前成员。

## 玩家控制验证

`LgsmPlayerControl` 是另行归属的 LanGame Lua/C++ 实现，复用本页固定加载器。2026-10-17 在 build `25811348` 的隔离实服中核对反射签名、游戏线程、对象所属和模块哈希，并通过游戏原生 INI importer 导入合成 EOS 账号：原生有效性与类型检查通过，空服封禁后 `KnownPlayerList` 和 INI 均为 true，重启仍为 true；原生解封后均为 false，再重启仍为 false。未使用真实账号，未验证真人踢出、封禁后重连或客户端认证。探针进程已全部回收。

原生桥只从游戏当前 `KnownPlayerList` 复制身份，调用已核对的原生管理及名单持久化路径，不修改玩家权限，不通过客户端 RPC 冒充管理员。桥的 `dispatched` 仅说明调用完成；生产 Lua 继续核对内存/INI 标志，封禁还要求账号离线。当前接口和内部布局只对固定 Shipping SHA-256 有效，升级必须重新验证，不扫描猜测新地址。手动保存独立调用游戏反射接口，见 [保存契约](../control/LgsmPlayerControl/SAVE.md)。历史构造用例不能代替真实玩家断连验收。

封禁桥的版本配置直接由完整 Shipping 哈希选择：

| Steam build | Shipping SHA-256 |
| --- | --- |
| `25811348` | `89ed65335a46db067a361d35ae4e6be80857a4c1f2453510600d632879dfbeed` |
| `25991736` | `da1761a644a9d9b19da0c2da86ad98c7c4ce8ff265f710bf1897e3b007d2dcbe` |
| `26161880` | `210adebb1d4971c3a85b5210783c7b861eea9e153bf884a460840f43e96321ba` |

build `25991736` 的配置来自两个实际 PE 的限定差分：身份工厂、封禁、解封、名单持久化的完整调用体，以及 MatchmakingSubsystem、原生会话状态构造器和 `KnownPlayerList` 访问体，在代码与数据地址重定位外保持原有参数、引用计数和字段布局。该版名单持久化中的两个 image-base 分发表也分别核对，不按统一地址增量推算。独立检查以只读、不可执行的 PE 映射核验两版全部五个入口前缀及未知哈希拒绝。此项是静态兼容证据；该版隔离合成账号的封禁、解封及重启持久化仍需实服验收，不能替代真人断连或认证测试。

同一隔离副本只启用冻结生产控制脚本和原生桥，两次不同请求取得完整空名单，`boot_id` 一致，每次只读原生 probe 均为 ready。布局修正后的生产脚本 SHA-256 为 `d39a41375193abdfd1040b202815ff70edd26e33f8950f540b08a193b520aa6e`；本次独立 MSVC 构建的桥 SHA-256 为 `7ed776ff6724389adbab6caec50d19637eaeddafd97cdec33a7476b494b14e99`。应用内嵌构建可能产生不同 PE 哈希，安装按本次构建记录校验。

桥放在 `LgsmPlayerControl/native/`，Lua 用 `package.loadlib` 显式加载。[固定 UE4SS 源码](https://github.com/UE4SS-RE/RE-UE4SS/blob/527a483b63b4dd0104fe1ca1a3934a06b87fcfb2/UE4SS/src/UE4SSProgram.cpp#L1227)仅因 `dlls` 目录存在就将其识别为另一种 C++ Mod；这种 Mod 要求独立生命周期导出。原生桥不使用这个插件 ABI，不能放入 `dlls`，即使留下空目录也会错误尝试安装。修正布局后的隔离生产启动已确认无此生命周期和安装失败日志。

### 2026-10-25：build 26161880 原生配置

本次只读检查已安装的 app `4019830`：ACF 的 `buildid` 与 `TargetBuildID` 均为 `26161880`，`StateFlags=4`、`UpdateResult=0`；Shipping 大小为 `205138592` 字节，完整 SHA-256 与上表一致。原生桥原先只认识前两版，因而在加载器允许当前版本后仍返回 `module_fingerprint`。本次按当前 PE 的真实入口和调用契约补充独立配置，没有降低哈希或入口字节校验。

| 当前 PE 契约 | RVA |
| --- | --- |
| 已知玩家身份工厂 | `0x59a4920` |
| 封禁 / 解封 | `0x5d56e90` / `0x5d547a0` |
| KnownPlayerList 更新 / SaveBans | `0x596d860` / `0x5d55440` |
| Matchmaker / Settings 类全局 | `0xbb35eb0` / `0xbb21af0` |
| Matchmaker / 原生状态虚表 | `0x9dfe2c0` / `0xa0e2cd0` |

完整调用体和原生包装调用方确认：工厂仍接受 `RCX=KnownPlayer`、`RDX=16-byte sret`，复制身份与 `FString`；封禁和解封仍分别接收原有参数，并消费一次拥有的共享引用。类注册、构造器和初始化调用方确认对象大小为 `0x730`、`0x108`、`0x3d0`，子系统 `+0x6c8` 为状态对象、`UObject.Outer +0x20` 与状态 `+8` 指向同一 GameInstance。`GetWorld` 仍通过 `+0x30` 和 `+0x2c0` 取世界，反射属性描述中的世界所属 GameInstance 偏移为 `0x228`。状态会话前置检查仍使用 `+0x388`、`+0x398`。

`KnownPlayerList` 仍位于 Settings `+0xd0`，条目步长为 `88`，封禁标志为条目 `+0x41`。SaveBans 从状态 `+0x298` 的名单生成更新，再由原生路径在标志变化时调用 `SaveConfig`；封禁的离线分支仍需桥显式调用 SaveBans。两个名单 variant 分发表分别位于 `0x92fc038`（共享身份复制）和 `0x9028430`（释放），均按当前指令和目标函数核对。此次未取得旧 Shipping 映像，没有把历史差分结论当作本次逐字节比较结果，也未扩展 build `26087591` 的原生桥范围。

回归入口为 [`control/tests`](../control/tests/CMakeLists.txt)，构建目标 `native_profile_tests`，唯一运行参数为当前 Shipping 程序的绝对路径。测试直接编译生产桥的准入和字节检查，以 `SEC_IMAGE_NO_EXECUTE` 只读映射当前文件，核对实际调用目标、类与虚表绑定、布局锚点，以及未知哈希和入口字节变更的拒绝。原代码先失败于 `build_26161880_rejected_by_native_bridge`，加入本配置后通过。MSVC `19.51.36257.0`、Windows SDK `10.0.26100.0` 的 x64 Release 测试与实际 DLL 构建均退出 `0`，保留 `/W4 /WX /guard:cf`。

这是当前包的静态适配与构建证据。本次未执行游戏代码、启动服务器或写入真实安装包及实例；build `26161880` 的封禁、解封、重启持久化和真实玩家断连仍待隔离实服验收。

## 加载器来源

Steam build `26161880` 的 Shipping SHA-256 为 `210adebb1d4971c3a85b5210783c7b861eea9e153bf884a460840f43e96321ba`。与 build `26087591` 的 `ecaa5ebc6e0b08108856703a4aceed7b5cf0ea04d708b82dfc66ef74709a4269` 相比，两份 Jagex 签名映像的节布局、导入和导出一致，可执行代码仅有返回构建号的常量从 `252835` 改为 `254097`，其余差异限于构建与签名元数据。加载器仍按完整映像哈希准入，不在运行时忽略这些字段或推断未知版本兼容。静态差分不替代当前名单、原生保存完成及重启读回验收，也不扩展上述原生封禁桥的版本范围。

UE4SS 核心固定为 `3.0.1-1125-g527a483b`，源码 commit 为 `527a483b63b4dd0104fe1ca1a3934a06b87fcfb2`。下载官方固定资产 [UE4SS_v3.0.1-1125-g527a483b.zip](https://github.com/UE4SS-RE/RE-UE4SS/releases/download/experimental/UE4SS_v3.0.1-1125-g527a483b.zip)后必须核对下表中的 SHA-256；下载链接本身不能替代哈希核对。该 ZIP 的 SHA-256 已与 [GitHub Release API 资产 546250363](https://api.github.com/repos/UE4SS-RE/RE-UE4SS/releases/assets/546250363) 的 `digest` 核对，大小为 8,718,500 字节：

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

UE4SS、代理生成器及所用第一方依赖采用 [MIT 许可证](https://github.com/UE4SS-RE/RE-UE4SS/blob/527a483b63b4dd0104fe1ca1a3934a06b87fcfb2/LICENSE)。`fmt 11.2.0` 采用其 [MIT 许可证](https://github.com/fmtlib/fmt/blob/11.2.0/LICENSE)。构建输出附带两份许可证；复用或分发时保留。Windows 构建内嵌上表中的完整官方归档、固定源码构建的专服代理及许可证。`scripts/build_ue4ss_runtime.ps1` 仅从固定官方 URL 下载，或读取 `LANGAME_UE4SS_ARCHIVE` 指定的本地归档；均核对长度和哈希，不执行下载内容。实例启动从内嵌字节准备加载器，将核心及代理的许可证一并保留在 `ue4ss/`，无需启动时下载。仓库不保存这些上游源文件、Windows 系统 DLL 或生成的代理 DLL。

RSDWTools/Dev Kit 仅用于发现可观察的游戏 API 线索。本实现不复制其模块、界面、配置或派生脚本，也不将其许可解释为可重新分发整个工具包。

## 已确认的实服事实

### 2026-10-16：自动下载、首次安装及原生重启

当时的自动准备沿用生产 Mod 下载器，历史联网探针成功取得固定官方资产，并通过准确大小、发行包 SHA-256 及三个必要成员的独立哈希校验。

`isolated_dragonwilds_automatic_loader_install_survives_native_restart` 在不带加载器的标记独立副本中调用生产安装器，随后经生产 `spawn_launch_plan` 连续启动两次。两次运行分别取得两个不同请求标识对应的完整空名单，停止后拒绝旧进程；原生生成日志后，安装检查仍保持就绪。测试通过 1 项、失败 0 项，用时 17.21 秒。本次内嵌代理为 448,512 字节，SHA-256 `3244536c75b12db2cb6ba75f62bdd450a677763fdbc01ddf57a6e7cf9bb8fa4e`，仍来自同一固定源码清单。首次启动后游戏从保存的 INI 移除了 `AdminPassword`，重启探针沿用原生保存状态，不重复要求首次夹具的明文字段。

构造回归另覆盖已有加载器保留、重复启动、多个安装中断位置、文件及恢复记录篡改、应用重构建、未知游戏版本停用与恢复。原生测试使用回环地址、独立口令和可丢弃副本，未操作用户服务器或实际玩家。上述结果验证下载、安装及空服读取，不代表桌面完整启动队列、多人加入退出、账号认证、备份或存档持久化已验收。

### 2026-10-15：1.0.1 空服名单复验

隔离的 Steam 专服 build `25811348`，Windows depot `4019831`、manifest `201953044557705869`，Shipping 可执行文件 SHA-256 为 `89ed65335a46db067a361d35ae4e6be80857a4c1f2453510600d632879dfbeed`，UE 版本为 `5.6.1-248854`。沿用上表中的官方 `1125` 核心，按本目录脚本重建代理，未升级加载器或修改生产 Lua：

- 复验代理 SHA-256 为 `cdd3cf56d2af50dc35deef48e0fef4e3d56e922e0262d209d86d368c86351d6f`，大小 448,512 字节；32 个固定源码均通过 SHA-256 与 Git blob 校验，构建退出码为 0。17 个导出名称及序号与本机已验签的 `version.dll` 相符，依赖仍仅为 `KERNEL32.dll`、`USER32.dll` 和 `SHELL32.dll`。
- 生产 `LgsmPlayerQuery/Scripts/main.lua` 的 SHA-256 为 `0bda4f9149c9ff8a6c2f3c8854165bea1f77c0c0bfab8fb65f7987f71485d3e4`，与仓库原脚本逐字节一致。
- 原生测试 `isolated_dragonwilds_bootstrap_reads_two_current_empty_snapshots_and_rejects_stopped_process` 通过生产 `spawn_launch_plan` 启动 `RSDragonwildsServer.exe` 并确认进程归属；两个不同请求标识分别取得 `complete=true`、`current_players=0`、`players=[]` 的当前响应，均通过 Rust 严格解析。停止受控进程树后，读取器拒绝旧 PID。
- 该复验通过 1 项、失败 0 项，测试进程退出码 0、用时 8.08 秒；收尾核对所属进程残留为 0，临时盘符映射已移除。

2026-10-15 的复验证据限于新版加载器和当前空名单通道，不代表桌面实例启停、自动保存、备份、全部设置或任一客户端已验收。没有进行真实多人加入、退出、重连、账号认证或服主权限测试。下面的 28 个 Lua 构造用例和重启观测属于 2026-09-08 的历史结果，2026-10-15 未重跑。机器记录以 `evidence.json` 的 `revalidations` 数组保存，该次复验未记录具体请求标识或 `boot_id`。

### 2026-09-08：历史验证

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
