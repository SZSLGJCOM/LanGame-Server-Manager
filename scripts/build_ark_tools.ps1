param([Parameter(Mandatory = $true)][string]$OutDir, [switch]$ProxyOnly)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
# Cargo can inherit PowerShell 7's PSModulePath when it starts Windows PowerShell
# 5.1. Resolve built-in modules from this host, without changing the parent env.
foreach ($module in @('Microsoft.PowerShell.Management', 'Microsoft.PowerShell.Utility')) {
    Import-Module ([IO.Path]::Combine($PSHOME, 'Modules', $module, "$module.psd1")) -ErrorAction Stop
}
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$OutputEncoding = [Console]::OutputEncoding
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$outputRoot = [IO.Path]::GetFullPath($OutDir)
$portableTarget = Join-Path $projectRoot 'target'
if (($outputRoot.StartsWith($projectRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or $outputRoot -eq $projectRoot) -and
    -not $outputRoot.StartsWith($portableTarget + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'ARK build output must use Cargo OUT_DIR, outside source directories.'
}
$archiveRoot = $env:LANGAME_ARK_ARCHIVE_DIR
if (-not [string]::IsNullOrEmpty($archiveRoot)) {
    if (-not [IO.Path]::IsPathRooted($archiveRoot) -or [IO.Path]::GetPathRoot($archiveRoot).Length -lt 3 -or
        -not (Test-Path -LiteralPath $archiveRoot -PathType Container)) {
        throw 'LANGAME_ARK_ARCHIVE_DIR must be an existing absolute directory.'
    }
    $archiveRoot = [IO.Path]::GetFullPath($archiveRoot)
    if ($outputRoot -eq $archiveRoot -or $outputRoot.StartsWith($archiveRoot.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'ARK build output must be outside the read-only archive directory.'
    }
} else { $archiveRoot = $null }
$archiveCacheRoot = if ($archiveRoot) { $null } else { $env:LANGAME_ARK_ARCHIVE_CACHE_DIR }
if (-not [string]::IsNullOrEmpty($archiveCacheRoot)) {
    if (-not [IO.Path]::IsPathRooted($archiveCacheRoot) -or [IO.Path]::GetPathRoot($archiveCacheRoot).Length -lt 3 -or
        -not (Test-Path -LiteralPath $archiveCacheRoot -PathType Container)) {
        throw 'LANGAME_ARK_ARCHIVE_CACHE_DIR must be an existing absolute directory.'
    }
    $archiveCacheRoot = [IO.Path]::GetFullPath($archiveCacheRoot)
    if ($outputRoot -eq $archiveCacheRoot -or $outputRoot.StartsWith($archiveCacheRoot.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'ARK build output must be outside the read-only archive cache directory.'
    }
} else { $archiveCacheRoot = $null }
[IO.Directory]::CreateDirectory($outputRoot) | Out-Null
$sourceRoot = Join-Path $projectRoot 'modules/ark-tools'
Add-Type -AssemblyName System.Net.Http

function Get-ArchiveSha256([string]$Path) {
    $algorithm = [Security.Cryptography.SHA256]::Create()
    $stream = [IO.File]::OpenRead($Path)
    try {
        return [BitConverter]::ToString($algorithm.ComputeHash($stream)).Replace('-', '').ToLowerInvariant()
    } finally { $stream.Dispose(); $algorithm.Dispose() }
}

function Get-VerifiedArchive([string]$Name, [string]$Url, [string]$Sha256) {
    # An explicit offline input remains strict. Workstation caches are only hints:
    # source updates can pin a different SDK while the synchronized ZIP stays old.
    if ($archiveRoot) {
        $archive = Join-Path $archiveRoot $Name
        if (-not (Test-Path -LiteralPath $archive -PathType Leaf)) {
            throw "Missing ARK archive in LANGAME_ARK_ARCHIVE_DIR: $Name"
        }
        if ((Get-ArchiveSha256 $archive) -ne $Sha256) {
            throw "SHA-256 mismatch for offline ARK archive: $archive (expected $Sha256)"
        }
        return $archive
    }
    $destination = Join-Path $outputRoot $Name
    if ((Test-Path -LiteralPath $destination -PathType Leaf) -and (Get-ArchiveSha256 $destination) -eq $Sha256) {
        return $destination
    }
    if ($archiveCacheRoot) {
        $cached = Join-Path $archiveCacheRoot $Name
        if ((Test-Path -LiteralPath $cached -PathType Leaf) -and (Get-ArchiveSha256 $cached) -eq $Sha256) {
            return $cached
        }
        Write-Host "[ARK] Cached $Name is missing or outdated; fetching the pinned version."
    }
    try {
        $partial = $destination + '.download'
        $client = [Net.Http.HttpClient]::new()
        $cancel = [Threading.CancellationTokenSource]::new([TimeSpan]::FromSeconds(120))
        $response = $null; $inputStream = $null; $outputStream = $null
        try {
            $response = $client.GetAsync($Url, [Net.Http.HttpCompletionOption]::ResponseHeadersRead, $cancel.Token).GetAwaiter().GetResult()
            $response.EnsureSuccessStatusCode() | Out-Null
            $limit = 64MB
            if ($response.Content.Headers.ContentLength -gt $limit) { throw "Archive exceeds 64 MiB: $Name" }
            $inputStream = $response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
            $outputStream = [IO.File]::Create($partial)
            $buffer = [byte[]]::new(65536)
            $total = 0L
            while (($read = $inputStream.ReadAsync($buffer, 0, $buffer.Length, $cancel.Token).GetAwaiter().GetResult()) -gt 0) {
                $total += $read
                if ($total -gt $limit) { throw "Archive exceeds 64 MiB: $Name" }
                $outputStream.Write($buffer, 0, $read)
            }
        } finally {
            if ($outputStream) { $outputStream.Dispose() }
            if ($inputStream) { $inputStream.Dispose() }
            if ($response) { $response.Dispose() }
            $cancel.Dispose(); $client.Dispose()
        }
        if ((Get-ArchiveSha256 $partial) -ne $Sha256) {
            throw "SHA-256 mismatch for $Name"
        }
        # Keep the previous cache intact until the complete replacement verifies.
        if (Test-Path -LiteralPath $destination -PathType Leaf) {
            [IO.File]::Replace($partial, $destination, [NullString]::Value)
        } else {
            [IO.File]::Move($partial, $destination)
        }
    } finally {
        if (Test-Path -LiteralPath $partial -PathType Leaf) { [IO.File]::Delete($partial) }
    }
    return $destination
}

Add-Type -AssemblyName System.IO.Compression.FileSystem
function Expand-Selected([string]$Archive, [string]$Prefix, [string]$Destination, [string[]]$Allowed, [string]$StripPrefix = '') {
    $zip = [IO.Compression.ZipFile]::OpenRead($Archive)
    try {
        foreach ($entry in $zip.Entries) {
            if (-not $entry.FullName.StartsWith($Prefix, [StringComparison]::Ordinal)) { continue }
            $relative = $entry.FullName.Substring($Prefix.Length)
            if ($relative.EndsWith('/') -or -not ($Allowed | Where-Object { $relative -eq $_ -or ($_.EndsWith('/') -and $relative.StartsWith($_, [StringComparison]::Ordinal)) })) { continue }
            if ($StripPrefix -and $relative.StartsWith($StripPrefix, [StringComparison]::Ordinal)) {
                $relative = $relative.Substring($StripPrefix.Length)
            }
            $target = [IO.Path]::GetFullPath((Join-Path $Destination $relative))
            $boundary = [IO.Path]::GetFullPath($Destination) + [IO.Path]::DirectorySeparatorChar
            if (-not $target.StartsWith($boundary, [StringComparison]::OrdinalIgnoreCase)) { throw 'SDK archive path escaped output directory.' }
            # Keep canonical containment checks above; use the extended Windows
            # namespace only for I/O so Windows PowerShell 5.1 accepts long paths.
            $ioTarget = if ($target.StartsWith('\\?\', [StringComparison]::Ordinal)) { $target }
                elseif ($target.StartsWith('\\', [StringComparison]::Ordinal)) { '\\?\UNC\' + $target.Substring(2) }
                else { '\\?\' + $target }
            [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($ioTarget)) | Out-Null
            [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $ioTarget, $true)
        }
    } finally { $zip.Dispose() }
}

$asaCommit = 'bd376a0422291b0b1bf15a0b7868e7e275605da9'
$aseCommit = '9459cb3941c2e62d4144cbb8294fe6c4c772d564'
$loaderCommit = '4e04dc8982e1473920278f199638b29dc511d539'
$asa = Get-VerifiedArchive 'AsaApi-sdk.zip' "https://codeload.github.com/ArkServerApi/AsaApi/zip/$asaCommit" 'e179b1de7037ea48e69531e4aac75e02126fa63a8054cc32236f818a20ea95bc'
$loader = Get-VerifiedArchive 'AsaApiLoader-sdk.zip' "https://codeload.github.com/ArkServerApi/AsaApiLoader/zip/$loaderCommit" 'dd3c6ac17d2bdc9d072ca01c8e86f9451e6de909c2bf901bb5b2c63cb939e833'
# MSVC still applies MAX_PATH to relative includes containing ../, even when
# the normalized file exists. Keep the SDK's Public/Private relationship while
# dropping archive-only Core ancestors so managed snapshot builds stay in range.
$asaRoot = Join-Path $outputRoot 'asa'
$aseRoot = Join-Path $outputRoot 'ase'
$loaderRoot = Join-Path $outputRoot 'AsaApiLoader'
Expand-Selected $loader "AsaApiLoader-$loaderCommit/" $loaderRoot @('Version/version.def', 'Version/version_asm.asm', 'LICENSE')
if ($ProxyOnly) {
    # Native regression tests compile the actual proxy and assembly using the
    # same hash-verified SDK JSON header, without building either game plugin.
    Expand-Selected $asa "AsaApi-$asaCommit/" $asaRoot @('AsaApi/Core/Public/json.hpp', 'LICENSE') 'AsaApi/Core/'
} else {
    $ase = Get-VerifiedArchive 'AseApi-sdk.zip' "https://codeload.github.com/ArkServerApi/AseApi/zip/$aseCommit" '500cc0e0d46e75e3c67374a81914fe318ee1f88db8bca0c291f2e8a72ede3def'
    $asaRelease = Get-VerifiedArchive 'asa-runtime.zip' 'https://github.com/ArkServerApi/AsaApi/releases/download/2.11/AsaApi_2.11.zip' '64f5f1165ef73f3439bba3794e77e379489167cc385c0ac5c4717ca60ad3e88a'
    Expand-Selected $asa "AsaApi-$asaCommit/" $asaRoot @('AsaApi/Core/Public/', 'AsaApi/Core/Private/Ark/Globals.h', 'LICENSE') 'AsaApi/Core/'
    Expand-Selected $ase "AseApi-$aseCommit/" $aseRoot @('version/Core/Public/', 'version/Core/Private/Ark/Globals.h', 'out_lib/ArkApi.lib', 'LICENSE') 'version/Core/'
    Expand-Selected $asaRelease '' $asaRoot @('Lib/AsaApi.lib')
    # UnrealString includes fmt/format.h while Logger already includes this exact
    # bundled fmt version. Expose that same dependency under its public include path.
    $fmtRoot = Join-Path $outputRoot 'fmt-include'
    [IO.Directory]::CreateDirectory((Join-Path $fmtRoot 'fmt')) | Out-Null
    Copy-Item -Path (Join-Path $asaRoot 'Public/Logger/spdlog/fmt/bundled/*') -Destination (Join-Path $fmtRoot 'fmt') -Force
}

$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
if (-not (Test-Path -LiteralPath $vswhere)) { throw 'Visual Studio C++ Build Tools are required to build ARK tools.' }
$vsRoot = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if ($LASTEXITCODE -ne 0 -or -not $vsRoot) { throw 'Visual Studio C++ toolchain was not found.' }
$msvcRoot = (Get-ChildItem -LiteralPath (Join-Path $vsRoot 'VC/Tools/MSVC') -Directory | Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1).FullName
$toolBin = Join-Path $msvcRoot 'bin/Hostx64/x64'
$sdkRoot = (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows Kits\Installed Roots').KitsRoot10
$sdkVersion = (Get-ChildItem -LiteralPath (Join-Path $sdkRoot 'Include') -Directory | Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName 'um/Windows.h') } | Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1).Name
$includePaths = @((Join-Path $msvcRoot 'include')) + @('ucrt', 'shared', 'um', 'winrt') | ForEach-Object { if ([IO.Path]::IsPathRooted($_)) { $_ } else { Join-Path $sdkRoot "Include/$sdkVersion/$_" } }
$libPaths = @((Join-Path $msvcRoot 'lib/x64'), (Join-Path $sdkRoot "Lib/$sdkVersion/ucrt/x64"), (Join-Path $sdkRoot "Lib/$sdkVersion/um/x64"))
$previousInclude = $env:INCLUDE
$previousLib = $env:LIB
$previousPath = $env:PATH
try {
    $env:INCLUDE = $includePaths -join ';'
    $env:LIB = $libPaths -join ';'
    $env:PATH = "$toolBin;$previousPath"
    $editions = if ($ProxyOnly) { @() } else { @('ase', 'asa') }
    foreach ($edition in $editions) {
        $headers = if ($edition -eq 'asa') { Join-Path $asaRoot 'Public' } else { Join-Path $aseRoot 'Public' }
        $importLib = if ($edition -eq 'asa') { Join-Path $asaRoot 'Lib/AsaApi.lib' } else { Join-Path $aseRoot 'out_lib/ArkApi.lib' }
        $object = Join-Path $outputRoot "LgsmArkTools-$edition.obj"
        $dll = Join-Path $outputRoot "LgsmArkTools-$edition.dll"
        $compile = @('/nologo', '/std:c++20', '/EHsc', '/MD', '/O2', '/utf-8', '/bigobj', '/DNOMINMAX', '/DUNICODE', '/D_UNICODE', '/D_SILENCE_ALL_CXX17_DEPRECATION_WARNINGS', '/D_DISABLE_CONSTEXPR_MUTEX_CONSTRUCTOR', '/LD', "/I$headers", "/I$headers/API/UE", "/Fo$object", (Join-Path $sourceRoot 'LgsmArkTools.cpp'))
        if ($edition -eq 'asa') { $compile += @('/DLGSM_ASA', "/I$fmtRoot") }
        & (Join-Path $toolBin 'cl.exe') @compile '/link' $importLib "/OUT:$dll" "/IMPLIB:$outputRoot/LgsmArkTools-$edition.lib" '/INCREMENTAL:NO'
        if ($LASTEXITCODE -ne 0) { throw "ARK $edition plugin compilation failed." }
    }
    & (Join-Path $toolBin 'ml64.exe') /nologo /c "/Fo$outputRoot/asa-version-asm.obj" (Join-Path $loaderRoot 'Version/version_asm.asm')
    if ($LASTEXITCODE -ne 0) { throw 'ASA version proxy assembly failed.' }
    & (Join-Path $toolBin 'cl.exe') /nologo /std:c++20 /EHsc /MT /O2 /utf-8 /LD "/I$asaRoot/Public" "/Fo$outputRoot/asa-version.obj" (Join-Path $sourceRoot 'asa_version.cpp') '/link' "$outputRoot/asa-version-asm.obj" "/DEF:$loaderRoot/Version/version.def" "/OUT:$outputRoot/asa-version.dll" "/IMPLIB:$outputRoot/asa-version.lib" /INCREMENTAL:NO
    if ($LASTEXITCODE -ne 0) { throw 'ASA version proxy compilation failed.' }
} finally {
    $env:INCLUDE = $previousInclude
    $env:LIB = $previousLib
    $env:PATH = $previousPath
}
if ($ProxyOnly) { Write-Output 'Built asa-version.dll using the pinned SDK and loader assembly.' }
else { Write-Output 'Built LgsmArkTools-ase.dll, LgsmArkTools-asa.dll and asa-version.dll.' }
