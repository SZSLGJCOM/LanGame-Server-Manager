param(
  [Parameter(Mandatory = $true)]
  [string] $FrontendDir,

  [Parameter(Mandatory = $true)]
  [string] $FrontendUrl,

  [Parameter(Mandatory = $false)]
  [int] $TimeoutSeconds = 5
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$frontendRoot = [IO.Path]::GetFullPath($FrontendDir).TrimEnd('\')
$uri = [Uri]$FrontendUrl
if ($uri.Scheme -ne "http" -or $uri.Host -notin @("127.0.0.1", "localhost")) {
  throw "Frontend development URL must use the local HTTP endpoint."
}

function Get-ListenerProcessIds {
  return @(
    Get-NetTCPConnection -State Listen -LocalPort $uri.Port -ErrorAction SilentlyContinue |
      Select-Object -ExpandProperty OwningProcess -Unique
  )
}

$listenerProcessIds = @(Get-ListenerProcessIds)
if ($listenerProcessIds.Count -eq 0) { exit 0 }
if ($listenerProcessIds.Count -ne 1) {
  [Console]::Error.WriteLine(
    "[LanGame] Multiple processes listen on frontend port $($uri.Port); refusing to stop any of them."
  )
  exit 2
}

$listenerProcessId = [int]$listenerProcessIds[0]
$listener = Get-CimInstance Win32_Process -Filter "ProcessId=$listenerProcessId"
$commandLine = [string]$listener.CommandLine
$normalizedCommand = $commandLine.Replace('/', '\')
$frontendFragment = ($frontendRoot + "\node_modules\").ToLowerInvariant()
$normalizedLower = $normalizedCommand.ToLowerInvariant()
$escapedPort = [regex]::Escape([string]$uri.Port)
$ownedViteListener = (
  $listener.Name -ieq "node.exe" -and
  $normalizedLower.Contains($frontendFragment) -and
  $normalizedCommand -match '(?i)vite\\+bin\\+vite\.js' -and
  $normalizedCommand -match '(?i)--host\s+"?127\.0\.0\.1"?(?:\s|$)' -and
  $normalizedCommand -match "(?i)--port\s+`"?$escapedPort`"?(?:\s|$)" -and
  $normalizedCommand -match '(?i)--strictPort(?:\s|$)'
)

if (-not $ownedViteListener) {
  [Console]::Error.WriteLine(
    "[LanGame] Port $($uri.Port) is owned by an unrelated process (PID $listenerProcessId); refusing to stop it."
  )
  exit 2
}

Write-Host "[LanGame] Restarting stale frontend dev server (PID $listenerProcessId)..."
Stop-Process -Id $listenerProcessId -Force
$deadline = (Get-Date).AddSeconds($TimeoutSeconds)
do {
  if (@(Get-ListenerProcessIds).Count -eq 0) { exit 0 }
  Start-Sleep -Milliseconds 100
} while ((Get-Date) -lt $deadline)

[Console]::Error.WriteLine(
  "[LanGame] Frontend port $($uri.Port) remained occupied after stopping PID $listenerProcessId."
)
exit 3
