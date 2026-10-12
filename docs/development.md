# 开发指南 / Development

[简体中文](#简体中文) | [English](#english)

## 简体中文

提交改动前阅读[贡献指南](../CONTRIBUTING.md#简体中文)与[贡献协议](../CONTRIBUTOR_AGREEMENT.md#简体中文)。安装和使用说明见[项目首页](../README.md)，构建安装包见[桌面发行指南](desktop-release.md)。

### 仓库结构

| 路径 | 职责 |
| --- | --- |
| `apps/desktop/` | React 界面、翻译、前端检查与 Tauri 命令适配层 |
| `crates/` | 领域类型、存储、运行时、模块接入、SteamCMD 与 Windows 支持 |
| `modules/` | 游戏清单、设置 Schema、原生模板与验收夹具 |
| `migrations/` | 数据库结构 |
| `scripts/` | 校验、生成与构建工具 |
| `docs/` | 使用指南、接入规范、协议与配置来源 |
| `.github/` | 问题表单、贡献模板与 CI |

### 运行链路与职责

Windows 正式运行时，React 页面与 hooks 通过 `api.ts` / `api-transport.ts` 调用 Tauri。桌面进程保留窗口、托盘、文件选择与应用更新操作；其余调用经当前用户命名管道进入后台 runtime service。后台进程拥有服务器进程、运行状态、存储写入与恢复任务，界面重载后可以重新连接。显式启用的 LAN HTTP 入口复用业务分发，但本地专用维护操作保留独立权限边界；开发预览数据仅在开发模式加载。

`app-core` 定义共享领域数据，`app-modules` 读取游戏清单与 schema；`app-storage` 负责 SQLite、实例配置、存档与备份；`app-runtime` 和 `app-platform-win` 负责进程生命周期及 Windows 能力；`app-steamcmd`、`app-network`、`app-knowledge` 分别承载安装、网络策略和知识检索。Tauri commands 编排这些模块。更新安装先下载并验证包，再停止后台服务及游戏进程，最后运行安装器。

修改 IPC 时同步核对前端参数、桌面本地命令、runtime service 与 LAN 分发，避免在多处手工登记时漂移。客户端顶层命令参数使用 Tauri 的 camelCase，不重复发送 snake_case 别名；嵌套 `input` 遵循对应 Rust Serde 数据类型的命名，不能递归改名。桌面进程归属和仅限命名管道访问是不同边界，命令登记不能混合这两种含义。

存储就绪缓存保存数据库文件身份。运行概览与实例详情共用一次局部数据库读取，数据库阶段在成功及错误路径都显式关闭连接，再读取配置、分析日志和查询玩家。保留数据库替换检测、前置就绪检查、事务和恢复语义；连接创建次数减少不能替代实际耗时测量。

### 构建与运行

Windows 开发环境需要 Git、MSVC C++ 构建工具、WebView2、Python 3.11 或更高版本，以及仓库固定的 Rust 和 Node.js/npm 环境。Rust 版本以 [`rust-toolchain.toml`](../rust-toolchain.toml) 为准，Node.js/npm 要求见 [`apps/desktop/package.json`](../apps/desktop/package.json) 的 `engines`。Tauri 的系统依赖见[官方前置条件](https://v2.tauri.app/start/prerequisites/)。

TypeScript 7 CLI 负责正式类型检查；`@typescript/typescript6` 为源码分析与测试夹具提供稳定的 compiler API，两者有不同职责。`windows-core` 的 COM 类型必须与 `webview2-com` 一致，当前分别固定为 `0.62.2` / `0.39.1`，应作为兼容组合核验，不能仅按更大的版本号单独替换。

从仓库根目录安装锁定的依赖并构建：

```powershell
npm ci --prefix apps/desktop --strict-allow-scripts
npm --prefix apps/desktop run build
cargo check --workspace --locked
```

在一个终端运行前端：

```powershell
npm --prefix apps/desktop run dev
```

在另一个终端启动桌面：

```powershell
cargo run --locked --manifest-path apps/desktop/src-tauri/Cargo.toml --bin langame-desktop
```

### 改动与验证

保持既有模块边界，领域逻辑放在对应 crate，界面和 Tauri 适配层负责呈现与调用。异步任务应有明确的所有者、并发边界、取消、超时和清理路径。存储、进程、网络与界面输入须在相应边界校验。依赖变更须审查兼容性、许可证与锁文件差异。

本地与 GitHub 共用 `scripts/verify_project.py`。先运行改动直接相关的测试，再运行快速层；不得通过减少断言或修改预期掩盖失败。`--plan` 可以先查看实际命令。

```powershell
python -B scripts/verify_project.py --tier quick
```

| 层级 | 检查内容 | 执行时机 |
| --- | --- | --- |
| 快速反馈 | Python 工具测试与当前源码公开边界；前端单元和契约、翻译与模块一致性、类型与构建；Rust 格式、全工作区 Clippy/编译、核心单测及备份/迁移/实例创建/数据库句柄释放回归 | 本地相关检查后、每次 push/PR |
| 界面相关回归 | 8 个关键浏览器文件及受影响的完整功能组，保留文件中的全部用例 | 已知局部界面或其测试变更的 push/PR |
| 完整回归 | 快速层规则、完整公开历史与导出计划、全部前端浏览器用例、完整 Rust 工作区、Windows 合成进程生命周期、Tauri/WebView2 恢复 | 重要边界变更的 push/PR、每日定时、手动发行前检查 |
| 安装发行验收 | 实际签名安装包、安装/升级/卸载、旧版更新、存档与配置保留、关键开服流程 | 发布前在获准的隔离 Windows 环境 |

快速反馈以 5–10 分钟为优化目标；冷缓存、依赖和测试规模会影响耗时。此目标不是超时放宽或跳过失败的理由。CI 保存 Rust 依赖及其编译产物，前端只构建一次并传给 Rust；Clippy 已执行编译与类型检查，不再在同一工作流重复 `cargo check`。

前端 `*-browser.test.cjs` 属于真实浏览器层，两个既有命名例外集中在共享入口。其他测试以 4 个文件为并发上限；浏览器在每个进程内串行，CI 分两个互不重叠的分片。新增真实浏览器测试遵循命名约定。Rust 的 `app-core`、`app-modules`、`app-knowledge` 用 4 个测试线程；涉及 Windows 进程、全局环境、存储和外部边界的其余套件保留串行，避免把资源争用当成产品缺陷。

实例路径检查为所有实例派生实际运行、存档及原生配置归属，并在写入前重新验证冲突；准备外部包前后的检查分别读取当前记录与文件系统。完整路径诊断只为正在查看报告的实例生成，不为其他实例计算无人读取的诊断。Windows 祖先路径的符号链接和 reparse 属性使用同一次 metadata 读取，仍检查全部祖先。比较性能时保留同一测试范围与线程数，分别报告编译、测试执行和缓存耗时。

Windows 桌面使用 Tauri 和 WebView2 呈现 React 界面。CI 的 `Desktop UI regression` 在独立 Edge/Chrome 中挂载生产组件，通过模拟 IPC 或受控 HTTP 服务检查 DOM、交互、焦点、布局和订阅清理；它不启动完整 Tauri 应用，也不证明真实数据库写入或游戏进程启停。`Verify native Tauri and WebView2 recovery` 才运行原生宿主及其 WebView2 恢复检查；真实安装、升级和关键开服仍按发行验收执行。相同测试 worker 可复用同一夹具的 Vite 编译缓存，每个场景仍拥有独立浏览器配置、服务和界面状态，worker 结束后回收其缓存。

只执行某层时仍使用同一入口：

```powershell
python -B scripts/verify_project.py --component frontend-tests --tier quick
python -B scripts/verify_project.py --component frontend-browser
python -B scripts/verify_project.py --component frontend-browser --base HEAD~1
python -B scripts/verify_project.py --component rust --tier quick
python -B scripts/verify_project.py --tier full --output 'C:\Verification\LanGame-candidate'
```

已有 npm 入口也复用共享规则：`npm --prefix apps/desktop run verify` 运行前端快速检查和当前源码秘密/公开边界检查，`verify:release` 增加全部浏览器测试和完整公开历史检查。这两个入口均不包含 Rust 或原生桌面验收；CI 的前端分片不重复独立 policy 任务中的公开检查。

完整检查使用新的仓库外证据目录；`runtime/report.json` 和 `desktop/report.json` 记录实际运行结果。`--nsis-root` 可附加运行安装器守卫及语言夹具；没有提供时明确报告未执行，不将其算作安装/升级验收。实际发行要求见[桌面发行指南](desktop-release.md)。

工作区 Cargo 封装可通过 `--cargo-wrapper` 显式传入，Node 资源封装用 `--node-wrapper`。用 `--frontend-output` 指定尚不存在的仓库外构建目录，或以 `--frontend-dist` 复用当前候选版本的已验证前端产物；托管原生验收必须用 `--native-executable` 指向构建回执中的专用 `desktop-reliability` 可执行文件，入口不会猜测共享缓存路径。托管主机应先按本机规则构建前端，再选择 `--component rust` / `native`；公共源码命令不绑定任何工作站路径。

CI 的风险路径在共享入口统一定义，浏览器、Rust 和原生桌面三层独立选择。已核实消费者的局部界面变更运行关键集合和相关完整功能组；关键集合覆盖创建实例、配置控件与恢复、维护保存、实例导航、退出、订阅清理和生产通信。功能组复用现有测试文件，不减少文件内的断言或视口。共享壳、公共 API、全局样式、前端依赖、公共浏览器运行器以及未知归属保留全部浏览器测试。私有夹具通过实际引用定位所属测试；不能确定归属时回退全量。普通前端单测或源码扫描测试变更保留快速层。`frontend-browser` 不带选择参数仍运行完整集合；`--base <commit>` 使用与 CI 相同的规则选择该提交到 `HEAD` 的相关集合，基线无效时回退全量；显式 `--tier full` 始终全量。

`app-knowledge` 与 `app-modules` 的内部变更保留快速层，因为该层已运行它们的全部测试，并编译检查所有消费者；`app-core` 及其余 Rust crate、未知新 crate、Tauri/IPC、根目录游戏模块和迁移变更进入 Rust 完整层。仅 Rust 内部算法变更不会连带运行全部浏览器测试。Tauri、共享领域契约、存储、运行时、Windows 平台或原生验收夹具变更还运行 Tauri/WebView2 验收。Tauri、共享领域契约、游戏模块和迁移变更保守加入全部浏览器测试；共享依赖、构建与检查入口变更进入全部三层，包括各 crate 的 `Cargo.toml`、`build.rs` 和 ONNX/CRT 运行库构建输入。其他 Python 工具改动运行工具测试与完整公开边界检查。无法解析 push/PR 基线时执行全部三层；手动选择 `quick` 则尊重显式范围。提交前仍应先运行相关行为测试，不能仅依赖自动分类。每日完整回归和手动发行前检查运行同一套命令，不维护另一份测试清单。可在 GitHub 的 CI 工作流手动选择 `full`；这只验证代码，不创建或发布安装包。

模拟进程、浏览器组件与真实游戏验收是不同证据；报告实际执行的范围、环境和结果。需要游戏包、外部服务或真实凭据的可选验收应在获准的隔离环境中执行。

### 模块与生成文件

新增游戏或修改配置、启动、生命周期、端口、查询及玩家管理时，遵循[游戏接入验证](game-integration-validation.md)。设置与排除项记录在 `config-sources.toml`，脱敏夹具放在 `modules/<module-id>/config-fixtures/`；支持声明应有权威来源或可复现的专用服务器证据。

配置台账与验收 Markdown 来自模块源文件。修改源文件或生成器后重新生成，不直接编辑生成结果：

```powershell
python -B scripts/verify_game_config_provenance.py --write
python -B scripts/verify_game_config_acceptance.py --require-all --write
```

提交前核对生成结果及配置覆盖：

```powershell
python -B scripts/verify_game_config_provenance.py --check
python -B scripts/verify_module_setting_coverage.py
python -B scripts/verify_game_config_acceptance.py --require-all --check
python -B scripts/verify_library_media.py
```

生成目标为 `docs/game-config-source-ledger.md`、`docs/game-config-source-ledger.csv` 与 `docs/game-config-acceptance/*.md`。Tauri 生成的 `apps/desktop/src-tauri/gen/schemas/` 不提交；权限变更应修改 `capabilities/default.json` 或 `tauri.conf.json`。

### 数据与公开边界

涉及持久化格式的变更须保留有效数据，先备份，再验证转换、完整性与实际启动；不得删除数据或修改迁移校验和绕过失败。测试使用隔离数据，生成物、日志和本机运行数据不进入提交。

凭据、私人地址、玩家数据、专有游戏二进制和未授权媒体不得进入源码或问题报告。保留第三方许可证与署名，漏洞通过[安全政策](../SECURITY.md#简体中文)私下报告。提交前运行公开边界检查：

```powershell
python -B scripts/verify_no_tracked_secrets.py
python -B scripts/verify_open_source_boundary.py
python -B scripts/verify_open_source_history.py
```

历史检查需要完整克隆。源码检查通过不替代安装、升级、卸载和数据保留验收。

## English

Read [Contributing](../CONTRIBUTING.md#english) and the [Contributor Agreement](../CONTRIBUTOR_AGREEMENT.md#english) before submitting changes. See the [project introduction](../README.en.md) for usage and [desktop release guide](desktop-release.md) for installer builds.

The repository separates the React/Tauri application in `apps/desktop/`, reusable Rust components in `crates/`, game definitions and fixtures in `modules/`, database schema in `migrations/`, and verification tools in `scripts/`.

### Runtime flow and ownership

On Windows, React views and hooks call Tauri through `api.ts` / `api-transport.ts`. The desktop process owns windows, the tray, file selection and application updates. Other calls reach the background runtime service through a current-user named pipe. That service owns game processes, runtime state, storage mutations and recovery workers, allowing the interface to reconnect after a reload. The explicitly enabled LAN HTTP entry shares business dispatch while local-only maintenance retains a separate authorization boundary. Preview data loads only in development mode.

`app-core` defines shared domain data and `app-modules` reads game manifests and schemas. `app-storage` owns SQLite, instance configuration, saves and backups; `app-runtime` and `app-platform-win` own process lifecycle and Windows integration. `app-steamcmd`, `app-network` and `app-knowledge` provide installation, network policy and knowledge retrieval. Tauri commands coordinate these components. Updates download and verify the package before stopping the runtime service and game processes and invoking the installer.

For IPC changes, check frontend arguments, desktop-local commands, runtime-service routing and LAN dispatch together to prevent manual registrations from drifting. Clients use Tauri camelCase for top-level arguments without sending duplicate snake_case aliases. Nested `input` follows its Rust Serde type; do not rename fields recursively. Desktop-process ownership and named-pipe-only authorization are separate command boundaries.

Storage readiness caches database file identity. The combined runtime overview and instance-details read uses one operation-local database connection, explicitly closed on success and error before configuration reads, log analysis and player queries. Preserve database-replacement detection, readiness checks before reconciliation, transactions and recovery semantics. Fewer connection creations do not establish a measured latency improvement.

### Build and run

Use Windows with Git, the MSVC C++ build tools, WebView2, Python 3.11 or later, and the versions declared in [`rust-toolchain.toml`](../rust-toolchain.toml) and the `engines` field of [`apps/desktop/package.json`](../apps/desktop/package.json). Follow the [build and run commands above](#构建与运行), starting the frontend and desktop in separate terminals. Install dependencies from their lock files.

TypeScript 7 provides the production type checker; `@typescript/typescript6` supplies the stable compiler API used for syntax inspection and test fixtures. Keep `windows-core` COM types compatible with `webview2-com`: the current pair is pinned to `0.62.2` / `0.39.1` and must be evaluated together rather than upgraded independently by version number.

### Change and verification

Keep domain logic in the owning crate and interface adapters small. Bound asynchronous work and provide cancellation, error handling and cleanup. Validate untrusted inputs at storage, process and network boundaries. Review dependency compatibility, licensing and lock-file changes.

Run focused tests, then `python -B scripts/verify_project.py --tier quick`. Local and CI commands share this entry point. Quick checks include frontend unit/contracts, type/build checks, Rust workspace Clippy/compilation, core tests and retained-data regressions. The `full` tier adds all browser and Rust tests, complete publication-history checks, synthetic Windows processes and native Tauri/WebView2 recovery; use a new external `--output` directory.

The existing `npm --prefix apps/desktop run verify` entry runs quick frontend checks and current secret/publication boundaries. `verify:release` adds all browser tests and publication history. Both reuse the shared rules and exclude Rust/native acceptance. CI frontend jobs leave publication checks to the independent policy job.

Instance path checks derive runtime, save and native configuration ownership for every instance and revalidate conflicts before writes. Checks before and after external package preparation independently read current records and filesystem state. Full path diagnostics are produced only for the instance whose report is requested. Windows ancestor checks reuse one metadata observation for symlink and reparse attributes while still checking every ancestor. Compare performance with the same test scope and thread count, reporting compilation, test execution and cache costs separately.

The Windows desktop renders React through Tauri and WebView2. CI's `Desktop UI regression` mounts production components in isolated Edge/Chrome sessions with mock IPC or controlled HTTP services to check DOM behavior, focus, layout and subscription cleanup. It does not start the complete Tauri app or verify real database writes and game processes. `Verify native Tauri and WebView2 recovery` runs the native host and WebView2 recovery checks; actual installation, upgrade and hosting flows remain release acceptance requirements. A worker can reuse the Vite compilation cache for the same fixture while every scenario retains its own browser profile, server and UI state; the worker removes its cache on completion.

CI selects browser, Rust and native desktop regression independently. Local UI changes with verified consumers select eight critical browser files plus the affected complete behavioral families. The critical set covers creation, configuration and recovery, maintenance saves, navigation, exit, subscription cleanup and production transport. Selection retains every case and viewport in each chosen file. Shared shell/API/style/dependency changes, common browser infrastructure and unknown ownership retain all browser tests. Private fixtures select their actual consumers, falling back to all tests when ownership is unclear. Ordinary unit/scanner changes retain quick feedback. `--component frontend-browser` without a selection still runs every browser file; adding `--base <commit>` uses the same selection rules as CI for that commit through `HEAD`, falling back to the complete suite for an invalid base. An explicit `--tier full` always runs the complete suite.

Internal changes in `app-knowledge` and `app-modules` retain quick checks, which already run their complete tests and compile-check all consumers. `app-core`, the other Rust crates, unknown new crates, Tauri/IPC, root game modules and migrations select complete Rust tests without automatically selecting browser or native tests. Tauri, shared domain contracts, storage, runtime, Windows integration and native fixtures also select native Tauri/WebView2 acceptance. Tauri, shared contracts, modules and migrations select complete browser regression; shared dependency/build/verification entries select all three layers, including crate manifests, `build.rs` files and ONNX/CRT runtime build inputs. Other Python-tool changes select tool tests and complete publication checks. Missing push/PR baselines select all layers, while an explicit manual `quick` selection is honored. The daily schedule and manual pre-release run use the same complete rules.

Rust dependency caching and a shared frontend artifact avoid repeated preparation. The 5–10 minute quick-feedback target must be measured on CI, not assumed. Use `--plan`, `--component`, `--cargo-wrapper`, `--node-wrapper`, `--frontend-output`, `--frontend-dist` and `--native-executable` as described above for focused or managed execution. Preserve existing assertions and failure handling. Real signed installation, upgrade and retained-data acceptance remain separate release requirements; optional NSIS guard checks require `--nsis-root`. Report synthetic, browser and native-game evidence separately. Opt-in tests requiring game packages, external services or credentials need an authorized isolated environment.

Follow [Game Integration Validation](game-integration-validation.md#english) for module changes. Maintain `config-sources.toml` and sanitized fixtures, then use the [generation and check commands](#模块与生成文件); do not edit generated ledgers or acceptance Markdown directly. Tauri's generated schemas are ignored output; edit the source capability or Tauri configuration instead.

Preserve retained data during schema changes and verify backups, conversion, integrity and actual startup. Never bypass validation by deleting data or rewriting migration checksums. Keep credentials, private addresses, player data, runtime output, proprietary binaries and unlicensed media out of commits. Preserve third-party notices and use the [private security reporting process](../SECURITY.md#english). Run the [publication-boundary checks](#数据与公开边界); history checks require a complete clone. Source checks do not replace installation, upgrade, uninstall or data-retention acceptance.
