//! A local owned managed payload verifies CLR loading without launching a game.
use super::*;
use sha2::Sha256;

const TOKEN: &str = "LGSM_DOTNET_RUNTIME_OK_v1";
const SOURCE: &str = "using System; public static class LanGameRuntimeProbe { public static int Main() { Console.WriteLine(\"LGSM_DOTNET_RUNTIME_OK_v1\"); return 0; } }";

pub(super) async fn execute(
    owned_root: &Path,
    runtime: &Path,
    version: [u32; 3],
    deadline: InstallDeadline,
) -> Result<bool, SteamCmdError> {
    let assembly = ensure_assembly(owned_root, deadline).await?;
    let config = assembly.with_file_name(format!(
        "probe-{}.runtimeconfig.json",
        display_version(version)
    ));
    let configuration = serde_json::json!({"runtimeOptions": {
        "framework":{"name":"Microsoft.NETCore.App","version":display_version(version)},
        "rollForward":"Disable"
    }});
    fs::write(&config, configuration.to_string()).map_err(|error| {
        preparation_error(format!(
            "Cannot write the owned CLR probe configuration: {error}"
        ))
    })?;
    let mut command = Command::new(runtime.join("dotnet.exe"));
    command
        .arg("exec")
        .arg("--runtimeconfig")
        .arg(&config)
        .arg("--fx-version")
        .arg(display_version(version))
        .arg("--roll-forward")
        .arg("Disable")
        .arg(&assembly)
        .env("DOTNET_ROOT", runtime)
        .env("DOTNET_ROOT_X64", runtime)
        .env("DOTNET_MULTILEVEL_LOOKUP", "0")
        .env("DOTNET_ROLL_FORWARD_TO_PRERELEASE", "0")
        .env("DOTNET_EnableDiagnostics", "0")
        .env("CORECLR_ENABLE_PROFILING", "0")
        .env("COR_ENABLE_PROFILING", "0")
        .env_remove("DOTNET_STARTUP_HOOKS")
        .env_remove("DOTNET_ADDITIONAL_DEPS")
        .env_remove("DOTNET_SHARED_STORE")
        .env_remove("DOTNET_DiagnosticPorts");
    let output =
        match run_command_capture(command, deadline.limited_to(Duration::from_secs(20))).await {
            Ok(output) => output,
            Err(SteamCmdError::SpawnCommand { .. }) => return Ok(false),
            Err(error) => return Err(error),
        };
    Ok(output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == TOKEN)
}

async fn ensure_assembly(
    owned_root: &Path,
    deadline: InstallDeadline,
) -> Result<PathBuf, SteamCmdError> {
    let source_hash = digest(SOURCE.as_bytes());
    for root in cache_roots(owned_root, "dotnet-probe") {
        if !matches_identity(&root, "dotnet-probe", &source_hash) {
            continue;
        }
        let assembly = root.join("probe.dll");
        let valid = read_json(&root.join("probe-integrity.json"))
            .ok()
            .and_then(|marker| {
                marker
                    .get("assembly_sha256")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .zip(assembly_digest(&assembly))
            .is_some_and(|(expected, actual)| expected == actual);
        if valid {
            return Ok(assembly);
        }
    }
    let compiler = compiler_path()?;
    let stage = scratch(owned_root, "probe-stage");
    fs::create_dir(&stage).map_err(|source| SteamCmdError::CreatePath {
        path: stage.clone(),
        source,
    })?;
    let result = async {
        let source = stage.join("probe.cs");
        let assembly = stage.join("probe.dll");
        let resources = stage.join("probe.res");
        fs::write(&source, SOURCE).map_err(|error| {
            preparation_error(format!("Cannot write owned CLR probe source: {error}"))
        })?;
        // Framework csc otherwise creates CSC<guid>.TMP for default Win32
        // resources. That extra filename exceeds legacy MAX_PATH in managed
        // scratch paths. A valid empty RES (its null resource header only)
        // avoids the temporary resource generator; this CLR payload needs no
        // native manifest, version information or icon.
        let mut empty_resources = [0_u8; 32];
        empty_resources[4..8].copy_from_slice(&32_u32.to_le_bytes());
        empty_resources[8..10].copy_from_slice(&u16::MAX.to_le_bytes());
        empty_resources[12..14].copy_from_slice(&u16::MAX.to_le_bytes());
        fs::write(&resources, empty_resources).map_err(|error| {
            preparation_error(format!("Cannot write owned CLR probe resources: {error}"))
        })?;
        let mut command = Command::new(compiler);
        command
            .arg("/nologo")
            .arg("/utf8output")
            .arg("/target:exe")
            .arg("/platform:anycpu")
            .arg("/optimize+")
            .arg(format!("/out:{}", command_path_string(&assembly)))
            .arg(format!("/win32res:{}", command_path_string(&resources)))
            .arg(command_path_string(&source))
            // Framework csc uses legacy Win32 temporary-file APIs. Give this
            // child its owned ordinary path rather than an inherited extended
            // managed-scratch path; no process-global environment is changed.
            .env("TEMP", command_path_string(&stage))
            .env("TMP", command_path_string(&stage));
        let output =
            run_command_capture(command, deadline.limited_to(Duration::from_secs(30))).await?;
        if !output.status.success() {
            return Err(preparation_error(format!(
                "Cannot compile the local CLR probe: {}",
                output_excerpt(&output.stdout, &output.stderr)
            )));
        }
        let assembly_hash = assembly_digest(&assembly).ok_or_else(|| {
            preparation_error(
                "The local CLR probe compiler did not produce a bounded managed assembly.",
            )
        })?;
        fs::write(
            stage.join("probe-integrity.json"),
            serde_json::json!({"assembly_sha256":assembly_hash}).to_string(),
        )
        .map_err(|error| preparation_error(error.to_string()))?;
        deadline.check_cancelled()?;
        let published = publish_runtime(
            &stage,
            owned_root,
            &format!("probe-{}", &source_hash[..16]),
            "dotnet-probe",
            &source_hash,
        )?;
        Ok(published.join("probe.dll"))
    }
    .await;
    if !matches!(
        &result,
        Err(SteamCmdError::InstallProcessCleanupFailed { .. })
    ) {
        await_failed_directory_cleanup(vec![stage]).await;
    }
    result
}

#[cfg(windows)]
fn compiler_path() -> Result<PathBuf, SteamCmdError> {
    let system = app_runtime::windows_system_directory().map_err(|error| {
        preparation_error(format!(
            "Cannot locate the trusted Windows compiler: {error}"
        ))
    })?;
    let windows = system.parent().ok_or_else(|| {
        preparation_error(
            "Windows system directory has no parent for the local CLR probe compiler.",
        )
    })?;
    for relative in [
        "Microsoft.NET/Framework64/v4.0.30319/csc.exe",
        "Microsoft.NET/Framework/v4.0.30319/csc.exe",
    ] {
        let path = windows.join(relative);
        if path.is_file() {
            return Ok(path);
        }
    }
    Err(preparation_error(
        "The Windows .NET Framework C# compiler is missing; the managed runtime health probe cannot be prepared.",
    ))
}

#[cfg(not(windows))]
fn compiler_path() -> Result<PathBuf, SteamCmdError> {
    Err(preparation_error(
        "The managed runtime health probe requires Windows.",
    ))
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn assembly_digest(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (!bytes.is_empty() && bytes.len() <= 2 * 1024 * 1024).then(|| digest(&bytes))
}

#[cfg(all(test, windows))]
#[path = "portable_dotnet_probe_tests.rs"]
mod tests;
