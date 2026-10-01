# Runtime architecture / 运行架构

[English](#english) | [简体中文](#简体中文)

## English

### Ownership

| Component | Responsibility |
| --- | --- |
| `modules/*/module.toml` | Game-specific launch surface, process roles, supported queries and commands |
| `crates/app-runtime/` | Launching, process identity, lifecycle supervision, stdin and ConPTY ownership |
| `crates/app-platform-win/` | Windows process/window inspection, window suppression and host metrics |
| `crates/app-storage/` | Instance state, run history, configuration, backups and bounded log snapshots |
| `apps/desktop/src-tauri/src/commands_runtime_*.rs` | Workflows connecting process ownership, persistence and UI events |
| `apps/desktop/src-tauri/src/runtime_log_stream/` | Shared log-file identity for streaming and command-response collection |
| `apps/desktop/src/views/servers/RuntimeSurfaceWorkbench.tsx` | Console display, process selection and command results |
| `apps/desktop/src-tauri/src/lan_host.rs` | Optional authenticated LAN HTTP transport for application commands |

### Console and native windows

The console is a management surface, not a replacement implementation of Windows CMD. `managed_terminal` redirects process output to the run log; input uses stdin or a supported game protocol. `managed_pseudo_console` uses Windows ConPTY for software requiring a console session, with a separate reader converting terminal output into a bounded transcript. `managed_native_window` manages a process with a native window; discovery and hiding do not convert its GUI into text or embed its controls in React.

Launches can use a hidden desktop and creation flags. Additional suppression targets windows belonging to verified process identities. Creation time and executable path establish identity: a reused PID does not prove ownership. The supervisor also verifies identity after a launcher hands off to a workload.

Background ConPTY launches retain a private desktop as well: the character console is hosted by ConPTY, while native windows created by the same workload stay on its private desktop. The command field resolves the module's declared stdin, Source RCON, WebSocket RCON or Telnet channel, checks its existing settings and displays returned server text. Ambiguous channels are rejected; action-only protocols are not treated as arbitrary text consoles. A confirmed stdin write is reported separately from a native response.

The workbench preserves up to 400 history lines across health polls; each poll refreshes the bounded document rather than replacing it with the 32-line overview. Non-ARK primary consoles follow the selected game log; shard tabs stay pinned to their run logs. ARK maps default to their latest run's native game log and also offer retained console output. Native timestamps are checked against the captured process creation time, so an unchanged previous file cannot supply the new run's output. Older runs only expose their retained console source. ARK native streams publish bounded documents when file metadata and document contents change, with one final read after the owning console producer finishes; attaching a map viewer does not retire another map's final drain. Source changes, event resets and explicit retries reload retained output. During startup, shard events form a separate bounded transcript. Once a run is published, console events request a same-source 400-line file refresh instead of appending unpositioned deltas to a snapshot. Each selected console has one active read and at most one coalesced follow-up; disposal prevents an old scope from publishing into a new selection. Native repeated lines remain unchanged. Stream errors use a separate field and remain visible independently of file content. If file reads fail, the existing tail and command input remain available with an explicit error; successful recovery reloads retained output. This does not provide byte-level snapshot-to-delta continuity or live new-line display while the authoritative read is failing.

Ctrl+C delivery first attaches to the tracked launcher's console. If `AttachConsole` reports that it has no console, the runtime searches its identity-verified descendants. Before signalling, it checks that every console member belongs to the managed process tree. Shared external consoles are rejected; errors after signalling never trigger another broadcast. Failures identify the native operation and retain its Windows error code.

For elevated launches, the authenticated parent sends an argument-free interrupt request to the already approved owner helper. The helper targets only the workload identity it captured at creation and applies the same console membership checks under its elevated token. Normal control is distinct from owner cleanup: native interrupt errors and failed reply delivery retain the Job. A timed-out reply preserves its partial frame, and resolving that pending receipt does not broadcast again. A successful interrupt receipt confirms delivery; process-tree completion still waits for both the root and its descendants.

GUI servers can explicitly declare `window_close` with `WM_CLOSE` (Core Keeper). A dedicated short-lived thread enters the owned private desktop, enumerates application windows, verifies pinned process creation/image identities and window owners, then queues only `WM_CLOSE` and restores its desktop. IME helpers, console surfaces and foreign windows are excluded. Message delivery is not exit confirmation. Every normal-stop transport must confirm that the complete owned process tree has exited before finalization. A missing shutdown strategy, timeout or unknown tree state returns an error while retaining ownership. Natural-exit reaping also retains surviving descendants until they exit; explicit failed-start cleanup and the final application-exit watchdog keep their separate recovery rules. See [WM_CLOSE](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-close) and [EnumDesktopWindows](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-enumdesktopwindows).

Manager-created command shells resolve `cmd.exe` from the Windows system directory, including elevated launches. They do not trust `COMSPEC`, the working directory or `PATH` to select the interpreter. Batch launches disable registry AutoRun commands with `/D` and reject shell control characters in the entry and arguments; native executable arguments remain direct process arguments.

Window inspection retains handles to identity-checked processes and rejects descendants older than their parent or processes created after the tree snapshot. Hiding uses asynchronous window requests, so an unresponsive server window cannot block the inspector; subsequent enumeration determines the observed hidden count.

These mechanisms cover declared integrations, not every window arbitrary third-party software can create. Elevated prompts, services, separate desktops/sessions and programs creating their own consoles need game-specific verification. Windows limits [`CREATE_NO_WINDOW`](https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags) to compatible console launches.

Windows launches own an anonymous [Job Object](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects), assigned atomically through [`PROC_THREAD_ATTRIBUTE_JOB_LIST`](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute) before child code runs. The owner survives launcher handoff, disallows breakaway, and terminates remaining members when its last handle closes. Explicit cleanup waits for verified process handles and empty Job accounting within a two-second budget; failure preserves ownership for retry. The creation attribute requires Windows 10 or later; an unsupported or incompatible Job assignment fails the launch. Elevated launches (currently SCUM) use the same executable as a small UAC-approved helper that owns the game's Job. Its one-launch local pipe verifies the peer account, executable and parent creation time; it duplicates only already-open output handles and reaps the tree on parent death or disconnect. This does not cover independently brokered Windows services. Headless lifecycle tests exercise this helper without UAC; they do not certify the real elevation prompt or a particular installed game.

The data path is **process output → file/transcript → bounded reader → serialized event or HTTP response → React text**. It includes decoding, allocation, serialization and rendering, so it is not end-to-end zero-copy. Desktop streaming polls every 250 ms with a 256 KiB read budget and 64 KiB pending-line budget. The workbench retains a bounded tail. Native windows are inspected separately; a successful command write does not prove the game completed the action or saved its world.

Explicit tray exit is final and coalesces repeated clicks without resetting its deadline. A hidden native helper captures the identity-verified original service handle and arms the first click's boot-relative deadline before acknowledging ownership. The interface exits after that acknowledgment, without waiting for IPC or game saves; an independent 500 ms cutoff also covers a stalled UI event loop. The helper submits shutdown over the dedicated current-user control pipe and monitors the original service independently. The service acknowledges acceptance after arming its own deadline and starting owned shutdown work. Manual stop and exit-triggered stop share one save/stop implementation and the instance's automatic backup policy; exit does not dispatch a separate early save. Backend saving and cleanup share a 120-second budget. Native watchdogs enforce it even if IPC, async execution, worker joins or locks stall; expiration terminates the original runtime and confirms its exit for at most two seconds. If helper handoff fails, the interface is hidden and its original watchdog retains cleanup ownership. An elevated helper independently observes parent death and applies bounded Job cleanup; UI disappearance alone is not confirmation that elevated descendants have exited. Unsaved progress can be lost at the cutoff. Errors remain diagnostics and never reopen work admission or cancel final exit; application updates still require completed shutdown and retain save-failure abort behavior. Module delays end early only when the complete owned process tree has exited. A joined, panicked LAN discovery worker preserves its original cause without vetoing completed server shutdown.

Final exit requests each tracked run's declared save action or first graceful-shutdown command while admitted storage operations drain. Only command delivery consumes the four dispatch slots; game grace periods and backups cannot hold up later instances' first save attempt. Run identities are checked before delivery, and pending native writes remain owned until completion. Delivery is not evidence that a game saved successfully. The subsequent normal shutdown still performs the full game-specific sequence, verifies process termination and creates backups where supported.

ConPTY output is drained independently of its input/control lifetime. [`ClosePseudoConsole`](https://learn.microsoft.com/en-us/windows/console/closepseudoconsole) has different completion behavior before and after Windows 11 24H2; its return does not always mean final output has arrived. EOF preserves the final unterminated transcript line. File readers identify replacements by the open file's identity, rather than assuming a longer file is the same log. Managed streaming uses cumulative offsets and retained open segments to finish old tails across rotation. Expired segments produce an explicit gap notice and discard unfinished lines. Player-response collection rejects replacement instead of accepting output from another generation.

### WebView and LAN HTTP

The desktop uses one Tauri `main` WebView with WebView2. Local capabilities and CSP define the native API and content boundary. Remote pages do not receive the main window's native capability set. External links use the shared HTTP(S) URL boundary. This is a web-based application UI, not an embedded general-purpose browser. See [Tauri capabilities](https://v2.tauri.app/security/capabilities/).

External links are validated in both the frontend and native boundary, rejecting credentials, control characters and ambiguous URL spellings. Windows dispatch uses `ShellExecuteW` on a blocking worker with one in-flight slot, without searching for a helper executable. Completion means the operating system accepted the link, not that the destination loaded successfully.

On Windows, production recovery observes WebView2 `ProcessFailed` events. Renderer exits or unresponsiveness request a reload; a failed or timed-out reload upgrades the next attempt to recreation. A browser-process exit recreates the `main` window. Recovery replaces only the frontend window and does not restart the Rust backend or managed game servers. Native callbacks report failures to one owned recovery worker, and generation tickets prevent stale completions from overriding a replacement.

Automatic recovery allows at most three attempts, with 1, 2 and 4 second backoffs. Duplicate pending failures coalesce, and a browser failure can upgrade a queued reload without advancing its deadline. Success does not reset the budget: the next failure reopens it only after ten continuous minutes without a recovery-relevant failure. Exhaustion pauses automatic recovery; the tray's manual retry explicitly opens a new budget. Application shutdown cancels recovery. The separate [real desktop host acceptance](runtime-reliability.md#real-desktop-host-acceptance) checks this boundary with an isolated WebView2 fixture; its commands and reports are distinct from the browser component tests.

LAN management is enabled explicitly with `--lan-host`. It listens on all IPv4 interfaces, defaults to port 9088, and requires a 32-byte unpadded base64url `LANGAME_LAN_TOKEN`. API requests present it in `X-LanGame-Token`. The browser reads it from the URL fragment, removes the fragment and keeps it in session storage. Possession grants management access; read-only LAN Directory discovery is a separate component.

Production HTTP(S) pages always use the management API, including on localhost. Missing credentials, malformed responses and network errors remain failures; they cannot substitute preview servers or retry a mutation. Synthetic data is restricted to local development previews. The browser refuses API redirects and omits cookies. Management JSON responses prohibit caching. LAN pages enforce a content policy that restricts scripts to the same origin; both desktop and LAN policies allow the blob workers used for HLS decoding without allowing inline scripts.

The LAN host uses four workers and a 16-connection waiting queue, rejects overload, bounds request headers and bodies, and applies connection read/write deadlines. Management responses disallow framing. It does not provide TLS, per-user accounts or role separation. Limit it to a trusted network or an authenticated encrypted transport; do not expose its plain HTTP listener directly to an untrusted network.

The HTTP parser rejects duplicate headers, malformed field syntax and unsupported transfer encodings instead of choosing an ambiguous request length or authentication value. Static assets use a fixed 64 KiB transfer buffer under the connection deadline. Windows checks the opened file handle's final path against the distribution root; linked private files, path traversal and NTFS alternate streams are not served. These framing checks follow the security boundary described in [HTTP/1.1 message framing](https://www.rfc-editor.org/rfc/rfc9112.html#section-6.3).

Desktop log subscriptions are released on view disposal, including late registration. Registration failure is handled immediately and existing polling remains available. A disconnected or timed-out browser request does not establish that a server-side mutation was cancelled, so writes must not be retried automatically as if they were read-only queries.

### Long-running operation

Resource controls include bounded log reads and UI tails, per-instance dispatch ownership, finite query budgets, bounded restart policy, storage-context exclusion and managed-task shutdown. Public-media caching admits at most 64 requests, runs four workers and caps its store at 2 GiB / 4096 entries. Objects and request lifetimes also have limits.

New diagnostic logs use `logs/desktop-app/active.jsonl`, with 8 MiB segments, a 32 MiB content budget and a 64 KiB record limit. Oversized records remain valid JSON with a truncation marker. Previous `desktop-app.log` files stay read-only and available to bounded recent-log queries.

New non-elevated Windows console logs use each instance's `logs/managed-console/` directory. The storage layer owns rotation: at most four 8 MiB segments per process run, 256 MiB of retained content and 128 process logs per instance. Only recorded, identity-verified files are eligible for deletion; active segments, unknown files, previous logs, game-native logs, saves and configuration are preserved. If active output or a deletion failure prevents staying within the budget, writing fails visibly rather than bypassing the cap. Bookkeeping occupies additional bounded space. Elevated output uses the existing direct-file path and is outside this retention policy.

Each ordinary managed launch has one output reader with a fixed buffer and pipe backpressure; ConPTY feeds the same storage sink after transcript conversion. A sink failure stops persistence while the reader drains and discards further output, preventing a full stdout pipe from freezing the server. Snapshots of that run's console log report the failure; a separately selected game-native log does not describe the console sink's state. Slow or hung filesystem calls remain a synchronous I/O boundary, so the pipe-drain deadline is not an unconditional deadline for every storage driver.

Runtime reconciliation retains one pending batch of exit events until persistence and restart scheduling are acknowledged. Instance mutation locks and the storage-context lease remain with those events across caller cancellation. Reaping skips instances owned by another lifecycle operation, so it cannot take exit records away from an ongoing save-and-stop workflow. Unknown interrupted log-update files are preserved and block further publication rather than accumulating new temporary names. Open readers may delay physical reclamation after an expired segment is unlinked.

Ordinary Windows stdin writes have one two-second deadline covering the command and newline, without detached timeout threads. Stop and owner invalidation cancel the writer; an incomplete accepted command closes the shared pipe before another command can use it. Dispatch return slots follow instance ownership through a failed stop and cannot restore input into a replacement run. ConPTY input retains its two-second write budget. A caller's confirmation timeout or disconnection does not cancel the application completion observer or prove that already accepted bytes were not executed.

ConPTY observes the same dispatch cancellation signal. Its input and output failure states are independent: a failed command write closes input without discarding later crash diagnostics, and a log persistence failure does not prevent sending a stop command through otherwise healthy input.

Unsupported terminal sequences and unexpected pipe failures stop transcript interpretation. A rejected sequence or incomplete final sequence preserves the already decoded text, including complete lines from the same read and the known prefix of an unfinished line. When the log sink remains writable, the run log records that capture stopped instead of presenting missing diagnostics as an idle console. Final transcript completion flushes the sink and retains any failure state. This conservative parser boundary does not provide a complete terminal emulator.

Host-monitor queries and firewall commands use the runtime's private Job ownership and bounded pipe capture. PowerShell, registry and network utilities resolve through the Windows system directory, with no current-directory executable search. Queries have a 15-second execution-and-drain budget, firewall commands have 30 seconds, and each captured stream is limited to 4 MiB; cleanup has a separate two-second budget. There are no detached timeout-reader threads. Hardware discovery starts in the existing blocking capture worker rather than during desktop state construction, and adapter-rate history retains only the current adapter inventory. These are per-command limits; a metrics snapshot can include several sequential queries. A firewall timeout does not roll back rules already applied, so callers must not treat it as proof that no mutation occurred.

SteamCMD command capture retains verified Job member handles while it observes the running tree. Normal completion checks both empty Job accounting and the exit signals of observed members. Timeout or cancellation cleanup captures the remaining members before termination and applies a separate five-second total cleanup budget. Process observation is capped at 4096 retained handles; an incomplete snapshot or failed cleanup is an error, not confirmed completion. This is not a complete history of every short-lived descendant.

SteamCMD content-log diagnostics asynchronously read at most the final 64 KiB from a file-length snapshot, retain at most 40 lines, and mark byte truncation. Concurrent appends cannot extend the read indefinitely. Both the pre-command and post-command excerpts share the existing installation deadline; previous multi-year logs are never loaded in full just to display their tail.

Deterministic tests establish specific lifecycle and failure invariants. They cannot prove indefinite uptime, every game's launcher behavior, or resilience to every WebView/driver/disk fault. Release qualification should include sustained output, repeated start/stop, exhausted or slow I/O, network interruption, sleep/resume and actual supported server versions. Record measurements and environment; a source review or short test is not long-duration stability evidence.

## 简体中文

### 职责与边界

游戏 `module.toml` 声明启动方式、进程角色及支持的查询和命令。`app-runtime` 管理进程身份、生命周期、stdin 与 ConPTY；`app-platform-win` 负责 Windows 进程、窗口和主机指标；`app-storage` 负责实例状态、运行记录、配置、备份与有界日志快照。桌面命令层连接这些能力，React 工作区显示结果。可选 LAN HTTP 服务调用同一应用命令，不另建业务规则。

### 控制台与原生窗口

内置控制台是服务器管理界面，不是重新实现的 Windows CMD：

- `managed_terminal` 将进程输出重定向至运行日志，输入使用 stdin 或游戏支持的管理协议。
- `managed_pseudo_console` 为依赖控制台会话的程序提供 ConPTY，通过独立读取器解析输出并写入运行日志。
- `managed_native_window` 管理带原生窗口的进程。发现和隐藏窗口不会把 GUI 转为文本流，也不会把原生控件嵌入 React 页面。

启动路径可以使用隐藏桌面和进程创建标志；补充窗口抑制只作用于身份已核验的进程。身份包含创建时间和可执行文件路径，PID 相同不等于原进程仍存在。启动器移交给实际服务器进程后，监督器仍核验记录的身份。

后台 ConPTY 启动同样持有私有桌面：字符控制台由 ConPTY 承载，同一进程创建的原生窗口留在私有桌面。命令框根据模块声明选择 stdin、Source RCON、WebSocket RCON 或 Telnet，核对现有配置并显示服务器返回文本；通道有歧义时拒绝盲选，专用动作协议不作为任意文本控制台。stdin 写入确认与服务器原生响应分别呈现。

工作区在健康轮询之间保留最多 400 行历史，每次轮询刷新有界日志文档，不用 32 行概览覆盖历史。非 ARK 主控制台跟随游戏日志，分片标签固定读取对应运行日志。ARK 地图默认显示其最新运行的游戏日志，同时保留控制台输出选择；原生时间戳须符合已捕获的进程创建时间，未更新的旧文件不能冒充本轮输出。历史运行只提供其保留的控制台来源。ARK 原生日志流只在文件元数据及文档内容变化时发送有界文档，随所属控制台生产者结束执行一次最终读取；挂接地图查看器不会取消其他地图尚未完成的尾部读取。来源切换、事件重置和主动重试会重新读取已保留输出。启动期间，各分片事件形成独立的有界转录。运行登记后，控制台事件触发同来源的 400 行文件刷新，不再把位置未知的增量追加到快照。每个选中控制台最多一个进行中的读取和一个合并待办；切换或卸载后，旧读取不能更新新选择。游戏原生重复行保持原样。流错误通过独立字段保留，不混入文件正文。文件读取失败时保留已有尾部和命令输入并明确报错，恢复成功后重新载入保留输出；这不提供快照到增量的字节级连续性保证，也不保证权威读取失败期间仍实时显示新行。

投递 Ctrl+C 时先连接受管启动器的控制台；仅当 `AttachConsole` 明确报告目标没有控制台时，才搜索经过身份核验的子进程。发送前确认控制台所有成员都属于受管进程树，拒绝与外部进程共享的控制台；发送后的错误不会触发再次广播。失败信息保留具体原生操作和 Windows 错误码。

GUI 服务器可显式声明 `window_close` 和 `WM_CLOSE`（Core Keeper）。独立短线程进入所持有的私有桌面，枚举应用窗口、核验固定进程句柄对应的创建时间与映像身份及窗口归属，只投递 `WM_CLOSE`，然后恢复线程桌面。IME 辅助窗口、控制台窗口和外部进程窗口不会收到消息。投递成功不代表已退出。所有正常停止通道都必须确认完整受管进程树已退出，才进入收尾；缺少停止策略、超时或进程树状态未知时均保留所有权并报错。自然退出回收同样等待存活后代自行退出；失败启动清理和应用最终退出 watchdog 保留各自独立的恢复规则。参见 [WM_CLOSE](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-close) 和 [EnumDesktopWindows](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-enumdesktopwindows)。

管理器创建的命令 shell（包括提权启动）从 Windows 系统目录确定 `cmd.exe`，不使用 `COMSPEC`、工作目录或 `PATH` 选择解释器。批处理启动通过 `/D` 禁用注册表 AutoRun 命令，并拒绝入口和参数中的 shell 控制字符；原生可执行文件的参数仍直接传给进程。

窗口检查持续持有核验后的进程句柄，拒绝早于父进程创建的后代和树快照之后才创建的进程。隐藏使用异步窗口请求，无响应的服务器窗口不会阻塞检查线程；后续枚举确认实际隐藏数量。

这些机制围绕已声明的游戏接入契约实现。提权提示、服务、独立桌面或会话、自行创建控制台的程序仍需逐游戏验证。Windows 的 [`CREATE_NO_WINDOW`](https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags) 有明确适用限制。

Windows 启动通过 [`PROC_THREAD_ATTRIBUTE_JOB_LIST`](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute) 在创建进程时原子加入独立、匿名的 [Job Object](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)。启动器移交后仍保留容器所有权，不允许成员脱离，最后一个容器句柄关闭时终止残余进程。显式清理在两秒预算内等待已核验的进程句柄退出和 Job 活跃数归零，失败保留所有权供重试。该创建属性要求 Windows 10 及以上，不支持或不兼容的容器分配会明确启动失败。提权启动（目前为 SCUM）由同一可执行文件中的专用助手复用 UAC 授权并持有游戏 Job；单次启动的本机管道核验对端账号、程序路径及父进程创建时间，仅复制父进程已打开的输出句柄，并在父进程死亡或连接断开时清理进程树。独立代理的 Windows 服务不在此保证内。无头生命周期测试不触发 UAC，不能替代真实提权提示或具体已安装游戏的验收。

实际数据流为：**进程输出 → 日志文件或终端转录 → 有界读取 → 序列化事件或 HTTP 响应 → React 文本**。其中存在解码、分配、序列化和渲染，不能称为全链路零拷贝。桌面日志流每 250 毫秒读取一次，单次最多 256 KiB，未完成行最多保留 64 KiB；界面只保留有限尾部。窗口检测是另一条路径，命令写入成功也不代表游戏已执行完成或保存到磁盘。

托盘显式退出是不可撤销的最终请求，重复点击合并且不重置倒计时。隐藏原生助手先捕获经过身份核验的原服务句柄，布防首次点击对应的系统启动相对截止点，再确认接管责任。界面收到接管回执即退出，不等待 IPC 或游戏保存；独立的 500 毫秒退出兜底覆盖界面事件循环卡住的情况。助手通过当前用户的独立控制管道提交停服，并独立监护原后台。后台布防自身截止并启动有所有者的停服任务后立即回复已接收。手动停止与退出触发的停止共用一次保存、停服流程及实例自动备份策略，退出不额外发送提前保存。后台保存和清理共用 120 秒预算，原生监护保证 IPC、异步执行器、线程 join 或业务锁卡住时仍执行截止；到期终止原后台，再用最多两秒确认其退出。助手交接失败时隐藏界面，由原界面监护继续持有清理责任。提权助手独立监视父进程死亡并执行有界 Job 清理；界面消失不能作为管理员后代进程已结束的确认。截止时未保存的进度可能丢失。错误保留为诊断，不能重新开放工作准入或取消最终退出；应用更新仍须确认停服完成，保存失败会中止更新退出。只有确认完整受管进程树已结束时，才提前结束模块剩余等待。已完成 join 的局域网发现线程发生 panic 时保留原始原因，不阻止已完成停服后的退出。

最终退出在已接纳的存储操作收尾期间，就向各受管运行会话投递模块声明的保存动作或首条正常关闭命令。四个并发槽仅用于命令投递，服务器宽限等待和备份不会占住后续实例首次保存的机会。投递前核验运行身份，尚未完成的原生输入仍由工作任务持有。投递成功不代表游戏已成功存档；之后的正常退出仍执行完整游戏关闭顺序、核验进程结束，并在支持时创建备份。

ConPTY 输出读取与输入、关闭控制分别管理。[`ClosePseudoConsole`](https://learn.microsoft.com/en-us/windows/console/closepseudoconsole) 在 Windows 11 24H2 前后的完成语义不同，函数返回不总是代表尾部输出已到达。EOF 收尾保留最后一条没有换行符的转录内容。日志读取器以打开文件的身份识别更替，不根据文件变长就假定仍是旧文件。受管日志流使用累计字节位置与已打开的历史片段，在轮转后接续尾部；片段已被保留策略淘汰时显示缺口提示并丢弃未完成行。玩家响应采集遇到日志更替会拒绝结果，避免混用不同文件代次的响应。

### 桌面 WebView 与 LAN HTTP

桌面使用 Tauri 的单个 `main` WebView，底层为 WebView2。CSP 与本地 capability 配置限制页面内容和原生接口；远程页面没有主窗口的原生权限集合，外链通过统一 HTTP(S) URL 边界。这是基于 Web 的应用界面，不是通用内置浏览器。权限模型见 [Tauri capabilities](https://v2.tauri.app/security/capabilities/)。

前端和原生边界都会校验外链，拒绝凭据、控制字符和存在解析歧义的 URL。Windows 通过独立阻塞工作线程调用 `ShellExecuteW`，最多保留一个在途请求，不搜索外部辅助程序。调用成功仅表示操作系统接受链接，不代表目标网页加载成功。

Windows 生产路径监控 WebView2 `ProcessFailed` 事件。Renderer 退出或无响应时先重新加载，加载失败或超时后将下一次尝试升级为重建；Browser 进程退出时重建 `main` 窗口。恢复只替换前端窗口，不重启 Rust 后端或受管游戏服务器。原生回调将故障交给单个受管恢复任务，代际票据防止旧完成事件覆盖替换后的窗口。

自动恢复最多尝试 3 次，分别退避 1、2、4 秒。同一待处理故障合并，Browser 故障可以升级已排队的加载操作，但不提前期限。恢复成功不会立即重置预算；只有连续 10 分钟未出现需要恢复的故障后，下一次故障才能重开预算。耗尽后暂停自动恢复，托盘中的手动重试可显式重开预算；应用退出会取消恢复。独立的[真实桌面宿主验收](runtime-reliability.md#真实桌面宿主验收)使用隔离的 WebView2 夹具检查此边界，其执行命令和报告与浏览器组件测试分开。

LAN 管理需通过 `--lan-host` 显式启用，监听全部 IPv4 网卡，默认端口为 9088。启动必须提供 32 字节、无 padding 的 base64url `LANGAME_LAN_TOKEN`，管理请求使用 `X-LanGame-Token` 请求头。浏览器读取 URL fragment 中的令牌后移除 fragment，并存入会话存储。该令牌具有管理权限，与只读局域网目录发现协议分离。

正式 HTTP(S) 页面始终调用管理 API，包括 localhost。缺少凭据、响应格式错误或网络故障均返回失败，不会代入模拟服务器或重试写操作；模拟数据仅限本地开发预览。浏览器拒绝管理请求重定向，不附带 Cookie，管理 JSON 响应禁止缓存。LAN 页面通过内容策略将脚本限定在同源；桌面和 LAN 策略均允许 HLS 解码所需的 blob worker，不放开内联脚本。

LAN 服务有 4 个工作线程和 16 个排队连接，拒绝超载，限制请求头、正文大小及连接读写时间，管理响应禁止被其他页面嵌入。它没有 TLS、独立账号或角色权限，应限定在可信网络或具备身份认证的加密传输之后，不应将明文 HTTP 监听直接开放给不可信网络。

HTTP 解析器拒绝重复请求头、畸形字段及未支持的传输编码，避免对正文长度或身份凭据作歧义解释。静态资源使用固定 64 KiB 缓冲传输，共享连接时限；Windows 根据已打开文件句柄的最终路径核对分发根目录，拒绝链接到目录外的私有文件、路径穿越及 NTFS 备用数据流。报文边界依据 [HTTP/1.1 消息分帧规范](https://www.rfc-editor.org/rfc/rfc9112.html#section-6.3)。

桌面日志订阅随视图销毁释放，注册晚于卸载完成时也会清理。订阅失败立即处理，既有日志轮询仍可使用。浏览器断开或等待超时不代表后端写操作已取消，不能把写操作当作查询自动重试。

### 长时间运行

已有约束包括日志读取与界面尾部上限、每实例命令派发所有权、协议查询时间预算、有界重启策略、存储上下文互斥及受管任务关停清理。公共媒体缓存最多接纳 64 个请求、同时执行 4 个，磁盘上限为 2 GiB / 4096 项，并限制单对象大小和请求时长。

新应用诊断日志写入 `logs/desktop-app/active.jsonl`，单片段 8 MiB、日志内容总量 32 MiB、单记录最多 64 KiB；过大记录保留有效 JSON 并标记截断。旧 `desktop-app.log` 只读保留，仍参与有界的近期日志查询。

新建的非提权 Windows 控制台日志写入各实例的 `logs/managed-console/`：每份进程运行日志最多 4 个 8 MiB 片段，每实例最多保留 256 MiB 内容和 128 份进程运行日志。仅清理拥有记录中登记且文件身份匹配的片段；活动片段、未知文件、既有日志、游戏原生日志、存档与配置均保留。若活动输出或删除失败导致无法满足预算，明确报告写入失败，不绕过上限继续追加。拥有记录等元数据另占有限空间。提权输出继续使用原直接文件路径，不纳入此轮转策略。

普通受管启动各有一个固定缓冲区的输出读取线程，通过管道背压限制堆积；ConPTY 转录后使用同一存储写入器。存储失败后停止持久化，继续排空并丢弃后续输出，避免 stdout 满管道卡住服务器。对应运行的控制台日志快照报告失败；单独选择的游戏原生日志不代表控制台写入器的状态。文件系统调用仍是同步 I/O 边界，管道排空期限不等于对任意磁盘驱动都成立的硬性关闭期限。

运行状态协调最多保留一批待确认退出事件，直到持久化与重启调度完成确认；调用者取消后，实例修改锁和存储上下文租约仍随事件保留。进程回收跳过已由其他生命周期操作持有的实例，避免抢走保存停服流程需要的退出记录。日志更新中断后遗留的未知临时文件保留并阻止后续发布，不反复生成新名字。过期片段取消文件名后，尚未关闭的读取句柄可能延迟物理空间回收。

普通 Windows stdin 将正文和换行符合计限制在两秒写入预算内，不创建脱离管理的超时线程；实例停止或所有者失效会取消写入，已部分接受的命令失败后关闭共享管道，防止与后续命令拼接。输入归还槽跟随实例所有权，在停止失败时可恢复，不能归还到替换后的运行。ConPTY 输入维持两秒写入预算。调用方确认超时或断开不会取消应用层完成观察，也不能证明已经写入的字节未被执行。

ConPTY 同样响应派发取消信号，输入与输出故障状态分别管理：命令写入失败会关闭输入，但不再丢弃后续崩溃诊断；日志持久化失败也不会阻止通过仍正常的输入发送停服命令。

不支持的终端控制序列和异常管道错误会停止转录解析。遇到被拒绝的序列或末尾不完整序列时，保留已解码文本，包括同次读取中的完整行和未完成行的已知前缀。日志写入器仍可用时，运行日志会明确记录采集已停止，避免把缺失的诊断误当作控制台空闲。最终转录会刷新写入器并保留失败状态；此保守解析边界不等同于完整的终端模拟器。

主机监控查询和防火墙命令复用运行时的私有 Job 所有权与有界管道采集。PowerShell、注册表和网络工具从 Windows 系统目录解析，不在当前目录搜索同名程序。查询的执行与输出排空共用 15 秒预算，防火墙命令为 30 秒，每条输出流最多 4 MiB，清理另有两秒预算，不创建脱离管理的超时读取线程。硬件发现延后到已有后台阻塞采集任务，不在桌面状态构造时执行；网卡速率历史仅保留当前网卡清单。这些是每条命令的限制，一次指标快照可能包含多条顺序查询。防火墙超时不会回滚已应用规则，调用方不能据此认定没有发生修改。

SteamCMD 命令采集在观察运行中的进程树时保留已核验的 Job 成员句柄；正常结束同时检查 Job 活跃数归零与已观察成员的退出信号。超时或取消清理在终止前捕获剩余成员，并采用独立的五秒总清理预算。最多保留 4096 个观察句柄，快照不完整或清理失败会明确报错，不视为完成；这不等于记录了每个短命后代的完整历史。

SteamCMD 内容日志诊断按文件长度快照异步读取末尾最多 64 KiB，保留最多 40 行并标记字节截断。并发追加不会无限延长本次读取；命令前后的摘录均受已有安装总期限约束，不再为显示尾部而全量载入长期积累的历史日志。

确定性测试能验证具体不变量，不能证明无限运行、所有游戏启动器或任意 WebView、驱动与磁盘故障下都稳定。发行验收仍应覆盖持续输出、反复启停、慢速或耗尽的 I/O、断网、休眠恢复及实际支持的服务器版本，并记录环境和指标，不能把源码审查或短时测试写成长期稳定性保证。
