//! Explicit fault injection only in the ignored disposable native acceptance.
//! The OS denies primary replacement; neither native code nor its flags change.
use super::*;
use crate::commands::commands_managed_save_control::{guard_pin_directories, guard_read_regular};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::time::{SystemTime, UNIX_EPOCH};

const UNKNOWN: &str = "The native save outcome is unknown. It may already have executed; verify the current server and its save files before retrying.";
const LIMIT: usize = 64 * 1024 * 1024;

fn configured(module: &str, value: Option<&str>) -> Result<bool, String> {
    match value {
        None => Ok(false),
        Some("true") if matches!(module, "sonsoftheforest" | "barotrauma") => Ok(true),
        _ => Err(
            "Native save failure acceptance requires its explicit Sons/Barotrauma opt-in".into(),
        ),
    }
}
fn nonce(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        && value.bytes().any(|c| c != b'0')
}
fn frame<'a>(bytes: &'a [u8], keys: &[&str]) -> Result<Vec<&'a str>, String> {
    if bytes.len() > 2048 || bytes.iter().any(|c| *c != b'\n' && !(32..=126).contains(c)) {
        return Err("Invalid failure-probe frame".into());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "Invalid failure-probe frame")?;
    let lines: Vec<_> = text
        .strip_suffix('\n')
        .ok_or("Incomplete failure-probe frame")?
        .split('\n')
        .collect();
    if lines.len() != keys.len() + 1 || lines[0] != "LGSM_SAVE_1" {
        return Err("Invalid failure-probe frame shape".into());
    }
    keys.iter()
        .zip(&lines[1..])
        .map(|(key, line)| {
            line.strip_prefix(&format!("{key}="))
                .ok_or_else(|| "Invalid failure-probe field".into())
        })
        .collect()
}
fn correlated_unknown(
    module: &str,
    session: &[u8],
    request: &[u8],
    response: &[u8],
    old_nonce: Option<&str>,
    pid: u32,
    request_window: std::ops::RangeInclusive<u128>,
) -> Result<(), String> {
    let session = frame(session, &["module", "run", "token"])?;
    let request = frame(request, &["run", "nonce", "token", "op", "expires"])?;
    let response = frame(
        response,
        &["module", "run", "nonce", "pid", "status", "issue"],
    )?;
    let expires: u128 = request[4]
        .parse()
        .map_err(|_| "Invalid failure-probe expiry")?;
    let issued = expires
        .checked_sub(5000)
        .ok_or("Invalid failure-probe expiry")?;
    if session[0] != module
        || !nonce(session[1])
        || !nonce(session[2])
        || request[0] != session[1]
        || !nonce(request[1])
        || Some(request[1]) == old_nonce
        || request[2] != session[2]
        || request[3] != "save"
        || expires.to_string() != request[4]
        || !request_window.contains(&issued)
        || pid == 0
        || response[0] != module
        || response[1] != session[1]
        || response[2] != request[1]
        || response[3] != pid.to_string()
        || response[4] != "unknown"
        || response[5] != "native_outcome_unknown"
    {
        return Err("Locked-primary request lacked its own post-admission unknown outcome".into());
    }
    Ok(())
}
fn ordinary(file: &File, limit: usize) -> Result<(), String> {
    let metadata = file
        .metadata()
        .map_err(|_| "Cannot inspect failure-probe file")?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Failure-probe file is redirected".into());
        }
    }
    if !metadata.is_file() || metadata.len() > limit as u64 {
        return Err("Failure-probe file is not a bounded ordinary file".into());
    }
    Ok(())
}
fn open(path: &Path, locked: bool) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options
            .share_mode(if locked { 1 } else { 1 | 2 | 4 })
            .custom_flags(0x0020_0000);
    }
    #[cfg(not(windows))]
    {
        let _ = locked;
        return Err("Locked-primary native acceptance requires Windows".into());
    }
    let file = options
        .open(path)
        .map_err(|_| "Cannot open failure-probe file")?;
    ordinary(&file, LIMIT)?;
    Ok(file)
}
fn digest(file: &mut File) -> Result<Vec<u8>, String> {
    ordinary(file, LIMIT)?;
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "Cannot rewind locked primary")?;
    let mut bytes = Vec::new();
    file.take(LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read locked primary")?;
    if bytes.len() > LIMIT {
        return Err("Locked primary exceeds bound".into());
    }
    Ok(Sha256::digest(bytes).to_vec())
}
fn write_failure(module: &str, path: &Path, tail: &[u8]) -> Result<(), String> {
    let text = String::from_utf8_lossy(tail)
        .to_ascii_lowercase()
        .replace('\\', "/");
    let path = path
        .to_string_lossy()
        .to_ascii_lowercase()
        .replace('\\', "/");
    let path = path.strip_prefix("//?/").unwrap_or(&path);
    let sharing = text.contains("sharing violation")
        || text.contains("used by another process")
        || text.contains("ioexception");
    let sink = if module == "sonsoftheforest" {
        text.contains("savegamemanager.writejsonzip")
    } else {
        text.contains("error compressing save file")
    };
    if !sharing || !sink || !text.contains(path) {
        return Err("Native log did not establish this fixture primary's attempted write and sharing failure".into());
    }
    Ok(())
}

pub(super) async fn verify<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    cycle: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let flag = std::env::var("LANGAME_NATIVE_SAVE_FAILURE_ACCEPTANCE")
        .map(Some)
        .or_else(|e| match e {
            std::env::VarError::NotPresent => Ok(None),
            _ => Err("Invalid native failure-acceptance flag"),
        })?;
    if !configured(&running.summary.module_id, flag.as_deref())? || cycle == "restart" {
        return Ok(());
    }
    if cycle != "initial"
        || std::env::var("LANGAME_NATIVE_MANUAL_SAVE").as_deref() != Ok("true")
        || std::env::var("LANGAME_NATIVE_SAVE_REQUIRE_COMPLETED").as_deref() != Ok("true")
    {
        return Err("Failure acceptance requires the initial real completion scenario".into());
    }
    if running.summary.module_id == "sonsoftheforest" {
        Box::pin(manual_save_probe::verify(runtime, running, "preflight")).await?;
        println!(
            "NATIVE_SAVE_PREFLIGHT module=sonsoftheforest cycle=initial save_completion=confirmed formal_receipt=false"
        );
    }
    let fixture = runtime.package.root.canonicalize()?;
    let program = creation::program_root(running)?.canonicalize()?;
    let saves = PathBuf::from(&running.saves_path).canonicalize()?;
    if program == fixture
        || saves == fixture
        || !program.starts_with(&fixture)
        || !saves.starts_with(&fixture)
    {
        return Err("Failure injection escaped its disposable fixture".into());
    }
    let _directory_pins = guard_pin_directories(&saves)?;
    let module = running.summary.module_id.as_str();
    let primary_path = saves.join(if module == "sonsoftheforest" {
        "DedicatedServer/Multiplayer/0000000001/SaveData.zip"
    } else {
        "lgsm_native_manual_save.save"
    });
    let _primary_pins = guard_pin_directories(
        primary_path
            .parent()
            .ok_or("Failure primary lacks parent")?,
    )?;
    let _program_pins = guard_pin_directories(&program)?;
    let channel = program.join(".langame-world-save");
    let _channel_pins = guard_pin_directories(&channel)?;
    let run = running
        .active_run
        .as_ref()
        .ok_or("Failure probe has no managed run")?;
    let processes: Vec<_> = run
        .processes
        .iter()
        .filter(|p| p.is_primary && p.status == "running" && p.run_id == run.run_id)
        .collect();
    if processes.len() != 1 {
        return Err("Failure probe has ambiguous native owner".into());
    }
    let owner = processes[0];
    let pid = owner.pid.ok_or("Failure probe has no PID")?;
    let identity = owner
        .process_identity
        .as_ref()
        .ok_or("Failure probe has no OS identity")?;
    if !app_runtime::process_matches_identity(pid, identity)? {
        return Err("Failure probe process changed".into());
    }
    let console = PathBuf::from(
        owner
            .log_path
            .as_ref()
            .ok_or("Failure probe has no console log")?,
    );
    let paths = if module == "sonsoftheforest" {
        vec![
            ("redloader", program.join("_Redloader/Latest.log")),
            ("console", console),
        ]
    } else {
        vec![("console", console)]
    };
    let mut log_pins = Vec::new();
    let mut logs = Vec::new();
    for (name, path) in paths {
        let path = path.canonicalize()?;
        if !path.starts_with(&fixture) {
            return Err("Failure-probe log escaped its fixture".into());
        }
        log_pins.extend(guard_pin_directories(
            path.parent().ok_or("Failure log lacks parent")?,
        )?);
        let mut file = open(&path, false)?;
        let offset = file.seek(SeekFrom::End(0))?;
        logs.push((name, file, offset));
    }
    let session = guard_read_regular(&channel.join("session.txt"), 2048)?;
    let old_request = match fs::symlink_metadata(channel.join("request.txt")) {
        Ok(_) => Some(guard_read_regular(&channel.join("request.txt"), 2048)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let old_fields = old_request
        .as_deref()
        .map(|bytes| frame(bytes, &["run", "nonce", "token", "op", "expires"]))
        .transpose()?;
    let old_nonce = old_fields.as_ref().map(|fields| fields[1]);
    let mut lock = open(&primary_path, true)?;
    let before_sha = digest(&mut lock)?;
    // Opening without truncation has no write side effect. Only the expected
    // Win32 sharing violation establishes that this fixture fault is active.
    match OpenOptions::new().write(true).open(&primary_path) {
        Err(error) if error.raw_os_error() == Some(32) => {}
        _ => {
            return Err("Primary fault did not establish a Windows write-sharing violation".into());
        }
    }
    let before = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    let action = runtime
        .descriptor
        .runtime
        .player_actions
        .iter()
        .find(|a| a.id == "save_world")
        .ok_or("Missing declared save action")?;
    if action.target_required || action.destructive || !action.role_values.is_empty() {
        return Err("Failure probe requires the ordinary target-free save action".into());
    }
    let input = serde_json::from_value(
        json!({"instanceId":running.summary.id,"command":action.command_template,"runtimeActionId":"save_world"}),
    )?;
    let result = crate::commands::send_instance_runtime_command(runtime.state.clone(), input).await;
    let after = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    if result.as_ref().err().map(String::as_str) != Some(UNKNOWN) {
        return Err("Locked-primary native save did not report unknown; failure rejection or success cannot pass".into());
    }
    correlated_unknown(
        module,
        &session,
        &guard_read_regular(&channel.join("request.txt"), 2048)?,
        &guard_read_regular(&channel.join("response.txt"), 2048)?,
        old_nonce,
        pid,
        before..=after,
    )?;
    if before_sha != digest(&mut lock)? {
        return Err("Native failed save changed the locked primary".into());
    }
    if !app_runtime::process_matches_identity(pid, identity)? {
        return Err("Failure probe process exited or changed".into());
    }
    let current = read_instance_details(&runtime.storage.paths, &running.summary.id).await?;
    if current.active_run.as_ref().map(|r| r.run_id) != Some(run.run_id)
        || current
            .active_run
            .as_ref()
            .and_then(|r| r.session_id.as_ref())
            != run.session_id.as_ref()
    {
        return Err("Failure-probe managed run changed".into());
    }
    let request = guard_read_regular(&channel.join("request.txt"), 2048)?;
    let request_fields = frame(&request, &["run", "nonce", "token", "op", "expires"])?;
    let expected_primary = if module == "sonsoftheforest" {
        format!(
            "<fixture>/{}",
            primary_path
                .strip_prefix(program.parent().ok_or("Program lacks instance root")?)?
                .to_string_lossy()
                .replace('\\', "/")
        )
    } else {
        primary_path.to_string_lossy().into_owned()
    };
    let header = format!(
        "LGSM_SONS_NATIVE_WRITE_FAILURE run={} nonce={} pid={pid} ",
        request_fields[0], request_fields[1]
    );
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        let mut evidence = Vec::new();
        let mut diagnostic_lines = Vec::new();
        for (name, file, offset) in &mut logs {
            if file.metadata()?.len() < *offset {
                return Err("Native failure log was truncated".into());
            }
            file.seek(SeekFrom::Start(*offset))?;
            let mut tail = Vec::new();
            (&mut *file).take(1024 * 1024 + 1).read_to_end(&mut tail)?;
            if tail.len() > 1024 * 1024 {
                return Err("Native failure log exceeded its bound".into());
            }
            let text = String::from_utf8_lossy(&tail);
            if module == "sonsoftheforest" {
                for line in text.lines() {
                    if let Some(start) = line.find(&header) {
                        let detail = &line[start + header.len()..];
                        // Only this exact nonce/run/PID's restricted diagnostic
                        // can establish the native fault. Adjacent logs cannot.
                        if let Some(detail) = detail.strip_prefix("detail=") {
                            evidence.extend_from_slice(detail.as_bytes());
                            evidence.push(b'\n');
                        }
                        if diagnostic_lines.len() < 24 {
                            diagnostic_lines.push(format!(
                                "NATIVE_SAVE_FAILURE_DIAGNOSTIC sink={name} {header}{detail}"
                            ));
                        }
                    }
                }
            } else {
                evidence.extend_from_slice(&tail);
            }
            if Instant::now() >= until {
                let lower = text.to_ascii_lowercase();
                eprintln!(
                    "NATIVE_SAVE_FAILURE_DIAGNOSTIC sink={name} bytes={} ioexception={} writejsonzip={} copy={} primary_name={}",
                    tail.len(),
                    lower.contains("ioexception"),
                    lower.contains("writejsonzip"),
                    lower.contains("file.copy"),
                    lower.contains("savedata.zip")
                );
            }
        }
        let established = write_failure(module, Path::new(&expected_primary), &evidence).is_ok();
        if established || Instant::now() >= until {
            for line in diagnostic_lines {
                eprintln!("{line}");
            }
        }
        if established {
            break;
        }
        if Instant::now() >= until {
            return Err("Native log did not establish this fixture primary's attempted write and sharing failure".into());
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    drop(lock); // A separate fresh nonce now executes the ordinary positive save.
    println!(
        "NATIVE_SAVE_FAILURE module={module} cycle=initial locked_primary=true native_write_attempt=sharing_failure correlated_new_nonce=true outcome=unknown primary_sha256_unchanged=true lock_released=true retried=false"
    );
    Ok(())
}

#[test]
fn locked_failure_receipt_requires_current_nonce_unknown_and_native_sink_failure() {
    let run = "a".repeat(32);
    let token = "b".repeat(32);
    let nonce = "c".repeat(32);
    let session = format!("LGSM_SAVE_1\nmodule=sonsoftheforest\nrun={run}\ntoken={token}\n");
    let request =
        format!("LGSM_SAVE_1\nrun={run}\nnonce={nonce}\ntoken={token}\nop=save\nexpires=5100\n");
    let response = format!(
        "LGSM_SAVE_1\nmodule=sonsoftheforest\nrun={run}\nnonce={nonce}\npid=42\nstatus=unknown\nissue=native_outcome_unknown\n"
    );
    let check = |body: &str, old: Option<&str>, pid| {
        correlated_unknown(
            "sonsoftheforest",
            session.as_bytes(),
            request.as_bytes(),
            body.as_bytes(),
            old,
            pid,
            99..=101,
        )
    };
    assert!(check(&response, None, 42).is_ok());
    assert!(check(&response, Some(&nonce), 42).is_err());
    assert!(check(&response, None, 43).is_err());
    assert!(
        check(
            &response.replace(
                "status=unknown\nissue=native_outcome_unknown",
                "status=completed\nissue=none"
            ),
            None,
            42
        )
        .is_err()
    );
    assert!(
        check(
            &response.replace(
                "status=unknown\nissue=native_outcome_unknown",
                "status=rejected\nissue=world_not_ready"
            ),
            None,
            42
        )
        .is_err()
    );
    let path = Path::new("D:/fixture/SaveData.zip");
    assert!(
        write_failure(
            "sonsoftheforest",
            path,
            b"IOException D:/fixture/SaveData.zip Sons.Save.SaveGameManager.WriteJsonZip"
        )
        .is_ok()
    );
    assert!(
        write_failure(
            "sonsoftheforest",
            path,
            b"IOException D:/other/SaveData.zip Sons.Save.SaveGameManager.WriteJsonZip"
        )
        .is_err()
    );
    assert!(
        write_failure(
            "sonsoftheforest",
            path,
            b"Autosave complete D:/fixture/SaveData.zip"
        )
        .is_err()
    );
    assert!(configured("vrising", Some("true")).is_err());
}
