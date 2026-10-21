//! One request-correlated native checkpoint for the owned Unity main thread.
use super::*;
use sha2::{Digest, Sha256};
use std::io::Read;

type Supervisor = Arc<StdMutex<app_runtime::RuntimeSupervisor>>;
const BUDGET: Duration = Duration::from_secs(60);
const GAME_SHA256: &str = "bc0c9b79b125ce9a6d96d532359fde8a539159378bd9f558309572b9744a9d7e";

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "GetTickCount64"]
    fn os_uptime_milliseconds() -> u64;
}

pub(in crate::commands) async fn save_world(
    state: &tauri::State<'_, DesktopState>,
    details: &InstanceDetails,
    submitted_at_unix_ms: u128,
    mutation: Option<tokio::sync::OwnedMutexGuard<()>>,
) -> Result<InstanceRuntimeCommandResult, String> {
    let deadline = Instant::now() + BUDGET;
    let mutation = mutation.ok_or("The Forest save requires the instance mutation lease.")?;
    let operation = state.begin_storage_context_operation("The Forest native save completion")?;
    let supervisor = state.runtime_supervisor.clone();
    let details = details.clone();
    crate::state::spawn_blocking_storage_context_task(&operation, move || {
        let _mutation = mutation;
        save_owned(&supervisor, &details, submitted_at_unix_ms, deadline)
    })
    .await
    .map_err(|_| "The Forest save worker failed; its outcome is unknown and was not retried.")?
}

fn target(
    details: &InstanceDetails,
) -> Result<(&ActiveInstanceRun, &app_core::InstanceProcessState), String> {
    if details.summary.module_id != "theforest"
        || !matches!(details.summary.status, InstanceStatus::Running)
    {
        return Err("The Forest saving requires its current running instance.".into());
    }
    let run = details
        .active_run
        .as_ref()
        .ok_or("The Forest has no managed run.")?;
    if details.summary.active_process_count != 1
        || run.process_count != 1
        || run.processes.len() != 1
    {
        return Err("The Forest saving requires one unambiguous managed process.".into());
    }
    let process = &run.processes[0];
    if process.process_key != "main"
        || !process.is_primary
        || process.status != "running"
        || process.exit_code.is_some()
        || process.crash_flag
        || process.run_id != run.run_id
        || process.session_id != run.session_id
        || process.pid.filter(|pid| *pid > 0).is_none()
        || run.pid != process.pid
        || process
            .process_identity
            .as_ref()
            .is_none_or(|id| id.creation_time == 0)
    {
        return Err("The Forest has no healthy current primary process identity.".into());
    }
    Ok((run, process))
}

fn save_owned(
    supervisor: &Supervisor,
    details: &InstanceDetails,
    submitted: u128,
    deadline: Instant,
) -> Result<InstanceRuntimeCommandResult, String> {
    let (run, process) = target(details)?;
    let pid = process.pid.ok_or("The Forest has no managed PID.")?;
    let identity = process
        .process_identity
        .as_ref()
        .ok_or("The Forest has no native process identity.")?;
    let path = process
        .log_path
        .as_deref()
        .map(PathBuf::from)
        .ok_or("The Forest has no owned console log.")?;
    let current = || -> Result<(), String> {
        check_deadline(deadline)?;
        let runtime = supervisor
            .lock()
            .map_err(|_| "Runtime supervisor lock poisoned.")?;
        let tracked = runtime
            .tracked_instances()
            .into_iter()
            .find(|item| item.summary.id == details.summary.id)
            .ok_or("The managed The Forest instance stopped; completion is unconfirmed.")?;
        if tracked.run_id != run.run_id
            || tracked.session_id != run.session_id
            || tracked.pid != Some(pid)
            || tracked.process_count != 1
            || tracked.log_path.as_deref() != process.log_path.as_deref()
            || !process_matches_identity(pid, identity).map_err(|error| error.to_string())?
        {
            return Err("The managed The Forest run, session or process changed; completion is unconfirmed.".into());
        }
        Ok(())
    };
    current()?;
    let instance = Path::new(&details.config_file_path)
        .parent()
        .and_then(Path::parent)
        .ok_or("The Forest has no instance directory.")?;
    let root =
        app_storage::resolve_instance_runtime_root(instance).map_err(|error| error.to_string())?;
    if dunce::canonicalize(root.join("TheForestDedicatedServer.exe"))
        .map_err(|_| "The Forest program is unavailable.")?
        != dunce::canonicalize(&identity.image_path)
            .map_err(|_| "The Forest process image is unavailable.")?
    {
        return Err("The managed The Forest program belongs to another instance.".into());
    }
    let _game = pin_hash(
        &root.join("TheForestDedicatedServer_Data/Managed/Assembly-CSharp.dll"),
        GAME_SHA256,
        64 * 1024 * 1024,
        deadline,
    )?;
    #[cfg(windows)]
    let payload: &[u8] = include_bytes!(concat!(
        env!("OUT_DIR"),
        "/theforest-control/LanGame.TheForest.Control.dll"
    ));
    #[cfg(not(windows))]
    let payload: &[u8] = &[];
    let expected: String = Sha256::digest(payload)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let _plugin = pin_hash(
        &root.join("DllMods/LanGame.TheForest.Control.dll"),
        &expected,
        512 * 1024,
        deadline,
    )?;
    require_managed_log(&path)?;
    let mut tail = crate::runtime_log_stream::runtime_log_tail_at_end(&path)
        .map_err(|_| "Unable to establish The Forest's save log boundary; no save was sent.")?;
    let nonce = request_nonce()?;
    let command = format!("save {nonce}");
    current()?;
    // The owned blocking worker retains both leases until write and observation
    // finish even if the caller disappears. There is exactly one stdin write.
    let mut lease = {
        let mut runtime = supervisor
            .lock()
            .map_err(|_| "Runtime supervisor lock poisoned.")?;
        crate::commands::commands_stdin_dispatch::ensure_expected_supervisor_run(
            &runtime,
            &details.summary.id,
            Some(run.run_id),
        )?;
        runtime
            .begin_command_dispatch(&details.summary.id, Some("main"))
            .map_err(|error| error.to_string())?
    };
    let delivery = if lease.target().pid != pid || lease.target().process_key != "main" {
        Err("The Forest command owner changed; no save was sent.")
    } else {
        lease
            .write_stdin_line(&command)
            .map_err(|_| "The Forest save delivery is unknown; the request was not retried.")
    };
    supervisor
        .lock()
        .map_err(|_| "The Forest command lease restoration failed; outcome is unconfirmed.")?
        .finish_command_dispatch(lease);
    delivery?;
    let mut receipt = Receipt::new(&nonce);
    let bytes = loop {
        current()?;
        let delta = read_runtime_log_delta_bounded(&path, &mut tail, 256 * 1024, 16 * 1024)
            .map_err(
                |_| "The Forest's save confirmation log could not be read; outcome is unconfirmed.",
            )?;
        if delta.stream_error.is_some() || delta.limit_exhausted {
            return Err("The Forest's save capture is incomplete; outcome is unconfirmed and was not retried.".into());
        }
        for line in delta.lines {
            receipt.observe(&line)?;
        }
        current()?;
        if let Some(bytes) = receipt.bytes {
            break bytes;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    Ok(InstanceRuntimeCommandResult {
        instance_id: details.summary.id.clone(),
        process_key: "main".into(),
        display_name: "The Forest native save".into(),
        pid,
        command: "save".into(),
        response_text: Some(format!(
            "The Forest confirmed this request's synchronous native checkpoint ({bytes} bytes, new file identity)."
        )),
        write_confirmation_pending: false,
        save_completion: Some(WorldSaveCompletion::Confirmed),
        submitted_at_unix_ms: submitted,
    })
}

fn check_deadline(deadline: Instant) -> Result<(), String> {
    if Instant::now() >= deadline {
        Err(
            "The Forest save observation expired; completion is unconfirmed and was not retried."
                .into(),
        )
    } else {
        Ok(())
    }
}

fn request_nonce() -> Result<String, String> {
    let milliseconds = request_uptime()?;
    Ok(format!(
        "{milliseconds:016x}{}",
        &uuid::Uuid::new_v4().simple().to_string()[..16]
    ))
}

#[cfg(windows)]
fn request_uptime() -> Result<u64, String> {
    // The native bridge uses this same system-wide monotonic clock. UTC/NTP
    // correction cannot revive a nonce already evicted from its replay set.
    Ok(unsafe { os_uptime_milliseconds() })
}

#[cfg(not(windows))]
fn request_uptime() -> Result<u64, String> {
    Err("The Forest save admission requires Windows system uptime.".into())
}

fn require_managed_log(path: &Path) -> Result<(), String> {
    match app_storage::managed_console_log::open_log_segments(path) {
        Ok(Some(_)) => Ok(()),
        _ => Err("The Forest save requires an intact managed console segment ledger; completion is unconfirmed.".into()),
    }
}

fn pin_hash(
    path: &Path,
    expected: &str,
    limit: u64,
    deadline: Instant,
) -> Result<std::fs::File, String> {
    check_deadline(deadline)?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1).custom_flags(0x0020_0000);
    }
    let mut file = options.open(path).map_err(
        |_| "Unable to pin The Forest's verified save implementation; no save was sent.",
    )?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > limit {
        return Err(
            "The Forest save implementation is not a supported ordinary file; no save was sent."
                .into(),
        );
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("The Forest save implementation cannot be a reparse point.".into());
        }
    }
    let mut sha = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        check_deadline(deadline)?;
        let count = file
            .read(&mut buffer)
            .map_err(|_| "Unable to verify The Forest's native save implementation.")?;
        if count == 0 {
            break;
        }
        sha.update(&buffer[..count]);
    }
    let actual: String = sha
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if actual != expected {
        return Err("The Forest's game or control DLL has no verified save-completion contract; no save was sent.".into());
    }
    Ok(file)
}

struct Receipt {
    completed: String,
    failed: String,
    bytes: Option<u64>,
}
impl Receipt {
    fn new(nonce: &str) -> Self {
        Self {
            completed: format!(
                "[LanGame native control] native_save_completed nonce={nonce} bytes="
            ),
            failed: format!("[LanGame native control] native_save_failed nonce={nonce} code="),
            bytes: None,
        }
    }
    fn observe(&mut self, line: &str) -> Result<(), String> {
        if line.starts_with(&self.failed) {
            return Err("The Forest reported failure for this native checkpoint; the request was not retried.".into());
        }
        if let Some(value) = line.strip_prefix(&self.completed) {
            if self.bytes.is_some()
                || value.is_empty()
                || !value.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err("The Forest returned a malformed or repeated save receipt; completion is unconfirmed.".into());
            }
            self.bytes = Some(value.parse::<u64>().ok().filter(|value| *value > 0).ok_or(
                "The Forest returned an invalid checkpoint length; completion is unconfirmed.",
            )?);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "commands_theforest_world_save_tests.rs"]
mod tests;
