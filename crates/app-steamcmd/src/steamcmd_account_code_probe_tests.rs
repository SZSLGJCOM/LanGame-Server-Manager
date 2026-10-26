//! Explicit opt-in against an isolated copy of the public Valve runtime. This
//! proves empty-password token parsing and log privacy, not real-account TOTP.
use super::*;
use crate::*;
use app_runtime::EphemeralConsole;
use std::io::{Read, Write};
use std::os::windows::fs::MetadataExt;
use std::path::Path;
use std::sync::{Arc, Mutex};

const CODE: &str = "7Q9XW";
const COPY_LIMIT: u64 = 512 * 1024 * 1024;
const LOG_LIMIT: u64 = 8 * 1024 * 1024;
const TOTAL_LOG_LIMIT: u64 = 32 * 1024 * 1024;
const PUBLIC_PROGRAMS: &[&str] = &[
    "steamcmd.exe",
    "steam.dll",
    "steamclient.dll",
    "steamclient64.dll",
    "steamconsole.dll",
    "steamconsole64.dll",
    "tier0_s.dll",
    "tier0_s64.dll",
    "vstdlib_s.dll",
    "vstdlib_s64.dll",
    "steamerrorreporter.exe",
];

#[derive(Default, serde::Serialize)]
struct Report {
    stage: &'static str,
    runtime_verified: bool,
    command_prompt_seen: bool,
    login_sent_once: bool,
    password_prompt_seen: bool,
    password_sent_once: bool,
    password_echoed: bool,
    code_echoed_in_private_sink: bool,
    password_found_in_logs: bool,
    code_found_in_logs: bool,
    logs_checked: usize,
    console_reaped: bool,
    bridge_closed: bool,
    isolated_root_removed: bool,
    error_code: Option<&'static str>,
}

#[derive(Default)]
struct SinkState {
    prompt: bool,
    password: bool,
    password_echoed: bool,
    code_echoed: bool,
    failed: bool,
}

struct SafeSink {
    state: Arc<Mutex<SinkState>>,
    password_marker: String,
}

impl Write for SafeSink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| std::io::Error::other("probe_state_unavailable"))?;
        // InteractiveSnapshots are complete edited rows. Retain only booleans;
        // neither the transcript nor a submitted marker survives this write.
        for row in String::from_utf8_lossy(bytes).lines() {
            let lower = row.trim().to_ascii_lowercase();
            state.prompt |= lower == "steam>";
            state.password |= lower == "password:";
            state.password_echoed |= row.contains(&self.password_marker);
            state.code_echoed |= row.contains(CODE);
            state.failed |= lower.starts_with("login failure:")
                || lower.starts_with("error! failed to log on:")
                || lower.starts_with("failed (");
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn checked_metadata(path: &Path) -> Result<std::fs::Metadata, &'static str> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "path_inspection_failed")?;
    if metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0 {
        return Err("reparse_path_rejected");
    }
    Ok(metadata)
}

fn check_ancestors(path: &Path) -> Result<(), &'static str> {
    for ancestor in path.ancestors() {
        checked_metadata(ancestor)?;
    }
    Ok(())
}

fn copy_public_file(
    source: &Path,
    destination: &Path,
    total: &mut u64,
) -> Result<(), &'static str> {
    let metadata = checked_metadata(source)?;
    if !metadata.is_file() {
        return Err("public_runtime_entry_not_file");
    }
    *total = total
        .checked_add(metadata.len())
        .ok_or("copy_limit_exceeded")?;
    if *total > COPY_LIMIT {
        return Err("copy_limit_exceeded");
    }
    fs::copy(source, destination).map_err(|_| "public_runtime_copy_failed")?;
    Ok(())
}

fn copy_public_runtime(source: &Path, destination: &Path) -> Result<(), &'static str> {
    let mut total = 0;
    for name in PUBLIC_PROGRAMS {
        let path = source.join(name);
        if path.exists() {
            copy_public_file(&path, &destination.join(name), &mut total)?;
        }
    }
    if !destination.join("steamcmd.exe").is_file() {
        return Err("source_runtime_missing");
    }
    let package = source.join("package");
    if !checked_metadata(&package)?.is_dir() {
        return Err("public_package_directory_invalid");
    }
    let manifest_path = package.join("steam_cmd_win64.manifest");
    if checked_metadata(&manifest_path)?.len() > 64 * 1024 {
        return Err("public_manifest_too_large");
    }
    let manifest = fs::read_to_string(&manifest_path).map_err(|_| "public_manifest_unreadable")?;
    let parsed = crate::steamcmd_update_manifest::parse_manifest(&manifest, "win64")
        .map_err(|_| "public_manifest_invalid")?;
    let target = destination.join("package");
    fs::create_dir(&target).map_err(|_| "public_package_copy_failed")?;
    copy_public_file(
        &manifest_path,
        &target.join("steam_cmd_win64.manifest"),
        &mut total,
    )?;
    let names = parsed
        .packages
        .iter()
        .flat_map(|package| [&package.file, &package.archive_file])
        .collect::<std::collections::BTreeSet<_>>();
    for name in names {
        let path = package.join(name);
        if path.exists() {
            copy_public_file(&path, &target.join(name), &mut total)?;
        }
    }
    Ok(())
}

fn scan_logs(root: &Path, password: &str, report: &mut Report) -> Result<(), &'static str> {
    let password_markers = [
        password.as_bytes().to_vec(),
        password.encode_utf16().flat_map(u16::to_le_bytes).collect(),
    ];
    let code_markers = [
        CODE.as_bytes().to_vec(),
        CODE.encode_utf16().flat_map(u16::to_le_bytes).collect(),
    ];
    let keep = password_markers
        .iter()
        .chain(&code_markers)
        .map(Vec::len)
        .max()
        .unwrap_or(1);
    let mut pending = vec![root.to_owned()];
    let mut entries = 0_usize;
    let mut total = 0_u64;
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).map_err(|_| "isolated_tree_read_failed")? {
            let path = entry.map_err(|_| "isolated_tree_read_failed")?.path();
            entries += 1;
            if entries > 4096 {
                return Err("isolated_tree_limit_exceeded");
            }
            let metadata = checked_metadata(&path)?;
            if metadata.is_dir() {
                pending.push(path);
                continue;
            }
            let in_logs = path
                .strip_prefix(root)
                .map_err(|_| "isolated_path_escaped")?
                .components()
                .any(|part| part.as_os_str().eq_ignore_ascii_case("logs"));
            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            if !in_logs
                && !["log", "txt"]
                    .iter()
                    .any(|value| extension.eq_ignore_ascii_case(value))
            {
                continue;
            }
            total = total
                .checked_add(metadata.len())
                .ok_or("log_limit_exceeded")?;
            if metadata.len() > LOG_LIMIT || total > TOTAL_LOG_LIMIT {
                return Err("log_limit_exceeded");
            }
            let mut file = fs::File::open(&path).map_err(|_| "isolated_log_read_failed")?;
            let mut tail = Vec::new();
            let mut buffer = [0_u8; 4096];
            let mut read = 0_u64;
            loop {
                let count = file
                    .read(&mut buffer)
                    .map_err(|_| "isolated_log_read_failed")?;
                if count == 0 {
                    break;
                }
                read += count as u64;
                if read > LOG_LIMIT {
                    return Err("log_limit_exceeded");
                }
                tail.extend_from_slice(&buffer[..count]);
                report.password_found_in_logs |= password_markers
                    .iter()
                    .any(|marker| tail.windows(marker.len()).any(|value| value == marker));
                report.code_found_in_logs |= code_markers
                    .iter()
                    .any(|marker| tail.windows(marker.len()).any(|value| value == marker));
                tail.drain(..tail.len().saturating_sub(keep));
            }
            report.logs_checked += 1;
        }
    }
    Ok(())
}

fn probe_console(
    root: &Path,
    update_source: Option<&str>,
    report: &mut Report,
) -> Result<(), &'static str> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "probe_clock_failed")?
        .as_nanos();
    let user = format!("lgsm_code_probe_{}_{stamp}", std::process::id());
    let password = format!("LgsmProbePassword7a_{}_{stamp}", std::process::id());
    let state = Arc::new(Mutex::new(SinkState::default()));
    let mut args = Vec::new();
    if let Some(source) = update_source {
        args.extend(["-overridepackageurl".to_owned(), source.to_owned()]);
    }
    args.extend([
        "+@NoPromptForPassword".into(),
        "0".into(),
        "+@ShutdownOnFailedCommand".into(),
        "1".into(),
    ]);
    let mut console = EphemeralConsole::spawn(
        &root.join("steamcmd.exe"),
        &args,
        root,
        SafeSink {
            state: state.clone(),
            password_marker: password.clone(),
        },
    )
    .map_err(|_| "isolated_console_launch_failed")?;
    report.stage = "wait_native_prompt";
    let expires = std::time::Instant::now() + Duration::from_secs(45);
    let mut after_password = None;
    let observed: Result<(), &'static str> = (|| {
        loop {
            let snapshot = state.lock().map_err(|_| "probe_state_unavailable")?;
            report.command_prompt_seen |= snapshot.prompt;
            report.password_prompt_seen |= snapshot.password;
            report.password_echoed |= snapshot.password_echoed;
            report.code_echoed_in_private_sink |= snapshot.code_echoed;
            let failed = snapshot.failed;
            drop(snapshot);
            if report.command_prompt_seen && !report.login_sent_once {
                console
                    .write_line(&format!("login {user} \"\" {CODE}"))
                    .map_err(|_| "synthetic_login_write_failed")?;
                report.login_sent_once = true;
                report.stage = "wait_hidden_password_prompt";
            }
            if report.password_prompt_seen && !report.password_sent_once {
                console
                    .write_line(&password)
                    .map_err(|_| "synthetic_password_write_failed")?;
                report.password_sent_once = true;
                report.stage = "observe_synthetic_authentication";
                after_password = Some(std::time::Instant::now() + Duration::from_secs(10));
            }
            if failed
                || console
                    .try_wait()
                    .map_err(|_| "isolated_console_read_failed")?
                    .is_some()
                || std::time::Instant::now() >= expires
                || after_password.is_some_and(|deadline| std::time::Instant::now() >= deadline)
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        Ok(())
    })();
    let cleanup = console.terminate_and_wait(Duration::from_secs(5));
    drop(console);
    report.console_reaped = cleanup.is_ok();
    if !report.console_reaped {
        return Err("isolated_console_cleanup_failed");
    }
    observed?;
    let snapshot = state.lock().map_err(|_| "probe_state_unavailable")?;
    report.password_echoed |= snapshot.password_echoed;
    report.code_echoed_in_private_sink |= snapshot.code_echoed;
    drop(snapshot);
    report.stage = "scan_isolated_logs";
    scan_logs(root, &password, report)?;
    if report.logs_checked == 0 {
        return Err("native_logs_not_observed");
    }
    if !report.login_sent_once || !report.password_prompt_seen || !report.password_sent_once {
        return Err("hidden_password_route_not_proven");
    }
    if report.password_echoed || report.password_found_in_logs || report.code_found_in_logs {
        return Err("synthetic_marker_privacy_failed");
    }
    Ok(())
}

async fn probe(root: &Path, source: &Path, report: &mut Report) -> Result<(), &'static str> {
    report.stage = "copy_public_runtime";
    prepare_configured_steamcmd_root(root).map_err(|_| "isolated_ownership_failed")?;
    copy_public_runtime(source, root)?;
    let settings = AppSettings {
        steamcmd_root: root.to_string_lossy().into_owned(),
        ..AppSettings::default()
    };
    let guard = acquire_steamcmd_operation(
        test_install_lock_context(),
        &settings,
        InstallDeadline::new("SteamCMD test lifecycle", GAME_INSTALL_LIFECYCLE_TIMEOUT),
    )
    .await
    .map_err(|_| "isolated_lock_failed")?;
    let bridge = crate::steamcmd_update_bridge::UpdateBridge::for_managed_root(
        root,
        InstallDeadline::new("isolated code probe", Duration::from_secs(90)),
    )
    .await
    .map_err(|_| "isolated_bridge_failed")?;
    let lease = AuthenticationLease { bridge, guard };
    report.stage = "verify_public_runtime";
    let verification = async {
        let mut command = Command::new(root.join("steamcmd.exe"));
        if let Some(source) = lease.update_source() {
            command.args(["-overridepackageurl", source]);
        }
        command.arg("+quit").current_dir(root);
        let mut reporter = crate::steamcmd_prepare::PrepareReporter::new(|_| {});
        let verified = crate::steamcmd_prepare_process::run_preparation_command(
            command,
            InstallDeadline::new("isolated code probe verification", Duration::from_secs(90)),
            Duration::from_secs(30),
            false,
            &mut reporter,
        )
        .await
        .map_err(|_| "public_runtime_verification_failed")?;
        if verified != crate::steamcmd_prepare_process::PreparationAttempt::Verified {
            return Err("public_runtime_not_verified");
        }
        crate::steamcmd_readiness::record_verified(&managed_steamcmd_status(&settings))
            .map_err(|_| "isolated_ready_record_failed")?;
        report.runtime_verified = true;
        Ok(())
    }
    .await;
    if let Err(error) = verification {
        lease.close().await;
        report.bridge_closed = true;
        return Err(error);
    }
    let root = root.to_owned();
    let mut owned_report = std::mem::take(report);
    // The worker owns both the native console and its lease. Awaiting it leaves
    // the runtime free to serve the same loopback manifest to the native child.
    let worker = tokio::task::spawn_blocking(move || {
        let result = probe_console(&root, lease.update_source(), &mut owned_report);
        (lease, owned_report, result)
    });
    let (lease, observed, result) = worker.await.map_err(|_| "isolated_console_worker_failed")?;
    *report = observed;
    lease.close().await;
    report.bridge_closed = true;
    result
}

#[tokio::test]
#[ignore = "explicit isolated public-runtime probe; never supplies a real Steam account"]
async fn isolated_native_empty_password_code_login_prompts_without_logging_markers() {
    let mut report = Report::default();
    let source = std::env::var_os("LANGAME_STEAMCMD_CODE_PROBE_SOURCE").map(PathBuf::from);
    let requested = std::env::var_os("LANGAME_STEAMCMD_CODE_PROBE_ROOT").map(PathBuf::from);
    let mut created = None;
    let outcome = async {
        report.stage = "validate_isolated_paths";
        let source = source.ok_or("source_environment_missing")?;
        let requested = requested.ok_or("probe_environment_missing")?;
        if !source.is_absolute() || !requested.is_absolute() {
            return Err("absolute_paths_required");
        }
        check_ancestors(&source)?;
        let source = fs::canonicalize(source).map_err(|_| "source_root_unavailable")?;
        let parent = requested.parent().ok_or("isolated_parent_missing")?;
        check_ancestors(parent)?;
        let root = fs::canonicalize(parent)
            .map_err(|_| "isolated_parent_unavailable")?
            .join(requested.file_name().ok_or("isolated_name_missing")?);
        if root.starts_with(&source) || source.starts_with(&root) {
            return Err("source_and_probe_overlap");
        }
        fs::create_dir(&root).map_err(|_| "isolated_root_must_be_new")?;
        created = Some(root.clone());
        probe(&root, &source, &mut report).await
    }
    .await;
    report.error_code = outcome.err();
    if let Some(root) = created {
        let cleanup = checked_metadata(&root).and_then(|_| {
            if fs::canonicalize(&root).map_err(|_| "isolated_root_changed")? != root {
                return Err("isolated_root_changed");
            }
            // Reinspect the complete owned tree before recursive cleanup. A
            // native-created reparse entry must never redirect deletion.
            let mut pending = vec![root.clone()];
            let mut entries = 0_usize;
            while let Some(directory) = pending.pop() {
                for entry in fs::read_dir(directory).map_err(|_| "isolated_tree_read_failed")? {
                    let path = entry.map_err(|_| "isolated_tree_read_failed")?.path();
                    entries += 1;
                    if entries > 4096 {
                        return Err("isolated_tree_limit_exceeded");
                    }
                    if checked_metadata(&path)?.is_dir() {
                        pending.push(path);
                    }
                }
            }
            fs::remove_dir_all(&root).map_err(|_| "isolated_root_cleanup_failed")
        });
        report.isolated_root_removed = cleanup.is_ok();
        if let Err(error) = cleanup {
            report.error_code = Some(error);
        }
    }
    if report.error_code.is_none() {
        report.stage = "complete";
    }
    println!(
        "STEAMCMD_CODE_PROBE {}",
        serde_json::to_string(&report).unwrap()
    );
    assert!(
        report.error_code.is_none(),
        "isolated SteamCMD code probe did not prove its contracts"
    );
}
