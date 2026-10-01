$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path -LiteralPath $vswhere -PathType Leaf)) {
    throw 'Install stable Visual Studio Build Tools with the MSVC x64 tools and redistributable files.'
}
$instances = @(& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -format json -utf8 | ConvertFrom-Json)
if ($LASTEXITCODE -ne 0 -or $instances.Count -ne 1) {
    throw 'Cannot locate a complete stable MSVC Build Tools installation.'
}
$instance = $instances[0]
if ($instance.isPrerelease -or -not $instance.isComplete -or -not $instance.isLaunchable) {
    throw 'Preview or incomplete Visual Studio installations cannot supply shipped CRT files.'
}
$installation = [System.IO.Path]::GetFullPath($instance.installationPath)
$versionFile = Join-Path $installation 'VC\Auxiliary\Build\Microsoft.VCRedistVersion.default.txt'
$version = (Get-Content -LiteralPath $versionFile -Raw).Trim()
if ($version -notmatch '^\d+\.\d+\.\d+$') { throw 'Unexpected MSVC redistributable version.' }
$redistBase = Join-Path $installation "VC\Redist\MSVC\$version\x64"
$directories = @(Get-ChildItem -LiteralPath $redistBase -Directory | Where-Object Name -Match '^Microsoft\.VC\d+\.CRT$')
if ($directories.Count -ne 1) { throw 'Expected one retail x64 CRT directory in the stable installation.' }
$redist = $directories[0].FullName
$files = foreach ($name in @('vcruntime140.dll', 'vcruntime140_1.dll', 'msvcp140.dll', 'msvcp140_1.dll')) {
    $path = Join-Path $redist $name
    $guard = [System.IO.File]::Open($path, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::Read)
    try {
    $file = Get-Item -LiteralPath $path
    if ($file.Length -gt 4MB -or ($file.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) {
        throw "Unexpected CRT file: $name"
    }
    $signature = Get-AuthenticodeSignature -LiteralPath $path
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch '(^|,\s*)O=Microsoft Corporation(,|$)') {
        throw "CRT file lacks a valid Microsoft signature: $name"
    }
    [pscustomobject]@{
        name = $name
        path = $file.FullName
        version = $file.VersionInfo.FileVersion
        sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    } finally { $guard.Dispose() }
}
[pscustomobject]@{
    version_file = $versionFile
    version = $version
    files = @($files)
} | ConvertTo-Json -Depth 4 -Compress
