# Game documentation sources / 游戏文档来源目录

[English](#english) | [简体中文](#简体中文)

## English

This is the **2026-09-28 source-directory review**: 32 supported modules,
55 sources and 111 exact entry URLs. It identifies retrieval scope and provenance;
it does not certify that 32 complete publisher manuals were downloaded or that
every body is accessible and current. Machine-readable policy lives in each
module's knowledge-sources.toml. The DST command-line guide, 7 Days to Die V3.0 notes
and Minecraft Java help API entries were independently reviewed on 2026-09-30;
this does not advance the other sources' review dates or certify runtime indexing.
See [the runtime workflow](game-knowledge.md).

**official** means publisher/developer material. **official_community** means a
publisher-hosted or explicitly endorsed community guide/wiki; it does not imply
that the publisher authored every page. **community** identifies an independently
maintained resource without verified publisher endorsement. Each source records
authority_evidence and reviewed_on: source identity and directory review, not
proof that all bodies were readable or current.

HTML directories discover only links under explicitly allowed same-origin paths.
Flat wiki pages, Steam guide IDs and versioned PDFs use exact URLs to avoid
ingesting unrelated encyclopedias or arbitrary forum threads. Raw GitHub Markdown
is reference text, never executable code. Exact seeds refresh those same pages;
a newly named PDF, new flat wiki article or different major-version directory
requires a directory review before entering the authorized scope.

Public availability is not an open redistribution license. Full extracted bodies
belong in the user's local library; the repository distributes source metadata.
Keep upstream notices and attribution. license_note and optional license_url
preserve known terms without inventing permission.

Direct robots review on 2026-09-28 found reference-only content signals on ARK,
HumanitZ and Terraria wiki.gg hosts. `use=reference` permits local indexing with
brief cited excerpts, while full-document delivery to LAN is disabled. Explicit
search/AI-input prohibitions prevent ingestion. Do not bypass access restrictions
using another hostname, crawler identity or search-engine cache. Several wiki bodies returned 403; Soulmask
Fandom blocked its body and robots requests; SCUM's Steam topic was age-gated.
These are access observations, not permanent availability or permission claims.
Synchronization checks current robots, response headers and page metadata again.
Minecraft Wiki explicitly excludes AI crawlers, including AI search crawlers,
in its robots file. Its two manifest sources have reference_only enabled: their
links remain available for human reference without background fetching or RAG
indexing.

RimWorld Together is a community mod, not Ludeon's dedicated server. Its reviewed
repository LICENSE is **CC BY-NC-ND 4.0**, not MIT. Review those restrictions before
reuse outside private reference. ARK also has separate attribution/noncommercial
terms recorded in [third-party notices](../THIRD_PARTY_NOTICES.md).

The former seven pending summaries do not become seven verified complete manuals.
Klei's command-line article is published through its reviewed anonymous article API;
its four forum guides previously passed runtime extraction and indexing as four
documents and 32 passages. The command-line API's public body is independently
verified, not proof of a successful local index refresh. ARK Evolved, HumanitZ and Terraria
remain subject to reference-only delivery; Romestead's official hotfix is not a full manual; SCUM's
endorsed topic is age-gated; Soulmask's endorsed Fandom guide remains inaccessible.

## 简体中文

这是 **2026-09-28 的来源目录复核**：覆盖 32 个模块、55 个来源、111 个精确入口。
复核确认的是来源身份、范围及已知缺口，不代表 32 份完整官方手册已经下载，也不
代表所有正文可访问或适用于本机安装版本。每个模块的 knowledge-sources.toml 保存
可执行的抓取范围；DST 启动参数、7 Days to Die V3.0 与 Minecraft Java 帮助的公开 API 入口
另于 2026-09-30 复核，不推进其他来源复核日期，也不表示本地索引已成功刷新。正文同步
与向量检索见[游戏知识库说明](game-knowledge.md#简体中文)。

来源类别须保留：official 为发行商或开发者资料；official_community 为官方托管或
明确推荐的社区 Wiki/指南；community 为独立维护、尚无已核实官方推荐的资料。
官方推荐不等于官方逐页编写。authority_evidence 留存归属依据，reviewed_on 是目录
和身份复核日，不能充当正文最新日期或抓取成功标记。

文档站仅在明确的同源目录范围内发现页面；平铺 Wiki、Steam 指南和固定版本 PDF
使用精确入口，防止整本游戏百科或论坛误入。现有页面可自动刷新；新 PDF 文件名、
新 Wiki 页面和新大版本目录仍须复核后加入。GitHub 原始 Markdown 作为文本阅读，
其中的命令不会自动执行。

公开可读不等于允许公开再分发。仓库仅分发来源元数据，正文保存在用户本地，并保留
上游署名与许可。ARK、HumanitZ、Terraria 的 wiki.gg 站点在本次 robots 复核中声明了
仅引用用途等内容信号：`use=reference` 允许本地索引及带出处的短摘，禁止向 LAN
交付全文；明确禁止搜索或 AI 输入时拒绝导入。不绕过访问限制。
Soulmask Fandom 的正文与 robots 请求受阻；其他来源也可能返回 403 或年龄验证。
实际同步会再次检查访问及索引限制。RimWorld Together 已打开的仓库许可证为
CC BY-NC-ND 4.0，不能误称 MIT。
Minecraft Wiki 的 robots 明确禁止包括 AI 搜索在内的多种 AI 抓取代理；其两项来源
已设置 reference_only，仅保留供人工查阅的链接，不进行后台正文抓取或 RAG 索引。

前版七个待核验项没有变成七份已收录手册：DST 的 Klei 启动参数文章已核实可通过
正式匿名 API 公开读取；四篇论坛指南首帖此前通过实际运行时提取及索引，生成 4 篇正文
和 32 个段落。新 API 正文可访问不表示本地索引已经刷新成功。
ARK Evolved、HumanitZ、Terraria 的 Wiki 正文仍受仅引用用途限制；
Romestead 官方更新公告不能替代完整手册；SCUM 存在年龄验证；Soulmask 官方
推荐的 Fandom 指南仍无法读取。各清单的 gaps 保留真实缺口。

### 32-module source matrix / 32 游戏来源矩阵

The linked entry is representative; each module manifest lists all exact seeds,
directory prefixes, ownership evidence and known gaps. “Directory discovery”
does not mean a crawl has succeeded. / 表中为代表入口；完整地址、来源依据和缺口见各模块
清单。目录发现表示允许的抓取方式，不是抓取成功回执。

| Module / 模块 | Source / 来源 | Retrieval / 机器入口 | Scope / 文档范围 |
| --- | --- | --- | --- |
| abioticfactor | [Abiotic Factor dedicated server wiki](https://raw.githubusercontent.com/wiki/DFJacob/AbioticFactorDedicatedServer/Home.md) (official) | Directory discovery (5 seeds) | Dedicated-server installation, launch parameters, sandbox configuration, ports and save migration. |
| arksurvivalascended | [ARK server administration wiki](https://ark.wiki.gg/wiki/Dedicated_server_setup) (official_community)<br>[ASA server patch notes](https://survivetheark.com/index.php?/forums/topic/773786-asa-server-patch-notes-server-v9336-updated-09242026/) (official) | Exact pages (3 seeds)<br>Exact pages (1 seeds) | ARK: Survival Ascended server setup, configuration, ports, save and cluster administration; retain ASA versus ASE applicability. |
| arksurvivalevolved | [ARK server administration wiki](https://ark.wiki.gg/wiki/Dedicated_server_setup) (official_community) | Exact pages (3 seeds) | ARK: Survival Evolved dedicated-server installation, configuration, ports, saves and console administration. |
| astroneer | [Astroneer dedicated servers and host-your-own guide](https://astroneer.space/dedicatedserver/) (official)<br>[Astroneer host-your-own dedicated-server setup details](https://blog.astroneer.space/p/astroneer-dedicated-server-details/) (official) | Exact pages (1 entries)<br>Exact pages (1 entries) | Self-hosted dedicated-server installation, Engine.ini and AstroServerSettings.ini, networking, ownership and world saves. |
| barotrauma | [Barotrauma server administration](https://barotraumagame.com/wiki/Hosting_a_Dedicated_Server) (official_community)<br>[Developer server-hosting guides](https://steamcommunity.com/app/602960/discussions/1/2250056952644462246/) (official; 2020-05-20) | Exact pages (5 seeds)<br>Exact pages (2 seeds; English and Chinese) | Dedicated-server hosting, server XML settings, client permissions, console administration and server mods. The accessible historical Steam guides cover setup and ports; the detailed wiki remains access-limited. |
| conanexiles | [Conan Exiles Enhanced server administration](https://exiles-enhanced.inflexion.io/servers/) (official)<br>[Conan Exiles dedicated-server setup and ports (original edition)](https://www.conanexiles.com/dedicated-servers/) (official)<br>[Conan Exiles technical manual (2020-09-15 PDF)](https://cdn.cloudflare.steamstatic.com/steam/apps/440900/manuals/Conan_Exiles_Technical_manual_2020.09.15.pdf) (official) | Directory discovery (1 entries)<br>Exact pages (1 entries)<br>Exact pages (1 entries) | Conan Exiles Enhanced server administration, settings, Linux hosting, regions, migration and troubleshooting; historical UE4 manual kept separately. |
| corekeeper | [Historical publisher patch notes](https://fireshinegames.co.uk/wp-content/uploads/Core-Keeper-Patch-notes-Post-1.0-1.0.0.16.pdf) (official)<br>[1.1.2.3 dedicated-server connection and compatibility notes](https://store.steampowered.com/news/app/1621690/view/591771368562885598) (official; 2025-08-14)<br>[Dedicated-server setup wiki](https://corekeeper.atma.gg/en/How_to_setup_a_dedicated_server) (community) | Exact PDF (1 seed)<br>Exact publisher feed (1 seed; two fixed post selectors)<br>Exact wiki page (1 seed) | Version-specific direct connections, cross-store PC play and mod compatibility; the complete bundled README/ARGUMENTS are not in the public web corpus. |
| dontstarve | [Dedicated Server Command Line Options Guide](https://support.klei.com/hc/en-us/articles/360029556192-Dedicated-Server-Command-Line-Options-Guide) (official)<br>[Klei dedicated-server setup and settings guides](https://kleiforums.com/forums/topic/64212-dedicated-server-quick-setup-guide-windows/) (official_community) | Reviewed anonymous article API (1 exact ID)<br>Exact pages (4 seeds) | Don't Starve Together dedicated-server installation, cluster/shard configuration, launch options, ports and world overrides. |
| enshrouded | [Enshrouded dedicated-server help articles](https://enshrouded.zendesk.com/hc/en-us/articles/16051370691485-Dedicated-Server-Installation-on-Steam) (official) | Documented anonymous article API (7 exact IDs; official HTML citations) | Dedicated-server installation, requirements, configuration, gameplay rules, roles, networking and server FAQs. |
| humanitz | [HumanitZ private server documentation](https://humanitz.wiki.gg/wiki/Private_Server_Hosting_Setup) (official_community) | Exact pages (8 seeds) | Private dedicated-server setup on Windows/Linux, configuration files, port forwarding, commands and server updates. |
| minecraft | [Minecraft Java Edition vanilla server download and setup](https://www.minecraft.net/en-us/download/server) (official)<br>[Minecraft Java Edition setup help article](https://help.minecraft.net/hc/en-us/articles/360058525452-How-to-Setup-a-Minecraft-Java-Edition-Server) (official)<br>[Minecraft End User License Agreement](https://www.minecraft.net/en-us/eula) (official)<br>[Minecraft Wiki server.properties — Java Edition sections only](https://minecraft.wiki/w/Server.properties) (community; reference only)<br>[Minecraft Wiki Java server setup tutorial (linked by Mojang)](https://minecraft.wiki/w/Tutorial:Setting_up_a_server) (official_community; reference only) | Exact page (1 entry)<br>Reviewed public website article API (1 exact ID)<br>Exact page (1 entry)<br>Reference only (1 link)<br>Reference only (1 link) | Minecraft Java Edition vanilla server download/setup and EULA; community configuration links are retained for reference without background RAG indexing. Bedrock and third-party server distributions are outside this module's applicability. |
| necesse | [Necesse dedicated-server entry](https://necessegame.com/server) (official)<br>[Necesse dedicated-server guides](https://necessewiki.com/Multiplayer) (official_community) | Exact pages (1 seeds)<br>Exact pages (3 seeds) | Dedicated-server installation, Windows/Linux networking, configuration, file locations, saves, commands and startup parameters. |
| nightingale | [Nightingale dedicated-server manual](https://a.storyblok.com/f/239842/x/e8167d5c91/nightingale-dedicated-server-0-8-final.pdf) (official) | Exact pages (1 seeds) | Dedicated-server installation, realm/player migration, configuration, network/log/status options, saves and backups. |
| palworld | [Palworld Server Guide](https://docs.palworldgame.com/) (official) | Directory discovery (1 seeds) | Complete English Palworld server-guide branches for getting started, configuration/operation and server APIs. |
| projectzomboid | [Project Zomboid server options and networking notes](https://projectzomboid.com/modding/zombie/network/ServerOptions.html) (official)<br>[PZwiki dedicated-server guides](https://pzwiki.net/wiki/Dedicated_server) (community) | Exact pages (2 seeds)<br>Exact pages (3 seeds) | Dedicated-server setup, settings, launch parameters, networking and version-specific server changes. |
| returntomoria | [Return to Moria dedicated-server guide](https://www.returntomoria.com/news-updates/dedicated-server) (official)<br>[Return to Moria server guides](https://northbeachgames.freshdesk.com/support/solutions/folders/154000747073) and [server FAQs](https://northbeachgames.freshdesk.com/support/solutions/folders/154000747074) (official)<br>[SteamCMD installation manual](https://northbeachgames.freshdesk.com/support/solutions/articles/154000217670-how-to-use-steamcmd-to-install-the-dedicated-server) (official) | Exact page (1 seed)<br>Directory discovery (2 seeds; reviewed list and pagination selectors)<br>Exact page (1 seed) | Dedicated-server installation, configuration, world migration, saves, permissions, commands and server FAQs. New articles listed in the two server folders are discovered automatically within a 128-page budget; article sidebars do not expand scope. |
| rimworld | [RimWorld Together maintainer server instructions](https://raw.githubusercontent.com/RimWorld-Together/Rimworld-Together/development/README.md) (community) | Exact pages (1 seeds) | RimWorld Together community-mod dedicated-server prerequisites, installation, persistent data and maintainer deployment instructions. |
| romestead | [Romestead dedicated-server setup guide](https://romestead.wiki.gg/wiki/Romestead_Dedicated_Server_Setup_Guide) (community)<br>[Romestead dedicated-server CPU settings, hotfix 0.25.1_4](https://store.steampowered.com/news/posts/?appids=1805320&enddate=1780080430&feed=steam_community_announcements) (official) | Exact page (1 seed)<br>Exact publisher post (1 seed; pinned content selector) | Community server setup guide plus publisher notes about server CPU settings. The hotfix is version-specific evidence, not a complete administration manual. |
| runescapedragonwilds | [RuneScape: Dragonwilds dedicated-server how-to](https://dragonwilds.runescape.com/news/how-to-dedicated-servers) (official) | Exact publisher website guide (1 seed; reviewed article-body selector) | Installation, configuration, networking, ownership, world management, backups and logging; some limits explicitly refer to version 0.11 (March 2026). |
| rust | [Rust server-hosting documentation](https://wiki.facepunch.com/rust/Creating-a-server) (official_community) | Exact pages (19 seeds) | Rust server installation/update, configuration, ports, RCON, maps, access control, browser metadata and server operation. |
| satisfactory | [Satisfactory publisher support and server scope](https://www.satisfactorygame.com/support/) (official)<br>[Satisfactory dedicated-server wiki](https://satisfactory.wiki.gg/wiki/Dedicated_servers) (official_community) | Exact pages (1 seeds)<br>Directory discovery (1 seeds) | Dedicated-server installation, configuration, networking, administration, saves, maintenance and HTTP API. |
| scum | [SCUM server-hosting topic recommended by a Steam moderator](https://steamcommunity.com/app/513710/discussions/0/603033663617116874/) (official_community)<br>[SCUM dedicated-server setup wiki](https://scum.wiki.gg/wiki/Scum_Dedicated_server_setup) (community) | Exact pages (1 seeds)<br>Exact pages (1 seeds) | SCUM dedicated-server setup, configuration, ports, persistence and server updates. |
| sevendaystodie | [V3.0 server sandbox and save migration notes](https://7-days-to-die.zendesk.com/hc/en-us/articles/50318172509972-V3-0-Dead-Hot-Summer-Release-Note) (official)<br>[7 Days to Die server setup wiki](https://7daystodie.wiki.gg/wiki/Server) (official_community) | Reviewed anonymous article API (1 exact ID)<br>Exact page (1 seed) | Dedicated-server setup, serverconfig.xml, sandbox configuration, saved-world compatibility and update guidance. |
| sonsoftheforest | [Sons of the Forest dedicated-server configuration guide](https://steamcommunity.com/sharedfiles/filedetails/?id=2992700419) (official_community) | Exact pages (1 seeds) | Dedicated-server installation/update, three network ports, self-tests, JSON configuration, ownership, saves and logging. |
| soulmask | [Soulmask private-server guide](https://soulmask.fandom.com/wiki/Private_Server) (official_community; retrieval blocked)<br>[Save-before-restart instructions](https://steamcommunity.com/games/2646460/announcements/detail/4150709171071702966) and [private-server permissions](https://steamcommunity.com/games/2646460/announcements/detail/4374768959777362633) (official; 2024 Early Access) | Exact wiki page (1 seed)<br>Exact publisher feed articles (2 seeds; fixed post selectors) | Historical save-before-restart and ban/mute procedures; no complete current-version installation or cross-map manual established. |
| squad | [Squad server licensing and administration policies](https://www.joinsquad.com/server-licensing-and-administration-policies) (official)<br>[Squad server installation and configuration](https://squad.fandom.com/wiki/Server_Installation) (official_community) | Exact pages (1 seeds)<br>Exact pages (2 seeds) | Dedicated-server installation, configuration, administration and official server-licensing policy. |
| terraria | [Terraria dedicated-server documentation](https://terraria.wiki.gg/wiki/Server) (official_community) | Exact pages (2 seeds) | Vanilla Terraria dedicated-server setup, serverconfig.txt, command-line parameters, ports, saves and server administration. |
| theforest | [The Forest dedicated-server tutorial](https://steamcommunity.com/sharedfiles/filedetails/?id=907906289) (official_community) | Exact pages (1 seeds) | The Forest dedicated-server installation/update, startup flags, server.cfg, ports, saves and administration. |
| unturned | [Unturned dedicated-server documentation](https://docs.smartlydressedgames.com/en/stable/servers/server-hosting.html) (official) | Directory discovery (1 seeds) | Publisher server documentation for installation, SteamCMD, configuration, networking, GSLT/Fake IP, saves, shutdown and hosting rules. |
| valheim | [Valheim dedicated-server guide](https://valheim.com/support/a-guide-to-dedicated-servers/) (official) | Exact pages (1 seeds) | Dedicated-server setup, arguments, world saves/backups, ports, cross-play and administration. |
| vrising | [V Rising current PC dedicated-server instructions](https://raw.githubusercontent.com/StunlockStudios/vrising-dedicated-server-instructions/master/README.md) (official) | Directory discovery (2 seeds) | Current PC V Rising dedicated-server installation/update, settings, saves, networking and RCON; preserve version-specific scope. |
| windrose | [Windrose dedicated-server guide](https://playwindrose.com/dedicated-server-guide) (official) | Exact pages (1 seeds) | Dedicated-server installation/update, startup, server/world JSON configuration, ports, persistence and troubleshooting. |

### Maintaining scope / 维护抓取范围

- Enshrouded's seven articles, [Klei's command-line guide](https://support.klei.com/api/v2/help_center/en-us/articles/360029556192.json)
  and [7 Days to Die V3.0 notes](https://7-days-to-die.zendesk.com/api/v2/help_center/en-us/articles/50318172509972.json)
  are published for anonymous users through the [documented Zendesk API](https://developer.zendesk.com/api-reference/help_center/help-center-api/articles/#show-article-by-locale).
  Only reviewed publisher/module/source/article bindings are accepted. JSON identity,
  locale, public visibility and reviewed canonical hosts are checked before extraction;
  API metadata dates are not text-edit dates or installed-version guarantees.
- [Minecraft Java help](https://help.minecraft.net/help_center/en-us/articles/360058525452)
  uses the official website's public article API, verified from its published frontend
  script. The HTML help page is a client-rendered shell. Its JSON retains the old
  publisher Zendesk canonical host; citations stay on help.minecraft.net. This does
  not enable the Minecraft Wiki sources, which remain reference-only.
- Core Keeper's two 2025-08-14 Steam posts are selected together from one exact
  archived feed page. They describe IP/port/password joins, PC-store compatibility
  and mod matching for 1.1.2.3. The complete README.txt and ARGUMENTS.txt were
  verified in the installed publisher package, but local mutable files are not
  ingested as public web documents or exempted from private-data redaction.
  Their local build association does not certify the newest upstream version.
- Jagex independently publishes the complete Dragonwilds how-to on its public
  game website. This source replaces the inaccessible help-center entry; the
  reviewed Framer body selector excludes navigation. Preserve the guide's
  explicit March 2026 / 0.11 limits when answering later-version questions.
- Soulmask's June/August 2024 publisher articles provide real save and
  moderation instructions through Steam's public HTML feed. They are pinned
  to two exact posts, not a crawl of all news. The April 2026 launch FAQ links
  its cross-map guide to Discord; that guide body has not been synchronized.
  Historical procedures must not be presented as a verified Soulmask 1.0 manual.
- Return to Moria uses two official Freshdesk server directories as long-lived
  discovery entries. Reviewed list and pagination selectors captured all 12
  Server Guides and 20 Server FAQs; article pages terminate discovery, excluding
  unrelated sidebar recommendations. New articles published in those folders are
  discovered automatically within a 128-page source budget. Directory listings
  identify scope, not operator-manual body text or successful synchronization.
  Other folders still require review. The detailed SteamCMD manual remains an
  independent exact source and refreshes its existing body without discovery.
- Rust's scope includes all 18 reviewed Server Hosting pages plus the console
  commands page. New flat wiki pages must be added explicitly.
- Abiotic Factor has five explicit raw-Markdown wiki pages. Their current links
  target GitHub HTML, so newly added wiki pages need their raw URL reviewed.
- Nightingale's PDF is version 0.8. Conan's historical PDF is dated 2020-09-15 and
  remains distinct from the Enhanced UE5 documentation. V Rising is scoped to
  1.1.x PC. Never discard those version differences when citing.
- 7 Days to Die's endorsed wiki uses `/wiki/Server`; the previous
  `/wiki/Dedicated_server` entry returned 404. The verified page includes
  historical examples, not a certified current V3.0 manual. Its reviewed robots
  signal `use=reference`; the corrected URL does not relax that restriction.
- Core Keeper has an official historical patch PDF plus a clearly identified
  community setup guide; no complete first-party operator manual was verified.
  Romestead's wiki has no verified publisher endorsement in this review.
- Romestead's original Steam announcement is a JavaScript page shell. The
  official Steam publisher feed supplies the same hotfix body; its manifest
  selects only post 1833968530892044 and does not discover other announcements.
- Barotrauma's independent developer-authored Steam guides were readable during
  source review, while its wiki returned HTTP 403 during synchronization. The
  Steam guides are dated 2020-05-20, not a verified current configuration manual.

Core Keeper 新增 2025-08-14 两篇官方专服说明，在同一精确 Steam 页面中只提取这两篇正文，
覆盖 1.1.2.3 的 IP/端口/密码直连及跨平台、模组兼容限制。已核实本机发行商随包的
README.txt、ARGUMENTS.txt，但没有把可被本地修改的文件冒充公共网页导入，也未宣称它们是最新上游版本。
Enshrouded 的 7 篇文章、Klei 启动参数与 7 Days to Die V3.0 说明使用正式匿名文章 API；
Minecraft Java 帮助使用官网前端实际调用的同源公开 API。仅接受审核过的发行商、游戏、
来源及精确文章 ID，核验已发布、公开权限、语言与允许的规范主机后提取正文，引用仍
指向官方网页。Minecraft Wiki 的两项来源继续仅供人工参考。API 时间不代表安装版本适用性。
Dragonwilds 使用 Jagex 在游戏官网独立发布的完整开服指南，
保留其中 2026 年 3 月 / 0.11 的版本限制。Soulmask 新增两篇 2024 年官方保存及权限
操作说明；其 2026 年跨地图指南指向 Discord，未取得可同步正文，不能据此声称当前完整手册齐全。

Moria 以两个官方服务端目录作为长期入口，本次目录与分页选择器覆盖全部 12 个指南
和 20 个 FAQ。文章页不继续发现链接，排除侧栏普通游戏问答；两个目录内的新增文章
可在每来源 128 页的预算内自动发现，其他目录仍需复核。目录列表仅用于确认范围，
不是运维正文或同步成功数量。SteamCMD 详细手册保留独立精确来源并刷新已有正文。
Rust 列入全部 18 个 Server Hosting 页面及控制台命令页。Abiotic Factor 的五个 raw 页面、新命名 PDF 和
其他平铺 Wiki 的新增页面，需要目录维护者复核后才能扩大抓取范围。Nightingale
0.8、Conan 2020 旧版 PDF 与 Enhanced UE5、V Rising 1.1.x PC 的版本区别必须保留。
7 Days to Die 官方认可 Wiki 的有效开服页面为 `/wiki/Server`，原
`/wiki/Dedicated_server` 返回 404；正文含历史版本示例，不能当作当前 V3.0 完整手册。
其 robots 声明 `use=reference`，修正地址不改变引用用途限制。
Core Keeper 未核实到完整第一方运维手册；Romestead Wiki 未核实发行商推荐关系。
Romestead 原 Steam 公告返回页面外壳；清单改用 Steam 官方发行商新闻正文，并固定
选择单篇 0.25.1_4 热修公告，不递归抓取其他新闻，不将其表述为完整运维手册。
Barotrauma 开发者的独立 Steam 中英文指南本次可读，Wiki 同步返回 HTTP 403；
Steam 指南发布于 2020-05-20，不能当作已核实的当前完整配置手册。

Run from the repository root / 在仓库根目录运行：

    python -B scripts/verify_game_knowledge.py --check
    python -B scripts/verify_game_knowledge.py --report-stale

The checker is offline. It validates metadata, bounded URLs and module coverage.
The stale report concerns source-directory reviews older than 30 days, not
runtime body freshness. / 脚本只做离线元数据、URL 范围和覆盖检查；过期报告表示来源目录
复核超过 30 天，不能替代运行时正文同步状态。
