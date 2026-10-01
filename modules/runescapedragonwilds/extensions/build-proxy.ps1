[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$SourceDirectory,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [string]$Generator = 'Visual Studio 18 2026',
    [switch]$DownloadSources
)

$ErrorActionPreference = 'Stop'
if (-not [Environment]::Is64BitProcess -or $env:OS -ne 'Windows_NT') {
    throw 'Use 64-bit PowerShell on Windows to build this proxy.'
}
$manifest = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'proxy-sources.json') -Raw | ConvertFrom-Json
$sourceRoot = [IO.Path]::GetFullPath($SourceDirectory)
$outputRoot = [IO.Path]::GetFullPath($OutputDirectory)
$separator = [IO.Path]::DirectorySeparatorChar
if ($outputRoot -eq $sourceRoot -or $outputRoot.StartsWith($sourceRoot.TrimEnd($separator) + $separator, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'The output directory must be separate from the source directory.'
}

function Assert-SourceHash {
    param([string]$Path, [object]$Entry)
    if ((Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -ne $Entry.sha256) {
        throw ('Source SHA-256 mismatch; file preserved: ' + $Entry.path)
    }
    $bytes = [IO.File]::ReadAllBytes($Path)
    $prefix = [Text.Encoding]::UTF8.GetBytes('blob ' + $bytes.Length + [char]0)
    $sha1 = [Security.Cryptography.SHA1]::Create()
    try {
        $blob = [BitConverter]::ToString($sha1.ComputeHash([byte[]]($prefix + $bytes))).Replace('-', '').ToLowerInvariant()
    } finally {
        $sha1.Dispose()
    }
    if ($blob -ne $Entry.git_blob) {
        throw ('Source Git blob mismatch: ' + $Entry.path)
    }
}

foreach ($entry in $manifest.files) {
    $path = [IO.Path]::GetFullPath((Join-Path $sourceRoot $entry.path))
    if (-not $path.StartsWith($sourceRoot.TrimEnd($separator) + $separator, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'A manifest path escapes the source directory.'
    }
    if ($entry.url -notmatch '^https://raw\.githubusercontent\.com/(UE4SS-RE/RE-UE4SS/527a483b63b4dd0104fe1ca1a3934a06b87fcfb2|fmtlib/fmt/40626af88bd7df9a5fb80be7b25ac85b122d6c21)/') {
        throw 'A source URL is outside the fixed upstream revisions.'
    }
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        if (-not $DownloadSources) {
            throw ('Source file is missing. Obtain the fixed sources first: ' + $entry.path)
        }
        New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
        Invoke-WebRequest -Uri $entry.url -OutFile $path -TimeoutSec 60 -UseBasicParsing
    }
    Assert-SourceHash -Path $path -Entry $entry
}

$systemDll = Join-Path $env:WINDIR 'System32/version.dll'
if ((Get-AuthenticodeSignature -LiteralPath $systemDll).Status -ne 'Valid') {
    throw 'The local Windows version.dll signature is not valid.'
}
$buildSource = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot 'proxy-build')).Path
$cache = Join-Path $outputRoot 'CMakeCache.txt'
if (Test-Path -LiteralPath $cache) {
    $cachedHome = Get-Content -LiteralPath $cache | Where-Object { $_.StartsWith('CMAKE_HOME_DIRECTORY:INTERNAL=') }
    $expectedHome = 'CMAKE_HOME_DIRECTORY:INTERNAL=' + $buildSource.Replace('\', '/')
    if ($cachedHome -ne $expectedHome) { throw 'The output directory belongs to another CMake project.' }
}
& cmake -S $buildSource -B $outputRoot -G $Generator -A x64 "-DLGSM_PROXY_SOURCE_DIR=$sourceRoot" "-DUE4SS_PROXY_PATH=$systemDll"
if ($LASTEXITCODE -ne 0) { throw ('CMake configure failed: ' + $LASTEXITCODE) }
& cmake --build $outputRoot --config Release --target proxy --parallel 2
if ($LASTEXITCODE -ne 0) { throw ('CMake build failed: ' + $LASTEXITCODE) }

$runtimeDirectory = Join-Path $outputRoot 'bin/Release'
$proxy = Join-Path $runtimeDirectory 'version.dll'
Copy-Item -LiteralPath (Join-Path $sourceRoot 'LICENSE') -Destination (Join-Path $runtimeDirectory 'LICENSE-UE4SS.txt')
Copy-Item -LiteralPath (Join-Path $sourceRoot 'deps/third/fmt-11.2.0/LICENSE') -Destination (Join-Path $runtimeDirectory 'LICENSE-fmt.txt')
$evidence = [ordered]@{
    ue4ss_commit = $manifest.ue4ss_commit
    fmt_version = $manifest.fmt_version
    source_files_verified = $manifest.files.Count
    source_manifest_sha256 = (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'proxy-sources.json') -Algorithm SHA256).Hash.ToLowerInvariant()
    generated_at_utc = [DateTime]::UtcNow.ToString('o')
    generator = $Generator
    architecture = 'x64'
    configuration = 'Release'
    configure_exit_code = 0
    build_exit_code = 0
    system_version_sha256 = (Get-FileHash -LiteralPath $systemDll -Algorithm SHA256).Hash.ToLowerInvariant()
    proxy_sha256 = (Get-FileHash -LiteralPath $proxy -Algorithm SHA256).Hash.ToLowerInvariant()
    proxy_bytes = (Get-Item -LiteralPath $proxy).Length
}
$evidence | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $outputRoot 'proxy-build-evidence.json') -Encoding utf8
Get-FileHash -LiteralPath $proxy -Algorithm SHA256
