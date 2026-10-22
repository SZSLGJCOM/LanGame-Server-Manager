# 手动原生保存

保存通过现有 UE4SS 扩展，在游戏线程反射调用 `PersistenceSubsystem:SaveGame(false)` 一次。接口只有 `bAdditionalLogging` 布尔参数，没有完成回执；游戏自行管理世界和玩家存档。LanGame 返回“已请求保存”（`Requested`），不会把函数返回或 `IsSavingGame` 变为 false 当作写盘成功。

调用前核对当前唯一服务器世界、GameInstance、PersistenceSubsystem 和 SpudSubsystem 的所属关系，以及两个 UFunction 的标志和参数。`IsSavingGame` 只用于拒绝正在保存的请求。保存独立于在线玩家名单和封禁用 C++ 桥，玩家连接暂未就绪或该桥不支持新游戏版本不会阻止可用的保存接口。

宿主为每次受控启动生成 `LGSM_DRAGON_SAVE_RUN`，保存在 `langame_player_control/save-session.txt`。保存期间持有该文件和祖先目录，先读取当前 boot/world 令牌，再发送唯一 nonce 的保存请求。真实 Shipping 进程的路径、创建时间和受管进程树在交换前后核对；过期请求、世界切换、签名变化或忙碌状态在调用前拒绝。同一 nonce 不重复执行，调用异常、响应丢失或超时均报告结果未知，不自动重发。调用方取消后，worker 仍持有存储 lease 与实例 mutation，直到交换结束。

这条路径不需要固定原生函数地址、MinHook、文件写入拦截或完成轮询。`LgsmPlayerControl.dll` 仍用于玩家封禁与解封，保留其单独的版本边界；反射踢人和只读名单按各自能力检查。所有扩展仅在停服时按已有归属记录升级，外来文件保持不变。

Lua 行为测试覆盖原生调用一次、忙碌、当前世界、函数签名、排队超时、未知结果、重复 nonce 和原有玩家管理。Rust 测试覆盖会话绑定、拒绝伪造完成、丢失响应不重发及取消后的资源所有权。实际保存验收另需在隔离实例中捕获手动调用后的存档快照，停止后只恢复该快照，再启动并读取原生世界内容；组件测试和 `Requested` 回执本身均不能替代此项验证。
