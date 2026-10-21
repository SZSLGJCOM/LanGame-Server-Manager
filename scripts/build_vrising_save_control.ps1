param([Parameter(Mandatory = $true)][string]$OutDir, [switch]$Test)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
foreach ($moduleName in @('Microsoft.PowerShell.Management', 'Microsoft.PowerShell.Utility')) {
    Import-Module -Name ([IO.Path]::Combine($PSHOME, 'Modules', $moduleName, $moduleName + '.psd1')) -ErrorAction Stop
}
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $PSScriptRoot 'build_control_output.ps1')
$output = Resolve-ControlBuildOutput -OutDir $OutDir -ProjectRoot $projectRoot -CargoLeaf 'vrising-save-control'
$source = Join-Path $projectRoot 'modules/vrising/control'
$vendor = Join-Path $projectRoot 'modules/enshrouded/control/vendor/minhook'
$manifest = Get-Content -LiteralPath (Join-Path $vendor 'SOURCE-MANIFEST.json') -Raw | ConvertFrom-Json
foreach ($file in $manifest.files) {
    if ((Get-FileHash -LiteralPath (Join-Path $vendor $file.path) -Algorithm SHA256).Hash.ToLowerInvariant() -ne $file.sha256) {
        throw ('Reviewed MinHook source changed: ' + $file.path)
    }
}
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$installation = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if ($LASTEXITCODE -ne 0 -or !$installation) { throw 'MSVC x64 Build Tools are unavailable.' }
$vcvars = Join-Path $installation 'VC/Auxiliary/Build/vcvars64.bat'
foreach ($path in @($output, $source, $vendor, $vcvars)) {
    if ($path.IndexOfAny([char[]]'%!&|<>^"') -ge 0 -or $path -match '[\x00-\x1f]') { throw 'Unsupported MSVC build path character.' }
}
[IO.Directory]::CreateDirectory($output) | Out-Null
Assert-PlainControlBuildOutput $output
$commands = @('@echo off', 'setlocal DisableDelayedExpansion', ('call "'+$vcvars+'"'), 'if errorlevel 1 exit /b 1',
    ('cd /d "'+$output+'"'),
    ('cl /nologo /c /O2 /W4 /MT /TC /I"'+$vendor+'/include" "'+$vendor+'/src/buffer.c" "'+$vendor+'/src/hook.c" "'+$vendor+'/src/trampoline.c" "'+$vendor+'/src/hde/hde64.c"'),
    'if errorlevel 1 exit /b 1')
$cpp = 'cl /nologo /std:c++20 /O2 /W4 /WX /MT /EHsc /utf-8'
$objects = 'buffer.obj hook.obj trampoline.obj hde64.obj'
$commands += @(
    ($cpp+' /LD "'+$source+'/native_bridge.cpp" "'+$source+'/native_unwind.cpp" '+$objects+' /FeLgsmVRisingSaveControl.dll /link bcrypt.lib /DYNAMICBASE /NXCOMPAT'),
    'if errorlevel 1 exit /b 1')
if ($Test) {
    $commands += @(
        ($cpp+' "'+$source+'/tests/native_completion_tests.cpp" /FeVRisingCompletionTests.exe'),
        'if errorlevel 1 exit /b 1', 'VRisingCompletionTests.exe', 'if errorlevel 1 exit /b 1',
        ('ml64 /nologo /c /Fonative_probe.obj "'+$source+'/tests/native_probe.asm"'),
        'if errorlevel 1 exit /b 1',
        ($cpp+' "'+$source+'/tests/native_unwind_tests.cpp" "'+$source+'/native_unwind.cpp" native_probe.obj '+$objects+' /FeVRisingUnwindTests.exe'),
        'if errorlevel 1 exit /b 1', 'VRisingUnwindTests.exe', 'if errorlevel 1 exit /b 1')
}
$commands += 'exit /b 0'
$batch = Join-Path $output 'build-vrising-save.cmd'
[IO.File]::WriteAllLines($batch, $commands, [Text.UTF8Encoding]::new($false))
& $env:ComSpec /d /c $batch
if ($LASTEXITCODE -ne 0) { throw 'V Rising native save observer build or checks failed.' }
Copy-Item -LiteralPath (Join-Path $vendor 'LICENSE.txt') -Destination (Join-Path $output 'LICENSE.minhook.txt')
Write-Output (Join-Path $output 'LgsmVRisingSaveControl.dll')
