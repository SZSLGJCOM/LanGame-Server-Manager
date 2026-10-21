param([Parameter(Mandatory = $true)][string]$OutDir, [switch]$RunTests)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
foreach ($moduleName in @('Microsoft.PowerShell.Management', 'Microsoft.PowerShell.Utility')) {
    Import-Module -Name ([IO.Path]::Combine($PSHOME, 'Modules', $moduleName, $moduleName + '.psd1')) -ErrorAction Stop
}
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $PSScriptRoot 'build_control_output.ps1')
$output = Resolve-ControlBuildOutput -OutDir $OutDir -ProjectRoot $projectRoot -CargoLeaf 'scum-save-control'
& (Join-Path $projectRoot 'modules/scum/control/Build.ps1') -OutputDirectory $output -RunComponents:$RunTests
if ($LASTEXITCODE -ne 0) { throw 'SCUM save control build failed.' }
Write-Output (Join-Path $output 'LgsmScumSaveControl.dll')
