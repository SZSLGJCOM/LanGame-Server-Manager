# 手动世界保存确认

保存复用现有 UE4SS 和玩家管理扩展，不移除或替换其他玩家安装的 mod。`save.lua` 在构建时前置拼接到原先归属的 `Scripts/main.lua`；同一个 `LgsmPlayerControl.dll` 保留玩家管理导出并新增 `luaopen_lgsm_dragonwilds_save`。启动前自动检查和准备加载器、DLL、脚本及 MinHook 许可证，运行中不热装。

固定官方 Windows build `25811348`，Shipping EXE SHA-256 `89ed65335a46db067a361d35ae4e6be80857a4c1f2453510600d632879dfbeed`。原生桥先校验实际映像、UE4SS、主线程及固定代码范围，再验证当前唯一世界、GameInstance、Persistence、SPUD 和原生 provider。允许游戏既有 Null 和 LocalPlayer provider，其他 provider 拒绝认证；不改游戏保存策略。

原生 provider 的弱引用序列号按需分配；合法的新建 provider 可暂为零。只读 Probe 不分配序列号，继续验证原有四个目标；provider 的完整弱引用准入留在 Save 阶段。普通身份读取及后续 epoch 校验仍拒绝未分配序列号。仅实际 Save 准入在原有目标检查通过后、确认游戏线程和固定 profile、provider 的对象表项、实际 class、所属 Persistence 与原生接口仍一致时，调用该版本原生弱引用构造函数分配正序列号，并重新核对返回的 index/serial、实际对象表项、class 和归属。不会直接接受零或负序列号。

此版本 `0x1693d00` 的第二个参数是 `UObject* const*`，与其他游戏的弱引用构造函数 ABI 不同；它调用 `0x167aac0` 原子分配序列号。完整构造函数、分配器各 chained fragment、异常展开信息和对应 `.pdata` 表项均纳入 profile。新增分配路径拒绝未初始化或溢出边界的原生序列号计数器，不改变游戏既有 Fatal 策略。实际身份分配不触发 Save，原有单次 dispatch 与完成确认条件不变。

宿主启动时生成 `LGSM_DRAGON_SAVE_RUN`（32 个小写十六进制字符），通过普通文件 `langame_player_control/save-session.txt` 绑定本次受控启动。外部非空同名环境覆盖被保留并明确拒绝。保存过程持有该文件和所有祖先目录，回执必须匹配本次 run、PID、精确 u64 FILETIME、boot、world、请求 nonce 和原始 save_id。

宿主先发只读 `saveprobe`，再发一次 `save`。进入原生根调用前登记 nonce；同一 nonce 只观察既有记录，不能重发原生保存。后续使用 fresh request_id 的 `savequery`，并携带原保存 request_id 作为 save_id。Query 只读，绝不创建或重发保存。宿主自动核实最多 30 秒；传输单次预算五秒，并预留查询预算，不在截止后接受迟到成功。调用方取消后，blocking worker 继续持有存储 lease 和实例 mutation，直到其生命周期结束。

原生执行固定 `PersistenceSubsystem::SaveGame(false,false)` 一次，保持所有游戏原生写入和返回值。只有以下条件同时满足才输出 completed：

- 同次根调用返回，当前世界及 provider 身份稳定；根声明的数量与所有 world/player 句柄、终端回调匹配，最终 aggregate 完整返回。
- 当前 slot 的唯一世界 writer 正确创建、序列化、flush 和 close；独立观察实际 Windows CloseHandle 结果，不能相信吞掉关闭失败的原生包装返回值。
- 必需 SpudCache 读取成功且内容/身份未变；缺失的 level 不能因游戏仅记日志并继续而被忽略。
- 版本备份复制、旧档备份移动及最终 publication 均返回成功并完成内容读回。最终世界文件与关闭后的 owned writer SHA-256、大小、卷及 file ID 匹配。目录和文件为普通对象，祖先不可被换走，读取文件只有一个链接。
- 所有原生观察写债释放。超时、异常、重复或外来回调、身份漂移和任何失败均不能被后来的 true 改写成 completed；未静默的 native debt 继续保留保护，新 Save 被拒绝。

原生记录最多 512 条，进程生命周期内不驱逐；容量满后拒绝新的保存请求。Native 观察器自身的 120 秒终止判定用于保留迟到回调和写债，不能延长宿主 30 秒确认预算。

只有 completed 会由宿主转换为 `WorldSaveCompletion::Confirmed`，`write_confirmation_pending=false`。任何失败、缺失或超时由程序自动核实后返回错误，并说明未报告成功、未重发；不把“命令已发出”或 void 函数返回当成保存成功。

验证分开记录：`Build-Components.ps1` 覆盖构造状态边界、真实 Windows 普通文件和失败输入、实际 MinHook trampoline/unwind、正式请求解析，以及 production sink 在真实文件系统上的 factory/close/copy/move/readback 反例。Lua fixture 保留玩家管理与保存协议覆盖。这些组件不会执行游戏专服，不能替代管理器正式构建、本机开服、完整保存及正常停止重启验收；新 native 闭合实现尚待该完整验收。

旧版 2026-10-19 隔离运行只证明两次原生调用、重启就绪和清理，不证明因果写入完成；不能用旧结果验收本次实现。
