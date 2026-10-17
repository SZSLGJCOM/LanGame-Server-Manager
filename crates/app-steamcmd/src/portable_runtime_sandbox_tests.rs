//! Opt-in Windows Sandbox evidence for the production portable-runtime API.
//! Uses synthetic game requirement metadata, never proprietary game programs.
use super::*;
use sha2::Sha256;

const INPUT: &str = r"C:\LGSMInput\portable";
const OUTPUT: &str = r"C:\LGSMOutput\portable";
const WORK: &str = r"C:\LGSMPortableRuntimeWork";
const WORKER_FILTER: &str = "portable_runtime::sandbox_tests::portable_runtime_sandbox_worker";

#[test]
#[ignore = "exports the current test binary and guest-only runner to LANGAME_PORTABLE_SANDBOX_EXPORT; does not launch a VM or download/install anything"]
fn export_portable_runtime_sandbox_bundle() -> Result<(), Box<dyn std::error::Error>> {
    let destination = env::var_os("LANGAME_PORTABLE_SANDBOX_EXPORT")
        .map(PathBuf::from)
        .ok_or(
            "Set LANGAME_PORTABLE_SANDBOX_EXPORT to a new directory in this task's work directory",
        )?;
    install_resources::canonical_resource_path(&destination)?;
    if destination.exists() && fs::read_dir(&destination)?.next().is_some() {
        return Err(
            "Portable Sandbox export destination must be empty; existing files were preserved"
                .into(),
        );
    }
    fs::create_dir_all(&destination)?;
    let fixture_check = validate_requirement_fixtures(&destination.join("fixture-preflight"))?;
    fs::write(
        destination.join("fixture-preflight.json"),
        serde_json::to_vec_pretty(&fixture_check)?,
    )?;
    let source = env::current_exe()?;
    let binary = destination.join("app-steamcmd-sandbox-tests.exe");
    fs::copy(&source, &binary)?;
    let requirements = app_runtime::inspect_windows_runtime_requirements(&binary)?;
    let requirements = requirements
        .iter()
        .map(|required| {
            serde_json::json!({
                "family":format!("{:?}", required.family),
                "architecture":format!("{:?}", required.architecture),
                "imported_dlls":required.imported_dlls,
            })
        })
        .collect::<Vec<_>>();
    let identity = windows_identity()?;
    let manifest = serde_json::json!({
        "schema_version":1,
        "scope":"production portable runtime preparation, not game launch",
        "binary":"app-steamcmd-sandbox-tests.exe", "binary_sha256":file_digest(&binary)?,
        "host_computer":identity["computer"], "worker_filter":WORKER_FILTER,
        "guest_input":INPUT, "guest_output":OUTPUT, "guest_work":WORK,
        "native_runtime_requirements":requirements,
        "bootstrap":"Run the native Sandbox VC14 x64 case before this runner. The runner checks the actual test-binary --list exit code before running the worker.",
        "cases":["minecraft-java25-fresh", "necesse-java17-release-repair", "projectzomboid-java25-cache-repair", "romestead-dotnet8-fresh"],
        "game_inputs":"Synthetic local metadata and a non-executable PE-header fixture; no proprietary game assets.",
    });
    fs::write(
        destination.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    fs::write(destination.join("worker.ps1"), WORKER_SCRIPT)?;
    println!("PORTABLE_SANDBOX_EXPORT {}", destination.display());
    Ok(())
}

#[tokio::test]
#[ignore = "guest-only official Java/.NET download, verification, extraction, publication and health probes; use the exported Windows Sandbox runner"]
async fn portable_runtime_sandbox_worker() -> Result<(), Box<dyn std::error::Error>> {
    let identity = windows_identity()?;
    let account = identity["user"]
        .as_str()
        .unwrap_or_default()
        .rsplit('\\')
        .next()
        .unwrap_or_default();
    if !account.eq_ignore_ascii_case("WDAGUtilityAccount") || identity["administrator"] != true {
        return Err(
            "The portable runtime worker requires the real Windows Sandbox administrator identity"
                .into(),
        );
    }
    let manifest = read_json(&Path::new(INPUT).join("manifest.json"))?;
    if manifest["host_computer"] == identity["computer"] {
        return Err("The portable runtime worker refuses the exporting host".into());
    }
    let token = env::var("LANGAME_PORTABLE_SANDBOX_TOKEN")?;
    if !(32..=128).contains(&token.len()) || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("A caller-provided one-run hexadecimal Sandbox token is required".into());
    }
    let request = read_json(Path::new(r"C:\LGSMInput\run.json"))?;
    if request["schema_version"] != 1 || request["run_token"] != token {
        return Err(
            "The portable token does not match the caller's read-only Sandbox request".into(),
        );
    }
    if request["host_name"].as_str().is_some_and(|host| {
        host.eq_ignore_ascii_case(identity["computer"].as_str().unwrap_or_default())
    }) {
        return Err("The portable worker refuses the caller's host computer".into());
    }
    if file_digest(&env::current_exe()?)?
        != manifest["binary_sha256"]
            .as_str()
            .ok_or("Export manifest has no test-binary digest")?
    {
        return Err(
            "The running guest test binary does not match the exported artifact digest".into(),
        );
    }
    let work = Path::new(WORK);
    let session = read_json(&work.join("session.json"))?;
    if session["run_token"] != token || session["computer"] != identity["computer"] {
        return Err(
            "The Sandbox session token or computer does not match the caller session".into(),
        );
    }
    let mut consumed = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(work.join("token-consumed"))?;
    consumed.write_all(token.as_bytes())?;
    consumed.sync_all()?;
    let output = Path::new(OUTPUT);
    fs::create_dir_all(output)?;
    let mut events = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output.join("events.jsonl"))?;
    let settings = AppSettings {
        steamcmd_root: work
            .join("tools with space/steamcmd")
            .to_string_lossy()
            .into_owned(),
        games_root: work.join("unused-games").to_string_lossy().into_owned(),
        servers_root: work.join("unused-instances").to_string_lossy().into_owned(),
        ..Default::default()
    };
    let locks = InstallLockContext::new(work.join("lock-namespace"));
    let mut cases = Vec::new();
    let mut java25 = None;
    for (id, module, major, fresh) in [
        ("minecraft-java25-fresh", "minecraft", Some(25u16), true),
        (
            "necesse-java17-release-repair",
            "necesse",
            Some(17u16),
            true,
        ),
        (
            "projectzomboid-java25-cache-repair",
            "projectzomboid",
            Some(25u16),
            false,
        ),
        ("romestead-dotnet8-fresh", "romestead", None, true),
    ] {
        let install = work.join("requirements").join(id);
        fs::create_dir_all(&install)?;
        let executables = requirement_fixture(&install, module, major)?;
        let started = std::time::Instant::now();
        let cancellation = InstallCancellation::new();
        let mut progress_error = None;
        let mut downloaded_bytes = 0;
        let mut saw_download = false;
        let mut saw_offline_reuse = false;
        let result = prepare_portable_runtime(&locks, &settings, module, &install, &executables,
            cancellation.clone(), |update| {
                let phase = update.install_progress.as_ref();
                if phase.is_some_and(|phase| phase.phase == InstallPhase::Downloading) { saw_download = true; }
                downloaded_bytes = downloaded_bytes.max(phase.and_then(|phase| phase.downloaded_bytes).unwrap_or(0));
                saw_offline_reuse |= update.detail.contains("reusing the verified installed runtime");
                let event = serde_json::json!({"case":id,"detail":update.detail,"progress":update.install_progress});
                if let Err(error) = writeln!(events, "{event}").and_then(|_| events.flush()) {
                    progress_error = Some(error.to_string()); cancellation.cancel();
                }
            }).await;
        let evaluated = async {
            if let Some(error) = progress_error { return Err(format!("Evidence output failed: {error}")); }
            let prepared = result.map_err(|error| error.to_string())?.ok_or("Production API returned no prepared runtime")?;
            if fresh && (!saw_download || downloaded_bytes == 0 || saw_offline_reuse) {
                return Err("Fresh official preparation did not prove an actual package download".into());
            }
            if !fresh && (saw_download || prepared.executable != java25) {
                return Err("The PZ repair did not reuse the already verified Java 25 package".into());
            }
            if module == "minecraft" { java25 = prepared.executable.clone(); }
            let artifact_root = if let Some(executable) = &prepared.executable {
                executable.parent().and_then(Path::parent).ok_or("Java executable has no runtime root")?.to_owned()
            } else {
                PathBuf::from(prepared.environment.get("DOTNET_ROOT_X64").ok_or("Prepared .NET root is missing")?)
            };
            let artifact = read_json(&artifact_root.join(MARKER)).map_err(|error| error.to_string())?;
            let health = additional_health_evidence(&prepared).await.map_err(|error| error.to_string())?;
            Ok::<_, String>(serde_json::json!({"runtime":artifact,"executable":prepared.executable,"environment":prepared.environment,"health":health}))
        }.await;
        let (status, evidence, error) = match evaluated {
            Ok(evidence) => ("passed", Some(evidence), None),
            Err(error) => ("failed", None, Some(error)),
        };
        cases.push(serde_json::json!({"id":id,"module":module,"status":status,"downloaded_bytes":downloaded_bytes,
            "saw_download":saw_download,"elapsed_ms":started.elapsed().as_millis(),"evidence":evidence,"error":error}));
        let report = serde_json::json!({"schema_version":1,"scope":"production portable runtime preparation; synthetic game requirements; no game launch",
            "guest":identity,"run_token_sha256":bytes_digest(token.as_bytes()),"test_binary_sha256":file_digest(&env::current_exe()?)?,"cases":cases});
        fs::write(
            output.join("results.json"),
            serde_json::to_vec_pretty(&report)?,
        )?;
    }
    if cases.iter().any(|case| case["status"] != "passed") {
        return Err(
            "One or more production portable runtime Sandbox cases failed; inspect results.json"
                .into(),
        );
    }
    Ok(())
}

fn requirement_fixture(
    install: &Path,
    module: &str,
    major: Option<u16>,
) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    if module == "minecraft" {
        fs::create_dir_all(install.join(".langame"))?;
        fs::write(install.join(".langame/minecraft-server.json"), serde_json::json!({"version_id":"installed-game-requirement-fixture","required_java_major":major}).to_string())?;
        return Ok(Vec::new());
    }
    if matches!(module, "necesse" | "projectzomboid") {
        let root = install.join(if module == "necesse" { "jre" } else { "jre64" });
        fs::create_dir_all(&root)?;
        fs::write(
            root.join("release"),
            format!(
                "JAVA_VERSION=\"{}.0.1\"\nOS_ARCH=\"amd64\"\nOS_NAME=\"Windows\"\n",
                major.ok_or("Missing fixture Java major")?
            ),
        )?;
        return Ok(Vec::new());
    }
    // Only the bounded PE architecture reader consumes this synthetic header.
    // It is never a game executable and is never spawned.
    let executable = install.join("Server.exe");
    let mut pe = vec![0u8; 70];
    pe[..2].copy_from_slice(b"MZ");
    pe[60..64].copy_from_slice(&64u32.to_le_bytes());
    pe[64..68].copy_from_slice(b"PE\0\0");
    pe[68..70].copy_from_slice(&0x8664u16.to_le_bytes());
    fs::write(&executable, pe)?;
    fs::write(executable.with_extension("runtimeconfig.json"), serde_json::json!({"runtimeOptions":{"framework":{"name":"Microsoft.NETCore.App","version":"8.0.0"}}}).to_string())?;
    Ok(vec![executable])
}

fn validate_requirement_fixtures(
    root: &Path,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let mut checked = Vec::new();
    for (module, major) in [("necesse", 17u16), ("projectzomboid", 25u16)] {
        let install = root.join(module);
        requirement_fixture(&install, module, Some(major))?;
        let bundled = install.join(if module == "necesse" { "jre" } else { "jre64" });
        let actual = java::bundled_major(&bundled)?;
        if actual != major {
            return Err("The Sandbox release fixture does not match the production parser".into());
        }
        checked.push(serde_json::json!({"module":module,"required_java_major":actual}));
    }
    let install = root.join("romestead");
    fs::create_dir_all(&install)?;
    let executables = requirement_fixture(&install, "romestead", None)?;
    if dotnet::discover(&executables)?.is_none() {
        return Err(
            "The Sandbox .NET fixture did not produce a production runtime requirement".into(),
        );
    }
    Ok(
        serde_json::json!({"java_release_fixtures":checked,"dotnet_requirement":"Microsoft.NETCore.App 8.0.0"}),
    )
}

#[test]
fn portable_sandbox_requirement_fixtures_match_production_parsers()
-> Result<(), Box<dyn std::error::Error>> {
    let root = crate::tests::unique_test_root();
    let result = validate_requirement_fixtures(&root);
    assert_eq!(root.parent(), Some(env::temp_dir().as_path()));
    fs::remove_dir_all(&root)?;
    assert_eq!(
        result?["java_release_fixtures"].as_array().map(Vec::len),
        Some(2)
    );
    Ok(())
}

#[test]
fn portable_sandbox_runner_is_valid_windows_powershell() -> Result<(), Box<dyn std::error::Error>> {
    let powershell =
        app_runtime::windows_system_directory()?.join("WindowsPowerShell/v1.0/powershell.exe");
    let parser = format!(
        "$tokens=$null; $errors=$null; $null=[Management.Automation.Language.Parser]::ParseInput('{}',[ref]$tokens,[ref]$errors); if ($errors.Count -gt 0) {{ $errors|ForEach-Object {{ Write-Output $_.Message }}; exit 1 }}",
        WORKER_SCRIPT.replace('\'', "''")
    );
    let output = app_runtime::capture_windows_utility(
        &powershell,
        &["-NoProfile", "-NonInteractive", "-Command", &parser],
        Duration::from_secs(15),
    )?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    Ok(())
}

async fn additional_health_evidence(
    prepared: &PortableRuntime,
) -> Result<serde_json::Value, SteamCmdError> {
    let deadline = InstallDeadline::new("Sandbox runtime output evidence", Duration::from_secs(30));
    let mut command = if let Some(java) = &prepared.executable {
        let mut command = Command::new(java);
        command.arg("-version");
        command
    } else {
        let root = prepared
            .environment
            .get("DOTNET_ROOT_X64")
            .ok_or_else(|| preparation_error("Prepared .NET root is absent"))?;
        let mut command = Command::new(Path::new(root).join("dotnet.exe"));
        command.arg("--list-runtimes");
        command
    };
    command.envs(&prepared.environment);
    let output = run_command_capture(command, deadline).await?;
    if !output.status.success() {
        return Err(preparation_error(
            "The additional Sandbox runtime evidence command failed",
        ));
    }
    Ok(
        serde_json::json!({"exit_code":output.status.code(),"stdout":String::from_utf8_lossy(&output.stdout),"stderr":String::from_utf8_lossy(&output.stderr),
        "managed_execution":"For .NET, the production prepare API already required its own compiled managed payload to execute successfully; --list-runtimes here records only the selected version."}),
    )
}

fn windows_identity() -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let powershell =
        app_runtime::windows_system_directory()?.join("WindowsPowerShell/v1.0/powershell.exe");
    let script = "$id=[Security.Principal.WindowsIdentity]::GetCurrent(); $p=New-Object Security.Principal.WindowsPrincipal($id); @{user=$id.Name;administrator=$p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator);computer=[Net.Dns]::GetHostName()}|ConvertTo-Json -Compress";
    let output = app_runtime::capture_windows_utility(
        &powershell,
        &["-NoProfile", "-NonInteractive", "-Command", script],
        Duration::from_secs(15),
    )?;
    if !output.status.success() {
        return Err("Cannot verify the real Windows identity".into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn file_digest(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher
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

const WORKER_SCRIPT: &str = r#"param([Parameter(Mandatory=$true)][string]$OutputDirectory,[Parameter(Mandatory=$true)][string]$RunToken)
$ErrorActionPreference='Stop'
$utf8=New-Object Text.UTF8Encoding($false)
$identity=[Security.Principal.WindowsIdentity]::GetCurrent()
$principal=New-Object Security.Principal.WindowsPrincipal($identity)
if (($identity.Name.Split('\')[-1] -ine 'WDAGUtilityAccount') -or -not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Portable worker requires the real Windows Sandbox administrator identity.' }
if ([IO.Path]::GetFullPath($OutputDirectory).TrimEnd('\') -cne 'C:\LGSMOutput\portable') { throw 'Portable output must be the fixed mapped guest output directory.' }
if ($RunToken -notmatch '^[0-9a-fA-F]{32,128}$') { throw 'A one-run caller token is required.' }
$portableInput='C:\LGSMInput\portable'
$manifest=[IO.File]::ReadAllText((Join-Path $portableInput 'manifest.json'),[Text.Encoding]::UTF8) | ConvertFrom-Json
$request=[IO.File]::ReadAllText('C:\LGSMInput\run.json',[Text.Encoding]::UTF8) | ConvertFrom-Json
if ($request.schema_version -ne 1 -or $request.run_token -cne $RunToken -or $request.host_name -ieq [Net.Dns]::GetHostName()) { throw 'Portable caller token/guest identity does not match the read-only Sandbox request.' }
if ($manifest.host_computer -ieq [Net.Dns]::GetHostName()) { throw 'Portable worker refuses the exporting host.' }
$work='C:\LGSMPortableRuntimeWork'
if (Test-Path -LiteralPath $work) { throw 'Portable guest work already exists; use a fresh Sandbox session.' }
New-Item -ItemType Directory -Path $work | Out-Null
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $work 'temp') | Out-Null
[IO.File]::WriteAllText((Join-Path $work 'session.json'),(@{run_token=$RunToken;computer=[Net.Dns]::GetHostName()}|ConvertTo-Json -Compress),$utf8)
$binary=Join-Path $portableInput $manifest.binary
if ((Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash -ine $manifest.binary_sha256) { throw 'Exported portable test binary SHA256 mismatch.' }
$env:TEMP=Join-Path $work 'temp'; $env:TMP=$env:TEMP
$bootstrap=Start-Process -FilePath $binary -ArgumentList @('--list') -Wait -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $OutputDirectory 'bootstrap.stdout.log') -RedirectStandardError (Join-Path $OutputDirectory 'bootstrap.stderr.log')
[IO.File]::WriteAllText((Join-Path $OutputDirectory 'bootstrap.json'),(@{exit_code=$bootstrap.ExitCode;binary_sha256=$manifest.binary_sha256;native_runtime_requirements=$manifest.native_runtime_requirements}|ConvertTo-Json -Depth 8),$utf8)
if ($bootstrap.ExitCode -ne 0) { throw 'The exported test binary cannot start. Verify the native VC14 x64 Sandbox bootstrap before portable tests.' }
$listed=[IO.File]::ReadAllText((Join-Path $OutputDirectory 'bootstrap.stdout.log'),[Text.Encoding]::UTF8)
if (-not [regex]::IsMatch($listed,('(?m)^'+[regex]::Escape($manifest.worker_filter)+': test\r?$'))) { throw 'The exported artifact does not list the exact portable worker test.' }
$env:LANGAME_PORTABLE_SANDBOX_TOKEN=$RunToken
$worker=Start-Process -FilePath $binary -ArgumentList @($manifest.worker_filter,'--exact','--ignored','--nocapture','--test-threads=1') -Wait -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $OutputDirectory 'worker.stdout.log') -RedirectStandardError (Join-Path $OutputDirectory 'worker.stderr.log')
[IO.File]::WriteAllText((Join-Path $OutputDirectory 'worker-exit.json'),(@{exit_code=$worker.ExitCode;binary_sha256=$manifest.binary_sha256}|ConvertTo-Json),$utf8)
$global:LASTEXITCODE=$worker.ExitCode
if ($worker.ExitCode -ne 0) { throw "Portable production runtime worker failed with exit code $($worker.ExitCode); inspect its report and stderr." }
$reportPath=Join-Path $OutputDirectory 'results.json'
if (-not (Test-Path -LiteralPath $reportPath -PathType Leaf)) { throw 'Portable worker returned without its required evidence report.' }
$report=[IO.File]::ReadAllText($reportPath,[Text.Encoding]::UTF8) | ConvertFrom-Json
$sha=[Security.Cryptography.SHA256]::Create()
try { $tokenDigest=([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($RunToken)))).Replace('-','').ToLowerInvariant() } finally { $sha.Dispose() }
if ($report.test_binary_sha256 -ine $manifest.binary_sha256 -or $report.run_token_sha256 -cne $tokenDigest -or $report.guest.computer -ine [Net.Dns]::GetHostName()) { throw 'Portable evidence is not bound to this guest, artifact and run token.' }
if (@($report.cases).Count -ne @($manifest.cases).Count) { throw 'Portable evidence did not contain every expected case.' }
foreach ($caseId in $manifest.cases) {
  $rows=@($report.cases | Where-Object { $_.id -ceq $caseId })
  if ($rows.Count -ne 1 -or $rows[0].status -cne 'passed') { throw "Portable case $caseId did not pass exactly once." }
}
[pscustomobject]@{exit_code=$worker.ExitCode;output=$OutputDirectory}
"#;
