# Dragonwilds 玩家读取扩展安装

LanGame 使用独立的 `LgsmPlayerQuery` Lua 扩展读取专服当前连接名单。加载器由官方 UE4SS 核心和本目录脚本构建的 `version.dll` 代理组成；首次安装加载器和专用配置后，LanGame 在启动准备阶段部署 Lua 脚本、启用标记和请求目录。

已验证组合为 Dragonwilds build `24574222`、Unreal Engine `5.6.1`、UE4SS `3.0.1-1125-g527a483b`。隔离专服通过 LanGame 正式启动路径运行，Rust 读取器连续获取两个不同请求对应的完整空名单，并在停服后拒绝旧进程；Lua 构造测试覆盖空服、多人数据和异常响应。真实多人加入、退出和重连尚未验收。证据和许可范围见 [SOURCE.md](SOURCE.md)。

名单中的数字标识仅区分本次运行的玩家会话，不是平台账号。玩家行保持只读，不提供踢人或封禁操作。扩展调用游戏自身的 `IsPlayerReady` 判断游戏就绪状态；这个检查不代表已经独立验证 EOS 账号认证。

## 构建专服代理

代理必须导出 Windows `version.dll` 的 API，不能把 `dwmapi.dll` 简单改名。本目录只构建官方代理生成器及必要依赖，不构建 UE4SS 核心，不使用 Nexus 的二进制或 RSDWTools 代码。

构建需要 64 位 Windows、64 位 PowerShell、CMake，以及带 C++ 工具和 Windows SDK 的 Visual Studio。已验证工具链为 CMake `4.2.1`、Visual Studio 2026、MSVC `19.51.36252.0` 和 Windows SDK `10.0.28000.0`。使用其他工具链时，生成的 DLL 哈希可能不同。

1. 从仓库根目录打开 PowerShell，准备仓库外的源码缓存目录和构建输出目录。
2. 首次运行时使用 `-DownloadSources`。脚本只从两个固定上游版本获取 [proxy-sources.json](proxy-sources.json) 列出的 32 个源码文件，并逐一验证 SHA-256 和 Git blob。已有文件哈希不符时停止并保留文件。

```powershell
$sourceDirectory = Read-Host '代理源码缓存目录完整路径（仓库外）'
$outputDirectory = Read-Host '代理构建输出目录完整路径（与源码目录分开）'
.\modules\runescapedragonwilds\extensions\build-proxy.ps1 `
    -SourceDirectory $sourceDirectory `
    -OutputDirectory $outputDirectory `
    -DownloadSources
```

源码准备完成后，同一命令去掉 `-DownloadSources` 即可离线构建。脚本仍会核对全部源码哈希，以及本机 `System32/version.dll` 的 Windows 签名。构建目录不能复用其他 CMake 项目。

成功后，输出目录包含：

- `bin/Release/version.dll`：安装到专服的代理。
- `bin/Release/LICENSE-UE4SS.txt` 和 `LICENSE-fmt.txt`：随代理保留的许可证。
- `proxy-build-evidence.json`：本次构建使用的源码清单哈希、系统 DLL 哈希、输出 DLL 哈希和退出码。

`proxy_generator.exe`、对象文件和导入库仅用于构建，无须安装到服务器。代理使用静态 C++ 运行库，只依赖 Windows 的 `KERNEL32.dll`、`USER32.dll` 和 `SHELL32.dll`。

## 首次安装

以下步骤用于**没有已有加载器的独立安装目录**。已有模组环境不要直接覆盖配置；使用下方「已有加载器」说明。

1. 在 LanGame 中停止目标实例，并确认所有使用同一安装目录的实例均已停止。
2. 准备官方资产 **`UE4SS_v3.0.1-1125-g527a483b.zip`**。它的 SHA-256 为 `4f9762f812329a640c8cfa14444c2bb97ecc213b8320bd4c5433383c3eef48f7`。官方 [`experimental-latest` 发布页](https://github.com/UE4SS-RE/RE-UE4SS/releases/tag/experimental-latest)会移动。使用通过上述哈希核对的 `1125` 资产，不自动替换成新版本，也不使用 `zDEV` 包。
3. 从仓库根目录运行以下 PowerShell 命令。目标安装目录必须包含 `RSDragonwildsServer.exe` 和实际专服程序。

```powershell
$ErrorActionPreference = 'Stop'
$extensionSource = (Resolve-Path -LiteralPath '.\modules\runescapedragonwilds\extensions').Path
$serverRoot = (Resolve-Path -LiteralPath (Read-Host 'Dragonwilds 专服安装目录完整路径')).Path
$zipPath = (Resolve-Path -LiteralPath (Read-Host '已保存的 UE4SS 1125 ZIP 完整路径')).Path
$proxyOutput = (Resolve-Path -LiteralPath (Read-Host '代理构建输出目录完整路径')).Path
$builtProxy = Join-Path $proxyOutput 'bin\Release\version.dll'
$buildEvidence = Get-Content -LiteralPath (Join-Path $proxyOutput 'proxy-build-evidence.json') -Raw | ConvertFrom-Json
$win64 = Join-Path $serverRoot 'RSDragonwilds\Binaries\Win64'
$loader = Join-Path $win64 'ue4ss'
$proxy = Join-Path $win64 'version.dll'

if (-not (Test-Path -LiteralPath (Join-Path $serverRoot 'RSDragonwildsServer.exe') -PathType Leaf) -or
    -not (Test-Path -LiteralPath (Join-Path $win64 'RSDragonwildsServer-Win64-Shipping.exe') -PathType Leaf)) {
    throw '目标不是完整的 Dragonwilds 专用服务器安装目录。'
}
if ((Test-Path -LiteralPath $proxy) -or (Test-Path -LiteralPath $loader) -or
    (Test-Path -LiteralPath (Join-Path $win64 'dwmapi.dll'))) {
    throw '目标已有加载器文件，安装已停止。'
}
$running = Get-CimInstance Win32_Process | Where-Object {
    $_.ExecutablePath -and $_.ExecutablePath.StartsWith(
        $serverRoot.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)
}
if ($running) { throw '目标目录仍有运行中的进程，请先停止对应实例。' }
if ((Get-FileHash -LiteralPath $zipPath -Algorithm SHA256).Hash -ne
    '4f9762f812329a640c8cfa14444c2bb97ecc213b8320bd4c5433383c3eef48f7') {
    throw 'UE4SS ZIP 与固定官方资产的哈希不符。'
}
if ($buildEvidence.ue4ss_commit -ne '527a483b63b4dd0104fe1ca1a3934a06b87fcfb2' -or
    $buildEvidence.build_exit_code -ne 0 -or
    (Get-FileHash -LiteralPath $builtProxy -Algorithm SHA256).Hash -ne $buildEvidence.proxy_sha256) {
    throw '代理文件与本目录构建脚本的成功记录不符。'
}

$extractRoot = Join-Path ([IO.Path]::GetTempPath()) ('langame-dragonwilds-loader-' + [Guid]::NewGuid().ToString('N'))
Expand-Archive -LiteralPath $zipPath -DestinationPath $extractRoot
New-Item -ItemType Directory -Path $loader | Out-Null
Copy-Item -LiteralPath $builtProxy -Destination $proxy
Copy-Item -LiteralPath (Join-Path $proxyOutput 'bin\Release\LICENSE-UE4SS.txt'),(Join-Path $proxyOutput 'bin\Release\LICENSE-fmt.txt') -Destination $loader
Copy-Item -LiteralPath (Join-Path $extractRoot 'ue4ss\UE4SS.dll'),(Join-Path $extractRoot 'ue4ss\LICENSE') -Destination $loader
Copy-Item -LiteralPath (Join-Path $extractRoot 'ue4ss\UE4SS_SDK_Backends') -Destination $loader -Recurse
Copy-Item -LiteralPath (Join-Path $extensionSource 'UE4SS-settings.ini') -Destination $loader
Get-FileHash -LiteralPath $proxy,(Join-Path $loader 'UE4SS.dll') -Algorithm SHA256
Write-Output ('解压目录：' + $extractRoot)
```

命令不复制默认 `Mods`、`mods.txt`、调试工具或原始 `dwmapi.dll`。官方核心 `ue4ss/UE4SS.dll` 的 SHA-256 应为 `91d5444f41d19d0bb502f0fbdc15d76421e20359e898caa39e41e134c8f87958`；代理哈希应与本次 `proxy-build-evidence.json` 一致。

4. 在 LanGame 中重新启动实例。启动准备阶段会生成 `ue4ss/Mods/LgsmPlayerQuery/enabled.txt`、`Scripts/main.lua` 和安装根目录下的 `langame_player_query/`。
5. 等待世界加载完成，再刷新玩家选项卡。空服应显示空名单；未就绪、扩展未加载或采集不完整时应显示错误，最后一次响应不能代替当前名单。

## 已有加载器与故障排查

LanGame 只在已有 UE4SS 加载器与本目录专用配置匹配时准备名单脚本，不下载二进制、不覆盖已有配置或 `mods.txt`。其他模组可能使用不同的调度和生命周期设置，不能仅打开 `HookEngineTick` 后就认定兼容。已有加载器与已验证组合不一致时，使用独立安装目录和独立世界的实例。

首先查看 `RSDragonwilds/Binaries/Win64/ue4ss/UE4SS.log`。确认核心为 `527a483b`，日志包含 `Mod 'LgsmPlayerQuery' has enabled.txt, starting mod.`，且没有 Lua 加载错误。没有 UE4SS 日志时，检查 `version.dll` 是否与实际专服程序同目录，以及 `ue4ss/UE4SS.dll` 是否完整；专服不依靠默认 `dwmapi.dll` 启动加载器。

Lua 未自动准备时，检查是否通过 LanGame 重新启动、专用配置是否完整匹配。修改配置或替换 DLL 前停止实例。游戏或加载器升级后，已有验证结果不能证明新组合兼容；不要改用加入、退出日志拼接名单。
