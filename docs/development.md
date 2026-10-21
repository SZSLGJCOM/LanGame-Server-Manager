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

所有实例在游戏程序更新之后、创建游戏进程之前，经 `commands_runtime_environment` 检查运行环境；固定游戏版本不会跳过环境检查。原生依赖从实际启动及验证程序的有界 PE 普通／延迟 imports 读取，微软安装器由有限目录选择，不执行游戏包中的任意安装脚本。Java 和 .NET 从已安装程序的版本要求选择兼容运行库，下载验证后发布到 SteamCMD 同级工具目录的独立版本目录，不覆盖游戏程序或在用运行库。缺失环境无法准备、安装后复检失败或系统要求重启时均阻止启动；已有环境通过探针而官方更新源不可达时，明确记录离线复用。系统安装一旦开始会持有安装锁并等待真实退出，再处理取消，避免把仍在执行的安装当作已取消。进度与失败原因进入启动控制台及既有后台任务。

运行环境的真实官方包安装、加载与 portable 准备验收见 [Windows Sandbox 验收说明](runtime-prerequisite-validation.md#简体中文)；普通单元测试不会执行这些系统安装。

桌面启动在后台建立连接，界面保持加载状态。连接前核实后台状态；若旧后台正在退出，等待其原始进程句柄确认结束后再启动新后台，不能向退出中的后台初始化存储。退出助手在关闭旧界面前持有按用户和原进程身份隔离的原生命名事件，覆盖退出请求尚未送达的窗口。启动期间再次退出会取消尚未创建后台的连接等待；已创建的后台仍交给同一退出流程处理，沿用点击退出时的截止时间，不重放业务操作。

产品命令在 `apps/desktop/src-tauri/src/command_registry.rs` 统一声明 Tauri 注册、执行进程、LAN 访问权限和完成策略。参数解码仍由各传输适配层负责；runtime service 的内部控制消息不进入产品命令表。修改 IPC 时同步核对前端参数和实际适配器，契约检查核对登记与适配器的双向覆盖。客户端顶层命令参数使用 Tauri 的 camelCase，不重复发送 snake_case 别名；嵌套 `input` 遵循对应 Rust Serde 数据类型的命名，不能递归改名。桌面进程归属和仅限命名管道访问是不同边界，命令登记不能混合这两种含义。

存储就绪缓存保存数据库文件身份。运行概览与实例详情共用一次局部数据库读取，数据库阶段在成功及错误路径都显式关闭连接，再读取配置、分析日志和查询玩家。保留数据库替换检测、前置就绪检查、事务和恢复语义；连接创建次数减少不能替代实际耗时测量。

生产运行期命令通过 `DesktopState::bootstrap_storage` 或调用方捕获的存储上下文访问数据，不再从进程环境重新定位数据根。运行时固定初始用户定位根，由其中保存的数据位置指向选中的数据目录；定位根与数据目录不能互换。每次重新进入存储时仍校验已保存的位置并读取当前设置，不能缓存解析后的路径来跳过数据目录、设置或数据库缺失检查。

路径更新与在途操作由存储上下文租约协调。后台文件工作捕获本次 `StorageBootstrap` 或 `StoragePaths`，通过已有任务 helper 携带租约，直至实际完成；实例锁与安装锁也由执行工作的任务持有，不能随等待方取消而提前释放。需要延后重新读取存储位置的后台任务捕获 `StorageSource`，保留相同用户定位根。授权、预览确认、实例变更前置条件及关闭流程的专用租约仍保持各自职责。

跨进程安装锁使用 `DesktopState` 持有的 `InstallLockContext`，显式传入安装、Mod、启动和移除链路；锁上下文固定在用户定位根，不能从选中的数据目录推导另一套锁根。用户目录环境变量全部缺失时，安装锁与存储模块使用同一默认定位根，不再单独回退到临时目录。测试夹具仍有进程全局环境和 Windows 进程依赖，桌面测试继续串行；环境变化回归放在独立子进程中，不能据此提高整个桌面套件的并发。

服务器列表与运行概览直接加载；配置、Mod、玩家、GM 工具和维护工作区首次访问时加载代码，共用已有的可恢复模块加载机制。维护工作区访问后保留挂载，由 `active` 控制后台读取，实例切换时重置；这用于保留在途操作与存档结果。新增重型目录或功能时检查实际构建依赖，避免能力判断等轻量入口再次静态引入整个工作区。

### 构建与运行

Windows 开发环境需要 Git、MSVC C++ 构建工具、WebView2、Python 3.11 或更高版本，以及仓库固定的 Rust 和 Node.js/npm 环境。Rust 版本以 [`rust-toolchain.toml`](../rust-toolchain.toml) 为准，Node.js/npm 要求见 [`apps/desktop/package.json`](../apps/desktop/package.json) 的 `engines`。Tauri 的系统依赖见[官方前置条件](https://v2.tauri.app/start/prerequisites/)。

TypeScript 7 CLI 负责正式类型检查；`@typescript/typescript6` 为源码分析与测试夹具提供稳定的 compiler API，两者有不同职责。`windows-core` 的 COM 类型必须与 `webview2-com` 一致，当前分别固定为 `0.62.2` / `0.39.1`，应作为兼容组合核验，不能仅按更大的版本号单独替换。

构建 ARK 扩展时，可将 `LANGAME_ARK_ARCHIVE_DIR` 设为已存在的绝对目录，复用 `AsaApi-sdk.zip`、`AseApi-sdk.zip`、`asa-runtime.zip` 和 `AsaApiLoader-sdk.zip`。文件名、官方来源及固定 SHA-256 见 `scripts/build_ark_tools.ps1`。构建输出不能写入此只读目录；缺失文件或校验失败会直接停止，不回退联网。`LANGAME_PROXY_SOURCE_DIR` 同样接受只读绝对目录，按 `modules/runescapedragonwilds/extensions/proxy-sources.json` 的相对路径提供 32 个固定版本源码；Dragonwilds 与 Unreal loader 构建校验后复制到各自输出目录，再沿用短路径构建及 Git blob 校验。未设置这些变量时保持原有下载与验证流程。

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

快速反馈以 5–10 分钟为优化目标；冷缓存、依赖和测试规模会影响耗时。此目标不是超时放宽或跳过失败的理由。CI 保存 Rust 依赖及其编译产物，前端只构建一次并传给 Rust；Clippy 已执行编译与类型检查，不再在同一工作流重复 `cargo check`。优化后的耗时须以对应提交的实际 GitHub 运行测量，本地验证通过不能证明已达到该目标。

Tauri 的上下文及嵌入资源由 `build.rs` 的 `CodegenContext` 生成，各宿主入口使用 `tauri_build_context!()`。生成文件必须先于桌面 rustc 编译存在，避免下一次列举或执行测试因资源时间戳变新而重复编译。前端目录、配置与图标的变更仍由 Tauri 构建脚本跟踪；`custom-protocol` 测试核对编译资源与前端产物及 CSP。Cargo fingerprint 日志只在测试步骤开启，避免诊断环境变量改变依赖缓存键。

前端 `*-browser.test.cjs` 属于真实浏览器层，两个既有命名例外集中在共享入口。其他测试以 4 个文件为并发上限；浏览器在每个进程内串行，CI 分四个互不重叠的分片。新增真实浏览器测试遵循命名约定。Rust 的 `app-core`、`app-modules`、`app-knowledge` 用 4 个测试线程；完整 `app-storage` 套件使用 2 个线程，保持各自的临时目录、数据库、端口及按路径归属的故障注入，原有全局写入锁继续生效。涉及 Windows 进程、全局环境及外部边界的其他套件保留串行。

CI 把 Rust 编译检查、核心、存储、进程服务、桌面和原生验收放到独立 Windows runner，不再把慢测试逐项串接。完整桌面套件在 CI 中从 Cargo 元数据确定测试目标，再编译并列举实际测试名，稳定分到四台 runner，通过 Cargo 按精确名称执行；每个测试进程仍为单线程。受管工作站的 Cargo 包装器尚不支持 metadata，因此仅目标发现沿用其支持的预编译路径。分片并集覆盖完整可执行集合，既有 ignored 测试保持原状，doctest 仅在第一片执行。`--component rust-tests --rust-group desktop --tier full --shard-index 1 --shard-count 4 --output <外部目录>` 可复现单片；省略分组仍使用本地连续执行入口。检查、原生与测试编译模式分别缓存，测试缓存仅由桌面第一片写入，避免快速核心组抢占不可更新的共享缓存键。并行会增加 runner 和缓存用量，实际收益须比较同类变更的总耗时、排队和缓存命中。

实例路径检查为所有实例派生实际运行、存档及原生配置归属，并在写入前重新验证冲突；准备外部包前后的检查分别读取当前记录与文件系统。完整路径诊断只为正在查看报告的实例生成，不为其他实例计算无人读取的诊断。Windows 祖先路径的符号链接和 reparse 属性使用同一次 metadata 读取，仍检查全部祖先。比较算法改动时固定测试范围与线程数；调整测试并发时使用同一源码、同一范围单独测量，分别报告编译、测试执行和缓存耗时。

模块发现每次枚举目录并读取当前 manifest 和 schema。原始 manifest 内容完全相同时，在进程内有界复用已成功校验的解析结果；不用时间戳或 TTL 判断文件是否改变，不缓存文件读取错误，schema 每次重新读取。返回的 descriptor 独立于保留的解析结果。实例程序模式与路径由一次同步校验共同返回，后续操作阶段仍重新检查文件与注册归属。

Windows 桌面使用 Tauri 和 WebView2 呈现 React 界面。CI 的 `Desktop UI regression` 在独立 Edge/Chrome 中挂载生产组件，通过模拟 IPC 或受控 HTTP 服务检查 DOM、交互、焦点、布局和订阅清理；它不启动完整 Tauri 应用，也不证明真实数据库写入或游戏进程启停。`Verify native Tauri and WebView2 recovery` 才运行原生宿主及其 WebView2 恢复检查；真实安装、升级和关键开服仍按发行验收执行。普通 React 交互夹具使用锁定 Vite 的实验性 bundled dev 模式，先完成当前 HTML 入口的初始打包，减少数百个开发模块请求的排队。生产通信、原始脚本及未适配打包请求的中间件夹具保留原生 ESM 和可达导入预编译，保证模块 URL、CSP 和加载失败拦截的断言仍有效。带中间件的夹具默认使用 ESM；仅返回场景数据的 React 夹具可以显式启用打包模式。服务器导航夹具也使用打包模式，将锁定 Vite 的真实 lazy 请求映射到源码模块，验证工作区实现未提前请求、访问后确实请求，以及首次 503 后通过独立入口恢复；引导文件的请求不代表工作区实现已经加载。缓存按实际模式隔离。这仅改变测试资源的交付方式，不改变产品构建或交互超时；升级 Vite 时须验证初始构建等待接口、lazy 请求映射及两种模式。相同测试 worker 可复用同一夹具和编译模式的 Vite 缓存，每个场景仍拥有独立浏览器配置、服务和界面状态，worker 结束后回收其缓存。

只执行某层时仍使用同一入口：

```powershell
python -B scripts/verify_project.py --component frontend-tests --tier quick
python -B scripts/verify_project.py --component frontend-browser
python -B scripts/verify_project.py --component frontend-browser --base HEAD~1
python -B scripts/verify_project.py --component rust --tier quick
python -B scripts/verify_project.py --component rust --base HEAD~1 --output 'C:\Verification\LanGame-change'
python -B scripts/verify_project.py --tier full --output 'C:\Verification\LanGame-candidate'
```

已有 npm 入口也复用共享规则：`npm --prefix apps/desktop run verify` 运行前端快速检查和当前源码秘密/公开边界检查，`verify:release` 增加全部浏览器测试和完整公开历史检查。这两个入口均不包含 Rust 或原生桌面验收；CI 的前端分片不重复独立 policy 任务中的公开检查。

完整检查使用新的仓库外证据目录；`runtime/report.json` 和 `desktop/report.json` 记录实际运行结果。`--nsis-root` 可附加运行安装器守卫及语言夹具；没有提供时明确报告未执行，不将其算作安装/升级验收。实际发行要求见[桌面发行指南](desktop-release.md)。

工作区 Cargo 封装可通过 `--cargo-wrapper` 显式传入，Node 资源封装用 `--node-wrapper`。用 `--frontend-output` 指定尚不存在的仓库外构建目录，或以 `--frontend-dist` 复用当前候选版本的已验证前端产物；托管原生验收必须用 `--native-executable` 指向构建回执中的专用 `desktop-reliability` 可执行文件，入口不会猜测共享缓存路径。托管主机应先按本机规则构建前端，再选择 `--component rust` / `native`；公共源码命令不绑定任何工作站路径。

CI 的风险路径在共享入口统一定义，浏览器、Rust 和原生桌面三层独立选择。已核实消费者的局部界面变更运行关键集合和相关完整功能组；关键集合覆盖创建实例、配置控件与恢复、维护保存、实例导航、退出、订阅清理和生产通信。功能组复用现有测试文件，不减少文件内的断言或视口。共享壳、公共 API、全局样式、前端依赖、公共浏览器运行器以及未知归属保留全部浏览器测试。私有夹具通过实际引用定位所属测试；不能确定归属时回退全量。普通前端单测或源码扫描测试变更保留快速层。`frontend-browser` 不带选择参数仍运行完整集合；`--base <commit>` 使用与 CI 相同的规则选择该提交到 `HEAD` 的相关集合，基线无效时回退全量；显式 `--tier full` 始终全量。

Rust 生产变更从各 `Cargo.toml` 的本地依赖关系选择所属包及其全部消费者，包含 target、build 和 dev dependencies；这些包运行完整测试，核心单测和存档保护的快速基线仍保留，同一包不重复运行完整套件。独立测试模块可在文件开头声明 `#![cfg(test)]`，由 Rust 自身保证只参与测试；只改这种文件时验证所属包，不连带触发浏览器和原生桌面验收。仅文件名含 `test` 不足以缩小范围，无法确认归属或删除文件时保守处理。`--base <commit>` 对该提交到 `HEAD` 使用与 CI 相同的选择规则。

Tauri、共享领域契约、游戏模块和迁移等生产边界变更仍运行全部浏览器测试；Tauri、共享领域契约、存储、运行时、Windows 平台和原生夹具变更保留 Tauri/WebView2 验收。共享依赖、构建与检查入口变更、未知归属或无效基线进入全部相关层。CI 分别显示 Rust 格式/编译/Clippy/许可检查与行为测试，按编译模式复用缓存；`rust-checks` 和 `rust-tests` 可单独执行，`rust` 仍完整包含两者。每日回归及手动 `full` 始终运行全部包和浏览器套件，不能被局部选择缩减；显式手动 `quick` 保留快速基线。提交前仍先运行相关行为测试。完整代码验证不创建或发布安装包，也不替代真实安装与开服验收。

公开历史扫描和快照导出由独立 `policy_full` 范围控制：定时、手动完整检查、公开检查实现、排除规则及快照入口变更触发。普通 Rust、界面或工具测试变更仍检查当前源码秘密和公开边界，不因行为测试进入完整层就重复扫描全部 Git 历史。

模拟进程、浏览器组件与真实游戏验收是不同证据；报告实际执行的范围、环境和结果。需要游戏包、外部服务或真实凭据的可选验收应在获准的隔离环境中执行。

真实生命周期使用 ignored 测试 `native_package_lifecycle`，单独以 `--ignored --test-threads=1 --nocapture` 执行。指定 `LANGAME_NATIVE_MODULE_ID` 和现有完整包的 `LANGAME_NATIVE_PACKAGE_ROOT`；测试复制输入，使用隔离数据库和实例，执行真实启动、停止、重启、归档、卸载及数据保留检查。`LANGAME_NATIVE_ACQUISITION=official_acquisition` 增加官方下载与生产创建流程；`LANGAME_NATIVE_PROGRAM_MAINTENANCE=update` 或 `validate` 在旧实例上运行真实更新或文件校验。网络模式使用测试自有 SteamCMD 目录；不指向现有 SteamCMD 或真实实例。跨主机复用测试可执行文件时，使用 `LANGAME_SMOKE_SOURCE_ROOT` 指定该二进制对应的独立源码快照，并记录两者的哈希；不覆盖远端开发工作树。

饥荒地图的真实验收使用 ignored 测试 `smoke_dst_native_world_generation_save_and_reload`。在已隔离的官方包和存储目录中，设置 `LANGAME_DST_SMOKE_SCENARIO=custom_mods`，以 `LANGAME_DST_SMOKE_CUSTOM_MODS_JSON` 提供 1–16 个互不重复的标准十进制工坊 ID 字符串数组；原有命名场景拒绝此覆盖。可选的 `LANGAME_DST_SMOKE_MAP_FIXTURES_JSON` 只在此场景接受 1–8 个互不重复的建筑 prefab 名称（ASCII `[a-z][a-z0-9_]*`，每项最多 128 字节）；两种 JSON 输入各最多 4096 字节。建筑只在首轮生成后由原生游戏布置，第二轮重载不会再次创建；报告绑定原始输入哈希、实体位置及 `test_arranged` 分类，不能据此声称自然生成或玩家制作。夹具不修改原作者 Mod、不创建或移动玩家；真实玩家移动和保存重载仍须分别验证。

饥荒原生验收可通过 `LANGAME_SMOKE_RESOURCE_POLICY` 指定仓库外 JSON 文件，记录可回收资源副本并检查复制容量。未设置时不创建托管登记；一旦设置，缺失或无效策略、越界活动根、重叠根及重解析路径均拒绝运行。策略为 `{"schemaVersion":1,"minimumFreeSpaceBytes":137438953472,"managedRoots":[{"activityRoot":"C:\\BuildScratch\\runs","registryRoot":"C:\\BuildScratch\\resource-registry"}]}`；路径必须为绝对路径，活动目录必须是配置根的严格子目录，登记目录必须位于所有活动根之外。每次复制重新读取容量底线并在文件清点前后查询剩余空间。托管工作站的启动器应显式注入与其实际根、登记消费者和容量规则一致的策略；登记本身不授予删除权限。

真实包校验基准为 ignored 测试 `native_valheim_verification_benchmark_and_three_instance_acceptance`，使用 `--ignored --test-threads=1 --nocapture` 单独执行。`LANGAME_VERIFICATION_FIXTURE_ROOT` 指向已复制的完整 Valheim 包，`LANGAME_VERIFICATION_STEAMCMD_ROOT` 指向含对应 `depotcache` 的独立目录；`LANGAME_SMOKE_RUNTIME_ROOT` 和 `LANGAME_VERIFICATION_REPORT` 必须是仓库外已创建的测试、报告目录。可用 `LANGAME_VERIFICATION_SECOND_FIXTURE_ROOT` 与 `LANGAME_VERIFICATION_SECOND_MODULE` 添加第二个包的计时。测试不会下载、运行 SteamCMD 或启动游戏；它复制输入，在副本中校验、篡改及创建三个实例，成功后清理副本并保留 JSON 报告。基准预热后交错执行四次双重哈希对照与四次当前证据复用，两边均完整执行官方 SHA-1/SHA-256；报告当前构建的本地耗时和 Windows 进程逻辑读取量，不代表旧提交完整二进制、冷磁盘或 SteamCMD 下载/validate 的耗时。

普通 `cargo test` 使用开发测试设置。发行优化设置使用 `cargo bench --locked -p langame-desktop --bin langame-desktop --features custom-protocol native_valheim_verification_benchmark_and_three_instance_acceptance -- --test --ignored --test-threads=1 --nocapture`；默认 bench profile 继承 release，末尾的 `--test` 让优化后的测试程序执行现有验收。报告记录编译时的 `debug_assertions_enabled`；发行对照必须为 `false`，且优化前后使用相同构建，只切换本轮证据复用，避免将 Debug SHA 依赖编译优化混入发行去重收益。托管工作站仍通过规定的 Cargo 封装执行。

仅重跑真实完整性及三实例验收时，选择 `native_valheim_three_instance_integrity_acceptance`，使用相同输入目录；它执行同样的完整哈希、归属及篡改断言，跳过耗时统计，报告中的计时字段为 `null`。

本地桌面 Debug 构建对当前 SHA-1/SHA-256 依赖单独启用 `opt-level=3`，避免多 GiB 原版文件核验被未优化的压缩循环拖慢。算法、校验范围、开发断言、运行时 CPU 能力判断及发行 profile 保持原有设置；测试继承该依赖配置。基准将编译时嵌入的 workspace manifest 摘要及依赖覆盖设置写入报告，比较构建参数时须保留原始对照并重新测量，不能混合统计。

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

游戏翻译的生成目录只保留手写词典未覆盖的 fallback。修改 schema、设置模块或手写词典的键后，运行 `node apps/desktop/scripts/generate_i18n_schema_catalogs.cjs`；单个游戏可追加 `--module <module-id>`。生成器按真实词典合并顺序判断覆盖，不删除前置词典之后仍生效的生成项。校验工具加载实际导出，检查动态词条、最终文案、占位符及中英文覆盖，不以源码字符串扫描代替运行结果。

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

The desktop connects off the UI thread while the interface remains in its loading state. Startup checks service admission before initializing storage. A closing service must exit through its pinned process handle before a replacement starts. The exit helper retains a native event scoped to the user and original process identity before releasing the old interface, covering the gap before shutdown IPC arrives. Exiting during startup cancels waits that have not created a service; an already created service still passes through coordinated shutdown using the original click deadline. Product operations are never replayed against a replacement service.

`app-core` defines shared domain data and `app-modules` reads game manifests and schemas. `app-storage` owns SQLite, instance configuration, saves and backups; `app-runtime` and `app-platform-win` own process lifecycle and Windows integration. `app-steamcmd`, `app-network` and `app-knowledge` provide installation, network policy and knowledge retrieval. Tauri commands coordinate these components. Updates download and verify the package before stopping the runtime service and game processes and invoking the installer.

Every instance passes `commands_runtime_environment` after game updates and before process creation, including pinned game versions. Bounded PE import inspection identifies native dependencies; a finite Microsoft catalog selects installers without evaluating game install scripts. Installed-game metadata determines compatible Java and .NET versions. Verified portable packages publish to separate version directories beside SteamCMD, preserving game programs and runtimes already in use. Preparation failures, failed post-install probes and required Windows restarts block startup. Offline reuse requires a successful runtime probe and an explicit log message. Once a system installer starts, its owner retains the installation lease until the actual process exits before responding to cancellation. Progress and failure reasons use the startup console and existing background jobs.

Real official-package installation, loading and portable preparation use the opt-in [Windows Sandbox acceptance workflow](runtime-prerequisite-validation.md#english); ordinary unit tests do not perform these system installations.

Product commands declare Tauri registration, execution ownership, LAN access and completion policy in `apps/desktop/src-tauri/src/command_registry.rs`. Transport adapters retain argument decoding; internal runtime-service control messages stay outside the product registry. For IPC changes, check frontend arguments and the actual adapters together; contract checks verify registration coverage in both directions. Clients use Tauri camelCase for top-level arguments without sending duplicate snake_case aliases. Nested `input` follows its Rust Serde type; do not rename fields recursively. Desktop-process ownership and named-pipe-only authorization are separate command boundaries.

Storage readiness caches database file identity. The combined runtime overview and instance-details read uses one operation-local database connection, explicitly closed on success and error before configuration reads, log analysis and player queries. Preserve database-replacement detection, readiness checks before reconciliation, transactions and recovery semantics. Fewer connection creations do not establish a measured latency improvement.

Production runtime commands access data through `DesktopState::bootstrap_storage` or the caller's captured storage context, without locating the data root again from process environment. The runtime fixes its initial user locator root, whose saved location points to the selected data directory; these roots are not interchangeable. Each new storage entry still validates the saved location and reads current settings. Caching resolved paths must not bypass missing-directory, settings or database checks.

Storage-context leases coordinate path changes and ongoing operations. Background filesystem work captures the current `StorageBootstrap` or `StoragePaths` and carries its lease through the existing task helpers until the work finishes. The executing task also owns its instance and installation locks, so cancelling the waiter cannot release them early. Background work that must resolve the saved location later captures `StorageSource`, preserving the same user locator root. Authorization, preview confirmation, instance mutation preconditions and dedicated shutdown leases retain their separate responsibilities.

Cross-process installation locking uses the `InstallLockContext` owned by `DesktopState`, passed explicitly through installation, Mod, startup and removal flows. Its lock context stays anchored to the user locator root; deriving another lock root from the selected data directory is invalid. When all user-directory environment variables are absent, installation locks use the storage module's default locator root instead of a separate temporary-directory fallback. Fixtures still depend on process-global environment and Windows processes, so desktop tests remain serial. Environment-change regressions run in isolated child processes and do not justify increasing concurrency for the entire desktop suite.

The server list and runtime overview load directly. Configuration, Mod, player, GM and maintenance workspaces load on first use through the existing recoverable module loader. Maintenance stays mounted after its first visit, uses `active` to suspend background reads, and resets when the selected instance changes, preserving ongoing operations and save results. Check actual build dependencies when adding large catalogs or features; lightweight capability checks must not pull an entire workspace into the initial load.

### Build and run

Use Windows with Git, the MSVC C++ build tools, WebView2, Python 3.11 or later, and the versions declared in [`rust-toolchain.toml`](../rust-toolchain.toml) and the `engines` field of [`apps/desktop/package.json`](../apps/desktop/package.json). Follow the [build and run commands above](#构建与运行), starting the frontend and desktop in separate terminals. Install dependencies from their lock files.

TypeScript 7 provides the production type checker; `@typescript/typescript6` supplies the stable compiler API used for syntax inspection and test fixtures. Keep `windows-core` COM types compatible with `webview2-com`: the current pair is pinned to `0.62.2` / `0.39.1` and must be evaluated together rather than upgraded independently by version number.

For ARK extension builds, `LANGAME_ARK_ARCHIVE_DIR` can point to an existing absolute directory containing `AsaApi-sdk.zip`, `AseApi-sdk.zip`, `asa-runtime.zip` and `AsaApiLoader-sdk.zip`. Official sources and pinned SHA-256 values are defined in `scripts/build_ark_tools.ps1`. Build output must stay outside this read-only input directory; missing or invalid archives stop the build without a network fallback. `LANGAME_PROXY_SOURCE_DIR` similarly accepts a read-only absolute directory containing the 32 pinned source files at the relative paths in `modules/runescapedragonwilds/extensions/proxy-sources.json`. Dragonwilds and Unreal loader builds verify and copy these files into their own output directories, retaining short-path builds and Git blob checks. Leaving these variables unset preserves the normal download and verification flow.

### Change and verification

Keep domain logic in the owning crate and interface adapters small. Bound asynchronous work and provide cancellation, error handling and cleanup. Validate untrusted inputs at storage, process and network boundaries. Review dependency compatibility, licensing and lock-file changes.

Run focused tests, then `python -B scripts/verify_project.py --tier quick`. Local and CI commands share this entry point. Quick checks include frontend unit/contracts, type/build checks, Rust workspace Clippy/compilation, core tests and retained-data regressions. The `full` tier adds all browser and Rust tests, complete publication-history checks, synthetic Windows processes and native Tauri/WebView2 recovery; use a new external `--output` directory.

The existing `npm --prefix apps/desktop run verify` entry runs quick frontend checks and current secret/publication boundaries. `verify:release` adds all browser tests and publication history. Both reuse the shared rules and exclude Rust/native acceptance. CI frontend jobs leave publication checks to the independent policy job.

The complete Rust tier runs core/modules/knowledge with four test threads and the full `app-storage` package with two. Storage fixtures retain separate directories, databases and ports, with path-owned fault injection and the existing global write locks. Other Windows process, environment and external-boundary suites remain serial. CI places compilation checks, core, storage, services, desktop and native acceptance on independent Windows runners. Browser and complete desktop tests each use four disjoint shards. In CI, desktop shards discover targets through Cargo metadata, then compile and enumerate actual test names and execute exact selections through Cargo with one test thread. The managed workstation wrapper does not admit metadata, so target discovery alone retains its supported prebuild path. Existing ignored tests remain ignored and doctests run only in the first shard. Use `--component rust-tests --rust-group desktop --tier full --shard-index 1 --shard-count 4 --output <external-directory>` to reproduce a shard. Omitting the group retains the sequential local entry point.

Clippy, native and test compilation modes use separate caches. Only the first desktop shard writes the shared test cache, preventing a quick core-only run from occupying its immutable key with incomplete dependencies. Parallel jobs trade runner and cache usage for wall-clock time; measure actual runtime, queueing and cache hits. Publication history and snapshot checks have an independent `policy_full` selection for scheduled/full runs and changes to publication tools, exclusion rules or snapshot entry points. Ordinary Rust, UI and test changes retain current-source secret and publication checks without automatically rescanning all Git history.

Tauri context and embedded assets are generated by `CodegenContext` in `build.rs`; each host entry uses `tauri_build_context!()`. Generation must precede desktop rustc compilation so newer asset timestamps do not force another compilation when listing or running tests. Tauri still tracks frontend directories, configuration and icons. The `custom-protocol` regression compares embedded resources with the frontend build and checks CSP preservation. Cargo fingerprint logging is scoped to the test step so diagnostic environment variables do not change dependency cache keys.

Instance path checks derive runtime, save and native configuration ownership for every instance and revalidate conflicts before writes. Checks before and after external package preparation independently read current records and filesystem state. Full path diagnostics are produced only for the instance whose report is requested. Windows ancestor checks reuse one metadata observation for symlink and reparse attributes while still checking every ancestor. Compare algorithm changes with a fixed test scope and thread count. Measure concurrency changes separately on the same source and test scope, reporting compilation, test execution and cache costs separately.

Module discovery enumerates directories and reads the current manifest and schema on every call. A bounded process-local store reuses successfully validated parsing results only when the complete original manifest content is identical; timestamps and TTLs do not determine freshness. File-read errors are not retained, schemas are read anew, and returned descriptors are independent of retained parsing results. Instance program mode and path come from one synchronous validation pass; subsequent operation phases still recheck files and registered ownership.

The Windows desktop renders React through Tauri and WebView2. CI's `Desktop UI regression` mounts production components in isolated Edge/Chrome sessions with mock IPC or controlled HTTP services to check DOM behavior, focus, layout and subscription cleanup. It does not start the complete Tauri app or verify real database writes and game processes. `Verify native Tauri and WebView2 recovery` runs the native host and WebView2 recovery checks; actual installation, upgrade and hosting flows remain release acceptance requirements. Ordinary React interaction fixtures use the locked Vite version's experimental bundled dev mode, preparing the current HTML entry's initial bundle to avoid hundreds of queued development-module requests. Production transport, raw-script and middleware fixtures without bundled-request handling retain native ESM and reachable-import preparation so module-URL, CSP and load-failure assertions remain meaningful. Middleware defaults to ESM; React fixtures serving only scenario data may explicitly opt into bundled mode. The server navigation fixture also bundles, mapping the locked Vite version's real lazy requests to source modules to verify that workspace implementations are requested only after navigation and recover from an initial 503 through a separate entry; requesting an entry stub does not mean its workspace implementation has loaded. Caches are separated by the actual compilation mode. This changes test delivery only, preserving product builds and interaction deadlines; Vite upgrades must verify the initial-build wait interface, lazy-request mapping and both modes. A worker can reuse compilation caches for the same fixture and compilation mode while every scenario retains its own browser profile, server and UI state; the worker removes its cache on completion.

CI selects browser, Rust and native desktop regression independently. Local UI changes with verified consumers select eight critical browser files plus the affected complete behavioral families. The critical set covers creation, configuration and recovery, maintenance saves, navigation, exit, subscription cleanup and production transport. Selection retains every case and viewport in each chosen file. Shared shell/API/style/dependency changes, common browser infrastructure and unknown ownership retain all browser tests. Private fixtures select their actual consumers, falling back to all tests when ownership is unclear. Ordinary unit/scanner changes retain quick feedback. `--component frontend-browser` without a selection still runs every browser file; adding `--base <commit>` uses the same selection rules as CI for that commit through `HEAD`, falling back to the complete suite for an invalid base. An explicit `--tier full` always runs the complete suite.

Rust production changes select the owning package and all consumers from the actual local Cargo dependencies, including target, build and dev dependencies. Selected packages run complete suites while retaining the quick core and saved-data baseline; no package runs its complete suite twice. Independent test modules may start with Rust's `#![cfg(test)]` attribute. Changes limited to these modules select their owning package without browser or native desktop acceptance. A test-like filename alone is insufficient; deleted files and unknown ownership retain conservative selection. `--base <commit>` applies the same selection to that commit through `HEAD` locally.

Production changes to Tauri, shared contracts, game modules and migrations retain all browser tests. Tauri, shared contracts, storage, runtime, Windows integration and native fixtures retain native Tauri/WebView2 acceptance. Shared dependency, build and verification changes or an invalid baseline select all relevant layers. CI displays Rust format/compile/Clippy/license checks separately from behavior tests, with caches scoped by compilation mode. `rust-checks` and `rust-tests` expose these steps; `rust` includes both. Daily regression and manual `full` always run all packages and browser tests; explicit manual `quick` retains the quick baseline. Complete code verification neither publishes installers nor replaces installation and hosting acceptance.

Rust dependency caching and a shared frontend artifact avoid repeated preparation. Measure the 5–10 minute quick-feedback target using actual GitHub runs of the corresponding commit; passing local checks does not establish that the target has been reached. Use `--plan`, `--component`, `--cargo-wrapper`, `--node-wrapper`, `--frontend-output`, `--frontend-dist` and `--native-executable` as described above for focused or managed execution. Preserve existing assertions and failure handling. Real signed installation, upgrade and retained-data acceptance remain separate release requirements; optional NSIS guard checks require `--nsis-root`. Report synthetic, browser and native-game evidence separately. Opt-in tests requiring game packages, external services or credentials need an authorized isolated environment.

Follow [Game Integration Validation](game-integration-validation.md#english) for module changes. Maintain `config-sources.toml` and sanitized fixtures, then use the [generation and check commands](#模块与生成文件); do not edit generated ledgers or acceptance Markdown directly. Tauri's generated schemas are ignored output; edit the source capability or Tauri configuration instead.

Run the ignored `native_package_lifecycle` test alone with `--ignored --test-threads=1 --nocapture`. Set `LANGAME_NATIVE_MODULE_ID` and `LANGAME_NATIVE_PACKAGE_ROOT` to an existing complete package. The fixture copies its input and uses an isolated database and instances for real start, stop, restart, archive, uninstall and data-preservation checks. `LANGAME_NATIVE_ACQUISITION=official_acquisition` adds official downloads and production instance creation; `LANGAME_NATIVE_PROGRAM_MAINTENANCE=update` or `validate` exercises real maintenance of the existing test instance. Network modes use fixture-owned SteamCMD storage. To reuse a test executable on another host, set `LANGAME_SMOKE_SOURCE_ROOT` to an independent matching source snapshot and retain both hashes; leave the host's development checkout intact.

The ignored `smoke_dst_native_world_generation_save_and_reload` test accepts arbitrary Workshop samples through `LANGAME_DST_SMOKE_SCENARIO=custom_mods` and `LANGAME_DST_SMOKE_CUSTOM_MODS_JSON`, a JSON array of 1–16 unique canonical decimal Workshop ID strings. Use isolated official package copies and evidence storage; existing named scenarios reject this override. Optional `LANGAME_DST_SMOKE_MAP_FIXTURES_JSON` accepts 1–8 unique building prefab names only in this scenario (ASCII `[a-z][a-z0-9_]*`, at most 128 bytes each); each JSON input is limited to 4096 bytes. The native game places these structures only after the first generation, never again on reload. Reports bind the exact input hashes, entity positions and `test_arranged` classification; this is not evidence of natural generation or player crafting. The fixture does not change author Mods or create or move players. Real-player movement and save/reload still require separate verification.

For the DST native smoke test, `LANGAME_SMOKE_RESOURCE_POLICY` optionally names an external JSON file for disposable-copy provenance and capacity admission. Without it, no managed registration is created. Once configured, missing or invalid policy, out-of-scope activity roots, overlapping roots and reparse paths fail closed. Use schema `{"schemaVersion":1,"minimumFreeSpaceBytes":137438953472,"managedRoots":[{"activityRoot":"C:\\BuildScratch\\runs","registryRoot":"C:\\BuildScratch\\resource-registry"}]}`. Paths must be absolute; each activity must be a strict child of its configured root, and registries must be outside every activity root. Each copy rereads the capacity floor and queries free space before and after inventory. Managed workstation launchers must explicitly supply a policy matching their actual roots, registry consumer and capacity rules. Registration alone does not authorize deletion.

The ignored `native_valheim_verification_benchmark_and_three_instance_acceptance` test runs alone with `--ignored --test-threads=1 --nocapture`. Point `LANGAME_VERIFICATION_FIXTURE_ROOT` to a copied complete Valheim package and `LANGAME_VERIFICATION_STEAMCMD_ROOT` to an isolated directory containing its exact `depotcache`. Create external directories for `LANGAME_SMOKE_RUNTIME_ROOT` and `LANGAME_VERIFICATION_REPORT`. Optional `LANGAME_VERIFICATION_SECOND_FIXTURE_ROOT` and `LANGAME_VERIFICATION_SECOND_MODULE` add timing for another package. The test copies its inputs, verifies and tampers with owned copies, and creates three instances without downloading, running SteamCMD or starting games. Successful runs remove owned copies and retain JSON reports. After warmup it interleaves four full second-hash controls and four current-evidence reuse samples; both perform complete official SHA-1/SHA-256 verification. Measurements describe the current build's local wall time and Windows logical process reads, not an old-commit binary, cold disk or native download/validate timing.

Ordinary `cargo test` uses development test settings. For release optimization, run `cargo bench --locked -p langame-desktop --bin langame-desktop --features custom-protocol native_valheim_verification_benchmark_and_three_instance_acceptance -- --test --ignored --test-threads=1 --nocapture`. The default bench profile inherits release; the final `--test` executes the existing acceptance in the optimized test harness. Reports record the compiled `debug_assertions_enabled` value. Release comparisons require `false` and the same build in both modes, switching only current-operation evidence reuse rather than mixing in the Debug SHA dependency overrides. Managed workstations still use their required Cargo wrapper.

Select `native_valheim_three_instance_integrity_acceptance` with the same inputs to repeat full hashes, ownership and tamper assertions without repeating timing. Its report uses `null` timing fields.

Local debug builds optimize only the current SHA-1/SHA-256 dependencies at level 3 for multi-GiB package verification. Algorithms, verification coverage, development assertions, runtime CPU dispatch and release profiles keep their existing settings; tests inherit the dependency overrides. Timing reports embed the workspace manifest hash and package overrides from compilation. Preserve original controls and measure build-parameter changes separately.

Generated game translations retain only fallback keys not overridden by hand-authored catalogs. After changing schema, settings modules or authored message keys, run `node apps/desktop/scripts/generate_i18n_schema_catalogs.cjs`; `--module <module-id>` limits schema regeneration to one game. Generation respects runtime spread order. Verification loads actual exports, including computed messages, to check effective values, placeholders and language coverage.

Preserve retained data during schema changes and verify backups, conversion, integrity and actual startup. Never bypass validation by deleting data or rewriting migration checksums. Keep credentials, private addresses, player data, runtime output, proprietary binaries and unlicensed media out of commits. Preserve third-party notices and use the [private security reporting process](../SECURITY.md#english). Run the [publication-boundary checks](#数据与公开边界); history checks require a complete clone. Source checks do not replace installation, upgrade, uninstall or data-retention acceptance.
