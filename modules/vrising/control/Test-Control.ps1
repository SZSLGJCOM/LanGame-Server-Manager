param([string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
if (!$OutputDirectory) { $OutputDirectory = Join-Path ([IO.Path]::GetTempPath()) ('langame-vrising-test-' + [Guid]::NewGuid().ToString('N')) }
[IO.Directory]::CreateDirectory($OutputDirectory) | Out-Null
$compiler = Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
$tests = Join-Path $PSScriptRoot 'tests'
$shared = Join-Path $PSScriptRoot '../../corekeeper/control'
$settings = Join-Path $OutputDirectory 'ProjectM.Shared.dll'
$project = Join-Path $OutputDirectory 'ProjectM.dll'
$executable = Join-Path $OutputDirectory 'SaveRequestTests.exe'
& $compiler /nologo /langversion:5 /warnaserror+ /target:library "/out:$settings" (Join-Path $tests 'NativeSettingsFixture.cs')
if ($LASTEXITCODE -ne 0) { throw 'Native settings fixture compilation failed.' }
& $compiler /nologo /langversion:5 /warnaserror+ /target:library "/out:$project" "/reference:$settings" (Join-Path $tests 'NativeWorldFixture.cs')
if ($LASTEXITCODE -ne 0) { throw 'Native world fixture compilation failed.' }
$sources = @((Join-Path $shared 'SaveProtocol.cs'), (Join-Path $shared 'SaveFiles.cs'), (Join-Path $PSScriptRoot 'VRisingSave.cs'), (Join-Path $tests 'SaveRequestTests.cs'))
& $compiler /nologo /langversion:5 /warnaserror+ /target:exe "/out:$executable" "/reference:$settings" "/reference:$project" @sources
if ($LASTEXITCODE -ne 0) { throw 'V Rising request test compilation failed.' }
& $executable
if ($LASTEXITCODE -ne 0) { throw "V Rising request tests failed ($LASTEXITCODE)." }
