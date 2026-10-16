param(
    [Parameter(Mandatory = $true)][string]$OutDir
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
# Cargo may inherit pwsh's PSModulePath while invoking Windows PowerShell 5.
# Load this host's built-in commands explicitly, including those used by the
# shared proxy builder, without changing user or machine environment settings.
foreach ($moduleName in @('Microsoft.PowerShell.Management', 'Microsoft.PowerShell.Utility', 'Microsoft.PowerShell.Security')) {
    $moduleManifest = [IO.Path]::Combine($PSHOME, 'Modules', $moduleName, $moduleName + '.psd1')
    Import-Module -Name $moduleManifest -ErrorAction Stop
}
if ($OutDir -notmatch '^(?:[A-Za-z]:[\\/]|[\\/]{2}[^\\/\r\n]+[\\/][^\\/\r\n]+(?:[\\/]|$))') {
    throw 'Dragonwilds player query requires an absolute drive-root or UNC OutDir.'
}
$output = [IO.Path]::GetFullPath($OutDir)
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if ($output.Equals($projectRoot, [StringComparison]::OrdinalIgnoreCase) -or
    $output.StartsWith($projectRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Dragonwilds player query build output must be outside the repository.'
}
foreach ($path in @($output, $projectRoot)) {
    if ($path.IndexOfAny([char[]]'%!&|<>^"') -ge 0 -or $path.Contains("`n") -or $path.Contains("`r")) {
        throw 'MSVC build paths contain unsupported command characters.'
    }
}
$extensions = Join-Path $projectRoot 'modules/runescapedragonwilds/extensions'
$manifest = Get-Content -LiteralPath (Join-Path $extensions 'proxy-sources.json') -Raw | ConvertFrom-Json
if ($manifest.schema -ne 1 -or $manifest.ue4ss_commit -ne '527a483b63b4dd0104fe1ca1a3934a06b87fcfb2' -or
    $manifest.fmt_version -ne '11.2.0' -or $manifest.files.Count -ne 32) {
    throw 'Dragonwilds proxy source manifest is not the verified fixed revision.'
}
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
if (-not (Test-Path -LiteralPath $vswhere -PathType Leaf)) { throw 'MSVC C++ Build Tools are required.' }
$installationVersion = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationVersion
if ($LASTEXITCODE -ne 0 -or -not $installationVersion) { throw 'MSVC x64 toolchain was not found.' }
$generator = switch (([Version]$installationVersion).Major) {
    18 { 'Visual Studio 18 2026' }
    17 { 'Visual Studio 17 2022' }
    default { throw 'Dragonwilds player query requires Visual Studio 2022 or 2026.' }
}
if (-not (Get-Command cmake -ErrorAction SilentlyContinue)) { throw 'CMake is required to build the Dragonwilds player query proxy.' }
$cmakeVersionText = & cmake --version
if ($LASTEXITCODE -ne 0 -or ($cmakeVersionText -join "`n") -notmatch 'cmake version (\d+\.\d+\.\d+)' -or
    [Version]$Matches[1] -lt [Version]'3.24.0') {
    throw 'CMake 3.24 or newer is required to build the Dragonwilds player query proxy.'
}

# Cargo owns this cache, so cleanup follows the existing managed build retention.
# Only the 32 pinned source files are cached, with at most 2 MiB per file.
$source = Join-Path $output 'sources'
$sourcePrefix = $source.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
$maximumSourceBytes = 2MB
Add-Type -AssemblyName System.Net.Http
$client = [Net.Http.HttpClient]::new()
$budget = [Diagnostics.Stopwatch]::StartNew()
try {
    foreach ($entry in $manifest.files) {
        $path = [IO.Path]::GetFullPath((Join-Path $source $entry.path))
        if (-not $path.StartsWith($sourcePrefix, [StringComparison]::OrdinalIgnoreCase) -or
            $entry.url -notmatch '^https://raw\.githubusercontent\.com/(UE4SS-RE/RE-UE4SS/527a483b63b4dd0104fe1ca1a3934a06b87fcfb2|fmtlib/fmt/40626af88bd7df9a5fb80be7b25ac85b122d6c21)/' -or
            $entry.sha256 -notmatch '^[a-f0-9]{64}$') {
            throw 'Dragonwilds proxy source entry is outside the fixed source manifest.'
        }
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            $remainingMilliseconds = 300000 - $budget.ElapsedMilliseconds
            if ($remainingMilliseconds -le 0) { throw 'Dragonwilds proxy source download exceeded its five-minute budget.' }
            [IO.Directory]::CreateDirectory((Split-Path -Parent $path)) | Out-Null
            $partial = $path + '.' + [Guid]::NewGuid().ToString('N') + '.partial'
            $cancellation = [Threading.CancellationTokenSource]::new()
            $cancellation.CancelAfter([int][Math]::Min(60000, $remainingMilliseconds))
            $response = $null
            $inputStream = $null
            $fileStream = $null
            try {
                $response = $client.GetAsync($entry.url, [Net.Http.HttpCompletionOption]::ResponseHeadersRead, $cancellation.Token).GetAwaiter().GetResult()
                $response.EnsureSuccessStatusCode() | Out-Null
                if ($response.Content.Headers.ContentLength -gt $maximumSourceBytes) { throw ('Proxy source exceeds the download size limit: ' + $entry.path) }
                $inputStream = $response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
                $fileStream = [IO.File]::Open($partial, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write)
                $buffer = [byte[]]::new(16384)
                $total = 0
                while (($read = $inputStream.ReadAsync($buffer, 0, $buffer.Length, $cancellation.Token).GetAwaiter().GetResult()) -gt 0) {
                    $total += $read
                    if ($total -gt $maximumSourceBytes) { throw ('Proxy source exceeds the download size limit: ' + $entry.path) }
                    $fileStream.Write($buffer, 0, $read)
                }
                $fileStream.Dispose()
                $fileStream = $null
                if ((Get-FileHash -LiteralPath $partial -Algorithm SHA256).Hash -ne $entry.sha256) {
                    throw ('Downloaded proxy source SHA-256 mismatch: ' + $entry.path)
                }
                Move-Item -LiteralPath $partial -Destination $path
            } finally {
                if ($null -ne $fileStream) { $fileStream.Dispose() }
                if ($null -ne $inputStream) { $inputStream.Dispose() }
                if ($null -ne $response) { $response.Dispose() }
                $cancellation.Dispose()
                if (Test-Path -LiteralPath $partial -PathType Leaf) { Remove-Item -LiteralPath $partial }
            }
        }
        if ((Get-Item -LiteralPath $path).Length -gt $maximumSourceBytes -or
            (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $entry.sha256) {
            throw ('Cached proxy source SHA-256 mismatch; file preserved: ' + $entry.path)
        }
    }
} finally {
    $client.Dispose()
    $budget.Stop()
}

# The shared builder repeats both SHA-256 and Git blob checks and verifies the
# signed system version.dll before generating its forwarding exports.
$buildSource = [IO.Path]::GetFullPath((Join-Path $extensions 'proxy-build'))
# Cargo-owned outputs can be reused by the checkout and an immutable source
# snapshot. Keep each CMake binary tree bound to its own source directory.
$canonicalSource = $buildSource.Replace('\', '/').TrimEnd('/').ToLowerInvariant()
$sourceHasher = [Security.Cryptography.SHA256]::Create()
try {
    $buildName = 'build-' + [BitConverter]::ToString(
        $sourceHasher.ComputeHash([Text.Encoding]::UTF8.GetBytes($canonicalSource))
    ).Replace('-', '').ToLowerInvariant()
} finally { $sourceHasher.Dispose() }
$build = Join-Path $output $buildName
$cache = Join-Path $build 'CMakeCache.txt'
if (Test-Path -LiteralPath $cache -PathType Leaf) {
    $cachedHome = Get-Content -LiteralPath $cache | Where-Object { $_.StartsWith('CMAKE_HOME_DIRECTORY:INTERNAL=') }
    if ($cachedHome -ne ('CMAKE_HOME_DIRECTORY:INTERNAL=' + $buildSource.Replace('\', '/'))) {
        throw 'The proxy build output belongs to another CMake project.'
    }
}

# MSBuild still creates tlog paths subject to MAX_PATH. Map this Cargo-owned
# directory briefly instead of moving its sources or output into another cache.
# Per-drive mutexes separate concurrent builds; exact-target removal cannot
# remove a different mapping if another tool changes the drive concurrently.
if (-not ('LgsmDragonwildsDriveMapping' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
public static class LgsmDragonwildsDriveMapping {
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern uint QueryDosDevice(string name, StringBuilder target, int length);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool DefineDosDevice(uint flags, string name, string target);
    public static string Target(string drive) {
        var target = new StringBuilder(32768);
        if (QueryDosDevice(drive, target, target.Capacity) != 0) return target.ToString();
        int error = Marshal.GetLastWin32Error();
        if (error == 2) return null;
        throw new Win32Exception(error, "Cannot inspect the temporary proxy build drive.");
    }
    public static void Create(string drive, string target) {
        if (!DefineDosDevice(9, drive, target))
            throw new Win32Exception(Marshal.GetLastWin32Error(), "Cannot map the temporary proxy build drive.");
    }
    public static void Remove(string drive, string target) {
        if (!DefineDosDevice(15, drive, target))
            throw new Win32Exception(Marshal.GetLastWin32Error(), "Cannot remove the owned temporary proxy build drive.");
    }
}
'@
}
$rawTarget = if ($output.StartsWith('\\')) { '\??\UNC\' + $output.TrimStart('\') } else { '\??\' + $output }
$mapping = $null
for ($letter = 90; $letter -ge 70; $letter--) {
    $drive = [string][char]$letter + ':'
    $mutex = [Threading.Mutex]::new($false, ('Local\LanGame.Dragonwilds.ProxyDrive.' + $letter))
    $held = $false
    $assigned = $false
    try {
        try { $held = $mutex.WaitOne(0) } catch [Threading.AbandonedMutexException] { $held = $true }
        if (-not $held -or $null -ne [LgsmDragonwildsDriveMapping]::Target($drive)) { continue }
        [LgsmDragonwildsDriveMapping]::Create($drive, $rawTarget)
        $mapping = [pscustomobject]@{ Drive = $drive; Mutex = $mutex }
        $assigned = $true
        break
    } finally {
        if (-not $assigned) {
            if ($held) { $mutex.ReleaseMutex() }
            $mutex.Dispose()
        }
    }
}
if ($null -eq $mapping) { throw 'No unused temporary drive is available for the Dragonwilds proxy build.' }
try {
    $shortRoot = $mapping.Drive + '\'
    $shortSource = Join-Path $shortRoot 'sources'
    $shortBuild = Join-Path $shortRoot $buildName
    # Regenerate only this verified project's CMake metadata: a prior build may
    # have used the original long path or a different temporary drive letter.
    & cmake --fresh -S $buildSource -B $shortBuild -G $generator -A x64 "-DLGSM_PROXY_SOURCE_DIR=$shortSource"
    if ($LASTEXITCODE -ne 0) { throw ('Short-path CMake configure failed: ' + $LASTEXITCODE) }
    & (Join-Path $extensions 'build-proxy.ps1') -SourceDirectory $shortSource -OutputDirectory $shortBuild -Generator $generator
    foreach ($name in @('version.dll', 'LgsmPlayerControl.dll', 'LICENSE-UE4SS.txt', 'LICENSE-fmt.txt')) {
        Copy-Item -LiteralPath (Join-Path $shortBuild ('bin/Release/' + $name)) -Destination (Join-Path $output $name)
    }
} finally {
    try { [LgsmDragonwildsDriveMapping]::Remove($mapping.Drive, $rawTarget) } finally {
        $mapping.Mutex.ReleaseMutex()
        $mapping.Mutex.Dispose()
    }
}
Write-Output (Join-Path $output 'version.dll')
