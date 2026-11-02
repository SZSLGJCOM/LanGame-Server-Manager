param([Parameter(Mandatory = $true)][string]$OutDir)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $PSScriptRoot 'build_control_output.ps1')
$output = ConvertTo-ControlBuildPath $OutDir $true
$repository = ConvertTo-ControlBuildPath $projectRoot
if ($output -eq [IO.Path]::GetPathRoot($output).TrimEnd('\') -or $output -eq $repository -or
    $output.StartsWith($repository + '\', [StringComparison]::OrdinalIgnoreCase) -or
    $repository.StartsWith($output + '\', [StringComparison]::OrdinalIgnoreCase) -or
    (Test-Path -LiteralPath $output)) {
    throw 'ASA proxy tests require a new directory outside the repository and its ancestors.'
}
Assert-PlainControlBuildOutput $output
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$installation = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if ($LASTEXITCODE -ne 0 -or -not $installation) { throw 'MSVC C++ Build Tools are required for ASA proxy tests.' }
$vcvars = Join-Path $installation 'VC/Auxiliary/Build/vcvars64.bat'
$source = Join-Path $projectRoot 'modules/ark-tools'
foreach ($path in @($source, $vcvars)) { $null = ConvertTo-ControlBuildPath $path }
[IO.Directory]::CreateDirectory($output) | Out-Null
$production = Join-Path $output 'production'
& (Join-Path $PSScriptRoot 'build_ark_tools.ps1') -OutDir $production -ProxyOnly
$commands = [Collections.Generic.List[string]]::new()
$commands.Add('@echo off')
$commands.Add('setlocal DisableDelayedExpansion')
$commands.Add(('call "{0}"' -f $vcvars))
$commands.Add('if errorlevel 1 exit /b 1')
$commands.Add(('cd /d "{0}"' -f $output))
$cpp = 'cl /nologo /std:c++20 /O2 /MT /EHsc /utf-8'
$commands.Add(('{0} "{1}/tests/proxy_host_tests.cpp" /Feproxy_host.exe /link /INCREMENTAL:NO' -f $cpp, $source))
$commands.Add('if errorlevel 1 exit /b 1')
foreach ($mode in 0..2) {
    $commands.Add(('{0} /LD "{1}/tests/proxy_host_tests.cpp" /DLGSM_ASA_API_FIXTURE={2} /Feapi-{2}.dll /link /INCREMENTAL:NO' -f $cpp, $source, $mode))
    $commands.Add('if errorlevel 1 exit /b 1')
}
$batch = Join-Path $output 'compile.cmd'
[IO.File]::WriteAllLines($batch, $commands, [Text.Encoding]::Default)
& cmd.exe /d /c $batch
if ($LASTEXITCODE -ne 0) { throw 'ASA proxy fixture compilation failed.' }
$hash = (Get-FileHash -LiteralPath (Join-Path $output 'proxy_host.exe') -Algorithm SHA256).Hash.ToLowerInvariant()
$utf8 = [Text.UTF8Encoding]::new($false)
function Write-Map([string]$Path, [int]$ValueSize, [switch]$Duplicate) {
    # Cache.h's actual x64 format: u64 key length, key bytes, then 8 or 32 bytes.
    [byte[]]$entry = [BitConverter]::GetBytes([UInt64]1) + [byte][char]'x' + [byte[]]::new($ValueSize)
    if ($Duplicate) { $entry += $entry }
    [IO.File]::WriteAllBytes($Path, $entry)
}
$cases = @(
    @{ Name = 'missing-api'; Mode = $null; Entered = '0'; Completed = '0'; Diagnostic = 'AsaApi.dll load failed' },
    @{ Name = 'missing-entry'; Mode = 0; Entered = '0'; Completed = '0'; Diagnostic = 'InitApi export missing' },
    @{ Name = 'exception'; Mode = 1; Entered = '1'; Completed = '0'; Diagnostic = 'InitApi threw a C++ exception' },
    @{ Name = 'success'; Mode = 2; Entered = '1'; Completed = '1'; Diagnostic = '' },
    @{ Name = 'success-raw-key'; Mode = 2; Entered = '1'; Completed = '1'; Diagnostic = '' },
    @{ Name = 'success-generation'; Mode = 2; Entered = '1'; Completed = '1'; Diagnostic = '' },
    @{ Name = 'download-enabled'; Mode = 2; Entered = '0'; Completed = '0'; Diagnostic = 'automatic symbol download must be disabled' },
    @{ Name = 'missing-config'; Mode = 2; Entered = '0'; Completed = '0'; Diagnostic = 'symbol configuration is unavailable' },
    @{ Name = 'invalid-config'; Mode = 2; Entered = '0'; Completed = '0'; Diagnostic = 'symbol configuration is invalid' },
    @{ Name = 'missing-key'; Mode = 2; Entered = '0'; Completed = '0'; Diagnostic = 'prepared symbol metadata is missing' },
    @{ Name = 'wrong-executable'; Mode = 2; Entered = '0'; Completed = '0'; Diagnostic = 'prepared symbols do not match' },
    @{ Name = 'invalid-key'; Mode = 2; Entered = '0'; Completed = '0'; Diagnostic = 'symbol metadata is invalid' },
    @{ Name = 'unsafe-generation'; Mode = 2; Entered = '0'; Completed = '0'; Diagnostic = 'symbol generation path is invalid' },
    @{ Name = 'missing-offsets'; Mode = 2; Entered = '0'; Completed = '0'; Diagnostic = 'prepared offsets are missing' },
    @{ Name = 'truncated-offsets'; Mode = 2; Entered = '0'; Completed = '0'; Diagnostic = 'prepared symbol map is truncated' },
    @{ Name = 'duplicate-offsets'; Mode = 2; Entered = '0'; Completed = '0'; Diagnostic = 'prepared symbol map has duplicate keys' },
    @{ Name = 'empty-bitfields'; Mode = 2; Entered = '0'; Completed = '0'; Diagnostic = 'prepared symbol map is empty' },
    @{ Name = 'junction-cache'; Mode = 2; Entered = '0'; Completed = '0'; Diagnostic = 'symbol directory is unavailable or unsafe' }
)
foreach ($case in $cases) {
    $directory = Join-Path $output $case.Name
    $cache = Join-Path $directory 'ArkApi/Cache'
    [IO.Directory]::CreateDirectory($cache) | Out-Null
    Copy-Item -LiteralPath (Join-Path $output 'proxy_host.exe') -Destination (Join-Path $directory 'ArkAscendedServer.exe')
    Copy-Item -LiteralPath (Join-Path $production 'asa-version.dll') -Destination (Join-Path $directory 'version.dll')
    if ($null -ne $case.Mode) {
        Copy-Item -LiteralPath (Join-Path $output "api-$($case.Mode).dll") -Destination (Join-Path $directory 'ArkApi/AsaApi.dll')
    }
    $config = Join-Path $directory 'config.json'
    [IO.File]::WriteAllText($config, '{"settings":{"AutomaticCacheDownload":{"Enable":false}}}', $utf8)
    $selected = $cache
    $generation = ''
    if ($case.Name -eq 'success-generation') {
        $generation = "generations/$hash-1-2-3"
        $selected = Join-Path $cache $generation
        [IO.Directory]::CreateDirectory($selected) | Out-Null
    }
    $key = Join-Path $cache 'cached_key.cache'
    $metadata = @{ version = 1; executable_hash = $hash; last_modified = ''; cache_directory = $generation }
    [IO.File]::WriteAllText($key, ($metadata | ConvertTo-Json -Compress), $utf8)
    $offsets = Join-Path $selected 'cached_offsets.cache'
    $bitfields = Join-Path $selected 'cached_bitfields.cache'
    Write-Map $offsets 8
    Write-Map $bitfields 32
    switch ($case.Name) {
        'success-raw-key' { [IO.File]::WriteAllText($key, "`r`n$($hash.ToUpperInvariant()) `t", $utf8) }
        'download-enabled' { [IO.File]::WriteAllText($config, '{"settings":{"AutomaticCacheDownload":{"Enable":true}}}', $utf8) }
        'missing-config' { [IO.File]::Delete($config) }
        'invalid-config' { [IO.File]::WriteAllText($config, '{"settings":null}', $utf8) }
        'missing-key' { [IO.File]::Delete($key) }
        'wrong-executable' { [IO.File]::WriteAllText($key, ('0' * 64), $utf8) }
        'invalid-key' { [IO.File]::WriteAllText($key, '{broken', $utf8) }
        'unsafe-generation' {
            $metadata.cache_directory = '../outside'
            [IO.File]::WriteAllText($key, ($metadata | ConvertTo-Json -Compress), $utf8)
        }
        'missing-offsets' { [IO.File]::Delete($offsets) }
        'truncated-offsets' { [IO.File]::WriteAllBytes($offsets, [byte[]]@(1)) }
        'duplicate-offsets' { Write-Map $offsets 8 -Duplicate }
        'empty-bitfields' { [IO.File]::WriteAllBytes($bitfields, [byte[]]::new(0)) }
        'junction-cache' {
            $retained = Join-Path $directory 'retained-cache'
            [IO.Directory]::Move($cache, $retained)
            New-Item -ItemType Junction -Path $cache -Target $retained | Out-Null
        }
    }
    $stdout = Join-Path $directory 'stdout.log'
    $stderr = Join-Path $directory 'stderr.log'
    $selectionArgument = if ($generation) { $generation } else { '-' }
    $process = Start-Process -FilePath (Join-Path $directory 'ArkAscendedServer.exe') -ArgumentList @($case.Entered, $case.Completed, $selectionArgument) -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    try {
        if (-not $process.WaitForExit(10000)) { $process.Kill(); $process.WaitForExit(); throw "ASA proxy $($case.Name) timed out." }
        $errorText = [IO.File]::ReadAllText($stderr)
        if ($process.ExitCode -ne 0) { throw "ASA proxy $($case.Name) failed ($($process.ExitCode)): $errorText" }
        if (($case.Diagnostic -and -not $errorText.Contains($case.Diagnostic)) -or
            (-not $case.Diagnostic -and $errorText.Length -ne 0)) {
            throw "ASA proxy $($case.Name) diagnostic did not match: $errorText"
        }
        Write-Output "$($case.Name): $([IO.File]::ReadAllText($stdout).Trim())"
    } finally { $process.Dispose() }
}
Write-Output "PASS: all $($cases.Count) isolated ASA proxy cases; production exports and pinned inputs verified; no game or external API was loaded."
