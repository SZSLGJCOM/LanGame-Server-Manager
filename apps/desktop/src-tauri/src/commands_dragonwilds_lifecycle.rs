use super::startup_log_cursor::StartupLogCursor;
use super::*;
use app_storage::{DragonwildsBootstrap, DragonwildsBootstrapObservation};

const INITIALIZATION_TIMEOUT: Duration = Duration::from_secs(180);
const INITIALIZATION_POLL: Duration = Duration::from_millis(250);
const INITIALIZATION_STOP_TIMEOUT: Duration = Duration::from_secs(20);
const NATIVE_LOG: &str = "RSDragonwilds/Saved/Logs/RSDragonwilds.log";

pub(super) enum BootstrapOutcome {
    Ready,
    CleanupPending(Box<PendingBootstrapCleanup>),
}

pub(super) struct PendingBootstrapCleanup {
    pub plan: ProcessLaunchPlan,
    pub spawned: app_runtime::SpawnedProcess,
    pub guard: Option<DragonwildsBootstrap>,
    pub message: String,
}

pub(super) struct BootstrapAdmission {
    pub prepared: DragonwildsBootstrap,
    pub program_guards: super::RuntimeProgramGuards,
}

/// The formal launch retains its arguments. Initialization uses the same native
/// program and ports on loopback, with no operator command or log redirection.
fn initialization_plan(primary: &ProcessLaunchPlan) -> Result<ProcessLaunchPlan, String> {
    let mut plan = primary.clone();
    let engine = Path::new(&primary.launch_plan.install_root)
        .join("RSDragonwilds/Binaries/Win64/RSDragonwildsServer-Win64-Shipping.exe");
    plan.launch_plan.executable_path = engine.to_string_lossy().into_owned();
    plan.launch_plan.args = [
        "-log",
        "-NewConsole",
        "-stdout",
        "-FullStdOutLogOutput",
        "-MULTIHOME=127.0.0.1",
        "-unattended",
        "-NoSplash",
    ]
    .map(String::from)
    .to_vec();
    for prefix in ["-port=", "-queryport="] {
        let value = primary
            .launch_plan
            .args
            .iter()
            .find(|arg| arg.to_ascii_lowercase().starts_with(prefix))
            .ok_or_else(|| {
                format!("Dragonwilds initialization requires the managed {prefix} argument")
            })?;
        plan.launch_plan.args.push(value.clone());
    }
    plan.launch_plan.command_line = format!(
        "\"{}\" {}",
        plan.launch_plan.executable_path,
        plan.launch_plan.args.join(" ")
    );
    plan.process_key = "dragonwilds-bootstrap".into();
    plan.display_name = "Dragonwilds initialization".into();
    plan.log_path = Path::new(&primary.log_path)
        .with_extension("dragonwilds-bootstrap.log")
        .to_string_lossy()
        .into_owned();
    Ok(plan)
}

pub(super) async fn initialize_if_required(
    app_handle: Option<&tauri::AppHandle>,
    state: &tauri::State<'_, DesktopState>,
    instance: &InstanceDetails,
    primary: &ProcessLaunchPlan,
    reservation: &RuntimeStartReservationLease,
    resource_group: &app_runtime::RuntimeResourceGroup,
    admission: BootstrapAdmission,
) -> Result<(super::RuntimeProgramGuards, BootstrapOutcome), String> {
    let BootstrapAdmission {
        prepared,
        program_guards,
    } = admission;
    let plan = match initialization_plan(primary) {
        Ok(plan) if prepared.install_root() == Path::new(&plan.launch_plan.install_root) => plan,
        Ok(_) => {
            return Err(abort_prepared(
                Some(prepared),
                "Dragonwilds initialization runtime does not match its launch plan".into(),
            )
            .await);
        }
        Err(error) => return Err(abort_prepared(Some(prepared), error).await),
    };
    if let Err(error) = publish_pending_start_console_log_path(
        app_handle,
        state,
        &instance.summary.id,
        &plan.log_path,
        Some(&plan.process_key),
        Some(&plan.display_name),
        None,
    ) {
        return Err(abort_prepared(Some(prepared), error).await);
    }
    let _ = append_startup_console_line(
        &plan.log_path,
        "正在初始化龙之荒野世界；保存完成并正常停服后应用预配置玩法，再正式启动。",
        "",
    );
    let group = resource_group.clone();
    let worker_reservation = reservation.clone();
    let log_path = plan.log_path.clone();
    let result = spawn_storage_context_task(reservation, async move {
        let outcome = run_bootstrap(prepared, plan, worker_reservation, group).await?;
        Ok((program_guards, outcome))
    })
    .await
    .map_err(|error| format!("Dragonwilds initialization worker failed: {error}"))
    .and_then(|result| result);
    if !matches!(&result, Ok((_, BootstrapOutcome::CleanupPending(_)))) {
        // Its producer has stopped (or never started). Drain the final lines
        // and retire this source; a successful start only tracks the main log.
        finish_runtime_log_stream_for_process(state, &instance.summary.id, &log_path);
    }
    result
}

async fn inspect_owned(
    prepared: &mut Option<DragonwildsBootstrap>,
    reservation: &RuntimeStartReservationLease,
) -> Result<DragonwildsBootstrapObservation, String> {
    let owned = prepared
        .take()
        .ok_or("Dragonwilds initialization lease is unavailable")?;
    let (owned, observed) = spawn_blocking_storage_context_task(reservation, move || {
        let observed = owned
            .inspect_native_state()
            .map_err(|error| error.to_string());
        (owned, observed)
    })
    .await
    .map_err(|error| format!("Dragonwilds native world reader failed: {error}"))?;
    *prepared = Some(owned);
    observed
}

async fn abort_prepared(prepared: Option<DragonwildsBootstrap>, cause: String) -> String {
    match prepared {
        Some(prepared) => match Box::pin(prepared.abort_after_stopped()).await {
            Ok(()) => cause,
            Err(error) => format!("{cause}; Dragonwilds configuration recovery: {error}"),
        },
        None => cause,
    }
}

async fn finish_prepared(
    prepared: &mut Option<DragonwildsBootstrap>,
    reservation: &RuntimeStartReservationLease,
) -> Result<(), String> {
    if reservation.is_cancelled() {
        return Err(abort_prepared(
            prepared.take(),
            "Dragonwilds initialization was cancelled; generated world and pending settings were retained".into(),
        )
        .await);
    }
    Box::pin(
        prepared
            .take()
            .ok_or("Dragonwilds initialization lease is unavailable")?
            .finish_after_stopped(),
    )
    .await
    .map_err(|error| error.to_string())
}

async fn run_bootstrap(
    prepared: DragonwildsBootstrap,
    plan: ProcessLaunchPlan,
    reservation: RuntimeStartReservationLease,
    resource_group: app_runtime::RuntimeResourceGroup,
) -> Result<BootstrapOutcome, String> {
    let world_name = prepared.world_name().to_owned();
    let native_log = prepared.install_root().join(NATIVE_LOG);
    let mut prepared = Some(prepared);
    let initial = match inspect_owned(&mut prepared, &reservation).await {
        Ok(observed) => observed,
        Err(error) => return Err(abort_prepared(prepared, error).await),
    };
    // Storage accepts this only for the world bound to a retained initialization
    // journal. Existing worlds without that ownership never enter this path.
    // Loading such a world again would not produce an immediate initial save.
    if initial.world_ready {
        finish_prepared(&mut prepared, &reservation).await?;
        let _ = append_startup_console_line(
            &plan.log_path,
            "已恢复上次初始化完成的世界并应用预配置玩法，继续正式启动。",
            "",
        );
        return Ok(BootstrapOutcome::Ready);
    }
    if reservation.is_cancelled() {
        return Err(abort_prepared(
            prepared,
            "Dragonwilds initialization was cancelled before launch".into(),
        )
        .await);
    }
    // The Job owner remains outside blocking callbacks, including panic paths.
    let process = Arc::new(std::sync::Mutex::new(None::<app_runtime::SpawnedProcess>));
    let spawn_owner = process.clone();
    let spawn_plan = plan.clone();
    let spawned = spawn_blocking_storage_context_task(&reservation, move || {
        let writer =
            app_storage::managed_console_log::ManagedConsoleLog::open(&spawn_plan.log_path)
                .map_err(|error| error.to_string())?;
        let readiness = WorldReadiness::before_spawn(native_log, world_name)?;
        let spawned = app_runtime::spawn_launch_plan_in_resource_group(
            &spawn_plan.launch_plan,
            &spawn_plan.log_path,
            Some(Box::new(writer)),
            &resource_group,
        )
        .map_err(|error| error.to_string())?;
        *spawn_owner
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(spawned);
        Ok::<WorldReadiness, String>(readiness)
    })
    .await
    .map_err(|error| format!("Dragonwilds initialization launch worker failed: {error}"))
    .and_then(|result| result);
    let identity = process
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_ref()
        .map(|spawned| (spawned.pid, spawned.process_identity.clone()));
    let (pid, identity) = match identity {
        Some(identity) => identity,
        None => {
            return Err(abort_prepared(
                prepared,
                spawned.err().unwrap_or_else(|| {
                    "Dragonwilds initialization returned no process owner".into()
                }),
            )
            .await);
        }
    };
    let _ = append_startup_console_line(
        &plan.log_path,
        &format!("Dragonwilds initialization owns native process {pid}."),
        "",
    );
    let (mut readiness, mut observed) = match spawned {
        Ok(mut readiness) => {
            let result =
                observe_initialization(&mut prepared, pid, &identity, &reservation, &mut readiness)
                    .await;
            (Some(readiness), result)
        }
        Err(error) => (None, Err(error)),
    };
    let stop_owner = process.clone();
    let stopped = spawn_blocking_storage_context_task(&reservation, move || {
        let mut owner = stop_owner.lock().unwrap_or_else(|error| error.into_inner());
        let spawned = owner
            .as_mut()
            .ok_or("Dragonwilds process owner is unavailable")?;
        app_runtime::stop_spawned_unreal_process_gracefully(spawned, INITIALIZATION_STOP_TIMEOUT)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Dragonwilds initialization stop worker failed: {error}"))
    .and_then(|result| result);
    let spawned = process
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take()
        .ok_or("Dragonwilds process ownership disappeared during shutdown")?;
    // Root exit alone does not prove all descendants stopped. Preserve the Job
    // and password journal until the ordinary supervisor can own Stop recovery.
    if stopped.is_err() || spawned.child.is_some() {
        let cause = observed
            .err()
            .unwrap_or_else(|| "Dragonwilds generated its world".into());
        let message = format!(
            "{cause}; Dragonwilds initialization process-tree shutdown is unconfirmed: {}. The process remains registered for stop; generated data and pending settings are retained.",
            stopped
                .err()
                .unwrap_or_else(|| "owned process handle remains".into()),
        );
        reservation.record_cleanup_failure(&message);
        return Ok(BootstrapOutcome::CleanupPending(Box::new(
            PendingBootstrapCleanup {
                plan,
                spawned,
                guard: prepared,
                message,
            },
        )));
    }
    let exit_code = stopped?;
    if exit_code == Some(0)
        && reservation.is_cancelled()
        && let Some(readiness) = readiness.take()
    {
        // A cancel may arrive after Save succeeds but before the polling loop
        // records the generated GUID. Once the exact owned tree has exited,
        // finish reading this launch's log and bind only its proven saved world.
        // This records recovery ownership; it never applies the pending rules.
        let recovery =
            bind_cancelled_world_after_stopped(&mut prepared, &reservation, readiness).await;
        if let Err(error) = recovery {
            let cause = observed
                .err()
                .unwrap_or_else(|| "Dragonwilds initialization was cancelled".into());
            observed = Err(format!("{cause}; cancelled world recovery: {error}"));
        }
    }
    if let Err(error) = observed {
        return Err(abort_prepared(prepared, error).await);
    }
    if exit_code != Some(0) {
        return Err(abort_prepared(prepared, format!(
            "Dragonwilds initialization stopped with unconfirmed normal native exit ({exit_code:?}); generated world and pending settings were retained."
        )).await);
    }
    finish_prepared(&mut prepared, &reservation).await?;
    let _ = append_startup_console_line(
        &plan.log_path,
        &format!(
            "Dragonwilds initialization native process {pid} exited with code 0; its complete owned tree stopped and pending world settings were applied."
        ),
        "",
    );
    Ok(BootstrapOutcome::Ready)
}

#[derive(Clone)]
struct WorldReadiness {
    cursor: StartupLogCursor,
    save_to_slot: String,
    save_completed: String,
    slot_saved: bool,
    completed: bool,
    more: bool,
}

impl WorldReadiness {
    fn before_spawn(path: PathBuf, world_name: String) -> Result<Self, String> {
        Ok(Self {
            cursor: StartupLogCursor::capture(path, false)?,
            save_to_slot: format!("Save to slot {world_name}: Success"),
            save_completed: format!("Save completed SUCCESSFULLY (slot: {world_name})"),
            slot_saved: false,
            completed: false,
            more: false,
        })
    }

    fn poll(&mut self, cancelled: impl Fn() -> bool) -> Result<bool, String> {
        let progress = self.cursor.poll(cancelled)?;
        if progress.reset {
            self.slot_saved = false;
            self.completed = false;
        }
        if progress.oversized_record {
            return Err(
                "Dragonwilds initialization native log contains an oversized record".into(),
            );
        }
        for line in progress.lines {
            if line.contains("LogSpudSubsystem: ") && line.ends_with(&self.save_to_slot) {
                self.slot_saved = true;
            }
            if self.slot_saved
                && line.contains("LogPersistence: [DedicatedServer] ")
                && line.ends_with(&self.save_completed)
            {
                self.completed = true;
            }
        }
        self.more = progress.more;
        Ok(self.slot_saved && self.completed && !self.more)
    }

    fn after_stopped(&mut self) -> Result<bool, String> {
        // A stopped producer is finite. Keep the same cursor and bounded reads;
        // no old file or prelaunch fragment is promoted into this run's proof.
        loop {
            let saved = self.poll(|| false)?;
            if saved || !self.more {
                return Ok(saved);
            }
        }
    }
}

async fn bind_cancelled_world_after_stopped(
    prepared: &mut Option<DragonwildsBootstrap>,
    reservation: &RuntimeStartReservationLease,
    mut readiness: WorldReadiness,
) -> Result<(), String> {
    let saved = spawn_blocking_storage_context_task(reservation, move || readiness.after_stopped())
        .await
        .map_err(|error| format!("Dragonwilds final native log reader failed: {error}"))??;
    if saved {
        inspect_owned(prepared, reservation).await?;
    }
    Ok(())
}

async fn observe_initialization(
    prepared: &mut Option<DragonwildsBootstrap>,
    pid: u32,
    identity: &app_core::ProcessIdentity,
    reservation: &RuntimeStartReservationLease,
    readiness: &mut WorldReadiness,
) -> Result<(), String> {
    let deadline = Instant::now() + INITIALIZATION_TIMEOUT;
    loop {
        if reservation.is_cancelled() {
            return Err(
                "Dragonwilds initialization was cancelled; retaining generated data".into(),
            );
        }
        if !app_runtime::process_matches_identity(pid, identity)
            .map_err(|error| error.to_string())?
        {
            return Err(
                "Dragonwilds native initialization exited before completing its world save".into(),
            );
        }
        let log_reservation = reservation.clone();
        let mut owned_readiness = readiness.clone();
        let (next_readiness, saved) = spawn_blocking_storage_context_task(reservation, move || {
            let saved = owned_readiness.poll(|| log_reservation.is_cancelled());
            (owned_readiness, saved)
        })
        .await
        .map_err(|error| format!("Dragonwilds initialization log reader failed: {error}"))?;
        *readiness = next_readiness;
        // Do not decode a partially written first save. Only the current native
        // process's complete success markers authorize inspecting the new bytes.
        if saved? && inspect_owned(prepared, reservation).await?.world_ready {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("Dragonwilds initialization exceeded its 180-second native save deadline; generated data and pending settings were retained".into());
        }
        tokio::select! {
            biased;
            _ = reservation.cancelled() => {}
            _ = tokio::time::sleep(INITIALIZATION_POLL) => {}
        }
    }
}

#[cfg(test)]
#[path = "commands_dragonwilds_lifecycle_tests.rs"]
mod tests;
