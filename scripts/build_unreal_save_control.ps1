param(
    [Parameter(Mandatory = $true)][string]$OutDir,
    [Alias('RunTests')][switch]$CompileTests,
    [switch]$WarningAsError = $true
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
foreach ($moduleName in @('Microsoft.PowerShell.Management', 'Microsoft.PowerShell.Utility')) {
    Import-Module -Name ([IO.Path]::Combine($PSHOME, 'Modules', $moduleName, $moduleName + '.psd1')) -ErrorAction Stop
}
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $PSScriptRoot 'build_control_output.ps1')
$output = Resolve-ControlBuildOutput -OutDir $OutDir -ProjectRoot $projectRoot -CargoLeaf 'native-world-save'
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
if (-not (Test-Path -LiteralPath $vswhere -PathType Leaf)) { throw 'MSVC C++ Build Tools are required for native world-save adapters.' }
$installation = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if ($LASTEXITCODE -ne 0 -or -not $installation) { throw 'MSVC x64 toolchain was not found.' }
$vcvars = Join-Path $installation 'VC/Auxiliary/Build/vcvars64.bat'
$games = @(
    @{ Module = 'nightingale'; Name = 'LgsmNightingaleSaveControl' },
    @{ Module = 'conanexiles'; Name = 'LgsmConanSaveControl' }
)
$paths = @($output, $projectRoot, $vcvars)
if ($CompileTests) { $paths += Join-Path $projectRoot 'apps/desktop/src-tauri/native/tests/world_save_validation_tests.cpp' }
foreach ($game in $games) {
    $paths += Join-Path $projectRoot ('modules/' + $game.Module + '/control/bridge.cpp')
    if ($CompileTests) { $paths += Join-Path $projectRoot ('modules/' + $game.Module + '/control/tests/bridge_tests.cpp') }
}
foreach ($path in $paths) {
    if ($path.IndexOfAny([char[]]'%!&|<>^"') -ge 0 -or $path.Contains("`n") -or $path.Contains("`r")) { throw 'Unsupported MSVC build path characters.' }
}
foreach ($path in $paths[2..($paths.Count - 1)]) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw ('Native build input is missing: ' + $path) }
}
[IO.Directory]::CreateDirectory($output) | Out-Null
Assert-PlainControlBuildOutput $output
$warnings = if ($WarningAsError) { '/WX' } else { '' }
$commands = @('@echo off', 'setlocal DisableDelayedExpansion', ('call "{0}"' -f $vcvars), 'if errorlevel 1 exit /b 1', ('cd /d "{0}"' -f $output))
foreach ($game in $games) {
    $source = Join-Path $projectRoot ('modules/' + $game.Module + '/control/bridge.cpp')
    $commands += 'cl /nologo /std:c++20 /O2 /W4 {0} /MT /EHsc /utf-8 /LD "{1}" /Fo{2}.obj /Fe{2}.dll /link bcrypt.lib /DYNAMICBASE /NXCOMPAT' -f $warnings, $source, $game.Name
    $commands += 'if errorlevel 1 exit /b 1'
    if ($CompileTests) {
        $tests = Join-Path $projectRoot ('modules/' + $game.Module + '/control/tests/bridge_tests.cpp')
        $commands += 'cl /nologo /std:c++20 /W4 {0} /MT /EHsc /utf-8 "{1}" /Fo{2}.tests.obj /Fe{2}.tests.exe /link bcrypt.lib' -f $warnings, $tests, $game.Name
        $commands += @('if errorlevel 1 exit /b 1', ($game.Name + '.tests.exe'), 'if errorlevel 1 exit /b 1')
    }
}
if ($CompileTests) {
    $validation = Join-Path $projectRoot 'apps/desktop/src-tauri/native/tests/world_save_validation_tests.cpp'
    $fixture = Join-Path $output ('WorldSaveValidationFixture-'+[guid]::NewGuid().ToString('N'))
    if (Test-Path -LiteralPath $fixture) { throw 'Native validation fixture already exists.' }
    [IO.Directory]::CreateDirectory($fixture) | Out-Null
    $commands += 'cl /nologo /std:c++20 /O2 /W4 {0} /MT /EHsc /utf-8 "{1}" /FoWorldSaveValidationTests.obj /FeWorldSaveValidationTests.exe /link bcrypt.lib' -f $warnings, $validation
    $commands += @('if errorlevel 1 exit /b 1', ('WorldSaveValidationTests.exe "{0}"' -f $fixture), 'if errorlevel 1 exit /b 1')
}
$commands += 'exit /b 0'
$batch = Join-Path $output 'build-native-world-save.cmd'
[IO.File]::WriteAllLines($batch, $commands, [Text.UTF8Encoding]::new($false))
& $env:ComSpec /d /c $batch
if ($LASTEXITCODE -ne 0) { throw 'Native world-save adapter build or contract checks failed.' }
foreach ($game in $games) { Write-Output (Join-Path $output ($game.Name + '.dll')) }
