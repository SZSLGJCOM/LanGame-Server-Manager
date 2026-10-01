param(
    [Parameter(Mandatory = $true)][string]$OutDir,
    [switch]$RunTests
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$source = Join-Path $PSScriptRoot '../modules/theforest/control'
$output = [IO.Path]::GetFullPath($OutDir)
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if ($output.StartsWith($projectRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -and
    -not $output.StartsWith((Join-Path $projectRoot 'target') + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'The Forest control build output must be outside source directories.'
}
$framework = Join-Path $env:WINDIR 'Microsoft.NET/Framework/v4.0.30319'
$compiler = Join-Path $framework 'csc.exe'
if (-not (Test-Path -LiteralPath $compiler -PathType Leaf)) {
    throw 'The Forest control bridge requires the Windows .NET Framework C# compiler.'
}
[IO.Directory]::CreateDirectory($output) | Out-Null
$common = @('/nologo', '/noconfig', '/optimize+', '/warnaserror+', ('/reference:' + (Join-Path $framework 'System.dll')))
& $compiler @common /target:library ('/out:' + (Join-Path $output 'LanGame.TheForest.Control.dll')) `
    (Join-Path $source 'LanGameTheForestControl.cs') (Join-Path $source 'ControlProtocol.cs') (Join-Path $source 'NativeCheckpoint.cs') (Join-Path $source 'NativeInput.cs')
if ($LASTEXITCODE -ne 0) { throw 'The Forest control bridge compilation failed.' }
if ($RunTests) {
    $test = Join-Path $output 'TheForestControlTests.exe'
    & $compiler @common /target:exe ('/out:' + $test) (Join-Path $source 'ControlProtocol.cs') (Join-Path $source 'NativeCheckpoint.cs') (Join-Path $source 'ControlProtocolTests.cs') (Join-Path $source 'BclContractTests.cs') (Join-Path $source 'NativeInput.cs') (Join-Path $source 'NativeInputTests.cs') (Join-Path $source 'LanGameTheForestControl.cs') (Join-Path $source 'WorldReadinessTests.cs')
    if ($LASTEXITCODE -ne 0) { throw 'The Forest control protocol test compilation failed.' }
    & $test $output
    if ($LASTEXITCODE -ne 0) { throw 'The Forest control protocol tests failed.' }
}
