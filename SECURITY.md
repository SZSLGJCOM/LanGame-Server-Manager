# Security Policy

[English](#english) | [简体中文](#简体中文)

## English

### Supported code

Security fixes are made on the current `master` branch. Older commits and unpublished local builds are not maintained separately.

### Report a vulnerability

Use the repository host's private security-advisory feature. Include:

- the affected commit or release;
- the affected component and platform;
- reproduction steps or a minimal proof of concept;
- the expected impact;
- any known workaround.

If private advisories are unavailable, use the [private contact request form](.github/ISSUE_TEMPLATE/contact_request.yml). The resulting issue is public and must contain only a request for a private channel. Do not disclose the vulnerability, credentials, private addresses, player data, or exploit details in that issue.

Maintainers will acknowledge the report, validate its scope, and coordinate disclosure after a fix is available. Response and release timing depend on severity and reproducibility; this document does not promise a fixed service level.

### Security boundaries

LanGame Server Manager launches third-party dedicated-server software and reads operator-selected files. A report is in scope when project code causes an authorization bypass, command or argument injection, unsafe file access, secret disclosure, insecure update behavior, or a comparable security failure.

Vulnerabilities in a game server, SteamCMD, WebView2, the operating system, or another dependency should normally be reported upstream. Reports showing that this project uses an upstream component unsafely remain in scope.

Never attach proprietary game files, real tokens, private keys, or personal data. Replace them with synthetic fixtures.

## 简体中文

### 支持范围

安全修复进入当前 `master` 分支。旧提交和未发布的本地构建不单独维护。

### 报告漏洞

请使用代码托管平台提供的私有安全公告功能，并提供：

- 受影响的提交或发行版本；
- 受影响的组件与平台；
- 复现步骤或最小化验证代码；
- 预期影响；
- 已知的临时规避方式。

如果托管平台不支持私有安全公告，请使用[私下联系请求表单](.github/ISSUE_TEMPLATE/contact_request.yml)。生成的 Issue 为公开内容，只能用于请求建立私下联系渠道，不得披露漏洞、凭据、私有地址、玩家数据或利用细节。

维护者会确认报告、核实影响范围，并在修复可用后协调披露。响应和发布周期取决于严重程度与可复现性；本文不承诺固定服务时限。

### 安全边界

LanGame Server Manager 会启动第三方专用服务器软件，并读取运维人员选择的文件。如果项目代码导致越权、命令或参数注入、越界文件访问、敏感信息泄露、不安全更新或同等级别的问题，该报告属于受理范围。

游戏服务器、SteamCMD、WebView2、操作系统或其他依赖自身的漏洞通常应向上游报告。如果本项目以不安全方式使用了上游组件，仍可向本项目报告。

禁止附带专有游戏文件、真实令牌、私钥或个人数据。复现材料应使用合成夹具。
