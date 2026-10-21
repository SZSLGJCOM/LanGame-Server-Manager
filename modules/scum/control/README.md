# SCUM 原生保存请求

维护页和卡片在游戏线程调用一次 `UEntitySystem::AutoSave(this, true)`。该原生入口返回 `void`，因此界面报告“保存请求已发送”，不声称数据库异步任务或写盘已经完成。原生调用异常、身份变化、请求过期和缺失回执仍作为失败或未知结果处理，不自动重发。保存不会停服或制作备份。

## 绑定版本与调用范围

当前绑定 Steam app `3792580` 的两个构建，按 `SCUM/Binaries/Win64/SCUMServer.exe` 完整 SHA-256 选择固定配置：

| Build | SHA-256 |
| --- | --- |
| 25247308 | `4ca11449c4c8d3a243c797c719f66e33192eefad53a9c64cca9aac925c9e39ad` |
| 25803875 | `070fcad918ba04eb9797db29674cadbd6691427b936fe354bca23b008c8c9971` |

加载器为官方 UE4SS `3.0.1-1125-g527a483b`，核心 SHA-256 为 `91d5444f41d19d0bb502f0fbdc15d76421e20359e898caa39e41e134c8f87958`，配置见 `extensions/UE4SS-settings.ini`。其他构建需要重新核对 ABI、对象布局及真实调用，不能只更新哈希。

`AutoSave` RVA `0x1b94040` 的 ABI 为 `void(this, bool force)`。正常 tick 传 `false`；本入口传 `true`，覆盖全局持久化状态（`0x1ba7060`）、已登记坐标变更（`0x1b94ee0`）和已登记实体的强制保存（`0x1b94250`）。它不调用退出函数，也不承诺覆盖独立子系统、尚未到达服务端的数据或整个进程内存。

上述地址来自 build 25247308。在 25803875 中，完整 528 字节 AutoSave 入口、三个保存阶段、EntitySystem 构造与 Actor::EndPlay 指令除重定位引用外一致，对象字段偏移未变。新入口为 `0x1bb2680`，三个阶段为 `0x1bc59c0`、`0x1bb3520`、`0x1bb2890`；类 singleton、vtable 与 GetWorld 分别为 `0x7500180`、`0x5edfe20`、`0x1552cc0`。构建测试分别执行两个配置的准入检查；这不替代各版本的真实保存验收。

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

固定版本曾在全新私有空服世界中执行一次真实 `AutoSave(true)`：原生返回 `returned`，生产协议的 `ready/invoked/call_returned=true`、`write_confirmed=false`，服务器继续运行。正常重启后同世界从数据库加载实体，数据库 `quick_check=ok`。此证据表明原生入口可以调用且世界可以重新加载，不证明每个子系统的因果落盘或真实多人行为；每次版本适配仍需重新验证。

本目录源码使用仓库许可证；官方加载器的来源、固定哈希及 MIT 许可证见[来源记录](../extensions/SOURCE.md)。原生 DLL 由仓库构建，上游二进制不纳入源码。
