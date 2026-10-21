param(
    [Parameter(Mandatory)][string]$RuntimeRoot,
    [Parameter(Mandatory)][string]$MetadataLoadContextAssembly
)
$ErrorActionPreference='Stop'
Add-Type -Path $MetadataLoadContextAssembly
$paths=[Collections.Generic.List[string]]::new()
foreach($directory in @('BepInEx/interop','BepInEx/core','dotnet')) {
    foreach($file in (Get-ChildItem (Join-Path $RuntimeRoot $directory) -File -Filter '*.dll')) {
        if($file.Name -notlike '*.Native.dll' -and $file.Name -notin @('coreclr.dll','clrjit.dll','clretwrc.dll','mscordaccore.dll','mscordbi.dll','hostpolicy.dll')) {$paths.Add($file.FullName)}
    }
}
$resolver=[Reflection.PathAssemblyResolver]::new($paths)
$context=[Reflection.MetadataLoadContext]::new($resolver,'System.Private.CoreLib')
$checks=0
function Check([bool]$condition,[string]$label) {if(!$condition){throw "Native binding mismatch: $label"};$script:checks++}
try {
    # Resolve every production assembly/type literal against the actual generated
    # interop. This reproduces the old ProjectM.Shared placement failure without
    # loading native code or invoking a game method.
    $source=Get-Content -LiteralPath (Join-Path $PSScriptRoot 'VRisingSave.cs') -Raw
    foreach($binding in [regex]::Matches($source,'NativeBinding\.Type\("([A-Za-z0-9_.]+)", "([A-Za-z0-9_.]+)"\)')) {
        $assembly=$context.LoadFromAssemblyName($binding.Groups[1].Value)
        Check ($null -ne $assembly.GetType($binding.Groups[2].Value,$true)) ('assembly/type '+$binding.Groups[2].Value)
    }
    $project=$context.LoadFromAssemblyName('ProjectM')
    $entities=$context.LoadFromAssemblyName('Unity.Entities')
    $server=$project.GetType('ProjectM.ServerBootstrapSystem',$true)
    $save=$project.GetType('ProjectM.TriggerPersistenceSaveSystem',$true)
    $serializer=$project.GetType('ProjectM.SerializePersistenceSystemV2',$true)
    $settings=$context.LoadFromAssemblyName('ProjectM.Shared').GetType('ProjectM.ServerRuntimeSettings',$true)
    $flags=[Reflection.BindingFlags]::Public -bor [Reflection.BindingFlags]::Instance
    $initialized=$server.GetProperty('ServerIsInitialized',$flags)
    Check ($initialized.PropertyType.FullName -eq 'System.Boolean' -and !$initialized.GetMethod.IsStatic) 'ServerIsInitialized instance bool'
    $world=$server.GetProperty('World',$flags)
    Check ($world.PropertyType.FullName -eq 'Unity.Entities.World' -and !$world.GetMethod.IsStatic) 'inherited World property'
    $worldType=$entities.GetType('Unity.Entities.World',$true)
    Check ($worldType.GetProperty('IsCreated',$flags).PropertyType.FullName -eq 'System.Boolean') 'world lifecycle guard'
    Check ($worldType.GetProperty('SequenceNumber',$flags).PropertyType.FullName -eq 'System.UInt64') 'world generation identity'
    $getSettings=$server.GetMethod('GetServerRuntimeSettings',$flags,$null,[Type[]]@(),$null)
    Check ($getSettings.ReturnType -eq $settings -and !$getSettings.IsStatic) 'runtime settings exact return assembly and type'
    $update=$server.GetMethod('OnUpdate',($flags -bor [Reflection.BindingFlags]::NonPublic),$null,[Type[]]@(),$null)
    Check ($update.ReturnType.FullName -eq 'System.Void' -and !$update.IsStatic) 'bootstrap main-thread hook signature'
    $generic=@($entities.GetType('Unity.Entities.World',$true).GetMethods($flags)|Where-Object {$_.Name -eq 'GetExistingSystemManaged' -and $_.IsGenericMethodDefinition -and $_.GetGenericArguments().Length -eq 1 -and $_.GetParameters().Length -eq 0})
    Check ($generic.Count -eq 1) 'unique existing-system generic method'
    Check ($generic[0].MakeGenericMethod([Type[]]@($save)).ReturnType -eq $save) 'persistence system satisfies actual generic constraints'
    Check ($generic[0].MakeGenericMethod([Type[]]@($serializer)).ReturnType -eq $serializer) 'serializer satisfies existing-system generic constraints'
    $saving=$serializer.GetProperty('IsSaving',$flags)
    Check ($saving.PropertyType.FullName -eq 'System.Boolean' -and !$saving.GetMethod.IsStatic) 'serializer IsSaving is instance bool'
    foreach($nativeType in @($server,$entities.GetType('Unity.Entities.World',$true),$save,$serializer)) {
        $pointer=$nativeType.GetProperty('Pointer',$flags)
        Check ($pointer.PropertyType.FullName -eq 'System.IntPtr' -and !$pointer.GetMethod.IsStatic) ('native identity pointer '+$nativeType.FullName)
    }
    foreach($name in @('IsReadyToSave','TriggerAutoSave')) {
        $method=$save.GetMethod($name,($flags -bor [Reflection.BindingFlags]::NonPublic),$null,[Type[]]@($settings),$null)
        Check ($method.ReturnType.FullName -eq 'System.Boolean' -and !$method.IsStatic -and !$method.GetParameters()[0].ParameterType.IsByRef) ($name+' exact bool(settings) signature')
    }
    Write-Output "PASS: $checks actual V Rising interop binding checks (metadata only, no native invocation)."
} finally {$context.Dispose()}
