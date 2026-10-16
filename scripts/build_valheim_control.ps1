param(
    [Parameter(Mandatory = $true)][string]$OutDir,
    [string]$BepInExAssembly,
    [string]$UnityCoreAssembly,
    [string]$UnityEngineAssembly,
    [string]$NetStandardAssembly,
    [switch]$RunTests
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($OutDir -notmatch '^[A-Za-z]:[\\/]' -or $OutDir.IndexOf(':', 2) -ge 0) {
    throw 'Valheim control requires an absolute local OutDir.'
}
$output = [IO.Path]::GetFullPath($OutDir)
if ($output.TrimEnd([char[]]'\/').Length -le 2) { throw 'Valheim control output cannot be a volume root.' }
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if ($output.Equals($projectRoot, [StringComparison]::OrdinalIgnoreCase) -or
    $output.StartsWith($projectRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Valheim control build output must be outside the repository.'
}
function Assert-PlainOutputAncestors([string]$Path) {
    for ($current = [IO.DirectoryInfo]::new($Path); $null -ne $current; $current = $current.Parent) {
        if (Test-Path -LiteralPath $current.FullName) {
            $attributes = [IO.File]::GetAttributes($current.FullName)
            if (($attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or
                ($attributes -band [IO.FileAttributes]::Directory) -eq 0) {
                throw 'Valheim control build output and its existing ancestors must be ordinary directories.'
            }
        }
    }
}
Assert-PlainOutputAncestors $output
$references = @($BepInExAssembly, $UnityCoreAssembly, $UnityEngineAssembly, $NetStandardAssembly)
$provided = @($references | Where-Object { -not [string]::IsNullOrEmpty($_) }).Count
if ($provided -ne 0 -and $provided -ne 4) { throw 'Provide all four real assembly references or none.' }
if ($provided -eq 4) {
    foreach ($reference in $references) {
        if (-not [IO.Path]::IsPathRooted($reference) -or -not (Test-Path -LiteralPath $reference -PathType Leaf)) {
            throw 'Explicit existing BepInEx, Unity CoreModule, UnityEngine and netstandard assembly paths are required.'
        }
    }
    $expected = @(
        'BepInEx, Version=5.4.23.5, Culture=neutral, PublicKeyToken=null',
        'UnityEngine.CoreModule, Version=0.0.0.0, Culture=neutral, PublicKeyToken=null',
        'UnityEngine, Version=0.0.0.0, Culture=neutral, PublicKeyToken=null',
        'netstandard, Version=2.1.0.0, Culture=neutral, PublicKeyToken=cc7b13ffcd2ddd51'
    )
    for ($index = 0; $index -lt $references.Count; $index++) {
        if ([Reflection.AssemblyName]::GetAssemblyName($references[$index]).FullName -ne $expected[$index]) {
            throw 'The supplied real compiler reference does not match the audited assembly identity.'
        }
    }
}
$framework = Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319'
$compiler = Join-Path $framework 'csc.exe'
if (-not (Test-Path -LiteralPath $compiler -PathType Leaf)) {
    throw 'Valheim control requires the Windows .NET Framework C# compiler.'
}
$source = Join-Path $projectRoot 'modules/valheim/control'
[IO.Directory]::CreateDirectory($output) | Out-Null
Assert-PlainOutputAncestors $output
$common = @('/nologo', '/noconfig', '/optimize+', '/warnaserror+', '/langversion:5')
foreach ($assembly in @('System.dll', 'System.Core.dll', 'System.Runtime.Serialization.dll', 'System.Xml.dll')) {
    $common += '/reference:' + (Join-Path $framework $assembly)
}
$shared = @('ControlProtocol.cs', 'ControlFiles.cs', 'NativeAccessPins.cs', 'ValheimNativeControl.cs') | ForEach-Object { Join-Path $source $_ }
$plugin = Join-Path $output 'LanGame.Valheim.Control.dll'
if ($provided -eq 0) {
    $referenceOutput = Join-Path $output 'reference'
    [IO.Directory]::CreateDirectory($referenceOutput) | Out-Null
    $referenceSource = Join-Path $source 'CompilerReferences.cs'
    $unityReference = Join-Path $referenceOutput 'UnityEngine.CoreModule.dll'
    $loaderReference = Join-Path $referenceOutput 'BepInEx.dll'
    & $compiler @common /define:REFERENCE_UNITY_CORE /target:library ('/out:' + $unityReference) $referenceSource
    if ($LASTEXITCODE -ne 0) { throw 'Unity compiler reference generation failed.' }
    & $compiler @common /define:REFERENCE_BEPINEX /target:library ('/out:' + $loaderReference) `
        ('/reference:' + $unityReference) $referenceSource
    if ($LASTEXITCODE -ne 0) { throw 'BepInEx compiler reference generation failed.' }
    if ([Reflection.AssemblyName]::GetAssemblyName($loaderReference).FullName -ne 'BepInEx, Version=5.4.23.5, Culture=neutral, PublicKeyToken=null' -or
        [Reflection.AssemblyName]::GetAssemblyName($unityReference).FullName -ne 'UnityEngine.CoreModule, Version=0.0.0.0, Culture=neutral, PublicKeyToken=null') {
        throw 'Generated compiler reference identity mismatch.'
    }
    $references = @($loaderReference, $unityReference)
}
$nativeReferences = $references | ForEach-Object { '/reference:' + [IO.Path]::GetFullPath($_) }
& $compiler @common @nativeReferences /target:library ('/out:' + $plugin) @shared (Join-Path $source 'LgsmValheimControl.cs')
if ($LASTEXITCODE -ne 0) { throw 'Valheim control compilation failed.' }
if ($RunTests) {
    $test = Join-Path $output 'ValheimControlTests.exe'
    & $compiler @common /define:CONTROL_TESTS /target:exe ('/out:' + $test) @shared `
        (Join-Path $source 'ControlTests.cs') (Join-Path $source 'NativeControlTests.cs')
    if ($LASTEXITCODE -ne 0) { throw 'Valheim control test compilation failed.' }
    $reparseRoot = Join-Path $output ('reparse_' + [Guid]::NewGuid().ToString('N'))
    $target = Join-Path $reparseRoot 'target'
    $junction = Join-Path $reparseRoot 'junction'
    [IO.Directory]::CreateDirectory($target) | Out-Null
    try {
        # A directory junction needs no developer mode or symbolic-link
        # privilege. Its target and the link are both owned test fixtures.
        New-Item -ItemType Junction -Path $junction -Target $target | Out-Null
        $reparseAccepted = $false
        try { Assert-PlainOutputAncestors (Join-Path $junction 'output'); $reparseAccepted = $true } catch { }
        if ($reparseAccepted) { throw 'Compiler output accepted a reparse ancestor.' }
        & $test $output $junction
        if ($LASTEXITCODE -ne 0) { throw 'Valheim control tests failed.' }
    }
    finally {
        if (Test-Path -LiteralPath $junction) { Remove-Item -LiteralPath $junction -Force }
        [IO.Directory]::Delete($target)
        [IO.Directory]::Delete($reparseRoot)
    }
}
Write-Output $plugin
