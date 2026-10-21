param([string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
if (!$OutputDirectory) { $OutputDirectory = Join-Path ([IO.Path]::GetTempPath()) ('langame-save-control-test-' + [Guid]::NewGuid().ToString('N')) }
[IO.Directory]::CreateDirectory($OutputDirectory) | Out-Null
$compiler = Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
$executable = Join-Path $OutputDirectory 'ProtocolTests.exe'
& $compiler /nologo /langversion:5 /warnaserror+ /target:exe "/out:$executable" (Join-Path $PSScriptRoot 'SaveProtocol.cs') (Join-Path $PSScriptRoot 'SaveFiles.cs') (Join-Path $PSScriptRoot 'ProtocolTests.cs')
if ($LASTEXITCODE -ne 0) { throw "Protocol test compilation failed ($LASTEXITCODE)." }
& $executable
if ($LASTEXITCODE -ne 0) { throw "Protocol tests failed ($LASTEXITCODE)." }
