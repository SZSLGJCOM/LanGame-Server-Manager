param([Parameter(Mandatory)][string]$OutputRoot)
$ErrorActionPreference = 'Stop'
$control = Split-Path -Parent $PSScriptRoot
$repository = [IO.Path]::GetFullPath((Join-Path $control '../../..'))
$output = Join-Path (Resolve-Path -LiteralPath $OutputRoot).Path ('barotrauma-control-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $output | Out-Null
$arguments = @('/nologo','/target:exe','/langversion:5','/warnaserror+',('/out:"'+(Join-Path $output 'DedicatedServer.exe')+'"'))
foreach ($source in @('modules/corekeeper/control/SaveProtocol.cs','modules/corekeeper/control/SaveFiles.cs',
    'modules/barotrauma/control/BarotraumaSave.cs','modules/barotrauma/control/BarotraumaControlTests.cs')) {
    $arguments += '"'+[IO.Path]::GetFullPath((Join-Path $repository $source))+'"'
}
$response = Join-Path $output 'tests.rsp'
[IO.File]::WriteAllLines($response,$arguments,[Text.UTF8Encoding]::new($true))
& (Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319/csc.exe') ('@'+$response)
if ($LASTEXITCODE -ne 0) { throw "Boundary test compilation failed ($LASTEXITCODE)" }
& (Join-Path $output 'DedicatedServer.exe')
if ($LASTEXITCODE -ne 0) { throw "Boundary tests failed ($LASTEXITCODE); artifacts: $output" }
Write-Output "Barotrauma boundary artifacts: $output; native game not started"
