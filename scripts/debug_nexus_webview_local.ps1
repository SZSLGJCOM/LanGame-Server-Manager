[CmdletBinding()]
param([ValidateRange(1024, 65535)][int]$DiagnosticPort = 43179)

# One development session only. Reuse the normal launcher and browser profile.
# This does not change registry, firewall, persistent environment, or app settings.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$launcher = Join-Path $PSScriptRoot 'start_langame_desktop_local.ps1'
if (-not (Test-Path -LiteralPath $launcher -PathType Leaf)) {
    throw "Desktop launcher is missing: $launcher"
}
$profilePath = [IO.Path]::GetFullPath(
    (Join-Path $env:LOCALAPPDATA 'cn.langame.servermanager\EBWebView'))
$activeProfile = @(Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" |
    Where-Object {
        $_.CommandLine -and $_.CommandLine.Replace('/', '\').IndexOf(
            $profilePath, [StringComparison]::OrdinalIgnoreCase) -ge 0
    })
if ($activeProfile.Count -gt 0) {
    throw 'Exit LanGame, including its tray process, then run this script again. This diagnostic script does not terminate processes.'
}

$previousArguments = [Environment]::GetEnvironmentVariable(
    'WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS', 'Process')
if ($previousArguments -match '--remote-debugging-(?:port|pipe|address)') {
    throw 'This shell already has WebView debugging arguments. Use a normal fresh PowerShell window.'
}
# Port 0 changes navigator.webdriver in Chromium. Keep inspection equivalent
# to the normal app, without overriding page properties or browser identity.
$listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, $DiagnosticPort)
try { $listener.Start() }
finally { $listener.Stop() }
$debugArguments = "--remote-debugging-port=$DiagnosticPort --remote-debugging-address=127.0.0.1"
if (-not [string]::IsNullOrWhiteSpace($previousArguments)) {
    $debugArguments = $previousArguments + ' ' + $debugArguments
}

try {
    [Environment]::SetEnvironmentVariable(
        'WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS', $debugArguments, 'Process')
    Write-Host '[LanGame] Starting one development session with local-only WebView diagnostics.'
    Write-Host '[LanGame] Open Nexus Account in the app. Exit the app and use the normal launcher to end diagnostics.'
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $launcher
    if ($LASTEXITCODE -ne 0) {
        throw "Desktop launcher failed with exit code $LASTEXITCODE."
    }
}
finally {
    [Environment]::SetEnvironmentVariable(
        'WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS', $previousArguments, 'Process')
}
