# SCUM 原生保存请求

维护页和卡片在游戏线程调用一次 `UEntitySystem::AutoSave(this, true)`。该原生入口返回 `void`，因此界面报告“保存请求已发送”，不声称数据库异步任务或写盘已经完成。原生调用异常、身份变化、请求过期和缺失回执仍作为失败或未知结果处理，不自动重发。保存不会停服或制作备份。

## 绑定版本与调用范围

原生桥为 Steam app `3792580` 的以下四个构建提供固定配置，按 `SCUM/Binaries/Win64/SCUMServer.exe` 完整 SHA-256 选择：

| Build | SHA-256 |
| --- | --- |
| 25247308 | `4ca11449c4c8d3a243c797c719f66e33192eefad53a9c64cca9aac925c9e39ad` |
| 25803875 | `070fcad918ba04eb9797db29674cadbd6691427b936fe354bca23b008c8c9971` |
| 26320224 | `a862d658271f26d082de5df7bba01e8af9c3d051cb9634e908bb94d8a9efbc7a` |
| 26593814 | `805396516e64d17f1713f24ce97a7d525a6d761db46ce069f40cf63114add6d9` |

加载器为官方 UE4SS `3.0.1-1125-g527a483b`，核心 SHA-256 为 `91d5444f41d19d0bb502f0fbdc15d76421e20359e898caa39e41e134c8f87958`，配置见 `extensions/UE4SS-settings.ini`。其他构建需要重新核对 ABI、对象布局及真实调用，不能只更新哈希。

`AutoSave` RVA `0x1b94040` 的 ABI 为 `void(this, bool force)`。正常 tick 传 `false`；本入口传 `true`，覆盖全局持久化状态（`0x1ba7060`）、已登记坐标变更（`0x1b94ee0`）和已登记实体的强制保存（`0x1b94250`）。它不调用退出函数，也不承诺覆盖独立子系统、尚未到达服务端的数据或整个进程内存。

上述地址来自 build 25247308。在 25803875 中，完整 528 字节 AutoSave 入口、三个保存阶段、EntitySystem 构造与 Actor::EndPlay 指令除重定位引用外一致，对象字段偏移未变。新入口为 `0x1bb2680`，三个阶段为 `0x1bc59c0`、`0x1bb3520`、`0x1bb2890`；类 singleton、vtable 与 GetWorld 分别为 `0x7500180`、`0x5edfe20`、`0x1552cc0`。构建测试分别执行两个配置的准入检查；这不替代各版本的真实保存验收。

build 26320224（`1.3.5.2.172724+172352`）的入口为 `0x1bc0530`，三个阶段为 `0x1bd4330`、`0x1bc13d0`、`0x1bc0740`；类 singleton、vtable 与 GetWorld 为 `0x753f010`、`0x5f13ba0`、`0x1554920`。以 x64 unwind 链界定的 AutoSave 代码为 515 字节（先前 528 字节范围还含相邻入口前的填充）。入口、全局/坐标阶段、类构造/单例和 Actor BeginPlay 状态函数完整匹配，保留字段偏移、常量和内部跳转，仅归一化外部调用及 RIP 重定位。

build 26320224 的实体保存阶段确有变化：2523 字节变为 2524 字节，两处 `+0x90` 虚调用经 `0x186c250` 的尾转发辅助函数完成，并额外传入 force 参数地址。桥只调用已核对的 `void(EntitySystem*, bool)` 入口；这些内部变化不能称为整个保存阶段字节等价。只读原生世界已核对该版类 singleton、vtable、GetWorld、入口字节和对象就绪字段，详情见[该版原生契约](../reference-fixtures/2026-10-30-native_contract_build_26320224.json)。

build 26593814（`1.3.6.0.177121+0`）的独立静态分析定位到 `AutoSave` RVA `0x1bcc000`、三个阶段 `0x1bdf360` / `0x1bccea0` / `0x1bcc210`，以及类 singleton `0x75c3e90`、vtable `0x5f87490`、GetWorld `0x1555d10`。其加载、正在保存和退出状态分别移到 `+0x7a8`、`+0x7aa`、`+0x7ab`；旧三版仍用 `+0x798`、`+0x79a`、`+0x79b`，由同一完整 EXE 哈希选择对应字段。新 `CreateEntity` 的原生加载错误分支、AutoSave 的状态设置/清除和 Shutdown 的设置点分别提供字段语义证据。只增加 EXE 哈希和函数地址会遗漏这项布局变化。

该版入口仍保留并传递 `bool force`，实体阶段为 2523 字节，重新直接调用两处 `vtable+0x90`。旧完整 EXE 未保留，官方历史 depot 的匿名读取返回 401，故本次没有宣称完整新旧函数等价，也没有外推内部实体虚函数等价。新包函数边界、完整指令摘要、字段定位和 schema 77 原生世界证据见[2026-11-12 原生契约](../reference-fixtures/2026-11-12-native_contract_build_26593814.json)；运行时保存与区域注册表验证范围在同一证据中分别记录。

执行前核对完整游戏与加载器哈希、函数字节、原生类/vptr、唯一 EntitySystem、当前实例 outer、原生 GetWorld、UWorld.OwningGameInstance、GameNetDriver 和当前 GameMode authority。当前 GameMode/GameState 的原生 ActorHasBegunPlay 状态须为 `+0x5c & 0xc0 == 0x80`，EntitySystem 不能处于加载、保存或退出状态。对象检查及调用都在 EngineTick 上执行；原生桥再次核验游戏线程。

宿主绑定 PID、进程创建时间、启动 token、boot/world token 与请求身份。原生桥在调用前持久化未知结果，随后仅记录正常返回；同一 nonce 不会再次调用。Lua 仅接受当前进程、当前请求的返回状态，不接受伪造的完成状态。此路径不安装 SQL、文件或任务队列钩子，也不把其他后台事务当作本次请求的完成证据。

## 安装与升级

启动实例时，LanGame 在创建游戏进程前准备实例私有程序目录、固定官方 UE4SS、入口代理、Lua 和 `native/LgsmScumSaveControl.dll`。兼容的已有加载器可复用，其他配置字段及模组保持不变。固定哈希、普通文件与目录归属校验不通过时保留现有文件并报告冲突。

此前受管实例中的 `dlls/main.dll` 提前观察器只在内容与本实例 ownership 记录一致时删除。删除中断可恢复；未知或修改过的 DLL 会保留并阻止覆盖。运行中的实例需正常重启后加载新策略。正式入口为 `SCUMServer.exe`。其子进程使用 `__COMPAT_LAYER=RunAsInvoker` 继承管理器当前 Windows token，不改变系统权限配置。

## 验证

```powershell
.\modules\scum\control\Build.ps1 -OutputDirectory <仓库和原服之外的输出目录> -RunComponents
```

MSVC `/W4 /WX /MT` 构建生产 DLL，并运行对象归属、authority、生命周期、加载/忙碌/退出、消息格式和单次 `force=true` ABI 检查。`tests/policy_fixture.lua` 执行生产 Lua 与共享协议，覆盖请求返回、明确失败、未知结果、伪造确认、启动身份、世界变化、排队/执行期限、迟到结果和不重发。其文件、DLL 与 UObject 边界使用构造环境，不能代替真实开服验收。

2026-10-30 的组件检查覆盖三个 profile。新版只读 native probe 经 owned Windows Job 回收，退出码 113；它未调用保存，不能作为正常停服或实际保存证据。区域编辑另以两版真实数据库副本通过公共 API、精确 EXE/schema 配对和备份读回；新版数据库为 schema 75，旧版 25803875 为 schema 73。

2026-11-12 的 MSVC 组件检查已覆盖四个 profile，同时验证当前布局三个状态字段逐项拒绝，以及另一版本布局中的非零字节不会误阻止有效请求。独立 vanilla 副本生成 schema 77 并达到原生就绪；直接 Ctrl+C 得到 `0xC000013A`，不作为正常 exit 0 或正式管理后端停止证据。

同日只读运行观察逐项核对新版类 singleton、vtable、GetWorld、入口字节、实例与世界归属和就绪状态，11 项检查通过。新版三个状态字节为 0，authority 为 3，GameMode/GameState 的 Actor 状态均为 `0x89`。原生区域注册表和 schema 77 公共数据库读写也分别通过，边界见版本契约。该观察没有发送保存请求；Ctrl+C 后仍有 owned 子进程，运行器按失败处理并通过 Job 回收，不能把读取通过写成整次探针正常退出。

同日的[正式管理后端生命周期验收](../reference-fixtures/2026-11-12-managed_lifecycle_build_26593814.json)对 build 26593814 完成初次启动、重启、备份恢复后启动及归档恢复后启动。四轮各发送一次 `AutoSave(true)`，均收到 `ready/invoked/call_returned=true`、`write_confirmed=false`，回执状态保持 `Requested`。四次正式停止均为原生 exit 0、`crash_flag=false`，active run 清除；59 个世界文件的备份恢复、归档提交中断恢复、保留 81 个保存文件的程序库卸载和最终清理通过。

该运行使用冻结的 native-build-04，显式配置 `max_ping=400`；其内嵌 inventory 仍为旧默认 200，不能据此宣称后续默认值修正已被该二进制覆盖。[默认值证据](../reference-fixtures/2026-11-12-generated_defaults_build_26593814.json)单独记录最终源码构建：更新后的完整 inventory/schema 字节已进入 storage 测试产物，18 项 SCUM 回归通过；这些既有测试没有直接断言 MaxPing 的数值。没有把停服后的备份恢复等同于某次手动保存的完整因果落盘，也未验证客户端战斗或每个子系统的持久性。

固定版本曾在全新私有空服世界中执行一次真实 `AutoSave(true)`：原生返回 `returned`，生产协议的 `ready/invoked/call_returned=true`、`write_confirmed=false`，服务器继续运行。正常重启后同世界从数据库加载实体，数据库 `quick_check=ok`。此证据表明原生入口可以调用且世界可以重新加载，不证明每个子系统的因果落盘或真实多人行为；每次版本适配仍需重新验证。

2026-10-30 对 build 26320224 的独立普通权限副本执行了唯一一次真实保存：生产 Lua、共享协议和编译桥保持原样，`saveprobe` 就绪后原生返回 `returned`，生产回执仍为 `write_confirmed=false`。请求后出现原生 Auto-save 日志，SQLite Backup API 在发送停止信号前取得 schema 75、`quick_check=ok` 的一致快照；7 张表发生逻辑变化，EntitySystem 时间戳从 0 变为 9.036680035293102。恢复同一快照并保留原生 `Saved/Metadata.json` 后，新进程从 schema 75 加载 3730 个实体，保留该时间戳并再次通过就绪探测，未再发送保存。输入、产物及原始日志哈希见[本次保存与重载证据](../reference-fixtures/2026-10-30-save_reload_build_26320224.json)。

该独立测试中的 Ctrl+C 退出码为 `0xC000013A`，未使用 Job 强制回收，但不作为正常 exit0 或正式后台停止验收。原生 Auto-save 的 `0 objects` 是集合规模返回值，不是两处内部虚调用的执行计数；该验收不据此宣称这些分支全部覆盖，也不外推多人或每个子系统的因果落盘。首次独立恢复遗漏 Metadata 导致原生重建库，其失败样本已保留；生产 checkpoint 原本保留 Saved 根元数据，无此恢复范围错误。

2026-10-31 的[正式管理后端生命周期验收](../reference-fixtures/2026-10-31-managed_lifecycle_build_26320224.json)使用同一 build 26320224 的独立副本，完成初次启动、重启、备份恢复后启动及归档恢复后启动。四轮均通过原生就绪探测，各发送一次正式保存请求，原生回执为 `ready/invoked/call_returned=true`、`write_confirmed=false`；四次正式保存回执状态均为 `Requested`。四次正式停止均记录原生进程 exit 0、`crash_flag=false`，运行记录清除。

该验收恢复了 59 个停服后的保存文件，验证恢复前保护备份及程序、配置字节保持；模拟归档提交中断后恢复成功。仅卸载程序库时保留了实例程序和 81 个保存文件，最终实例与程序库清理通过，外层运行器未发现清理异常。日志的 `persisted_readback=verified` 表示实例设置、配置读回及新的运行身份，不是本次保存请求的因果落盘证明。该验收未启用精确手动保存检查点读回，也未验证区域编辑后的规则回滚、客户端行为或断电持久性；此前独立保存重载和 schema 75 数据库副本证据保留各自范围。

本目录源码使用仓库许可证；官方加载器的来源、固定哈希及 MIT 许可证见[来源记录](../extensions/SOURCE.md)。原生 DLL 由仓库构建，上游二进制不纳入源码。
