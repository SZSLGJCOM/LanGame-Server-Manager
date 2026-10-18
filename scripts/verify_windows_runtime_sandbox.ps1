<#
.SYNOPSIS
Runs exported production Windows runtime scripts inside Windows Sandbox only.
.DESCRIPTION
Export with the ignored export_windows_runtime_sandbox_manifest test and
LGSM_RUNTIME_SANDBOX_EXPORT_DIR. Prepare input with -ManifestPath -WorkRoot
-PrepareOnly, then launch with -PreparedRoot when Windows Sandbox is available.
The host never downloads or installs a runtime. Guest results are flushed after
each case. Closing a running sandbox can interrupt an installer: a host timeout
reports the running guest rather than terminating it.
#>
[CmdletBinding(DefaultParameterSetName = 'Prepare')]
param(
    [Parameter(Mandatory, ParameterSetName = 'Prepare')]
    [Parameter(Mandatory, ParameterSetName = 'Guest')]
    [string]$ManifestPath,
    [Parameter(Mandatory, ParameterSetName = 'Prepare')]
    [string]$WorkRoot,
    [Parameter(ParameterSetName = 'Prepare')]
    [string]$PortableExportDirectory,
    [Parameter(ParameterSetName = 'Prepare')]
    [switch]$PortableOnly,
    [Parameter(ParameterSetName = 'Prepare')]
    [string]$NativeHttpExportDirectory,
    [Parameter(ParameterSetName = 'Prepare')]
    [string]$HostDownloadReport,
    [Parameter(ParameterSetName = 'Prepare')]
    [switch]$RemainingDownloads,
    [Parameter(ParameterSetName = 'Prepare')]
    [string]$DownloadedPackagesDirectory,
    [Parameter(ParameterSetName = 'Prepare')]
    [string]$BootstrapPackagePath,
    [Parameter(ParameterSetName = 'Prepare')]
    [string]$BootstrapPackageProvenance = 'Operator-prefetched Microsoft package; input hash and guest signature are recorded.',
    [Parameter(ParameterSetName = 'Prepare')]
    [switch]$PrepareOnly,
    [Parameter(Mandatory, ParameterSetName = 'Run')]
    [string]$PreparedRoot,
    [Parameter(Mandatory, ParameterSetName = 'Guest')]
    [switch]$Guest,
    [Parameter(Mandatory, ParameterSetName = 'Guest')]
    [string]$OutputRoot,
    [Parameter(Mandatory, ParameterSetName = 'Guest')]
    [string]$RunToken,
    [ValidateRange(5, 120)]
    [int]$TimeoutMinutes = 60
)

$ErrorActionPreference = 'Stop'
$utf8 = New-Object Text.UTF8Encoding($false)
$remainingCaseIds = @('vc-14-x64', 'vc-14-x86', 'vc-2013-x86')

function Get-NativeCaseCount([string]$Selection) {
    return @{ full = 10; 'portable-bootstrap' = 1; 'download-remaining' = 1; 'remaining-downloads' = 3 }[$Selection]
}

function Assert-PlainPath([string]$Path) {
    $full = [IO.Path]::GetFullPath($Path)
    if (![IO.Path]::IsPathRooted($full)) { throw 'An absolute path is required.' }
    $item = $full
    while ($item) {
        if (Test-Path -LiteralPath $item) {
            if ((Get-Item -LiteralPath $item -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
                throw "Reparse points are not allowed: $item"
            }
        }
        $parent = [IO.Directory]::GetParent($item)
        $item = if ($null -eq $parent) { $null } else { $parent.FullName }
    }
    return $full
}

function Write-NewUtf8([string]$Path, [string]$Text) {
    $stream = [IO.File]::Open($Path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Read)
    try {
        $bytes = $utf8.GetBytes($Text)
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush($true)
    } finally { $stream.Dispose() }
}

function Write-JsonAtomically([string]$Path, $Value) {
    $temporary = $Path + '.' + [Guid]::NewGuid().ToString('N') + '.tmp'
    try {
        Write-NewUtf8 $temporary ($Value | ConvertTo-Json -Depth 30)
        # VSMB mapped folders do not support File.Replace. Publish immutable
        # snapshots by renaming to a previously unused name instead.
        [IO.File]::Move($temporary, $Path)
    } finally {
        if ([IO.File]::Exists($temporary)) { [IO.File]::Delete($temporary) }
    }
}

function Assert-GuestIdentity {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    if ($identity.Name.Split('\')[-1] -cne 'WDAGUtilityAccount' -or
        !$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator) -or
        $env:USERPROFILE -ine (Join-Path $env:SystemDrive 'Users\WDAGUtilityAccount') -or
        [IO.Path]::GetFullPath($ManifestPath) -cne 'C:\LGSMInput\manifest.json' -or
        [IO.Path]::GetFullPath($OutputRoot) -cne 'C:\LGSMOutput') {
        throw 'Runtime installation is permitted only in the dedicated Windows Sandbox guest.'
    }
    if ([IO.Directory]::Exists("C:\LanGame Sandbox O'Brien")) {
        throw 'The dedicated guest working directory already exists; use a fresh sandbox. This guest will not be stopped.'
    }
    $request = Get-Content -LiteralPath 'C:\LGSMInput\run.json' -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($request.schema_version -ne 1 -or $request.run_token -cne $RunToken -or
        $RunToken -notmatch '^[a-f0-9]{32}$' -or $env:COMPUTERNAME -ieq $request.host_name -or
        (Get-FileHash -LiteralPath $ManifestPath -Algorithm SHA256).Hash -ine $request.manifest_sha256) {
        throw 'The sandbox run identity or immutable input manifest does not match.'
    }
    if (!(Get-NativeCaseCount $request.case_selection) -or
        ($request.case_selection -eq 'portable-bootstrap' -and ![IO.File]::Exists('C:\LGSMInput\portable\worker.ps1')) -or
        ($request.case_selection -eq 'download-remaining' -and ![IO.File]::Exists('C:\LGSMInput\native-http\worker.ps1')) -or
        ($request.case_selection -eq 'remaining-downloads' -and ![IO.File]::Exists('C:\LGSMInput\downloaded-packages\results.json'))) {
        throw 'Unexpected native-case selection or missing required portable worker.'
    }
    if ($request.bootstrap_package -and ($request.case_selection -notin @('portable-bootstrap', 'download-remaining') -or
        $request.bootstrap_package.relative_path -cne 'bootstrap\vc-14-x64.exe')) {
        throw 'A prefetched bootstrap package is restricted to the portable-only VC14 x64 case.'
    }
    foreach ($file in $request.input_files) {
        $path = Join-Path 'C:\LGSMInput' $file.relative_path
        if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ine $file.sha256) {
            throw 'The immutable guest input bundle does not match the prepared run.'
        }
    }
    return $request
}

function Invoke-ProductionScript([string]$Script, [string]$Architecture, [string]$Label) {
    $scriptPath = Join-Path $guestScratch ($Label + '-' + [Guid]::NewGuid().ToString('N') + '.ps1')
    Write-NewUtf8 $scriptPath $Script
    $system = if ($Architecture -eq 'x86') { 'SysWOW64' } else { 'System32' }
    $shell = Join-Path $env:WINDIR ($system + '\WindowsPowerShell\v1.0\powershell.exe')
    $command = '& ([ScriptBlock]::Create([IO.File]::ReadAllText(''' + $scriptPath.Replace("'", "''") + ''',[Text.Encoding]::UTF8)))'
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command))
    $start = New-Object Diagnostics.ProcessStartInfo
    $start.FileName = $shell
    $start.Arguments = '-NoProfile -NonInteractive -EncodedCommand ' + $encoded
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.StandardOutputEncoding = $utf8
    $start.StandardErrorEncoding = $utf8
    $process = New-Object Diagnostics.Process
    $process.StartInfo = $start
    try {
        if (!$process.Start()) { throw 'Could not start trusted guest PowerShell.' }
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        # Production installers are atomic: never kill this waiter or its child.
        $process.WaitForExit()
        $stdout = $stdoutTask.GetAwaiter().GetResult()
        $stderr = $stderrTask.GetAwaiter().GetResult()
        $outPath = Join-Path $OutputRoot ($Label + '.stdout.txt')
        $errPath = Join-Path $OutputRoot ($Label + '.stderr.txt')
        Write-NewUtf8 $outPath $stdout
        Write-NewUtf8 $errPath $stderr
        $parsed = $null
        $parseError = $null
        try { $parsed = $stdout | ConvertFrom-Json } catch { $parseError = $_.Exception.Message }
        return [pscustomobject]@{
            process_exit_code = $process.ExitCode
            value = $parsed
            parse_error = $parseError
            stdout_path = $outPath
            stderr_path = $errPath
        }
    } finally { $process.Dispose() }
}

function Invoke-Probes($Case, [string]$Stage) {
    $values = @()
    foreach ($probe in $Case.probes) {
        $result = Invoke-ProductionScript $probe.script $probe.architecture ($Case.id + '-' + $Stage + '-' + $probe.architecture)
        $values += [pscustomobject]@{
            architecture = $probe.architecture
            dlls = $probe.dlls
            ready = ($result.process_exit_code -eq 0 -and $null -ne $result.value -and $result.value.ready -eq $true)
            process_exit_code = $result.process_exit_code
            value = $result.value
            stdout_path = $result.stdout_path
            stderr_path = $result.stderr_path
        }
    }
    return $values
}

function Copy-GuestDiagnostic([string]$Path, [string]$Name) {
    if ($Path -and [IO.File]::Exists($Path)) {
        $target = Join-Path $OutputRoot $Name
        Copy-Item -LiteralPath $Path -Destination $target
        return $target
    }
    return $null
}

function Assert-NativeHttpArtifacts([string]$Directory, $Manifest, $Binding) {
    $reportPath = Join-Path $Directory 'results.json'
    $report = Get-Content -LiteralPath $reportPath -Raw -Encoding UTF8 | ConvertFrom-Json
    $hostSource = $Binding -and $Binding.source_kind -ceq 'host-production-http'
    if ($report.schema_version -ne 1 -or @($report.cases).Count -ne 3 -or
        ($hostSource -and $report.scope -cne 'host production HTTP recovery verification; no installation') -or
        (!$hostSource -and (!$report.guest.administrator -or $report.guest.user.Split('\')[-1] -cne 'WDAGUtilityAccount')) -or
        ($Binding -and ((Get-FileHash -LiteralPath $reportPath -Algorithm SHA256).Hash -ine $Binding.report_sha256 -or
        (!$hostSource -and $report.run_token_sha256 -cne $Binding.run_token_sha256) -or $report.test_binary_sha256 -ine $Binding.binary_sha256))) {
        throw 'Production HTTP artifact evidence does not match its declared source run.'
    }
    foreach ($id in $remainingCaseIds) {
        $rows = @($report.cases | Where-Object { $_.id -ceq $id })
        $expected = @($Manifest.cases | Where-Object { $_.id -ceq $id })
        $relative = if ($hostSource) { $id + '.exe' } else { 'packages/' + $id + '.exe' }
        $artifact = Join-Path $Directory $relative
        $url = if ($hostSource) { $rows[0].official_url } else { $rows[0].url }
        $bytes = if ($hostSource) { $rows[0].download.bytes } else { $rows[0].downloaded_bytes }
        $sha256 = if ($hostSource) { $rows[0].download.sha256 } else { $rows[0].sha256 }
        if ($rows.Count -ne 1 -or $expected.Count -ne 1 -or $rows[0].status -cne 'passed' -or
            $url -cne $expected[0].url -or (!$hostSource -and $rows[0].export_relative_path -cne $relative) -or
            $rows[0].observed_downloaded_bytes -le 0 -or $bytes -le 0 -or
            (Get-Item -LiteralPath $artifact).Length -ne $bytes -or
            (Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash -ine $sha256) {
            throw "Production HTTP case $id does not prove the exact fresh official package."
        }
    }
    return $report
}

function Invoke-GuestDownload([string]$Url, [string]$Path, [string]$Label) {
    $curl = Join-Path ([Environment]::SystemDirectory) 'curl.exe'
    if (![IO.File]::Exists($curl)) { throw 'The Windows system curl executable is unavailable.' }
    # Only TLS handshake error 35 is retried. Three 58-second attempts plus
    # 1+2 seconds of backoff stay below the 180-second download budget.
    $arguments = '--fail --location --proto "=https" --proto-redir "=https" --silent --show-error --max-time 58 --output "' + $Path + '" --write-out "%{url_effective}" "' + $Url + '"'
    $start = New-Object Diagnostics.ProcessStartInfo
    $start.FileName = $curl
    $start.Arguments = $arguments
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.StandardOutputEncoding = $utf8
    $start.StandardErrorEncoding = $utf8
    $attempts = New-Object 'System.Collections.Generic.List[object]'
    for ($attempt = 1; $attempt -le 3; $attempt++) {
        $download = New-Object Diagnostics.Process
        $download.StartInfo = $start
        try {
            if (!$download.Start()) { throw 'Could not start the trusted Windows curl executable.' }
            $stdoutTask = $download.StandardOutput.ReadToEndAsync()
            $stderrTask = $download.StandardError.ReadToEndAsync()
            $download.WaitForExit()
            $stdout = $stdoutTask.GetAwaiter().GetResult()
            $stderr = $stderrTask.GetAwaiter().GetResult()
            $stderrPath = Join-Path $OutputRoot ($Label + '-download-attempt' + $attempt.ToString('D2') + '.stderr.txt')
            Write-NewUtf8 $stderrPath $stderr
            $attempts.Add(@{ attempt = $attempt; curl_exit_code = $download.ExitCode; final_url = $stdout.Trim(); stderr_path = $stderrPath })
            if ($download.ExitCode -ne 35 -or $attempt -eq 3) {
                return [pscustomobject]@{ curl_exit_code = $download.ExitCode; final_url = $stdout.Trim(); stderr = $stderr; attempts = @($attempts.ToArray()) }
            }
        } finally { $download.Dispose() }
        Start-Sleep -Seconds $attempt
    }
}

function Invoke-GuestMatrix {
    $request = Assert-GuestIdentity
    $manifest = Get-Content -LiteralPath $ManifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
    $expectedRoot = "C:\LanGame Sandbox O'Brien\运行库\runtimes\windows-packages"
    if ($manifest.schema_version -ne 1 -or $manifest.cases.Count -ne 10 -or
        [IO.Path]::GetFullPath($manifest.guest_package_root) -cne $expectedRoot) {
        throw 'Unexpected production-script manifest or guest working path.'
    }
    $script:guestScratch = "C:\LanGame Sandbox O'Brien\scripts"
    $null = [IO.Directory]::CreateDirectory($guestScratch)
    $null = [IO.Directory]::CreateDirectory($expectedRoot)
    $null = [IO.Directory]::CreateDirectory((Join-Path $expectedRoot 'empty-consumer-directory'))
    $selectedCases = @(if ($request.case_selection -in @('portable-bootstrap', 'download-remaining')) {
        $manifest.cases | Where-Object { $_.id -ceq 'vc-14-x64' }
    } elseif ($request.case_selection -eq 'remaining-downloads') {
        $manifest.cases | Where-Object { $_.id -cin $remainingCaseIds }
    } else { $manifest.cases })
    $expectedCaseCount = Get-NativeCaseCount $request.case_selection
    if ($selectedCases.Count -ne $expectedCaseCount) { throw 'The selected production cases do not match the run request.' }
    . ([ScriptBlock]::Create($manifest.signature_function))
    if ($request.download_evidence.source_kind -eq 'host-production-http') {
        $httpManifestPath = 'C:\LGSMInput\downloaded-packages\http-manifest.json'
        $httpManifest = Get-Content -LiteralPath $httpManifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
        if ((Get-FileHash -LiteralPath $httpManifestPath -Algorithm SHA256).Hash -ine $request.download_evidence.http_manifest_sha256 -or
            $httpManifest.binary_sha256 -ine $request.download_evidence.binary_sha256) { throw 'The host download report no longer matches its exported production HTTP binary.' }
    }
    $downloadArtifacts = if ($request.case_selection -eq 'remaining-downloads') { Assert-NativeHttpArtifacts 'C:\LGSMInput\downloaded-packages' $manifest $request.download_evidence } else { $null }
    $results = New-Object 'System.Collections.Generic.List[object]'
    $report = [ordered]@{
        schema_version = 1
        run_token = $RunToken
        started_utc = [DateTime]::UtcNow.ToString('o')
        environment = @{ identity = [Security.Principal.WindowsIdentity]::GetCurrent().Name; hostname = $env:COMPUTERNAME; os = [Environment]::OSVersion.VersionString; powershell = $PSVersionTable.PSVersion.ToString() }
        scope = $manifest.scope
        download_scope = if ($request.download_evidence.source_kind -eq 'host-production-http') { 'Packages were freshly downloaded and verified by the production HTTP client on the host; this fresh guest verifies and cold-installs those exact bytes. Guest download networking is not exercised.' } elseif ($request.case_selection -eq 'remaining-downloads') { 'Packages were first downloaded by the production HTTP client in a separate isolated guest; this fresh guest verifies and cold-installs those exact bytes.' } elseif ($request.bootstrap_package) { 'Prefetched cache bootstrap: guest HTTP download is not exercised. API worker downloads use their production HTTP client.' } else { 'Guest Windows system curl; the production reqwest downloader is not exercised.' }
        case_selection = $request.case_selection
        expected_native_case_count = $expectedCaseCount
        cases = @()
        complete = $false
        fatal_error = $null
        portable = $null
        native_http = $null
        download_evidence = $request.download_evidence
    }
    Write-JsonAtomically (Join-Path $OutputRoot 'snapshot-00.json') $report
    try {
        foreach ($case in $selectedCases) {
            $result = [ordered]@{ id = $case.id; name = $case.name; official_url = $case.url; started_utc = [DateTime]::UtcNow.ToString('o'); baseline = @(); download = $null; installer = $null; postprobe = @(); status = 'failed'; failure_stage = $null; error = $null; diagnostics = @() }
            $stage = 'baseline'
            try {
                $result.baseline = @(Invoke-Probes $case 'baseline')
                if (@($result.baseline | Where-Object { $_.process_exit_code -ne 0 -or $null -eq $_.value }).Count) { throw 'Production baseline probe did not return a health result.' }
                if ($request.case_selection -eq 'remaining-downloads' -and @($result.baseline | Where-Object { $_.ready }).Count) { throw 'A remaining-runtime cold install requires an actually missing baseline.' }
                $package = [IO.Path]::GetFullPath($case.package_path)
                if (![IO.Path]::GetDirectoryName($package).Equals($expectedRoot, [StringComparison]::OrdinalIgnoreCase)) { throw 'Package escaped the dedicated guest root.' }
                $uri = [Uri]$case.url
                if ($uri.Scheme -ne 'https' -or $uri.Host -notin @('aka.ms', 'download.microsoft.com', 'download.visualstudio.microsoft.com')) { throw 'Unexpected download source.' }
                $stage = 'download'
                $temporary = Join-Path $expectedRoot ('download-' + [Guid]::NewGuid().ToString('N') + [IO.Path]::GetExtension($package))
                if ($request.bootstrap_package -and $case.id -ceq 'vc-14-x64') {
                    $prefetched = Join-Path 'C:\LGSMInput' $request.bootstrap_package.relative_path
                    [IO.File]::Copy($prefetched, $temporary)
                    $result.download = @{ source = 'prefetched-cache'; provenance = $request.bootstrap_package.provenance; input_sha256 = $request.bootstrap_package.sha256; attempts = @() }
                } elseif ($request.case_selection -eq 'remaining-downloads') {
                    $source = @($downloadArtifacts.cases | Where-Object { $_.id -ceq $case.id })[0]
                    if ($request.download_evidence.source_kind -eq 'host-production-http') {
                        [IO.File]::Copy((Join-Path 'C:\LGSMInput\downloaded-packages' ($case.id + '.exe')), $temporary)
                        $result.download = @{ source = 'production-http-host'; official_url = $source.official_url; downloaded_bytes = $source.download.bytes; input_sha256 = $source.download.sha256; attempts = @() }
                    } else {
                        [IO.File]::Copy((Join-Path 'C:\LGSMInput\downloaded-packages' $source.export_relative_path), $temporary)
                        $result.download = @{ source = 'production-http-isolated-guest'; official_url = $source.url; downloaded_bytes = $source.downloaded_bytes; input_sha256 = $source.sha256; source_run_token_sha256 = $downloadArtifacts.run_token_sha256; attempts = @() }
                    }
                } else {
                    $download = Invoke-GuestDownload $case.url $temporary $case.id
                    $result.download = @{ source = 'guest-curl'; final_url = $download.final_url; curl_exit_code = $download.curl_exit_code; attempts = $download.attempts }
                    if ($download.curl_exit_code -ne 0) { throw "Official download failed (Windows curl exit $($download.curl_exit_code) after $($download.attempts.Count) attempt(s)): $($download.stderr)" }
                }
                $stage = 'verification'
                $hash = (Get-FileHash -LiteralPath $temporary -Algorithm SHA256).Hash
                $signature = Get-AuthenticodeSignature -LiteralPath $temporary
                $result.download.sha256 = $hash
                $result.download.expected_sha256 = $case.expected_sha256
                $result.download.bytes = (Get-Item -LiteralPath $temporary).Length
                $result.download.signature_status = [string]$signature.Status
                $result.download.signer_subject = $signature.SignerCertificate.Subject
                $result.download.file_version = [Diagnostics.FileVersionInfo]::GetVersionInfo($temporary).ProductVersion
                if ($request.bootstrap_package -and $hash -ine $request.bootstrap_package.sha256) { throw 'Prefetched bootstrap SHA-256 does not match the immutable input.' }
                if ($request.case_selection -eq 'remaining-downloads' -and $hash -ine $result.download.input_sha256) { throw 'Package bytes differ from the production HTTP source guest.' }
                if ($case.expected_sha256 -and $hash -ine $case.expected_sha256) { throw 'Official package SHA-256 mismatch.' }
                Assert-MicrosoftSignature $temporary
                [IO.File]::Move($temporary, $package)
                $stage = 'installation'
                $result.installer = Invoke-ProductionScript $case.installer_script 'x64' ($case.id + '-installer')
                if ($result.installer.value.log_path) {
                    $result.diagnostics += Copy-GuestDiagnostic $result.installer.value.log_path ($case.id + '-msi.log')
                }
                if ($result.installer.value.started -and $result.installer.value.exit_code -in @(3010, 1641)) {
                    $result.status = 'reboot_required'
                } elseif ($result.installer.process_exit_code -eq 0 -and $result.installer.value.started -and $result.installer.value.exit_code -eq 0) {
                    $result.status = 'installed_pending_probe'
                } else { throw 'Production installer did not complete successfully; inspect recorded stdout/stderr and diagnostics.' }
            } catch { $result.error = $_.Exception.ToString(); $result.failure_stage = $stage }
            finally {
                try { $result.postprobe = @(Invoke-Probes $case 'postprobe') }
                catch { $result.error = [string]$result.error + '; postprobe: ' + $_.Exception.ToString(); if (!$result.failure_stage) { $result.failure_stage = 'postprobe' } }
                if ($result.status -eq 'installed_pending_probe') {
                    $result.status = if ($result.postprobe.Count -eq $case.probes.Count -and !@($result.postprobe | Where-Object { !$_.ready }).Count) { 'passed' } else { 'failed' }
                    if ($result.status -eq 'failed') { $result.failure_stage = 'postprobe' }
                }
                if ($case.id -eq 'directx-june-2010') {
                    foreach ($log in @('DirectX.log', 'DXError.log')) {
                        $result.diagnostics += Copy-GuestDiagnostic (Join-Path $env:WINDIR $log) ($case.id + '-' + $log)
                    }
                }
                if ($case.id -like 'vc-*') {
                    foreach ($log in Get-ChildItem -LiteralPath $env:TEMP -Filter 'dd_*.log' -File -ErrorAction SilentlyContinue) {
                        if ($log.LastWriteTimeUtc -ge [DateTime]::Parse($result.started_utc).ToUniversalTime()) {
                            $result.diagnostics += Copy-GuestDiagnostic $log.FullName ($case.id + '-' + $log.Name)
                        }
                    }
                }
                $result.finished_utc = [DateTime]::UtcNow.ToString('o')
                $results.Add([pscustomobject]$result)
                $report.cases = @($results.ToArray())
                Write-JsonAtomically (Join-Path $OutputRoot ($case.id + '-result.json')) $result
                Write-JsonAtomically (Join-Path $OutputRoot ('snapshot-' + $results.Count.ToString('D2') + '.json')) $report
            }
        }
        if ($request.case_selection -eq 'download-remaining') {
            if (@($report.cases | Where-Object { $_.status -ne 'passed' }).Count) { throw 'The production HTTP worker requires a successful verified CRT bootstrap.' }
            $nativeEvidence = Join-Path $OutputRoot 'native-http'
            $global:LASTEXITCODE = 0
            $nativeError = $null
            try { & ([ScriptBlock]::Create([IO.File]::ReadAllText('C:\LGSMInput\native-http\worker.ps1', [Text.Encoding]::UTF8))) -OutputDirectory 'C:\LGSMNativeHttpWork\packages' -EvidenceDirectory $nativeEvidence -RunToken $RunToken }
            catch { $nativeError = $_.Exception.ToString() }
            $nativeReportPath = Join-Path $nativeEvidence 'results.json'
            $nativeExitPath = Join-Path $nativeEvidence 'worker-exit.json'
            $nativeExit = if ([IO.File]::Exists($nativeExitPath)) { (Get-Content -LiteralPath $nativeExitPath -Raw -Encoding UTF8 | ConvertFrom-Json).exit_code } else { $null }
            $report.native_http = @{ exit_code = $nativeExit; output_directory = $nativeEvidence; results_sha256 = if ([IO.File]::Exists($nativeReportPath)) { (Get-FileHash -LiteralPath $nativeReportPath -Algorithm SHA256).Hash } else { $null }; error = $nativeError }
            if ($nativeError -or $nativeExit -ne 0) { throw "The production native HTTP worker failed: $nativeError" }
            $null = Assert-NativeHttpArtifacts $nativeEvidence $manifest $null
        }
        $portableWorker = 'C:\LGSMInput\portable\worker.ps1'
        if ([IO.File]::Exists($portableWorker)) {
            $portableOutput = Join-Path $OutputRoot 'portable'
            $null = [IO.Directory]::CreateDirectory($portableOutput)
            $global:LASTEXITCODE = 0
            & ([ScriptBlock]::Create([IO.File]::ReadAllText($portableWorker, [Text.Encoding]::UTF8))) -OutputDirectory $portableOutput -RunToken $RunToken
            $report.portable = @{ exit_code = $LASTEXITCODE; output_directory = $portableOutput }
        }
    } catch { $report.fatal_error = $_.Exception.ToString() }
    finally {
        $report.complete = $true
        $report.finished_utc = [DateTime]::UtcNow.ToString('o')
        Write-JsonAtomically (Join-Path $OutputRoot 'results.json') $report
        $resultHash = (Get-FileHash -LiteralPath (Join-Path $OutputRoot 'results.json') -Algorithm SHA256).Hash
        Write-JsonAtomically (Join-Path $OutputRoot 'complete.json') @{ run_token = $RunToken; finished_utc = $report.finished_utc; results_sha256 = $resultHash }
    }
}

if ($Guest) {
    # Guard before the shutdown finally as well as before every possible install.
    $null = Assert-GuestIdentity
    try { Invoke-GuestMatrix }
    catch {
        Write-JsonAtomically (Join-Path $OutputRoot 'guest-error.json') @{ run_token = $RunToken; error = $_.Exception.ToString(); finished_utc = [DateTime]::UtcNow.ToString('o') }
        throw
    }
    finally { & (Join-Path $env:WINDIR 'System32\shutdown.exe') /s /t 0 }
    return
}

if ($PSCmdlet.ParameterSetName -eq 'Prepare') {
    $hostSource = [bool]$HostDownloadReport
    if ($hostSource -and (!$RemainingDownloads -or !$NativeHttpExportDirectory -or $DownloadedPackagesDirectory)) { throw 'A host download report requires only the remaining cold-install mode and its HTTP export manifest.' }
    if ($BootstrapPackagePath -and !$PortableOnly -and !$NativeHttpExportDirectory) { throw 'A bootstrap package requires a portable or native HTTP worker.' }
    if (($RemainingDownloads -and ($PortableExportDirectory -or $PortableOnly -or (!$hostSource -and $NativeHttpExportDirectory) -or $BootstrapPackagePath)) -or
        ($NativeHttpExportDirectory -and ($PortableExportDirectory -or $PortableOnly)) -or
        ($RemainingDownloads -ne [bool]($DownloadedPackagesDirectory -or $hostSource))) { throw 'Remaining downloads, native HTTP, and portable modes must have separate inputs.' }
    if ($NativeHttpExportDirectory -and !$hostSource -and (!$BootstrapPackagePath -or ![IO.File]::Exists((Join-Path $NativeHttpExportDirectory 'worker.ps1')))) { throw 'The native HTTP phase requires its worker and verified CRT bootstrap.' }
    if ($PortableOnly -and (!$PortableExportDirectory -or
        ![IO.File]::Exists((Join-Path $PortableExportDirectory 'worker.ps1')))) {
        throw '-PortableOnly requires a verified portable export bundle with worker.ps1.'
    }
    $manifestSource = Assert-PlainPath $ManifestPath
    $downloadEvidence = $null
    if ($hostSource) {
        $hostReportPath = Assert-PlainPath $HostDownloadReport
        $httpManifestPath = Assert-PlainPath (Join-Path $NativeHttpExportDirectory 'manifest.json')
        $httpManifest = Get-Content -LiteralPath $httpManifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
        if ($httpManifest.schema_version -ne 1 -or $httpManifest.binary_sha256 -notmatch '^[a-fA-F0-9]{64}$' -or @($httpManifest.cases).Count -ne 3) { throw 'The production HTTP export manifest has no valid binary binding.' }
        $downloadEvidence = @{ source_kind = 'host-production-http'; report_sha256 = (Get-FileHash -LiteralPath $hostReportPath -Algorithm SHA256).Hash; binary_sha256 = $httpManifest.binary_sha256; http_manifest_sha256 = (Get-FileHash -LiteralPath $httpManifestPath -Algorithm SHA256).Hash; provenance = 'Fresh official production HTTP downloads on the host; these exact hash-bound packages are cold-installed in this isolated guest.' }
        $null = Assert-NativeHttpArtifacts (Split-Path -Parent $hostReportPath) (Get-Content -LiteralPath $manifestSource -Raw -Encoding UTF8 | ConvertFrom-Json) $downloadEvidence
    } elseif ($RemainingDownloads) {
        $downloadSource = Assert-PlainPath $DownloadedPackagesDirectory
        $downloadReport = Assert-NativeHttpArtifacts $downloadSource (Get-Content -LiteralPath $manifestSource -Raw -Encoding UTF8 | ConvertFrom-Json) $null
        $sourceOutput = Split-Path -Parent $downloadSource
        $sourceResultsPath = Join-Path $sourceOutput 'results.json'
        $sourceComplete = Get-Content -LiteralPath (Join-Path $sourceOutput 'complete.json') -Raw -Encoding UTF8 | ConvertFrom-Json
        $sourceResults = Get-Content -LiteralPath $sourceResultsPath -Raw -Encoding UTF8 | ConvertFrom-Json
        $sourceRequest = Get-Content -LiteralPath (Join-Path (Split-Path -Parent $sourceOutput) 'prepared.json') -Raw -Encoding UTF8 | ConvertFrom-Json
        $reportHash = (Get-FileHash -LiteralPath (Join-Path $downloadSource 'results.json') -Algorithm SHA256).Hash
        $sha = [Security.Cryptography.SHA256]::Create()
        try { $sourceTokenHash = ([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($sourceComplete.run_token)))).Replace('-', '').ToLowerInvariant() } finally { $sha.Dispose() }
        if (!$sourceResults.complete -or $sourceResults.fatal_error -or $sourceResults.case_selection -cne 'download-remaining' -or
            $sourceComplete.run_token -cne $sourceRequest.run_token -or $sourceResults.run_token -cne $sourceComplete.run_token -or
            $downloadReport.run_token_sha256 -cne $sourceTokenHash -or
            $sourceResults.native_http.exit_code -ne 0 -or $sourceResults.native_http.results_sha256 -ine $reportHash -or
            (Get-FileHash -LiteralPath $sourceResultsPath -Algorithm SHA256).Hash -ine $sourceComplete.results_sha256) { throw 'The source download guest did not complete a bound successful production HTTP run.' }
        $downloadEvidence = @{ source_kind = 'guest-production-http'; report_sha256 = $reportHash; run_token_sha256 = $downloadReport.run_token_sha256; binary_sha256 = $downloadReport.test_binary_sha256; source_results_sha256 = $sourceComplete.results_sha256; provenance = 'Fresh official production HTTP downloads in a separate completed Windows Sandbox guest; these exact hash-bound artifacts are cold-installed here.' }
    }
    $work = Assert-PlainPath $WorkRoot
    $repository = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot)).TrimEnd('\') + '\'
    if (($work.TrimEnd('\') + '\').StartsWith($repository, [StringComparison]::OrdinalIgnoreCase)) { throw 'Sandbox output must remain outside the repository.' }
    $null = [IO.Directory]::CreateDirectory($work)
    $PreparedRoot = Join-Path $work ('windows-runtime-sandbox-' + [Guid]::NewGuid().ToString('N'))
    $inputDirectory = Join-Path $PreparedRoot 'input'
    $outputDirectory = Join-Path $PreparedRoot 'output'
    $null = [IO.Directory]::CreateDirectory($inputDirectory)
    $null = [IO.Directory]::CreateDirectory($outputDirectory)
    Copy-Item -LiteralPath $manifestSource -Destination (Join-Path $inputDirectory 'manifest.json')
    Copy-Item -LiteralPath $PSCommandPath -Destination (Join-Path $inputDirectory 'guest-runner.ps1')
    if ($PortableExportDirectory) {
        $portable = Assert-PlainPath $PortableExportDirectory
        foreach ($item in Get-ChildItem -LiteralPath $portable -Recurse -Force) { $null = Assert-PlainPath $item.FullName }
        Copy-Item -LiteralPath $portable -Destination (Join-Path $inputDirectory 'portable') -Recurse
    }
    foreach ($bundle in @(@{ path = $NativeHttpExportDirectory; name = 'native-http' }, @{ path = $DownloadedPackagesDirectory; name = 'downloaded-packages' })) {
        if ($bundle.path -and !$hostSource) {
            $source = Assert-PlainPath $bundle.path
            foreach ($item in Get-ChildItem -LiteralPath $source -Recurse -Force) { $null = Assert-PlainPath $item.FullName }
            Copy-Item -LiteralPath $source -Destination (Join-Path $inputDirectory $bundle.name) -Recurse
        }
    }
    if ($hostSource) {
        $hostInputs = Join-Path $inputDirectory 'downloaded-packages'
        $null = [IO.Directory]::CreateDirectory($hostInputs)
        Copy-Item -LiteralPath $hostReportPath -Destination (Join-Path $hostInputs 'results.json')
        Copy-Item -LiteralPath $httpManifestPath -Destination (Join-Path $hostInputs 'http-manifest.json')
        foreach ($id in $remainingCaseIds) { Copy-Item -LiteralPath (Assert-PlainPath (Join-Path (Split-Path -Parent $hostReportPath) ($id + '.exe'))) -Destination (Join-Path $hostInputs ($id + '.exe')) }
    }
    $bootstrap = $null
    if ($BootstrapPackagePath) {
        $bootstrapSource = Assert-PlainPath $BootstrapPackagePath
        $bootstrapDirectory = Join-Path $inputDirectory 'bootstrap'
        $null = [IO.Directory]::CreateDirectory($bootstrapDirectory)
        $bootstrapCopy = Join-Path $bootstrapDirectory 'vc-14-x64.exe'
        Copy-Item -LiteralPath $bootstrapSource -Destination $bootstrapCopy
        $bootstrap = @{ relative_path = 'bootstrap\vc-14-x64.exe'; sha256 = (Get-FileHash -LiteralPath $bootstrapCopy -Algorithm SHA256).Hash; bytes = (Get-Item -LiteralPath $bootstrapCopy).Length; provenance = $BootstrapPackageProvenance }
    }
    $token = [Guid]::NewGuid().ToString('N')
    $inputFiles = @(Get-ChildItem -LiteralPath $inputDirectory -Recurse -File | Sort-Object FullName | ForEach-Object {
        @{ relative_path = $_.FullName.Substring($inputDirectory.Length + 1); sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash }
    })
    $selection = if ($RemainingDownloads) { 'remaining-downloads' } elseif ($NativeHttpExportDirectory) { 'download-remaining' } elseif ($PortableOnly) { 'portable-bootstrap' } else { 'full' }
    $request = @{ schema_version = 1; run_token = $token; host_name = $env:COMPUTERNAME; case_selection = $selection; bootstrap_package = $bootstrap; download_evidence = $downloadEvidence; manifest_sha256 = (Get-FileHash -LiteralPath (Join-Path $inputDirectory 'manifest.json') -Algorithm SHA256).Hash; runner_sha256 = (Get-FileHash -LiteralPath (Join-Path $inputDirectory 'guest-runner.ps1') -Algorithm SHA256).Hash; input_files = $inputFiles }
    $bootstrap = '& ([ScriptBlock]::Create([IO.File]::ReadAllText(''C:\LGSMInput\guest-runner.ps1''))) -Guest -ManifestPath ''C:\LGSMInput\manifest.json'' -OutputRoot ''C:\LGSMOutput'' -RunToken ''' + $token + ''''
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($bootstrap))
    $escape = { param($value) [Security.SecurityElement]::Escape($value) }
    $configuration = @"
<Configuration>
  <vGPU>Disable</vGPU><Networking>Enable</Networking><MemoryInMB>4096</MemoryInMB>
  <AudioInput>Disable</AudioInput><VideoInput>Disable</VideoInput><PrinterRedirection>Disable</PrinterRedirection><ClipboardRedirection>Disable</ClipboardRedirection>
  <MappedFolders>
    <MappedFolder><HostFolder>$(& $escape $inputDirectory)</HostFolder><SandboxFolder>C:\LGSMInput</SandboxFolder><ReadOnly>true</ReadOnly></MappedFolder>
    <MappedFolder><HostFolder>$(& $escape $outputDirectory)</HostFolder><SandboxFolder>C:\LGSMOutput</SandboxFolder><ReadOnly>false</ReadOnly></MappedFolder>
  </MappedFolders>
  <LogonCommand><Command>powershell.exe -NoProfile -NonInteractive -EncodedCommand $encoded</Command></LogonCommand>
</Configuration>
"@
    Write-NewUtf8 (Join-Path $PreparedRoot 'runtime-matrix.wsb') $configuration
    $request.configuration_sha256 = (Get-FileHash -LiteralPath (Join-Path $PreparedRoot 'runtime-matrix.wsb') -Algorithm SHA256).Hash
    Write-NewUtf8 (Join-Path $inputDirectory 'run.json') ($request | ConvertTo-Json -Depth 5)
    Write-NewUtf8 (Join-Path $PreparedRoot 'prepared.json') ($request | ConvertTo-Json -Depth 5)
    Write-Output "Prepared Windows runtime sandbox: $PreparedRoot"
    if ($PrepareOnly) { return }
}

$PreparedRoot = Assert-PlainPath $PreparedRoot
$inputDirectory = Join-Path $PreparedRoot 'input'
$outputDirectory = Join-Path $PreparedRoot 'output'
$request = Get-Content -LiteralPath (Join-Path $PreparedRoot 'prepared.json') -Raw | ConvertFrom-Json
if (!(Get-NativeCaseCount $request.case_selection)) { throw 'Unexpected prepared native-case selection.' }
$expectedCaseCount = Get-NativeCaseCount $request.case_selection
$configurationPath = Join-Path $PreparedRoot 'runtime-matrix.wsb'
if ((Get-FileHash -LiteralPath (Join-Path $inputDirectory 'manifest.json') -Algorithm SHA256).Hash -ine $request.manifest_sha256 -or
    (Get-FileHash -LiteralPath (Join-Path $inputDirectory 'guest-runner.ps1') -Algorithm SHA256).Hash -ine $request.runner_sha256 -or
    @(Get-ChildItem -LiteralPath $outputDirectory -Force).Count) { throw 'Prepared input changed or this sandbox run already has evidence; prepare a fresh run.' }
if ((Get-FileHash -LiteralPath $configurationPath -Algorithm SHA256).Hash -ine $request.configuration_sha256 -or
    (Get-FileHash -LiteralPath (Join-Path $inputDirectory 'run.json') -Algorithm SHA256).Hash -ine (Get-FileHash -LiteralPath (Join-Path $PreparedRoot 'prepared.json') -Algorithm SHA256).Hash) {
    throw 'Prepared sandbox configuration or request changed.'
}
foreach ($file in $request.input_files) {
    $path = Assert-PlainPath (Join-Path $inputDirectory $file.relative_path)
    if (!$path.StartsWith($inputDirectory.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase) -or
        (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ine $file.sha256) { throw 'Prepared input bundle changed.' }
}
if (@(Get-ChildItem -LiteralPath $inputDirectory -Recurse -File).Count -ne $request.input_files.Count + 1) { throw 'Unexpected files were added to the prepared input bundle.' }
$sandbox = Join-Path ([Environment]::SystemDirectory) 'WindowsSandbox.exe'
if (![IO.File]::Exists($sandbox)) { throw 'Windows Sandbox is unavailable; the host will not install any runtime.' }
if (Get-Process -Name vmmemWindowsSandbox, WindowsSandboxServer -ErrorAction SilentlyContinue) { throw 'Windows Sandbox is in use; wait for the coordinated guest to finish.' }
Write-NewUtf8 (Join-Path $PreparedRoot 'started.json') (@{run_token=$request.run_token; started_utc=[DateTime]::UtcNow.ToString('o')} | ConvertTo-Json)
$launcher = Start-Process -FilePath $sandbox -ArgumentList ('"' + $configurationPath + '"') -WindowStyle Hidden -PassThru
Write-Output "Windows Sandbox launcher PID: $($launcher.Id); completion is identified by the guest run token."
$deadline = [DateTime]::UtcNow.AddMinutes($TimeoutMinutes)
$lastCount = -1
while (!(Test-Path -LiteralPath (Join-Path $outputDirectory 'complete.json'))) {
    if (Test-Path -LiteralPath (Join-Path $outputDirectory 'guest-error.json')) {
        $guestError = Get-Content -LiteralPath (Join-Path $outputDirectory 'guest-error.json') -Raw -Encoding UTF8 | ConvertFrom-Json
        if ($guestError.run_token -cne $request.run_token) { throw 'Unexpected guest error identity.' }
        throw "Sandbox guest failed before completing its report: $($guestError.error)"
    }
    if ([DateTime]::UtcNow -gt $deadline) { throw "Sandbox deadline exceeded; the guest was left running to preserve installer atomicity. Evidence: $outputDirectory" }
    $snapshotFile = Get-ChildItem -LiteralPath $outputDirectory -Filter 'snapshot-*.json' -File | Sort-Object Name | Select-Object -Last 1
    if ($snapshotFile) {
        $snapshot = Get-Content -LiteralPath $snapshotFile.FullName -Raw -Encoding UTF8 | ConvertFrom-Json
        if ($snapshot.cases.Count -ne $lastCount) {
            foreach ($case in @($snapshot.cases | Select-Object -Skip ([Math]::Max(0, $lastCount)))) { Write-Output ($case.id + ': ' + $case.status) }
            $lastCount = $snapshot.cases.Count
        }
    }
    Start-Sleep -Seconds 2
}
$complete = Get-Content -LiteralPath (Join-Path $outputDirectory 'complete.json') -Raw | ConvertFrom-Json
if ($complete.run_token -cne $request.run_token) { throw 'Unexpected completion identity.' }
if ((Get-FileHash -LiteralPath (Join-Path $outputDirectory 'results.json') -Algorithm SHA256).Hash -ine $complete.results_sha256) { throw 'Final guest report does not match its completion marker.' }
$report = Get-Content -LiteralPath (Join-Path $outputDirectory 'results.json') -Raw -Encoding UTF8 | ConvertFrom-Json
Write-Output "Windows runtime sandbox results: $(Join-Path $outputDirectory 'results.json')"
if (!$report.complete -or $report.fatal_error -or $report.case_selection -cne $request.case_selection -or
    $report.expected_native_case_count -ne $expectedCaseCount -or $report.cases.Count -ne $expectedCaseCount -or
    @($report.cases | Where-Object { $_.status -ne 'passed' }).Count -or
    ($report.portable -and $report.portable.exit_code -ne 0) -or
    ($request.case_selection -eq 'portable-bootstrap' -and !$report.portable) -or
    ($request.case_selection -eq 'download-remaining' -and (!$report.native_http -or $report.native_http.exit_code -ne 0))) { throw 'Sandbox matrix contains failed, incomplete, or reboot-required cases; inspect the structured results.' }
