# 桌面更新元数据服务

统一入口为 `https://langame.cn/updates/server-manager/latest.json`。现有 Nginx
只提供几 KiB 的静态 Tauri 清单：APNIC 中分配给 CN 的来源地址收到 GitCode 固定版本
附件地址，其余收到 GitHub 原清单。安装包、签名附件和大文件不经过官网服务器。
本目录提供部署契约；生成器不上传、不修改 Nginx、不创建账户，也不启动常驻服务。

## 数据流与边界

1. 正式发行流程生成小包、签名和 GitHub `latest.json`，验证构建回执、安装包哈希与
   Tauri 签名。把**同一安装包字节**上传到受控 GitCode 附件仓库，下载核验哈希、签名及
   匿名可用性后，取得固定版本的永久下载 URL。使用 GitCode 官方文档定义的入口：
   `https://api.gitcode.com/api/v5/repos/SZSLGJCOM/LanGame-Server-Manager-Releases/releases/vVERSION/attach_files/NAME/download`。
   同步流程先确认附件存在，再通过此入口匿名回读并验证完整字节；文档中的 API 形式
   不代表本仓库附件已经上传或已证实可匿名读取。也接受已在真实 GitCode 分发链上观察到
   的精确网页永久形式 `https://gitcode.com/SZSLGJCOM/LanGame-Server-Manager-Releases/releases/download/vVERSION/NAME`；
   该形式用于本仓库前同样必须完整匿名回读核验。除此之外的路径和临时带签名 CDN URL
   均拒绝，也不在客户端/元数据里附带 token。
2. 从 [APNIC 官方目录](https://ftp.apnic.net/stats/apnic/) 保存标准
   `delegated-apnic-YYYYMMDD` 文件，并在发行证据中记录来源、日期及校验结果。官方目录
   提供 `.asc` 和 `CURRENT_PUBLIC_KEY`，密钥信任需独立核实。生成器检查完整记录数、
   摘要数和日期，接受最近 31 天的数据；这些检查不能证明文件真实来源。
3. 离线生成部署包。全球清单保留输入原字节；CN 清单只替换 `windows-x86_64.url`，
   版本、发布说明、日期、签名与包名保持不变。当前只支持正式 Windows x64 小包。
   GitCode URL 必须显式提供，且必须精确匹配受控仓库、版本及包名；未知地址时停止，
   不生成假链接或把“已验证”标记当作密码学证据。

```powershell
$verifiedManifest = 'C:\release-input\latest.json'
$apnicFile = 'C:\release-input\delegated-apnic-YYYYMMDD'
# 从已核验的 GitCode 发行页取得，不用猜测值替代。
$gitcodeAssetUrl = Read-Host 'Verified permanent GitCode installer URL'
python -B scripts/prepare_desktop_update_service.py `
  --verified-manifest $verifiedManifest --apnic-file $apnicFile `
  --gitcode-url $gitcodeAssetUrl `
  --deploy-root /var/lib/langame/desktop-updates/releases/RELEASE-ID `
  --output-dir C:\release-output\desktop-updates-RELEASE-ID
```

输出目录必须在仓库外且尚不存在。输出包括 `latest-global.json`、`latest-cn.json`、
`china-cidrs.conf`、`http.conf`、`server-location.conf` 和带输入/输出 SHA-256 的
`deployment.json`。后者明确说明仅做结构和公钥 ID 检查，没有验签或请求远端附件。
无秘密、私钥、账户凭据或用户 IP 写入生成文件。

## 匿名定时刷新

同步流程只有在七个正式附件全部回读、校验后，才用一次带并发检查的提交更新 GitCode
`main` 的 `updates/server-manager/release.json`。它是最后发布的完整版本指针，不能从
未完成的 Release、`latest` 标记或浏览器页面推测版本。当前指针结构为：

- `schema_version: 1`、`version`。
- `github: {repository, release_id, tag}`；仓库固定，tag 必须为 `v` 加同一版本。
- `manifest` 为 GitHub `latest.json` 对象；`manifest_text` 为该文件的原始 UTF-8 文本。
  两者必须一致，并以原文本字节验证 `latest.json` 附件的 size/SHA-256。
- `gitcode_url` 必须指向本版本小包的固定附件 API 或已核验网页永久地址，且精确等于其
  `assets` 记录 URL。每个附件分别进行同样的精确校验，不接受域名/文件名子串匹配。
- `assets` 恰好七项，每项只有 `name`、`size`、`sha256`、`url`：小包及 `.sig`、
  离线包及 `.sig`、`latest.json`、`SHA256SUMS`、`SHA256SUMS.offline`。

生产者和消费者共用
`scripts/refresh_desktop_update_service.py::validate_release_pointer(pointer, public_key)`。
该函数仅校验结构、版本、URL、记录一致性及小包签名格式/公钥 ID；附件的真实哈希与
两份 Minisign 签名由同步发布流程验证。不能把 pointer 内自述的哈希当作独立验签证据。

服务器无 token，固定匿名读取：
`https://api.gitcode.com/api/v5/repos/SZSLGJCOM/LanGame-Server-Manager-Releases/raw/updates/server-manager/release.json?ref=main`。
使用 GitCode 官方的[获取 raw 文件 API](https://docs.gitcode.com/docs/apis/get-api-v-5-repos-owner-repo-raw-path/)，
其中 `ref=main` 固定读取最后提交的完整版本指针。2026-10-08 已在本机和部署服务器上
不带 token、Cookie、代理或重定向地读取 0.0.3 的完整 5,461 字节指针；两端 SHA-256
一致，与发布提交内容一致，并通过生产 `validate_release_pointer`。该版本的七个附件
也已通过匿名完整回读、SHA-256 及两份安装包签名验证。网站 `raw.gitcode.com` 预览链接
对此 JSON 返回 403“暂不支持预览”，不能由 README 可读推断指针也可读。
服务只做一次最多 256 KiB 的 HTTPS 请求，不采用环境代理、
不跟随重定向、没有备用任意 URL。指针不满足契约时保留已有 current 并失败退出。

将三个 Python 文件 `refresh_desktop_update_service.py`、`prepare_desktop_update_service.py`、
`generate_desktop_update_manifest.py` 放到 root 管理的 `/opt/langame-desktop-updates/`。
需要 Python 3.11+，不需要 pip 依赖。将应用的**公开** updater key 存到 root 管理的
`/etc/langame-desktop-updates/desktop-updates.pub`，不得放入私钥。独立目录由 root 拥有且为
`0755`，公钥文件为 `0644`，让专用服务用户能读取；不要放宽现有 `/etc/langame` 等其他
服务配置目录的权限。管理员预建无交互登录的 `lgsm-updates`
用户及组；它只能写自己的 `/var/lib/langame/desktop-updates` 状态目录。
本目录的 `.service` / `.timer` 是待管理员审阅接入的模板，不会被生成器自动安装。

Nginx 部署生成器增加：

```text
--manifest-root /var/lib/langame/desktop-updates/current
```

此时 `--deploy-root` 中的 APNIC CIDR 是独立、只读、按管理员部署更新的路由表；Nginx
map 的清单地址指向 timer 管理的 `current`。刷新器不下载 APNIC、不更新路由表、不运行
Nginx reload。APNIC 表仍须单独按期刷新并 `nginx -t` / reload；31 天检查只在表生成时
执行，timer 不证明已部署的表仍新鲜。

每个通过校验的新版本写入 `revisions/VERSION-IDENTITY/`，文件完整写入并 fsync 后创建
相对 symlink，再通过原子 rename 切换 `current`。共享锁拒绝并发刷新；旧版本目录保留供
管理员审查/回退，不自动删除。版本倒退、同版本不同 identity、已有目录内容变化均拒绝。
重试相同完整版本只返回 `unchanged`，不会改 current。身份 hash 使用严格字段的稳定 JSON
序列化，附件排列顺序不影响身份。清单不缓存文件句柄，切换不需要 reload。

首次部署没有成功的完整 pointer 时，`current` 保持不存在，Nginx 返回 **404**，不能返回
示例清单、空清单或假成功。刷新失败时已安装旧清单仍可读取；这仅证明已有版本可用，
不证明已取得最新版本。以 `systemctl status langame-desktop-updates.service`、服务退出码
和 journal 中 `published` / `unchanged` 结果核查刷新状态；不要把 timer 活跃当作发布成功。
网络读取有 deadline，systemd 另设 60 秒总上限；每五分钟执行一次，不在失败任务内重试。
首次读取成功不保证后续 API 可用性；新指针可见性仍取决于服务下次成功刷新，并非即时推送。

已发布 pointer 的记录需要修正时提升版本，不能在同版本下静默替换签名或附件。紧急回退
由管理员显式停 timer、核验旧目录后恢复旧 symlink；默认服务会拒绝从远端触发的降级。
镜像发布者不需要生产 SSH key 或服务器命令权限。

## 本机自动镜像

发布工作站的 `LanGameDesktopReleaseMirror` 计划任务每五分钟匿名检查 GitHub 最新正式
Release，以当前用户、非管理员权限运行，需要开机、登录和已配置的 GitHub 代理可用。
部署目录为 `%LOCALAPPDATA%\LanGameReleaseMirror`，其中包含启动脚本、私有配置、固定
发布器快照、状态及诊断记录；任务不依赖开发工作树或临时对话目录，也不自动拉取执行
远程代码。发布器的四个 Python 文件、Node 验签器及公开 updater 配置逐一固定哈希。
后续版本从 Release 和签名可信注释读取，快照配置中的旧版本号不会锁死未来发版。
更换公钥、发布器或运行工具时需要重新核对快照及哈希。

GitCode classic 令牌实际授予账户级“项目读写”，不能描述为单仓库权限；发布器在代码中
限制目标仓库。令牌以 Windows CurrentUser DPAPI 加密，目录与文件仅当前用户和 SYSTEM
可访问，解密值只传入本次 Python 子进程环境，不放进参数、日志或仓库。服务器刷新仍为
匿名读取。GitHub 请求沿用明确配置的本机代理，GitCode API、下载和上传显式绕过代理，
避免国内附件绕境外上传；这只是网络路径策略，不能代替实际地区和速度验收。

GitHub 的 `sync-gitcode-release.yml` 工作流须保持 `disabled_manually`。本机每次新发布
前检查它已禁用且无待运行或运行中的任务，再读取令牌；原有云端 Secret 不会被读取或
自动撤销。切换发布位置前先停当前发布者，并确认没有在途上传。

发布前写入持久化 intent 并持有独占锁。七个附件完整回读、验签及最终 pointer 发布回执
全部成功后，才将状态记为 `succeeded`。上传超时、断电或结果不明会保留 `running` 或
`needs_attention`，阻止后续自动上传；不能直接删状态重新跑。先检查远端附件、完整字节
和 pointer 提交结果，再为已查明的状态安排新的发布尝试。每次尝试保留独立产物及日志，
不自动删除。定时检查成功或任务处于 Ready 均不证明新版本已经同步成功。

```powershell
Get-ScheduledTask -TaskName LanGameDesktopReleaseMirror
Get-ScheduledTaskInfo -TaskName LanGameDesktopReleaseMirror
Get-Content "$env:LOCALAPPDATA\LanGameReleaseMirror\state.json"
```

首次部署只检查版本、不需要发布时，`state.json` 可以尚不存在。仅检查失败的诊断写入
`last-check.json`；完整发布证据位于对应尝试目录。暂停使用 `Disable-ScheduledTask`，
它只阻止后续触发，不会中断已开始的上传；先核实当前尝试是否仍在执行。

## Nginx 1.24 接入前提

- `http.conf` 只能在 `http {}` 中 include 一次；`server-location.conf` 只能在现有
  `langame.cn` HTTPS `server {}` 中 include。生成文件不改变证书、监听地址或其他站点。
  确认 Nginx 没有通过 `--without-http_geo_module` 移除原生 `geo`。
- 将完整包放到 `--deploy-root` 指定的**独立版本目录**，该目录必须位于所有站点的
  webroot 之外，仅给 Nginx 必要读取权限；不能同时提供可访问的 CN/global 清单 URL。
  路由只接受精确 `latest.json` 路径，安装包没有 `proxy_pass` 或 alias。
- 地域输入固定为 `$remote_addr`，没有 `X-Region`、查询参数或客户端 Cookie 覆盖。
  直接 ingress 时，它必须是实际 TCP 客户端 IP。检查完整 `nginx -T` 中是否继承了
  `real_ip_header` / `set_real_ip_from`：不得信任所有来源或未经验证的转发头。
  如站点实际位于可信反向代理/CDN 后，先单独核实并配置精确受信任代理范围及真实 IP
  契约；本片段不自动启用 realip，也不猜代理范围。未满足此条件不能宣称按用户地区分流。
- APNIC 的 `CN` 是资源最初分配组织的国家，**不是当前使用地点的权威 GeoIP 数据**。
  VPN、境外使用的 CN 地址及转移记录可能误分。它只用于选择下载地址，不能用于身份、
  权限或地域访问控制；客户端仍应保留经签名验证的其他下载源。
- 精确入口设置 `Cache-Control: private, no-store`，关闭 ETag、If-Modified-Since 与 Range，
  避免共享缓存/304复用另一地区清单。站点前方 CDN 必须对此路径明确绕过缓存。
  Nginx 1.24 的 `add_header` 会改变父级头部继承；接入时检查并保留现有站点必要安全响应头。

## 发布、验证与回退

先部署和验证固定版本附件，再生成清单。保留上一部署包和已有 include 配置；完整上传新
版本目录并核对 `deployment.json` 后，更新两处 include 引用，执行 `nginx -t`，成功才
reload。不要原地覆盖正在使用的清单。失败时保留旧配置/旧进程，修复或回退 include 后
再次验证；生成器不代替这一步部署验收。

上线前至少检查：Nginx 1.24 真实语法；CN IPv4/IPv6 与非 CN 来源各取一次清单；同一来源
附加伪造 `X-Region`/`X-Forwarded-For` 不改变选择；GET/HEAD 和禁止方法；响应无共享
缓存/304；两清单除了 URL 完全相同；两个附件下载哈希相同且通过内置公钥验签。
必须使用真实来源或隔离测试 Nginx 的测试连接，不能在正式入口加入调试地域头。
记录实际客户端到下载、签名验证、安装和重启的结果；静态配置测试不等于在线升级成功。

APNIC 数据按发行流程刷新；超过 31 天需重新取得。原先只内置 GitHub feed 的旧客户端
无法通过部署此服务直接迁移；需要原 GitHub 更新通路或手动安装带新入口的正式版本。

公共代理不是本服务的存储后端。`gh-proxy.org` 的公开用途文档侧重开源软件，尚未确认
本项目源码可见许可是否适用；`ghfast.top` 未公开明确大规模自动更新额度/SLA；
`update.hwdns.net` 曾对本项目附件返回 403。可达样本不代表授权或保障，不把代理宣传为
无限免费、官方托管或已签约服务；未经授权不联系运营方、不注册或付费。

## 验证与原始资料

```powershell
python -B -m unittest scripts.tests.test_desktop_update_service scripts.tests.test_refresh_desktop_update_service
```

- [Nginx geo：连接 IP、IPv4/IPv6 CIDR、include](https://nginx.org/en/docs/http/ngx_http_geo_module.html)
- [Nginx map](https://nginx.org/en/docs/http/ngx_http_map_module.html)
- [Nginx alias](https://nginx.org/en/docs/http/ngx_http_core_module.html#alias)
- [Nginx realip 信任范围](https://nginx.org/en/docs/http/ngx_http_realip_module.html)
- [APNIC 格式与国家字段限制](https://www.apnic.net/about-apnic/corporate-documents/documents/resource-guidelines/rir-statistics-exchange-format/)
- [GitCode 官方固定版本附件下载 API](https://docs.gitcode.com/en/docs/apis/get-api-v-5-repos-owner-repo-releases-attach-files-file-name-download/)
- [GH-Proxy 服务条款](https://gh-proxy.com/terms)、[GHFast 服务联系页](https://ghfast.top/contact)

原子 symlink、锁及故障恢复的五个发布测试仅在 POSIX/Linux 运行；Windows 不具备同一
symlink 权限/语义时明确跳过。部署验收必须在 Linux 跑完整套件，并追加真实 Nginx 1.24
语法与 HTTP 行为检查，不能把 Windows 部分通过宣称为服务器验收完成。
