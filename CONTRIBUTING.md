# Contributing

[English](#english) | [简体中文](#简体中文)

## English

Thank you for contributing to LanGame Server Manager. Keep each pull request focused, preserve established module boundaries, and verify the behavior in proportion to its risk.

### Choose the right entry point

- Use the bug report form for a reproducible product defect.
- Use the feature request form for a scoped product or engineering proposal. An implementation or architecture design is optional at the issue stage.
- Use the game server request form for a Windows dedicated server that is not yet represented by a module.
- Follow [SECURITY.md](SECURITY.md) for vulnerabilities; do not disclose them in a public issue.

Search existing issues, [README.md](README.md), and relevant documentation before opening a new report. Link the issue from the pull request when one exists.

### Prepare the change

1. Read [LanGame Contributor Agreement 1.0](CONTRIBUTOR_AGREEMENT.md#english) before preparing material for inclusion, then follow the public setup instructions in [README.md](README.md).
2. Read the [development guide](docs/development.md#english) for repository structure, engineering conventions, and verification commands. Identify the owning module and affected callers.
3. For changes that cross a storage, process, network, or UI boundary, record the affected data flow and lifecycle before implementation.
4. Keep credentials, private addresses, player data, runtime output, proprietary game files, and unlicensed media outside the repository.

### Documentation

Keep [README.md](README.md) and [README.zh-CN.md](README.zh-CN.md) aligned. The README introduces the product and setup; [docs/README.md](docs/README.md) routes readers to operator and integration guides. Put reusable technical contracts in the relevant guide rather than adding root-level audit reports.

Describe current behavior, prerequisites, limitations, and verifiable commands. Remove task history and superseded instructions. Use repository-relative links and portable examples, keep identifiers unchanged, and verify links after moving or renaming a document. Third-party license texts must retain their original content and attribution.

### Pull requests

Target `master` and complete the [pull request template](.github/pull_request_template.md). Explain the problem and resulting behavior, link the related issue when one exists, and include the actual test commands, results, and environment.

Use a descriptive title, such as `fix(palworld): preserve settings when a save fails` or `docs: clarify Windows build prerequisites`. Conventional Commits prefixes are recommended. Describe the final change without task history.

Describe architecture, security, dependency, license, or operator impact when applicable. Include a recovery path for changes to persisted data or server operation. Explain unrelated CI failures with evidence; do not weaken tests or checks to make a pull request pass.

Keep dependency updates, formatting, and unrelated features in separate pull requests unless the same change requires them. Review the upstream release notes and lock-file diff for dependency changes.

### Contribution rights

This project uses the [LanGame Source-Available License 1.0](LICENSE), not a standard open-source license. Private noncommercial changes need not be submitted. Patches and source forks used to propose, discuss, review, and submit improvements to the official project are permitted; they do not authorize independent modified releases, public build artifacts, or commercial use. Hosting-platform rights that apply independently are preserved.

Read [LanGame Contributor Agreement 1.0](CONTRIBUTOR_AGREEMENT.md#english). You keep your copyright while granting the Official Maintainers permission to use, modify, and distribute your contribution within LanGame Server Manager, including paid official versions and written commercial-use permission for customers. The grant does not cover unrelated products, separate sale of the contribution, arbitrary relicensing, customer sublicensing, or customers' independent modified releases. Public noncommercial limits and the brand rules remain in place.

For each PR, actively check the agreement confirmation in the template yourself or add your own confirmation comment. State:

> I have read and agree to LanGame Contributor Agreement 1.0 for the contributions I am authorized to license in this PR, as identified by the commit IDs in my confirmation. I retain copyright and permit official commercial use and customer commercial-use licensing within LanGame Server Manager.

List the covered commit IDs and identify any third-party materials, coauthors, or employer/client rights. A person without the necessary authority cannot confirm for another rightsholder. A patch submitted outside a PR needs the same explicit agreement and an exact patch or commit reference. Ordinary issues, discussions, private changes, or silence are not acceptance. Updating the policy does not grant additional rights over earlier contributions; use the valid existing grants or obtain express agreement for the identified older contribution.

Before merge, maintainers must verify each needed rightsholder's confirmation and record the contributor account or authorized representative, confirmation time, agreement version with its exact text or immutable reference, covered commit IDs, and PR/patch reference in the review record. Do not check the contributor's box on their behalf. Added material or changed rights after confirmation needs renewed confirmation for the affected commits. Do not merge under these terms with unresolved consent or ownership; CI checks do not establish either. Keep private authorization evidence out of public PRs and use the private contact process when needed. Separately licensed third-party material retains its own terms.

## 简体中文

感谢参与 LanGame Server Manager。每个 Pull Request 应聚焦一个明确目标，保持既有模块边界，并按风险完成相应验证。

### 选择正确的反馈入口

- 可复现的产品缺陷使用缺陷报告表单。
- 范围明确的产品或工程改进使用功能建议表单。Issue 阶段可以不提供实现方案或架构设计。
- 尚未纳入模块目录的 Windows 专用游戏服务器使用游戏服务器接入表单。
- 安全漏洞按 [SECURITY.md](SECURITY.md) 私下报告，不得在公开 Issue 中披露。

提交新问题前，请检索已有 Issue、[README.zh-CN.md](README.zh-CN.md) 和相关文档。如已有对应 Issue，请在 Pull Request 中关联。

### 准备改动

1. 准备提交供接纳的材料前，先阅读 [LanGame 贡献协议 1.0](CONTRIBUTOR_AGREEMENT.md#简体中文)，再按 [README.zh-CN.md](README.zh-CN.md) 中的说明准备开发环境。
2. 阅读[开发指南](docs/development.md#简体中文)，了解仓库结构、工程约定和验证命令，确认所属模块和受影响的调用方。
3. 改动跨越存储、进程、网络或界面边界时，先记录受影响的数据流与生命周期。
4. 凭据、私有地址、玩家数据、运行输出、专有游戏文件和未经许可的媒体不得进入仓库。

### 文档

[README.md](README.md) 和 [README.zh-CN.md](README.zh-CN.md) 应保持一致。README 介绍产品与安装环境，[docs/README.md](docs/README.md#简体中文) 提供操作和接入指南索引。可复用的技术契约放入对应指南，不在根目录增加任务审计报告。

文档应说明当前行为、前置条件、限制与可验证的命令。删除任务流水和失效操作说明。链接使用仓库相对路径，示例保持可移植，机器标识符不得改写；移动或重命名文档后应验证链接。第三方许可证正文必须保留原文与署名。

### Pull Request

目标分支为 `master`，并填写 [Pull Request 模板](.github/pull_request_template.md)。说明问题和改动后的行为，如有相关 Issue 则附上链接，同时提供实际执行的测试命令、结果与环境。

标题应准确描述最终改动，例如 `fix(palworld): preserve settings when a save fails` 或 `docs: clarify Windows build prerequisites`。建议使用 Conventional Commits 前缀，不在标题中记录任务过程。

按需说明架构、安全、依赖、许可证或操作流程的影响。涉及持久化数据或服务器运行时，提供恢复方式。对于无关的 CI 失败，给出证据说明，不得降低测试或检查标准来获得通过结果。

依赖更新、格式化和无关功能应分别提交，除非同一项改动确实需要它们。变更依赖时，检查上游发布说明和锁文件差异。

### 贡献权利

本项目采用 [LanGame Source-Available License 1.0](LICENSE)，属于源码可见许可，并非标准开源许可。私下的非商业修改无需提交或公开。允许通过补丁、贡献分支和 Pull Request 向官方提出、讨论、审查和提交改进；不因此允许自行发布修改版、公开构建产物或商业使用。托管平台独立授予的适用权利仍予保留。

请阅读 [LanGame 贡献协议 1.0](CONTRIBUTOR_AGREEMENT.md#简体中文)。你保留版权，同时允许官方维护者在 LanGame Server Manager 项目内使用、修改和发布贡献，包括官方收费版本及向客户书面授予商业使用许可。授权不包含无关产品、单独出售贡献、任意更换许可、客户再许可或客户独立发行修改版。公众的非商业限制及品牌规则继续适用。

每个 PR 由贡献者本人主动勾选模板中的协议确认，或本人补充以下确认评论：

> 我已阅读并同意 LanGame Contributor Agreement 1.0，授权范围为本次 PR 中我有权许可、且在本确认中按提交 ID 明确列出的贡献。我保留版权，并允许 LanGame Server Manager 官方在本项目内商用这些贡献及向客户授予商业使用许可。

列出所覆盖的提交 ID，说明第三方材料、共同作者及雇主或客户的权利。没有相应授权的人不能替其他权利人确认。PR 以外提交的补丁同样需要明确同意，并指向确定的补丁或提交。普通问题反馈、讨论、私下修改或沉默不表示同意。更新规则不会自动扩大对旧贡献的权利；应沿用原有有效授权，或针对指定旧贡献另行取得明确同意。

合并前，维护者须核对所有必要权利人的确认，并在审查记录中保留：贡献者账号或授权代表、确认时间、协议版本及准确正文或不可变引用、所覆盖的提交 ID、PR 或补丁引用。不得代贡献者勾选。确认后新增材料或权利来源发生变化时，应对受影响提交补充确认。授权或权属尚未解决时，不得按本协议合并；CI 通过不代表已获得同意或证明权属。私有授权证明不得放进公开 PR，必要时使用私下联系流程。单独许可的第三方材料继续适用原有条款。
