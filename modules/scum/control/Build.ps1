param([Parameter(Mandatory)][string]$OutputDirectory,[switch]$RunComponents)
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
    $dll = Join-Path $output 'LgsmScumSaveControl.dll'
    & (Join-Path $bin 'cl.exe') @compile /LD (Join-Path $source 'bridge.cpp') "/Fo$output/" "/Fe$dll" /link bcrypt.lib /INCREMENTAL:NO
    if ($LASTEXITCODE -ne 0) { throw 'SCUM native bridge compilation failed.' }
    Write-Output "Built fixed-profile SCUM bridge: $dll"
    & (Join-Path $bin 'dumpbin.exe') /dependents $dll | Set-Content -LiteralPath (Join-Path $output 'dll-dependencies.txt') -Encoding UTF8
    if ($LASTEXITCODE -ne 0) { throw 'SCUM DLL dependency inspection failed.' }
    if ($RunComponents) {
        $test = Join-Path $output 'ScumBridgeTests.exe'
        & (Join-Path $bin 'cl.exe') @compile (Join-Path $source 'tests/bridge_tests.cpp') "/Fo$output/" "/Fe$test" /link bcrypt.lib /INCREMENTAL:NO
        if ($LASTEXITCODE -ne 0) { throw 'SCUM native boundary test compilation failed.' }
        & $test
        if ($LASTEXITCODE -ne 0) { throw 'SCUM native ownership and request checks failed.' }
    }
} finally {
    $env:INCLUDE,$env:LIB,$env:PATH = $previousInclude,$previousLib,$previousPath
}
