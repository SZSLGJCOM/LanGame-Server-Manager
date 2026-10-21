param(
    [Parameter(Mandatory)][string]$LoaderRoot,
    [Parameter(Mandatory)][string]$OutputRoot,
    [string]$DotnetRoot = 'C:/Program Files/dotnet',
    [switch]$NativeSecondResolutionName,
    [switch]$ConvertedDelegateBaseline
)
$ErrorActionPreference = 'Stop'
$control = Split-Path -Parent $PSScriptRoot
$repository = [IO.Path]::GetFullPath((Join-Path $control '../../..'))
$outputParent = (Resolve-Path -LiteralPath $OutputRoot).Path
$output = Join-Path $outputParent ('sons-backup-tests-' + [Guid]::NewGuid().ToString('N'))
if (Test-Path -LiteralPath $output) { throw 'Test output must be exclusively new.' }
New-Item -ItemType Directory -Path $output | Out-Null
$manifest = Get-Content -LiteralPath (Join-Path $control 'redloader.json') -Raw | ConvertFrom-Json -AsHashtable
$references = [Collections.Generic.List[string]]::new()
$runtime = Join-Path $output 'private-dotnet/shared/Microsoft.NETCore.App/6.0.7'
New-Item -ItemType Directory -Path $runtime -Force | Out-Null
foreach ($relative in ($manifest.files.Keys | Sort-Object)) {
    $runtimeFile = $relative -match '^_Redloader/dotnet/[^/]+$'
    $sdkFile = $relative -match '^_Redloader/net6/[^/]+\.dll$'
    if (!$runtimeFile -and !$sdkFile) { continue }
    $source = Join-Path $LoaderRoot $relative
    if ((Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant() -ne $manifest.files[$relative]) {
        throw "Reviewed RedLoader file changed: $relative"
    }
    Copy-Item -LiteralPath $source -Destination $(if ($runtimeFile) { $runtime } else { $output })
    $name = [IO.Path]::GetFileName($relative)
    if (($runtimeFile -and $name.EndsWith('.dll') -and !$name.EndsWith('.Native.dll') -and
        ($name.StartsWith('System.') -or $name -in @('mscorlib.dll','netstandard.dll','Microsoft.CSharp.dll'))) -or
        ($sdkFile -and $name -in @('RedLoader.dll','SonsSdk.dll','0Harmony.dll'))) {
        $references.Add('/reference:"' + $source + '"')
    }
}
# The loader includes .NET 6.0.7. Its Harmony/MonoMod must run on that runtime,
# even when the machine's installed SDK/runtime is newer. This is a private,
# disposable layout; no global runtime is installed or configured.
$privateHost = Join-Path $output 'private-dotnet'
$fxr = Get-ChildItem -LiteralPath (Join-Path $DotnetRoot 'host/fxr') -Directory |
    Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1
if (!$fxr) { throw 'An existing local dotnet host/fxr is required; nothing is downloaded.' }
$privateFxr = Join-Path $privateHost ('host/fxr/' + $fxr.Name)
New-Item -ItemType Directory -Path $privateFxr -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $DotnetRoot 'dotnet.exe') -Destination $privateHost
Copy-Item -LiteralPath (Join-Path $fxr.FullName 'hostfxr.dll') -Destination $privateFxr
$compiler = Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
function Build-TestAssembly([string]$Name, [string]$Target, [string[]]$Sources) {
    $arguments = @('/nologo',('/target:' + $Target),'/langversion:5','/warnaserror+','/nostdlib+','/warnaserror-:1701',
        ('/out:"' + (Join-Path $output ($Name + '.dll')) + '"')) + $references.ToArray()
    foreach ($source in $Sources) { $arguments += '"' + [IO.Path]::GetFullPath($source) + '"' }
    $response = Join-Path $output ($Name + '.rsp')
    [IO.File]::WriteAllLines($response, $arguments, [Text.UTF8Encoding]::new($true))
    & $compiler /noconfig ('@' + $response)
    if ($LASTEXITCODE -ne 0) { throw "Compiler failed for $Name with exit $LASTEXITCODE" }
}
Build-TestAssembly 'Il2Cppmscorlib' 'library' @((Join-Path $PSScriptRoot 'SonsCopyBoundary.cs'))
$references.Add('/reference:"' + (Join-Path $output 'Il2Cppmscorlib.dll') + '"')
Build-TestAssembly 'Il2CppInterop.Runtime' 'library' @((Join-Path $PSScriptRoot 'SonsInjectionBoundary.cs'))
$references.Add('/reference:"' + (Join-Path $output 'Il2CppInterop.Runtime.dll') + '"')
Build-TestAssembly 'Sons.Save' 'library' @((Join-Path $PSScriptRoot 'SonsBackupBoundary.cs'))
$references.Add('/reference:"' + (Join-Path $output 'Sons.Save.dll') + '"')
$nativeSource = Join-Path $control 'SonsSave.cs'
if ($NativeSecondResolutionName) {
    # Differential native-naming baseline, not an old production binary. All
    # archive/filesystem behavior remains real; exclusive publication must fail
    # on the second same-second request just as admitted native CreateNew does.
    $baseline = [IO.File]::ReadAllText($nativeSource).Replace(
        'string destination = Publish(temporary, parent, stem);',
        'string destination = Path.Combine(parent, stem + ".zip"); File.Move(temporary, destination);')
    $nativeSource = Join-Path $output 'NativeSecondResolutionName.cs'
    [IO.File]::WriteAllText($nativeSource, $baseline, [Text.UTF8Encoding]::new($false))
}
$completionSource = Join-Path $control 'SonsSaveCompletion.cs'
if ($ConvertedDelegateBaseline) {
    $baseline = [IO.File]::ReadAllText($completionSource).Replace(
        'NativeCallbackReceiver.Get(beforeType, afterType, out capture.Before, out capture.After);',
        ('capture.Before = NativeBinding.Method(beforeType, "op_Implicit", BindingFlags.Public | BindingFlags.Static, beforeType, typeof(Action)).Invoke(null, new object[] {new Action(capture.BeforeSave)});' +
         'capture.After = NativeBinding.Method(afterType, "op_Implicit", BindingFlags.Public | BindingFlags.Static, afterType, typeof(Action<bool>)).Invoke(null, new object[] {new Action<bool>(capture.AfterSave)});'))
    $completionSource = Join-Path $output 'ConvertedDelegateBaseline.cs'
    [IO.File]::WriteAllText($completionSource, $baseline, [Text.UTF8Encoding]::new($false))
}
$shared = @((Join-Path $repository 'modules/corekeeper/control/SaveProtocol.cs'),
    (Join-Path $repository 'modules/corekeeper/control/SaveFiles.cs'), $nativeSource,
    $completionSource)
Build-TestAssembly 'SonsBackupTests' 'exe' ($shared + @((Join-Path $control 'SonsBackupTests.cs')))
$runtimeConfig = '{"runtimeOptions":{"tfm":"net6.0","framework":{"name":"Microsoft.NETCore.App","version":"6.0.7"}}}'
[IO.File]::WriteAllText((Join-Path $output 'SonsBackupTests.runtimeconfig.json'), $runtimeConfig, [Text.UTF8Encoding]::new($false))
& (Join-Path $privateHost 'dotnet.exe') (Join-Path $output 'SonsBackupTests.dll') $output
if ($LASTEXITCODE -ne 0) { throw "Sons backup regression failed with exit $LASTEXITCODE; artifacts: $output" }
if ($NativeSecondResolutionName) { throw 'Native naming baseline unexpectedly passed the same-second regression.' }
Build-TestAssembly 'SonsReadinessTests' 'exe' ($shared + @((Join-Path $control 'SonsSaveReadinessTests.cs')))
[IO.File]::WriteAllText((Join-Path $output 'SonsReadinessTests.runtimeconfig.json'), $runtimeConfig, [Text.UTF8Encoding]::new($false))
& (Join-Path $privateHost 'dotnet.exe') (Join-Path $output 'SonsReadinessTests.dll')
if ($LASTEXITCODE -ne 0) { throw "Sons readiness regression failed with exit $LASTEXITCODE; artifacts: $output" }
Write-Output "Sons boundary tests completed; no native game started. Artifacts: $output"
Build-TestAssembly 'SonsSaveCompletionTests' 'exe' ($shared + @((Join-Path $control 'SonsSaveCompletionTests.cs')))
[IO.File]::WriteAllText((Join-Path $output 'SonsSaveCompletionTests.runtimeconfig.json'), $runtimeConfig, [Text.UTF8Encoding]::new($false))
& (Join-Path $privateHost 'dotnet.exe') (Join-Path $output 'SonsSaveCompletionTests.dll') $output
if ($LASTEXITCODE -ne 0) { throw "Sons completion regression failed with exit $LASTEXITCODE; artifacts: $output" }
if ($ConvertedDelegateBaseline) { throw 'Converted delegate baseline unexpectedly passed native registry reflection.' }
& (Join-Path $privateHost 'dotnet.exe') (Join-Path $output 'SonsSaveCompletionTests.dll') $output 'successful-native-cleanup-failure'
if ($LASTEXITCODE -ne 0) { throw "Sons successful-native cleanup regression failed with exit $LASTEXITCODE; artifacts: $output" }
