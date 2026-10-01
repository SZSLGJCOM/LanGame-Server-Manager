//! Actual native-loader/stdin checks, only within the disposable package fixture.
use super::*;
use std::io::{Read, Seek, SeekFrom};

pub(super) async fn verify<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
) -> Result<(), Box<dyn std::error::Error>> {
    if running.summary.module_id != "theforest" {
        return Ok(());
    }
    let fixture = runtime.package.root.canonicalize()?;
    let saves = Path::new(&running.saves_path).canonicalize()?;
    if !saves.starts_with(&fixture) || saves == fixture {
        return Err("The Forest command probe requires disposable fixture saves".into());
    }
    let run = running
        .active_run
        .as_ref()
        .ok_or("The Forest probe requires an active run")?;
    let process = run
        .processes
        .iter()
        .find(|p| p.process_key == "main")
        .ok_or("The Forest probe requires its exact main process")?;
    let pid = process
        .pid
        .ok_or("The Forest probe requires its native PID")?;
    let log = PathBuf::from(
        process
            .log_path
            .as_ref()
            .ok_or("The Forest probe requires its managed log")?,
    )
    .canonicalize()?;
    if !log.starts_with(&fixture) {
        return Err("The Forest probe log escaped its fixture".into());
    }
    for (command, acknowledgement) in [
        ("help", "[LanGame native control] help status save shutdown"),
        (
            "status",
            "[LanGame native control] world_ready=True serialization_suspended=False",
        ),
        (
            "save",
            "[LanGame native control] native_save_completed bytes=",
        ),
    ] {
        let before = (command == "save")
            .then(|| save_inventory(&saves))
            .transpose()?;
        let offset = fs::metadata(&log)?.len();
        let response = crate::commands::send_instance_gm_command(
            runtime.state.clone(),
            serde_json::from_value(json!({
                "instanceId": running.summary.id,
                "processKey": "main",
                "transport": "stdin",
                "command": command,
            }))?,
        )
        .await?;
        if response.instance_id != running.summary.id
            || response.process_key != "main"
            || response.pid != pid
        {
            return Err("The Forest command was routed outside its observed run".into());
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let mut file = fs::File::open(&log)?;
            if file.metadata()?.len() < offset {
                return Err("The Forest probe log was replaced".into());
            }
            file.seek(SeekFrom::Start(offset))?;
            let mut bytes = Vec::new();
            file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
            if bytes.len() > 1024 * 1024 {
                return Err("The Forest command output exceeded its bound".into());
            }
            let text = String::from_utf8_lossy(&bytes);
            if let Some(failure) = text.lines().find(|line| {
                line.starts_with("[LanGame native control] command failed:")
                    || line.starts_with("[LanGame native control] command refused:")
            }) {
                // The bridge emits only fixed stages and exception type names.
                // Keep this evidence even when fixture cleanup precedes observers.
                return Err(format!("The Forest native {command}: {failure}").into());
            }
            if text.lines().any(|line| line.starts_with(acknowledgement)) {
                break;
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "The Forest native {command} acknowledgement was not observed"
                )
                .into());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        if let Some(before) = before {
            let after = save_inventory(&saves)?;
            if before == after
                || !after.iter().any(|(path, (bytes, _))| {
                    path.file_name()
                        .is_some_and(|name| name.to_string_lossy().ends_with("__RESUME__"))
                        && *bytes > 0
                })
            {
                return Err("The Forest Save command did not produce changed nonempty native checkpoint data".into());
            }
        }
        println!(
            "NATIVE_THEFOREST_CONTROL command={command} acknowledgement=current_run_new_log{}",
            if command == "save" {
                " checkpoint=changed_nonempty"
            } else {
                ""
            }
        );
    }
    Ok(())
}

pub(super) fn verify_stopped(
    running: &InstanceDetails,
    stopped: &app_core::StopInstanceResult,
    elapsed: Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    if running.summary.module_id != "theforest" {
        return Ok(());
    }
    let run = running
        .active_run
        .as_ref()
        .ok_or("The Forest stop has no observed run")?;
    let original = run
        .processes
        .iter()
        .find(|p| p.process_key == "main")
        .ok_or("The Forest stop has no observed main process")?;
    let result = stopped
        .processes
        .iter()
        .find(|p| p.process_key == "main")
        .ok_or("The Forest stop omitted its main process")?;
    if stopped.summary.id != running.summary.id
        || stopped.run_id != run.run_id
        || stopped.session_id != run.session_id
        || result.run_id != original.run_id
        || result.pid != original.pid
        || result.exit_code != Some(0)
    {
        return Err("The Forest normal stop identity or native exit code did not match".into());
    }
    let log = original
        .log_path
        .as_deref()
        .ok_or("The Forest stop has no managed log")?;
    let mut bytes = Vec::new();
    fs::File::open(log)?
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("The Forest stopped log exceeded its bound".into());
    }
    let text = String::from_utf8_lossy(&bytes);
    if text
        .lines()
        .filter(|line| line.starts_with("[LanGame native control] native_save_completed bytes="))
        .count()
        != 2
        || text
            .lines()
            .filter(|line| *line == "[LanGame native control] native_shutdown_requested")
            .count()
            != 1
        || text
            .lines()
            .filter(|line| *line == "[LanGame native control] input_reader_stopped")
            .count()
            != 1
    {
        return Err("The Forest stop did not retain both native save acknowledgements and one shutdown request".into());
    }
    println!(
        "NATIVE_THEFOREST_CONTROL command=shutdown native_saves=2 native_shutdown_requests=1 exit_code=0 run_identity=matched elapsed_ms={}",
        elapsed.as_millis()
    );
    Ok(())
}

pub(super) fn report_stop_failure(state: &DesktopState, running: &InstanceDetails) {
    if running.summary.module_id != "theforest" {
        return;
    }
    if let (Ok(mut supervisor), Some(run)) =
        (state.runtime_supervisor.lock(), running.active_run.as_ref())
    {
        let tree = supervisor.instance_process_tree_is_running(&running.summary.id);
        eprintln!("NATIVE_THEFOREST_STOP_DIAGNOSTIC owned_tree_running={tree:?}");
        for process in &run.processes {
            if let Some(pid) = process.pid {
                let alive = supervisor.owned_process_is_running(
                    &running.summary.id,
                    process.run_id,
                    &process.process_key,
                    pid,
                );
                let identity = app_runtime::inspect_process_identity(pid).ok().flatten();
                eprintln!(
                    "NATIVE_THEFOREST_STOP_DIAGNOSTIC tracked_pid={pid} owned_handle_running={alive:?} creation={:?} image={:?}",
                    identity.as_ref().map(|value| &value.creation_time),
                    identity
                        .as_ref()
                        .and_then(|value| Path::new(&value.image_path).file_name())
                );
            }
        }
    }
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let process = running
            .active_run
            .as_ref()
            .and_then(|run| run.processes.iter().find(|p| p.process_key == "main"))
            .ok_or("missing observed main process")?;
        let log = process
            .log_path
            .as_deref()
            .ok_or("missing observed managed log")?;
        let mut bytes = Vec::new();
        fs::File::open(log)?
            .take(8 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err("managed log exceeded diagnostic bound".into());
        }
        // RenderTexture noise can evict every control line from a tail. These
        // exact prefixes contain only our fixed messages/stages and type names.
        for line in String::from_utf8_lossy(&bytes)
            .lines()
            .filter(|line| {
                line.starts_with("[LanGame native control] ")
                    || *line == "Bolt shutdown"
                    || *line == "Shutdown."
            })
            .take(64)
        {
            eprintln!("NATIVE_THEFOREST_STOP_DIAGNOSTIC {line}");
        }
        let inventory = save_inventory(Path::new(&running.saves_path))?;
        for (path, (size, _)) in inventory.iter().take(16) {
            eprintln!(
                "NATIVE_THEFOREST_STOP_DIAGNOSTIC save_file={} bytes={size}",
                path.display()
            );
        }
        Ok(())
    })();
    if result.is_err() {
        eprintln!("NATIVE_THEFOREST_STOP_DIAGNOSTIC bounded_control_transcript_unavailable");
    }
}
