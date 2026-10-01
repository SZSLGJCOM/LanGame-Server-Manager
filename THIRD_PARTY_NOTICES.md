# Third-Party Notices

This file describes third-party documentation, data, dependencies and runtime components. The exact redistributed font files and their evidence are recorded separately in [`THIRD_PARTY_ASSETS/NOTICE`](THIRD_PARTY_ASSETS/NOTICE). Tauri installs both this overview and that ledger as resources.

The notices below identify known upstream terms; they do not provide legal advice, transfer upstream rights, or guarantee that a particular downstream use is permitted.

Installed resource copies use `THIRD_PARTY_LICENSES-RUST.txt`, `THIRD_PARTY_LICENSES-NPM.txt`, `fonts/inter/OFL.txt`, `THIRD_PARTY_ASSETS/NOTICE` and this `THIRD_PARTY_NOTICES.md`. Repository-relative links elsewhere in this document refer to the source tree, not necessarily the installed resource layout. Native model/runtime and plugin notices accompany the components as described below.

## Distribution boundaries

| Material | Current delivery | Applicable boundary |
| --- | --- | --- |
| Inter 4.1 fonts | Source and frontend bundle, with `fonts/inter/OFL.txt` | Retain OFL and attribution; verify the exact files against the asset ledger. |
| Project icons and Minecraft fallback illustration | Source and frontend/icon build inputs | Separate terms in `NOTICE`; project creation does not transfer third-party game or trademark rights. |
| Remote game covers, screenshots and trailers | URL records in source; fetched and cached during use | Public availability and an approved network origin do not establish permission for display, caching, LAN delivery or commercial use. |
| Game knowledge pages | Source metadata in modules; text retrieved into local runtime storage | Publisher terms apply to copying and reuse; access controls and robots signals do not establish complete copyright permission. |
| IBM model | Downloaded on demand with license and attribution | Apache-2.0 as declared for the pinned model; weights are not in the repository. |
| ONNX Runtime and MSVC CRT | Embedded during Windows build, then extracted with their notices | Separate native-component licenses and redistribution conditions apply. |
| SteamCMD and proprietary game servers | Obtained through their configured upstream channels | Not relicensed by LanGame; do not add runtime installations or caches to a source snapshot or installer. |

The Steam/Microsoft media links in `apps/desktop/src/data/module-store-data.json` and the network list in `crates/app-network/official-sources.json` record origins, not grants of media rights. Consult the applicable game publisher's terms, the [Steam API terms](https://steamcommunity.com/dev/apiterms) for data obtained through the covered APIs, and the [Minecraft Usage Guidelines](https://www.minecraft.net/en-us/usage-guidelines) for Minecraft material. An API grant must not be assumed to cover every store endpoint or linked third-party asset. Before public distribution, record the basis for the actual uses, obtain permission where needed, or replace/remove the affected material.

Module knowledge-source `license_note` and `license_url` fields record upstream terms. The current retrieval pipeline does not interpret them as an automatic license decision. `reference_only` is an explicit source setting; robots and content signals impose separate technical restrictions. In particular, ARK Wiki's noncommercial/share-alike terms and RimWorld Together's repository-documentation noncommercial/no-derivatives terms cannot be overridden by a commercial-use authorization from LanGame. Review covered text and intended reuse separately; do not describe all fetched pages as cleared for commercial use or AI reuse.

## ARK Wiki server-configuration material

- Upstream contributor: ARK Wiki contributors.
- Source page: <https://ark.wiki.gg/wiki/Server_configuration>
- Upstream copyright notice: <https://ark.wiki.gg/wiki/ARK_Wiki:Copyrights>
- Declared license: Creative Commons Attribution-NonCommercial-ShareAlike 4.0 International (`CC BY-NC-SA 4.0`).
- License text: <https://creativecommons.org/licenses/by-nc-sa/4.0/>
- Preserved private-source path: `docs/game-config-acceptance/sources/ark-server-configuration-2026-07-13.wikitext`.
- Public project records: `docs/game-config-acceptance/ark-official-crosswalk-2026-07-13.json`, `modules/arksurvivalascended/config-sources.toml`, and `modules/arksurvivalevolved/config-sources.toml`.

The complete raw page and description fields are excluded from the public snapshot. The public crosswalk retains configuration identifiers, types, values, availability and version facts, source locations, and project-specific classifications. Explanatory type fields are normalized to technical type/list facts; edition-specific defaults retain their values without Wiki presentation markup. It also identifies the source URL, verification date, content digest and upstream license. Schema generation does not import upstream explanations; project-authored help is maintained separately. This notice does not assert that every element of the retained records is independently free of upstream rights.

The public `scripts/audit_ark_official_settings.py` tool therefore requires the raw wikitext as an explicit local argument and verifies its pinned SHA-256 digest before processing it. The source URL and expected digest are printed in the command help; the tool does not download or redistribute the page. Project-authored English and Chinese field help is maintained in [`scripts/ark_field_descriptions.json`](scripts/ark_field_descriptions.json). Refresh that help with `python -B scripts/apply_ark_official_crosswalk.py --descriptions-only`, then regenerate its catalogs with `node apps/desktop/scripts/generate_i18n_schema_catalogs.cjs --module arksurvivalascended --module arksurvivalevolved`.

The repository's LanGame Source-Available License 1.0 applies to project-authored code and original portions of those records only. It does not replace the upstream license or grant rights in upstream text, game names, or trademarks. Its restrictions on commercial use and independent modified releases do not override rights independently granted for covered material under CC BY-NC-SA. Redistributors should review the upstream terms for their intended use, including applicable attribution, noncommercial and share-alike conditions.

## Return to Moria configuration facts

[`modules/returntomoria/reference-configs/server-defaults-build-21872765.ini`](modules/returntomoria/reference-configs/server-defaults-build-21872765.ini) is a project-maintained fixture derived from the recorded first launch of Steam public build `21872765` on `2026-07-13`. It retains the six section names and 25 key/value pairs used to verify the integration, without upstream explanatory comments. Normalization does not establish a new game run or a later verification of the build.

The [source ledger](modules/returntomoria/config-sources.toml) records the pre-normalization sample's SHA-256 separately from the distributed LF-normalized fixture's SHA-256. The former identifies the source sample used for normalization, not the original game-generated bytes or a file distributed by this repository. The [publisher's dedicated-server guide](https://www.returntomoria.com/news-updates/dedicated-server) remains the source for operator explanations. This record does not grant rights to publisher prose, game assets, names, or trademarks.

## Local multilingual embedding model

LAN uses IBM's [Granite Embedding 97M Multilingual R2](https://huggingface.co/ibm-granite/granite-embedding-97m-multilingual-r2/blob/835ad14087e140460703cf0fae09f97d469d65c2/README.md), pinned to revision `835ad14087e140460703cf0fae09f97d469d65c2`. The publisher declares Apache License 2.0. LAN downloads the publisher's unmodified `onnx/model_quint8_avx2.onnx`, tokenizer and configuration, checks their embedded SHA-256 digests, and stores the complete license and model attribution as `LICENSE.txt`. Model weights are not distributed in this repository.

Inference uses the `ort` Rust binding and Microsoft's official CPU-only ONNX Runtime 1.30.0. A checksum-pinned official archive supplies the native libraries, MIT license and full third-party notices at build time. Stable MSVC Build Tools supplies unmodified retail x64 CRT files under Microsoft's [Distributable Code terms](https://learn.microsoft.com/en-us/visualstudio/releases/2026/redistribution). The executable embeds these files and their manifest; it materializes a verified offline runtime beside the model cache. A compatible, already loaded System32 CRT may be used under the Windows system trust boundary. Rust dependency notices are included in the generated inventory below; native ONNX Runtime and CRT notices accompany the extracted runtime.

## Dependency notices

The current native-runtime build selects the latest complete stable MSVC installation, while its generated CRT notice points to the Visual Studio 2026 redistribution list. A release must verify that this list and the applicable Build Tools license cover the actual supplying installation, and correct the version-specific notice if needed. A Microsoft signature and matching hashes establish file identity, not redistribution rights.

ARK creature tools use the MIT-licensed ArkServerApi ASE/ASA SDKs and ASA version-loader export stubs. The build pins their source archives and verifies SHA-256; the instance installer independently verifies the released runtime framework. Sources, revisions and checksums are recorded in [`modules/ark-tools/SOURCE.md`](modules/ark-tools/SOURCE.md). The complete incorporated upstream notices are in [`modules/ark-tools/THIRD_PARTY_NOTICES.txt`](modules/ark-tools/THIRD_PARTY_NOTICES.txt) and are installed beside the plugin. These third-party terms do not replace the LanGame project license.

Rust and npm lock files provide the dependency inventory used by this source tree. Each dependency remains subject to its own license and notice files.

The checked-in [`apps/desktop/public/THIRD_PARTY_LICENSES.txt`](apps/desktop/public/THIRD_PARTY_LICENSES.txt) contains the complete license texts for npm packages classified as production dependencies by `apps/desktop/package-lock.json`. Vite copies this file from `public` to the root of `dist`; Tauri then distributes it as part of `frontendDist`.

The npm notice is generated deterministically by `apps/desktop/scripts/generate_npm_third_party_licenses.mjs`. The generator reads license files from the exact packages installed by `npm ci`. When a published package omits its license file, generation succeeds only for a reviewed package-and-version-specific upstream source. Any unreviewed omission, metadata mismatch, stale override, or generated-file drift fails the check. Run the following commands from `apps/desktop` after changing npm dependencies:

```text
node scripts/generate_npm_third_party_licenses.mjs
node scripts/generate_npm_third_party_licenses.mjs --check
```

The checked-in [`apps/desktop/src-tauri/THIRD_PARTY_LICENSES-RUST.txt`](apps/desktop/src-tauri/THIRD_PARTY_LICENSES-RUST.txt) provides the corresponding Rust inventory and legal texts. Tauri maps this file explicitly into the installed resources. The Rust generator verifies every crates.io archive against its `Cargo.lock` SHA-256 before reading package metadata and legal files. Published crates that omit a license file require an exact package/version/repository/VCS review rule with either a hash-pinned upstream text or a checksum-verified repository sibling; unknown omissions fail generation.

The Rust inventory deliberately over-includes every crates.io entry in `Cargo.lock`, including target-specific, development, build, and optional packages that may not be linked into the Windows executable. Inclusion is conservative and does not establish actual linkage. After changing Rust dependencies, run:

```text
python -B scripts/generate_rust_third_party_licenses.py
python -B scripts/generate_rust_third_party_licenses.py --check
```

CI first runs `cargo fetch --locked`, then verifies the artifact with `--check --offline` so the legal inventory cannot depend on unreviewed network content.

For each registry dependency, the Rust inventory provides a version-specific source archive URL and its expected SHA-256. Download that archive, verify the digest and extract it to obtain the corresponding published crate source and its license files. For MPL-covered code included in an executable distribution, those sources remain available under MPL; LanGame's noncommercial and modified-release restrictions do not restrict the recipients' independent MPL rights. If a release modifies covered source, its actual corresponding modified source must also be made available; an unchanged upstream archive is not a substitute. See the [Mozilla MPL FAQ](https://www.mozilla.org/en-US/MPL/2.0/FAQ/#q8-i-want-to-distribute-outside-my-organization-executable-programs-or-libraries-that-i-have-compiled-from-someone-elses-unchanged-mpl-licensed-source-code-either-standalone-or-part-of-a-larger-work-what-do-i-have-to-do).

---

# 第三方材料声明

本文记录第三方文档、数据、依赖和运行库。随产品分发的字体及其来源证据单独记录在 [`THIRD_PARTY_ASSETS/NOTICE`](THIRD_PARTY_ASSETS/NOTICE)。Tauri 将本文和该台账一并安装为资源文件。

以下内容仅说明已知来源及其上游条款，不构成法律意见，不转让上游权利，也不保证特定下游用途已获得许可。

安装资源中的副本分别为 `THIRD_PARTY_LICENSES-RUST.txt`、`THIRD_PARTY_LICENSES-NPM.txt`、`fonts/inter/OFL.txt`、`THIRD_PARTY_ASSETS/NOTICE` 和本文 `THIRD_PARTY_NOTICES.md`。本文其余仓库相对链接指向源码树，路径不一定与安装资源布局相同。模型、原生运行库和插件的声明按下文说明随相应组件保留。

## 分发边界

| 材料 | 当前交付方式 | 适用边界 |
| --- | --- | --- |
| Inter 4.1 字体 | 随源码和前端包分发，附 `fonts/inter/OFL.txt` | 保留 OFL 和署名，实际文件须与素材台账一致。 |
| 项目图标和 Minecraft 回退插图 | 随源码及前端/图标构建输入分发 | 分别适用 `NOTICE`；项目自行制作不代表取得第三方游戏或商标权。 |
| 远程游戏封面、截图和预告片 | 源码记录 URL，使用时获取并缓存 | 地址公开、网络来源经过核验，不代表已获展示、缓存、局域网提供或商用许可。 |
| 游戏知识页面 | 模块提供来源元数据，正文获取到本地运行数据 | 复制和使用受发布者条款约束；访问控制和 robots 信号不能替代完整版权依据。 |
| IBM 模型 | 按需下载，附许可与归属 | 固定模型按发布方声明适用 Apache-2.0，权重不在仓库内。 |
| ONNX Runtime 与 MSVC CRT | Windows 构建时内嵌，运行时连同声明释放 | 分别适用原生组件许可证及再分发条件。 |
| SteamCMD 与专有游戏服务端 | 从配置的上游渠道取得 | 不由 LanGame 重新授权；运行时安装目录和缓存不得因此加入源码快照或安装包。 |

`apps/desktop/src/data/module-store-data.json` 中的 Steam/Microsoft 媒体链接，以及 `crates/app-network/official-sources.json` 网络清单记录来源，不构成素材授权。应核对相应游戏发行方规则；通过适用 API 取得的数据还需核对 [Steam API 条款](https://steamcommunity.com/dev/apiterms)，Minecraft 材料需核对其[使用规则](https://www.minecraft.net/en-us/usage-guidelines)。不能将某项 API 授权自动推广到全部商店接口或链接的第三方素材。公开分发前，应记录实际用途的依据、取得必要许可，或替换/移除相应材料。

知识源的 `license_note`、`license_url` 记录上游条款，当前抓取流程不会自动根据这些字段裁定使用许可。`reference_only` 是需要明确配置的来源选项；robots 和内容信号另行提供技术限制。特别是 ARK Wiki 的非商业/相同方式共享条款，以及 RimWorld Together 仓库文档的非商业/禁止演绎条款，不会被 LanGame 自己签发的商用许可覆盖。须分别复核受保护正文和拟定用途，不能宣称所有抓取页面均已获商用或 AI 再利用授权。

## ARK Wiki 服务器配置材料

- 上游贡献者：ARK Wiki contributors。
- 来源页面：<https://ark.wiki.gg/wiki/Server_configuration>
- 上游版权说明：<https://ark.wiki.gg/wiki/ARK_Wiki:Copyrights>
- 声明的许可证：Creative Commons Attribution-NonCommercial-ShareAlike 4.0 International（`CC BY-NC-SA 4.0`）。
- 许可证正文：<https://creativecommons.org/licenses/by-nc-sa/4.0/>
- 私有留存原文：`docs/game-config-acceptance/sources/ark-server-configuration-2026-07-13.wikitext`。
- 公开项目记录：`docs/game-config-acceptance/ark-official-crosswalk-2026-07-13.json`、`modules/arksurvivalascended/config-sources.toml` 和 `modules/arksurvivalevolved/config-sources.toml`。

公开快照排除完整原文和描述字段。公开对照表保留配置标识符、类型、数值、适用版本、来源位置和项目分类；说明型类型字段规范为技术类型/列表事实，不同游戏版本的默认值保留实际值并去除 Wiki 展示标记。来源 URL、核验日期、内容摘要和上游许可证仍予保留。Schema 生成器不会导入上游说明，项目自行编写的帮助文本单独维护；本声明不主张保留记录中的每一项内容均不受上游权利约束。

因此，公开的 `scripts/audit_ark_official_settings.py` 工具要求调用者显式传入本地 wikitext 文件，并在处理前核对固定的 SHA-256 摘要。来源 URL 和预期摘要可通过命令帮助查看；工具不会下载或再分发该页面。项目自行编写的中英文字段帮助维护在 [`scripts/ark_field_descriptions.json`](scripts/ark_field_descriptions.json)。更新后先执行 `python -B scripts/apply_ark_official_crosswalk.py --descriptions-only`，再执行 `node apps/desktop/scripts/generate_i18n_schema_catalogs.cjs --module arksurvivalascended --module arksurvivalevolved` 生成对应词条。

仓库的 LanGame Source-Available License 1.0 仅适用于项目原创代码及上述记录中的原创部分。它不替代上游许可证，也不授予上游文本、游戏名称或商标的相关权利。项目的商用及独立改版发行限制，不覆盖相关材料按 CC BY-NC-SA 独立授予的权利。再分发者应根据预期用途复核上游条款，包括适用的署名、非商业和相同方式共享条件。

## Return to Moria 配置事实

[`modules/returntomoria/reference-configs/server-defaults-build-21872765.ini`](modules/returntomoria/reference-configs/server-defaults-build-21872765.ini) 是项目维护的夹具，依据已记录的 `2026-07-13` Steam public build `21872765` 首次启动配置制作，仅保留验证接入所需的 6 个节名和 25 组键值，不含上游解释性注释。规范化不代表重新运行游戏或更新构建的验证日期。

[来源台账](modules/returntomoria/config-sources.toml) 分别记录规范化前样本与当前 LF 换行分发夹具的 SHA-256。前者标识规范化所用的来源样本，不代表游戏最初生成的字节，也不表示仓库分发该文件。操作说明继续以[发行方专服指南](https://www.returntomoria.com/news-updates/dedicated-server)为来源；本记录不授予发行方说明正文、游戏资产、名称或商标的权利。

## 本地多语言向量模型

LAN 使用 IBM 的 [Granite Embedding 97M Multilingual R2](https://huggingface.co/ibm-granite/granite-embedding-97m-multilingual-r2/blob/835ad14087e140460703cf0fae09f97d469d65c2/README.md)，固定版本为 `835ad14087e140460703cf0fae09f97d469d65c2`。发布方声明采用 Apache License 2.0。LAN 下载发布方未修改的 `onnx/model_quint8_avx2.onnx`、分词器与配置，按内置 SHA-256 校验，并以 `LICENSE.txt` 保留完整许可证和模型归属。模型权重不随本仓库分发。

推理使用 `ort` Rust 绑定及微软官方 ONNX Runtime 1.30.0 CPU 运行时。构建阶段从固定哈希的官方归档提取原生库、MIT 许可及完整第三方声明，并从稳定版 MSVC Build Tools 取得未修改的零售版 x64 CRT，适用微软的[可分发代码条款](https://learn.microsoft.com/en-us/visualstudio/releases/2026/redistribution)。可执行文件内嵌这些文件及清单，首次使用时在模型缓存旁离线释放并核验；已加载且版本兼容的 System32 CRT 可在 Windows 系统信任边界内复用。Rust 依赖声明见下方生成清单，原生 ONNX Runtime 和 CRT 声明随释放的运行时保留。

## 依赖声明

当前原生运行库构建会选择最新的完整稳定版 MSVC 安装，但生成的 CRT 声明固定指向 Visual Studio 2026 再分发清单。发行时须核对该清单及相应 Build Tools 许可是否适用于实际来源版本，必要时修正版本对应声明。微软签名和哈希一致只能证明文件身份，不证明再分发权利。

ARK 生物工具使用 MIT 许可的 ArkServerApi ASE/ASA SDK 和 ASA version-loader 导出桩。构建锁定来源归档并校验 SHA-256；实例安装器另外核验发布的运行框架。来源、版本和摘要见 [`modules/ark-tools/SOURCE.md`](modules/ark-tools/SOURCE.md)，完整上游声明见 [`modules/ark-tools/THIRD_PARTY_NOTICES.txt`](modules/ark-tools/THIRD_PARTY_NOTICES.txt)，并随插件安装。这些第三方条款不替代 LanGame 项目许可。

Rust 与 npm 锁文件记录本源码树使用的依赖。每项依赖仍受其自身许可证及声明文件约束。

仓库中的 [`apps/desktop/public/THIRD_PARTY_LICENSES.txt`](apps/desktop/public/THIRD_PARTY_LICENSES.txt) 收录了 `apps/desktop/package-lock.json` 归类为生产依赖的全部 npm 包许可证正文。Vite 会将该文件从 `public` 复制到 `dist` 根目录，Tauri 随后将其作为 `frontendDist` 的一部分分发。

npm 声明文件由 `apps/desktop/scripts/generate_npm_third_party_licenses.mjs` 确定性生成。生成器读取 `npm ci` 安装的精确版本包内许可证；若上游发布包未附带许可证文件，仅允许使用经过审查且绑定包名与版本的上游原文。出现未经审查的缺失项、元数据不一致、失效覆盖项或产物漂移时，检查会直接失败。修改 npm 依赖后，在 `apps/desktop` 目录运行：

```text
node scripts/generate_npm_third_party_licenses.mjs
node scripts/generate_npm_third_party_licenses.mjs --check
```

仓库中的 [`apps/desktop/src-tauri/THIRD_PARTY_LICENSES-RUST.txt`](apps/desktop/src-tauri/THIRD_PARTY_LICENSES-RUST.txt) 提供对应的 Rust 清单及法律文本。Tauri 会将该文件显式映射到安装资源。Rust 生成器在读取包元数据及法律文件前，会先根据 `Cargo.lock` 的 SHA-256 校验每个 crates.io 归档。若上游发布包未附带许可证文件，必须存在绑定精确包名、版本、仓库及 VCS 提交的审计规则，并使用哈希锁定的上游正文或已校验同仓库 sibling crate；未知缺失项会直接导致生成失败。

Rust 清单有意过度收录 `Cargo.lock` 中的全部 crates.io 条目，包括可能未链接到 Windows 可执行文件的目标平台、开发、构建及可选依赖。该策略偏保守，收录本身不表示实际链接。修改 Rust 依赖后运行：

```text
python -B scripts/generate_rust_third_party_licenses.py
python -B scripts/generate_rust_third_party_licenses.py --check
```

CI 会先执行 `cargo fetch --locked`，再通过 `--check --offline` 校验产物，避免法律清单依赖未经审查的网络内容。

Rust 清单为每个 registry 依赖提供精确版本的源码归档 URL 和预期 SHA-256。下载后核对摘要并解压，即可取得对应发布的 crate 源码和许可文件。可执行分发中包含的 MPL 代码，其源码继续按 MPL 提供；LanGame 的非商业和禁止独立改版发行限制不限制接收者独立享有的 MPL 权利。若某发行版修改了受覆盖源码，还须提供实际对应的修改源码，不能以未修改的上游归档替代。参见 [Mozilla MPL FAQ](https://www.mozilla.org/en-US/MPL/2.0/FAQ/#q8-i-want-to-distribute-outside-my-organization-executable-programs-or-libraries-that-i-have-compiled-from-someone-elses-unchanged-mpl-licensed-source-code-either-standalone-or-part-of-a-larger-work-what-do-i-have-to-do)。
