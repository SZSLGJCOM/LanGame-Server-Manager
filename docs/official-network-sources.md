# Official network sources / 官方网络来源

[English](#english) | [简体中文](#简体中文)

## English

The source-controlled [official source policy](../crates/app-network/official-sources.json) defines the public assets and installer resources whose origins are verified as equivalent. Chinese interface requests prefer the listed official China origins; English requests prefer international origins, with the other region available for fallback. This is a language-based default preference, not location detection. Resources without a verified China equivalent retain their existing sources. The policy adds no static CDN IP override, paid accelerator, or third-party mirror. The inventory below also covers original-source and third-party integrations; inclusion does not mean a resource has a China CDN or passes through this policy.

Failed sources have a bounded cooldown. Among healthy equivalent sources, language preference comes before recent success; recent success only determines order within the same region. A prior international success therefore cannot indefinitely override the Chinese default. Changing language updates new requests and displayed media without restarting an ongoing installer. Public Workshop and news requests carry their own locale, so different LAN clients do not overwrite a shared language setting. A selected screenshot remains ahead of its fallback cover; regional ordering only applies to equivalent copies of the same resource.

The news panel keeps successful responses for ten minutes under separate game/language/count keys. Changing game or language starts the corresponding request without displaying the previous selection's state, and a failed read offers an explicit retry. News source preference does not translate publisher-authored announcements. Don't Starve Together world import and backup restore capture the request's interface language for the public Workshop metadata validation before preparing required Mods; assistant-initiated restore retains the language bound to its confirmed preview. Later language changes do not reroute that in-progress operation. These preferences do not change native SteamCMD content delivery.

| Resource | Configured delivery |
| --- | --- |
| Steam store images | Akamai, Fastly, and the official Steam China shared CDN; only the declared asset path prefixes |
| Steam description animations | WebM and MP4 under `/store_item_assets/steam/apps/` use the same verified shared CDN origins; image paths are not accepted as videos |
| Steam trailers | Akamai, Fastly, and the official Steam China video CDN; HLS manifests, child playlists and segments use the same source rules |
| Other catalog media | Existing original references, including Minecraft images at `store-images.s-microsoft.com` and trailers at `cdn.trailers.xboxservices.com`; resources outside the equivalence policy retain direct browser delivery without regional substitution |
| Workshop preview images | `images.steamusercontent.com` and `steamuserimages-a.akamaihd.net`; no verified China-specific UGC origin |
| Anonymous Workshop search, localized details and item-type HTML | `steamcommunity.com` and the built-in Steam Akamai connection described below; two exact paths only, without a China-specific mirror |
| Public Workshop details, collections and Steam news | `api.steampowered.com` and `api.steamchina.com`, restricted to three exact read-only API paths and public parameters |
| Steam store full descriptions | Public `IStoreBrowseService/GetItems/v1/` at `api.steamchina.com` and `api.steampowered.com`; Chinese prefers China and English prefers international. Only the verified full-description request shape is eligible; the older international AppDetails endpoint remains a recovery path |
| Steam review summaries | International `store.steampowered.com/appreviews/`; no verified China equivalent |
| SteamCMD Windows bootstrap ZIP | The exact installer URLs at Steam Akamai, Steam Fastly, and `media.steampowered.com`; the last URL has a different path |
| Managed SteamCMD initialization and self-update | Valve's signed win64 manifest and versioned packages at `client-update.steamstatic.com`, Fastly, Akamai, and the Steam media host; packages are size/SHA-256 checked before native SteamCMD applies them |
| Minecraft version metadata | `piston-meta.mojang.com` and `launchermeta.mojang.com`; version details must match the manifest's SHA-1 and selected version ID |
| Minecraft server objects | `piston-data.mojang.com` and `launcher.mojang.com`; upstream SHA-1 and declared size are checked before publication |
| Java prerequisite | Adoptium API, with the official Adoptium GitHub release API as metadata fallback for the same required Java major; Windows x64 HotSpot JRE GA identity, size and SHA-256 are mandatory. The ZIP still comes from the selected official GitHub release, not an independent China CDN |
| Terraria and RimWorld Together server archives | The module's official release URL and bounded retries; RimWorld Together pins its release SHA-256 and byte size; no verified second independent official source |
| Modrinth and Thunderstore metadata and packages | The original provider APIs and their actual package URLs, with bounded retries; Modrinth verifies the metadata's byte size and SHA-512 (SHA-1 only when SHA-512 is absent); no verified alternate source |
| CurseForge slug resolution | `api.curse.tools`, an existing third-party integration; neither a CurseForge-operated API nor an official CDN, and no source substitution |
| ARK Server API extension preparation | Pinned ASE/ASA releases from the extension maintainers' GitHub repositories; archive size, SHA-256 and expected payload are verified; no China-specific alternative |
| ASA extension offset cache | The extension's configured cache services at `cdn.pelayori.com`, `cdn.shadowhunter.co.za` and `cdn.shadowhunter-systems.co.za`; the extension sends the server EXE SHA-256 to retrieve matching files after the installation notice obtains permission; these third-party services are not Valve or the game's official CDN |
| Local knowledge embedding model | IBM Granite's pinned Hugging Face checkpoint and the explicitly allowed Hugging Face delivery hosts; file size/SHA-256, bounded retries and cancellation; no China-specific replacement model or mirror |
| Official knowledge synchronization | The reviewed publisher documentation sources and their approved paths, including scoped publisher help-center APIs; source, redirect and public-address checks remain enforced; no locale-based origin replacement |
| Remote AI and local model providers | The operator's configured endpoint; requests remain with that provider, and loopback endpoints bypass HTTP proxies; no CDN substitution or cross-origin redirect of credentials/context |
| Application updates | Disabled in the default configuration; an explicitly enabled release build uses the configured GitHub release feed and signed update artifacts through the updater; no China-specific feed or CDN replacement |
| WebView2 prerequisite during desktop installation | The Tauri/NSIS `offlineInstaller` flow embeds Microsoft's Evergreen x64 standalone installer at build time; a user missing WebView2 does not need a separate runtime download during installation. Build-time acquisition remains on Microsoft's official delivery path |
| Steam game-server and Workshop content | SteamCMD's native SteamPipe/Workshop delivery; bootstrap URL alternatives do not replace the content protocol |
| Game-server control, LAN management and discovery | Instance endpoints, the authenticated LAN management service and local discovery protocols; these are not public interchangeable CDN resources |
| Websites opened in the system browser | The requested Steam, mod-provider, release or documentation website; browser routing remains under the user's environment |

The full-description API is separate from the Steam China storefront and its catalog. Its narrowly verified public request does not authorize substitution of store, account, login, review or other StoreBrowse operations. The existing CurseForge slug resolver is a third-party integration, not an official CDN, and receives no source substitution. Local game-control endpoints, user-configured AI providers, authenticated services and externally opened websites are not redirected. Application update endpoints and signature verification remain owned by the updater configuration; this policy does not add an update endpoint.

The [ARK extension installer](../apps/desktop/src-tauri/src/ark_tools_install.rs), [model downloader](../crates/app-knowledge/src/embedding_download.rs), [knowledge fetcher](../crates/app-knowledge/src/fetch.rs), [AI endpoint policy](../apps/desktop/src-tauri/src/assistant_http_client.rs), [desktop configuration](../apps/desktop/src-tauri/tauri.conf.json) and [release guide](desktop-release.md) define their respective routes. Third-party server processes may also contact their own account, master-server, telemetry or mod services; LanGame's public-source policy does not redirect those processes. A successful media or metadata request does not prove that game authentication, package acquisition or an extension's later downloads will succeed.

Build and catalog-maintenance traffic is separate from the installed application's runtime. Rust/npm dependency acquisition follows the repository's toolchain and lock files. The [ONNX Runtime build input](../crates/app-knowledge/runtime_build.rs) uses a pinned Microsoft GitHub release archive, an explicit GitHub delivery-host allowlist and SHA-256/size verification; a verified local archive can be supplied through `LANGAME_ORT_ARCHIVE`. The resulting runtime libraries are bundled, so end users do not repeat this build download. The [Rust license generator](../scripts/generate_rust_third_party_licenses.py) may retrieve missing checksum-locked crate archives from `static.crates.io`. [Catalog generation](../scripts/fetch_module_store_data.py) fetches its declared Steam inputs during maintenance; neither generator runs as an implicit startup network check. Build traffic, explicit source-audit probes and the WebView2 bootstrapper are outside the runtime source policy.

Full descriptions first use the two official public API origins, with one positive 32-bit App ID, `schinese`/`english`, `CN`/`US`, and `include_full_description: true`. Additional query keys, duplicate JSON fields and unapproved request fields cannot change origin. Responses must identify exactly the requested application through both `id` and `appid`, with application item type, success and visibility. The full BBCode is converted to escaped HTML, retaining images and looping video sources; the existing sanitizer and regional media routing still apply. The two origins share up to eight seconds, reserving the rest of a 16-second total for the older AppDetails recovery path and its two content languages. Recoverable transport, parsing and identity failures can use another verified source; HTTP 401/403/429, deferred retry windows and explicitly unavailable items stop the operation. First-time full descriptions therefore no longer require `store.steampowered.com` when an official public API origin is reachable. AppDetails recovery retains its `steam_appid` identity check and optional publisher-supplied review quotes; those quotes are not part of the verified full-description field.

On 2026-10-01, read-only requests for Don't Starve Together, Palworld, Valheim, Project Zomboid and Factorio in both languages returned matching full-description text on both official API origins. For three of those games in both languages, normalized text also matched AppDetails `about_the_game`. These samples establish the checked field's semantics, not equivalence of every StoreBrowse field or guaranteed connectivity on every Chinese network. The explicit Rust probe `live_regional_full_descriptions_are_equivalent` checks the production request, identity validation and renderer; `LANGAME_STORY_PROBE_FILE` can save its real HTML outside the repository for the browser media probe.

Successful descriptions are cached for ten minutes within the current frontend session, separately by game and requested language. Sanitized successful HTML also has a bounded, per-browser IndexedDB snapshot cache: at most 64 entries, 256 KiB of HTML per entry and 4 MiB of HTML in total; snapshots older than thirty days are not reused. On a failed or empty live read, a matching snapshot can supply the description with its saved time, an explicit historical-content notice and a retry action. No matching snapshot means the bundled local overview remains available. Failed or empty reads never erase a valid snapshot or update its saved time, and never become successful cache entries; storage denial, corruption or timeout does not prevent live requests or the local overview. This HTML cache is separate from the persistent media cache below and does not guarantee that its referenced images or videos remain available offline.

The description remains rich HTML, including images, GIFs and inline looping videos. Steam currently delivers many apparent GIFs as alternate WebM/MP4 `<source>` elements on its shared asset hosts. Sanitization retains their media types; playback preserves the declared format alternatives and tries another format when decoding fails, while CDN failures retain bounded same-resource fallback. HTML `muted` must be applied to the video property before autoplay. The optional `library-story-live-browser.test.cjs` probe reads descriptions through the selected running Windows service (`LANGAME_STEAM_RUNTIME_PID`) and verifies real image decoding and advancing video playback in Chromium under the desktop CSP. Its standalone browser uses direct CDN delivery; it does not expose or claim to verify the desktop's private cache protocol.

### Workshop HTML routing and connection failures

Anonymous Workshop search, localized details and item-type lookup use a built-in connection to Steam's Akamai endpoint as well as the Community origin. For the CDN candidate, the HTTPS URL and TLS SNI use `steamcommunity-a.akamaihd.net`, while the HTTP `Host` header remains `steamcommunity.com`. Certificate and hostname validation remain enabled for the HTTPS destination. Chinese requests try the CDN candidate first; English requests try the Community origin first. Fallback is finite and shares a 20-second total request budget and an 8 MiB response limit. This CDN route is not a China-specific mirror and does not guarantee availability on every network.

Community reads share one in-flight request across browsing and item verification. Concurrent verification of the same item shares its result, and verified types retain the bounded 15-minute cache. A 429 pauses both Community connections; a server `Retry-After` (seconds or HTTP date) determines the wait, with a 60-second local cooldown when no usable value is supplied. No automatic retry is scheduled. A failed item-type lookup retains public metadata as `unverified` / `unknown`, without authorizing a download or misclassifying the item as a guide. Other successfully verified items remain usable. Warnings and retry actions appear in the desktop's bottom activity bar.

Item details request public API metadata and the selected-language Community page concurrently. When metadata succeeds but the page fails its network, parsing or identity checks, the details retain the API's original title, description and collection members. A visible warning identifies the text as the author's original content and offers a retry; a successful retry replaces it with localized content and clears the warning. That failed details request does not issue additional Community type lookups for the root item or its members. Only authoritative API/collection evidence or an existing valid type cache can establish their types: otherwise they remain unverified and cannot be downloaded. A missing or private item keeps the API's unavailable status even when a page response exists.

Searching an exact item ID or Steam item URL retains an unverified result when the public API confirms that it belongs to the selected game but Community type verification is unavailable. It is shown as pending verification, not as an empty search or an installable Mod. Confirmed other-game items, guides and items outside the selected Mod/collection category remain excluded.

These two HTML connections have a separate, fixed-size failure cache. A failure eligible for source fallback puts that connection behind the other for 60 seconds, before applying the current request's language preference. Both connections remain available; equal health or an expired cooldown restores the language order. An HTTP 200 alone does not clear a failure because the caller must still validate the Workshop content. Authentication, throttling and deferred-retry responses neither switch connections nor create a cooldown that would bypass the same restriction on the next request.

The route is restricted to anonymous `GET` requests on two exact paths. `/workshop/browse/` accepts only `appid`, `section`, `browsesort`, `actualsort`, `p`, `numperpage`, `l`, `days` and `searchtext`; `/sharedfiles/filedetails/` accepts only `id` and `l`. Requests with authentication, cookies, unapproved parameters or other paths are not eligible. Redirects are disabled. The application does not modify system hosts, install certificates, pin CDN IP addresses, or introduce a third-party proxy. HTTP 401, 403, 429 and deferred `Retry-After` windows do not trigger a different source attempt.

Don't Starve Together Workshop browsing uses `/workshop/browse/`. Public item and collection metadata still use the two approved API origins. When the details API omits content type and no authoritative cached evidence exists, `/sharedfiles/filedetails/` supplies the type evidence needed to distinguish a Mod from a guide or other shared content. Entering an item ID can therefore still require a Community HTML response through either connection. This HTML route is independent of SteamCMD initialization and does not replace native SteamPipe/Workshop delivery for game-server packages or Mod payloads. Successful HTML lookup does not prove that those downloads or installation can complete.

The official [QueryFiles API](https://partner.steamgames.com/doc/webapi/IPublishedFileService#QueryFiles) requires a Steam Web API key; it is not an anonymous replacement for HTML search. Search results must contain a recognizable server-rendered catalog for the requested game; the parser preserves its ordered items rather than collecting arbitrary page links. Item-type evidence must identify the exact requested item. Login pages, challenges, guides, mismatched games and unrecognized HTML must not become installable Mods merely because the HTTP status is 200.

On the affected Windows computer, these read-only probes distinguish the relevant routes without installing anything:

```powershell
curl.exe --noproxy "*" --connect-timeout 8 --max-time 20 --silent --show-error --output NUL --write-out "SteamCMD HTTP %{http_code}\n" "https://client-update.steamstatic.com/steam_cmd_win64"
curl.exe --noproxy "*" --connect-timeout 8 --max-time 20 --silent --show-error --output NUL --write-out "Workshop browse HTTP %{http_code}\n" "https://steamcommunity.com/workshop/browse/?appid=322330&l=english"
curl.exe --noproxy "*" --connect-timeout 8 --max-time 20 --max-filesize 8388608 --silent --show-error --header "Host: steamcommunity.com" --output NUL --write-out "Workshop CDN browse HTTP %{http_code}\n" "https://steamcommunity-a.akamaihd.net/workshop/browse/?appid=322330&section=readytouseitems&browsesort=trend&actualsort=trend&p=1&numperpage=30&l=schinese&days=7"
curl.exe --noproxy "*" --connect-timeout 8 --max-time 20 --max-filesize 8388608 --silent --show-error --header "Host: steamcommunity.com" --output NUL --write-out "Workshop CDN item type HTTP %{http_code}\n" "https://steamcommunity-a.akamaihd.net/sharedfiles/filedetails/?id=378160973&l=english"
curl.exe --noproxy "*" --connect-timeout 8 --max-time 20 --silent --show-error --output NUL --write-out "China metadata HTTP %{http_code}\n" --data "itemcount=1&publishedfileids%5B0%5D=378160973" "https://api.steamchina.com/ISteamRemoteStorage/GetPublishedFileDetails/v1/"
curl.exe --noproxy "*" --connect-timeout 8 --max-time 20 --silent --show-error --output NUL --write-out "International metadata HTTP %{http_code}\n" --data "itemcount=1&publishedfileids%5B0%5D=378160973" "https://api.steampowered.com/ISteamRemoteStorage/GetPublishedFileDetails/v1/"
```

`--noproxy "*"` disables curl's HTTP proxy use for the probe; it does not disable a VPN or change system settings. Repeat without that option to compare the proxy environment. HTTP 200 proves an HTTP response, not a successful Mod lookup or installation. Inspecting a saved response must confirm the search page's `window.SSR.renderContext` catalog and the requested game, or native item-type metadata such as `PublishedFileAward` for the exact requested ID; never execute page scripts to extract evidence. Preserve the failing stage, hostname and error category when reporting a problem; never include account credentials, proxy passwords or management tokens.

### Failure and data handling

Image candidates have bounded load time and end at the existing placeholder if all fail. Video loaders own cancellation and never combine partial responses from different sources. HLS uses the actual successful URL as the base for relative child resources. Public HTTP reads have a total time budget, response-size limits and finite retries. HTTP 401, 403, 429 and deferred retry windows do not trigger another source attempt. Signed URLs, credential headers, unknown public parameters and unrelated paths are not transferred to an alternate origin.

Installer metadata reads share a 45-second budget across headers, bodies, validation and source fallback. Metadata and file downloads reserve a share of the remaining deadline for each pending official candidate, so a stalled primary cannot consume every alternate's opportunity. Source timeouts may advance to another candidate; cancellation and the overall operation deadline stop the operation. File writes finish before staging cleanup, and a new candidate starts at byte zero.

Install downloads use unique temporary files, bounded streaming, and the existing verification and publication/rollback transactions. A restart truncates the temporary file before receiving a fresh complete response. A failed or cancelled download cannot publish a partial package. Archive contents still must pass the module's executable check. A source's presence in the policy does not promise that every resource or region is available or faster.

Archive modules can declare `[install.download_integrity_windows]` with a 64-digit hexadecimal `sha256` and a positive byte `size`. Both fields are required when the table is present; update them together with the pinned release URL using publisher-provided metadata. A mismatch rejects the download before extraction and leaves existing installation data intact. Sources without a publisher digest, including the current Terraria archive and Thunderstore package metadata, retain their existing bounded download and installation checks without claiming digest verification.

Store catalog generation accepts HLS playlists or native MP4/WebM trailer files. It prefers HLS and selects a native file when HLS is unavailable; DASH-only entries are excluded. Unsupported entries do not consume the catalog's playable-trailer limit.

### Persistent media cache

The source registry and cache policy verify network origins and delivery behavior; they do not grant copyright permission. Display, caching, LAN delivery and redistribution require a basis under the relevant publisher's terms. See [third-party distribution boundaries](../THIRD_PARTY_NOTICES.md#distribution-boundaries). Runtime cache contents are not source or installer inputs.

Verified public images and videos use persistent caches under the application data directory: `cache/media` for the desktop adapter and `cache/media-service` for the runtime service (Windows base: `%LOCALAPPDATA%/LanGame/ServerManager`). Equivalent CDN origins share a key within a cache; the asset path and approved query parameters, including content versions and UGC transformations, remain part of that key. Changing interface language or restarting the application does not discard cached media. Changed asset URLs or version parameters create new entries.

Fresh hits require no upstream request. Cache freshness follows the origin's cache headers, defaults to seven days when unspecified, and is capped at thirty days. Expired objects use ETag or Last-Modified validation where available; an unchanged response renews the existing bytes. `no-store`, private responses and `Vary: *` are not persisted. A complete stale object may be used on a transport outage only when its cache directives allow it. This cache does not decide whether a publisher's change is a “major update.”

The cache holds up to 2 GiB of media payload and 4,096 entries, with an index limited to 8 MiB and temporary atomic publication requiring up to 16 MiB extra. Least recently used entries are removed from its dedicated, marked directory. Content hashes protect stored objects; temporary files are published atomically. Disk work runs outside the async executor, and accesses are coalesced per resource. Warm hits do not wait for network download slots. Native video uses 1 MiB blocks for Range playback and seeking. HLS explicit byte ranges are returned in full, up to 16 MiB, with the original remote playlist base preserved. Missing blocks require a strong ETag and remain pinned to one source; each native playback lease also retains its accepted source, ETag and total length independently of disk eviction. A representation change restarts playback instead of combining versions. Native no-store ranges, ranges without a strong ETag, full objects over 16 MiB and unsupported media use the existing bounded direct-source path.

The desktop uses an asynchronous `lgsm-media` protocol. Authenticated LAN clients register short-lived opaque media references and reuse the host's disk cache; management tokens never appear in media URLs. Media transfer has separate bounded admission so it does not occupy management request workers. The local adapter disables browser HTTP caching because the persistent cache owns validation. Pure frontend preview mode and sources outside the verified policy retain their original browser delivery.

The shared [media request policy](../crates/app-network/media-request-policy.json) gives a cache request a 20-second backend budget, including resource-lock and worker-admission waits, source fallback and cache storage. Images, posters, textures and native video allow this budget plus eight seconds for client delivery, for a maximum of 28 seconds per cache-proxy request. Direct-source attempts retain their eight-second limit. HLS preserves a caller's shorter direct timeout and adds only the backend budget when using the cache proxy, up to 28 seconds. Explicit failures fall back immediately; successful or cancelled requests clear their timers.

### Evidence and maintenance

The initial mapping was checked on 2026-09-13 against the [international Steam store](https://store.steampowered.com/app/322330/) and [Steam China](https://store.steamchina.com/) native page configuration, then with matching-resource responses. Three SteamCMD URLs returned identical 774,825-byte ZIPs and SHA-256 `7669b170dee42db8ee2273775ed7dfb2d173bdba1b849f70d2c7b379290bce13`; this is verification evidence, not a permanent version pin. Steam video manifests matched byte for byte, and China child-playlist/segment responses supported CORS and byte ranges. The three public API sample responses matched byte for byte. Publisher-authenticated APIs are outside that evidence; see [Steam Web API boundaries](https://partner.steamgames.com/doc/webapi_overview) and [Steam China](https://partner.steamgames.com/doc/store/china).

The Workshop CDN/Host connection method was investigated using the third-party [Steam Community Caddy configuration](https://github.com/linuxjinjin/steam-community/blob/main/steamcommunity_302.caddy.json) and [Steamcommunity 302 documentation](https://www.dogfight360.com/blog/knowledge-base/steamcommunity_302_13_manual/). These are method references, not Valve documentation or a supported-API guarantee. On 2026-09-21, read-only requests from a Windows host without a VPN returned HTTP 200 with the Workshop server-rendered catalog and item-type evidence through this CDN connection using normal TLS validation. Those samples establish the tested paths and content, not universal regional availability. LanGame implements only the bounded anonymous reads described above and does not install or run either reference tool.

Java metadata fallback reads only the public [Adoptium Temurin repositories](https://github.com/adoptium) and requires the [release asset SHA-256 digest](https://docs.github.com/en/rest/releases/assets) before accepting a package. The fallback can select a newer GA patch of the same required major while Adoptium indexing catches up; it cannot select a different major, early-access build, architecture or Java distribution.

The model downloader follows a fixed allowlist of HTTPS delivery hosts from Hugging Face's [official domain metadata](https://huggingface.co/.well-known/meta.json), including its declared AWS, GCP and LFS hosts. It follows the upstream redirect for the pinned object rather than constructing cross-region URLs. Header requests share the normal bounded retry policy; 401/403/429 and deferred retry responses do not restart the outer body-download loop. Completed files retain their pinned size/SHA-256 checks. The `huggingface.cn` and `hf.co` aliases redirect back to the same Hub delivery path in the checked samples and are not configured as independent mirrors.

Minecraft sources follow [Mojang's server download](https://www.minecraft.net/en-us/download/server) and version manifest. Java follows [Adoptium's API contract](https://github.com/adoptium/api.adoptium.net/blob/main/docs/cookbook.adoc). Mod packages use [Modrinth's version metadata](https://docs.modrinth.com/api/operations/getversions/) and [Thunderstore's publisher-maintained API implementation](https://github.com/thunderstore-io/Thunderstore).

Before adding or changing a group, establish publisher ownership, verify the same object and path semantics, and update the policy tests. Do not infer origin equivalence from a domain name alone. Keep automatic tests on controlled fixtures: source failures, body interruption, throttling, size/hash mismatch, cancellation, and successful fallback. Live availability checks are explicit diagnostics, not startup work or unconditional tests.

## 简体中文

来源清单与缓存策略核验网络来源和传输行为，不授予版权使用许可。展示、缓存、局域网提供及再分发须有相应发布者条款依据，见[第三方分发边界](../THIRD_PARTY_NOTICES.md#分发边界)。运行时缓存不作为源码或安装包输入。

已核实来源的图片、视频接入持久缓存：桌面适配器使用应用数据目录下的 `cache/media`，运行服务使用 `cache/media-service`；Windows 基础目录默认为 `%LOCALAPPDATA%/LanGame/ServerManager`。在同一缓存内，相同资源的国内外 CDN 共用副本；重启、切换语言不清空缓存。资源路径、内容版本和图片变换参数参与缓存标识，地址或版本改变后下载新资源。

新鲜缓存直接读取本地，不请求上游。有效期遵循官方缓存头，未声明时默认七天，最长三十天；到期优先用 ETag 或 Last-Modified 检查变化，未变化则继续复用原文件。官方要求禁止保存、私有响应或 `Vary: *` 时不落盘。网络传输故障时，只在缓存指令允许且内容完整的情况下使用过期副本，不依靠猜测“是否大更新”决定刷新。

媒体正文缓存上限为 2 GiB、4,096 个资源，索引最多 8 MiB，原子发布时另需最多 16 MiB 临时空间；满后清理较久未使用的条目，只操作有归属标记的专用目录。文件校验内容哈希并原子发布，磁盘操作在工作线程执行，同资源并发请求合并；本地命中不等待网络下载名额。普通视频按 1 MiB 分块，支持边播边缓存和拖动；HLS 显式字节范围完整返回，上限 16 MiB。分块要求强 ETag 并固定来源；每次普通视频播放还独立保留来源、ETag 与总长度，不受磁盘淘汰影响，检测变化后重新加载。禁止缓存的普通视频、缺少强 ETag 的分块、超过 16 MiB 的整对象及不支持的媒体保留现有直连路径。

桌面和局域网客户端都复用宿主的媒体缓存。局域网媒体使用已认证接口登记的不透明短期引用，管理令牌不写进图片或视频地址；媒体传输独立限流，不占管理请求的处理线程。纯前端预览和官方来源清单外的资源继续使用浏览器原有加载方式。

共用的[媒体请求策略](../crates/app-network/media-request-policy.json)为缓存请求提供 20 秒后端总预算，涵盖资源锁、工作名额等待、候选源回退与缓存存储。图片、封面、纹理及原生视频在客户端另外保留 8 秒传输时间，每次缓存代理请求最多 28 秒；直连候选仍为每源 8 秒。HLS 保留调用方更短的直连超时，仅缓存代理请求追加后端预算，上限同样为 28 秒。明确失败立即回退，不必等满超时；成功和取消均清理计时器。

[官方来源清单](../crates/app-network/official-sources.json)定义了已核实等价来源的公共素材与安装资源。同一资源只有经过核实才允许切换域名；中文界面优先清单中已核实的国内官方源，英文界面优先国际官方源，另一地区作为备用。这是根据语言设置默认偏好，不是检测所在地。没有国内等价源的资源继续使用现有来源，不固定 CDN IP，也不加入付费加速器或第三方镜像。下表同时覆盖原站与第三方集成；列入表中不代表拥有国内 CDN，也不代表经过该换源策略。

失败的来源会暂时冷却；其余候选先按语言偏好排序，再在同一地区优先复用近期成功的来源，避免历史国际成功记录长期压过中文的国内默认。切换语言后，新请求与当前媒体使用新顺序，正在安装的任务不会因此重启。工坊与新闻请求各自携带语言，局域网不同客户端互不覆盖偏好。截图始终优先于备用封面，仅同一资源的等价地址参与地区排序。

资讯面板将成功结果按游戏、语言和条数分别缓存十分钟；切换游戏或语言后发起对应请求，不显示上一选择的状态，读取失败时提供明确的重试入口。资讯来源偏好不会翻译发布者撰写的公告。饥荒联机版导入世界和恢复备份会捕获本次请求的界面语言，用于准备所需 Mod 前的公开工坊元数据校验；助手发起的恢复沿用已确认预览绑定的语言。随后切换界面语言不改写正在执行的操作，也不改变 SteamCMD 原生内容下载协议。

| 资源 | 已接入方式 |
| --- | --- |
| Steam 商店图片 | Akamai、Fastly、蒸汽平台官方国内图片 CDN，仅匹配已声明的资源路径 |
| Steam 简介动图 | `/store_item_assets/steam/apps/` 下的 WebM、MP4 使用同一组已核实的国内外共享 CDN；图片路径不作为视频接受 |
| Steam 视频 | Akamai、Fastly、蒸汽平台官方国内视频 CDN，覆盖 HLS 主清单、子清单与分片 |
| 其他目录媒体 | 保留已有原始引用，包括 `store-images.s-microsoft.com` 的 Minecraft 图片及 `cdn.trailers.xboxservices.com` 的预告片；等价来源策略以外的资源由浏览器直连，不按地区换源 |
| 工坊预览图 | Steam UGC 原域名与官方 Akamai 域名，未确认国内 UGC 专用等价源 |
| 匿名工坊搜索、本地化详情与条目类型网页 | Steam 社区原站与内置 Steam Akamai CDN 连接，仅限下文两个精确路径，不是国内专用镜像 |
| 工坊详情、集合、Steam 新闻 | 国际与中国官方 API，仅允许三个公开只读接口的精确路径和公开参数 |
| Steam 商店完整简介 | `api.steamchina.com` 与 `api.steampowered.com` 的公开 `IStoreBrowseService/GetItems/v1/`；中文优先国内、英文优先国际，仅放行已核实的完整简介请求；原国际 AppDetails 接口保留为恢复路径 |
| Steam 评价摘要 | 国际 `store.steampowered.com/appreviews/` 接口，未确认国内等价接口 |
| SteamCMD 引导包 | Akamai、Fastly、Steam 媒体站的三个精确安装包地址，按各自真实路径请求 |
| 托管 SteamCMD 初始化与自身更新 | Valve 签名 win64 清单和版本包，使用 `client-update.steamstatic.com`、Fastly、Akamai、Steam 媒体站；下载包校验大小与 SHA-256，再由原生 SteamCMD 应用更新 |
| Minecraft 版本元数据 | `piston-meta.mojang.com` 与 `launchermeta.mojang.com`；版本详情校验清单 SHA-1 与所选版本 ID |
| Minecraft 服务器文件 | `piston-data.mojang.com` 与 `launcher.mojang.com`；发布前校验上游 SHA-1 和声明大小 |
| Java 前置 | 优先 Adoptium API，同一所需 Java 主版本的官方 Adoptium GitHub 发行 API 作为元数据备用；必须匹配 Windows x64 HotSpot JRE 正式版身份、大小及 SHA-256。ZIP 仍来自所选官方 GitHub 发行，不是独立国内 CDN |
| Terraria、RimWorld Together 服务端 | 官方发行地址与有限重试；RimWorld Together 固定发行包 SHA-256 与字节数；未确认第二个独立官方备用源 |
| Modrinth、Thunderstore 元数据与模组包 | 原提供方 API 及其返回的实际下载地址，采用有限重试；Modrinth 校验元数据声明的字节数与 SHA-512，仅缺少 SHA-512 时使用 SHA-1；未确认备用等价源 |
| CurseForge slug 解析 | 既有第三方集成 `api.curse.tools`；不是 CurseForge 运营的 API 或官方 CDN，不做换源 |
| ARK Server API 扩展准备 | 扩展维护者 GitHub 仓库中的固定 ASE/ASA 发行包，核验归档大小、SHA-256 和预期文件；无国内专用备用源 |
| ASA 扩展偏移缓存 | 扩展配置的 `cdn.pelayori.com`、`cdn.shadowhunter.co.za`、`cdn.shadowhunter-systems.co.za` 缓存服务；安装说明征得许可后，由扩展发送服务器 EXE 的 SHA-256 获取匹配文件；这些第三方服务不是 Valve 或游戏官方 CDN |
| 本地知识库向量模型 | IBM Granite 固定 Hugging Face checkpoint 及明确允许的 Hugging Face 分发域名，核验文件大小与 SHA-256，支持有限重试和取消；不换成国内其他模型或镜像 |
| 官方知识库同步 | 已审核的发布方文档来源及允许路径，包括限定范围的发布方帮助中心 API；保留来源、重定向与公网地址校验，不按语言替换域名 |
| 远程 AI 与本地模型服务 | 操作者配置的端点，请求保持在该提供方，回环地址绕过 HTTP 代理；不换 CDN，也不跨来源重定向凭据或上下文 |
| 应用更新 | 默认配置关闭；显式启用的发行构建通过更新器使用配置的 GitHub 发行源及签名更新包；没有国内专用更新源或 CDN 替换 |
| 桌面安装时的 WebView2 前置 | 按 Tauri/NSIS 的 `offlineInstaller` 配置，在构建时嵌入微软 Evergreen x64 独立安装程序；用户缺少 WebView2 时，安装阶段无需另行下载运行时。构建时仍使用微软官方分发路径 |
| Steam 服务端与工坊 MOD 文件 | 继续由 SteamCMD 原生内容协议下载；引导包 CDN 不用于替换游戏内容地址 |
| 游戏服务器控制、局域网管理与发现 | 实例端点、已认证的局域网管理服务及本地发现协议，不属于可互换的公共 CDN 资源 |
| 系统浏览器打开的网站 | 请求的 Steam、模组提供方、发行或文档网站，连接继续由用户的浏览器环境管理 |

完整简介使用公开 API，独立于蒸汽平台商店及其游戏目录；本次核实的窄范围请求不允许替换商店、账号、登录、评价或其他 StoreBrowse 操作。已有 CurseForge 链接解析服务属于第三方集成，不标记为官方 CDN，也不换源。局域网游戏控制、用户配置的 AI 服务、需要认证的接口和外部浏览器网站不做域名替换。应用更新地址与签名验证仍由现有更新配置负责，不自行新增发行地址。

[ARK 扩展安装器](../apps/desktop/src-tauri/src/ark_tools_install.rs)、[模型下载器](../crates/app-knowledge/src/embedding_download.rs)、[知识库抓取器](../crates/app-knowledge/src/fetch.rs)、[AI 端点策略](../apps/desktop/src-tauri/src/assistant_http_client.rs)、[桌面配置](../apps/desktop/src-tauri/tauri.conf.json)及[发行指南](desktop-release.md)分别定义各自的链路。第三方游戏服务端进程还可能访问自己的账号、主服务器、遥测或模组服务；LanGame 的公共来源策略不改写这些进程。媒体或元数据读取成功，不代表游戏认证、安装包获取或扩展后续下载也能成功。

构建及目录维护联网与已安装应用的运行时分开：Rust/npm 依赖获取遵循仓库工具链与锁文件；[ONNX Runtime 构建输入](../crates/app-knowledge/runtime_build.rs)使用固定的微软 GitHub 发行归档、明确的 GitHub 分发域名白名单及 SHA-256/大小校验，也可通过 `LANGAME_ORT_ARCHIVE` 提供经过校验的本地归档。生成的运行时库随应用打包，终端用户不重复此构建下载。[Rust 许可证生成器](../scripts/generate_rust_third_party_licenses.py)可从 `static.crates.io` 补取受锁文件校验和约束的缺失 crate 归档；[目录生成器](../scripts/fetch_module_store_data.py)仅在维护时读取声明的 Steam 来源，二者都不作为隐式启动联网检查。构建流量、显式来源审计探针及 WebView2 引导程序均在运行时来源策略之外。

商店完整简介优先使用两个官方公开 API，严格限定单个正数 32 位 App ID、`schinese`/`english`、`CN`/`US` 和 `include_full_description: true`。额外查询参数、重复 JSON 字段及未经允许的请求字段不会换源。响应必须唯一匹配请求游戏的 `id` 与 `appid`，且条目类型、成功及可见状态有效。完整 BBCode 转为经过转义的 HTML，保留图片与循环视频的多格式来源，再经过既有前端清洗及媒体地区排序。两个官方 API 共用最多八秒，整个请求共十六秒，剩余时间留给旧 AppDetails 接口及其两种内容语言。连接、解析或身份校验错误可尝试下一官方来源；HTTP 401/403/429、要求延后重试和明确不可用条目直接停止。只要任一公开 API 可达，首次完整正文不再依赖国际商店域名。旧 AppDetails 恢复路径保留 `steam_appid` 核验及可选媒体评价引述；这些引述不属于已验证的完整简介字段。

2026-10-01 对饥荒联机版、幻兽帕鲁、英灵神殿、僵尸毁灭工程和异星工厂的中英文只读请求，两个官方 API 返回的完整简介逐项一致；其中三款游戏的两种语言，去除排版后的正文也与 AppDetails 的 `about_the_game` 一致。这证明的是所测字段的语义，不代表所有 StoreBrowse 字段等价或所有国内网络始终可达。显式 Rust 探针 `live_regional_full_descriptions_are_equivalent` 检查生产请求、身份校验和渲染；设置仓库外的 `LANGAME_STORY_PROBE_FILE` 可以保存真实 HTML，供浏览器媒体探针继续验证。

成功简介按游戏和请求语言在当前前端会话中缓存十分钟。清洗后的成功 HTML 另存入当前浏览器的有界 IndexedDB 快照：最多 64 条，每条 HTML 上限 256 KiB，总 HTML 上限 4 MiB，超过三十天的快照不再复用。实时读取失败或返回空内容时，可显示匹配快照，并明确标示保存时间、历史内容提示及重试入口；没有匹配快照时继续显示内置本地概览。失败或空读取不删除有效历史快照、不刷新保存时间，也不作为成功结果缓存；存储被禁用、损坏或超时不会阻止实时请求与本地概览。该 HTML 缓存独立于媒体持久缓存，不保证正文引用的图片或视频离线可用。

简介继续完整保留图片、GIF 和内嵌循环视频。Steam 目前把不少视觉上的动图作为共享资源域名下的 WebM／MP4 多个 `<source>` 返回。清洗保留媒体类型，播放器保留备用格式，解码失败可换格式，CDN 故障仍按同一资源有界切换；自动播放前把 HTML 的 `muted` 属性同步到视频对象。显式设置 `LANGAME_STEAM_RUNTIME_PID` 后，`library-story-live-browser.test.cjs` 会从指定的当前 Windows 后台服务读取真实简介，在应用同一 CSP 下检查 Chromium 图片解码和视频时间推进。该独立浏览器走真实 CDN 直连，不暴露桌面私有缓存协议，也不把它作为桌面缓存协议已验证的证据。

### 工坊网页连接与故障排查

匿名工坊搜索、本地化详情与条目类型查询内置 Steam Akamai CDN 和社区原站两个连接候选。CDN 候选的 HTTPS URL 与 TLS SNI 使用 `steamcommunity-a.akamaihd.net`，HTTP `Host` 保持 `steamcommunity.com`，仍严格校验 HTTPS 目标的证书与主机名。中文请求先尝试 CDN，英文请求先尝试社区原站；失败后有限回退，共享每次请求总计 20 秒预算与 8 MiB 响应上限。该线路不是国内专用镜像，也不保证所有网络始终可用。

这两个网页连接使用独立、固定容量的失败缓存。可换源的故障会让对应连接冷却 60 秒，先尝试另一连接，再按本次请求的语言偏好排序；两个候选始终保留，健康状态相同或冷却到期后恢复语言顺序。单凭 HTTP 200 不清除失败记录，因为调用方还需校验工坊内容。认证、限流与要求延后重试的响应既不触发换源，也不建立会让下一次请求绕过同类限制的冷却记录。

此连接仅允许两个精确路径的匿名 `GET` 请求：`/workshop/browse/` 的参数白名单为 `appid`、`section`、`browsesort`、`actualsort`、`p`、`numperpage`、`l`、`days`、`searchtext`；`/sharedfiles/filedetails/` 仅允许 `id`、`l`。含认证信息、Cookie、其他参数或其他路径的请求不进入该线路，禁止跟随重定向。不修改系统 hosts、不安装证书、不固定 CDN IP，也不引入第三方代理。HTTP 401、403、429 或要求延后重试的 `Retry-After` 不会触发换源绕过。

浏览与类型核验共用一个正在执行的社区请求；同一条目的并发核验共享结果，已核验类型继续使用有界的 15 分钟缓存。收到 429 后，两条社区连接共同暂停：按服务端 `Retry-After`（秒数或 HTTP 日期）等待，没有可用值时采用 60 秒本地冷却，不安排自动重试。单项类型核验失败时保留已取得的公开资料，以 `unverified` / `unknown` 表示待核验，不放行下载，也不误判成指南；其他核验成功的条目仍可使用。警告和重试入口统一显示在桌面底部动态栏。

条目详情并行读取公开 API 元数据与当前语言的社区网页。元数据成功但网页联网、解析或身份校验失败时，详情保留 API 返回的原始标题、说明和集合成员；界面明确提示正在显示作者原始内容，并提供重试，成功重试后替换为本地化内容并清除警告。该次失败的详情请求不再为根条目或成员追加社区类型查询；只有可信 API/集合证据或已有有效类型缓存可以确定类型，其余保持待核验且不能下载。缺失或私有条目仍按 API 标记为不可用，不因收到网页而改成成功。

搜索精确条目 ID 或 Steam 条目 URL 时，若公开 API 已确认条目属于当前游戏，但社区类型核验不可用，会保留待核验结果，不显示成空搜索，也不当作可安装 Mod。已经确认属于其他游戏、指南或不符合当前 Mod/合集分类的条目仍被排除。

饥荒联机版工坊搜索通过 `/workshop/browse/` 获取列表。条目与集合元数据仍使用国内外两个已核实的官方 API；详情 API 缺少内容类型且没有可信缓存证据时，使用 `/sharedfiles/filedetails/` 区分 Mod、指南和其他内容。因此，粘贴工坊 ID 仍可能需要经上述任一连接读取社区网页。此网页连接独立于 SteamCMD 初始化，也不替换游戏服务器包和 Mod 内容所用的原生 SteamPipe/Workshop 下载协议。网页查询成功不代表下载与安装也能完成。

Java 元数据备用入口只读取公开的 [Adoptium Temurin 官方仓库](https://github.com/adoptium)，要求发行资产提供有效 SHA-256。Adoptium 索引同步期间，备用入口可能选中同一所需主版本的较新正式补丁，不允许换主版本、预览版、架构或 Java 发行版。

模型下载器按 Hugging Face [官方域名元数据](https://huggingface.co/.well-known/meta.json)维护固定 HTTPS 分发白名单，包含其声明的 AWS、GCP 与 LFS 主机；跟随固定对象的官方重定向，不自行拼接跨地区对象地址。响应头请求复用有界重试策略，401/403/429 和要求延后重试的响应不会被外层正文下载循环重新请求；完整文件仍按固定大小及 SHA-256 校验。已检查样本中的 `huggingface.cn`、`hf.co` 会返回同一 Hub 分发链路，因此未将其配置为独立镜像。

官方 [QueryFiles 接口](https://partner.steamgames.com/doc/webapi/IPublishedFileService#QueryFiles)要求 Steam Web API 密钥，不能直接替代匿名网页搜索。搜索响应必须含所请求游戏的可识别服务端渲染目录，保留其真实条目顺序，不能把页面上任意链接当作搜索结果；条目类型证据必须匹配请求 ID。登录页、验证页、指南、其他游戏内容或无法识别的 HTML，不能因为 HTTP 200 就成为可安装 Mod。

在发生故障的 Windows 电脑上执行英文部分的只读命令，可分别检查 SteamCMD 更新清单、社区原站搜索、CDN 搜索与条目类型页面、两个元数据 API，不会安装任何文件。`--noproxy "*"` 只让本次 curl 请求不使用 HTTP 代理，不会关闭 VPN 或更改系统设置；去掉该选项可比较代理环境。HTTP 200 仅证明收到 HTTP 响应；进一步检查响应时，应核对 `window.SSR.renderContext` 中的搜索目录与游戏归属，或与指定 ID 匹配的 `PublishedFileAward` 等原生类型证据，不执行网页脚本。报告问题时保留失败阶段、域名与错误类别，勿附带账号凭据、代理密码或管理令牌。

该 CDN 与 Host 连接方法参考了第三方 [Steam Community Caddy 配置](https://github.com/linuxjinjin/steam-community/blob/main/steamcommunity_302.caddy.json)和 [Steamcommunity 302 使用说明](https://www.dogfight360.com/blog/knowledge-base/steamcommunity_302_13_manual/)，二者均不是 Valve 官方文档或稳定 API 承诺。2026-09-21 在未使用 VPN 的 Windows 主机上，以正常 TLS 校验完成只读请求，CDN 路径返回 HTTP 200、工坊服务端渲染目录及条目类型证据。样本证明的是所测路径和内容，不代表所有地区始终可用；应用仅内置上述有界匿名读取，不安装或运行这些参考工具。

图片和视频有有限的候选源与超时，全部失败后显示既有占位内容。视频取消时会清理请求，不混合不同来源的残缺分片。HTTP 请求预算覆盖响应头与正文；遇到权限错误、限流或服务器要求延后重试时停止当前逻辑请求。签名 URL、认证头、未获准的参数和无关路径不会转移到备用来源。

安装元数据的响应头、正文、校验与来源回退共用 45 秒预算。元数据与文件下载会为尚未尝试的官方候选分配剩余时间，避免慢速主源耗尽所有备用源的机会。单个来源超时可进入下一候选；取消或整个操作超时则终止。文件写入完成后才清理暂存文件，换源从零重新写入。

安装包以流式方式写入独立临时文件，中断重试从头重新写入；保留校验、原子替换和回滚流程。下载失败或取消不会发布半成品，解压后仍检查模块声明的服务器文件。清单中的来源经过公开官方配置与同资源样本核对，不能据此保证所有地区、所有资源始终可用或更快。来源依据和维护要求见英文部分；这些探测不会在程序启动时自动运行。

归档模块可通过 `[install.download_integrity_windows]` 声明 64 位十六进制 `sha256` 与正整数的字节数 `size`；声明此表时两个字段均必需。RimWorld Together 已固定官方发行包的摘要与大小，更新发行 URL 时必须依据发布方元数据同步更新这两个值。校验失败会在解压前拒绝下载，保留已有安装数据。当前 Terraria 归档与 Thunderstore 包元数据没有可用的发布方摘要，仍采用既有有界下载与安装检查，不宣称具备摘要验证。

商店素材采集只接受 HLS 清单及原生 MP4/WebM 文件，优先 HLS，不可用时选择原生视频；只有 DASH 的条目不会进入目录。不支持的条目不占用可播放视频数量上限。
