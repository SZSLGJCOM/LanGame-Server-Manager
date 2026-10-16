param(
    [Parameter(Mandatory = $true)][ValidateSet('Prepare', 'Query', 'VerifyReload')][string]$Mode,
    [Parameter(Mandatory = $true)][string]$FixtureRoot,
    [string]$SessionManifest,
    [int]$ExpectedPid
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Assert-PlainPath([string]$Path) {
    if ($Path -notmatch '^[A-Za-z]:[\\/]' -or $Path.IndexOf(':', 2) -ge 0) { throw 'Absolute local fixture path required.' }
    for ($current = [IO.FileInfo]::new($Path); $null -ne $current; $current = $current.Directory) {
        if (Test-Path -LiteralPath $current.FullName) {
            if (([IO.File]::GetAttributes($current.FullName) -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw 'Smoke-test paths cannot contain reparse points.'
            }
        }
        if ($current -is [IO.DirectoryInfo]) { break }
    }
    for ($directory = [IO.DirectoryInfo]::new([IO.Path]::GetDirectoryName($Path)); $null -ne $directory; $directory = $directory.Parent) {
        if (([IO.File]::GetAttributes($directory.FullName) -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw 'Smoke-test ancestors cannot contain reparse points.'
        }
    }
}
function Read-BoundedUtf8([string]$Path, [int]$Maximum) {
    Assert-PlainPath $Path
    $stream = [IO.FileStream]::new($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete)
    try {
        if ($stream.Length -gt $Maximum) { throw 'Smoke-test input exceeds its size limit.' }
        $bytes = [byte[]]::new($Maximum + 1)
        $length = 0
        while ($length -lt $bytes.Length) {
            $count = $stream.Read($bytes, $length, $bytes.Length - $length)
            if ($count -eq 0) { break }
            $length += $count
        }
        if ($length -gt $Maximum) { throw 'Smoke-test input grew beyond its size limit.' }
        return [Text.UTF8Encoding]::new($false, $true).GetString($bytes, 0, $length)
    }
    finally { $stream.Dispose() }
}
if ($FixtureRoot -notmatch '^[A-Za-z]:[\\/]') { throw 'Absolute private fixture root required.' }
$fixture = [IO.Path]::GetFullPath($FixtureRoot)
if ($fixture -notmatch '(?i)[\\/]work[\\/]' -or -not [IO.Directory]::Exists($fixture)) {
    throw 'Use the existing private work fixture; this helper never copies or launches a server.'
}
Assert-PlainPath $fixture
$managed = Join-Path $fixture 'valheim_server_Data/Managed'
$expected = @{
    'assembly_valheim.dll' = '0dfc7e81436f822121148efed859ba1e661d58b9484bd45e3b6f33b6dd86bfad'
    'assembly_utils.dll' = '63e114e7cfdcc38e4f4391c18b91f774290274d2b9200e6d85d757cbe3673e18'
    'Splatform.dll' = '8953c24a2f2af7ae257cc64fb36dde9d102c871c29b4cdd97d0c3c001de5bd5a'
}
foreach ($name in $expected.Keys) {
    $path = Join-Path $managed $name
    Assert-PlainPath $path
    if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expected[$name]) {
        throw 'Smoke fixture is not the audited local build 25730807.'
    }
}
if ($Mode -eq 'Prepare') {
    $sessionRoot = Join-Path $fixture ('lgsm-control-smoke-' + [Guid]::NewGuid().ToString('N'))
    $ipc = Join-Path $sessionRoot 'ipc'
    $savedir = Join-Path $sessionRoot 'saves'
    [IO.Directory]::CreateDirectory($ipc) | Out-Null
    [IO.Directory]::CreateDirectory($savedir) | Out-Null
    $encoding = [Text.UTF8Encoding]::new($false)
    $tokenFile = Join-Path $ipc 'session.json'
    [IO.File]::WriteAllText($tokenFile, (@{protocol = 1; token = [Guid]::NewGuid().ToString('N')} | ConvertTo-Json -Compress), $encoding)
    $manifestPath = Join-Path $sessionRoot 'manifest.json'
    $manifest = [ordered]@{
        protocol = 1; fixture = $fixture; control_root = $ipc; savedir = $savedir
        executable = (Join-Path $fixture 'valheim_server.exe'); created_utc = [DateTime]::UtcNow.ToString('o')
    }
    [IO.File]::WriteAllText($manifestPath, ($manifest | ConvertTo-Json), $encoding)
    # The token stays only in session.json. The caller launches the reviewed
    # loader/plugin separately with ROOT env and this isolated savedir.
    Write-Output $manifestPath
    return
}
if ($ExpectedPid -le 0 -or [string]::IsNullOrEmpty($SessionManifest)) { throw 'A prepared manifest and actual server PID are required.' }
$manifestPath = [IO.Path]::GetFullPath($SessionManifest)
if (-not $manifestPath.StartsWith($fixture + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Session manifest must belong to the selected work fixture.'
}
$manifest = Read-BoundedUtf8 $manifestPath 4096 | ConvertFrom-Json
$ipc = [IO.Path]::GetFullPath($manifest.control_root)
$savedir = [IO.Path]::GetFullPath($manifest.savedir)
$sessionRoot = [IO.Path]::GetDirectoryName($manifestPath)
foreach ($path in @($ipc, $savedir)) {
    if (-not $path.StartsWith($sessionRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Manifest paths escaped the prepared isolated session.'
    }
    Assert-PlainPath $path
}
$process = Get-Process -Id $ExpectedPid
if (-not $process.Path.Equals((Join-Path $fixture 'valheim_server.exe'), [StringComparison]::OrdinalIgnoreCase) -or
    $process.StartTime.ToUniversalTime() -lt [DateTime]::Parse($manifest.created_utc).ToUniversalTime()) {
    throw 'PID does not identify the prepared fixture run.'
}
$session = Read-BoundedUtf8 (Join-Path $ipc 'session.json') 4096 | ConvertFrom-Json
if ($session.protocol -ne 1 -or $session.token -cnotmatch '^[0-9a-f]{32}$') { throw 'Invalid session bootstrap.' }
$op = if ($Mode -eq 'Query') { 'query' } else { 'reload' }
$nonce = [Guid]::NewGuid().ToString('N')
$request = @{protocol = 1; nonce = $nonce; token = $session.token; op = $op} | ConvertTo-Json -Compress
$temporary = Join-Path $ipc ('request_' + $nonce + '.tmp')
$destination = Join-Path $ipc 'request.json'
Assert-PlainPath $destination
try {
    [IO.File]::WriteAllText($temporary, $request, [Text.UTF8Encoding]::new($false))
    if ([IO.File]::Exists($destination)) { [IO.File]::Replace($temporary, $destination, [NullString]::Value) }
    else { [IO.File]::Move($temporary, $destination) }
}
finally { if ([IO.File]::Exists($temporary)) { [IO.File]::Delete($temporary) } }
$deadline = [Diagnostics.Stopwatch]::StartNew()
$response = $null
while ($deadline.Elapsed.TotalSeconds -lt 10) {
    $responsePath = Join-Path $ipc 'response.json'
    if ([IO.File]::Exists($responsePath)) {
        $candidate = Read-BoundedUtf8 $responsePath 65536 | ConvertFrom-Json
        if ($candidate.nonce -ceq $nonce) { $response = $candidate; break }
    }
    Start-Sleep -Milliseconds 100
}
if ($null -eq $response) { throw 'Controller response deadline exceeded.' }
if ($response.protocol -ne 1 -or $response.pid -ne $ExpectedPid -or $response.op -cne $op) { throw 'Response identity mismatch.' }
if ($response.status -cne 'ok') { throw ('Native controller returned error: ' + $response.error) }
if ($Mode -eq 'VerifyReload') {
    foreach ($entry in @(@('admin', 'adminlist.txt'), @('permitted', 'permittedlist.txt'))) {
        $text = Read-BoundedUtf8 (Join-Path $savedir $entry[1]) 524288
        if ($text.Length -gt 0 -and $text[0] -eq [char]0xfeff) { $text = $text.Substring(1) }
        $lines = @($text -split '\r?\n' |
            Where-Object { $_.Length -gt 0 -and -not $_.StartsWith('//', [StringComparison]::Ordinal) })
        $actual = @($response.access.($entry[0]))
        if ($lines.Count -ne $actual.Count) { throw 'Native reload list count differs from its persisted file.' }
        for ($index = 0; $index -lt $lines.Count; $index++) {
            if ($lines[$index] -cne $actual[$index]) { throw 'Native reload list differs from its persisted file.' }
        }
    }
}
[pscustomobject]@{status = $response.status; op = $op; pid = $ExpectedPid; players = @($response.players).Count; response_file = $responsePath}
