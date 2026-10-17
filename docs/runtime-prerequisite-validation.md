# 运行环境实装验收 / Runtime prerequisite acceptance

[简体中文](#简体中文) | [English](#english)

## 简体中文

真实安装验收需要已可用的 Windows Sandbox。普通单元测试不会下载或安装系统运行库；下面两个 `ignored` 测试只在宿主导出生产安装脚本和当前测试程序。运行器只在独立沙盒中安装，宿主只准备输入并接收报告。

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

受管工作站将 `runtimeWork` 改为当前 Codex 任务的 `work/` 目录，并使用规定的 Cargo 包装器执行上述 Rust 命令。运行器使用独立 GUID 目录，不覆盖已有证据；`-PrepareOnly` 可仅准备输入，再用 `-PreparedRoot <准备目录>` 启动。沙盒仅映射只读输入和本次可写报告目录，并关闭剪贴板、音视频与打印机重定向。默认在来宾内下载官方包；宿主不执行安装包。报告写完后来宾自动关机；宿主超时不会强杀仍可能持有安装事务的来宾。

`-PortableOnly` 在另一个新沙盒中仅安装 VC++ x64 引导环境，再执行 portable worker。可用 `-BootstrapPackagePath` 显式提供已有的微软官方引导包，输入仍绑定摘要并在来宾验签；报告区分缓存引导与首次下载，缓存引导不能证明当前官方包下载流程成功。

系统运行库矩阵包含 VC++ 2010、2012、2013、v14 的 x86/x64 共八个安装包，DirectX June 2010 的双架构 DLL 加载，以及 XNA 的四个程序集加载。安装与探针直接来自生产代码，探针每次使用新进程。报告记录安装前状态、官方来源、实际摘要与签名、安装器退出码和安装后加载结果；要求重启的退出码单列，不能当作可启动成功。同一次矩阵使用一个新沙盒，逐项安装，不能把每项描述为各自独立的全新系统快照。原生包下载使用沙盒运行器的 Windows curl；导出的系统脚本不会执行宿主 Rust 的 HTTP 客户端或安装事务编排。

Portable worker 直接调用生产 `prepare_portable_runtime`：Minecraft Java 25 首次准备、Necesse Java 17 损坏包修复、PZ 复用已验证的 Java 25，以及 Romestead .NET 8 首次准备。游戏要求使用自有合成元数据与 PE 文件头，不复制或启动游戏程序；运行库真实下载、校验、解压、发布，并执行 Java 或生产 .NET 托管探针。`results.json`、逐项报告及 `portable/` 下的事件与结果保存在本次输出目录。安装、运行库加载和完整游戏启动是不同的验收范围。

## English

Real installation acceptance requires a working Windows Sandbox. Normal unit tests do not download or install system runtimes. The two ignored tests in the commands above only export production scripts and the current test binary on the host; the runner installs packages exclusively inside its separate guest. Managed workstations must set `runtimeWork` to the current Codex task's `work/` directory and continue to use their prescribed Cargo wrapper.

Use a fresh external work directory. The runner preserves existing evidence, maps inputs read-only and only the new report directory writable, and disables clipboard, audio, video and printer redirection. By default, packages download inside the guest. `-PrepareOnly` prepares inputs without launching; `-PreparedRoot <prepared directory>` starts that prepared run. The guest shuts down after flushing its reports. A host timeout does not kill a guest that may still own an installation transaction.

`-PortableOnly` uses another fresh sandbox with only the native VC++ x64 bootstrap and the portable worker. An explicit `-BootstrapPackagePath` can supply a previously obtained official Microsoft bootstrap package. Its digest remains bound to the input bundle and its signature is verified inside the guest. Reports distinguish cached bootstrap from fresh download; cached bootstrap does not establish current download-path success.

The native matrix covers eight VC++ installers (2010, 2012, 2013 and v14, each x86/x64), both DirectX June 2010 architectures, and all four XNA assemblies. It exports the production installer and probe scripts, uses fresh probe processes, and records baseline readiness, official source, actual digest and signature, installer exit code and post-install loading. Restart-required outcomes remain distinct from successful readiness. Cases install sequentially in one fresh sandbox, rather than each receiving a separate clean snapshot. Native downloads use the runner's Windows curl; exported scripts do not exercise the host Rust HTTP client or transaction orchestration.

The portable worker calls the production `prepare_portable_runtime` API for fresh Minecraft Java 25 preparation, Necesse Java 17 bundle repair, PZ reuse of verified Java 25, and fresh Romestead .NET 8 preparation. Synthetic requirement metadata and PE headers replace game assets. Actual official runtimes download, verify, extract, publish and execute Java or the production managed .NET probe. Aggregate and per-case reports, plus portable events and results, remain in the external output directory. Runtime preparation acceptance does not establish complete game-launch acceptance.
