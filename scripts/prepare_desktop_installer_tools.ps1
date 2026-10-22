[CmdletBinding()]
param([Parameter(Mandatory = $true)][string]$OutputRoot)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT' -or [string]::IsNullOrWhiteSpace($env:RUNNER_TEMP)) {
    throw 'Installer tools require a Windows runner with RUNNER_TEMP.'
}
$package = Get-Content -LiteralPath (Join-Path $PSScriptRoot '../apps/desktop/package.json') -Raw | ConvertFrom-Json
if ($package.devDependencies.'@tauri-apps/cli' -cne '2.12.3') {
    throw 'Review the installer tool pins when changing the Tauri CLI version.'
}
$runnerTemp = [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\')
$output = [IO.Path]::GetFullPath($OutputRoot)
if (-not $output.StartsWith($runnerTemp + '\', [StringComparison]::OrdinalIgnoreCase) -or
    (Test-Path -LiteralPath $output)) {
    throw 'Installer tools need a new directory beneath RUNNER_TEMP.'
}

# Tool versions and SHA1 pins: tauri-cli-v2.12.3/crates/tauri-bundler/src/bundle/windows/nsis/mod.rs
# https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.3/crates/tauri-bundler/src/bundle/windows/nsis/mod.rs
# SHA256 digests and sizes are also pinned from the corresponding official GitHub release assets.
$downloads = @(
    @{
        name = 'nsis-3.13.zip'
        url = 'https://github.com/tauri-apps/binary-releases/releases/download/nsis-3.13/nsis-3.13.zip'
        bytes = 2363925
        sha1 = 'db14f8af2ea346b786c6ae19343f61aaf5e09b8e'
        sha256 = 'ba63dffc4410ee89193e1cb5a41989991bd77c61068da17e3156d136b7b0b3d8'
    },
    @{
        name = 'nsis_tauri_utils.dll'
        url = 'https://github.com/tauri-apps/nsis-tauri-utils/releases/download/nsis_tauri_utils-v0.5.3/nsis_tauri_utils.dll'
        bytes = 34304
        sha1 = '75197fee3c6a814fe035788d1c34ead39349b860'
        sha256 = '5ba143b5db4a87d32d6e7802e033330aae56cbceabe0d1e3ba41948385ad4709'
    }
)
New-Item -ItemType Directory -Path $output | Out-Null
foreach ($download in $downloads) {
    $path = Join-Path $output $download.name
    Invoke-WebRequest -Uri $download.url -OutFile $path -MaximumRedirection 3 -TimeoutSec 120
    if ((Get-Item -LiteralPath $path).Length -ne $download.bytes -or
        (Get-FileHash -LiteralPath $path -Algorithm SHA1).Hash.ToLowerInvariant() -cne $download.sha1 -or
        (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -cne $download.sha256) {
        throw "Installer tool download does not match the pinned asset: $($download.name)"
    }
}
Expand-Archive -LiteralPath (Join-Path $output 'nsis-3.13.zip') -DestinationPath $output
$nsisRoot = Join-Path $output 'nsis-3.13'
$plugins = Join-Path $nsisRoot 'Plugins/x86-unicode/additional'
New-Item -ItemType Directory -Path $plugins -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $output 'nsis_tauri_utils.dll') -Destination $plugins
if (-not (Test-Path -LiteralPath (Join-Path $nsisRoot 'makensis.exe') -PathType Leaf)) {
    throw 'The verified archive did not contain makensis.exe.'
}
Write-Output $nsisRoot
