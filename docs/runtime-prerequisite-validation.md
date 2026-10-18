# 运行环境实装验收 / Runtime prerequisite acceptance

[简体中文](#简体中文) | [English](#english)

## 简体中文

真实安装验收需要已可用的 Windows Sandbox。普通单元测试不会下载或安装系统运行库；`ignored` 导出测试只在宿主导出生产安装脚本和当前测试程序。运行器只在独立沙盒中安装，宿主只准备输入并接收报告。

实例启动与验收 worker 共用微软运行库下载函数。默认保留双栈网络；本次操作中经过正常 HTTPS 响应取得的微软重定向目标会被记录。连接或握手失败时，在同一整体预算内对该动态目标进行一次 IPv4 补试，继续校验 TLS、包摘要与微软签名。非微软重定向、HTTP 鉴权或限流、服务端退避、内容校验失败、磁盘错误及取消不会触发该补试。Windows PowerShell 验签显式加载其自身 `$PSHOME` 下的系统安全模块，避免继承 PowerShell 7 的模块路径后误加载不兼容模块。

在仓库外新建本次验收目录并导出输入：

```powershell
$runtimeWork = Join-Path 'C:\Temp' ('lgsm-runtime-acceptance-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $runtimeWork -Force | Out-Null
$env:LGSM_RUNTIME_SANDBOX_EXPORT_DIR = Join-Path $runtimeWork 'native-export'
$env:LANGAME_PORTABLE_SANDBOX_EXPORT = Join-Path $runtimeWork 'portable-export'
cargo test --locked -p app-steamcmd export_windows_runtime_sandbox_manifest -- --ignored --test-threads=1 --nocapture
cargo test --locked -p app-steamcmd export_portable_runtime_sandbox_bundle -- --ignored --test-threads=1 --nocapture
& ./scripts/verify_windows_runtime_sandbox.ps1 -ManifestPath (Join-Path $env:LGSM_RUNTIME_SANDBOX_EXPORT_DIR 'manifest.json') -WorkRoot $runtimeWork -PortableExportDirectory $env:LANGAME_PORTABLE_SANDBOX_EXPORT
```

受管工作站将 `runtimeWork` 设为符合本机资源策略的独立验收工作目录，并使用规定的 Cargo 包装器执行上述 Rust 命令。运行器使用独立 GUID 目录，不覆盖已有证据；`-PrepareOnly` 可仅准备输入，再用 `-PreparedRoot <准备目录>` 启动。沙盒仅映射只读输入和本次可写报告目录，并关闭剪贴板、音视频与打印机重定向。默认在来宾内下载官方包；宿主不执行安装包。报告写完后来宾自动关机；宿主超时不会强杀仍可能持有安装事务的来宾。

`-PortableOnly` 在另一个新沙盒中仅安装 VC++ x64 引导环境，再执行 portable worker。可用 `-BootstrapPackagePath` 显式提供已有的微软官方引导包，输入仍绑定摘要并在来宾验签；报告区分缓存引导与首次下载，缓存引导不能证明当前官方包下载流程成功。

VC++ v14 x64/x86 和 2013 x86 可使用两阶段验收，单独覆盖生产 HTTP 下载和冷安装。先导出生产下载 worker：

```powershell
$env:LANGAME_NATIVE_HTTP_SANDBOX_EXPORT = Join-Path $runtimeWork 'native-http-export'
cargo test --locked -p app-steamcmd export_native_http_sandbox_bundle -- --ignored --test-threads=1 --nocapture
```

需要先定位宿主网络时，可将 `LANGAME_NATIVE_HTTP_DIAGNOSTIC_ROOT` 设为仓库外尚不存在的目录，显式运行 `cargo test --locked -p app-steamcmd verify_native_http_recovery -- --ignored --test-threads=1 --nocapture`。这个检查通过同一生产函数真实下载并验签三项包，记录默认或 IPv4 恢复路由；宿主不执行安装，任何一项失败都会返回非零。它不代替沙盒冷安装。

第一阶段将 `-NativeHttpExportDirectory $env:LANGAME_NATIVE_HTTP_SANDBOX_EXPORT` 和 `-BootstrapPackagePath <已取得的微软VC14-x64包>` 传给运行器；运行器打印本次准备目录。这个沙盒仅用缓存包引导测试程序，再通过生产 HTTP 客户端、下载恢复、签名验证和发布函数首次下载三项官方包，不复用引导缓存。第二阶段将 `-RemainingDownloads -DownloadedPackagesDirectory <第一阶段准备目录>/output/native-http` 传给运行器，使用另一个全新沙盒安装这三个包。第二阶段要求安装前运行库实际缺失，并验证第一阶段完成标记、报告摘要、运行标识及包的摘要和字节数；复制后再次验签，再执行生产安装与独立加载探针。两个阶段均需提供 `-ManifestPath` 和仓库外的 `-WorkRoot`。报告明确记录下载与冷安装发生于不同沙盒，不能描述为一次沙盒中的完整事务编排。

系统运行库矩阵包含 VC++ 2010、2012、2013、v14 的 x86/x64 共八个安装包，DirectX June 2010 的双架构 DLL 加载，以及 XNA 的四个程序集加载。安装与探针直接来自生产代码，探针每次使用新进程。报告记录安装前状态、官方来源、实际摘要与签名、安装器退出码和安装后加载结果；要求重启的退出码单列，不能当作可启动成功。同一次完整矩阵使用一个新沙盒，逐项安装，不能把每项描述为各自独立的全新系统快照。默认完整矩阵的下载使用沙盒运行器的 Windows curl；生产 HTTP 客户端由上述两阶段 worker 单独覆盖，导出的安装脚本不会执行宿主 Rust 的安装事务编排。

宿主代理或合成 DNS 可能导致沙盒无法访问官方源。若宿主的 `verify_native_http_recovery` 已完整下载并验签三包，可改用 `-RemainingDownloads -HostDownloadReport <宿主诊断目录>/results.json -NativeHttpExportDirectory <下载worker导出目录>` 在新沙盒中冷安装。运行器保留原始宿主报告，验证其测试二进制摘要与导出清单一致，并逐项核对官方 URL、非零下载字节数、实物长度及 SHA-256；来宾再次核对这些输入，验签并要求安装前缺失、安装后实际加载成功。此模式不能同时提供 `-DownloadedPackagesDirectory`，也不会构造沙盒下载成功报告。它证明宿主生产下载与沙盒冷安装，不能证明沙盒的联网下载路径可用。

Portable worker 直接调用生产 `prepare_portable_runtime`：Minecraft Java 25 首次准备、Necesse Java 17 损坏包修复、PZ 复用已验证的 Java 25，以及 Romestead .NET 8 首次准备。游戏要求使用自有合成元数据与 PE 文件头，不复制或启动游戏程序；运行库真实下载、校验、解压、发布，并执行 Java 或生产 .NET 托管探针。`results.json`、逐项报告及 `portable/` 下的事件与结果保存在本次输出目录。安装、运行库加载和完整游戏启动是不同的验收范围。

## English

Real installation acceptance requires a working Windows Sandbox. Normal unit tests do not download or install system runtimes. The ignored export tests only export production scripts and the current test binary on the host; the runner installs packages exclusively inside its separate guest. Managed workstations must set `runtimeWork` to a separate acceptance directory permitted by their resource policy and use their prescribed Cargo wrapper.

Instance startup and the acceptance worker share the native Microsoft downloader. Normal dual-stack requests record official HTTPS redirect targets for the current operation. A transport or handshake failure may retry that dynamic target once over IPv4 within the same owner budget, preserving TLS, digest and Microsoft signature verification. Untrusted redirects, HTTP authentication or rate limits, server deferrals, content validation, disk errors and cancellation do not trigger this recovery. Windows PowerShell explicitly imports its Security module from its own `$PSHOME`, avoiding an incompatible module inherited through PowerShell 7's module path.

Use a fresh external work directory. The runner preserves existing evidence, maps inputs read-only and only the new report directory writable, and disables clipboard, audio, video and printer redirection. By default, packages download inside the guest. `-PrepareOnly` prepares inputs without launching; `-PreparedRoot <prepared directory>` starts that prepared run. The guest shuts down after flushing its reports. A host timeout does not kill a guest that may still own an installation transaction.

`-PortableOnly` uses another fresh sandbox with only the native VC++ x64 bootstrap and the portable worker. An explicit `-BootstrapPackagePath` can supply a previously obtained official Microsoft bootstrap package. Its digest remains bound to the input bundle and its signature is verified inside the guest. Reports distinguish cached bootstrap from fresh download; cached bootstrap does not establish current download-path success.

The native HTTP export command above enables two-stage acceptance for VC++ v14 x64/x86 and 2013 x86. First pass `-NativeHttpExportDirectory` and `-BootstrapPackagePath` to the runner, together with the usual manifest and external work root. This guest uses the verified cached bootstrap only to load the test binary, then downloads all three official packages afresh through the production HTTP client, downloader, signature verification and publication functions. Next pass `-RemainingDownloads -DownloadedPackagesDirectory <first-stage prepared directory>/output/native-http` to cold-install those packages in another fresh guest. The second stage validates the first stage's completion marker, report digest, run identity, package digests and byte counts, then repeats signature verification and requires a genuinely missing baseline before production installation and independent loading probes. Reports explicitly distinguish download and cold installation across separate guests from a single end-to-end transaction.

For host network diagnosis, set `LANGAME_NATIVE_HTTP_DIAGNOSTIC_ROOT` to a nonexistent external directory and explicitly run `cargo test --locked -p app-steamcmd verify_native_http_recovery -- --ignored --test-threads=1 --nocapture`. This downloads and verifies the three packages through the same production function, records default and IPv4 recovery routes, performs no host installation and exits nonzero if any case fails. It does not establish cold-install acceptance.

The native matrix covers eight VC++ installers (2010, 2012, 2013 and v14, each x86/x64), both DirectX June 2010 architectures, and all four XNA assemblies. It exports the production installer and probe scripts, uses fresh probe processes, and records baseline readiness, official source, actual digest and signature, installer exit code and post-install loading. Restart-required outcomes remain distinct from successful readiness. Cases install sequentially in one fresh sandbox, rather than each receiving a separate clean snapshot. The default full matrix downloads through the runner's Windows curl; the separate two-stage worker exercises the production HTTP client. Exported installer scripts do not exercise the host Rust transaction orchestration.

Host proxies or synthetic DNS can make official sources unreachable from Sandbox. When the host `verify_native_http_recovery` has fully downloaded and verified all three packages, use `-RemainingDownloads -HostDownloadReport <host-diagnostic-directory>/results.json -NativeHttpExportDirectory <download-worker-export-directory>` to cold-install them in a fresh guest. The runner preserves the original host report, binds its test binary digest to the export manifest, and verifies each official URL, positive byte count, artifact length and SHA-256. The guest repeats these checks and Microsoft signature verification, requires a missing baseline, then installs and independently loads the runtimes. This mode excludes `-DownloadedPackagesDirectory` and does not create a successful guest-download report. It establishes production host downloads plus guest cold installation, rather than a working guest network download path.

The portable worker calls the production `prepare_portable_runtime` API for fresh Minecraft Java 25 preparation, Necesse Java 17 bundle repair, PZ reuse of verified Java 25, and fresh Romestead .NET 8 preparation. Synthetic requirement metadata and PE headers replace game assets. Actual official runtimes download, verify, extract, publish and execute Java or the production managed .NET probe. Aggregate and per-case reports, plus portable events and results, remain in the external output directory. Runtime preparation acceptance does not establish complete game-launch acceptance.
