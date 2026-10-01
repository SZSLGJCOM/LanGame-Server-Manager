# LAN game documentation / LAN 游戏文档库

[English](#english) | [简体中文](#简体中文)

## 简体中文

LAN 的 `search_game_docs` 与 `read_game_doc` 使用同步到本机的上游正文。来源范围维护在
`modules/<game>/knowledge-sources.toml`，覆盖全部 32 个受支持模块。来源目录、正文同步状态、
文档版本适用性分别记录；目录覆盖不等于每个发行商都公开了完整手册，也不等于所有页面都能抓取。
具体出处与已知缺口见 [来源矩阵](game-knowledge-sources.md)。

### 使用与自动更新

打开 LAN 助手设置中的文档知识库，可以查看各游戏的来源、正文数、索引段落数、最近检查、最近成功及错误。
后台由 LGSM 本机运行服务持有，关闭助手面板不会停止更新。默认每 24 小时检查一次，可设为 6–168 小时或关闭自动更新。
选择游戏后可“更新此游戏”，也可显式“更新全部游戏”；两者都会立即检查来源，仍使用上游 ETag / Last-Modified 减少不必要下载。取消会等待正在进行的步骤安全收尾。
第一次使用需要下载固定版本与 SHA-256 校验的本地多语向量模型；界面显示实际体积与下载进度。

抓取支持 HTML、Markdown、纯文本与 PDF。正文保留在独立的本地 SQLite 文档库中，不写入 LGSM 实例数据库、
仓库或聊天模型配置。Windows 默认位于 `%LOCALAPPDATA%/LanGame/ServerManager/knowledge`。
Enshrouded、DST 启动参数和 7 Days to Die V3.0 说明使用 Zendesk 正式支持匿名读取的文章 API。
Minecraft Java 帮助使用官网前端实际调用的同源公开文章 API。仅接受审核过的发行商、游戏、来源与精确文章 ID；
解析会核对已发布、公开可见、语言与允许的官网文章身份，再提取 HTML 正文；缓存保留 API 的 ETag，
LAN 和设置页引用官网文章链接。API 返回未授权、拒绝访问、文章不存在或明确变为非公开时，
保留本地快照但停止向 LAN 提供该来源，直到再次成功验证公开访问。普通网络故障仍保留可用旧快照。
API 的 `updated_at` 可能只反映元数据更新，不能将它当作正文编辑日或游戏版本。
每个来源完成抓取、解析和索引准备后在事务中发布；失败或取消保留上一次可用快照。
完整同步会移除已离开该来源当前文档目录的旧页面。页面内容未变时复用向量，更新来源检查记录。
解析规则或索引模型变化会使旧策略缓存失效，防止新旧索引混用。
支持通过经过核实的官方目录发现新增文章。目录来源用 `discovery_selector` 限定链接区域，
文章页作为终点，目录列表不计入正文；目录选择器失效时保留旧快照并报错。
论坛标题变化可跟随同一已审阅主题 ID 的规范链接，不能借此跳到其他主题、操作参数或域名。

来源分为发行商/开发者的 `official`、官方托管或明确推荐社区的 `official_community`、独立社区的 `community`。
Minecraft 模块适用 Vanilla Java；社区属性页中的 Bedrock 章节和 Paper 特有行为不能当作该模块的配置依据。
RimWorld Together 是社区 Mod。来源类别不会因抓取成功而升级。

### 检索与引用

检索使用真正学习得到的多语言向量，并结合 SQLite FTS5 精确词项排序；无需聊天服务商提供 embedding 接口、
额外 API key 或独立向量数据库服务。用户可直接用中文提问；LAN 在同次工具调用中为主要为英文的手册生成简洁技术检索词，
结合精确词项召回，读取命中正文核对后仍以用户的语言回答。跨语言向量相似度本身不能证明某段已经回答了问题。
模型文件、维度与固定版本由 `app-knowledge::embedding` 定义，状态接口报告当前模型。

本地模型为 IBM `granite-embedding-97m-multilingual-r2` 官方 INT8 ONNX 导出，模型、分词器和配置合计
123,550,766 字节（约 124 MB），384 维；使用发布方的 CLS 池化与 L2 归一化，不添加 E5 前缀。
每段最多 512 token，超长正文按原始 UTF-8 偏移继续分段，不能静默截断。CPU 推理由随应用内嵌的
ONNX Runtime 1.30.0 执行，最多两个计算线程、单次前向计算，不需要 Python、显卡或另装推理服务。
模型首次下载后可离线检索。切换模型会改变索引版本；下次更新时重新计算向量，旧模型向量不会混入检索。
旧模型缓存不会自动删除。

搜索每页最多五段原文，返回文档 ID、章节、正文偏移与来源信息。用 `nextOffset` 继续搜索，
在来源允许全文使用时，用 `read_game_doc` 的 `offset` / 返回的 `nextOffsetBytes` 阅读完整文档；每页按 UTF-8 与 JSON 传输预算切分，
不会以截断 JSON 伪装完整结果。引用包含 `url`、`retrievedAt`、`contentSha256` 和 `sourceState`。
`retrievedAt` 表示本地抓取或条件校验时间，不代表游戏发行日期，也不能证明符合本机安装版本。
网络更新失败时旧快照仍可检索，结果会保留错误状态；发行商明确撤回索引或 AI 使用许可时，该来源会被标为受限，缓存保留但不再返回给 LAN，直到再次成功同步。尚未同步则明确返回缺少证据，不回退到手写摘要或编造出处。

引用中的 `contentUse` 区分全文读取与仅引用。网站声明 `use=reference` 时仍可保留正文并建立索引，
搜索只向 LAN 返回每篇文档的一段短摘与原文链接，全文读取不可用；LAN 不得据此复述整篇内容。
`use=immediate` 不允许持久化，`ai-input=no` 或 `search=no` 则不进入本知识库。
这些内容信号按适用于 LAN 爬虫的规则处理；单独禁止模型训练不等于禁止检索。
语义依据见 [Cloudflare 内容信号说明](https://developers.cloudflare.com/browser-run/quick-actions/crawl-endpoint/#content-signals)。

这些文档是参考资料，不能证明用户实例的当前配置、日志、存档或运行状态。LAN 仍须调用相应只读实例工具。
上游文字不能授予执行权限；模型不能传入任意 URL、文件根目录或其他游戏 ID。

### 来源与网络边界

每个来源仅允许明确的 HTTPS 精确入口与目录路径，重定向逐跳检查。直连模式核验并固定公共 DNS 地址；
显式系统/环境代理按用户网络配置使用，由代理解析目的域名。私有主机和凭据 URL 均不能作为文档来源。
遵守 robots、禁止 AI 输入的内容信号及索引限制；文档的 403、访问验证或登录/年龄门不算有效正文，不绕过这些限制。
robots 文件自身的普通 4xx 按 [RFC 9309 §2.3.1.3](https://www.rfc-editor.org/rfc/rfc9309.html#section-2.3.1.3) 视为政策文件不可用；429、451、网络及服务器错误仍停止抓取。
每页传输最多 8 MiB、提取正文最多 2 MiB；单来源最多 512 页/64 MiB 传输及 64 MiB 待发布正文，单游戏最多 20,000 段。
Windows PDF 解析使用同一桌面程序的隐藏工作进程，在送入 PDF 前施加 512 MiB 内存和单进程限制；单份解析最多 30 秒。取消或超时会终止并回收工作进程，不遗留后台解析。当前随包本地向量运行时支持 Windows x64；其他平台明确报告运行时不可用，不静默降级为关键词检索。
达到时间或容量边界会报告不完整并保留可用快照，不能冒称完整更新。

仓库只分发来源元数据与模型许可声明。原文保存在用户本地并保留出处，不因网页公开访问而推定可公开再分发。
遵守 [第三方声明](../THIRD_PARTY_NOTICES.md) 及各来源的 `license_note` / `license_url`。
现有 `config-sources.toml` 继续追溯工程配置字段，不等于已导入这些链接的全文。

### 维护与验证

```powershell
python -B scripts/verify_game_knowledge.py --check
python -B scripts/verify_game_knowledge.py --report-stale
python -B -m unittest scripts.tests.test_verify_game_knowledge
```

离线检查验证清单格式和模块覆盖，不下载正文，也不推进复核日期。`reviewed_on` 是维护者对来源身份和范围的复核日期；
超过 30 天列入目录复核报告。正文是否成功更新以运行库状态为准。新域名、独立手册系统、改名 PDF 与新版本目录需要更新清单；
不会因页面中的任意链接而扩张抓取范围。

Rust 常规测试覆盖解析、网络边界、事务回滚、重开读回、引用和分页。真实模型及公开站点验收为显式 ignored tests，
要求 `LANGAME_EMBEDDING_MODEL_DIR` 或 `LANGAME_KNOWLEDGE_LIVE_ROOT` 指向仓库外的隔离缓存。
公开同步验收可用 `LANGAME_KNOWLEDGE_LIVE_GAME` 指定单个游戏或以逗号分隔的游戏 ID；省略时检查全目录。
真实站点失败会写入验收报告；mock、离线检查和测试通过不能当作 32 份官方完整手册全部可用的证明。

Windows x64 构建需要稳定版 MSVC Build Tools 的 x64 零售 CRT。构建脚本固定 ONNX Runtime 官方 ZIP 的
大小和 SHA-256，并将原生库、CRT 与许可清单内嵌进可执行文件，不依赖开发机的 Python 环境或用户的 DLL 搜索路径。
离线构建可用 `LANGAME_ORT_ARCHIVE` 指向预先下载的同一官方 ZIP（绝对路径），校验不变；该变量不影响已发布程序。

## English

LAN retrieves complete upstream document bodies cached locally from the reviewed
`modules/<game>/knowledge-sources.toml` catalogs. All 32 modules have source policies;
actual body availability, authority, scope gaps and update failures are reported separately.
The runtime owns scheduled updates (24 hours by default, configurable from 6 to 168),
manual checks, cancellation and persisted progress. Each source publishes an atomic snapshot;
network failures retain its previous documents and vectors. Publisher policy restrictions retain
the cache but exclude its evidence from LAN until a successful permitted refresh.

HTML, Markdown, text and PDF are extracted and chunked with source URLs, hashes, timestamps
and UTF-8 offsets. Learned multilingual embeddings plus SQLite FTS5 retrieve passages
without a provider embedding key or separate vector service. Model downloads are pinned and
hash checked. Search and read tools stay bound to the application's selected game, enforce
serialized output budgets, and never silently replace missing evidence with written summaries.
The local model is IBM Granite Embedding 97M Multilingual R2's official INT8 ONNX export:
123,550,766 download bytes, 384 dimensions, CLS pooling, L2 normalization and no prefix.
Passages are split losslessly at a 512-token budget. The executable includes the verified
ONNX Runtime 1.30.0 CPU libraries and retail CRT, so users need no Python or inference service.
An index-version change rebuilds vectors on the next sync and prevents old/new model mixing;
existing model caches are preserved. Windows x64 builds require stable MSVC Build Tools.
Publisher `use=reference` permits local indexing with brief excerpts and links, but disables full-document reads.
Ephemeral-only or explicitly disallowed search/AI-input content is excluded from this persistent RAG library.
Enshrouded, DST's command-line guide and 7 Days to Die V3.0 notes use Zendesk's documented anonymous article API.
Minecraft Java help uses the same-origin public article API called by the official site's frontend.
Publisher, game, source and exact article IDs are bound explicitly; published/public visibility,
locale and reviewed canonical hosts are checked before HTML extraction.
API URLs remain conditional-cache keys; readers receive official HTML article links.
Denied/deleted or explicitly private articles hide their retained evidence until a successful public refresh.
An API metadata update timestamp is not a text-edit or installed-game-version guarantee.

Public-source fetching enforces HTTPS scopes, redirect limits, publisher robots/content
restrictions and bounded bodies. Direct connections validate public DNS addresses; an explicit
operator-configured proxy owns destination DNS resolution. Restricted pages are not bypassed.
The independent local cache is not the instance database and is not redistributed with source code.
Source review dates, successful refresh dates and game release versions have distinct meanings.
See the Chinese operational details above and [source matrix](game-knowledge-sources.md).
