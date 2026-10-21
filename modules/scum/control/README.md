# SCUM 原生持久化保存控制

维护页和卡片调用固定版本 `UEntitySystem::AutoSave(this, true)`，覆盖该入口的全局持久化状态、已登记坐标变更和已登记实体强制保存。程序自动追踪本次原生任务、SQL 事务、系统写盘结果和完整数据库页读回，通过后才确认成功。此处的范围是这个原生保存入口；没有把其他独立子系统、未收到服务端的客户端数据或整个进程内存当作已保存。操作不会制作备份或停服。

控制仅接受 Steam app `3792580`、build `25247308`、`SCUM/Binaries/Win64/SCUMServer.exe`，SHA-256 `4ca11449c4c8d3a243c797c719f66e33192eefad53a9c64cca9aac925c9e39ad`。加载器为官方 UE4SS `3.0.1-1125-g527a483b`，核心 SHA-256 `91d5444f41d19d0bb502f0fbdc15d76421e20359e898caa39e41e134c8f87958`，专用配置复用 `extensions/UE4SS-settings.ini`。其他构建必须重新取证，不能只更新哈希放行。

## 安装与启动

点击启动后，LanGame 在创建游戏进程前自动准备独立程序目录、安装固定官方 UE4SS 和真正的 `dwmapi.dll` 代理、配置保存所需字段，并部署自有 Lua、共享闭合协议、`LgsmScumSaveControl.dll` 及提前加载的 C++ 入口 `dlls/main.dll`（`LgsmScumSaveBootstrap.dll`）。归档与文件均校验固定哈希；兼容已装加载器可复用，其他配置字段、注释及现有模组保留。有歧义的重复配置、未知核心或入口代理及被外部修改的受管文件会保留并报告冲突；已知中断发布可以恢复。不修改共享程序目录。运行中的服务器须正常重启，使本次启动身份生效。正式入口为 `SCUMServer.exe`，没有额外根目录启动器。

当前构建的清单请求 `highestAvailable`。SCUM 的子进程环境使用 `__COMPAT_LAYER=RunAsInvoker`，以管理器当前 Windows token 运行，不增加权限；只影响该子进程，不修改全局环境、注册表或防火墙。文件与系统权限仍由操作系统执行。

## 固定原生合同

`UEntitySystem::AutoSave` RVA `0x1b94040` 的 ABI 为 `void(this, bool force)`。正常 tick 传 false，退出路径传 true 后另行设置退出标志。控制仅调用一次 `AutoSave(currentEntitySystem, true)`；该调用包含 `global 0x1ba7060`、`transforms 0x1b94ee0` 和 `registered entities 0x1b94250(force=true)` 三阶段。true 分支遍历所有已注册实体的持久化检查，不受正常 tick 时间预算限制。未调用退出函数、玩家 SaveState、控制台、RCON 或其他未证实的数据库入口。

执行前同时确认完整游戏/加载器哈希、函数字节、原生类/vptr、唯一 EntitySystem、当前实例 outer、原生 GetWorld、UWorld.OwningGameInstance、GameNetDriver、当前 GameMode authority，以及当前 GameMode/GameState 的原生 ActorHasBegunPlay 状态。EntitySystem 必须具备 authority 且不在加载、保存或退出中。所有对象检查及唯一一次调用都位于 EngineTick；原生桥再次核验 UE4SS 游戏线程标志。

原生 ActorHasBegunPlay 的固定状态来自该二进制 `AActor::EndPlay` RVA `0x3ec3aa0`：对象 `+0x5c` 高两位必须为 `0x80`，拆除前即清除。此处不使用空服中仍为 false 的 GameState 网络复制标志。

宿主确认 PID、进程创建时间、启动 token、当前 boot/world token 与请求身份。提前加载的 C++ 模块必须在任何非 CDO 的 EntitySystem/UDbConnection 创建前安装观察器，晚装明确拒绝。完整固定代码范围、265 项 SQLite API 表、原生 vtable 和全部自有 hook/IAT 都核对；hooks 与未决状态保留到进程退出，不随 Lua 失联或非法 IPC 卸载。

原队列 `0x1abced0` 在返回前已唤醒 worker，因此观察器在同一递归生产者锁中登记真实 sequence，再允许 worker 复制。worker/batch `0x1ad3aa0/0x1acea90` 会顺序执行同一队列中的不同业务任务；观察器逐一核对实际 ID、callable 形状及复制后的 payload，在原 `0x1aceb8c` 调用点为每个任务单独建立 Scope。完成还要经过原 main pump `0x1ab9df0` 的 B0 continuation/C0 ID。D0 是取消集合；原 `0x1ab5d90` 成功取消时即拒绝保存成功，不能把被跳过的 continuation 或空队列算作完成。

固定 `FItemEntityDbAsyncCache` 的 12 个 producer、11 个 writer 可以与本次 AutoSave 共用一个 worker 批次。只有精确 caller/嵌套 callable/vptr、当前主线程、EntitySystem→worker weak 身份及同一原生数据库连接全部符合时，才登记独立、有界的物品任务子账本。物品任务只允许原生无 continuation 的 C0 完成路径；其任务、SQL 及系统写盘各自闭合，不能支付 AutoSave 三阶段或本次任务的债务。其他来源、未知 callable、重复/丢失/乱序 ID、取消和跨 epoch 均拒绝。子账本的失败或未知同时使本次请求失败或未知，不能以后台任务成功覆盖主保存失败。

原 worker 在事件合并后也会调用空 batch；固定 `0x1aceaba` 在 A0 count 为零时直接返回，不执行 callback 或 SQL。形状合法的空调用仍原样执行一次，不建立 frame、不登记或支付任何完成债务；负数、越界容量和非空无数据等坏形状继续拒绝。

实际 SQLite `3.39.4` 的 prepare、bind、step、reset、finalize、exec 及事务结果分别保留，包含绕过公开 step 的内部 VM 路径。`UDbConnection +0x48` 限定连接线程，`+0x50/+0x58/+0x60/+0x68` 区分原生 BEGIN/COMMIT/ROLLBACK statement。原“提交”包装函数在成功回滚后也可能返回 true，因此包装返回值和 autocommit 都不能单独确认。系统 WriteFile、截断、刷新、映射和 CloseHandle 的真实结果另行观察；SQLite 吞掉的关闭失败仍使本次保存失败。

每个实际发生写入的本次连接必须在它自己的线程、事务闭合后，用原包 SQLite serialize(flags=0) 获取独立完整页载荷，与独立 READONLY 物理连接的完整页载荷逐字节比较（含 WAL 视图）。数据库、现存 WAL/SHM、父目录、文件 ID 和单链接普通文件归属都校验。读回不是备份，不执行 `UDbConnection::SaveToPath 0x1aff310` 或 SQLite backup API。

已进入请求的原生结果在它自己的全部任务、事务、系统 IO、读回和辅助账本闭合后终结。主线程下一次无 Scope 的 SQL 调用在增加新 SQL inflight 之前，可复用原 observe 的对象、profile、deadline 和全部闭合判定封存结果；任何剩余债务继续按原规则拒绝外来 SQL，不减去活跃计数。封存后的独立 SQL 原样执行，不加入或支付此前请求的账本。公共 Query 仍需通过原请求、当前 world 与进程身份检查才能发布终态；此边界不重发 Save，也不放宽 Cancel 或原生准入。

原坐标待保存集合会在异步 IO 前被清除。已进入的失败/未知请求因此保留该进程代的未清偿债务，后继空队列不能洗成成功；需重新启动该实例后重新建立观察器。每次只调用一次原保存入口，不因超时或回执丢失重发。同 nonce 的内部 Query 可推进这个已接收请求的原生准入；它不是纯只读操作，也不创建第二个 Save。原生总预算从首次请求准入开始计 30 秒，不因 Query 重置；`failed` 明确传 `save_failed`，`unknown` 明确传 `save_call_unknown`，不会留下无错误的“写入待确认”让用户自行检查。

原生诊断保留本次首次拒绝原因。后续外来 SQL、清理或超时事件继续累积 failed/unknown 状态，不覆盖最初的原因；failed 优先级以及原生任务、SQL、系统写盘和完整读回的确认条件保持不变。
原生诊断还锁存首次外来队列的有限上下文：调用者 RVA、原 worker 身份检查是否执行及其结果、原生任务与 SQL/IO 债务状态，以及扣除当前队列钩子自身 Flight 后的其他任务计数。诊断不新增 native 锁、任务扫描或保存放行；首次拒绝与失败/未知状态保留，只有完整原生完成链仍可确认保存。

## 同一请求的 worker 准入

Probe ready 只表示 profile、实体和 world 已就绪，不预留数据库 worker。Save 遇到合法非空的原生队列、未归还 ID、已知 task/SQL/IO inflight 或暂时锁冲突时，在同一请求内等待下一次既有 Query。等待阶段不安装 Runtime.active，不建立 root 或辅助任务账本。主线程用原顺序非阻塞取得 CS18/40/68，复核同一对象、worker/thread/world/profile、四队列与 ID pool 全空后，才发布 owner 并调用一次原 AutoSave(true)。worker 停止、未知/变化身份、坏队列、过期和异常均终态，不会由晚 Query 恢复。

共享观察验证失败会终止匹配请求的私有准入状态；错 nonce 不取消其他请求。已进入原根的失败/未知仍保留 Runtime.active、IO 债和 pins，新请求不能以空队列绕过。公共 Cancel 后没有后续 Query 时，现有 Lua 取消/代际检查阻止下一次回调；原生端没有新增 hostCancelAck，也没有后台线程自主启动等待中的 Save。未进入根的过期占位可在后继新请求中终止，不能释放已进入根的债务。

## 构建与验证

```powershell
.\modules\scum\control\Build.ps1 -OutputDirectory <仓库和原服之外的输出目录> -RunComponents
# 可选：仅提供已经物理复制、无 reparse/hard-link 的固定映像。
# <fixture>\image\SCUMServer.exe 必须为上面的精确 SHA。
.\modules\scum\control\Build.ps1 -OutputDirectory <输出目录> -RunComponents -NativeFixtureRoot <fixture>
```

MSVC `/W4 /WX /MT` 构建 DLL、提前入口和组件。组件包括已有 authority/readiness/消息格式拒绝回归、真实 MinHook/Windows unwind，以及固定包实际 SQLite 的事务/WAL/完整页读回/写入/刷新/关闭/绑定故障。产品 hooks 直接安装到受控物理副本执行，另外运行原队列、原 worker/batch/main pump/取消函数和真实 SQLite 写入闭环；仅 UObject/系统事件/分配器边界由构造夹具提供，未 mock SQLite 或 native task drain。所有原生写调用前保留普通绝对 owned-path OS guard；原始包仅只读。上述组件不启动游戏、不等同正式开服或真实玩家验收。提供 NativeFixtureRoot 时，另运行 90 项同请求准入和真实 shared IPC/cache 回归：四队列/ID/inflight、另线程实际持锁的事件屏障、原 deadline、wrong tuple、终态旧 nonce/晚 Query、新请求及原根异常后 IO debt/pin 保留。该组件只读映射 fixture 的固定 PE，所有新 IPC 写到 OutputDirectory 下的全新子目录；原 AutoSave body 是外部计数边界，未以空 body 制造确认。

混合批次组件直接调用一次原 `AutoSave(true)`，让原三阶段在空的已登记实体集合上运行，执行原全局 writer 与原物品 bool/float producer/writer、worker、复制队列和 main pump，并由真实 SQLite 与完整页读回确认。原生事件 ack 强制覆盖完整 worker 的自动空唤醒，不靠固定延时。另检查全部固定物品 callable 元数据、后台写入失败、未知来源、重复/乱序、世界代变化，以及遗漏主任务 C0 时物品完成不能代偿、随后按期限明确拒绝。

`tests/policy_fixture.lua` 执行真实生产 Lua 与共享协议，40 例覆盖成功、明确失败、未知、同 nonce 多次观察、启动身份、世界变化、排队/执行期限、迟到结果和不重发。文件/DLL/UObject 边界由确定性构造环境替代，不把 fixture 称真实游戏。

以下是旧桥历史记录；其 `returned` 证据不作为本次完成观察器的整服验收。新桥仍需由完整管理器启动后正式验证保存、正常停服及同世界重启读回。

2026-10-19 在普通程序副本、全新私有世界、随机密码和独立端口上执行真实生产 Lua/DLL。原生 probe 通过固定文件哈希、对象归属与实际 ActorHasBegunPlay 检查；只发送一次实体持久化请求，原生回执 `returned`，生产协议返回 `ready/invoked/call_returned=true`、`write_confirmed=false`，PID 与创建时间匹配。服务器继续运行至少 30 秒，未触发退出流程。

随后不发送手动保存地重启相同私有世界：原生日志 `DbLoadState` 从数据库加载 3,960 个实体，正式 The_Island 世界再次通过原生就绪检查。重启前后数据库 `quick_check=ok`，原有 3,963 个实体 ID 中有 3,960 个保留，运行时另有正常增删。此空服样本证明真实调用可返回、世界持续运行及存量实体可加载；不据此声称每个子系统的因果落盘确认或真实多人行为已验收。全部副本路径所属进程已回收，未触碰原服数据或系统权限配置。

本目录原生桥和 Lua 由 LanGame 独立编写，使用仓库许可证；官方加载器的来源、固定哈希及 MIT 许可证见 [来源记录](../extensions/SOURCE.md)。原生 DLL 由仓库构建，不把上游二进制纳入源码。

首次 foreign queue 诊断还记录 `queue_parent_rva` 和 `queue_parent_context`：固定通用包装器 `0x1ba6380` 仅在本线程保留原始 system 与直接 caller 的有限来源。context 位 1 表示包装器帧存在，位 2 表示 caller 在固定映像内，位 4 表示 system 与当前请求相同；这些元数据不建立 Scope、不收养任务、不放宽原队列/SQL/完成规则。嵌套调用、异常返回和不同线程都恢复原 TLS；首份拒绝诊断保持不变。加入两字段后的保守诊断上限为 457 字节，仍受 512 字节界限约束。

新增包装器组件直接运行生产 hook，验证不透明参数、原调用次数、LastError、异常传播、嵌套恢复和线程隔离。`NativeFixtureRoot` 还执行固定物理副本完整 437 字节包装器与 87 字节 callable adapter，通过真实生产 MinHook/trampoline；仅分配器、weak-object、队列发布、cookie 和 functor 生命周期边界由 host 提供，不执行游戏入口、Save、SQL 或 worker。这是受控 ABI 证据，不能替代真实开服保存确认。
