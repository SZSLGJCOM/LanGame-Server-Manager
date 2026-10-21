param(
    [Parameter(Mandatory)][string]$RuntimeRoot,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [Parameter(Mandatory)][string]$RuntimeConfiguration
)
$ErrorActionPreference = 'Stop'
$runtime = (Resolve-Path -LiteralPath $RuntimeRoot).Path
$output = [IO.Path]::GetFullPath($OutputDirectory)
if ($output -eq $runtime -or $output.StartsWith($runtime.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase)) {
    throw 'Test output must be outside the inspected native package.'
}
$managed = Join-Path $runtime 'CoreKeeperServer_Data\Managed'
foreach ($identity in @(
    @('Pug.Other.dll','042b6db4b62a34c5354b19e478302b723b3c89a4e6c6912ffd0ce91c78311938'),
    @('0Harmony.dll','01fc6fcb6408811b72e25bfc3628cca08b27473758966777254500d07d170b4e'),
    @('netstandard.dll','6ae62e082dc494a2433984177f60ca4db5fae69b1f360a8b33754172b310b8c5')
)) {
    if ((Get-FileHash -LiteralPath (Join-Path $managed $identity[0]) -Algorithm SHA256).Hash.ToLowerInvariant() -ne $identity[1]) {
        throw ('Unsupported native test identity: '+$identity[0])
    }
}
if (Test-Path -LiteralPath $output) { throw 'Choose a new external test output directory.' }
[IO.Directory]::CreateDirectory($output) | Out-Null
$compiler = Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
$executable = Join-Path $output 'CoreKeeperCompletionTests.exe'
$arguments = @('/nologo','/noconfig','/langversion:5','/warnaserror+',('/out:'+$executable),
    ('/reference:'+(Join-Path $managed '0Harmony.dll')),('/reference:'+(Join-Path $managed 'netstandard.dll')))
foreach ($name in @('System.dll','System.Core.dll')) { $arguments += '/reference:'+(Join-Path (Split-Path $compiler) $name) }
foreach ($name in @('SaveProtocol.cs','SaveFiles.cs','CoreKeeperSaveCompletion.cs','CoreKeeperSaveHooks.cs','CoreKeeperCompletionTests.cs')) {
    $arguments += Join-Path $PSScriptRoot $name
}
& $compiler $arguments
if ($LASTEXITCODE -ne 0) { throw ('Completion test compilation failed: '+$LASTEXITCODE) }
Copy-Item -LiteralPath (Join-Path $managed '0Harmony.dll') -Destination (Join-Path $output '0Harmony.dll')
# The bundled native SDK targets netstandard 2.1; the Windows Framework
# compiler is supported, but reflection over Unity DLLs needs a modern host.
& dotnet exec --runtimeconfig (Resolve-Path -LiteralPath $RuntimeConfiguration).Path $executable $managed
if ($LASTEXITCODE -ne 0) { throw ('Completion boundary tests failed: '+$LASTEXITCODE) }
