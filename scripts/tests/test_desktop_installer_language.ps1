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
$tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$fixtureId = [guid]::NewGuid().ToString('N')
$fixtureRoot = Join-Path $tempRoot "lgsm-installer-language-$fixtureId with spaces"
$registryPath = "Software\LGSMInstallerLanguageTest-$fixtureId"
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
Var PassiveMode
Var UpdateMode
!define MUI_LANGDLL_REGISTRY_ROOT "HKCU"
!define MUI_LANGDLL_REGISTRY_KEY "@KEY@"
!define MUI_LANGDLL_REGISTRY_VALUENAME "Installer Language"
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "SimpChinese"
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
  SetErrorLevel 0
  Quit
FunctionEnd
Section
SectionEnd
'@
    $source = $source.Replace('@EXE@', $executable).Replace('@HOOKS@', $hooks)
    $source = $source.Replace('@KEY@', $registryPath)
    $source = $source.Replace('@BEFORE@', $before).Replace('@AFTER@', $after)
    $sourcePath = Join-Path $fixtureRoot 'language.nsi'
    [System.IO.File]::WriteAllText($sourcePath, $source, [System.Text.UTF8Encoding]::new($false))
    $compilerOutput = & $nsis /V2 $sourcePath 2>&1
    if ($LASTEXITCODE -ne 0 -or $compilerOutput -match 'unknown variable/constant') {
        throw "NSIS language fixture compilation failed: $compilerOutput"
    }
    if ($CompileOnly) {
        Write-Output 'NSIS language fixture compiled; runtime behavior was not tested.'
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
    Write-Output 'NSIS language initialization: 8 scenarios passed; application registry and data untouched.'
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
    }
    $resolved = [System.IO.Path]::GetFullPath($fixtureRoot)
    if (-not $resolved.StartsWith($tempRoot.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase) -or
        (Split-Path -Leaf $resolved) -notlike 'lgsm-installer-language-*') {
        throw 'Refusing cleanup outside the owned system-temp fixture.'
    }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
