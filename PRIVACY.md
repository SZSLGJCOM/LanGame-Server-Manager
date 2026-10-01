# Privacy and data / 隐私与数据说明

## English

### Scope and responsibility

Updated: 2026-09-30. LanGame Team provides this notice for LanGame Server Manager. It describes the application and its configured connections, not every game, mod, website, model service, or independently operated management host. Those operators have their own data practices and terms.

The manager runs on a Windows computer without requiring a LanGame account. Server data is normally stored on the computer running the manager. If you connect to someone else's management interface, your instructions and submitted data reach that host. The host's administrator controls its storage and connected services; running the application locally does not make all its network activity local.

### Data stored on the manager host

The manager stores application settings, the server database, instance configuration, saves, backups, process output, application logs, and available player-management records to provide server management and troubleshooting. Paths are configurable. The system page shows the active paths. Windows defaults place settings, the database and application logs under %LOCALAPPDATA%/LanGame/ServerManager; game programs, instance files and backups can be elsewhere.

Interface preferences may also be stored in the desktop WebView or browser's local storage. This is separate from the manager's database and files. Local system-resource measurements support the monitoring interface; they are not, by themselves, uploads to LanGame. Application logs use size-based rotation, not a fixed number of days, and may include paths or other context. Protect access to the host, browser profile, backups and any directories you synchronize or share.

### AI recipient, credentials and transport

AI requests go from the manager host to the service URL you configure. The interface shows its origin: scheme, host and port. OpenAI Compatible, Anthropic Compatible and Ollama identify connection protocols; they do not establish who operates the address, whether it is an official vendor service, or whether processing stays on the manager host.

AI API keys are stored in the manager host's operating-system credential store. Requests send the applicable key to the configured service for authentication. Saving a key locally does not mean it is never transmitted. Changing the service URL changes the intended recipient; verify it before checking the connection, sending a message or continuing a task.

HTTPS protects the connection to the configured service; HTTP does not encrypt that connection. A loopback address identifies the manager host, even when you use a browser on another computer. It does not guarantee that the service itself never forwards data elsewhere. Browser-to-manager transport is a separate connection. The selected service and any configured network proxy may have their own logging and access rules.

### Data included in AI work

When you send a message, request AI-generated text, continue a task or use an AI action, the service receives the request and the conversation or task context needed for that work. Depending on the task, this can include previous messages, instance and game identifiers, settings, configuration excerpts, Mod or plugin source text, process logs, ports, observed network endpoints, host CPU/memory/platform information, retrieved documentation and tool results. A single task can make several model requests as it gathers evidence and checks results.

Diagnostics mask recognized credential fields and local absolute paths. This is not full anonymization: free-form messages, logs, file content and model replies can still contain secrets, player identifiers or other personal data. Do not paste credentials or information you are not authorized to share. Confirming a server change is separate from sending diagnostic context; read-only investigation can send context before an operation confirmation appears.

You may use the ordinary management functions without starting an AI conversation. If information must stay on the manager host, use a service you control and verify its address and own network behavior. Stopping a task stops further work as cancellation completes; it cannot retrieve requests already delivered. Charges, retention, training use and deletion at a model service depend on that service's terms and your account agreement. LanGame does not promise those conditions for arbitrary compatible endpoints.

### Connection checks, model lists and knowledge

Check connection runs only when you request it. It sends up to three small synthetic model requests to check chat, tool calling and result replay; it does not read server files or conversation history, create a conversation archive or operate a server. The provider may charge for these requests. Changing settings cancels an active check; cancellation cannot undo a completed request.

Selecting Ollama, changing its address or opening its configured settings can query that address for a model list; Refresh models does the same. This is a service request, including when the address points to another computer. It does not send conversation history.

Knowledge updates fetch source documents and, when needed, a semantic model from their download sources. Automatic updates are enabled by default with a 24-hour interval; a library never synchronized before can update when the background scheduler first runs. You can disable automatic updates or change their interval in knowledge settings. These downloads do not require an AI chat. Document caching, vector indexing and semantic retrieval run on the manager host without a paid embedding API. When AI uses retrieved text, the selected excerpts are included in requests to the configured model service. Local retrieval does not make subsequent AI inference local.

### Conversation history and deletion

Conversation messages and task checkpoints are saved beside the selected database in assistant-sessions/. Archives include redacted messages, recorded requests, tool results and task state. They do not save AI API keys, confirmation tokens, prepared changes or opaque provider reasoning blocks. Redaction does not guarantee that every personal detail in free-form content has been removed.

The archive supports at most 16 conversations. Loading or writing archives removes records whose last modification is at least 30 days old; adding a new archive at capacity removes the oldest. This is cleanup during archive access, not a promise of deletion at an exact time while the application is stopped. Delete a conversation in history to remove its local archive. Deleting it does not remove game data, separate backups or copies retained by a model service.

Clear key removes the credential associated with the currently selected protocol and service URL. Keys saved for other addresses are separate. Clearing browser data does not erase the manager's files or credentials. Changing storage paths or uninstalling the application is not a guarantee that server data, backups or all credentials are erased. Use the relevant deletion controls, check what they cover and preserve needed data first.

### LAN discovery and remote management

The manager's runtime service runs LAN discovery while it is active and publishes discovery announcements when there are publishable server entries. These can expose the node name, instance and game identifiers/names, connection addresses and ports, timestamps and source IP address to listeners on the local network. The node name can derive from the Windows computer name. Discovery announcements are not encrypted or authenticated and do not grant management authority.

Optional LAN management is a separate feature. An authorized remote interface can submit instructions and read the management data its access permits. Its browser stores the management access token in tab-session storage and sends it with management requests. The current HTTP management channel is not encrypted by itself. Disabling remote management does not by itself disable discovery. Do not treat a local network as a private or trusted audience.

On Windows, closing the main window hides it to the tray. The runtime service, servers and scheduled knowledge updates can remain active; closing the assistant panel does not stop knowledge updates either. Hiding a window is not a network-off control.

### Other external connections

Browsing the game library or opening details can load images, video, descriptions, news or reviews from publishers, Steam and other configured media/API sources. Cached content can be reused; uncached or refreshed content contacts the source. Installing, validating or updating game programs and Workshop content contacts the relevant download/platform services. Knowledge synchronization contacts its listed sources. Opening an external link passes control to that site or application.

These services can receive the source IP address, requested resource, feature parameters such as game/item IDs, language or search terms, and ordinary connection metadata. Games, SteamCMD, mods, embedded media and independently configured services may have their own accounts, logs, telemetry and retention. Their behavior is not covered by a promise that LanGame stores server files locally. Application update checks are disabled by default and the current configuration has no update feed; builds with an enabled feed contact that configured source.

### Reports, requests and changes

The application does not automatically attach your local logs to an issue. Review and remove credentials, player information, private addresses and proprietary files before sharing logs or screenshots. Public issues and pull requests are public; do not place private evidence in them.

For a privacy or data question addressed to LanGame Team, use the project repository's Private contact request form and choose Privacy or data request. The initial issue is public: request a private channel without including the underlying data. Follow the private channel supplied by the maintainers. For data held by another management host, model provider or game service, contact that operator; deleting a local record does not submit a deletion request to them.

This notice must track changes to actual storage, recipients and network behavior. Its date does not create consent to new processing, grant rights over player data or replace permissions required for a particular service or deployment.

## 简体中文

### 范围与责任

更新日期：2026-09-30。本说明由 LanGame Team 提供，介绍 LanGame Server Manager 及其所配置连接的数据行为，不覆盖所有游戏、Mod、网站、模型服务或他人独立运营的管理端。这些运营方有各自的数据处理方式与条款。

管理器运行在 Windows 电脑上，无需 LanGame 账号。服务器数据通常保存在运行管理端的电脑。如果你连接的是他人的管理界面，指令及提交的数据会到达那台主机，由其管理员控制存储与所连接的服务。在本机运行应用，不代表应用的全部网络活动都留在本机。

### 管理端本机保存的数据

为提供服务器管理与排障，管理器会保存应用设置、服务器数据库、实例配置、存档、备份、进程输出、应用日志，以及可用的玩家管理记录。存储路径可以配置，系统页显示当前实际路径。Windows 默认将设置、数据库和应用日志放在 %LOCALAPPDATA%/LanGame/ServerManager 下；游戏程序、实例文件和备份可以位于其他目录。

界面偏好也可能保存在桌面 WebView 或浏览器的本地存储中，与管理端数据库及文件分开。本机资源采样用于监控界面，采样本身不等于上传给 LanGame。应用日志按容量轮转，不按固定天数保留，内容可能包含路径等上下文。请妥善管理主机、浏览器配置、备份及同步或共享目录的访问权限。

### AI 接收方、密钥与传输

AI 请求由管理端主机发往你填写的服务地址，界面显示其协议、主机和端口。OpenAI 兼容、Anthropic 兼容、Ollama 是连接协议名称，不证明地址由谁运营、是否为对应厂商官方服务，也不证明处理过程留在管理端本机。

AI API Key 保存在管理端主机的操作系统凭据库中，请求时会将适用的密钥发送给配置的服务用于认证。“本机保存”不等于“从不发送”。更换服务地址会改变预期接收方；检测连接、发送消息或继续任务前，请先核对地址。

HTTPS 保护到所配置服务的传输，HTTP 不加密这段连接。回环地址指管理端主机，即使你从另一台电脑的浏览器访问；它也不保证该服务不会继续向外发送数据。浏览器到管理端是另一段连接。所选服务和配置的网络代理可能有自己的日志及访问规则。

### AI 工作会包含哪些数据

发送消息、请求 AI 生成文本、继续任务或使用 AI 操作时，服务会收到请求及完成工作所需的对话或任务上下文。根据任务需要，可能包含历史消息、实例与游戏标识、设置、配置片段、Mod 或插件源码、进程日志、端口、观察到的网络端点、主机 CPU/内存/平台信息、检索到的文档及工具结果。一次任务可能在收集证据和核对结果时多次请求模型。

诊断会遮蔽能识别的凭据字段和本机绝对路径，但这不等于完整匿名化：自由文本消息、日志、文件内容和模型回复仍可能含有秘密、玩家标识或其他个人信息。请勿粘贴凭据或无权分享的信息。确认修改服务器与发送诊断上下文是两件事；只读调查也可能在操作确认出现之前发送上下文。

不发起 AI 对话也可以使用普通管理功能。数据必须留在管理端本机时，应使用自己控制的服务，并核对地址及该服务自身的联网行为。停止任务会在取消完成后停止后续工作，但不能取回已送达的请求。模型服务如何计费、保留数据、用于训练或受理删除，由该服务条款及你的账号约定决定；LanGame 不对任意兼容地址作统一承诺。

### 连接检测、模型列表与知识库

“检测连接”仅在你主动点击后运行，发送最多三次少量合成测试请求，检查聊天、工具调用和结果回传；不会读取服务器文件或历史对话，不建立会话归档，也不操作服务器。模型服务可能计费。修改设置会取消正在进行的检测，但无法撤回已完成的请求。

选择 Ollama、更改其地址或打开已有的 Ollama 设置时，界面可能向该地址查询模型列表；“刷新模型”也会查询。这仍是一次服务请求，地址在其他电脑上时同样如此，但不会发送历史对话。

知识库更新会访问所列资料来源，并在需要时从下载源获取语义模型。自动更新默认开启，周期为 24 小时；从未同步的知识库，在后台调度首次运行时就可能开始更新。可在知识库设置中关闭自动更新或调整周期，这些下载不需要先发起 AI 聊天。文档缓存、向量索引及语义检索在管理端本机完成，不调用付费向量模型接口。AI 使用检索结果时，选中的正文片段会加入发给所配置模型服务的上下文。本地检索不代表后续 AI 推理也在本地。

### 对话记录与删除范围

对话消息和任务检查点保存在当前数据库旁的 assistant-sessions/ 目录。归档包括脱敏后的消息、记录的要求、工具结果及任务状态，不保存 AI API Key、确认令牌、待执行修改或模型不透明推理块。脱敏并不保证已移除自由文本中的每一项个人信息。

归档最多保留 16 个会话。加载或写入归档时，会清理距最后修改已满 30 天的记录；新增会话达到容量时移除最旧归档。这是在访问归档时执行的清理规则，不保证应用停止期间仍按某个准确时间删除。在历史记录中删除会话，会删除对应本地归档，不会同时删除游戏数据、其他备份或模型服务已保留的副本。

“清除密钥”删除当前协议与服务地址对应的凭据，其他地址保存的密钥是分开的。清除浏览器数据不会清除管理端文件和凭据；更换存储路径或卸载应用，也不保证服务器数据、备份和全部凭据已被擦除。请使用相应删除入口，核对其覆盖范围，并先保留需要的数据。

### 局域网发现与远程管理

管理端运行时服务启动后会运行局域网发现，有可发布的服务器条目时发送发现公告。局域网中的监听者可能看到节点名称、实例及游戏的标识和名称、连接地址与端口、时间戳及源 IP；节点名可能来自 Windows 电脑名称。发现公告不加密、不认证，也不授予管理权限。

可选的局域网管理是另一项功能，获准连接的远程界面可提交指令，并读取权限范围内的管理数据。浏览器在标签页会话存储中保留管理访问令牌，并随管理请求发送；当前 HTTP 管理通道本身不加密。关闭远程管理不等于关闭发现公告，不能将局域网当作只有自己可见或天然可信的环境。

Windows 下关闭主窗口会隐藏到托盘，运行时服务、服务器和定时知识更新可能继续工作；关闭助手面板也不会停止知识更新。隐藏窗口不是断网开关。

### 其他外部连接

浏览游戏库或打开详情时，可能从发行方、Steam 及配置的其他媒体或接口来源加载图片、视频、介绍、新闻和评测。缓存可复用，缺少缓存或刷新内容时会访问来源。安装、校验、更新游戏程序及创意工坊内容会访问相应下载或平台服务；知识库同步会访问其资料来源。打开外部链接后，由对应网站或应用处理后续访问。

这些服务可能收到来源 IP、所请求的资源、游戏或物品 ID、语言、搜索词等功能参数及一般连接信息。游戏、SteamCMD、Mod、嵌入媒体及单独配置的服务可能有自己的账号、日志、遥测和保留规则，不能用“LanGame 将服务器文件保存在本机”来概括它们的行为。应用默认关闭更新检查，当前配置没有更新源；配置并启用更新源的构建会访问相应来源。

### 反馈、请求与变更

应用不会自动把本机日志附到 Issue 中。分享日志或截图前，请检查并移除凭据、玩家信息、私有地址和专有文件。公开 Issue 与 Pull Request 的内容对外可见，不得放入私有证明材料。

向 LanGame Team 提出隐私或数据问题时，可使用项目仓库的“请求私下联系”表单，选择“隐私或数据请求”。最初生成的 Issue 是公开的，只请求建立私下渠道，不填写具体数据；再通过维护者提供的私下渠道沟通。他人管理端、模型服务商或游戏服务持有的数据，应联系相应运营方；删除本地记录不会自动向他们提交删除请求。

实际存储、接收方或联网行为变化时，应同步本说明。更新日期不代表你同意新的处理行为，也不授予处理玩家数据的权利，或替代某项服务及部署所需的许可。
