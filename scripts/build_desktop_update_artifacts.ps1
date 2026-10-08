[CmdletBinding(DefaultParameterSetName = 'Prepare')]
param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$OutputRoot,
    [Parameter(ParameterSetName = 'Prepare')]
    [switch]$DryRun,
    [Parameter(Mandatory = $true, ParameterSetName = 'Build')]
    [switch]$BuildPortable,
    [Parameter(Mandatory = $true, ParameterSetName = 'Managed')]
    [switch]$BuildManaged,
    [Parameter(ParameterSetName = 'Managed')]
    [switch]$SignUpdates,
    [Parameter(ParameterSetName = 'Managed')]
    [ValidateRange(0, 21600)][int]$WaitTimeoutSeconds = 1800,
    [Parameter(Mandatory = $true, ParameterSetName = 'Export')]
    [ValidateNotNullOrEmpty()]
    [string]$ArtifactRoot,
    [switch]$EnableGitHubUpdates,
    [switch]$EnableRegionalUpdates,
    [switch]$OfflineInstaller,
    [string]$NotesFile = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
# Windows PowerShell may inherit PowerShell 7's PSModulePath from its caller.
# Resolve built-in hash/signature commands from the executing shell itself.
foreach ($module in @('Microsoft.PowerShell.Utility', 'Microsoft.PowerShell.Security')) {
    Import-Module (Join-Path $PSHOME "Modules/$module/$module.psd1") -ErrorAction Stop
}
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$desktopRoot = Join-Path $root 'apps/desktop'
$tauriManifest = Join-Path $desktopRoot 'src-tauri/Cargo.toml'
$configPath = Join-Path $desktopRoot 'src-tauri/tauri.conf.json'
$feedUrl = 'https://github.com/SZSLGJCOM/LanGame-Server-Manager/releases/latest/download/latest.json'
$regionalFeedUrl = 'https://langame.cn/updates/server-manager/latest.json'
if ($EnableRegionalUpdates) { $EnableGitHubUpdates = $true }
$updateFeeds = @(
    if ($EnableRegionalUpdates) { $regionalFeedUrl }
    if ($EnableGitHubUpdates) { $feedUrl }
)

function Resolve-ExternalDirectory {
    param([Parameter(Mandatory = $true)][string]$Path)
    if ($Path -match '^[\\/]{2}[?.][\\/]') {
        throw 'Release paths do not support device paths or extended-length paths.'
    }
    $absolute = [System.IO.Path]::GetFullPath($(if ([System.IO.Path]::IsPathRooted($Path)) {
        $Path
    } else { Join-Path $root $Path }))
    $prefix = $root.TrimEnd('\', '/') + [System.IO.Path]::DirectorySeparatorChar
    if ($absolute.Equals($root, [StringComparison]::OrdinalIgnoreCase) -or
        $absolute.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Release directories must be outside the source repository.'
    }
    # Reject aliases into the repository, including an existing junction ancestor.
    $ancestor = $absolute
    while (-not [string]::IsNullOrWhiteSpace($ancestor)) {
        if (Test-Path -LiteralPath $ancestor) {
            $item = Get-Item -LiteralPath $ancestor -Force
            if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw 'Release directories cannot use symlinks or junctions.'
            }
            if (-not $item.PSIsContainer) { throw 'Release directory must be a directory.' }
        }
        $ancestor = Split-Path -Parent $ancestor
    }
    return $absolute
}

function Write-Json {
    param([string]$Path, [object]$Value)
    $stream = [System.IO.File]::Open($Path, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
    try {
        $bytes = [System.Text.UTF8Encoding]::new($false).GetBytes(($Value | ConvertTo-Json -Depth 8) + "`n")
        $stream.Write($bytes, 0, $bytes.Length)
    }
    finally { $stream.Dispose() }
}

function Get-Fingerprint {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $null }
    $item = Get-Item -LiteralPath $Path
    return "$($item.Length):$($item.LastWriteTimeUtc.Ticks):$((Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash)"
}

$outputRootAbs = Resolve-ExternalDirectory -Path $OutputRoot
$configBytes = [System.IO.File]::ReadAllBytes($configPath)
$config = [System.Text.Encoding]::UTF8.GetString($configBytes).TrimStart([char]0xfeff) | ConvertFrom-Json
$hasher = [System.Security.Cryptography.SHA256]::Create()
try { $configSha256 = ([BitConverter]::ToString($hasher.ComputeHash($configBytes))).Replace('-', '').ToLowerInvariant() }
finally { $hasher.Dispose() }
$package = Get-Content -LiteralPath (Join-Path $desktopRoot 'package.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$cargoText = Get-Content -LiteralPath (Join-Path $root 'Cargo.toml') -Raw -Encoding UTF8
$workspace = [regex]::Match($cargoText, '(?ms)^\[workspace\.package\]\s*(?<body>.*?)(?=^\[|\z)')
$cargoVersion = [regex]::Match($workspace.Groups['body'].Value, '(?m)^version\s*=\s*"(?<version>[^"]+)"\s*$').Groups['version'].Value
$version = [string]$config.version
if ($version -notmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' -or
    $version -cne [string]$package.version -or $version -cne $cargoVersion) {
    throw 'Tauri, desktop package.json and Cargo workspace must share one stable major.minor.patch version.'
}
$desktopCargo = Get-Content -LiteralPath $tauriManifest -Raw -Encoding UTF8
if ($desktopCargo -notmatch '(?m)^version\.workspace\s*=\s*true\s*$') {
    throw 'The desktop crate must inherit the verified Cargo workspace version.'
}
if (@($config.plugins.updater.endpoints).Count -ne 0) {
    throw 'Keep the source update feed disabled; use -EnableGitHubUpdates for an explicit release configuration.'
}
if ([string]::IsNullOrWhiteSpace([string]$config.plugins.updater.pubkey)) {
    throw 'An updater public key is required in tauri.conf.json.'
}
$webviewInstallMode = $config.bundle.windows.webviewInstallMode
if ($webviewInstallMode.type -cne 'embedBootstrapper' -or $webviewInstallMode.silent -isnot [bool] -or
    -not $webviewInstallMode.silent) {
    throw 'Keep WebView2 configured as embedBootstrapper with silent=true; use -OfflineInstaller for the separate offline package.'
}
if ($OfflineInstaller) { $webviewInstallMode = @{ type = 'offlineInstaller'; silent = $true } }
if (-not [string]::IsNullOrWhiteSpace($NotesFile)) {
    $NotesFile = (Resolve-Path -LiteralPath $NotesFile).Path
    if (-not (Test-Path -LiteralPath $NotesFile -PathType Leaf)) { throw 'NotesFile must be a text file.' }
}
$artifactName = "$($config.productName)_${version}_x64-setup.exe"
$publicArtifactName = $artifactName.Replace(' ', '.')
if ($OfflineInstaller) { $publicArtifactName = $publicArtifactName.Replace('_x64-setup.exe', '_x64-offline-setup.exe') }
$checksumName = if ($OfflineInstaller) { 'SHA256SUMS.offline' } else { 'SHA256SUMS' }
if ($publicArtifactName -cnotmatch '^[A-Za-z0-9][A-Za-z0-9._-]*$' -or $publicArtifactName.EndsWith('.')) {
    throw 'Public artifact names must use ASCII letters, digits, dots, underscores or hyphens.'
}
$artifactUrl = 'https://github.com/SZSLGJCOM/LanGame-Server-Manager/releases/download/v' +
    $version + '/' + $publicArtifactName
$updatesValue = if ($EnableGitHubUpdates) { 'true' } else { 'false' }
$signedArtifacts = -not $BuildManaged -or $SignUpdates -or $EnableGitHubUpdates
$mergeConfig = @{
    bundle = @{
        createUpdaterArtifacts = [bool]$signedArtifacts
        windows = @{ webviewInstallMode = $webviewInstallMode }
    }
    plugins = @{ updater = @{ endpoints = $updateFeeds } }
    build = @{ beforeBuildCommand = 'cmd /c "set VITE_LANGAME_DESKTOP_UPDATES_ENABLED=' + $updatesValue + '&& npm run build"' }
}

# Portable builds never bypass the workstation's snapshot and resource policy.
$managedAncestor = Get-Item -LiteralPath $root
$managedCargoEntry = $null
while ($null -ne $managedAncestor) {
    $candidateEntry = Join-Path $managedAncestor.FullName 'scripts/invoke-codex-cargo.ps1'
    if (Test-Path -LiteralPath $candidateEntry -PathType Leaf) {
        $managedCargoEntry = $candidateEntry
        break
    }
    $managedAncestor = $managedAncestor.Parent
}
if ($BuildPortable -and $managedCargoEntry) {
    throw 'BuildPortable is unavailable on the managed OPC host. Use -BuildManaged for the snapshot-based desktop bundle.'
}
if ($BuildManaged -and -not $managedCargoEntry) {
    throw 'BuildManaged requires the managed Cargo entry on this workstation.'
}
if (($BuildPortable -or ($BuildManaged -and $signedArtifacts)) -and [string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY)) {
    throw 'TAURI_SIGNING_PRIVATE_KEY is required for signed builds and must match the configured updater public key.'
}
$artifactRootAbs = if ($ArtifactRoot) { Resolve-ExternalDirectory -Path $ArtifactRoot } else { $null }
if ($artifactRootAbs -and $artifactRootAbs.Equals($outputRootAbs, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'ArtifactRoot and OutputRoot must be different directories.'
}

New-Item -ItemType Directory -Force -Path $outputRootAbs | Out-Null
$mergeConfigPath = Join-Path $outputRootAbs 'tauri.release.conf.json'
$planPath = Join-Path $outputRootAbs 'desktop-release-plan.json'
foreach ($path in @($mergeConfigPath, $planPath)) {
    if (Test-Path -LiteralPath $path) { throw 'Use a new OutputRoot; release preparation never overwrites existing output.' }
}
if ($BuildManaged) {
    foreach ($name in @($publicArtifactName, "$publicArtifactName.sig", $checksumName, 'latest.json', 'desktop-offline-artifacts.json',
        'desktop-installer-artifacts.json', 'desktop-build-receipt.json', 'desktop-build-receipt.artifacts')) {
        if (Test-Path -LiteralPath (Join-Path $outputRootAbs $name)) {
            throw "Use a new OutputRoot; existing release output will not be replaced: $name"
        }
    }
}
Write-Json -Path $mergeConfigPath -Value $mergeConfig
$mergeConfigSha256 = (Get-FileHash -LiteralPath $mergeConfigPath -Algorithm SHA256).Hash.ToLowerInvariant()
Write-Json -Path $planPath -Value @{
    version = $version
    requested_updates_enabled = [bool]$EnableGitHubUpdates
    requested_webview_install_mode = [string]$webviewInstallMode.type
    generates_update_manifest = -not [bool]$OfflineInstaller
    artifact_build_configuration = 'not-inspected'
    update_feed = $(if ($updateFeeds.Count) { $updateFeeds[0] } else { $null })
    update_feeds = $updateFeeds
    source_artifact_name = $artifactName
    artifact_name = $publicArtifactName
    artifact_url = $artifactUrl
    tauri_merge_config = $mergeConfigPath
    frontend_updates_environment = $updatesValue
    mode = $PSCmdlet.ParameterSetName
    published = $false
}
if ($PSCmdlet.ParameterSetName -eq 'Prepare') {
    Write-Host "Local release plan: $planPath"
    Write-Host 'No build, signature, network request or publication was performed.'
    return
}

if ($BuildManaged) {
    $receiptPath = Join-Path $outputRootAbs 'desktop-build-receipt.json'
    $artifacts = @('release/langame-desktop.exe', "release/bundle/nsis/$artifactName")
    if ($signedArtifacts) { $artifacts += "release/bundle/nsis/$artifactName.sig" }
    $buildArguments = @(
        '-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
        '-File', $managedCargoEntry, '-Project', 'LanGameServerManager', '-CargoCommand', 'build',
        '-SourceSnapshot', '-DesktopBundle', 'LanGameServerManager', '-BundleConfigPath', $mergeConfigPath,
        '-SnapshotReceiptPath', $receiptPath, '-SnapshotArtifacts', ($artifacts -join ','),
        '-WaitTimeoutSeconds', [string]$WaitTimeoutSeconds
    )
    if ($EnableGitHubUpdates) { $buildArguments += '-DesktopUpdatesEnabled' }
    $buildArguments += '--locked'
    $oldStaticVcruntime = [Environment]::GetEnvironmentVariable('STATIC_VCRUNTIME', 'Process')
    try {
        [Environment]::SetEnvironmentVariable('STATIC_VCRUNTIME', 'true', 'Process')
        & powershell.exe @buildArguments
        if ($LASTEXITCODE -ne 0) { throw "Managed desktop build failed with exit code $LASTEXITCODE. No installer was exported." }
    }
    finally {
        [Environment]::SetEnvironmentVariable('STATIC_VCRUNTIME', $oldStaticVcruntime, 'Process')
    }
    Import-Module (Join-Path (Split-Path -Parent $managedCargoEntry) 'codex-snapshot-build.psm1') -Force
    $verified = Read-CodexSnapshotBuildReceipt -Path $receiptPath
    if ($verified.receipt.project -cne 'LanGameServerManager') { throw 'Build receipt belongs to another project.' }
    if ($verified.receipt.staticVcruntime -isnot [bool] -or -not $verified.receipt.staticVcruntime) {
        throw 'Build receipt does not confirm static Visual C++ runtime linkage.'
    }
    if ($null -eq $verified.desktopBundle -or $verified.desktopBundle.kind -cne 'LanGameServerManager' -or
        $verified.desktopBundle.inputConfigurationSha256 -cne $mergeConfigSha256 -or
        [bool]$verified.desktopBundle.updatesEnabled -ne [bool]$EnableGitHubUpdates -or
        [bool]$verified.desktopBundle.signed -ne [bool]$signedArtifacts) {
        throw 'Build receipt configuration does not match the requested installer configuration.'
    }
    $capturedConfig = @($verified.source.source.files | Where-Object { $_.path -ceq 'apps/desktop/src-tauri/tauri.conf.json' })
    if ($capturedConfig.Count -ne 1 -or $capturedConfig[0].sha256 -cne $configSha256) {
        throw 'Installer source configuration changed before capture. Prepare again from the captured version.'
    }
    $executable = @($verified.receipt.artifacts | Where-Object { $_.targetRelativePath -ceq 'release/langame-desktop.exe' })
    if ($executable.Count -ne 1) { throw 'Build receipt does not contain exactly one desktop executable.' }
    & python -B (Join-Path $PSScriptRoot 'verify_desktop_runtime_dependencies.py') $executable[0].path
    if ($LASTEXITCODE -ne 0) { throw 'Desktop executable runtime dependency verification failed.' }
    $installer = @($verified.receipt.artifacts | Where-Object { $_.targetRelativePath -ceq "release/bundle/nsis/$artifactName" })
    if ($installer.Count -ne 1) { throw 'Build receipt does not contain the expected installer.' }
    $artifactRootAbs = Split-Path -Parent $installer[0].path
    if (-not $signedArtifacts) {
        $destination = Join-Path $outputRootAbs $publicArtifactName
        [System.IO.File]::Copy($installer[0].path, $destination, $false)
        $digest = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($digest -cne $installer[0].sha256) { throw 'Installer changed while exporting.' }
        $signatureStatus = [string](Get-AuthenticodeSignature -LiteralPath $destination).Status
        $checksum = [System.IO.File]::Open((Join-Path $outputRootAbs $checksumName), [IO.FileMode]::CreateNew)
        try {
            $checksumBytes = [Text.UTF8Encoding]::new($false).GetBytes("$digest  $publicArtifactName`n")
            $checksum.Write($checksumBytes, 0, $checksumBytes.Length)
        }
        finally { $checksum.Dispose() }
        Write-Json -Path (Join-Path $outputRootAbs 'desktop-installer-artifacts.json') -Value @{
            version = $version; published = $false; updates_enabled = $false
            updater_signature = 'not-requested'; windows_signature_status = $signatureStatus
            build_receipt = $receiptPath; source_identity_sha256 = $verified.receipt.sourceIdentitySha256
            artifacts = @(@{ name = $publicArtifactName; sha256 = $digest; bytes = (Get-Item -LiteralPath $destination).Length })
        }
        Write-Host "Local installer: $destination"
        Write-Host 'No files were uploaded or published. Update signing and feed checks are disabled.'
        return
    }
    # The exported source configuration is fixed to the configuration verified
    # against the immutable build receipt, even if the live worktree changes.
    $configPath = Join-Path $outputRootAbs 'tauri.source.conf.json'
    Write-Json -Path $configPath -Value $config
}

if ($BuildPortable) {
    Push-Location $desktopRoot
    $oldUpdatesValue = [Environment]::GetEnvironmentVariable('VITE_LANGAME_DESKTOP_UPDATES_ENABLED', 'Process')
    $oldStaticVcruntime = [Environment]::GetEnvironmentVariable('STATIC_VCRUNTIME', 'Process')
    try {
        $env:VITE_LANGAME_DESKTOP_UPDATES_ENABLED = $updatesValue
        $metadataJson = & cargo metadata --manifest-path $tauriManifest --no-deps --format-version 1 --locked
        if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed.' }
        $cargoTargetRoot = [string](($metadataJson | ConvertFrom-Json).target_directory)
        $artifactRootAbs = Join-Path $cargoTargetRoot 'x86_64-pc-windows-msvc/release/bundle/nsis'
        $payloadPath = Join-Path $artifactRootAbs $artifactName
        $signaturePath = "$payloadPath.sig"
        $beforePayload = Get-Fingerprint -Path $payloadPath
        $beforeSignature = Get-Fingerprint -Path $signaturePath
        [Environment]::SetEnvironmentVariable('STATIC_VCRUNTIME', 'true', 'Process')
        & cargo tauri build --target x86_64-pc-windows-msvc --bundles nsis --config $mergeConfigPath -- --locked
        if ($LASTEXITCODE -ne 0) { throw 'cargo tauri build failed.' }
        $afterPayload = Get-Fingerprint -Path $payloadPath
        $afterSignature = Get-Fingerprint -Path $signaturePath
        if ($null -eq $afterPayload -or $null -eq $afterSignature -or
            $beforePayload -eq $afterPayload -or $beforeSignature -eq $afterSignature) {
            throw 'The build must produce a fresh NSIS installer and matching signature for the current version.'
        }
        $binaryRoot = Join-Path $cargoTargetRoot 'x86_64-pc-windows-msvc/release'
        & python -B (Join-Path $PSScriptRoot 'verify_desktop_runtime_dependencies.py') `
            (Join-Path $binaryRoot 'langame-desktop.exe') (Join-Path $binaryRoot 'install_catalog.exe')
        if ($LASTEXITCODE -ne 0) { throw 'Desktop executable runtime dependency verification failed.' }
    }
    finally {
        [Environment]::SetEnvironmentVariable('VITE_LANGAME_DESKTOP_UPDATES_ENABLED', $oldUpdatesValue, 'Process')
        [Environment]::SetEnvironmentVariable('STATIC_VCRUNTIME', $oldStaticVcruntime, 'Process')
        Pop-Location
    }
}

$payloadPath = Join-Path $artifactRootAbs $artifactName
$signaturePath = "$payloadPath.sig"
foreach ($path in @($payloadPath, $signaturePath)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Missing current-version NSIS artifact: $([System.IO.Path]::GetFileName($path))"
    }
}
$manifestArguments = @(
    '-B', (Join-Path $PSScriptRoot 'generate_desktop_update_manifest.py'),
    '--version', $version,
    '--artifact-file', $payloadPath, '--signature-file', $signaturePath,
    '--tauri-config', $configPath
)
if ($OfflineInstaller) {
    $manifestArguments += @('--offline-installer', '--output', (Join-Path $outputRootAbs 'desktop-offline-artifacts.json'))
} else {
    $manifestArguments += @('--artifact-url', $artifactUrl, '--output', (Join-Path $outputRootAbs 'latest.json'))
    if ($NotesFile) { $manifestArguments += @('--notes-file', $NotesFile) }
}
& python @manifestArguments
if ($LASTEXITCODE -ne 0) { throw 'Release manifest validation/export failed.' }
Write-Host "Local release assets: $outputRootAbs"
Write-Host 'No files were uploaded or published. Verify the real installer before release.'
