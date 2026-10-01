use super::*;
use app_storage::{WindroseBootstrap, WindroseBootstrapObservation};

#[path = "commands_windrose_bootstrap_readiness.rs"]
mod bootstrap_readiness;
use bootstrap_readiness::WorldReadiness;

const INITIALIZATION_TIMEOUT: Duration = Duration::from_secs(120);
const INITIALIZATION_POLL: Duration = Duration::from_secs(1);
const INITIALIZATION_STOP_TIMEOUT: Duration = Duration::from_secs(20);

pub(super) struct PreparedBootstrap {
    session: WindroseBootstrap,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

pub(super) struct RetainedBootstrap {
    _session: Option<WindroseBootstrap>,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

pub(super) async fn prepare_if_required(
    paths: &app_storage::StoragePaths,
    instance: &InstanceDetails,
) -> Result<Option<PreparedBootstrap>, String> {
    if instance.summary.module_id != "windrose" {
        return Ok(None);
    }
    let Some(session) = app_storage::prepare_windrose_bootstrap(paths, &instance.summary.id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    static GATE: std::sync::OnceLock<Arc<tokio::sync::Semaphore>> = std::sync::OnceLock::new();
    let gate = GATE
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(1)))
        .clone();
    let permit = match gate.try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => return Err(abort_prepared(Some(session),
            "另一个 Windrose 实例正在首次初始化，请待其完成后重试；当前实例的配置和生成数据已保留。".into()).await),
    };
    Ok(Some(PreparedBootstrap {
        session,
        _permit: permit,
    }))
}

pub(super) enum BootstrapOutcome {
    Ready,
    CleanupPending(Box<PendingBootstrapCleanup>),
}

pub(super) struct PendingBootstrapCleanup {
    pub plan: ProcessLaunchPlan,
    pub spawned: app_runtime::SpawnedProcess,
    pub guard: RetainedBootstrap,
    pub message: String,
}

pub(super) struct BootstrapAdmission {
    pub prepared: PreparedBootstrap,
    pub program_guards: super::commands_runtime_lifecycle::RuntimeProgramGuards,
}

pub(super) async fn initialize_if_required(
    app_handle: Option<&tauri::AppHandle>,
    state: &tauri::State<'_, DesktopState>,
    instance: &InstanceDetails,
    primary: &ProcessLaunchPlan,
    reservation: &RuntimeStartReservationLease,
    resource_group: &app_runtime::RuntimeResourceGroup,
    admission: BootstrapAdmission,
) -> Result<
    (
        super::commands_runtime_lifecycle::RuntimeProgramGuards,
        BootstrapOutcome,
    ),
    String,
> {
    let BootstrapAdmission {
        prepared,
        program_guards,
    } = admission;
    let PreparedBootstrap {
        session: prepared,
        _permit: permit,
    } = prepared;
    if prepared.install_root() != Path::new(&primary.launch_plan.install_root) {
        return Err(abort_prepared(
            Some(prepared),
            "Windrose initialization runtime does not match its launch plan".into(),
        )
        .await);
    }
    let mut plan = primary.clone();
    // The official StartServerForeground.bat starts this engine directly.
    // Its exit code, rather than the outer launcher exiting, is the bootstrap
    // shutdown receipt for the process that owns the native world database.
    let engine = prepared
        .install_root()
        .join("R5/Binaries/Win64/WindroseServer-Win64-Shipping.exe");
    plan.launch_plan.executable_path = engine.to_string_lossy().into_owned();
    plan.launch_plan.args = ["-log", "-NewConsole", "-stdout", "-FullStdOutLogOutput"]
        .map(String::from)
        .to_vec();
    plan.launch_plan.command_line = format!(
        "\"{}\" {}",
        plan.launch_plan.executable_path,
        plan.launch_plan.args.join(" ")
    );
    plan.process_key = String::from("windrose-bootstrap");
    plan.display_name = String::from("Windrose initialization");
    plan.log_path = Path::new(&primary.log_path)
        .with_extension("windrose-bootstrap.log")
        .to_string_lossy()
        .into_owned();
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
        "Windrose 首次启动：由原生服务器生成身份和世界，完整停止后应用实例配置。",
        "",
    );
    let group = resource_group.clone();
    let worker_reservation = reservation.clone();
    spawn_storage_context_task(reservation, async move {
        let outcome = run_bootstrap(prepared, permit, plan, worker_reservation, group).await?;
        Ok((program_guards, outcome))
    })
    .await
    .map_err(|error| format!("Windrose initialization worker failed: {error}"))?
}

async fn inspect_owned(
    prepared: &mut Option<WindroseBootstrap>,
    reservation: &RuntimeStartReservationLease,
) -> Result<WindroseBootstrapObservation, String> {
    let owned = prepared
        .take()
        .ok_or("Windrose initialization lease is unavailable")?;
    let (owned, observed) = spawn_blocking_storage_context_task(reservation, move || {
        let observed = owned
            .inspect_native_state()
            .map_err(|error| error.to_string());
        (owned, observed)
    })
    .await
    .map_err(|error| format!("Windrose native state reader failed: {error}"))?;
    *prepared = Some(owned);
    observed
}

fn check_initialization_ports(observed: &WindroseBootstrapObservation) -> Result<(), String> {
    // A fresh native build may use relay transport (port -1). Module examples
    // do not establish which port, if any, this initialization will listen on.
    if observed.use_direct_connection != Some(true) {
        return Ok(());
    }
    let port = observed
        .direct_connection_server_port
        .filter(|port| *port > 0)
        .ok_or("Windrose 原生配置已启用直连，但未提供有效端口；保留配置和生成数据，请修复原生直连端口后重试。")?;
    let ports = ["udp", "tcp"].map(|protocol| PortBinding {
        name: if protocol == "udp" {
            "direct"
        } else {
            "direct_tcp"
        }
        .into(),
        protocol: protocol.into(),
        port,
    });
    if app_runtime::remap_taken_port_bindings("0.0.0.0", &ports)?.is_some() {
        return Err(format!(
            "Windrose 原生配置的直连端口 {port}/TCP+UDP 当前被占用，未启动初始化，也未停止其他实例。首次配置保持可恢复。",
        ));
    }
    Ok(())
}

async fn abort_prepared(prepared: Option<WindroseBootstrap>, cause: String) -> String {
    match prepared {
        Some(prepared) => match Box::pin(prepared.abort_after_stopped()).await {
            Ok(()) => cause,
            Err(error) => format!("{cause}; Windrose configuration recovery: {error}"),
        },
        None => cause,
    }
}

async fn run_bootstrap(
    prepared: WindroseBootstrap,
    permit: tokio::sync::OwnedSemaphorePermit,
    plan: ProcessLaunchPlan,
    reservation: RuntimeStartReservationLease,
    resource_group: app_runtime::RuntimeResourceGroup,
) -> Result<BootstrapOutcome, String> {
    let mut prepared = Some(prepared);
    let initial = match inspect_owned(&mut prepared, &reservation).await {
        Ok(observed) => observed,
        Err(error) => return Err(abort_prepared(prepared, error).await),
    };
    if reservation.is_cancelled() {
        return Err(abort_prepared(
            prepared,
            "Windrose initialization was cancelled before launch".into(),
        )
        .await);
    }
    let checked = spawn_blocking_storage_context_task(&reservation, move || {
        check_initialization_ports(&initial)
    })
    .await
    .map_err(|error| format!("Windrose port check failed: {error}"))?;
    if let Err(error) = checked {
        return Err(abort_prepared(prepared, error).await);
    }
    let spawn_plan = plan.clone();
    // Retain the Job owner outside blocking callbacks. A callback panic must
    // not discard a surviving tree before the normal registration path sees it.
    let process = Arc::new(std::sync::Mutex::new(None::<app_runtime::SpawnedProcess>));
    let spawn_owner = process.clone();
    let spawned = spawn_blocking_storage_context_task(&reservation, move || {
        let writer =
            app_storage::managed_console_log::ManagedConsoleLog::open(&spawn_plan.log_path)
                .map_err(|error| error.to_string())?;
        let readiness = WorldReadiness::before_spawn(Path::new(&spawn_plan.log_path))
            .map_err(|error| format!("Windrose bootstrap log checkpoint: {error}"))?;
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
    .map_err(|error| format!("Windrose initialization launch worker failed: {error}"))
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
                spawned
                    .err()
                    .unwrap_or_else(|| "Windrose initialization returned no process owner".into()),
            )
            .await);
        }
    };
    let result = match spawned {
        Ok(readiness) => {
            observe_initialization(
                &mut prepared,
                pid,
                &identity,
                &plan.log_path,
                &reservation,
                readiness,
            )
            .await
        }
        Err(error) => Err(error),
    };
    let stop_owner = process.clone();
    let stopped = spawn_blocking_storage_context_task(&reservation, move || {
        let mut owner = stop_owner.lock().unwrap_or_else(|error| error.into_inner());
        let spawned = owner
            .as_mut()
            .ok_or("Windrose process owner is unavailable")?;
        app_runtime::stop_spawned_unreal_process_gracefully(spawned, INITIALIZATION_STOP_TIMEOUT)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Windrose initialization stop worker failed: {error}"))
    .and_then(|result| result);
    let spawned = process
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take()
        .ok_or("Windrose process ownership disappeared during shutdown")?;
    // A dead root PID alone does not prove that the Job's descendants exited.
    if stopped.is_err() || spawned.child.is_some() {
        let cause = result
            .err()
            .unwrap_or_else(|| "Windrose generated its world".into());
        return Ok(BootstrapOutcome::CleanupPending(Box::new(
            PendingBootstrapCleanup {
                plan,
                spawned,
                guard: RetainedBootstrap {
                    _session: prepared,
                    _permit: permit,
                },
                message: format!(
                    "{cause}; Windrose initialization process-tree shutdown is unconfirmed: {}. The process remains registered for stop; generated data and recovery state are retained.",
                    stopped
                        .err()
                        .unwrap_or_else(|| "owned process handle remains".into()),
                ),
            },
        )));
    }
    let exit_code = match stopped {
        Ok(exit_code) => exit_code,
        Err(error) => return Err(abort_prepared(prepared, error).await),
    };
    if let Err(error) = result {
        return Err(abort_prepared(prepared, error).await);
    }
    if exit_code != Some(0) {
        return Err(abort_prepared(prepared, format!(
            "Windrose 初始化进程树已停止，但未确认原生服务器正常退出（退出码 {exit_code:?}）；保留生成数据和恢复记录，下次启动将重新验证，不继续正式启动。"
        )).await);
    }
    Box::pin(
        prepared
            .take()
            .ok_or("Windrose initialization lease is unavailable")?
            .finish_after_stopped(),
    )
    .await
    .map_err(|error| error.to_string())?;
    let _ = append_startup_console_line(
        &plan.log_path,
        "Windrose 原生初始化已正常停服；已登记生成的世界，继续应用实例配置并正式启动。",
        "",
    );
    Ok(BootstrapOutcome::Ready)
}

async fn observe_initialization(
    prepared: &mut Option<WindroseBootstrap>,
    pid: u32,
    identity: &app_core::ProcessIdentity,
    log_path: &str,
    reservation: &RuntimeStartReservationLease,
    mut readiness: WorldReadiness,
) -> Result<(), String> {
    let deadline = Instant::now() + INITIALIZATION_TIMEOUT;
    let mut last_observation = None;
    loop {
        if reservation.is_cancelled() {
            return Err("Windrose initialization was cancelled; retaining generated data".into());
        }
        if !app_runtime::process_matches_identity(pid, identity)
            .map_err(|error| error.to_string())?
        {
            return Err(
                "Windrose native initialization exited before generating a matching world".into(),
            );
        }
        let observed = inspect_owned(prepared, reservation).await?;
        let owned_path = PathBuf::from(log_path);
        let (next_readiness, host_ready) =
            spawn_blocking_storage_context_task(reservation, move || {
                let ready = readiness.poll(&owned_path);
                (readiness, ready)
            })
            .await
            .map_err(|error| format!("Windrose bootstrap log reader failed: {error}"))?;
        readiness = next_readiness;
        let host_ready =
            host_ready.map_err(|error| format!("Windrose bootstrap readiness log: {error}"))?;
        let public_state = (
            observed.use_direct_connection,
            observed.direct_connection_server_port,
            observed.world_ready,
        );
        if last_observation != Some(public_state) {
            let line = format!(
                "Windrose native initialization observed UseDirectConnection={:?}, DirectConnectionServerPort={:?}, world_ready={}",
                public_state.0, public_state.1, public_state.2,
            );
            let _ = append_startup_console_line(log_path, &line, "");
            #[cfg(test)]
            println!("NATIVE_WINDROSE_BOOTSTRAP {line}");
            last_observation = Some(public_state);
        }
        if observed.world_ready && host_ready {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(
                "Windrose native initialization exceeded its 120-second current-host and world-generation deadline"
                    .into(),
            );
        }
        tokio::select! {
            biased;
            _ = reservation.cancelled() => {}
            _ = tokio::time::sleep(INITIALIZATION_POLL) => {}
        }
    }
}

#[cfg(all(test, windows))]
#[path = "commands_windrose_lifecycle_tests.rs"]
mod tests;
