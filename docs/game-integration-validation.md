# Game Integration Validation / 游戏接入验证

[English](#english) | [简体中文](#简体中文)

## English

This document defines the stable admission and verification process for a new or changed LanGame Server Manager game module. It applies to installation, native configuration, process lifecycle, ports, backups, server queries, join descriptors, and player or administrator operations.

Generated acceptance records show the current evidence state of a module; they are not a substitute for this admission process.

### Admission gates

A module is ready for review only when all applicable gates are satisfied:

1. **Native Windows server:** an identifiable dedicated-server package runs natively on Windows. A client-hosted session, Wine-only path, or unverified executable is not sufficient.
2. **Lawful acquisition:** the package has an authoritative acquisition source and documented redistribution limits. The repository does not contain proprietary binaries or game assets.
3. **Reproducible operating contract:** installation, executable selection, startup arguments, configuration files, save paths, ports, update behavior, graceful stop, and backup ownership are known or explicitly marked unsupported.
4. **Evidence-backed settings:** each schema field maps to a verified native key, launch argument, generated file, or derived value. Native surfaces that are intentionally not modeled have a recorded reason.
5. **Bounded lifecycle:** process ownership, readiness, timeouts, cancellation, failure reporting, and cleanup are defined. Retries and polling are bounded.
6. **Capability-specific proof:** query, join, player, administrator, mod, and workshop operations are enabled independently and only when their host-callable behavior is verified.
7. **Public-safe evidence:** fixtures and records are deterministic and sanitized. They contain no credentials, player data, private addresses, machine-specific absolute paths, private logs, or unlicensed media.

### Evidence hierarchy

Use the strongest available evidence, in this order:

1. Publisher or developer documentation, specifications, release notes, and official support statements.
2. Official distribution metadata, dedicated-server package manifests, and public branch metadata.
3. A reproducible probe of a lawfully acquired dedicated-server package or running server.
4. Maintained community material only as a lead or corroboration; it cannot be the sole basis for a support claim when first-party evidence is available.

Each source entry in `config-sources.toml` records a stable source ID, kind, authority, URL or portable path, and a concise description of what it proves. Record the relevant build or release context and verification date in the ledger or acceptance fixture. Do not use a workstation path as evidence; describe the package-relative artifact or reproducible acquisition command instead.

Use only the evidence statuses accepted by the repository verifier:

- `exhaustive_verified`
- `best_effort_verified`
- `blocked_upstream`
- `verified_with_direct_connection`
- `download_verified_requires_elevation`

A status describes evidence strength or an upstream constraint. It must not be used to claim behavior that was not observed.

### Required module artifacts

| Artifact | Responsibility |
| --- | --- |
| `modules/<module-id>/module.toml` | Identity, Windows support, acquisition, process plan, default ports, paths, lifecycle, and optional runtime capabilities |
| `modules/<module-id>/schema.json` | Typed operator settings, defaults, limits, secrets, sections, and presentation metadata |
| `modules/<module-id>/config-sources.toml` | Provenance for every modeled native setting and every deliberate exclusion |
| `modules/<module-id>/templates/` | Native configuration materialized from validated settings |
| `modules/<module-id>/config-fixtures/*.json` | Sanitized inputs, evidence references, classifications, lifecycle expectations, and native output assertions |
| `modules/<module-id>/smoke.toml` | Optional, evidence-backed real-server smoke probes |

Modify these source artifacts rather than the generated documents under `docs/game-config-acceptance/` or the generated source ledger.

### Implementation and review flow

1. **Establish the package boundary.** Verify the official server package, Windows executable, architecture, prerequisites, install/update command, and license constraints.
2. **Model the runtime.** Define paths, port roles, process arguments, environment, readiness, stop behavior, backup roots, and resource ownership.
3. **Model configuration.** Enumerate native settings from authoritative evidence. Map supported items into the schema and templates; document exclusions instead of guessing.
4. **Create acceptance fixtures.** Use safe relative paths and non-secret values. Assert the exact native files and launch arguments produced from the settings.
5. **Add optional capabilities separately.** Query, join, player actions, mods, and workshop support each require their own protocol and lifecycle evidence.
6. **Generate records.** Regenerate provenance and acceptance documents from the reviewed sources.
7. **Verify and review.** Run deterministic checks, inspect the complete diff, and record the sanitized test environment and any real-server smoke result.

### Join and player capability rules

A `runtime.join` profile is optional. The current `steam_connect` profile is valid only when:

- `client_app_id` is greater than zero;
- `join_port_name` and `query_port_name` are non-empty names declared in `default_ports`;
- the join port belongs to the runtime player port role;
- the query port is a candidate of `runtime.player_query`;
- the query protocol is `a2s_info`; and
- the default query binding uses UDP.

At runtime, the LanGame LAN Directory sender additionally requires one unique, non-zero actual binding for each named port, a UDP query binding, and a wildcard bind address or a bind address equal to the datagram source address. Otherwise it publishes `join: null`. See [LanGame LAN Directory Protocol v2](lan-directory-protocol-v2.md).

Player counts, rosters, moderation, allowlists, bans, and administrator actions are separate capabilities. A game-client screen or documented client command does not prove that the dedicated server exposes a stable, host-callable interface. When no such interface is verified, keep the capability absent and record the evidence gap without adding a compatibility stub.

### Generate and verify

Run generation only after changing module evidence or generator sources:

~~~powershell
python -B scripts/verify_game_config_provenance.py --write
python -B scripts/verify_game_config_acceptance.py --require-all --write
~~~

These maintenance scripts own specific module sources and fixture sets:

| Script | Inputs and generated files |
| --- | --- |
| [`apply_barotrauma_native_settings.py`](../scripts/apply_barotrauma_native_settings.py) | Pinned Barotrauma reference XML → schema fields, provenance mappings and the native XML template |
| [`generate_ark_acceptance_fixtures.py`](../scripts/generate_ark_acceptance_fixtures.py) | ARK crosswalk, module schemas and provenance → ASE and ASA configuration and launch fixtures |
| [`generate_native_inventory_acceptance_fixtures.py`](../scripts/generate_native_inventory_acceptance_fixtures.py) | Module specifications and provenance → fixtures for Core Keeper, Enshrouded, Minecraft, Necesse, Project Zomboid, Sons of the Forest and V Rising |
| [`generate_native_output_acceptance_fixtures.py`](../scripts/generate_native_output_acceptance_fixtures.py) | Module specifications, schemas and provenance → fixtures for Barotrauma, Rust, 7 Days to Die, Squad and Valheim |
| [`generate_scum_acceptance_fixture.py`](../scripts/generate_scum_acceptance_fixture.py) | SCUM provenance and fixture definitions → configuration, launch and lifecycle fixture; `--check` compares the current file |

Run only the relevant script when its inputs change. These scripts overwrite
their owned source or fixture files; review the complete diff, preserve unrelated
assertions, and verify the affected native behavior before regenerating records.

Then run the deterministic gates from the repository root:

~~~powershell
python -B scripts/verify_game_config_provenance.py --check
python -B scripts/verify_module_setting_coverage.py
python -B scripts/verify_game_config_acceptance.py --require-all --check
python -B scripts/verify_library_media.py
npm --prefix apps/desktop run verify:configuration-workspace
npm --prefix apps/desktop run verify:configuration-regressions
~~~

If Rust behavior changes, also run:

~~~powershell
cargo check --workspace --all-targets --all-features --locked
cargo test --workspace --all-features --locked
~~~

Run the broader `npm --prefix apps/desktop run verify` gate before submission. A real dedicated-server smoke run is required when the claim depends on installation, process, network, save, update, or administration behavior that deterministic fixtures cannot prove. Publish only the sanitized result and evidence classification; never publish the server package, credentials, private logs, or player data.

### Review checklist

- [ ] The module represents a native Windows dedicated server and has a lawful acquisition path.
- [ ] Every support claim cites evidence at the correct authority and build context.
- [ ] Every schema field and exclusion is represented in `config-sources.toml`.
- [ ] Fixtures assert native output and use only portable, sanitized data.
- [ ] Ports, paths, lifecycle, cancellation, timeouts, and backup ownership are explicit.
- [ ] Optional capabilities are absent unless their independent evidence is complete.
- [ ] Generated documents were regenerated from sources and pass check mode.
- [ ] The pull request records the test environment, risk, rollback, and any operator action.

## 简体中文

本文规定 LanGame Server Manager 新增或修改游戏模块时使用的稳定准入与验证流程，适用于安装、原生配置、进程生命周期、端口、备份、服务器查询、加入描述以及玩家与管理员操作。

生成的验收记录用于说明模块当前的证据状态，不能替代本文规定的准入流程。

### 准入门槛

模块进入评审前，必须满足所有适用条件：

1. **Windows 原生服务器：**存在可识别、可在 Windows 原生运行的专用服务器程序包。客户端开服、仅支持 Wine 的路径或未经验证的可执行文件均不满足要求。
2. **合法获取：**程序包有权威获取来源和明确的再分发限制。仓库不得包含专有二进制文件或游戏资产。
3. **可复现的运行契约：**安装、可执行文件选择、启动参数、配置文件、存档路径、端口、更新、正常停止和备份归属已经确认；无法支持的内容必须明确标注。
4. **由证据支持的设置：**每个 Schema 字段都映射到已验证的原生键、启动参数、生成文件或派生值。主动不建模的原生能力必须记录原因。
5. **有边界的生命周期：**明确进程所有权、就绪条件、超时、取消、失败报告和清理方式；重试与轮询必须有上限。
6. **能力分别取证：**查询、加入、玩家、管理员、模组和创意工坊操作应分别启用，且必须先验证服务器侧可调用行为。
7. **可公开的证据：**夹具和记录应确定、已脱敏，不含凭据、玩家数据、私有地址、本机绝对路径、私有日志或未经许可的媒体。

### 证据优先级

按以下顺序使用最强证据：

1. 发行方或开发者文档、规范、发布说明及官方支持声明。
2. 官方分发元数据、专用服务器程序包清单与公开分支元数据。
3. 对合法获取的专用服务器程序包或运行中服务器执行的可复现探测。
4. 维护良好的社区资料仅可作为线索或旁证；存在第一方证据时，不得将其作为支持结论的唯一依据。

`config-sources.toml` 中的每个来源条目应记录稳定的来源 ID、类型、权威主体、URL 或可移植路径，以及该来源能够证明的事实。相关构建或发布上下文与验证日期应写入来源台账或验收夹具。不得使用工作站路径作为证据；应改为程序包内相对路径或可复现的获取命令。

证据状态只能使用仓库校验器接受的值：

- `exhaustive_verified`
- `best_effort_verified`
- `blocked_upstream`
- `verified_with_direct_connection`
- `download_verified_requires_elevation`

状态用于描述证据强度或上游限制，不得用于宣称未观察到的行为。

### 必需的模块产物

| 产物 | 职责 |
| --- | --- |
| `modules/<module-id>/module.toml` | 模块标识、Windows 支持、获取方式、进程计划、默认端口、路径、生命周期与可选运行能力 |
| `modules/<module-id>/schema.json` | 带类型的运维设置、默认值、限制、密钥、分区和展示元数据 |
| `modules/<module-id>/config-sources.toml` | 每个已建模原生设置及每个主动排除项的来源 |
| `modules/<module-id>/templates/` | 由已验证设置生成的原生配置 |
| `modules/<module-id>/config-fixtures/*.json` | 已脱敏输入、证据引用、分类、生命周期预期和原生输出断言 |
| `modules/<module-id>/smoke.toml` | 可选、由证据支持的真实服务器烟测探针 |

应修改上述源文件，不要直接修改 `docs/game-config-acceptance/` 下的生成文档或生成的来源台账。

### 实施与评审流程

1. **确认程序包边界。**验证官方服务器程序包、Windows 可执行文件、体系结构、前置条件、安装与更新命令及许可证限制。
2. **建立运行模型。**定义路径、端口角色、进程参数、环境、就绪条件、停止行为、备份根目录和资源所有权。
3. **建立配置模型。**根据权威证据列举原生设置，将受支持项映射到 Schema 与模板；无法确认的内容应记录为排除项，不得猜测。
4. **创建验收夹具。**使用安全的相对路径和非敏感值，断言设置生成的准确原生文件与启动参数。
5. **分别增加可选能力。**查询、加入、玩家操作、模组和创意工坊支持分别需要协议与生命周期证据。
6. **生成记录。**根据已评审源文件重新生成来源和验收文档。
7. **验证并评审。**执行确定性检查，检查完整差异，并记录已脱敏的测试环境和真实服务器烟测结果。

### 加入与玩家能力规则

`runtime.join` 配置为可选能力。当前 `steam_connect` 配置仅在以下条件全部满足时有效：

- `client_app_id` 大于零；
- `join_port_name` 与 `query_port_name` 是非空名称，且均在 `default_ports` 中声明；
- 加入端口属于运行时玩家端口角色；
- 查询端口属于 `runtime.player_query` 的候选端口；
- 查询协议为 `a2s_info`；
- 默认查询绑定使用 UDP。

运行时，LanGame 局域网目录发送端还要求两个命名端口分别只有一个非零实际绑定、查询绑定使用 UDP，且实例绑定地址为通配地址或与数据报源地址一致。否则发送端会发布 `join: null`。详情见 [LanGame 局域网目录协议 v2](lan-directory-protocol-v2.md)。

玩家数量、玩家列表、处罚、允许名单、封禁与管理员操作属于相互独立的能力。游戏客户端界面或客户端命令文档不能证明专用服务器提供稳定的服务器侧调用接口。无法验证该接口时，应保持能力缺失并记录证据缺口，不得添加兼容占位实现。

### 生成与验证

仅在模块证据或生成器源文件发生变化后执行生成命令：

~~~powershell
python -B scripts/verify_game_config_provenance.py --write
python -B scripts/verify_game_config_acceptance.py --require-all --write
~~~

以下维护脚本分别负责特定模块源文件和夹具：

| 脚本 | 输入与生成文件 |
| --- | --- |
| [`apply_barotrauma_native_settings.py`](../scripts/apply_barotrauma_native_settings.py) | 固定版本的 Barotrauma 参考 XML → Schema 字段、来源映射及原生 XML 模板 |
| [`generate_ark_acceptance_fixtures.py`](../scripts/generate_ark_acceptance_fixtures.py) | ARK 对照表、模块 Schema 与来源台账 → ASE、ASA 配置及启动夹具 |
| [`generate_native_inventory_acceptance_fixtures.py`](../scripts/generate_native_inventory_acceptance_fixtures.py) | 模块规格与来源台账 → Core Keeper、Enshrouded、Minecraft、Necesse、Project Zomboid、Sons of the Forest、V Rising 夹具 |
| [`generate_native_output_acceptance_fixtures.py`](../scripts/generate_native_output_acceptance_fixtures.py) | 模块规格、Schema 与来源台账 → Barotrauma、Rust、7 Days to Die、Squad、Valheim 夹具 |
| [`generate_scum_acceptance_fixture.py`](../scripts/generate_scum_acceptance_fixture.py) | SCUM 来源台账与夹具定义 → 配置、启动及生命周期夹具；`--check` 比较现有文件 |

仅在对应输入变化时运行相关脚本。脚本会覆盖其负责的源文件或夹具；重新生成记录前，
应审查完整差异、保留无关断言，并验证受影响的原生行为。

随后在仓库根目录执行确定性门禁：

~~~powershell
python -B scripts/verify_game_config_provenance.py --check
python -B scripts/verify_module_setting_coverage.py
python -B scripts/verify_game_config_acceptance.py --require-all --check
python -B scripts/verify_library_media.py
npm --prefix apps/desktop run verify:configuration-workspace
npm --prefix apps/desktop run verify:configuration-regressions
~~~

Rust 行为发生变化时，还应执行：

~~~powershell
cargo check --workspace --all-targets --all-features --locked
cargo test --workspace --all-features --locked
~~~

提交前执行完整的 `npm --prefix apps/desktop run verify` 门禁。如果支持结论依赖安装、进程、网络、存档、更新或管理行为，且确定性夹具无法证明，则必须执行真实专用服务器烟测。公开内容仅限已脱敏结果和证据分类，不得公开服务器程序包、凭据、私有日志或玩家数据。

### 评审清单

- [ ] 模块对应可在 Windows 原生运行的专用服务器，并有合法获取路径。
- [ ] 每项支持结论均引用正确权威级别和构建上下文的证据。
- [ ] 每个 Schema 字段和排除项均记录在 `config-sources.toml` 中。
- [ ] 夹具断言原生输出，且只使用可移植、已脱敏的数据。
- [ ] 端口、路径、生命周期、取消、超时和备份归属均有明确定义。
- [ ] 可选能力仅在独立证据完整后启用。
- [ ] 生成文档来自源文件，且通过检查模式。
- [ ] Pull Request 已记录测试环境、风险、回滚方式和所需运维操作。
