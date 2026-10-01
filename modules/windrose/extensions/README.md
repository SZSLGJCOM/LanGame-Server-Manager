# Windrose 玩家读取扩展安装

LanGame 通过独立的 `LgsmPlayerQuery` 扩展读取当前连接名单。首次安装只需要官方 UE4SS 加载器和本目录的专用配置；LanGame 会在下次启动实例前自动准备 Lua 脚本、启用标记和请求目录。

已验证组合为 Windrose build `24913903`（`0.10.0.9.32`）与 UE4SS `3.0.1-1125-g527a483b`。普通权限可运行，无须安装 Windrose+、Dashboard 或倍率 PAK。实服验收目前覆盖空服、重启和停服；真实多人加入、退出和重连尚未验收。完整证据见 [SOURCE.md](SOURCE.md)。

## 首次安装

1. 在 LanGame 中停止目标实例。确认所有使用同一 Windrose 安装目录的实例均已停止。
2. 从 [UE4SS 官方发布页](https://github.com/UE4SS-RE/RE-UE4SS/releases/tag/experimental)下载 **`UE4SS_v3.0.1-1125-g527a483b.zip`**，或使用[该版本的下载链接](https://github.com/UE4SS-RE/RE-UE4SS/releases/download/experimental/UE4SS_v3.0.1-1125-g527a483b.zip)。不要使用名称以 `zDEV` 开头的包。安装前按下方命令校验 SHA-256；如果上游不再提供此资产，不要直接用其他版本替代。
3. 从 LanGame Server Manager 仓库根目录打开 PowerShell，运行以下命令。根据提示填写目标安装目录和已下载 ZIP 的完整路径；安装目录必须包含 `WindroseServer.exe`。

以下命令仅用于**没有加载器的独立安装目录**。发现已有 `dwmapi.dll` 或 `ue4ss` 目录时会停止，不覆盖现有加载器或配置。

```powershell
$ErrorActionPreference = 'Stop'
$extensionSource = (Resolve-Path -LiteralPath '.\modules\windrose\extensions').Path
$serverRoot = (Resolve-Path -LiteralPath (Read-Host 'Windrose 安装目录完整路径')).Path
$zipPath = (Resolve-Path -LiteralPath (Read-Host 'UE4SS ZIP 完整路径')).Path
$win64 = Join-Path $serverRoot 'R5\Binaries\Win64'
$loader = Join-Path $win64 'ue4ss'
$proxy = Join-Path $win64 'dwmapi.dll'

if (-not (Test-Path -LiteralPath (Join-Path $serverRoot 'WindroseServer.exe') -PathType Leaf) -or
    -not (Test-Path -LiteralPath (Join-Path $win64 'WindroseServer-Win64-Shipping.exe') -PathType Leaf)) {
    throw '目标不是完整的 Windrose 专用服务器安装目录。'
}
if ((Test-Path -LiteralPath $proxy) -or (Test-Path -LiteralPath $loader)) {
    throw '目标已存在加载器文件。请按下方“已有加载器”说明处理。'
}
$running = Get-CimInstance Win32_Process | Where-Object {
    $_.ExecutablePath -and $_.ExecutablePath.StartsWith(
        $serverRoot.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)
}
if ($running) {
    throw '目标安装目录仍有运行中的进程。请先在 LanGame 中停止对应实例。'
}
$expectedZipHash = '4f9762f812329a640c8cfa14444c2bb97ecc213b8320bd4c5433383c3eef48f7'
if ((Get-FileHash -LiteralPath $zipPath -Algorithm SHA256).Hash -ne $expectedZipHash) {
    throw 'ZIP 的 SHA-256 与已验证的官方资产不符，安装已停止。'
}

$extractRoot = Join-Path ([IO.Path]::GetTempPath()) ('langame-windrose-loader-' + [Guid]::NewGuid().ToString('N'))
Expand-Archive -LiteralPath $zipPath -DestinationPath $extractRoot
New-Item -ItemType Directory -Path $loader | Out-Null
Copy-Item -LiteralPath (Join-Path $extractRoot 'dwmapi.dll') -Destination $proxy
Copy-Item -LiteralPath (Join-Path $extractRoot 'ue4ss\UE4SS.dll') -Destination $loader
Copy-Item -LiteralPath (Join-Path $extractRoot 'ue4ss\LICENSE') -Destination $loader
Copy-Item -LiteralPath (Join-Path $extractRoot 'ue4ss\UE4SS_SDK_Backends') -Destination $loader -Recurse
Copy-Item -LiteralPath (Join-Path $extensionSource 'UE4SS-settings.ini') -Destination $loader

Get-FileHash -LiteralPath $proxy,(Join-Path $loader 'UE4SS.dll') -Algorithm SHA256
Write-Output ('解压目录：' + $extractRoot)
```

命令只复制加载器 DLL、许可证、SDK 后端描述和专用配置，不复制官方 ZIP 中的默认 `Mods`、`mods.txt` 或默认配置。解压目录保留，便于核对文件。

4. 核对命令最后输出的两个 DLL 哈希：

| 文件 | SHA-256 |
| --- | --- |
| `dwmapi.dll` | `f2f9e57f7707c8456b117391b1ab45fd7149332f89a3a94726eda0cc2300e834` |
| `ue4ss/UE4SS.dll` | `91d5444f41d19d0bb502f0fbdc15d76421e20359e898caa39e41e134c8f87958` |

5. 在 LanGame 中重新启动实例。启动准备阶段会自动创建下列标有“自动准备”的文件和目录。
6. 等待世界加载完成，进入玩家选项卡并刷新。没有玩家时应显示空名单；扩展未加载、世界尚未就绪或采集不完整时应显示相应错误，不应把最后一次响应当作当前名单。

```text
<Windrose 安装目录>/
├─ WindroseServer.exe
├─ R5/
│  └─ Binaries/Win64/
│     ├─ WindroseServer-Win64-Shipping.exe
│     ├─ dwmapi.dll                         ← 官方 ZIP
│     └─ ue4ss/
│        ├─ UE4SS.dll                       ← 官方 ZIP
│        ├─ LICENSE                         ← 官方 ZIP，保留
│        ├─ UE4SS_SDK_Backends/
│        │  └─ UE4SS.json                   ← 官方 ZIP
│        ├─ UE4SS-settings.ini              ← 本目录专用配置
│        └─ Mods/LgsmPlayerQuery/
│           ├─ enabled.txt                 ← LanGame 自动准备
│           └─ Scripts/main.lua            ← LanGame 自动准备
└─ langame_player_query/                   ← LanGame 自动准备
   ├─ request.json                         ← 刷新时生成
   └─ response.json                        ← 扩展生成
```

## 已有加载器

不要把本目录的 `UE4SS-settings.ini` 直接覆盖到已有模组环境。Windrose+ 等扩展可能采用不同的调度或生命周期 hook 设置，不能仅打开 `HookEngineTick` 后假定兼容。

LanGame 仅在发现 `UE4SS.dll`，且配置的全部节、键和值与专用配置一致时自动准备读取脚本；注释、空白和行序可不同。缺失配置、重复键、额外选项或值不一致都会使自动准备停止。LanGame 不自动下载或替换加载器，不修改已有配置或 `mods.txt`。

已有加载器若正是上述已验证版本和专用配置，停止实例后重新启动即可。否则使用独立安装目录和独立世界的实例，并确认实例启动计划指向新目录，再执行首次安装步骤。不要让两个运行实例共用同一个世界数据库。

## 无法读取时

先查看 `R5/Binaries/Win64/ue4ss/UE4SS.log`，确认包含 `Starting Lua mod 'LgsmPlayerQuery'`，且没有脚本加载错误。然后核对加载器版本、DLL 哈希及专用配置是否完整。

如果未自动生成 `Mods/LgsmPlayerQuery`，检查是否通过 LanGame 重新启动，以及已有加载器配置是否符合上述条件。只刷新玩家选项卡不会安装扩展。

如果日志确认扩展已加载，但刷新仍超时，保留本次响应和日志用于诊断；不要启用异步 UObject 读取或改用 Windrose+ 的缓存状态文件。修改加载器配置前停止实例。游戏或加载器更新后，原有验证结果不能证明新组合兼容。
