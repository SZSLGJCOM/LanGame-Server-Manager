[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$NsisRoot,
    [switch]$CompileOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$nsis = (Resolve-Path -LiteralPath (Join-Path $NsisRoot 'makensis.exe')).Path
$plugins = (Resolve-Path -LiteralPath (Join-Path $NsisRoot 'Plugins/x86-unicode/additional')).Path
$hooks = Join-Path $repoRoot 'apps/desktop/src-tauri/installer/hooks.nsh'
$languages = Join-Path $repoRoot 'apps/desktop/src-tauri/installer/languages'
$historicalFixtures = Join-Path $PSScriptRoot 'fixtures/retired-package-resources'
$tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
# Keep paths short enough for NSIS while exercising a directory containing spaces.
$fixtureRoot = Join-Path $tempRoot ('lgr ' + [guid]::NewGuid().ToString('N'))
$installRoot = Join-Path $fixtureRoot 'app'
$junction = Join-Path $installRoot 'retired'
$junctionTarget = Join-Path $fixtureRoot 'junction-target'
$receipt = Join-Path $fixtureRoot 'reached.txt'
$installer = Join-Path $fixtureRoot 'cleanup.exe'
$uninstaller = Join-Path $fixtureRoot 'uninstall.exe'
$mainBinaryName = 'lgsm-resource-probe-' + [guid]::NewGuid().ToString('N')
$utf8 = [Text.UTF8Encoding]::new($false)
$ownedProcesses = [Collections.Generic.List[Diagnostics.Process]]::new()
$scenarios = 0
$null = [IO.Directory]::CreateDirectory($installRoot)

function Assert-OwnedPath([string]$Path) {
    $absolute = [IO.Path]::GetFullPath($Path)
    if (-not $absolute.StartsWith($fixtureRoot + '\', [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Fixture operation escaped its unique temporary directory.'
    }
}

function Write-Payload([string]$Path, [byte[]]$Bytes) {
    Assert-OwnedPath $Path
    $null = [IO.Directory]::CreateDirectory((Split-Path -Parent $Path))
    [IO.File]::WriteAllBytes($Path, $Bytes)
}

function Hash-Bytes([byte[]]$Bytes) {
    $algorithm = [Security.Cryptography.SHA256]::Create()
    try { return [BitConverter]::ToString($algorithm.ComputeHash($Bytes)).Replace('-', '').ToLowerInvariant() }
    finally { $algorithm.Dispose() }
}

function Assert-FileHash([string]$Path, [string]$Expected) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf) -or
        (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -ine $Expected) {
        throw "Preserved fixture bytes changed: $Path"
    }
}

function Remove-OwnedJunction {
    if (Test-Path -LiteralPath $junction) {
        Assert-OwnedPath $junction
        if (((Get-Item -LiteralPath $junction -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0) {
            throw 'Expected the owned junction; refusing directory deletion.'
        }
        # Nonrecursive deletion removes the link itself, never its target contents.
        [IO.Directory]::Delete($junction)
    }
}

function Run-Cleanup([string]$Mode, [int]$ExpectedExit, [switch]$Update) {
    if (Test-Path -LiteralPath $receipt) { Remove-Item -LiteralPath $receipt }
    $executable = if ($Mode -eq 'install') { $installer } else { $uninstaller }
    $arguments = if ($Mode -eq 'install') { @('/S') } else { @('/S', "_?=$fixtureRoot") }
    if ($Update) { $arguments = @('/P', '/UPDATE') }
    $process = Start-Process -FilePath $executable -ArgumentList $arguments -PassThru -WindowStyle Hidden
    $ownedProcesses.Add($process)
    $null = $process.Handle
    if (-not $process.WaitForExit(20000)) { throw 'Disposable resource cleanup exceeded its deadline.' }
    if ($process.ExitCode -ne $ExpectedExit) {
        throw "Expected $Mode cleanup exit $ExpectedExit, got $($process.ExitCode)."
    }
    if ((Test-Path -LiteralPath $receipt) -ne ($ExpectedExit -eq 0)) {
        throw 'Resource cleanup reached the next section after reporting failure.'
    }
    if ($Update -and [IO.File]::ReadAllText($receipt) -cne 'install reached; passive=1; update=1') {
        throw 'The installer did not receive the real passive update arguments.'
    }
    $script:scenarios++
}

# Original package fixtures from Git blobs 742dbcfe3cb8d900bd4e4392c836c2fbf9554488
# and ead1b2943558012dcc065011e11c43a6f598a79d; CI reads these checked-in inputs.
# Both line endings were distributed by earlier Windows builds.
$retired = @(
    @{
        Relative = 'modules\enshrouded\config-fixtures\2026-09-30-native_ban_contract_build_23178631.json'
        LF = 'a7e2731217a7522f6d28bf0355cdfa098253d1d90e2c6d4c1594e407de239719'
        CRLF = 'ccff88d90223848426dc7ac8ecb0c9969d497b1ee886c07e9b28303a52810b60'
        Fixture = 'enshrouded.json'
        LFLength = 1346
        CRLFLength = 1400
    },
    @{
        Relative = 'modules\terraria\config-fixtures\2026-09-28-official_dedicated_zip_1458.json'
        LF = '55860a78dc8e8640923556b8469ab9d375b8c82fc36e815a9dd9908160d75ec5'
        CRLF = 'fd5de98799c4655ee34dc4c232f58a804fcdab066be290b6e7cda1ba56c8b873'
        Fixture = 'terraria.json'
        LFLength = 1093
        CRLFLength = 1144
    }
)

try {
    $payload = $utf8.GetBytes('{"fixture":"retired resource"}' + "`n")
    $payloadHash = Hash-Bytes $payload
    $managed = Join-Path $installRoot 'retired\managed.json'
    $userFile = Join-Path $installRoot 'user-content.txt'
    $userBytes = $utf8.GetBytes('User-created fixture content must remain unchanged.')
    $userHash = Hash-Bytes $userBytes
    $source = @'
Unicode true
RequestExecutionLevel user
SilentInstall silent
SilentUnInstall silent
Name "LGSM disposable resource cleanup"
!define PRODUCTNAME "LGSM disposable resource cleanup"
!define VERSION "0.1.0"
!define MAINBINARYNAME "@MAINBINARYNAME@"
OutFile "@INSTALLER@"
!include MUI2.nsh
!include LogicLib.nsh
!include FileFunc.nsh
!addplugindir "@PLUGINS@"
; Match Tauri's include order and require the real hook to replace its guard.
!macro CheckIfAppIsRunning executablePath productName
  !error "The unsafe default guard must have been replaced."
!macroend
!include "@HOOKS@"
Var PassiveMode
Var UpdateMode
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "SimpChinese"
!include "@LANGUAGES@\English.nsh"
!include "@LANGUAGES@\SimpChinese.nsh"
Section
  StrCpy $INSTDIR "@ROOT@"
  StrCpy $PassiveMode 0
  StrCpy $UpdateMode 0
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/P" $1
  IfErrors +2
    StrCpy $PassiveMode 1
  ClearErrors
  ${GetOptions} $0 "/UPDATE" $1
  IfErrors +2
    StrCpy $UpdateMode 1
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "" "retired\managed.json" "@HASH@" "@HASH@"
  !insertmacro NSIS_HOOK_POSTINSTALL
  FileOpen $0 "@RECEIPT@" w
  FileWrite $0 "install reached; passive=$PassiveMode; update=$UpdateMode"
  FileClose $0
  WriteUninstaller "@UNINSTALLER@"
SectionEnd
Section "Uninstall"
  StrCpy $INSTDIR "@ROOT@"
  StrCpy $PassiveMode 1
  StrCpy $UpdateMode 0
  !insertmacro NSIS_HOOK_PREUNINSTALL
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "un." "retired\managed.json" "@HASH@" "@HASH@"
  FileOpen $0 "@RECEIPT@" w
  FileWrite $0 "uninstall reached"
  FileClose $0
SectionEnd
'@
    $source = $source.Replace('@INSTALLER@', $installer).Replace('@UNINSTALLER@', $uninstaller)
    $source = $source.Replace('@PLUGINS@', $plugins).Replace('@HOOKS@', $hooks)
    $source = $source.Replace('@LANGUAGES@', $languages).Replace('@MAINBINARYNAME@', $mainBinaryName)
    $source = $source.Replace('@ROOT@', $installRoot)
    $source = $source.Replace('@HASH@', $payloadHash).Replace('@RECEIPT@', $receipt)
    $script = Join-Path $fixtureRoot 'cleanup.nsi'
    [IO.File]::WriteAllText($script, $source, $utf8)
    $compilerOutput = & $nsis /V2 $script 2>&1
    if ($LASTEXITCODE -ne 0) { throw "NSIS compilation failed: $compilerOutput" }
    if ($compilerOutput -match 'unknown variable/constant') { throw "Unresolved NSIS symbol: $compilerOutput" }
    if ($CompileOnly) {
        Write-Output 'NSIS resource cleanup compiled for installer/uninstaller paths; execution was not requested.'
        return
    }

    Write-Payload $userFile $userBytes
    foreach ($mode in @('install', 'uninstall')) {
        Write-Payload $managed $payload
        Run-Cleanup $mode 0 -Update:($mode -eq 'install')
        if (Test-Path -LiteralPath $managed) { throw 'An unchanged retired resource was not removed.' }
        Assert-FileHash $userFile $userHash

        $edited = $utf8.GetBytes('{"fixture":"user edited resource"}' + "`n")
        Write-Payload $managed $edited
        Run-Cleanup $mode 0
        Assert-FileHash $managed (Hash-Bytes $edited)
        Assert-FileHash $userFile $userHash
        Remove-Item -LiteralPath $managed

        $oversized = $utf8.GetBytes('x' * 65537)
        Write-Payload $managed $oversized
        Run-Cleanup $mode 0
        Assert-FileHash $managed (Hash-Bytes $oversized)
        Assert-FileHash $userFile $userHash
        Remove-Item -LiteralPath $managed
        [IO.Directory]::Delete((Split-Path -Parent $managed))

        # Both the file and its parent are absent; repeated cleanup is successful.
        Run-Cleanup $mode 0
        Run-Cleanup $mode 0
        Assert-FileHash $userFile $userHash

        Write-Payload (Join-Path $junctionTarget 'managed.json') $payload
        $null = New-Item -ItemType Junction -Path $junction -Target $junctionTarget
        try {
            Run-Cleanup $mode 0
            if (((Get-Item -LiteralPath $junction -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0) {
                throw 'Cleanup changed the junction.'
            }
            Assert-FileHash (Join-Path $junctionTarget 'managed.json') $payloadHash
        }
        finally { Remove-OwnedJunction }

        Write-Payload $managed $payload
        # Reading is allowed, but the existing handle denies deletion sharing.
        $locked = [IO.File]::Open($managed, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
        try {
            Run-Cleanup $mode 11
            Assert-FileHash $managed $payloadHash
            Assert-FileHash $userFile $userHash
        }
        finally { $locked.Dispose() }
        Run-Cleanup $mode 0
        if (Test-Path -LiteralPath $managed) { throw 'The unlocked retired resource was not removed.' }

        foreach ($ending in @('LF', 'CRLF')) {
            foreach ($item in $retired) {
                $text = [IO.File]::ReadAllText((Join-Path $historicalFixtures $item.Fixture), $utf8).Replace("`r`n", "`n")
                if ($ending -eq 'CRLF') { $text = $text.Replace("`n", "`r`n") }
                $bytes = $utf8.GetBytes($text)
                if ($bytes.Length -ne $item["${ending}Length"] -or (Hash-Bytes $bytes) -cne $item[$ending]) {
                    throw 'Historical package fixture identity changed.'
                }
                $path = Join-Path $installRoot $item.Relative
                Write-Payload $path $bytes
                Write-Payload (Join-Path (Split-Path -Parent $path) 'user-content.txt') $userBytes
            }
            Run-Cleanup $mode 0
            foreach ($item in $retired) {
                $path = Join-Path $installRoot $item.Relative
                if (Test-Path -LiteralPath $path) { throw "Production retirement table left $($item.Relative)." }
                Assert-FileHash (Join-Path (Split-Path -Parent $path) 'user-content.txt') $userHash
            }
        }
    }
    Write-Output "NSIS retired-resource cleanup: $scenarios scenarios passed; actual installations and user data untouched."
}
finally {
    foreach ($process in $ownedProcesses) {
        if (-not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
        $process.Dispose()
    }
    if (Test-Path -LiteralPath $junction) {
        if (((Get-Item -LiteralPath $junction -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            Remove-OwnedJunction
        }
    }
    $resolved = [IO.Path]::GetFullPath($fixtureRoot)
    if (-not $resolved.StartsWith($tempRoot + '\', [StringComparison]::OrdinalIgnoreCase) -or
        $resolved -cne $fixtureRoot -or (Split-Path -Leaf $resolved) -notmatch '^lgr [a-f0-9]{32}$' -or
        ((Get-Item -LiteralPath $resolved -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw 'Refusing recursive cleanup outside the owned temporary fixture.'
    }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
