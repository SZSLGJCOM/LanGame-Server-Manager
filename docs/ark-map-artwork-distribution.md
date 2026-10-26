# ARK 底图分发

图片清单与坐标取自 `apps/desktop/src/data/ark-map-artwork.json`；当前清单为 2026-10-25 核对的 19 张 ASE/ASA 底图。图片在版本化资源目录公开分发，客户端按需下载，不内嵌在前端或安装包中。

## 已发布资源

2026-10-25 经用户授权发布 19 张原始 JPEG（30,481,377 字节）及 README、NOTICE、manifest，两端均完成 22 个文件的匿名回读和逐文件 SHA-256 校验。原始图片已独立完成真实解码与尺寸核对，远端字节与这些已解码原图一致。

- [GitHub 资源目录](https://github.com/SZSLGJCOM/LanGame-Server-Manager/tree/36836ffe1dc0688e29bcae4abe64e39fe42beba8/assets/ark-map-artwork/2026-10-25/)：提交 `36836ffe1dc0688e29bcae4abe64e39fe42beba8`。
- [GitCode 资源清单](https://api.gitcode.com/api/v5/repos/SZSLGJCOM/LanGame-Server-Manager-Releases/raw/assets/ark-map-artwork/2026-10-25/manifest.json?ref=4d0b496715e04cffc3d1e94a9ee38215788504c4)：完整批次提交 `4d0b496715e04cffc3d1e94a9ee38215788504c4`。

`crates/app-network/official-sources.json` 是运行时来源与摘要的权威配置：每张图仅允许两个固定提交的精确 URL，限定 image 用途，不放行整个 GitHub/GitCode 域名。中文优先 GitCode，英文优先 GitHub；失败切换另一镜像，两者均不可用后使用独立的 ARK Wiki 原图，仍无法加载才显示网格。下载有界，镜像通过 SHA-256 校验后才进入缓存；两镜像共享包含摘要的缓存身份，换语言和重启可复用缓存。浏览器无法使用原生校验缓存时不会直接加载未校验的镜像。解码尺寸也必须与清单一致。

多层地图、未知地图或缺少可靠坐标投影的地图继续使用网格。来源可用性受各地网络和托管平台限制；应用通过失败切换与本地缓存处理，不承诺永久可用。

固定底图在策略中显式设置 `cacheImmutable: true`，代表应用下载资源，必须同时限定 image 用途并固定 SHA-256。GitCode 的公开 raw API 和公开静态下载地址均返回 `no-store, must-revalidate, no-cache`；因此普通 HTTP 缓存不能满足下载后复用。这项明确授权的资源存储模式在校验成功后将原图保存在现有容量与条目数受限的 LRU 中，以摘要版本或用户清理作为失效依据，仍保留原始 HTTP 响应头。普通图片、Wiki、视频及未开启此模式的摘要图片继续遵守原有 HTTP 缓存规则，两种模式的存储身份也相互隔离。

远端资源验证使用匿名 GET：GitCode 这个 raw API 的 HEAD 可返回 404，不能据此判断 GET 资源不存在。

## 准备

`scripts/prepare_ark_map_artwork.py` 只准备本地资源包，不访问网络、不读取凭据、不上传、不提交，也不修改源码仓库或安装包。

先把清单中的原始 JPEG 放在一个目录中。图片应已完成真实解码和地图坐标对齐核验。输出目录必须尚不存在，其父目录必须存在，且不得位于源码仓库或原图目录内。

在仓库根目录运行：

```powershell
python -B scripts/prepare_ark_map_artwork.py `
  --images-dir 'C:\ReleaseStaging\ark-map-artwork\images' `
  --output-dir 'C:\ReleaseStaging\ark-map-artwork\publication-package'
```

再次准备时指定另一个新的输出目录；工具拒绝覆盖，即使既有目录为空。`--catalog` 可以显式选择另一份经过审核的清单，默认读取源码中的权威清单。

工具校验普通文件、JPEG 标记结构、SOF 实际尺寸与清单尺寸一致，以及每张不超过 16 MiB。它拒绝路径穿越、符号链接、Windows 目录联接/重解析点、重复文件名、缺图和不完整 JPEG；不对压缩像素做完整解码，因此不能用这一步代替图像查看和坐标核验。

输入校验完成后才创建输出目录。复制使用原始字节，并比对复制前后的 SHA-256；源文件在校验后改变会使打包失败。正常异常路径只清理本次创建的确切文件和空目录；强制终止或清理失败可能留下不完整目录，工具会拒绝复用。`manifest.json` 最后写入，命令只有全部写入成功才返回退出码 0 和成功摘要。

## 产物与拟定链接

```text
publication-package/
  README.md
  NOTICE.md
  manifest.json
  assets/ark-map-artwork/2026-10-25/<清单中的原始图片文件名>
```

`manifest.json` 记录清单文件 SHA-256、图片数量与字节总量，以及逐图的 SHA-256、字节数、尺寸、edition/mapName、坐标边界、原图 URL、坐标数据修订链接和许可来源文件页。

每项 `proposedURLs` 列出拟定的匿名下载地址：

- GitHub：`https://raw.githubusercontent.com/SZSLGJCOM/LanGame-Server-Manager/master/assets/ark-map-artwork/2026-10-25/<image>`
- GitCode：`https://api.gitcode.com/api/v5/repos/SZSLGJCOM/LanGame-Server-Manager-Releases/raw/assets/ark-map-artwork/2026-10-25/<image>?ref=main`

这些是准备工具生成的候选链接，其输出的 `published` 始终为 `false`，`remoteValidation` 始终为 `not-performed`，不能当作发布回执。上方“已发布资源”记录已完成的本批发布；远端清单使用 `mirrors` 字段保留仓库入口，客户端另行将 URL 固定到已回读验证的提交。GitCode 使用官方 [raw API](https://docs.gitcode.com/docs/apis/get-api-v-5-repos-owner-repo-raw-path/)；不把 Release 附件返回的临时 CDN 地址写入清单。

候选链接使用分支名，不天然不可变，启用时须改用实际发布提交。发布后不得替换同一版本目录内的字节；更换图片必须建立新版本目录并重新审核 hash、尺寸及坐标。图片缓存和下载校验绑定具体字节身份，不能仅凭文件名、HTTP 200 或 Content-Type 接受响应。

## 版权来源与发布边界

图片不由项目源码许可证覆盖。各文件页的游戏素材版权、fair-use 或公有领域标记可能不同；`NOTICE.md` 中性保留原图和文件页链接，不统一宣称某个作者或许可证，也不授予新的使用权。准备包不等于取得再分发许可。

发布须在明确授权范围内，并依据逐图文件页说明保留适用条件。本批已取得用户的公开发布授权。保留 README、NOTICE、manifest 和来源信息；不能把工具生成的候选状态改成“已验证发布”来替代真实上传和验证。

发布后，必须从两端逐图匿名下载并核对原始 SHA-256、大小、真实格式与尺寸，确认内容不是登录页、错误页或占位图，才能启用相应镜像。界面按语言选择首选来源，失败换备用来源，缓存仍不可用时回到网格；这部分由应用的受控媒体下载与缓存实现。

## 测试

```powershell
python -B -m unittest scripts.tests.test_prepare_ark_map_artwork -v
```

测试中的手写 JPEG 标记夹具只验证头部探测与打包行为，不是真实可解码图片。测试覆盖尺寸不符、缺损/超限、路径越界、禁止覆盖、Windows 目录联接、来源清单及失败清理；真实符号链接用例在宿主没有创建权限时会明确跳过。
