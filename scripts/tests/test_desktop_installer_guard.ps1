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
$tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$fixtureRoot = Join-Path $tempRoot ('lgsm-installer-guard-' + [guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $fixtureRoot
$utf8 = [System.Text.UTF8Encoding]::new($false)
$ownedProcesses = [System.Collections.Generic.List[System.Diagnostics.Process]]::new()
$scenarios = 0

function Write-Fixture([string]$Name, [string]$Source) {
    $path = Join-Path $fixtureRoot $Name
    [System.IO.File]::WriteAllText($path, $Source, $utf8)
    $path
}

function Compile-Fixture([string]$Path, [string[]]$Defines = @()) {
    $compilerOutput = & $nsis /V2 @Defines $Path 2>&1
    if ($LASTEXITCODE -ne 0) { throw "NSIS compilation failed: $compilerOutput" }
    if ($compilerOutput -match 'unknown variable/constant') { throw "Unresolved NSIS symbol: $compilerOutput" }
}

function Run-Guard([string]$Executable, [string]$Receipt, [int]$ExpectedExit) {
    $process = Start-Process -FilePath $Executable -ArgumentList '/S' -PassThru -WindowStyle Hidden
    $ownedProcesses.Add($process)
    if (-not $process.WaitForExit(20000)) { throw 'Fixture guard exceeded its bounded deadline.' }
    if ($process.ExitCode -ne $ExpectedExit) {
        throw "Expected guard exit $ExpectedExit, got $($process.ExitCode)."
    }
    if ((Test-Path -LiteralPath $Receipt) -ne ($ExpectedExit -eq 0)) {
        throw 'Installer reached its file mutation section despite a rejected process guard.'
    }
    $script:scenarios++
}

try {
    # The probe is a disposable sleeping process, never the real desktop/runtime.
    $probeName = 'lgsm-probe-' + [guid]::NewGuid().ToString('N') + '.exe'
    $probeExe = Join-Path $fixtureRoot $probeName
    $readyPath = Join-Path $fixtureRoot 'probe-ready.txt'
    $probeSource = @'
Unicode true
RequestExecutionLevel user
SilentInstall silent
Name "LGSM disposable process probe"
OutFile "@PROBE@"
!include FileFunc.nsh
Section
  FileOpen $0 "@READY@" w
  FileWrite $0 "ready"
  FileClose $0
  ${GetParameters} $0
  ${GetOptions} $0 "/DURATION=" $1
  Sleep $1
SectionEnd
'@
    $probeFile = Write-Fixture 'probe.nsi' ($probeSource.Replace('@PROBE@', $probeExe).Replace('@READY@', $readyPath))
    Compile-Fixture $probeFile

    function Start-Probe([int]$Duration) {
        if (Test-Path -LiteralPath $readyPath) { Remove-Item -LiteralPath $readyPath }
        $process = Start-Process -FilePath $probeExe -ArgumentList "/DURATION=$Duration" -PassThru -WindowStyle Hidden
        $ownedProcesses.Add($process)
        $deadline = [DateTime]::UtcNow.AddSeconds(5)
        while (-not (Test-Path -LiteralPath $readyPath)) {
            if ($process.HasExited -or [DateTime]::UtcNow -gt $deadline) { throw 'Probe did not become ready.' }
            Start-Sleep -Milliseconds 20
        }
        return $process
    }

    $source = @'
Unicode true
RequestExecutionLevel user
SilentInstall silent
SilentUnInstall silent
Name "LGSM disposable maintenance guard"
!define PRODUCTNAME "LGSM disposable maintenance guard"
!define VERSION "0.1.0"
OutFile "${OUTPUT}"
!include MUI2.nsh
!include LogicLib.nsh
!addplugindir "@PLUGINS@"
; Tauri's template defines this macro before loading installerHooks.
!macro CheckIfAppIsRunning executableName productName
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
  StrCpy $PassiveMode 1
  StrCpy $UpdateMode ${UPDATE}
  !insertmacro CheckIfAppIsRunning "@PROBENAME@" "LGSM disposable process probe"
  FileOpen $0 "${RECEIPT}" w
  FileWrite $0 "install reached"
  FileClose $0
  WriteUninstaller "${UNINSTALLER}"
SectionEnd
Section "Uninstall"
  StrCpy $PassiveMode 1
  StrCpy $UpdateMode 0
  !insertmacro CheckIfAppIsRunning "@PROBENAME@" "LGSM disposable process probe"
  FileOpen $0 "${UNINSTALL_RECEIPT}" w
  FileWrite $0 "uninstall reached"
  FileClose $0
SectionEnd
'@
    $fixtureSource = $source.Replace('@PLUGINS@', $plugins).Replace('@HOOKS@', $hooks)
    $fixtureSource = $fixtureSource.Replace('@LANGUAGES@', $languages).Replace('@PROBENAME@', $probeName)
    $fixture = Write-Fixture 'guard.nsi' $fixtureSource
    $manual = Join-Path $fixtureRoot 'manual.exe'
    $update = Join-Path $fixtureRoot 'update.exe'
    $manualReceipt = Join-Path $fixtureRoot 'manual-reached.txt'
    $updateReceipt = Join-Path $fixtureRoot 'update-reached.txt'
    $uninstaller = Join-Path $fixtureRoot 'uninstall.exe'
    $uninstallReceipt = Join-Path $fixtureRoot 'uninstall-reached.txt'
    $common = @("/DUNINSTALLER=$uninstaller", "/DUNINSTALL_RECEIPT=$uninstallReceipt")
    Compile-Fixture $fixture ($common + @("/DOUTPUT=$manual", "/DRECEIPT=$manualReceipt", '/DUPDATE=0'))
    Compile-Fixture $fixture ($common + @("/DOUTPUT=$update", "/DRECEIPT=$updateReceipt", '/DUPDATE=1'))

    if ($CompileOnly) {
        Write-Output 'NSIS compilation passed for both languages and install/uninstall guards; execution was not requested.'
        return
    }

    Run-Guard $manual $manualReceipt 0
    Remove-Item -LiteralPath $manualReceipt
    $probe = Start-Probe 30000
    Run-Guard $manual $manualReceipt 10
    if ($probe.HasExited) { throw 'Manual install killed the running process.' }
    # _?= keeps this disposable uninstaller in-process so its exit is observable.
    $uninstallProcess = Start-Process -FilePath $uninstaller -ArgumentList @('/S', "_?=$fixtureRoot") -PassThru -WindowStyle Hidden
    $ownedProcesses.Add($uninstallProcess)
    if (-not $uninstallProcess.WaitForExit(5000) -or $uninstallProcess.ExitCode -ne 10 -or $probe.HasExited -or
        (Test-Path -LiteralPath $uninstallReceipt)) { throw 'Uninstall did not safely refuse the live probe.' }
    $scenarios++
    Run-Guard $update $updateReceipt 10
    if ($probe.HasExited) { throw 'Updater killed the running process after its wait budget.' }
    $probe.Kill()
    $probe.WaitForExit()
    $probe = Start-Probe 1400
    Run-Guard $update $updateReceipt 0
    if (-not $probe.HasExited) { throw 'Updater proceeded before the process exited.' }
    $uninstallProcess = Start-Process -FilePath $uninstaller -ArgumentList @('/S', "_?=$fixtureRoot") -PassThru -WindowStyle Hidden
    $ownedProcesses.Add($uninstallProcess)
    if (-not $uninstallProcess.WaitForExit(5000) -or $uninstallProcess.ExitCode -ne 0 -or
        -not (Test-Path -LiteralPath $uninstallReceipt)) { throw 'Idle uninstall did not proceed.' }
    $scenarios++
    Write-Output "NSIS maintenance guard: $scenarios scenarios passed; real application and data untouched."
}
finally {
    foreach ($process in $ownedProcesses) {
        if (-not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
        $process.Dispose()
    }
    $resolved = [System.IO.Path]::GetFullPath($fixtureRoot)
    if (-not $resolved.StartsWith($tempRoot.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase) -or
        (Split-Path -Leaf $resolved) -notlike 'lgsm-installer-guard-*') {
        throw 'Refusing cleanup outside the owned system-temp fixture.'
    }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
