[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$ServerRoot,
    [Parameter(Mandatory)][string]$ArchivePath,
    [Parameter(Mandatory)][string]$ExtractionDirectory
)

$ErrorActionPreference = 'Stop'
$serverDirectory = (Resolve-Path -LiteralPath $ServerRoot).Path
$archive = (Resolve-Path -LiteralPath $ArchivePath).Path
$extraction = [IO.Path]::GetFullPath($ExtractionDirectory)
$win64 = Join-Path $serverDirectory 'SCUM/Binaries/Win64'
$loader = Join-Path $win64 'ue4ss'
$proxy = Join-Path $win64 'dwmapi.dll'
$archiveHash = '4f9762f812329a640c8cfa14444c2bb97ecc213b8320bd4c5433383c3eef48f7'
$coreHash = '91d5444f41d19d0bb502f0fbdc15d76421e20359e898caa39e41e134c8f87958'

if (-not (Test-Path -LiteralPath (Join-Path $win64 'SCUMServer.exe') -PathType Leaf)) {
    throw 'The target is not a complete SCUM dedicated server installation.'
}
if ((Test-Path -LiteralPath $loader) -or (Test-Path -LiteralPath $proxy) -or
    (Test-Path -LiteralPath (Join-Path $win64 'version.dll'))) {
    throw 'An existing loader is present. Use a separate installation; existing mods must be preserved.'
}
if (Test-Path -LiteralPath $extraction) {
    throw 'The extraction directory must be new and outside the server installation and repository.'
}
$repository = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../../..')).Path
foreach ($boundary in @($serverDirectory, $repository)) {
    if ($extraction.Equals($boundary, [StringComparison]::OrdinalIgnoreCase) -or
        $extraction.StartsWith($boundary.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) {
        throw 'The extraction directory must be outside the server installation and repository.'
    }
}
$processes = Get-CimInstance Win32_Process
if ($processes | Where-Object { $_.Name -eq 'SCUMServer.exe' -and -not $_.ExecutablePath }) {
    throw 'A SCUM process cannot be inspected. Use an elevated PowerShell session to verify that the target installation is stopped.'
}
$running = $processes | Where-Object {
    $_.ExecutablePath -and $_.ExecutablePath.StartsWith(
        $serverDirectory.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)
}
if ($running) { throw 'Stop every instance using this installation before installing the loader.' }
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $archiveHash) {
    throw 'The archive is not the verified official UE4SS 3.0.1-1125-g527a483b asset.'
}

Expand-Archive -LiteralPath $archive -DestinationPath $extraction
$core = Join-Path $extraction 'ue4ss/UE4SS.dll'
if ((Get-FileHash -LiteralPath $core -Algorithm SHA256).Hash -ne $coreHash) {
    throw 'The extracted UE4SS core does not match the verified asset.'
}
$scripts = Join-Path $loader 'Mods/LgsmPlayerQuery/Scripts'
New-Item -ItemType Directory -Path $scripts -Force | Out-Null
Copy-Item -LiteralPath $core,(Join-Path $extraction 'ue4ss/LICENSE') -Destination $loader
Copy-Item -LiteralPath (Join-Path $extraction 'ue4ss/UE4SS_SDK_Backends') -Destination $loader -Recurse
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'UE4SS-settings.ini') -Destination $loader
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'LgsmPlayerQuery/Scripts/main.lua') -Destination $scripts
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'LgsmPlayerQuery/enabled.txt') -Destination (Split-Path $scripts)
New-Item -ItemType Directory -Path (Join-Path $serverDirectory 'langame_player_query') -Force | Out-Null
# Install the entry DLL last so the game never sees an incomplete loader during preparation.
Copy-Item -LiteralPath (Join-Path $extraction 'dwmapi.dll') -Destination $proxy
Write-Output 'Installed the verified SCUM read-only player-query loader. Start the instance through LanGame.'
