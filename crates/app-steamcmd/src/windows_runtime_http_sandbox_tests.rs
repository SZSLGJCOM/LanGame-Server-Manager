//! Guest-only production HTTP downloads, exported separately from cold installs.
//! The three packages must be downloaded afresh; no installer executes here.
use super::*;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};

#[path = "windows_runtime_http_diagnostic_tests.rs"]
mod diagnostics;

const INPUT: &str = r"C:\LGSMInput\native-http";
const EVIDENCE: &str = r"C:\LGSMOutput\native-http";
const WORK: &str = r"C:\LGSMNativeHttpWork";
const PACKAGES: &str = r"C:\LGSMNativeHttpWork\packages";
const FILTER: &str = "windows_runtime_setup::tests::http_sandbox::native_http_sandbox_worker";
const IDS: [&str; 3] = ["vc-14-x64", "vc-14-x86", "vc-2013-x86"];

fn packages() -> [RuntimePackage; 3] {
    [
        catalog::package(Family::VisualCpp2015, Architecture::X64),
        catalog::package(Family::VisualCpp2015, Architecture::X86),
        catalog::package(Family::VisualCpp2013, Architecture::X86),
    ]
}

#[test]
#[ignore = "exports a guest-only production HTTP worker; no host download or installation"]
fn export_native_http_sandbox_bundle() -> Result<(), Box<dyn std::error::Error>> {
    let destination = std::env::var_os("LANGAME_NATIVE_HTTP_SANDBOX_EXPORT")
        .map(PathBuf::from)
        .ok_or("Set LANGAME_NATIVE_HTTP_SANDBOX_EXPORT to a fresh task work directory")?;
    validate_cache_path(&destination)?;
    if destination.exists() && fs::read_dir(&destination)?.next().is_some() {
        return Err(
            "Native HTTP export destination must be empty; existing files preserved".into(),
        );
    }
    fs::create_dir_all(&destination)?;
    let binary = destination.join("app-steamcmd-native-http-tests.exe");
    fs::copy(std::env::current_exe()?, &binary)?;
    let identity = windows_identity()?;
    let cases = packages().iter().map(|package| serde_json::json!({
        "id":package.id,"name":package.name,"official_url":package.url,
        "expected_sha256":package.sha256,"package_path":package_path(Path::new(PACKAGES),*package),
        "artifact_name":format!("{}.exe",package.id)
    })).collect::<Vec<_>>();
    let manifest = serde_json::json!({
        "schema_version":1,"scope":"production HTTP download and Microsoft package verification; installers run separately in a fresh VM",
        "binary":"app-steamcmd-native-http-tests.exe","binary_sha256":file_digest(&binary)?,
        "host_computer":identity["computer"],"worker_filter":FILTER,"cases":cases,
        "guest_input":INPUT,"guest_evidence":EVIDENCE,"guest_work":WORK,"guest_packages":PACKAGES,
        "bootstrap":"The Rust test binary requires VC14 x64. VM A may bootstrap from a previously verified package; all three packages below must then be downloaded afresh by production HTTP. VM B cold-installs only those new artifacts."
    });
    fs::write(
        destination.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    fs::write(destination.join("worker.ps1"), WORKER_SCRIPT)?;
    println!("NATIVE_HTTP_SANDBOX_EXPORT {}", destination.display());
    Ok(())
}

#[tokio::test]
#[ignore = "Windows Sandbox guest only: real official production HTTP downloads and signature verification, without installation"]
async fn native_http_sandbox_worker() -> Result<(), Box<dyn std::error::Error>> {
    let identity = windows_identity()?;
    let account = identity["user"]
        .as_str()
        .unwrap_or_default()
        .rsplit('\\')
        .next()
        .unwrap_or_default();
    if !account.eq_ignore_ascii_case("WDAGUtilityAccount") || identity["administrator"] != true {
        return Err(
            "Native HTTP worker requires the real Windows Sandbox administrator identity".into(),
        );
    }
    let token = std::env::var("LANGAME_NATIVE_HTTP_SANDBOX_TOKEN")?;
    if token.len() != 32 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Native HTTP worker requires the caller's 32-digit one-run token".into());
    }
    let manifest = read_json(&Path::new(INPUT).join("manifest.json"))?;
    let request = read_json(Path::new(r"C:\LGSMInput\run.json"))?;
    if request["schema_version"] != 1 || request["run_token"] != token {
        return Err("Native HTTP token does not match the read-only caller request".into());
    }
    let computer = identity["computer"]
        .as_str()
        .ok_or("No actual guest computer name")?;
    for host in [&request["host_name"], &manifest["host_computer"]] {
        if host
            .as_str()
            .is_some_and(|host| host.eq_ignore_ascii_case(computer))
        {
            return Err("Native HTTP worker refuses the host computer".into());
        }
    }
    let binary_sha256 = file_digest(&std::env::current_exe()?)?;
    if manifest["binary_sha256"] != binary_sha256 {
        return Err("Running native HTTP test binary does not match the exported artifact".into());
    }
    let work = Path::new(WORK);
    let session = read_json(&work.join("session.json"))?;
    if session["run_token"] != token || session["computer"] != identity["computer"] {
        return Err("Native HTTP session does not match this guest and run token".into());
    }
    let mut consumed = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(work.join("token-consumed"))?;
    consumed.write_all(token.as_bytes())?;
    consumed.sync_all()?;
    let evidence = Path::new(EVIDENCE);
    validate_cache_path(evidence)?;
    let artifacts = evidence.join("packages");
    fs::create_dir_all(&artifacts)?;
    let root = Path::new(PACKAGES);
    validate_cache_path(root)?;
    if root.exists() {
        return Err(
            "Native HTTP download directory already exists; preserve it and use a fresh guest"
                .into(),
        );
    }
    fs::create_dir_all(root)?;
    let mut events = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(evidence.join("events.jsonl"))?;
    let mut cases = Vec::new();
    for package in packages() {
        let started = std::time::Instant::now();
        let cancellation = InstallCancellation::new();
        let deadline = InstallDeadline::new(
            "Sandbox production native HTTP download",
            Duration::from_secs(180),
        );
        let path = package_path(root, package);
        let artifact = artifacts.join(format!("{}.exe", package.id));
        let mut observed_bytes = 0;
        let mut progress_error = None;
        let mut routes = Vec::new();
        let result = cancellation.scope(async {
            // Intentionally bypass every cache decision: the production helper
            // creates a new staging file and always requests the official URL.
            let downloaded = http_download::download_native_package_with_progress(package.url, &path,
                DownloadIntegrity {sha256:package.sha256,..Default::default()},deadline,|event| {
                    let (bytes, total) = match event {
                        http_download::NativeDownloadEvent::Route { ipv4, origin } => {
                            let route = serde_json::json!({"ipv4":ipv4,"origin":origin});
                            if let Err(error) = writeln!(events,"{}",serde_json::json!({"case":package.id,"route":route})).and_then(|_|events.flush()) {
                                progress_error=Some(error.to_string()); cancellation.cancel();
                            }
                            routes.push(route);
                            return;
                        },
                        http_download::NativeDownloadEvent::Progress { bytes, total } => (bytes, total),
                    };
                    observed_bytes = observed_bytes.max(bytes);
                    let event = serde_json::json!({"case":package.id,"official_url":package.url,"downloaded_bytes":bytes,"total_bytes":total});
                    if let Err(error) = writeln!(events,"{event}").and_then(|_|events.flush()) {
                        progress_error=Some(error.to_string()); cancellation.cancel();
                    }
                }).await?;
            if let Some(error) = &progress_error { return Err(runtime_error(format!("Native HTTP progress evidence failed: {error}"))); }
            process::verify_package(downloaded.path(),package,deadline).await?;
            deadline.check_cancelled()?;
            let bytes = fs::metadata(downloaded.path()).map_err(|error|runtime_error(error.to_string()))?.len();
            if observed_bytes == 0 || bytes == 0 {
                return Err(runtime_error("The fresh native HTTP download did not record any package bytes"));
            }
            let sha256 = file_digest(downloaded.path()).map_err(|error|runtime_error(error.to_string()))?;
            downloaded.record_success();
            downloaded.persist(&path)?;
            let mut source = fs::File::open(&path).map_err(|error|runtime_error(error.to_string()))?;
            let mut target = fs::OpenOptions::new().write(true).create_new(true).open(&artifact).map_err(|error|runtime_error(error.to_string()))?;
            std::io::copy(&mut source,&mut target).map_err(|error|runtime_error(error.to_string()))?;
            target.sync_all().map_err(|error|runtime_error(error.to_string()))?;
            drop(target);
            if file_digest(&artifact).map_err(|error|runtime_error(error.to_string()))? != sha256 {
                return Err(runtime_error("Exported native HTTP artifact hash differs from the verified download"));
            }
            deadline.check_cancelled()?;
            Ok::<_,SteamCmdError>(serde_json::json!({"sha256":sha256,"bytes":bytes,"signature":"production process::verify_package succeeded"}))
        }).await;
        let (status, result, error) = match result {
            Ok(result) => ("passed", Some(result), None),
            Err(error) => ("failed", None, Some(error.to_string())),
        };
        let sha256 = result
            .as_ref()
            .and_then(|value| value.get("sha256"))
            .cloned();
        let downloaded_bytes = result
            .as_ref()
            .and_then(|value| value.get("bytes"))
            .cloned();
        cases.push(serde_json::json!({"id":package.id,"url":package.url,"path":path,"export_path":artifact,"sha256":sha256,"downloaded_bytes":downloaded_bytes,
            "official_url":package.url,"package_path":path,"artifact_path":artifact,"export_relative_path":format!("packages/{}.exe",package.id),
            "routes":routes,"observed_downloaded_bytes":observed_bytes,"elapsed_ms":started.elapsed().as_millis(),"status":status,"download":result,"error":error}));
        let report = serde_json::json!({"schema_version":1,"scope":"fresh official production HTTP downloads and signature verification; no installation",
            "guest":identity,"run_token_sha256":bytes_digest(token.as_bytes()),"test_binary_sha256":binary_sha256,"cases":cases});
        fs::write(
            evidence.join("results.json"),
            serde_json::to_vec_pretty(&report)?,
        )?;
    }
    if cases.iter().any(|case| case["status"] != "passed") {
        return Err(
            "One or more production native HTTP downloads failed; inspect results.json".into(),
        );
    }
    Ok(())
}

#[test]
fn native_http_cases_match_production_catalog() {
    assert_eq!(packages().map(|package| package.id), IDS);
    for package in packages() {
        assert!(package.url.starts_with("https://"));
        assert_eq!(package.kind, catalog::InstallerKind::VisualCpp);
        assert_eq!(
            package_path(Path::new(PACKAGES), package)
                .file_name()
                .and_then(|name| name.to_str()),
            Some(format!("{}.exe", package.id).as_str())
        );
    }
}

#[test]
fn native_http_runner_is_valid_windows_powershell() -> Result<(), Box<dyn std::error::Error>> {
    let script = format!(
        "$tokens=$null; $errors=$null; $null=[Management.Automation.Language.Parser]::ParseInput('{}',[ref]$tokens,[ref]$errors); if ($errors.Count -gt 0) {{ $errors|ForEach-Object {{ Write-Output $_.Message }}; exit 1 }}",
        WORKER_SCRIPT.replace('\'', "''")
    );
    let powershell =
        app_runtime::windows_system_directory()?.join("WindowsPowerShell/v1.0/powershell.exe");
    let result = app_runtime::capture_windows_utility(
        &powershell,
        &["-NoProfile", "-NonInteractive", "-Command", &script],
        Duration::from_secs(15),
    )?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    Ok(())
}

fn read_json(path: &Path) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(65_537).read_to_end(&mut bytes)?;
    if bytes.len() > 65_536 {
        return Err("Native HTTP evidence metadata exceeds 64 KiB".into());
    }
    Ok(serde_json::from_slice(&bytes)?)
}

fn windows_identity() -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let powershell =
        app_runtime::windows_system_directory()?.join("WindowsPowerShell/v1.0/powershell.exe");
    let script = "$id=[Security.Principal.WindowsIdentity]::GetCurrent(); $p=New-Object Security.Principal.WindowsPrincipal($id); @{user=$id.Name;administrator=$p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator);computer=[Net.Dns]::GetHostName()}|ConvertTo-Json -Compress";
    let result = app_runtime::capture_windows_utility(
        &powershell,
        &["-NoProfile", "-NonInteractive", "-Command", script],
        Duration::from_secs(15),
    )?;
    if !result.status.success() {
        return Err("Cannot verify actual Windows identity".into());
    }
    Ok(serde_json::from_slice(&result.stdout)?)
}

fn file_digest(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65_536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn bytes_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

const WORKER_SCRIPT: &str = r#"param([Parameter(Mandatory=$true)][string]$OutputDirectory,[Parameter(Mandatory=$true)][string]$EvidenceDirectory,[Parameter(Mandatory=$true)][string]$RunToken)
$ErrorActionPreference='Stop'
$utf8=New-Object Text.UTF8Encoding($false)
$identity=[Security.Principal.WindowsIdentity]::GetCurrent()
$principal=New-Object Security.Principal.WindowsPrincipal($identity)
if (($identity.Name.Split('\')[-1] -ine 'WDAGUtilityAccount') -or -not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Native HTTP worker requires the real Windows Sandbox administrator identity.' }
if ([IO.Path]::GetFullPath($OutputDirectory).TrimEnd('\') -cne 'C:\LGSMNativeHttpWork\packages' -or [IO.Path]::GetFullPath($EvidenceDirectory).TrimEnd('\') -cne 'C:\LGSMOutput\native-http') { throw 'Native HTTP worker requires its fixed guest package and evidence paths.' }
if ($RunToken -notmatch '^[0-9a-fA-F]{32}$') { throw 'A one-run caller token is required.' }
$inputRoot='C:\LGSMInput\native-http'
$manifest=[IO.File]::ReadAllText((Join-Path $inputRoot 'manifest.json'),[Text.Encoding]::UTF8)|ConvertFrom-Json
$request=[IO.File]::ReadAllText('C:\LGSMInput\run.json',[Text.Encoding]::UTF8)|ConvertFrom-Json
if ($request.schema_version -ne 1 -or $request.run_token -cne $RunToken -or $request.host_name -ieq [Net.Dns]::GetHostName() -or $manifest.host_computer -ieq [Net.Dns]::GetHostName()) { throw 'Native HTTP worker token or guest does not match the read-only caller request.' }
$work='C:\LGSMNativeHttpWork'
if (Test-Path -LiteralPath $work) { throw 'Native HTTP work already exists; use a fresh Sandbox session.' }
New-Item -ItemType Directory -Path $work|Out-Null
New-Item -ItemType Directory -Path $EvidenceDirectory -Force|Out-Null
New-Item -ItemType Directory -Path (Join-Path $work 'temp')|Out-Null
[IO.File]::WriteAllText((Join-Path $work 'session.json'),(@{run_token=$RunToken;computer=[Net.Dns]::GetHostName()}|ConvertTo-Json -Compress),$utf8)
$binary=Join-Path $inputRoot $manifest.binary
if ((Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash -ine $manifest.binary_sha256) { throw 'Native HTTP test binary hash mismatch.' }
$env:TEMP=Join-Path $work 'temp';$env:TMP=$env:TEMP
$bootstrap=Start-Process -FilePath $binary -ArgumentList @('--list') -Wait -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $EvidenceDirectory 'bootstrap.stdout.log') -RedirectStandardError (Join-Path $EvidenceDirectory 'bootstrap.stderr.log')
if ($bootstrap.ExitCode -ne 0) { throw 'Native HTTP test binary cannot load; VM A requires the verified VC14 x64 bootstrap.' }
$listed=[IO.File]::ReadAllText((Join-Path $EvidenceDirectory 'bootstrap.stdout.log'),[Text.Encoding]::UTF8)
if (-not [regex]::IsMatch($listed,('(?m)^'+[regex]::Escape($manifest.worker_filter)+': test\r?$'))) { throw 'The exported test binary does not list the exact native HTTP worker.' }
$env:LANGAME_NATIVE_HTTP_SANDBOX_TOKEN=$RunToken
$worker=Start-Process -FilePath $binary -ArgumentList @($manifest.worker_filter,'--exact','--ignored','--nocapture','--test-threads=1') -Wait -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $EvidenceDirectory 'worker.stdout.log') -RedirectStandardError (Join-Path $EvidenceDirectory 'worker.stderr.log')
[IO.File]::WriteAllText((Join-Path $EvidenceDirectory 'worker-exit.json'),(@{exit_code=$worker.ExitCode;binary_sha256=$manifest.binary_sha256}|ConvertTo-Json),$utf8)
$global:LASTEXITCODE=$worker.ExitCode
if ($worker.ExitCode -ne 0) { throw "Native HTTP production worker failed with exit $($worker.ExitCode); inspect results.json." }
$report=[IO.File]::ReadAllText((Join-Path $EvidenceDirectory 'results.json'),[Text.Encoding]::UTF8)|ConvertFrom-Json
$sha=[Security.Cryptography.SHA256]::Create()
try { $tokenDigest=([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($RunToken)))).Replace('-','').ToLowerInvariant() } finally { $sha.Dispose() }
if ($report.test_binary_sha256 -ine $manifest.binary_sha256 -or $report.run_token_sha256 -cne $tokenDigest -or $report.guest.computer -ine [Net.Dns]::GetHostName()) { throw 'Native HTTP evidence is not bound to this binary, guest and run token.' }
if (@($report.cases).Count -ne 3 -or @($manifest.cases).Count -ne 3) { throw 'Native HTTP evidence must contain exactly three cases.' }
foreach ($expected in $manifest.cases) {
  $rows=@($report.cases|Where-Object {$_.id -ceq $expected.id})
  if ($rows.Count -ne 1 -or $rows[0].status -cne 'passed' -or $rows[0].official_url -cne $expected.official_url -or $rows[0].observed_downloaded_bytes -le 0 -or $rows[0].download.bytes -le 0) { throw "Native HTTP case $($expected.id) did not prove a fresh production download exactly once." }
  $artifact=Join-Path (Join-Path $EvidenceDirectory 'packages') $expected.artifact_name
  if ((Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash -ine $rows[0].download.sha256 -or (Get-Item -LiteralPath $artifact).Length -ne $rows[0].download.bytes) { throw 'Native HTTP exported package does not match its verified download.' }
}
[pscustomobject]@{exit_code=$worker.ExitCode;evidence=$EvidenceDirectory;packages=(Join-Path $EvidenceDirectory 'packages')}
"#;
