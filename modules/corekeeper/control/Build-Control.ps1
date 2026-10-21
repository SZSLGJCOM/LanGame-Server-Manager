param(
    [Parameter(Mandatory)][ValidateSet('corekeeper','vrising','sonsoftheforest')][string]$Module,
    [Parameter(Mandatory)][string]$RuntimeRoot,
    [string]$OutputDirectory
)
$ErrorActionPreference = 'Stop'
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
$runtime = (Resolve-Path -LiteralPath $RuntimeRoot).Path
if (!$OutputDirectory) { $OutputDirectory=Join-Path ([IO.Path]::GetTempPath()) ('langame-save-build-'+[Guid]::NewGuid().ToString('N')) }
$output = [IO.Path]::GetFullPath($OutputDirectory)
if ($output.StartsWith($runtime.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase) -or $output -eq $runtime) {throw 'Build output must be outside the inspected runtime.'}
[IO.Directory]::CreateDirectory($output) | Out-Null
$compiler=Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
$references=[Collections.Generic.List[string]]::new()
if ($Module -eq 'corekeeper') {
    $expected=@{
        'CoreKeeperServer_Data/Managed/PugMod.SDK.Runtime.dll'='6f1bd3ea3c46c7e6392c42f27bca6baf1bfb13a4eaa89756941eb51e193e7664'
        'CoreKeeperServer_Data/Managed/UnityEngine.CoreModule.dll'='d184eb633e67cd97d5a0eb0faf3a8f05bdfe17e56c3cef5595cf15c685030aa6'
        'CoreKeeperServer_Data/Managed/netstandard.dll'='6ae62e082dc494a2433984177f60ca4db5fae69b1f360a8b33754172b310b8c5'
        'CoreKeeperServer_Data/Managed/0Harmony.dll'='01fc6fcb6408811b72e25bfc3628cca08b27473758966777254500d07d170b4e'
    }
    $source='CoreKeeperSave.cs'; $assembly='LanGame.CoreKeeper.Save.dll'
} else {
    if ($Module -eq 'vrising') {
        $manifest=Get-Content -LiteralPath (Join-Path $repository 'apps/desktop/src-tauri/assets/mod-loaders/vrising.json') -Raw | ConvertFrom-Json -AsHashtable
        $runtimePrefix='dotnet/';$sdkPrefix='BepInEx/core/'
        $sdk=@('BepInEx.Core.dll','BepInEx.Unity.IL2CPP.dll','0Harmony.dll','Il2CppInterop.Runtime.dll','Il2CppInterop.Common.dll')
        $source='VRisingSave.cs';$assembly='LanGame.VRising.Save.dll'
    } else {
        $manifest=Get-Content -LiteralPath (Join-Path $repository 'modules/sonsoftheforest/control/redloader.json') -Raw | ConvertFrom-Json -AsHashtable
        $runtimePrefix='_Redloader/dotnet/';$sdkPrefix='_Redloader/net6/'
        $sdk=@('RedLoader.dll','SonsSdk.dll');$source='SonsSave.cs';$assembly='LanGame.Sons.Save.dll'
    }
    $expected=@{}
    foreach($path in $manifest.files.Keys) {
        $name=[IO.Path]::GetFileName($path)
        if ($path.StartsWith($runtimePrefix,[StringComparison]::Ordinal) -and $path.Substring($runtimePrefix.Length) -eq $name -and
            !$name.EndsWith('.Native.dll',[StringComparison]::Ordinal) -and ($name -like 'System.*.dll' -or $name -in @('mscorlib.dll','netstandard.dll','Microsoft.CSharp.dll'))) {$expected[$path]=$manifest.files[$path]}
    }
    foreach($name in $sdk) {$path=$sdkPrefix+$name;$expected[$path]=$manifest.files[$path]}
}
foreach($path in ($expected.Keys | Sort-Object)) {
    $file=Join-Path $runtime $path
    if ((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expected[$path]) {throw "SDK identity mismatch: $path"}
    $references.Add('/reference:'+$file)
}
$arguments=@('/nologo','/noconfig','/target:library','/langversion:5','/warnaserror+')
if ($Module -eq 'corekeeper') {foreach($name in @('System.dll','System.Core.dll')) {$arguments+='/reference:'+(Join-Path (Split-Path $compiler) $name)}}
else {$arguments+=@('/nostdlib+','/warnaserror-:1701')}
$arguments+='/out:'+(Join-Path $output $assembly)
$arguments+=$references.ToArray()
if ($Module -eq 'sonsoftheforest') {$arguments+='/resource:'+(Join-Path $repository 'modules/sonsoftheforest/control/manifest.json')+',LanGame.Sons.manifest.json'}
$arguments+=@((Join-Path $PSScriptRoot 'SaveProtocol.cs'),(Join-Path $PSScriptRoot 'SaveFiles.cs'),(Join-Path $repository "modules/$Module/control/$source"))
if ($Module -eq 'corekeeper') {
    foreach($name in @('CoreKeeperSaveNative.cs','CoreKeeperSaveCompletion.cs','CoreKeeperSaveHooks.cs')) {$arguments+=Join-Path $PSScriptRoot $name}
} elseif ($Module -eq 'sonsoftheforest') {
    $arguments+=Join-Path $repository 'modules/sonsoftheforest/control/SonsSaveCompletion.cs'
}
& $compiler $arguments
if ($LASTEXITCODE -ne 0) {throw "Fixed extension compilation failed ($LASTEXITCODE)."}
Write-Output (Join-Path $output $assembly)
