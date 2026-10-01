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

### 构建与运行

Windows 开发环境需要 Git、MSVC C++ 构建工具、WebView2、Python 3.11 或更高版本，以及仓库固定的 Rust 和 Node.js/npm 环境。Rust 版本以 [`rust-toolchain.toml`](../rust-toolchain.toml) 为准，Node.js/npm 要求见 [`apps/desktop/package.json`](../apps/desktop/package.json) 的 `engines`。Tauri 的系统依赖见[官方前置条件](https://v2.tauri.app/start/prerequisites/)。

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

先运行改动直接相关的检查，再按影响完成仓库基线；不得通过减少断言、跳过失败或修改预期掩盖问题：

```powershell
python -B -m unittest discover -s scripts/tests -p "test_*.py"
npm --prefix apps/desktop run verify
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
```

进程、控制台、日志或故障恢复改动还应运行[运行可靠性检查](../scripts/verify_runtime_reliability.py)，按脚本的 `--help` 选择范围和仓库外输出目录。CI 的完整执行顺序见 [ci.yml](../.github/workflows/ci.yml)。模拟进程、浏览器组件与真实游戏验收是不同证据；报告实际执行的范围、环境和结果。需要游戏包、外部服务或真实凭据的可选验收应在获准的隔离环境中执行。

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

### Build and run

Use Windows with Git, the MSVC C++ build tools, WebView2, Python 3.11 or later, and the versions declared in [`rust-toolchain.toml`](../rust-toolchain.toml) and the `engines` field of [`apps/desktop/package.json`](../apps/desktop/package.json). Follow the [build and run commands above](#构建与运行), starting the frontend and desktop in separate terminals. Install dependencies from their lock files.

### Change and verification

Keep domain logic in the owning crate and interface adapters small. Bound asynchronous work and provide cancellation, error handling and cleanup. Validate untrusted inputs at storage, process and network boundaries. Review dependency compatibility, licensing and lock-file changes.

Run the relevant focused checks, then the [repository verification commands](#改动与验证) for the affected scope. Preserve existing assertions and failure handling. Follow [CI](../.github/workflows/ci.yml) for the full sequence, and use the [runtime reliability script](../scripts/verify_runtime_reliability.py) for process and recovery changes. Report synthetic, browser and native-game evidence separately. Opt-in tests requiring game packages, external services or credentials need an authorized isolated environment.

Follow [Game Integration Validation](game-integration-validation.md#english) for module changes. Maintain `config-sources.toml` and sanitized fixtures, then use the [generation and check commands](#模块与生成文件); do not edit generated ledgers or acceptance Markdown directly. Tauri's generated schemas are ignored output; edit the source capability or Tauri configuration instead.

Preserve retained data during schema changes and verify backups, conversion, integrity and actual startup. Never bypass validation by deleting data or rewriting migration checksums. Keep credentials, private addresses, player data, runtime output, proprietary binaries and unlicensed media out of commits. Preserve third-party notices and use the [private security reporting process](../SECURITY.md#english). Run the [publication-boundary checks](#数据与公开边界); history checks require a complete clone. Source checks do not replace installation, upgrade, uninstall or data-retention acceptance.
