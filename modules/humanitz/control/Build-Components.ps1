param([Parameter(Mandatory)][string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$source = [IO.Path]::GetFullPath($PSScriptRoot)
$repository = [IO.Path]::GetFullPath((Join-Path $source '../../..'))
$output = [IO.Path]::GetFullPath($OutputDirectory)
foreach ($protected in @($repository,[IO.Path]::GetFullPath('D:/LanGame/server-files'))) {
    if ($output -eq $protected -or $output.StartsWith($protected+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)) {
        throw 'HumanitZ component output must be outside source and original server packages.'
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
    & (Join-Path $bin 'cl.exe') @compile /LD (Join-Path $source 'script_bridge.cpp') "/Fo$output/" "/Fe$output/LgsmHumanitZInspect.dll" /link bcrypt.lib /INCREMENTAL:NO /guard:cf
    if ($LASTEXITCODE -ne 0) { throw 'HumanitZ script bridge compilation failed.' }
    & (Join-Path $bin 'cl.exe') @compile (Join-Path $source 'script_snapshot_tests.cpp') "/Fo$output/" "/Fe$output/HumanitZScriptTests.exe" /link /INCREMENTAL:NO
    if ($LASTEXITCODE -ne 0) { throw 'HumanitZ script component compilation failed.' }
    & "$output/HumanitZScriptTests.exe"
    if ($LASTEXITCODE -ne 0) { throw 'HumanitZ script memory boundaries failed.' }
} finally { $env:INCLUDE,$env:LIB,$env:PATH = $previousInclude,$previousLib,$previousPath }
