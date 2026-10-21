param([Parameter(Mandatory = $true)][string]$OutDir, [switch]$RunTests)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
foreach ($moduleName in @('Microsoft.PowerShell.Management', 'Microsoft.PowerShell.Utility')) {
    Import-Module -Name ([IO.Path]::Combine($PSHOME, 'Modules', $moduleName, $moduleName + '.psd1')) -ErrorAction Stop
}
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $PSScriptRoot 'build_control_output.ps1')
$output = Resolve-ControlBuildOutput -OutDir $OutDir -ProjectRoot $projectRoot -CargoLeaf 'windrose-save-control'
$source = Join-Path $projectRoot 'modules/windrose/control/bridge.cpp'
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
if (-not (Test-Path -LiteralPath $vswhere -PathType Leaf)) { throw 'MSVC C++ Build Tools are required to build the bundled Windrose control.' }
$installation = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if ($LASTEXITCODE -ne 0 -or -not $installation) { throw 'MSVC x64 toolchain was not found.' }
$vcvars = Join-Path $installation 'VC/Auxiliary/Build/vcvars64.bat'
foreach ($path in @($source, $vcvars)) {
    if ($path.IndexOfAny([char[]]'%!&|<>^"') -ge 0 -or $path.Contains("`n") -or $path.Contains("`r")) { throw 'Unsupported MSVC build path characters.' }
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw 'The native source or compiler setup is missing.' }
}
[IO.Directory]::CreateDirectory($output) | Out-Null
Assert-PlainControlBuildOutput $output
$commands = @('@echo off', 'setlocal DisableDelayedExpansion', ('call "{0}"' -f $vcvars), 'if errorlevel 1 exit /b 1',
    ('cd /d "{0}"' -f $output),
    ('cl /nologo /std:c++20 /O2 /W4 /WX /MT /EHsc /utf-8 /LD "{0}" /FeLgsmWindroseSaveControl.dll /link bcrypt.lib /DYNAMICBASE /NXCOMPAT' -f $source),
    'if errorlevel 1 exit /b 1', 'exit /b 0')
if ($RunTests) {
    $tests = Join-Path $projectRoot 'modules/windrose/control/tests/bridge_tests.cpp'
    $commands = $commands[0..($commands.Length - 2)] + @(
        ('cl /nologo /std:c++20 /W4 /WX /MT /EHsc /utf-8 "{0}" /Febridge_tests.exe /link bcrypt.lib' -f $tests),
        'if errorlevel 1 exit /b 1', 'bridge_tests.exe', 'if errorlevel 1 exit /b 1', 'exit /b 0')
}
$batch = Join-Path $output 'build-windrose-control.cmd'
[IO.File]::WriteAllLines($batch, $commands, [Text.UTF8Encoding]::new($false))
& $env:ComSpec /d /c $batch
if ($LASTEXITCODE -ne 0) { throw 'Windrose save control build failed.' }
Write-Output (Join-Path $output 'LgsmWindroseSaveControl.dll')
