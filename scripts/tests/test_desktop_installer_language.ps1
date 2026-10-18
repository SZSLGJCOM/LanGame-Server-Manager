[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$NsisRoot,
    [switch]$CompileOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$nsis = (Resolve-Path -LiteralPath (Join-Path $NsisRoot 'makensis.exe')).Path
$hooks = Join-Path $repoRoot 'apps/desktop/src-tauri/installer/hooks.nsh'
$languages = Join-Path $repoRoot 'apps/desktop/src-tauri/installer/languages'
$tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$fixtureId = [guid]::NewGuid().ToString('N')
$fixtureRoot = Join-Path $tempRoot "lgsm-installer-language-$fixtureId with spaces"
$productName = "LGSMInstallerLanguageTest-$fixtureId"
$registryPath = "Software\Entro Genesis\$productName"
$legacyRegistryPath = "Software\LanGame Team\$productName"
$defaultDirectory = Join-Path $fixtureRoot 'default app'
$oldDirectory = Join-Path $fixtureRoot 'previous custom app'
$newDirectory = Join-Path $fixtureRoot 'current custom app'
$explicitDirectory = Join-Path $fixtureRoot 'explicit app'
$null = New-Item -ItemType Directory -Path $fixtureRoot
$ownedProcesses = [System.Collections.Generic.List[System.Diagnostics.Process]]::new()
$registry = $null
$desktop = $null

# Start the disposable NSIS executable on an isolated desktop before its first
# window exists. No windows on the user's desktop are inspected or controlled.
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
public sealed class LgsmLanguageTestDesktop : IDisposable {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct StartupInfo {
        public int size;
        public string reserved, desktop, title;
        public int x, y, width, height, charsX, charsY, fill, flags;
        public short show, reservedLength;
        public IntPtr reservedData, input, output, error;
    }
    [StructLayout(LayoutKind.Sequential)]
    struct ProcessInfo {
        public IntPtr process, thread;
        public int processId, threadId;
    }
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern IntPtr CreateDesktop(string name, IntPtr device, IntPtr mode, int flags, uint access, IntPtr security);
    [DllImport("user32.dll", SetLastError = true)]
    static extern bool CloseDesktop(IntPtr desktop);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern bool CreateProcess(string application, StringBuilder command, IntPtr processSecurity,
        IntPtr threadSecurity, bool inherit, uint flags, IntPtr environment, string directory,
        ref StartupInfo startup, out ProcessInfo info);
    [DllImport("kernel32.dll")]
    static extern bool CloseHandle(IntPtr handle);
    readonly string name;
    IntPtr handle;
    public LgsmLanguageTestDesktop(string name) {
        this.name = name;
        handle = CreateDesktop(name, IntPtr.Zero, IntPtr.Zero, 0, 0x10000000, IntPtr.Zero);
        if (handle == IntPtr.Zero) throw new Win32Exception();
    }
    public Process Start(string executable, string arguments) {
        var startup = new StartupInfo { size = Marshal.SizeOf(typeof(StartupInfo)), desktop = name, flags = 1, show = 0 };
        ProcessInfo info;
        if (!CreateProcess(executable, new StringBuilder("\"" + executable + "\" " + arguments),
            IntPtr.Zero, IntPtr.Zero, false, 0, IntPtr.Zero, null, ref startup, out info)) throw new Win32Exception();
        try {
            var process = Process.GetProcessById(info.processId);
            // Retain an owned handle before closing CreateProcess's handles, so
            // the exit code remains available even when the probe exits quickly.
            var retainedHandle = process.Handle;
            return process;
        }
        finally { CloseHandle(info.thread); CloseHandle(info.process); }
    }
    public void Dispose() {
        if (handle != IntPtr.Zero) { CloseDesktop(handle); handle = IntPtr.Zero; }
    }
}
'@

try {
    $executable = Join-Path $fixtureRoot 'language.exe'
    $before = Join-Path $fixtureRoot 'before.txt'
    $after = Join-Path $fixtureRoot 'after.txt'
    $location = Join-Path $fixtureRoot 'location.txt'
    $installed = Join-Path $fixtureRoot 'installed.txt'
    # Match Tauri's initialization order, but never install anything. A private
    # desktop keeps the real LangDLL dialog away from the user's desktop.
    $source = @'
Unicode true
RequestExecutionLevel user
Name "LGSM disposable language probe"
OutFile "@EXE@"
!include MUI2.nsh
!include FileFunc.nsh
!macro CheckIfAppIsRunning executablePath productName
!macroend
!include "@HOOKS@"
; Like Tauri, define the product after including hooks, before expanding them.
!define PRODUCTNAME "@PRODUCT@"
!define VERSION "0.1.0"
!define MAINBINARYNAME "@PRODUCT@"
!define MANUFACTURER "Entro Genesis"
!define MANUKEY "Software\${MANUFACTURER}"
!define MANUPRODUCTKEY "${MANUKEY}\${PRODUCTNAME}"
!define PLACEHOLDER_INSTALL_DIR "placeholder\${PRODUCTNAME}"
InstallDir "${PLACEHOLDER_INSTALL_DIR}"
Var PassiveMode
Var UpdateMode
Var MigrationProbe
!define MUI_LANGDLL_REGISTRY_ROOT "HKCU"
!define MUI_LANGDLL_REGISTRY_KEY "${MANUPRODUCTKEY}"
!define MUI_LANGDLL_REGISTRY_VALUENAME "Installer Language"
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "SimpChinese"
!include "@LANGUAGES@\English.nsh"
!include "@LANGUAGES@\SimpChinese.nsh"
Function .onInit
  ${GetOptions} $CMDLINE "/P" $PassiveMode
  ${IfNot} ${Errors}
    StrCpy $PassiveMode 1
  ${EndIf}
  ${GetOptions} $CMDLINE "/UPDATE" $UpdateMode
  ${IfNot} ${Errors}
    StrCpy $UpdateMode 1
  ${EndIf}
  FileOpen $0 "@BEFORE@" w
  FileWrite $0 "$LANGUAGE"
  FileClose $0
  !insertmacro MUI_LANGDLL_DISPLAY
  FileOpen $0 "@AFTER@" w
  FileWrite $0 "$LANGUAGE"
  FileClose $0
  ${GetOptions} $CMDLINE "/MIGRATION" $MigrationProbe
  ${If} ${Errors}
    SetErrorLevel 0
    Quit
  ${EndIf}
  ; Match current-user x64 Tauri initialization: language precedes SetContext,
  ; and /D already sets INSTDIR so restoration only replaces the placeholder.
  SetShellVarContext current
  SetRegView 64
  ${If} $INSTDIR == "${PLACEHOLDER_INSTALL_DIR}"
    StrCpy $INSTDIR "@DEFAULT@"
    ReadRegStr $4 SHCTX "${MANUPRODUCTKEY}" ""
    ${If} $4 != ""
      StrCpy $INSTDIR $4
    ${EndIf}
  ${EndIf}
  FileOpen $0 "@LOCATION@" w
  FileWrite $0 "$INSTDIR"
  FileClose $0
  ${GetOptions} $CMDLINE "/CANCEL" $0
  ${IfNot} ${Errors}
    SetErrorLevel 23
    Abort
  ${EndIf}
  ; Manual language initialization has already run; the remaining fixture has
  ; no wizard pages or product payload and can complete without interaction.
  SetSilent silent
FunctionEnd
Section
  ${GetOptions} $CMDLINE "/FAIL" $0
  ${IfNot} ${Errors}
    SetErrorLevel 24
    Abort
  ${EndIf}
  ; Tauri stores these settings before its successful post-install hook.
  WriteRegStr SHCTX "${MANUPRODUCTKEY}" "" $INSTDIR
  WriteRegStr HKCU "${MANUPRODUCTKEY}" "Installer Language" $LANGUAGE
  !insertmacro NSIS_HOOK_POSTINSTALL
  FileOpen $0 "@INSTALLED@" w
  FileWrite $0 "completed"
  FileClose $0
SectionEnd
'@
    $source = $source.Replace('@EXE@', $executable).Replace('@HOOKS@', $hooks)
    $source = $source.Replace('@PRODUCT@', $productName).Replace('@LANGUAGES@', $languages)
    $source = $source.Replace('@BEFORE@', $before).Replace('@AFTER@', $after)
    $source = $source.Replace('@LOCATION@', $location).Replace('@INSTALLED@', $installed)
    $source = $source.Replace('@DEFAULT@', $defaultDirectory)
    $sourcePath = Join-Path $fixtureRoot 'language.nsi'
    [System.IO.File]::WriteAllText($sourcePath, $source, [System.Text.UTF8Encoding]::new($false))
    $compilerOutput = & $nsis /V2 $sourcePath 2>&1
    if ($LASTEXITCODE -ne 0 -or $compilerOutput -match 'unknown variable/constant') {
        throw "NSIS language fixture compilation failed: $compilerOutput"
    }
    if ($CompileOnly) {
        Write-Output 'NSIS language and publisher-settings fixture compiled; runtime behavior was not tested.'
        return
    }
    $registry = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($registryPath)
    $desktop = [LgsmLanguageTestDesktop]::new("LGSMTest-$fixtureId")

    function Test-Language([string]$Name, [string[]]$Arguments, [string]$SavedLanguage, [bool]$ExpectsDialog) {
        foreach ($path in @($before, $after)) {
            if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path }
        }
        $registry.DeleteValue('Installer Language', $false)
        if ($SavedLanguage) { $registry.SetValue('Installer Language', $SavedLanguage) }
        $process = $desktop.Start($executable, ($Arguments -join ' '))
        $ownedProcesses.Add($process)
        $deadline = [DateTime]::UtcNow.AddSeconds(10)
        while (-not (Test-Path -LiteralPath $before)) {
            if ($process.HasExited -or [DateTime]::UtcNow -gt $deadline) {
                throw "$Name did not reach the language macro (exit $($process.ExitCode))."
            }
            Start-Sleep -Milliseconds 20
        }
        if ($ExpectsDialog) {
            if ($process.WaitForExit(1000) -or (Test-Path -LiteralPath $after)) {
                throw "$Name skipped the manual language selection."
            }
            # The only operation between the receipts is the real language macro.
            # End this owned probe without interacting with its private dialog.
            $process.Kill()
            $process.WaitForExit()
        } else {
            if (-not $process.WaitForExit(5000)) { throw "$Name blocked in the language macro." }
            if ($process.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $after)) {
                throw "$Name did not complete language initialization (exit $($process.ExitCode), receipt $(Test-Path -LiteralPath $after))."
            }
            $expected = if ($SavedLanguage) { $SavedLanguage } else { [System.IO.File]::ReadAllText($before) }
            if ([System.IO.File]::ReadAllText($after) -ne $expected) { throw "$Name changed the selected language." }
        }
        if ($registry.GetValue('Installer Language') -ne $(if ($SavedLanguage) { $SavedLanguage } else { $null })) {
            throw "$Name changed the saved language during initialization."
        }
        Write-Output "PASS: $Name"
    }

    Test-Language 'old updater arguments without a saved language' @('/P', '/UPDATE', '/R', '/ARGS') '' $false
    Test-Language 'passive installation without a saved language' @('/P') '' $false
    Test-Language 'silent installation without a saved language' @('/S') '' $false
    Test-Language 'manual installation without a saved language' @() '' $true
    foreach ($language in @('1033', '2052')) {
        Test-Language "updater retains saved language $language" @('/P', '/UPDATE', '/R', '/ARGS') $language $false
        Test-Language "manual installation retains saved language $language" @() $language $false
    }

    function Assert-RegistryValues([string]$Path, [hashtable]$Expected, [string]$Name) {
        $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($Path)
        try {
            if ($Expected.Count -eq 0) {
                if ($null -ne $key) { throw "$Name left an obsolete product key." }
                return
            }
            if ($null -eq $key -or $key.GetValueNames().Count -ne $Expected.Count) {
                throw "$Name changed the expected registry value set."
            }
            foreach ($value in $Expected.Keys) {
                if ($key.GetValue($value) -cne $Expected[$value]) {
                    throw "$Name changed registry value '$value'."
                }
            }
        }
        finally { if ($null -ne $key) { $key.Dispose() } }
    }

    function Test-PublisherSettings {
        param(
            [string]$Name,
            [string[]]$Arguments,
            [hashtable]$OldValues,
            [hashtable]$NewValues,
            [string]$ExpectedDirectory,
            [string]$ExpectedLanguage,
            [hashtable]$ExpectedOldValues,
            [int]$ExpectedExit = 0,
            [switch]$Repeat
        )
        foreach ($value in $registry.GetValueNames()) { $registry.DeleteValue($value) }
        [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree($legacyRegistryPath, $false)
        foreach ($value in $NewValues.Keys) { $registry.SetValue($value, $NewValues[$value]) }
        if ($OldValues.Count -gt 0) {
            $oldKey = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($legacyRegistryPath)
            try {
                foreach ($value in $OldValues.Keys) { $oldKey.SetValue($value, $OldValues[$value]) }
            }
            finally { $oldKey.Dispose() }
        }
        $runs = if ($Repeat) { 2 } else { 1 }
        for ($run = 0; $run -lt $runs; $run++) {
            foreach ($path in @($before, $after, $location, $installed)) {
                if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path }
            }
            # /D must remain last and unquoted, as required by NSIS.
            $process = $desktop.Start($executable, '/MIGRATION ' + ($Arguments -join ' '))
            $ownedProcesses.Add($process)
            if (-not $process.WaitForExit(10000)) { throw "$Name blocked during publisher-settings migration." }
            if ($process.ExitCode -ne $ExpectedExit -or -not (Test-Path -LiteralPath $after) -or
                -not (Test-Path -LiteralPath $location)) {
                throw "$Name did not complete the expected initialization (exit $($process.ExitCode))."
            }
            $language = if ($ExpectedLanguage) { $ExpectedLanguage } else { [IO.File]::ReadAllText($before) }
            if ([IO.File]::ReadAllText($after) -cne $language -or
                [IO.File]::ReadAllText($location) -cne $ExpectedDirectory) {
                throw "$Name changed the expected language or installation directory."
            }
            if ((Test-Path -LiteralPath $installed) -ne ($ExpectedExit -eq 0)) {
                throw "$Name reached post-install completion despite cancellation or failure."
            }
            $expectedNew = @{ '' = $ExpectedDirectory; 'Installer Language' = $language }
            foreach ($value in $NewValues.Keys) {
                if ($value -ne '' -and $value -ne 'Installer Language') { $expectedNew[$value] = $NewValues[$value] }
            }
            Assert-RegistryValues $registryPath $expectedNew $Name
            Assert-RegistryValues $legacyRegistryPath $ExpectedOldValues $Name
        }
        Write-Output "PASS: $Name"
    }

    $old = @{ '' = $oldDirectory; 'Installer Language' = '2052' }
    $current = @{ '' = $newDirectory; 'Installer Language' = '1033' }
    Test-PublisherSettings 'updater migrates old settings and preserves unrelated values' @('/P', '/UPDATE', '/R', '/ARGS') `
        (@{ 'Keep' = 'old sentinel' } + $old) @{ 'Keep' = 'new sentinel' } $oldDirectory '2052' @{ 'Keep' = 'old sentinel' }
    Test-PublisherSettings 'manual installation migrates old directory and language' @() $old @{} $oldDirectory '2052' @{}
    Test-PublisherSettings 'fresh installation without old settings is repeatable' @('/P') @{} @{} $defaultDirectory '' @{} -Repeat
    Test-PublisherSettings 'new publisher settings take priority' @('/P', '/UPDATE') $old $current $newDirectory '1033' $old
    Test-PublisherSettings 'new directory wins while missing language is restored' @('/P') $old @{ '' = $newDirectory } $newDirectory '2052' $old
    Test-PublisherSettings 'new language wins while missing directory is restored' @('/P') $old @{ 'Installer Language' = '1033' } $oldDirectory '1033' @{}
    Test-PublisherSettings 'explicit directory overrides restored settings' @('/P', "/D=$explicitDirectory") $old @{} $explicitDirectory '2052' $old
    Test-PublisherSettings 'unidentified old directory retains its language record' @('/P') @{ 'Installer Language' = '2052' } @{} $defaultDirectory '2052' @{ 'Installer Language' = '2052' }
    Test-PublisherSettings 'old directory without a language migrates' @('/P') @{ '' = $oldDirectory } @{} $oldDirectory '' @{}
    Test-PublisherSettings 'initialization cancellation preserves old settings' @('/P', '/CANCEL') $old @{} $oldDirectory '2052' $old -ExpectedExit 23
    Test-PublisherSettings 'installation failure preserves old settings' @('/P', '/FAIL') $old @{} $oldDirectory '2052' $old -ExpectedExit 24
    Write-Output 'NSIS initialization: 8 language scenarios and 11 publisher-settings scenarios passed; real product keys and data untouched.'
}
finally {
    foreach ($process in $ownedProcesses) {
        if (-not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
        $process.Dispose()
    }
    if ($null -ne $desktop) { $desktop.Dispose() }
    if ($null -ne $registry) {
        $registry.Dispose()
        [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree($registryPath, $false)
        [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree($legacyRegistryPath, $false)
    }
    $resolved = [System.IO.Path]::GetFullPath($fixtureRoot)
    if (-not $resolved.StartsWith($tempRoot.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase) -or
        (Split-Path -Leaf $resolved) -notlike 'lgsm-installer-language-*') {
        throw 'Refusing cleanup outside the owned system-temp fixture.'
    }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
