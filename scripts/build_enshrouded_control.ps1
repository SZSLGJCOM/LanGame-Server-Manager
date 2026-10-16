param(
    [Parameter(Mandatory = $true)][string]$OutDir,
    [switch]$RunTests
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($OutDir -notmatch '^(?:[A-Za-z]:[\\/]|[\\/]{2}[^\\/\r\n]+[\\/][^\\/\r\n]+(?:[\\/]|$))') {
    throw 'Enshrouded control requires an absolute drive-root or UNC OutDir.'
}
$output = [IO.Path]::GetFullPath($OutDir)
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if ($output.Equals($projectRoot, [StringComparison]::OrdinalIgnoreCase) -or
    $output.StartsWith($projectRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Enshrouded control build output must be outside the repository.'
}
$source = Join-Path $projectRoot 'modules/enshrouded/control'
foreach ($path in @($output, $source)) {
    if ($path.IndexOfAny([char[]]'%!&|<>^"') -ge 0 -or $path.Contains("`n") -or $path.Contains("`r")) {
        throw 'MSVC build paths contain unsupported command characters.'
    }
}
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
if (-not (Test-Path -LiteralPath $vswhere -PathType Leaf)) { throw 'MSVC C++ Build Tools are required.' }
$installation = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if ($LASTEXITCODE -ne 0 -or -not $installation) { throw 'MSVC x64 toolchain was not found.' }
$vcvars = Join-Path $installation 'VC/Auxiliary/Build/vcvars64.bat'
if (-not (Test-Path -LiteralPath $vcvars -PathType Leaf)) { throw 'MSVC vcvars64.bat was not found.' }
[IO.Directory]::CreateDirectory($output) | Out-Null
$commands = [Collections.Generic.List[string]]::new()
$commands.Add('@echo off')
$commands.Add('setlocal DisableDelayedExpansion')
$commands.Add(('call "{0}"' -f $vcvars))
$commands.Add('if errorlevel 1 exit /b 1')
$commands.Add(('cd /d "{0}"' -f $output))
$vendor = Join-Path $source 'vendor/minhook'
$commands.Add(('cl /nologo /c /O2 /W4 /MT /TC /I"{0}/include" "{0}/src/buffer.c" "{0}/src/hook.c" "{0}/src/trampoline.c" "{0}/src/hde/hde64.c"' -f $vendor))
$commands.Add('if errorlevel 1 exit /b 1')
foreach ($assembly in @('update_observer', 'copy_observer')) {
    $commands.Add(('ml64 /nologo /c /Fo{0}.obj "{1}/{0}.asm"' -f $assembly, $source))
    $commands.Add('if errorlevel 1 exit /b 1')
}
$cpp = 'cl /nologo /std:c++20 /O2 /W4 /WX /MT /EHsc /utf-8'
if ($RunTests) {
    $commands.Add(('ml64 /nologo /c /Foupdate_probe.obj "{0}/tests/update_probe.asm"' -f $source))
    $commands.Add('if errorlevel 1 exit /b 1')
    $commands.Add(('{0} "{1}/tests/update_abi_tests.cpp" update_observer.obj update_probe.obj buffer.obj hook.obj trampoline.obj hde64.obj /Feupdate_abi_tests.exe' -f $cpp, $source))
    $commands.Add('if errorlevel 1 exit /b 1')
    $commands.Add('update_abi_tests.exe')
    $commands.Add('if errorlevel 1 exit /b 1')
    $commands.Add(('{0} "{1}/tests/update_unwind_tests.cpp" update_observer.obj update_probe.obj /Feupdate_unwind_tests.exe' -f $cpp, $source))
    $commands.Add('if errorlevel 1 exit /b 1')
    $commands.Add('update_unwind_tests.exe')
    $commands.Add('if errorlevel 1 exit /b 1')
    $commands.Add(('ml64 /nologo /c /Focopy_probe.obj "{0}/tests/copy_probe.asm"' -f $source))
    $commands.Add('if errorlevel 1 exit /b 1')
    $commands.Add(('{0} "{1}/tests/copy_abi_tests.cpp" copy_observer.obj copy_probe.obj buffer.obj hook.obj trampoline.obj hde64.obj /Fecopy_abi_tests.exe' -f $cpp, $source))
    $commands.Add('if errorlevel 1 exit /b 1')
    $commands.Add('copy_abi_tests.exe')
    $commands.Add('if errorlevel 1 exit /b 1')
    $commands.Add(('{0} "{1}/tests/control_tests.cpp" "{1}/control_protocol.cpp" /Fecontrol_tests.exe' -f $cpp, $source))
    $commands.Add('if errorlevel 1 exit /b 1')
    $commands.Add('control_tests.exe')
    $commands.Add('if errorlevel 1 exit /b 1')
    $commands.Add(('ml64 /nologo /c /Fotrampoline_probe.obj "{0}/tests/trampoline_probe.asm"' -f $source))
    $commands.Add('if errorlevel 1 exit /b 1')
    $commands.Add(('{0} "{1}/tests/trampoline_unwind_tests.cpp" "{1}/trampoline_unwind.cpp" trampoline_probe.obj buffer.obj hook.obj trampoline.obj hde64.obj /Fetrampoline_unwind_tests.exe' -f $cpp, $source))
    $commands.Add('if errorlevel 1 exit /b 1')
    $commands.Add('trampoline_unwind_tests.exe')
    $commands.Add('if errorlevel 1 exit /b 1')
}
$commands.Add(('{0} /LD "{1}/bridge.cpp" "{1}/native_profile.cpp" "{1}/control_protocol.cpp" "{1}/control_files.cpp" "{1}/trampoline_unwind.cpp" update_observer.obj copy_observer.obj buffer.obj hook.obj trampoline.obj hde64.obj /Fedbghelp.dll /link bcrypt.lib /DYNAMICBASE /NXCOMPAT' -f $cpp, $source))
$commands.Add('if errorlevel 1 exit /b 1')
$commands.Add('exit /b 0')
$batch = Join-Path $output 'build-control.cmd'
[IO.File]::WriteAllLines($batch, $commands, [Text.UTF8Encoding]::new($false))
& $env:ComSpec /d /c $batch
if ($LASTEXITCODE -ne 0) { throw 'Enshrouded control build or tests failed.' }
Copy-Item -LiteralPath (Join-Path $vendor 'LICENSE.txt') -Destination (Join-Path $output 'LICENSE.minhook.txt')
Write-Output (Join-Path $output 'dbghelp.dll')
