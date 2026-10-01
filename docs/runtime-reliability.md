# Runtime reliability acceptance / 运行可靠性验收

[English](#english) | [简体中文](#简体中文)

## English

This suite exercises the production runtime with synthetic Windows processes,
temporary files and loopback peers. It does not install games, start configured
instances, use player data, or certify third-party server versions. See
[runtime architecture](runtime-architecture.md) for the supported ownership and
window-management boundaries.

### Run the suite

Prepare the locked dependencies and build the frontend as described in the
[development guide](development.md). Windows and an installed Chrome or Edge are
required for the complete suite. `LANGAME_RELIABILITY_BROWSER` can select an
explicit browser executable; missing browser support fails rather than skips.
Use a new evidence directory outside the repository:

```powershell
python -B scripts/verify_runtime_reliability.py --plan
python -B scripts/verify_runtime_reliability.py --output ../runtime-reliability-results
```

The default process measurement uses 8 warmup cycles followed by 24 measured
cycles. Each cycle launches four processes: ordinary-pipe and ConPTY hosts each
exercise stop and subsequent launch/abnormal exit. `--cycles` accepts 4 through
256; it changes the finite sample size, not the supported uptime claim.

To use a separate frontend build, pass `--frontend-dist` with its absolute directory outside
the repository containing `index.html`. The evidence directory must be separate
from that distribution. The runner merges this location into the Rust child's
`TAURI_CONFIG`, preserving other overrides; without the option, the existing
configuration is inherited.

CI uses `--full-workspace`, which runs the complete Rust workspace baseline once
and then the Web checks. Ordinary scoped runs select the affected runtime,
storage and desktop tests. The runner never invokes ignored helper probes on
their own; their parent tests launch them with exact selectors and own their
process lifetimes.

### Acceptance boundaries

| Area | Exercised behavior | Required result |
| --- | --- | --- |
| Synthetic servers | Sustained unbroken output, Unicode commands, unread stdin, abrupt exit, launcher handoff, ordinary pipes and ConPTY | Output progress and exit tails arrive; commands preserve order; stalled input times out; descendants remain owned and are reaped |
| Repeated lifecycle | Stop, release and relaunch after warmup in an isolated sampler process | Exact output-reader thread handles and retained process handles signal exit, sinks are destroyed, and process handle counts stay within the observed warmup bound |
| Storage faults | Conflicting file handles, each rotation journal rename boundary, interrupted deletion acknowledgement, failed writes | Recovery preserves ordered retained bytes and unknown files; retries do not duplicate publication or grow abandoned filenames |
| Network faults | Partial/invalid requests, disconnects, read/write deadlines and a full worker queue | The same listener serves subsequent valid requests; active workers return to zero; overload is explicit |
| Time and cancellation | Exact expiry/deadline boundaries, multi-year clock advances, old restart tickets, duplicate exits, refresh cancellation | No early/repeated restart, configured restart limits remain effective, and refresh reservations are released |
| Web lifecycle | Repeated console subscriptions, navigation, late completion and sustained log bursts | Listeners are released, retired views cannot update replacements, and the shared display tail stays bounded |

The process report records every warmup and measured sample of handles, threads
and private committed memory. Thread counts and private memory are diagnostic
observations: Windows worker pools and allocators can change these values after
startup. They are not silently converted into a claim of zero memory leakage.
The suite checks application-owned resource release and bounded output separately.
ConPTY is assessed as terminal text; ordinary pipes additionally verify exact
payload counts.

The loopback scenarios use a real listener and socket I/O, with controlled
handlers instead of application commands that could affect configured servers.
Write-deadline tests inject a stalled writer after partial real socket output;
they do not infer a blocked connection merely from the OS socket-buffer setting.
Fault injection supplies failure conditions; it does not fill the workstation's
disk or exhaust global memory.

The Web stage also starts an isolated headless browser and a temporary Vite
server. It renders the actual console component with ReactDOM and StrictMode,
changes instances and log paths on the same root, and verifies DOM/listener
cleanup after unmount. Only the native event bridge is simulated. The browser
uses a separate disposable profile; no existing browser session is accessed.
These checks cover React behavior in Chromium, not WebView2's internal heap.

### Real desktop host acceptance

An additional Windows host fixture uses the actual Tauri/WebView2 host, native
IPC and events, the console component and runtime log stream. It injects renderer
and browser crashes through the fixture's own WebView2, requires native
`ProcessFailed` observations, and checks recovery plus ordered commands before
and after each fault while the same synthetic server remains alive. It also
checks the bounded DOM tail, unmount cleanup and owned process/output-worker exit.
The fixture uses separate temporary data, logs and a verified WebView2 profile;
it does not open existing user data or configured game instances.

The `desktop-reliability` Cargo feature is off by default. It enables an isolated
acceptance entry point; production WebView2 recovery does not require that feature.
After preparing dependencies and frontend assets, portable Windows PowerShell
commands from the repository root are:

```powershell
cargo build -p langame-desktop --features desktop-reliability --locked
if ($LASTEXITCODE -ne 0) { throw 'Desktop reliability build failed' }
$desktopExe = (Resolve-Path 'target/debug/langame-desktop.exe').Path
$desktopReport = Join-Path (Resolve-Path '..').Path 'desktop-reliability-report.json'
node apps/desktop/scripts/verify_desktop_reliability.cjs --executable $desktopExe --output $desktopReport
```

Use Cargo's actual output path if it differs from this portable default. Both
arguments must be absolute paths, and the report must be a new file outside the
repository. The general invocation is
`node apps/desktop/scripts/verify_desktop_reliability.cjs --executable ABS_EXE --output ABS_REPORT`.
Windows with WebView2 is required; missing support is a failure, not a skipped pass.

Keep the tested executable unchanged throughout the acceptance run.

Acceptance requires the driver's successful exit and a new report with the
executable hash, real host observations and verified cleanup. Failed scratch data
is retained for diagnosis. These instructions describe the checks, not a record
of a completed run; unit tests and upstream API documentation cannot substitute
for this report. The fixture does not measure WebView2 heap aging or certify every
native failure mode.

### Multi-instance, resource and cluster checks

The focused portable selectors are:

```powershell
cargo test -p app-runtime port_remap --locked
cargo test -p app-runtime resource --locked
cargo test -p app-storage ark_cluster --locked
```

Port checks cover a busy native peer, complete-group displacement, preserved offsets, mixed protocols and exhaustion. Resource checks include real Windows Job allocation probes sharing one instance budget; a rejected allocation must not be reported as a successfully applied policy. Cluster tests use newly created temporary data to verify membership/status rejection, checksums, path and junction boundaries, long Windows paths, protection snapshots, mid-group rollback, partial marker writes and explicit recovery after interruption. They do not restore configured worlds.

The feature-gated runtime-service fixture starts a real detached service, disconnects its first client, and reconnects an independent client to the same synthetic game run. Its entry point is `langame-desktop.exe --runtime-service-fixture ABS_CONFIG` after a `desktop-reliability` build. The JSON configuration contains an absolute local `root`, a unique `nonce`, and an optional `scenario` (`normal`, `tray_exit`, or `tray_exit_hang_save`; default `normal`). Keep the configuration beside the new root, with the root strictly named `%TEMP%\langame-runtime-service-<id>\fixture`; for example, the configuration can be `%TEMP%\langame-runtime-service-<id>\config.json`. Existing roots and reparse paths are rejected. Keep all input and reports outside the repository.

The `normal` scenario verifies log and command continuity, managed save/stop, persisted stopped state and owned-process exit. Its first client uses an isolated `about:blank` WebView and the production tray and close handler to verify close-to-tray, a retained connection to the same runtime, and restoration through the production show handler. The `tray_exit` and `tray_exit_hang_save` scenarios instead use headless clients and invoke the production tray-exit handler twice. An independent fixture owner verifies that the actual client, original service and synthetic game processes have exited before cleanup can run. `tray_exit` verifies the received save command, rereads the persisted world, and checks stopped state and backup creation. `tray_exit_hang_save` verifies receipt of the save request followed by forced exit at the deadline; it rejects fabricated save, stopped or backup results.

Only the dedicated, namespace-verified fixture can install the shorter tray-exit deadlines: 30 seconds for `tray_exit` and 4 seconds for `tray_exit_hang_save`. Production retains its single 120-second budget. Reports record elapsed time separately from the allowed 2-second native exit confirmation and 1-second observer allowance. The synthetic service fixture does not modify Windows Firewall. Its feature-gated, fixture-owned capability verifies the disposable storage namespace, loopback-only port and synthetic executable before treating firewall configuration as an external test boundary. The production start path and post-launch listener verification remain active. These checks do not establish survival across Windows sign-out/restart or certify native ARK transfers. Reports exclude native firewall configuration, tray-menu click automation and full application rendering.

### Sample a running process

For a lightweight CPU, memory, handle and thread baseline, use
[`collect-runtime-baseline.ps1`](../scripts/perf/collect-runtime-baseline.ps1).
It samples processes by name without starting or stopping them. Provide an
explicit output path outside the repository:

```powershell
$baselinePath = Join-Path $env:TEMP ("langame-runtime-{0}.jsonl" -f [guid]::NewGuid())
.\scripts\perf\collect-runtime-baseline.ps1 -ProcessName langame-desktop -OutputPath $baselinePath
```

The default sample lasts 300 seconds at two-second intervals. The JSONL file is
local diagnostic output; it is not a source artifact or a reliability acceptance
report. Review its contents before sharing it.

### Evidence and failure handling

The output directory contains `report.json` and one log per stage. The report
includes actual exit codes, passed/failed/ignored counts, required test-group
coverage, resource measurements, the Git revision and source-snapshot receipts
when available. It is local diagnostic evidence and is not automatically
published.

A nonzero process exit, zero tests, absent required tests, missing resource
measurements or a failed assertion fails acceptance. Previously written evidence
is never overwritten. A failed stage stops subsequent stages; inspect its log
and rerun into a new directory after fixing the cause. Helper probes reported
as ignored in the outer test listing are exercised by normal parent tests;
other environment-dependent ignored tests remain outside the reported coverage.

Tests use bounded waits and own their temporary processes, listeners and files.
CI's job timeout or the managed workstation's existing process guard owns the
outer execution deadline. Slow storage drivers, operating-system faults,
WebView internals and game-specific behavior still require separate evidence.
Accelerating scheduler time does not accelerate physical memory aging or prove
indefinite uptime.

## 简体中文

这组验收使用真实 Windows 进程、临时文件和本地回环连接，调用实际运行时实现；
不安装游戏、不启动已配置实例、不读取玩家数据，也不据此认证第三方服务器版本。
进程归属和窗口管理范围见[运行架构](runtime-architecture.md)。

### 执行方式

按[开发指南](development.md#简体中文)准备锁定依赖并构建前端。完整验收需要 Windows 及
已安装的 Chrome 或 Edge；可用 `LANGAME_RELIABILITY_BROWSER` 指定浏览器程序。
缺少浏览器会失败，不通过跳过来产生假通过。
证据目录必须是仓库外尚不存在的新目录：

```powershell
python -B scripts/verify_runtime_reliability.py --plan
python -B scripts/verify_runtime_reliability.py --output ../runtime-reliability-results
```

默认先预热 8 个周期，再测量 24 个周期；每周期包含 4 次启动，分别对普通管道和
ConPTY 执行停止、重新启动及异常退出。`--cycles` 可设为 4 至 256，只改变有限采样量，
不改变可承诺的运行时长。

通过 `--frontend-dist` 可指定仓库外包含 `index.html` 的前端产物绝对路径，
验收证据目录须与该产物目录分开。运行器只为 Rust 子进程合并 `TAURI_CONFIG` 中的
前端路径并保留其他覆盖项；未指定该选项时继承已有配置。
CI 使用 `--full-workspace` 执行一次完整 Rust 基线，再执行 Web 检查。

### 检查内容与保证范围

- **模拟服务器**：持续无换行输出、中文命令、不读取输入、异常退出和启动器移交，
  验证输出进度、命令顺序、写入超时以及后代进程的归属与清理。
- **重复生命周期**：独立进程预热后反复启停，检查真实输出读取线程和持有的进程句柄已退出、写入器已析构，
  且句柄数量不超过预热范围。保留每次线程和私有提交内存采样作为诊断数据，
  不把 Windows 线程池或分配器波动直接当成泄漏，也不把诊断曲线当成无泄漏证明。
- **故障注入**：覆盖文件占用、轮转各持久化边界、删除确认中断、写入失败，
  以及请求断连、读写期限和队列过载；验证数据保留、恢复后的请求处理与资源释放。
- **可控时间**：直接推进调度时刻，覆盖到期边界、多年跨度、重启上限、旧票据失效和缓存过期，
  并检查调用者取消或工作任务失败后仍能继续刷新。周期存档沿用已有显式时间测试入口。
- **Web 生命周期**：检查反复订阅和销毁、页面切换、迟到回调及大量日志，要求监听资源释放、
  旧视图无法更新新视图，显示尾部不超过预算。

网络测试使用真实监听器和 socket，处理器使用受控夹具，不调用会修改真实实例的管理命令。
写入期限测试在少量真实 socket 输出后注入停止进展的写入器，不根据操作系统缓存大小猜测连接已阻塞。
故障通过局部注入构造，不实际填满磁盘或耗尽整机内存。
ConPTY 验证终端转录，普通管道另检查精确输出字节数量。

Web 阶段还通过独立无界面浏览器和临时 Vite 服务，使用真实 ReactDOM 与 StrictMode 渲染控制台，
在同一个 root 上切换实例及日志路径，检查卸载后的 DOM 和监听器清理。只有原生事件桥被模拟。
浏览器使用独立临时 profile，不读取已有浏览器会话。这验证 Chromium 中的 React 行为，
不代表测量了 WebView2 内部堆内存。

### 真实桌面宿主验收

独立的 Windows 宿主夹具使用真实 Tauri/WebView2、原生 IPC 和事件、控制台组件及运行时日志流。
它通过夹具自身的 WebView2 注入 Renderer 和 Browser 崩溃，要求收到原生 `ProcessFailed` 事件，
检查界面恢复及每次故障前后的命令顺序，同时要求同一个模拟服务器保持运行。
还检查 DOM 尾部上限、卸载清理以及拥有的进程和输出线程退出。
夹具使用独立临时数据、日志及经核验的 WebView2 profile，不打开既有用户数据或已配置游戏实例。

`desktop-reliability` Cargo feature 默认关闭，只启用隔离的验收入口；生产 WebView2 恢复不依赖此 feature。
准备依赖和前端产物后，在仓库根目录运行的可移植 Windows PowerShell 示例为：

```powershell
cargo build -p langame-desktop --features desktop-reliability --locked
if ($LASTEXITCODE -ne 0) { throw 'Desktop reliability build failed' }
$desktopExe = (Resolve-Path 'target/debug/langame-desktop.exe').Path
$desktopReport = Join-Path (Resolve-Path '..').Path 'desktop-reliability-report.json'
node apps/desktop/scripts/verify_desktop_reliability.cjs --executable $desktopExe --output $desktopReport
```

若 Cargo 配置了其他产物位置，应替换示例中的可执行文件路径。两个参数均须为绝对路径，
报告必须是仓库外尚不存在的新文件。通用调用形式为
`node apps/desktop/scripts/verify_desktop_reliability.cjs --executable ABS_EXE --output ABS_REPORT`。
需要具备 WebView2 的 Windows 环境，缺少支持会失败，不视为跳过后通过。

验收过程中，所用可执行文件必须保持不变。

通过证据必须包含驱动成功退出及新生成的报告，记录可执行文件哈希、真实宿主观察结果和清理结果；
失败时保留临时数据供诊断。以上是检查方法，不代表已经完成一次验收；单元测试和上游 API 文档
不能替代实际报告。此夹具不测量 WebView2 堆内存长期老化，也不认证所有原生故障类型。

### 多实例、资源与集群检查

对应的可移植定向测试命令为：

```powershell
cargo test -p app-runtime port_remap --locked
cargo test -p app-runtime resource --locked
cargo test -p app-storage ark_cluster --locked
```

端口检查覆盖原生 peer 被占用、整组迁移、偏移关系、混合协议及端口耗尽。资源检查使用真实 Windows Job 分配探针验证单实例共用预算；被拒绝的分配不能记作限制成功应用。集群测试只使用新建临时数据，验证成员和状态拒绝、校验和、路径与目录联接边界、Windows 长路径、保护快照、整组中途失败回滚、部分标记写入以及中断后的显式恢复，不恢复已配置世界。

由 feature 控制的运行服务夹具启动真实独立服务，断开首个客户端，再用另一个客户端重连同一模拟服务器会话。使用 `desktop-reliability` 构建，入口为 `langame-desktop.exe --runtime-service-fixture ABS_CONFIG`。JSON 配置包含绝对本地目录 `root`、唯一 `nonce`，以及可选的 `scenario`（`normal`、`tray_exit` 或 `tray_exit_hang_save`，默认 `normal`）。配置文件放在新根目录旁，根目录严格使用 `%TEMP%\langame-runtime-service-<id>\fixture` 命名；例如，配置文件可为 `%TEMP%\langame-runtime-service-<id>\config.json`。拒绝已有根目录及重解析路径，全部输入与报告保存在仓库外。

`normal` 场景验证日志和命令连续性、托管保存与停服、持久停止状态及所属进程退出。首个客户端使用独立 profile 的 `about:blank` WebView 和生产托盘、关闭处理器，验证关闭后隐藏、客户端保持连接同一运行服务，以及通过生产显示处理器恢复窗口。`tray_exit` 与 `tray_exit_hang_save` 场景使用无窗口客户端，并连续两次调用生产托盘退出处理器。独立夹具拥有者在清理开始前确认真实客户端、原运行服务和模拟游戏进程均已退出。`tray_exit` 验证已收到保存命令、重新读回持久化世界数据，并检查停止状态及备份生成。`tray_exit_hang_save` 验证已收到保存请求并在截止时间强退，不接受虚假的保存、停止或备份结果。

只有通过专用临时命名空间验证的夹具才能设置缩短的托盘退出期限：`tray_exit` 为 30 秒，`tray_exit_hang_save` 为 4 秒；生产仍使用整次 120 秒总时限。报告分别记录实际耗时、允许的 2 秒原生退出确认及 1 秒观测余量。模拟服务夹具不修改 Windows 防火墙。仅验收 feature 下由夹具服务持有的测试能力，在核验临时存储边界、回环端口和模拟程序身份后，将防火墙配置隔离为外部测试边界；正式启动流程和启动后的实际监听地址校验仍然执行。这些检查不证明 Windows 注销或重启后存活，也不认证原生 ARK 跨图传输。报告明确排除原生防火墙配置、托盘菜单点击自动化和完整应用界面渲染。

### 采样运行中的进程

轻量记录 CPU、内存、句柄和线程基线时，可使用
[`collect-runtime-baseline.ps1`](../scripts/perf/collect-runtime-baseline.ps1)。
它按名称采样已有进程，不启动或停止进程。必须显式指定仓库外的输出路径：

```powershell
$baselinePath = Join-Path $env:TEMP ("langame-runtime-{0}.jsonl" -f [guid]::NewGuid())
.\scripts\perf\collect-runtime-baseline.ps1 -ProcessName langame-desktop -OutputPath $baselinePath
```

默认持续 300 秒，每两秒采样一次。JSONL 文件是本地诊断输出，不属于源码产物或可靠性
验收报告；分享前应检查内容。

### 如何判断结果

输出目录保存 `report.json` 和各阶段日志，记录真实退出码、测试计数、必需测试组、资源样本、
Git 版本及可用的源码快照标识。返回非零、零测试、必需测试缺失、资源证据缺失或断言失败，
均不能报告通过。旧证据不会被覆盖，失败后先查看对应日志，修复后使用新目录复测。

外层测试列表中标记 ignored 的内部进程探针，由普通父测试精确启动并管理；
依赖特定环境而未执行的测试不在验收覆盖范围内。
测试管理自身临时进程、监听器和文件，外层整体期限由 CI 或工作站资源守卫管理。
这些结果不能证明无限运行、所有游戏行为或任意驱动与 WebView 故障下都稳定；
虚拟时间推进也不能代替实际内存老化观察。
