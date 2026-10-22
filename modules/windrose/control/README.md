# Windrose 手动保存扩展

管理器的“立即保存”调用专服自己的 `UR5CoopProxy::SaveBackupsSync`，等待原生 checkpoint future；不会拷贝运行中的 RocksDB，也不将文件备份任务当作保存。`true` 回执仅确认本次原生 checkpoint 成功；`false`、超时、异常或丢失回执均显示结果待确认，不自动重发。

支持组合为 Steam app `4129620` / build `24913903`、游戏 `0.10.0.9.32-22d39a16`，以及官方 UE4SS `3.0.1-1125-g527a483b`。游戏 Shipping 映像 SHA-256 为 `441acd04428aa5bd6535bc6565ed659a343001615d5322e8c65141b08b356854`，UE4SS.dll 为 `91d5444f41d19d0bb502f0fbdc15d76421e20359e898caa39e41e134c8f87958`。加载器来源及配置见 [现有扩展证据](../extensions/SOURCE.md) 与 [专服 EngineTick profile](../extensions/UE4SS-settings.ini)。不兼容版本直接拒绝调用。

点击启动后，管理器在创建游戏进程前自动准备独立程序目录、安装固定官方 UE4SS 与真正的 `dwmapi.dll` 代理，并配置和发布保存扩展。内嵌归档、核心与代理均核验固定哈希，首次准备无需下载；只合并保存必需的配置字段，保留其他字段、注释和现有模组。已验证的兼容加载器可复用；有歧义的重复配置、未知核心或代理以及被外部修改的受管文件会保留并报告冲突。管理器发布自己持有的 `LgsmWorldSave/Scripts/main.lua`、`native/LgsmWindroseSaveControl.dll` 与 `enabled.txt`，以私有根目录 `.langame-windrose-world-save.json` 记录各文件哈希，加载器另有归属记录；已知中断可以恢复。共享程序目录不被修改。DLL 随应用打包，普通用户不需要 C++ 编译工具。

原生地址仅适用于上述完整映像：`SaveBackupsSync` RVA `0x6bd4d10`，ABI `bool(void* this)`；两个原生调用点只传 RCX。桥检查入口机器码、当前线程、真实 `R5CoopProxyServer` class/vtable/Outer，以及固定完整映像和加载器哈希。Lua 只在当前正式 GenlandiaMulty authority world、同一 GameInstance、已开始的 GameState、归属明确的 collection/documents 中使用它。调用前后检查世界代次。主机将回执中的原生 PID/创建时间与当前管理进程的唯一 Shipping 子进程比对，不能仅凭 bootstrap 仍存活接受成功。

请求仅有 `saveprobe` 和 `save`，以 nonce、boot/world token 和短期限关联；所有 UObject 读取和原生调用位于 EngineTick。每个请求只调用一次；watchdog 在调用进入后只能产生未知结果，迟到回调不能覆盖已失效代次。取消 UI 等待后，实际 worker 继续持有实例锁和 storage lease，直至本次 I/O 结束。准备还持有程序生命周期锁。安装使用带 pending 哈希的可恢复 CAS，跨版本恢复保留上一轮已验证的实际文件归属。

2026-10-19 在全新私有专服副本、独立 Saved 和随机密码中实测：只在 host-ready 后调用一次，原生日志同一操作 Id 出现 `IsInGameThread true` 与 `Result true`；当前世界 `_Latest.zip` 从 9,208 字节变为 14,218 字节且 SHA-256 改变。随后停止副本并重新启动，原生 DB 加载同一个 IslandId，正式世界重新 host-ready。最终生产 Lua + DLL + 公开文件协议再次实测 `write_confirmed=true`，重复轮询同一请求未产生第二次调用。该重启证据只描述隔离测试，不表示每次保存均执行了恢复测试。正式用户实例及存档未参与探针；真实多人状态尚未验收。

开发构建：`scripts/build_windrose_save_control.ps1 -OutDir <外部绝对目录> -RunTests`。Rust/Lua fixture 由桌面 crate 的 `windrose_world_save` 测试运行；Lua fixture 模拟引擎与文件边界，不等同于真实专服。
