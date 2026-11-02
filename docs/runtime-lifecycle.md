# 运行与存档生命周期 / Runtime and save lifecycle

[简体中文](#简体中文) | [English](#english)

## 简体中文

本文说明启动、更新、保存、停止、备份与归档的职责和失败边界。各游戏的启动方式、保存动作和停服策略以 `modules/<id>/module.toml` 的 `process`、`runtime.player_actions`、`runtime.shutdown` 为准；扩展实现和兼容性要求见对应模块的 `control/README.md`。这里不维护第二份游戏命令表，也不表示某个游戏版本已通过发行验收。

### 调用链与所有权

React 页面和 hooks 通过 `api.ts` / `api-transport.ts` 调用 Tauri。桌面进程负责窗口、托盘、文件选择及应用更新；服务器操作通过当前用户命名管道进入后台 runtime service。后台拥有游戏进程、运行身份、任务、存储写入和恢复，界面重新连接不会重新执行业务操作。显式启用的 LAN HTTP 入口复用业务分发，保留本地专用操作的访问限制。

产品命令由 `command_registry.rs` 声明执行进程、访问范围和完成策略，Tauri commands 编排领域模块。`app-modules` 读取游戏定义；`app-steamcmd` 负责程序获取与更新；`app-runtime` 和 `app-platform-win` 管理进程；`app-storage` 管理实例、配置、存档、备份和归档。存储上下文租约、实例锁和安装锁由执行工作的任务持有，持续到实际完成或取消收尾，不能随界面等待方退出而提前释放。

### 启动、更新与扩展

启动先核对实例和程序归属、目录冲突、未完成恢复及程序安装状态。游戏更新遵循实例的固定版本或更新策略；更新之后检查运行环境和扩展，再创建游戏进程。扩展准备完成与游戏世界加载完成是不同状态。

启动操作和日志订阅持续到操作结束，不随服务器页面卸载而释放。切换到游戏库或其他实例再返回时，保留本次启动状态与进度，拒绝重复提交。后台列表、详情和运行概览将有效启动预约显示为“启动中”，仅修改查询结果，不将临时状态写入数据库；重新加载界面后读取当前启动日志，避免使用上一轮运行的就绪或失败信息。操作完成时刷新状态并保留用户当前选择。

LanGameCMD 的显示流以每个运行日志源的会话和版本标识排序，完整行增量追加，未完成末行替换预览；实际重复日志不会按文本去重。完整行继续写入原有托管日志，预览不参与就绪解析或持久化。界面按浏览器帧合并更新，首次打开、事件缺口和连接重建通过同版本的有界文档补读；原生日志使用精确位置的文档补丁，继续按当前进程过滤。控制台显示保留最近 400 行及有界内容预算，不改变磁盘日志的保留策略。

一次已接受的启动独立拥有启动记录和日志，不依赖游戏运行记录是否已经创建。自检、更新、校验、启动结果及诊断连续保留；进入运行状态只接续游戏日志，不清除启动区域。重新打开或刷新后读取本次启动回执，可分页回看磁盘仍保留的较早内容。失败、取消和服务中断保留各自状态；诊断按事件身份幂等记录，不以日志文本去重。

SteamCMD 原始 stdout/stderr 按各自字节偏移传递并独立组装 UTF-8，任务进度与原始记录分开。内部下载、校验等测量替换当前进度，阶段边界和终态落盘；完整原始行保持原样。原生日志在启动等待期间即可进入启动记录，运行读取只越过已确认保留的完整行位置；文件身份、进程身份或边界内容变化时不得沿用旧位置。

所有模块通过 `runtime.startup_readiness` 声明就绪依据与启动期限，由同一个启动流程等待、校验进程身份、处理取消及清理。日志型模块共享 `app-storage` 的精确阶段解析；协议型模块验证完整查询响应、当前世界或本轮状态文件。内置 A2S 模块同时要求本次最终加载日志，查询端口提前回应不能替代世界加载完成。进程存在、端口监听、旧日志和旧状态文件都不能单独完成启动。日志在创建进程前建立读取基线，所有地图或分片全部就绪后才登记“运行中”。模块没有声明依据时，在创建正式进程前报告错误，不退回“进程活着即成功”。

同一个红色按钮在“启动中”显示“取消”，取消收尾期间显示“取消中”并禁止重复点击；就绪后显示“停止”，不新增按钮。发起和取消携带同一个启动尝试标识，迟到的请求不能误停下一次启动。取消会清理本次拥有的整个进程树，并等待工作任务和清理结果确认；返回 `startup_cancelled`，不声称世界已保存，也不创建停服自动备份。就绪已经提交后，原按钮仍执行正常保存和优雅停服。

Valheim 自定义 `-logFile` 的健康观察接续本次启动的最终日志来源、完整行游标及类型化就绪状态，绑定托管日志、会话和操作系统进程身份后保存到实例拥有的日志元数据中。重启管理器或修改下次启动配置不会改变当前运行的来源；原生日志替换、截断或缺失会撤销就绪证据，不重播旧文件中的启动完成信息。原生日志只读，托管控制台、停服与保存证据仍使用原有记录。

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

Startup operations and their log subscriptions remain owned until completion, independently of the server page. Returning from the library or another instance preserves progress and prevents duplicate requests. Lists, details and runtime overviews project active startup reservations as Starting without persisting that temporary state. A reloaded interface reads the current startup log instead of readiness or failure evidence from an earlier run. Completion refreshes state without changing the user's current selection.

LanGameCMD orders each console source by its stream identity and document revision. Completed rows append; unfinished rows replace their preview, without text-based deduplication of real repeated output. Completed rows retain the existing durable transcript; previews never enter readiness parsing or persistent logs. The interface batches rendering by browser frame and reconciles initial loads, event gaps and reconnects from a bounded document at the same revision. Native logs use exact positional document patches and retain their current-process filtering. The visible console keeps the latest 400 rows within a content budget, independently of the existing disk retention policy.

Each accepted startup owns its receipt and transcript independently of game run registration. Preflight, updates, verification, startup outcomes and diagnostics remain available when the running console attaches. Reloading reads the current attempt and pages through earlier retained output. Failure, cancellation and interrupted-service states remain distinct. Diagnostics are idempotent by event identity, never by log text.

SteamCMD stdout and stderr carry independent byte offsets and UTF-8 record boundaries, separate from task progress. Internal download and verification measurements replace the live progress row; stage boundaries and final measurements are durable. Complete raw records remain intact. Native output is captured during startup waiting, and the running reader resumes after the confirmed complete-record checkpoint only while its file identity, process identity and checkpoint content still match.

The same red control displays Cancel during startup, Cancelling with repeat clicks disabled during cleanup, and Stop after readiness commits. A locally initiated start and its cancellation carry the same startup attempt ID, so a cancellation that reaches the service first prevents that delayed attempt from launching and cannot stop a replacement. Cancellation interrupts preparation and forcefully cleans up the owned startup process trees; its receipt waits for all workers and confirmed cleanup. It returns `startup_cancelled` without a run ID, world-save claim or automatic stop backup. Once readiness commits, the same Stop control uses the normal save and graceful shutdown path.

Every module declares its evidence and deadline in `runtime.startup_readiness`. One shared startup barrier owns waiting, process identity checks, cancellation and cleanup. Log providers reuse the exact stage parsers in `app-storage`; protocol providers require a complete owned response, selected world or fresh native status file. Bundled A2S modules also require their current final loading log: an early query response cannot establish world readiness. Process liveness, a bound port and historical output cannot establish readiness. Log cursors are captured before process creation, and every launched map or shard must be ready before Running is registered. A missing declaration fails before the formal launch instead of falling back to process liveness. ASA uses this same barrier with its final join advertisement; its native timestamps must belong to the current live process.

For a custom Valheim `-logFile`, health observation continues from the final launch's native source, complete-line cursor and typed readiness state. Instance-owned log metadata binds this handoff to the managed transcript, session and OS process identity. Restarting the manager or editing the next launch's settings cannot redirect the current run. Native file replacement, truncation or disappearance revokes readiness instead of replaying historical completion markers. Native logs remain read-only; managed console, shutdown and save evidence keep their existing records.

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
