param([Parameter(Mandatory = $true)][string]$TestRoot)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$fixture = Join-Path ([IO.Path]::GetFullPath($TestRoot)) ('p-' + [Guid]::NewGuid().ToString('N').Substring(0, 8))
$fixtureRepository = Join-Path $fixture 'repo with spaces'
$fixtureScripts = Join-Path $fixtureRepository 'scripts'
[IO.Directory]::CreateDirectory($fixtureScripts) | Out-Null
$builders = @(
    @{ Name = 'ue4ss-runtime'; Script = 'build_ue4ss_runtime.ps1'; Leaf = 'ue4ss-runtime'; Stop = 'LANGAME_UE4SS_ARCHIVE requires an absolute archive path.' },
    @{ Name = 'enshrouded'; Script = 'build_enshrouded_control.ps1'; Leaf = 'enshrouded-control'; Stop = 'MSVC C++ Build Tools are required.' },
    @{ Name = 'valheim'; Script = 'build_valheim_control.ps1'; Leaf = 'valheim-control'; Stop = 'Valheim control requires the Windows .NET Framework C# compiler.' },
    @{ Name = 'dragonwilds'; Script = 'build_dragonwilds_player_query.ps1'; Leaf = 'dragonwilds-player-query'; Stop = 'MSVC C++ Build Tools are required.' },
    @{ Name = 'windrose'; Script = 'build_windrose_save_control.ps1'; Leaf = 'windrose-save-control'; Stop = 'MSVC C++ Build Tools are required to build the bundled Windrose control.' },
    @{ Name = 'native-save'; Script = 'build_unreal_save_control.ps1'; Leaf = 'native-world-save'; Stop = 'MSVC C++ Build Tools are required for native world-save adapters.' },
    @{ Name = 'scum-save'; Script = 'build_scum_save_control.ps1'; Leaf = 'scum-save-control'; Stop = 'MSVC C++ Build Tools are required to build the bundled SCUM control.' },
    @{ Name = 'unreal-loader'; Script = 'build_unreal_loader_proxies.ps1'; Leaf = 'unreal-loader-proxies'; Stop = 'MSVC C++ Build Tools are required for the bundled Unreal loader proxies.' }
)
foreach ($builder in $builders) {
    Copy-Item -LiteralPath (Join-Path $repository ('scripts/' + $builder.Script)) -Destination $fixtureScripts
}
foreach ($moduleId in @('scum')) {
    $control = Join-Path $fixtureRepository ('modules/' + $moduleId + '/control')
    [IO.Directory]::CreateDirectory($control) | Out-Null
    Copy-Item -LiteralPath (Join-Path $repository ('modules/' + $moduleId + '/control/Build.ps1')) -Destination $control
}
$helper = Join-Path $repository 'scripts/build_control_output.ps1'
if (Test-Path -LiteralPath $helper) { Copy-Item -LiteralPath $helper -Destination $fixtureScripts }
$extensions = Join-Path $fixtureRepository 'modules/runescapedragonwilds/extensions'
[IO.Directory]::CreateDirectory($extensions) | Out-Null
Copy-Item -LiteralPath (Join-Path $repository 'modules/runescapedragonwilds/extensions/proxy-sources.json') -Destination $extensions
$savedArchive = $env:LANGAME_UE4SS_ARCHIVE
$savedCargoOut = $env:OUT_DIR
$savedWindows = $env:WINDIR
$savedProgramFiles = ${env:ProgramFiles(x86)}
$checks = [Collections.Generic.List[object]]::new()
$createdJunctions = [Collections.Generic.List[string]]::new()

function Test-BuilderPath($Builder, [string]$Name, [string]$Path, [bool]$Allowed) {
    $message = ''
    try { & (Join-Path $fixtureScripts $Builder.Script) -OutDir $Path | Out-Null }
    catch { $message = $_.Exception.Message }
    # Accepted paths must reach the missing toolchain or invalid archive input.
    # Neither a compiler nor a download can run in this fixture.
    $passed = if ($Allowed) { $message -eq $Builder.Stop } else { $message -ne '' -and $message -ne $Builder.Stop }
    $checks.Add([pscustomobject]@{ name = $Builder.Name + '/' + $Name; passed = $passed; detail = $message })
}

$junction = Join-Path $fixture 'output-junction'
$cargoJunction = Join-Path $fixtureRepository 'target'
$junctionTarget = Join-Path $fixture 'junction-target'
[IO.Directory]::CreateDirectory($junctionTarget) | Out-Null
try {
    $env:LANGAME_UE4SS_ARCHIVE = 'relative-fixture.zip'
    $env:WINDIR = Join-Path $fixture 'missing-windows'
    ${env:ProgramFiles(x86)} = Join-Path $fixture 'missing-toolchain'
    foreach ($builder in $builders) {
        $env:OUT_DIR = Join-Path $fixtureRepository 'target/debug/build/langame-desktop-fixture/out'
        Test-BuilderPath $builder 'cargo-debug' (Join-Path $env:OUT_DIR $builder.Leaf) $true
        $env:OUT_DIR = Join-Path $fixtureRepository 'target/x86_64-pc-windows-msvc/release/build/langame-desktop-fixture/out'
        Test-BuilderPath $builder 'cargo-release' (Join-Path $env:OUT_DIR $builder.Leaf) $true
        Test-BuilderPath $builder 'external-output' (Join-Path $fixture ('external/' + $builder.Leaf)) $true
        Test-BuilderPath $builder 'repository-root' $fixtureRepository $false
        Test-BuilderPath $builder 'repository-ancestor' $fixture $false
        Test-BuilderPath $builder 'source-directory' (Join-Path $fixtureRepository 'modules') $false
        Test-BuilderPath $builder 'wrong-cargo-leaf' (Join-Path $env:OUT_DIR 'wrong-output') $false
        Test-BuilderPath $builder 'unowned-target' (Join-Path $fixtureRepository ('target/unowned/' + $builder.Leaf)) $false
        Test-BuilderPath $builder 'relative-output' ('relative/' + $builder.Leaf) $false
        Test-BuilderPath $builder 'volume-root' ([IO.Path]::GetPathRoot($fixture)) $false
        Test-BuilderPath $builder 'share-root' '\\localhost\C$' $false
        Test-BuilderPath $builder 'command-character' (Join-Path $fixture ('unsafe&path/' + $builder.Leaf)) $false
        Test-BuilderPath $builder 'alternate-data-stream' ((Join-Path $fixture $builder.Leaf) + ':stream') $false
        Test-BuilderPath $builder 'normalized-escape' (Join-Path $env:OUT_DIR ('../../../../../modules/' + $builder.Leaf)) $false
        $env:OUT_DIR = Join-Path $fixtureRepository 'modules'
        Test-BuilderPath $builder 'forged-source-out-dir' (Join-Path $env:OUT_DIR $builder.Leaf) $false
        $env:OUT_DIR = $null
        Test-BuilderPath $builder 'missing-cargo-out-dir' (Join-Path $fixtureRepository ('target/debug/build/fixture/out/' + $builder.Leaf)) $false
        Test-BuilderPath $builder 'repository-space-alias' ($fixtureRepository + ' \modules\' + $builder.Leaf) $false
        Test-BuilderPath $builder 'repository-dot-alias' ($fixtureRepository + '.\modules\' + $builder.Leaf) $false
        Test-BuilderPath $builder 'external-leaf-space-alias' ((Join-Path $fixture ('external/' + $builder.Leaf)) + ' ') $false
        Test-BuilderPath $builder 'external-leaf-dot-alias' ((Join-Path $fixture ('external/' + $builder.Leaf)) + '.') $false
        $env:OUT_DIR = Join-Path $fixtureRepository 'target/debug/build/fixture/out'
        Test-BuilderPath $builder 'cargo-dot-component' (Join-Path $env:OUT_DIR ('./' + $builder.Leaf)) $true
        Test-BuilderPath $builder 'cargo-parent-component' (Join-Path $env:OUT_DIR ('child/../' + $builder.Leaf)) $true
    }
    # Accepted delegated builders may create their owned output before detecting
    # the missing toolchain. Preserve those artifacts before testing a junction
    # at the same exact Cargo path; do not delete a nonempty directory as a link.
    if ([IO.Directory]::Exists($cargoJunction)) {
        $preserved = Join-Path $fixture 'accepted-cargo-output'
        if (-not ([IO.Path]::GetFullPath($cargoJunction)).StartsWith($fixture + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Fixture Cargo output escaped its root.' }
        [IO.Directory]::Move($cargoJunction, $preserved)
    }
    New-Item -ItemType Junction -Path $junction -Target $junctionTarget | Out-Null
    $createdJunctions.Add($junction)
    New-Item -ItemType Junction -Path $cargoJunction -Target $junctionTarget | Out-Null
    $createdJunctions.Add($cargoJunction)
    $fileAncestor = Join-Path $fixture 'ordinary-file'
    [IO.File]::WriteAllText($fileAncestor, 'fixture')
    foreach ($builder in $builders) {
        $env:OUT_DIR = Join-Path $fixtureRepository 'target/debug/build/fixture/out'
        Test-BuilderPath $builder 'external-reparse-ancestor' (Join-Path $junction $builder.Leaf) $false
        Test-BuilderPath $builder 'cargo-reparse-ancestor' (Join-Path $env:OUT_DIR $builder.Leaf) $false
        Test-BuilderPath $builder 'file-ancestor' (Join-Path $fileAncestor $builder.Leaf) $false
    }
} finally {
    $env:LANGAME_UE4SS_ARCHIVE = $savedArchive
    $env:OUT_DIR = $savedCargoOut
    $env:WINDIR = $savedWindows
    ${env:ProgramFiles(x86)} = $savedProgramFiles
    # Remove only the two fixture links, never their targets.
    foreach ($link in $createdJunctions) {
        if ([IO.Directory]::Exists($link)) { [IO.Directory]::Delete($link) }
    }
}
$failures = @($checks | Where-Object { -not $_.passed })
[pscustomobject]@{ checks = $checks.Count; failed = $failures.Count; failures = $failures } | ConvertTo-Json -Depth 4
if ($failures.Count -ne 0) { throw ('Control build output path regression failed: ' + $failures.Count + ' cases.') }
