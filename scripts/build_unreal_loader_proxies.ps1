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
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $PSScriptRoot 'build_control_output.ps1')
$output = Resolve-ControlBuildOutput -OutDir $OutDir -ProjectRoot $projectRoot -CargoLeaf 'unreal-loader-proxies'
foreach ($path in @($output, $projectRoot)) {
    if ($path.IndexOfAny([char[]]'%!&|<>^"') -ge 0 -or $path.Contains("`n") -or $path.Contains("`r")) {
        throw 'MSVC build paths contain unsupported command characters.'
    }
}
$sourceArchive = $env:LANGAME_PROXY_SOURCE_DIR
if (-not [string]::IsNullOrEmpty($sourceArchive)) {
    if (-not [IO.Path]::IsPathRooted($sourceArchive) -or [IO.Path]::GetPathRoot($sourceArchive).Length -lt 3 -or
        -not (Test-Path -LiteralPath $sourceArchive -PathType Container)) {
        throw 'LANGAME_PROXY_SOURCE_DIR must be an existing absolute directory.'
    }
    $sourceArchive = [IO.Path]::GetFullPath($sourceArchive)
    $archivePrefix = $sourceArchive.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
    if ($output -eq $sourceArchive -or $output.StartsWith($archivePrefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Proxy build output must be outside the read-only source directory.'
    }
} else { $sourceArchive = $null }
$extensions = Join-Path $projectRoot 'modules/runescapedragonwilds/extensions'
$manifest = Get-Content -LiteralPath (Join-Path $extensions 'proxy-sources.json') -Raw | ConvertFrom-Json
if ($manifest.schema -ne 1 -or $manifest.ue4ss_commit -ne '527a483b63b4dd0104fe1ca1a3934a06b87fcfb2' -or
    $manifest.fmt_version -ne '11.2.0' -or $manifest.files.Count -ne 32) {
    throw 'UnrealLoader proxy source manifest is not the verified fixed revision.'
}
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
if (-not (Test-Path -LiteralPath $vswhere -PathType Leaf)) { throw 'MSVC C++ Build Tools are required for the bundled Unreal loader proxies.' }
$installationVersion = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationVersion
if ($LASTEXITCODE -ne 0 -or -not $installationVersion) { throw 'MSVC x64 toolchain was not found.' }
$generator = switch (([Version]$installationVersion).Major) {
    18 { 'Visual Studio 18 2026' }
    17 { 'Visual Studio 17 2022' }
    default { throw 'Unreal loader proxies require Visual Studio 2022 or 2026.' }
}
if (-not (Get-Command cmake -ErrorAction SilentlyContinue)) { throw 'CMake is required to build the Unreal loader proxies.' }
$cmakeVersionText = & cmake --version
if ($LASTEXITCODE -ne 0 -or ($cmakeVersionText -join "`n") -notmatch 'cmake version (\d+\.\d+\.\d+)' -or
    [Version]$Matches[1] -lt [Version]'3.24.0') {
    throw 'CMake 3.24 or newer is required to build the Unreal loader proxies.'
}

[IO.Directory]::CreateDirectory($output) | Out-Null
Assert-PlainControlBuildOutput $output
# Source downloads, CMake metadata and MSBuild tlogs can exceed MAX_PATH.
# Keep all of them inside the existing mapped, Cargo-owned output directory.
# Per-drive mutexes separate concurrent builds; exact-target removal cannot
# remove a different mapping if another tool changes the drive concurrently.
if (-not ('LgsmUnrealLoaderDriveMapping' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
public static class LgsmUnrealLoaderDriveMapping {
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
    # The existing Dragonwilds builder reserves the same drive-letter resource.
    # Share its mutex namespace even when two Cargo builds run concurrently.
    $mutex = [Threading.Mutex]::new($false, ('Local\LanGame.Dragonwilds.ProxyDrive.' + $letter))
    $held = $false
    $assigned = $false
    try {
        try { $held = $mutex.WaitOne(0) } catch [Threading.AbandonedMutexException] { $held = $true }
        if (-not $held -or $null -ne [LgsmUnrealLoaderDriveMapping]::Target($drive)) { continue }
        [LgsmUnrealLoaderDriveMapping]::Create($drive, $rawTarget)
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
if ($null -eq $mapping) { throw 'No unused temporary drive is available for the UnrealLoader proxy build.' }
try {
    $shortRoot = $mapping.Drive + '\'
    # Cargo owns this cache, so cleanup follows the existing managed build retention.
    # Only the 32 pinned source files are cached, with at most 2 MiB per file.
    $source = Join-Path $shortRoot 'sources'
    Assert-PlainControlBuildOutput $source
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
                throw 'UnrealLoader proxy source entry is outside the fixed source manifest.'
            }
            if ($sourceArchive) {
                $archivePath = [IO.Path]::GetFullPath((Join-Path $sourceArchive $entry.path))
                if (-not $archivePath.StartsWith($archivePrefix, [StringComparison]::OrdinalIgnoreCase) -or
                    -not (Test-Path -LiteralPath $archivePath -PathType Leaf)) {
                    throw ('Missing proxy source in LANGAME_PROXY_SOURCE_DIR: ' + $entry.path)
                }
                if ((Get-Item -LiteralPath $archivePath).Length -gt $maximumSourceBytes -or
                    (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash -ne $entry.sha256) {
                    throw ('Configured proxy source SHA-256 mismatch: ' + $entry.path)
                }
                if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
                    [IO.Directory]::CreateDirectory((Split-Path -Parent $path)) | Out-Null
                    Assert-PlainControlBuildOutput (Split-Path -Parent $path)
                    Copy-Item -LiteralPath $archivePath -Destination $path
                }
            }
            if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
                $remainingMilliseconds = 300000 - $budget.ElapsedMilliseconds
                if ($remainingMilliseconds -le 0) { throw 'UnrealLoader proxy source download exceeded its five-minute budget.' }
                [IO.Directory]::CreateDirectory((Split-Path -Parent $path)) | Out-Null
                Assert-PlainControlBuildOutput (Split-Path -Parent $path)
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

    # Repeat the immutable Git identity check in addition to each download hash.
    foreach ($entry in $manifest.files) {
        $sourceBytes = [IO.File]::ReadAllBytes((Join-Path $source $entry.path))
        $blobPrefix = [Text.Encoding]::UTF8.GetBytes('blob ' + $sourceBytes.Length + [char]0)
        $blobHasher = [Security.Cryptography.SHA1]::Create()
        try {
            $blob = [BitConverter]::ToString($blobHasher.ComputeHash([byte[]]($blobPrefix + $sourceBytes))).Replace('-', '').ToLowerInvariant()
        } finally { $blobHasher.Dispose() }
        if ($blob -ne $entry.git_blob) { throw ('Proxy source Git identity mismatch: ' + $entry.path) }
    }
    # Generate real forwarding exports from the signed local Windows targets; a
    # renamed dwmapi.dll does not provide VERSION/DSOUND's imported functions.
    $buildSource = [IO.Path]::GetFullPath((Join-Path $projectRoot 'apps/desktop/src-tauri/native/unreal-loader-proxy'))
    # Cargo-owned outputs can be reused by the checkout and an immutable source
    # snapshot. Keep each CMake binary tree bound to its own source directory.
    $canonicalSource = $buildSource.Replace('\', '/').TrimEnd('/').ToLowerInvariant()
    $sourceHasher = [Security.Cryptography.SHA256]::Create()
    try {
        $buildName = 'build-' + [BitConverter]::ToString(
            $sourceHasher.ComputeHash([Text.Encoding]::UTF8.GetBytes($canonicalSource))
        ).Replace('-', '').ToLowerInvariant()
    } finally { $sourceHasher.Dispose() }
    $build = Join-Path $shortRoot $buildName
    Assert-PlainControlBuildOutput $build
    $cache = Join-Path $build 'CMakeCache.txt'
    if (Test-Path -LiteralPath $cache -PathType Leaf) {
        $cachedHome = Get-Content -LiteralPath $cache | Where-Object { $_.StartsWith('CMAKE_HOME_DIRECTORY:INTERNAL=') }
        if ($cachedHome -ne ('CMAKE_HOME_DIRECTORY:INTERNAL=' + $buildSource.Replace('\', '/'))) {
            throw 'The proxy build output belongs to another CMake project.'
        }
    }

    $shortSource = Join-Path $shortRoot 'sources'
    $shortBuild = Join-Path $shortRoot $buildName
    # Regenerate only this verified project's CMake metadata: a prior build may
    # have used the original long path or a different temporary drive letter.
    foreach ($proxyName in @('version', 'dsound', 'winmm')) {
        $systemDll = Join-Path $env:WINDIR ('System32/' + $proxyName + '.dll')
        if ((Get-AuthenticodeSignature -LiteralPath $systemDll).Status -ne 'Valid') {
            throw ('The local system proxy target is not signed: ' + $proxyName)
        }
        $proxyBuild = $shortBuild + '-' + $proxyName
        & cmake --fresh -S $buildSource -B $proxyBuild -G $generator -A x64 "-DLGSM_PROXY_SOURCE_DIR=$shortSource" "-DUE4SS_PROXY_PATH=$systemDll"
        if ($LASTEXITCODE -ne 0) { throw ('Proxy CMake configure failed: ' + $LASTEXITCODE) }
        & cmake --build $proxyBuild --config Release --target proxy --parallel 2
        if ($LASTEXITCODE -ne 0) { throw ('Proxy native build failed: ' + $LASTEXITCODE) }
        Copy-Item -LiteralPath (Join-Path $proxyBuild ('bin/Release/' + $proxyName + '.dll')) -Destination (Join-Path $output ($proxyName + '.dll'))
    }
    Copy-Item -LiteralPath (Join-Path $shortSource 'LICENSE') -Destination (Join-Path $output 'LICENSE-UE4SS.txt')
    Copy-Item -LiteralPath (Join-Path $shortSource 'deps/third/fmt-11.2.0/LICENSE') -Destination (Join-Path $output 'LICENSE-fmt.txt')
} finally {
    try { [LgsmUnrealLoaderDriveMapping]::Remove($mapping.Drive, $rawTarget) } finally {
        $mapping.Mutex.ReleaseMutex()
        $mapping.Mutex.Dispose()
    }
}
Write-Output (Join-Path $output 'version.dll')
