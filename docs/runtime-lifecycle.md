# 运行与存档生命周期 / Runtime and save lifecycle

[简体中文](#简体中文) | [English](#english)

## 简体中文

本文说明启动、更新、保存、停止、备份与归档的职责和失败边界。各游戏的启动方式、保存动作和停服策略以 `modules/<id>/module.toml` 的 `process`、`runtime.player_actions`、`runtime.shutdown` 为准；扩展实现和兼容性要求见对应模块的 `control/README.md`。这里不维护第二份游戏命令表，也不表示某个游戏版本已通过发行验收。

### 调用链与所有权

React 页面和 hooks 通过 `api.ts` / `api-transport.ts` 调用 Tauri。桌面进程负责窗口、托盘、文件选择及应用更新；服务器操作通过当前用户命名管道进入后台 runtime service。后台拥有游戏进程、运行身份、任务、存储写入和恢复，界面重新连接不会重新执行业务操作。显式启用的 LAN HTTP 入口复用业务分发，保留本地专用操作的访问限制。

产品命令由 `command_registry.rs` 声明执行进程、访问范围和完成策略，Tauri commands 编排领域模块。`app-modules` 读取游戏定义；`app-steamcmd` 负责程序获取与更新；`app-runtime` 和 `app-platform-win` 管理进程；`app-storage` 管理实例、配置、存档、备份和归档。存储上下文租约、实例锁和安装锁由执行工作的任务持有，持续到实际完成或取消收尾，不能随界面等待方退出而提前释放。

### 启动、更新与扩展

启动先核对实例和程序归属、目录冲突、未完成恢复及程序安装状态。游戏更新遵循实例的固定版本或更新策略；更新之后检查运行环境和扩展，再创建游戏进程。扩展准备完成与游戏世界加载完成是不同状态。

| 结果 | 行为 |
| --- | --- |
| 更新源检查失败，现有程序完整且归属有效 | 可以使用已核验程序启动，并明确提示尚未确认最新版本 |
| 安装已开始但未完成，程序损坏或归属不一致 | 修复前不启动 |
| 附加扩展不兼容或下载失败，且能安全停用 | 使用游戏原生路径启动，限制该扩展提供的能力 |
| 旧注入器无法安全停用、未知文件冲突或清理失败 | 停止启动，保留数据和可诊断错误 |
| 缺少必要运行环境，准备失败或要求系统重启 | 停止启动并报告原因 |

扩展降级不能吞掉取消、并发写入或清理失败。下载超时必须先结束对应工作并释放资源，才能继续已验证安全的原生启动。停止操作独立使用模块声明的原生退出方式；如果某个游戏的管理器停服依赖同一扩展，例如 The Forest，应同时显示停服能力受限，不承诺不存在的原生替代。

程序目录发布若遇到目标冲突、阶段记录无效，或未收到发布成功回执且目标与回滚目录同时存在，会保留两者并报告恢复失败，核实归属前不自动覆盖或删除。

### 保存与停止

保存检查当前运行身份、已加载世界、实际能力和传输配置；旧运行的日志或回执不能授权新运行的保存。列表卡片、维护工作区及定时保存调用同一保存动作。结果区分“已请求”“原生确认完成”和“结果未知”，请求送达不等于存档已写入。未知结果不自动重发有副作用的保存请求。

正常停止执行模块声明的退出流程，并等待整个受管实例进程树退出。发送退出命令、收到回执或仅主进程退出都不足以报告完成。超时保留进程所有权；强制停止是独立操作。

### 备份、恢复与归档

普通文件备份和恢复要求实例完全停止。手动保存不能建立在线文件复制的一致性，缺失存档目录也不能通过创建空目录伪装备份成功。备份先写临时目录再发布，保留已有备份。

恢复目录替换使用持久化恢复记录。中断后保留恢复副本并阻止不安全写入；重新恢复时先核对归属及内容，完成回滚或恢复。清理受阻时报告保留位置，不删除未确认归属或已改变的内容。

归档在停服后保存既有清单所覆盖的配置、存档、日志、备份和程序依赖。程序更新、卸载与归档恢复检查共享程序及归档依赖，不能因移除一个实例而破坏其他实例。恢复后启动仍经过相同的归属、环境和世界就绪检查。

### 验证边界

单元测试验证规则，合成进程验证生命周期，浏览器组件测试验证界面交互；它们不替代真实游戏加载和安装包验收。保存持久化验收在停服前捕获快照，排除周期及停服自动保存，随后由新进程读取具体世界内容。备份和归档恢复也先读回恢复内容，再执行新的保存。缺少可靠内容读取方式时，不把启动就绪当作恢复验收通过。

执行入口、隔离数据及证据要求见[开发指南](development.md#改动与验证)和[游戏接入验证](game-integration-validation.md#简体中文)。签名安装包、在线更新、安装、升级及卸载要求见[桌面发行指南](desktop-release.md)。验收记录应绑定源码、程序版本和实际环境，保存在仓库外的证据目录。

## English

This document describes ownership and failure boundaries for startup, updates, saving, shutdown, backup and archival. The authoritative game definitions are `process`, `runtime.player_actions` and `runtime.shutdown` in `modules/<id>/module.toml`. Each module's `control/README.md` documents extension compatibility. These definitions do not certify a game version for release.

### Request flow and ownership

React views and hooks call Tauri through `api.ts` / `api-transport.ts`. The desktop process owns windows, the tray, file selection and application updates. Server operations pass through a current-user named pipe to the runtime service, which owns game processes, run identities, tasks, storage mutations and recovery. Reconnecting the interface does not replay operations. The explicitly enabled LAN HTTP entry shares business dispatch while retaining local-only access restrictions.

`command_registry.rs` declares command execution, access and completion policy. Tauri commands coordinate game definitions in `app-modules`, acquisition in `app-steamcmd`, process ownership in `app-runtime` and `app-platform-win`, and persistent data in `app-storage`. Workers retain storage, instance and installation leases until execution or cancellation cleanup actually finishes.

### Startup and optional capabilities

Startup checks ownership, conflicting paths, incomplete recovery and installation state. It applies the instance's update policy, checks runtime prerequisites and extensions, then creates the game process. A prepared extension does not establish world readiness.

A failed source check may reuse verified installed bytes with an explicit warning that the latest version is unknown. An incomplete installation, invalid ownership or damaged program blocks startup. An unavailable optional extension permits native startup only when it can be safely disabled. Unknown injected files, failed cleanup and missing runtime prerequisites remain blocking errors. Cancellation and outstanding writes must finish before any fallback proceeds.

Shutdown uses the module's declared exit mechanism independently of optional saving. Where the game has no verified independent mechanism, such as The Forest's managed shutdown, extension failure also limits shutdown capability; the interface must expose that limitation.

Program publication preserves both directories and reports recovery failure when the destination conflicts, phase evidence is invalid, or both target and rollback exist without a successful publication receipt; it does not overwrite or delete them before ownership is established.

### Saving, shutdown and retained data

Saving requires the current run, a loaded world and valid capability and transport configuration. Cards, maintenance and scheduled saves use the same action. Delivery, native completion and an unknown result are distinct outcomes; old logs cannot confirm a new request. Unknown outcomes are not retried automatically.

Normal shutdown waits for the entire owned process tree. Command acceptance or the primary process exiting is insufficient. A timeout retains ownership; forced termination remains a separate operation.

File backups and restores require a stopped instance. A save request does not establish online-copy consistency. Backup creation publishes a completed temporary directory and preserves existing backups. Restore replacement uses a persistent recovery journal; interrupted operations retain recovery copies and block unsafe writes until ownership and contents are verified. Cleanup failures report retained locations instead of discarding uncertain data.

Archival preserves the configuration, saves, logs, backups and program dependencies covered by its manifest. Program maintenance and removal check shared and archived dependencies so that one instance cannot invalidate another. Restored instances pass through the same startup checks.

### Verification

Unit, synthetic-process and browser-component tests establish different boundaries. Real persistence acceptance captures the save before shutdown, excludes automatic saves, and verifies concrete world content through a new game process. Backup and archive recovery must read back restored content before another save. Startup readiness alone cannot establish content recovery.

See [Development](development.md#english) for isolated execution and evidence, [game integration validation](game-integration-validation.md#english) for game contracts, and the [desktop release guide](desktop-release.md) for signed-package, online-update, installation, upgrade and uninstall acceptance. Keep run-specific evidence outside the repository, bound to the tested source, program version and environment.
