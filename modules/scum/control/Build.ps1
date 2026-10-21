param([Parameter(Mandatory)][string]$OutputDirectory,[switch]$RunComponents,[string]$NativeFixtureRoot)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$source = [IO.Path]::GetFullPath($PSScriptRoot)
$repository = [IO.Path]::GetFullPath((Join-Path $source '../../..'))
. (Join-Path $repository 'scripts/build_control_output.ps1')
$output = Resolve-ControlBuildOutput -OutDir $OutputDirectory -ProjectRoot $repository -CargoLeaf 'scum-save-control'
$package = [IO.Path]::GetFullPath('D:/LanGame/server-files')
foreach ($protected in @($package)) {
    if ($output -eq $protected -or $output.StartsWith($protected+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)) {
        throw 'SCUM build output must be outside source and original server packages.'
    }
}
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
if (-not (Test-Path -LiteralPath $vswhere -PathType Leaf)) { throw 'MSVC C++ Build Tools are required to build the bundled SCUM control.' }
[IO.Directory]::CreateDirectory($output) | Out-Null
Assert-PlainControlBuildOutput $output
$vs = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if ($LASTEXITCODE -ne 0 -or !$vs) { throw 'MSVC C++ Build Tools unavailable.' }
$msvc = (Get-ChildItem -LiteralPath (Join-Path $vs 'VC/Tools/MSVC') -Directory | Sort-Object {[version]$_.Name} -Descending | Select-Object -First 1).FullName
$bin = Join-Path $msvc 'bin/Hostx64/x64'
$windows = (Get-ItemProperty 'HKLM:/SOFTWARE/Microsoft/Windows Kits/Installed Roots').KitsRoot10
$version = (Get-ChildItem -LiteralPath (Join-Path $windows 'Include') -Directory | Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName 'um/Windows.h') } | Sort-Object {[version]$_.Name} -Descending | Select-Object -First 1).Name
$previousInclude,$previousLib,$previousPath = $env:INCLUDE,$env:LIB,$env:PATH
try {
    $env:INCLUDE = (@((Join-Path $msvc 'include'))+(@('ucrt','shared','um','winrt') | ForEach-Object {Join-Path $windows "Include/$version/$_"})) -join ';'
    $env:LIB = @((Join-Path $msvc 'lib/x64'),(Join-Path $windows "Lib/$version/ucrt/x64"),(Join-Path $windows "Lib/$version/um/x64")) -join ';'
    $env:PATH = "$bin;$previousPath"
    $compile = @('/nologo','/std:c++20','/EHsc','/MT','/O2','/utf-8','/DNOMINMAX','/W4','/WX','/DUNICODE','/D_UNICODE')
    $vendor = [IO.Path]::GetFullPath((Join-Path $source '../../enshrouded/control/vendor/minhook'))
    $objects = @()
    foreach ($unit in @('buffer.c','hook.c','trampoline.c','hde/hde64.c')) {
        $object = Join-Path $output (([IO.Path]::GetFileNameWithoutExtension($unit)) + '.obj')
        & (Join-Path $bin 'cl.exe') /nologo /c /TC /O2 /MT /DNOMINMAX "/Fo$object" (Join-Path $vendor "src/$unit")
        if ($LASTEXITCODE -ne 0) { throw "MinHook compilation failed: $unit" }
        $objects += $object
    }
    $sources = @('bridge.cpp','native_save.cpp','native_dispatch.cpp','native_tasks.cpp','native_sql.cpp','native_files.cpp','native_hooks.cpp','native_profile.cpp') | ForEach-Object { Join-Path $source $_ }
    $dll = Join-Path $output 'LgsmScumSaveControl.dll'
    & (Join-Path $bin 'cl.exe') @compile /LD @sources "/Fo$output/" "/Fe$dll" /link @objects bcrypt.lib /INCREMENTAL:NO
    if ($LASTEXITCODE -ne 0) { throw 'SCUM native bridge compilation failed.' }
    $bootstrap = Join-Path $output 'LgsmScumSaveBootstrap.dll'
    & (Join-Path $bin 'cl.exe') @compile /LD (Join-Path $source 'bootstrap.cpp') "/Fo$output/" "/Fe$bootstrap" /link /INCREMENTAL:NO
    if ($LASTEXITCODE -ne 0) { throw 'SCUM early bootstrap compilation failed.' }
    Write-Output "Built fixed-profile SCUM bridge: $dll"
    & (Join-Path $bin 'dumpbin.exe') /dependents $dll | Set-Content -LiteralPath (Join-Path $output 'dll-dependencies.txt') -Encoding UTF8
    if ($LASTEXITCODE -ne 0) { throw 'SCUM DLL dependency inspection failed.' }
    if ($RunComponents) {
        $component = Join-Path $output 'ScumCompletionTests.exe'
        & (Join-Path $bin 'cl.exe') @compile (Join-Path $source 'tests/completion_tests.cpp') "/Fo$output/" "/Fe$component" /link /INCREMENTAL:NO
        if ($LASTEXITCODE -ne 0) { throw 'SCUM completion component compilation failed.' }
        & $component
        if ($LASTEXITCODE -ne 0) { throw 'SCUM completion checks failed.' }
        $hooks = Join-Path $output 'ScumHookTests.exe'
        $hookSources = @('native_save.cpp','native_dispatch.cpp','native_tasks.cpp','native_sql.cpp','native_files.cpp','native_hooks.cpp','native_profile.cpp','tests/hook_tests.cpp') | ForEach-Object {Join-Path $source $_}
        & (Join-Path $bin 'cl.exe') @compile /DLGSM_SCUM_SAVE_TESTS @hookSources "/Fo$output/" "/Fe$hooks" /link @objects bcrypt.lib /INCREMENTAL:NO
        if ($LASTEXITCODE -ne 0) { throw 'SCUM hook component compilation failed.' }
        & $hooks
        if ($LASTEXITCODE -ne 0) { throw 'SCUM native MinHook/unwind checks failed.' }
        $readiness = Join-Path $output 'ScumBridgeTests.exe'
        $readinessSources = @('native_save.cpp','native_dispatch.cpp','native_tasks.cpp','native_sql.cpp','native_files.cpp','native_hooks.cpp','native_profile.cpp','tests/bridge_tests.cpp') | ForEach-Object {Join-Path $source $_}
        & (Join-Path $bin 'cl.exe') @compile @readinessSources "/Fo$output/" "/Fe$readiness" /link @objects bcrypt.lib /INCREMENTAL:NO
        if ($LASTEXITCODE -ne 0) { throw 'SCUM existing readiness regression compilation failed.' }
        & $readiness
        if ($LASTEXITCODE -ne 0) { throw 'SCUM existing authority/readiness checks failed.' }
        $queue = Join-Path $output 'ScumForeignQueueDiagnosticTests.exe'
        $queueSources = @('native_save.cpp','native_dispatch.cpp','native_sql.cpp','native_files.cpp','native_hooks.cpp','native_profile.cpp','tests/queue_diagnostic_tests.cpp') | ForEach-Object {Join-Path $source $_}
        & (Join-Path $bin 'cl.exe') @compile /DLGSM_SCUM_SAVE_TESTS @queueSources "/Fo$output/" "/Fe$queue" /link @objects bcrypt.lib /INCREMENTAL:NO
        if ($LASTEXITCODE -ne 0) { throw 'SCUM foreign queue diagnostic component compilation failed.' }
        & $queue
        if ($LASTEXITCODE -ne 0) { throw 'SCUM foreign queue diagnostic component failed.' }
        $wrapper = Join-Path $output 'ScumWrapperParentTests.exe'
        $wrapperSources = @('native_save.cpp','native_dispatch.cpp','native_sql.cpp','native_files.cpp','native_hooks.cpp','native_profile.cpp','tests/wrapper_diagnostic_tests.cpp') | ForEach-Object {Join-Path $source $_}
        & (Join-Path $bin 'cl.exe') @compile /DLGSM_SCUM_SAVE_TESTS @wrapperSources "/Fo$output/" "/Fe$wrapper" /link @objects bcrypt.lib /INCREMENTAL:NO
        if ($LASTEXITCODE -ne 0) { throw 'SCUM wrapper parent diagnostic compilation failed.' }
        & $wrapper
        if ($LASTEXITCODE -ne 0) { throw 'SCUM wrapper parent diagnostic checks failed.' }
        if ($NativeFixtureRoot) {
            $fixture = [IO.Path]::GetFullPath($NativeFixtureRoot)
            $admission = Join-Path $output 'ScumPendingAdmissionTests.exe'
            $admissionSources = @('native_save.cpp','native_dispatch.cpp','native_sql.cpp','native_files.cpp','native_hooks.cpp','tests/pending_admission_tests.cpp','tests/pending_admission_probe.cpp') | ForEach-Object {Join-Path $source $_}
            & (Join-Path $bin 'cl.exe') @compile @admissionSources "/Fo$output/" "/Fe$admission" /link @objects bcrypt.lib /INCREMENTAL:NO
            if ($LASTEXITCODE -ne 0) { throw 'SCUM pending admission/transport compilation failed.' }
            & $admission $fixture $output
            if ($LASTEXITCODE -ne 0) { throw 'SCUM pending admission/transport checks failed.' }
            $nativeWrapper = Join-Path $output 'ScumNativeWrapperTests.exe'
            $nativeWrapperSources = @('native_save.cpp','native_dispatch.cpp','native_sql.cpp','native_files.cpp','native_hooks.cpp','native_profile.cpp','tests/native_wrapper_tests.cpp') | ForEach-Object {Join-Path $source $_}
            & (Join-Path $bin 'cl.exe') @compile /DLGSM_SCUM_SAVE_TESTS @nativeWrapperSources "/Fo$output/" "/Fe$nativeWrapper" /link @objects bcrypt.lib /INCREMENTAL:NO
            if ($LASTEXITCODE -ne 0) { throw 'SCUM complete copied wrapper ABI compilation failed.' }
            & $nativeWrapper $fixture
            if ($LASTEXITCODE -ne 0) { throw 'SCUM complete copied wrapper ABI checks failed.' }
            $sqlite = Join-Path $output 'ScumNativeSqliteTests.exe'
            & (Join-Path $bin 'cl.exe') @compile (Join-Path $source 'tests/native_sqlite_tests.cpp') "/Fo$output/" "/Fe$sqlite" /link bcrypt.lib /INCREMENTAL:NO
            if ($LASTEXITCODE -ne 0) { throw 'SCUM actual SQLite component compilation failed.' }
            & $sqlite $fixture
            if ($LASTEXITCODE -ne 0) { throw 'SCUM actual SQLite payload/fault checks failed.' }
            $nativeSources = @('native_save.cpp','native_dispatch.cpp','native_tasks.cpp','native_sql.cpp','native_files.cpp','native_hooks.cpp','native_profile.cpp') | ForEach-Object {Join-Path $source $_}
            foreach ($family in @('sql','task','mixed_task','virtualized_task')) {
                $probe = Join-Path $output "ScumNative${family}Tests.exe"
                & (Join-Path $bin 'cl.exe') @compile /DLGSM_SCUM_SAVE_TESTS @nativeSources (Join-Path $source "tests/native_${family}_tests.cpp") "/Fo$output/" "/Fe$probe" /link @objects bcrypt.lib /INCREMENTAL:NO
                if ($LASTEXITCODE -ne 0) { throw "SCUM product native component compilation failed: $family" }
                $modes = if ($family -eq 'sql') { @('healthy','rollback','write','flush','close','bind') } elseif ($family -eq 'task') { @('healthy','cancel') } elseif ($family -eq 'mixed_task') { @('mixed','late','aux_write','reorder','duplicate','missing_completion','world_epoch','unknown_producer') } else { @('mixed','late_aux','readonly_aux','root_no_write','rollback','missing_root','missing_aux','payload','world','worker','replay','unknown','os_failure','table','foreign_sql','foreign_sql_pending') }
                foreach ($mode in $modes) {
                    & $probe $fixture $mode
                    if ($LASTEXITCODE -ne 0) { throw "SCUM actual product $family/$mode checks failed." }
                }
            }
        }
    }
} finally {
    $env:INCLUDE,$env:LIB,$env:PATH = $previousInclude,$previousLib,$previousPath
}
