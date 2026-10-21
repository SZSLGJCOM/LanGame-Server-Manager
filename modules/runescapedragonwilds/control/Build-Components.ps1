param([Parameter(Mandatory)][string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$source = [IO.Path]::GetFullPath($PSScriptRoot)
$repository = [IO.Path]::GetFullPath((Join-Path $source '../../..'))
$output = [IO.Path]::GetFullPath($OutputDirectory)
foreach ($protected in @($repository,[IO.Path]::GetFullPath('D:/LanGame/server-files'))) {
    if ($output -eq $protected -or $output.StartsWith($protected+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)) {
        throw 'Dragonwilds component output must be outside source and original server packages.'
    }
}
[IO.Directory]::CreateDirectory($output) | Out-Null
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$vs = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if ($LASTEXITCODE -ne 0 -or !$vs) { throw 'MSVC C++ Build Tools unavailable.' }
$msvc = (Get-ChildItem -LiteralPath (Join-Path $vs 'VC/Tools/MSVC') -Directory | Sort-Object {[version]$_.Name} -Descending | Select-Object -First 1).FullName
$bin = Join-Path $msvc 'bin/Hostx64/x64'
$windows = (Get-ItemProperty 'HKLM:/SOFTWARE/Microsoft/Windows Kits/Installed Roots').KitsRoot10
$version = (Get-ChildItem -LiteralPath (Join-Path $windows 'Include') -Directory | Where-Object {Test-Path -LiteralPath (Join-Path $_.FullName 'um/Windows.h')} | Sort-Object {[version]$_.Name} -Descending | Select-Object -First 1).Name
$previousInclude,$previousLib,$previousPath = $env:INCLUDE,$env:LIB,$env:PATH
try {
    $env:INCLUDE = (@((Join-Path $msvc 'include'))+(@('ucrt','shared','um','winrt') | ForEach-Object {Join-Path $windows "Include/$version/$_"})) -join ';'
    $env:LIB = @((Join-Path $msvc 'lib/x64'),(Join-Path $windows "Lib/$version/ucrt/x64"),(Join-Path $windows "Lib/$version/um/x64")) -join ';'
    $env:PATH = "$bin;$previousPath"
    $compile = @('/nologo','/std:c++20','/EHsc','/MT','/O2','/utf-8','/DNOMINMAX','/W4','/WX','/DUNICODE','/D_UNICODE')
    foreach ($unit in @('save_native','save_sinks','save_profile')) {
        & (Join-Path $bin 'cl.exe') @compile /c (Join-Path $source "$unit.cpp") "/Fo$output/$unit.obj"
        if ($LASTEXITCODE -ne 0) { throw "Dragonwilds native compilation failed: $unit" }
    }
    $state = Join-Path $output 'DragonCompletionTests.exe'
    & (Join-Path $bin 'cl.exe') @compile (Join-Path $source 'tests/completion_tests.cpp') "/Fo$output/" "/Fe$state" /link /INCREMENTAL:NO
    if ($LASTEXITCODE -ne 0) { throw 'Dragonwilds state component compilation failed.' }
    & $state
    if ($LASTEXITCODE -ne 0) { throw 'Dragonwilds state component checks failed.' }
    $files = Join-Path $output 'DragonFileTests.exe'
    & (Join-Path $bin 'cl.exe') @compile (Join-Path $source 'save_files.cpp') (Join-Path $source 'tests/files_tests.cpp') "/Fo$output/" "/Fe$files" /link bcrypt.lib /INCREMENTAL:NO
    if ($LASTEXITCODE -ne 0) { throw 'Dragonwilds file component compilation failed.' }
    $data = Join-Path $output ('files-' + [Guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($data) | Out-Null
    & $files $data
    if ($LASTEXITCODE -ne 0) { throw 'Dragonwilds actual Windows file checks failed.' }
    $vendor = [IO.Path]::GetFullPath((Join-Path $source '../../enshrouded/control/vendor/minhook'))
    $objects = @()
    foreach ($unit in @('buffer.c','hook.c','trampoline.c','hde/hde64.c')) {
        $object = Join-Path $output (([IO.Path]::GetFileNameWithoutExtension($unit))+'.obj')
        & (Join-Path $bin 'cl.exe') /nologo /c /TC /O2 /MT /DNOMINMAX "/Fo$object" (Join-Path $vendor "src/$unit")
        if ($LASTEXITCODE -ne 0) { throw "MinHook compilation failed: $unit" }
        $objects += $object
    }
    $hooks = Join-Path $output 'DragonHookTests.exe'
    $units = @('save_native.cpp','save_sinks.cpp','save_profile.cpp','save_files.cpp','tests/hooks_tests.cpp') | ForEach-Object {Join-Path $source $_}
    & (Join-Path $bin 'cl.exe') @compile /DLGSM_DRAGON_SAVE_TESTS @units "/Fo$output/" "/Fe$hooks" /link @objects bcrypt.lib /INCREMENTAL:NO
    if ($LASTEXITCODE -ne 0) { throw 'Dragonwilds native hook component compilation failed.' }
    & $hooks
    if ($LASTEXITCODE -ne 0) { throw 'Dragonwilds actual MinHook/unwind components failed.' }
    $protocol = Join-Path $output 'DragonBridgeTests.exe'
    $units = @('bridge.cpp','save_native.cpp','save_sinks.cpp','save_profile.cpp','save_files.cpp','tests/bridge_tests.cpp') | ForEach-Object {Join-Path $source $_}
    & (Join-Path $bin 'cl.exe') @compile @units "/Fo$output/" "/Fe$protocol" /link @objects bcrypt.lib /INCREMENTAL:NO
    if ($LASTEXITCODE -ne 0) { throw 'Dragonwilds closed protocol component compilation failed.' }
    & $protocol
    if ($LASTEXITCODE -ne 0) { throw 'Dragonwilds closed protocol components failed.' }
    $sinks = Join-Path $output 'DragonSinkTests.exe'
    $units = @('save_native.cpp','save_sinks.cpp','save_profile.cpp','save_files.cpp','tests/sinks_tests.cpp') | ForEach-Object {Join-Path $source $_}
    & (Join-Path $bin 'cl.exe') @compile /DLGSM_DRAGON_SAVE_TESTS @units "/Fo$output/" "/Fe$sinks" /link @objects bcrypt.lib /INCREMENTAL:NO
    if ($LASTEXITCODE -ne 0) { throw 'Dragonwilds actual file-observer component compilation failed.' }
    $data = Join-Path $output ('sinks-' + [Guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($data) | Out-Null
    & $sinks $data
    if ($LASTEXITCODE -ne 0) { throw 'Dragonwilds actual production file-observer components failed.' }
} finally { $env:INCLUDE,$env:LIB,$env:PATH = $previousInclude,$previousLib,$previousPath }
