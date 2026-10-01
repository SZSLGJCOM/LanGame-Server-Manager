use super::commands_dst_world_state::{
    DstWorldStartPreview, acquire_dst_world_preview_permit, validate_dst_world_start_confirmation,
};
use super::commands_stdin_dispatch::dispatch_managed_stdin_command;
use super::*;
use app_storage::{materialize_instance_configuration_for_start, update_instance_ports};

#[path = "commands_dst_lifecycle.rs"]
pub(super) mod dst_lifecycle;
#[path = "commands_runtime_log_stream.rs"]
mod runtime_logs;
#[path = "commands_rust_shutdown.rs"]
pub(super) mod rust_shutdown;
#[path = "commands_shutdown_batch.rs"]
mod shutdown_batch;
#[path = "commands_shutdown_wait.rs"]
mod shutdown_wait;
#[path = "commands_windrose_lifecycle.rs"]
mod windrose_lifecycle;

const APP_EXIT_MAX_ACTIVE_RUN_DRAIN_ATTEMPTS: usize = 128;
const APP_SHUTDOWN_FAILED_EVENT: &str = "app-shutdown-failed";
const APP_STORAGE_DRAIN_TIMEOUT: Duration = Duration::from_secs(120);

pub(super) fn private_runtime_install_root(instance: &InstanceDetails) -> Result<String, String> {
    let config_path = Path::new(&instance.config_file_path);
    let instance_root = config_path
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| String::from("instance config path has no instance root"))?;
    app_storage::resolve_instance_runtime_root(instance_root)
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|error| error.to_string())
}

fn failed_start_error_with_cleanup(
    primary_error: impl Into<String>,
    cleanup: &SpawnedProcessStopReport,
) -> String {
    let primary_error = primary_error.into();
    match cleanup.issue_summary() {
        Some(issue) => format!("{primary_error}; failed-start cleanup: {issue}"),
        None => primary_error,
    }
}

#[tauri::command]
pub async fn preview_instance_launch(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
) -> Result<LaunchPlan, String> {
    let storage_context_operation =
        state.begin_storage_context_operation("instance launch preview")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;

    reconcile_runtime_state(&state).await?;
    let instance = commands_storage::read_instance_details_with_runtime_recovery(
        state.inner(),
        &storage_context_operation,
        &storage.paths,
        &instance_id,
    )
    .await?;
    let descriptors =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    let descriptor = find_descriptor(&descriptors, &instance.summary.module_id)?;
    build_instance_launch_preview(&storage.settings, descriptor, &instance)
}

pub(super) fn build_instance_launch_preview(
    settings: &app_core::AppSettings,
    descriptor: &ModuleDescriptor,
    instance: &InstanceDetails,
) -> Result<LaunchPlan, String> {
    let private_root = private_runtime_install_root(instance)?;
    let install_root_override = Some(private_root.as_str());
    let module = map_module_details_with_install_state(settings, descriptor, install_root_override);

    let projected = if app_core::ark_maps::is_ark(&instance.summary.module_id) {
        Some(app_core::ark_maps::project_process(instance, None)?)
    } else {
        None
    };
    let mut plan = build_launch_plan_with_override(
        settings,
        &module,
        projected.as_ref().unwrap_or(instance),
        install_root_override,
    )
    .map_err(|error| error.to_string())?;
    plan.uses_private_runtime = super::commands_program_storage::program_mode(instance)?
        == app_storage::InstanceProgramMode::Independent;
    Ok(plan)
}

#[tauri::command]
pub async fn start_instance_process(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    expected_world_start: Option<DstWorldStartPreview>,
) -> Result<StartInstanceResult, String> {
    start_instance_process_inner(app_handle, state, instance_id, expected_world_start).await
}

pub(super) async fn start_instance_process_inner(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    expected_world_start: Option<DstWorldStartPreview>,
) -> Result<StartInstanceResult, String> {
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;

    start_instance_process_after_reconcile(
        Some(&app_handle),
        &state,
        &storage,
        instance_id,
        "manual",
        expected_world_start,
    )
    .await
}

pub(super) async fn start_instance_process_after_reconcile(
    app_handle: Option<&tauri::AppHandle>,
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    instance_id: String,
    source: &str,
    expected_world_start: Option<DstWorldStartPreview>,
) -> Result<StartInstanceResult, String> {
    start_instance_process_with_preconditions(
        app_handle,
        state,
        storage,
        instance_id,
        source,
        RuntimeStartPreconditions {
            world_start: expected_world_start,
            instance: None,
            file_changes: Vec::new(),
        },
    )
    .await
}

#[derive(Default)]
pub(super) struct RuntimeStartPreconditions {
    pub(super) world_start: Option<DstWorldStartPreview>,
    pub(super) instance: Option<InstanceDetails>,
    pub(super) file_changes: Vec<app_storage::InstanceFilePatchResult>,
}

pub(super) type RuntimeProgramGuards = (
    tokio::sync::OwnedMutexGuard<()>,
    Option<app_steamcmd::GameInstallLifecycleGuard>,
);

include!("commands_runtime_start_evidence.rs");

async fn run_runtime_start_worker_to_completion<T, F>(
    runtime_start_reservation: &RuntimeStartReservationLease,
    worker: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: std::future::Future<Output = Result<T, String>> + Send + 'static,
{
    spawn_storage_context_task(runtime_start_reservation, worker)
        .await
        .map_err(|error| format!("runtime start worker failed: {error}"))?
}

async fn wait_for_runtime_start_delay(reservation: &RuntimeStartReservationLease, delay: Duration) {
    tokio::select! {
        biased;
        _ = reservation.cancelled() => {}
        _ = tokio::time::sleep(delay) => {}
    }
}

#[cfg(test)]
#[path = "commands_runtime_start_cancellation_tests.rs"]
mod runtime_start_cancellation_tests;

pub(super) enum InstanceRunConflict {
    ActiveRunRecord,
    TrackedRunning,
    NotRunning,
}

impl InstanceRunConflict {
    pub(super) fn into_error(self, instance_id: &str) -> String {
        let (code, reason, message) = match self {
            Self::ActiveRunRecord => (
                "instance_already_running",
                Some("active_run_record"),
                format!("instance `{instance_id}` already has an active run record"),
            ),
            Self::TrackedRunning => (
                "instance_already_running",
                Some("tracked_running"),
                format!("instance `{instance_id}` is already tracked as running"),
            ),
            Self::NotRunning => (
                "instance_not_running",
                None,
                format!("instance `{instance_id}` is not marked as running"),
            ),
        };
        let mut error = serde_json::json!({
            "code": code,
            "instance_id": instance_id,
            "message": message,
        });
        if let Some(reason) = reason {
            error["reason"] = serde_json::json!(reason);
        }
        error.to_string()
    }
}

pub(super) async fn start_instance_process_after_reconcile_reserved(
    app_handle: Option<&tauri::AppHandle>,
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    instance_id: String,
    source: &str,
    runtime_start_reservation: &RuntimeStartReservationLease,
    preconditions: RuntimeStartPreconditions,
) -> Result<StartInstanceResult, String> {
    let RuntimeStartPreconditions {
        world_start: expected_world_start,
        instance: expected_instance,
        file_changes: expected_file_changes,
    } = preconditions;
    let preserve_confirmed_ports = expected_instance.is_some();
    ensure_storage_context_snapshot_current(state, storage, "runtime start")?;
    // Preview workers always take admission before the instance lock.
    let world_preview_permit = if expected_world_start.is_some() {
        Some(acquire_dst_world_preview_permit().await?)
    } else {
        None
    };
    let instance_lock = state.acquire_instance_mutation(&instance_id).await;
    ensure_runtime_start_allowed(
        state,
        storage,
        &instance_id,
        source,
        runtime_start_reservation,
    )?;

    if read_active_instance_run(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Err(InstanceRunConflict::ActiveRunRecord.into_error(&instance_id));
    }

    {
        let runtime = state
            .runtime_supervisor
            .lock()
            .map_err(|_| String::from("runtime supervisor lock poisoned"))?;
        if runtime.is_tracked(&instance_id) {
            return Err(InstanceRunConflict::TrackedRunning.into_error(&instance_id));
        }
    }

    if app_storage::read_pending_ark_cluster_restore(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Err("This instance belongs to an interrupted ARK cluster restore. Open cluster maintenance and resolve that restore before starting any member.".into());
    }
    let recovery_paths = storage.paths.clone();
    let recovery_instance_id = instance_id.clone();
    let instance_lock =
        run_runtime_start_worker_to_completion(runtime_start_reservation, async move {
            app_storage::recover_interrupted_instance_runtime(
                &recovery_paths,
                &recovery_instance_id,
            )
            .await
            .map_err(|error| error.to_string())?;
            Ok(instance_lock)
        })
        .await?;
    let instance = read_instance_details(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?;
    // Native configuration can write into an exclusively used library before
    // launch. Reserve its program source before either rendering or starting.
    let program_root = app_storage::resolve_instance_runtime_root(
        super::commands_program_storage::instance_root(&instance)?,
    )
    .map_err(|error| error.to_string())?;
    let shared_install_guard = Some(
        super::commands_runtime_prestart_update::acquire_prestart_program_guard(
            &instance.summary.module_id,
            &program_root,
            runtime_start_reservation,
        )
        .await?,
    );
    // Never render or launch against a partially published native extension.
    super::commands_ark_tools::ensure_can_start_instance(&instance).await?;
    if app_storage::instance_uses_exclusive_program(super::commands_program_storage::instance_root(
        &instance,
    )?)
    .map_err(|error| error.to_string())?
    {
        let program = app_storage::resolve_instance_runtime_root(
            super::commands_program_storage::instance_root(&instance)?,
        )
        .map_err(|error| error.to_string())?;
        app_storage::ensure_program_archive_dependencies(&storage.paths, &program)
            .await
            .map_err(|error| error.to_string())?;
    }
    if let Some(expected) = expected_instance
        && serde_json::to_value(&instance).map_err(|error| error.to_string())?
            != serde_json::to_value(&expected).map_err(|error| error.to_string())?
    {
        return Err(String::from(
            "The server changed after the start operation was prepared. Request a new preview before starting.",
        ));
    }
    // A queued start must observe file changes made by the previous mutation
    // lock owner. InstanceDetails alone does not include private Mod file bytes.
    for expected in &expected_file_changes {
        let current = app_storage::read_instance_patch_file(
            &storage.paths,
            &instance_id,
            &expected.file,
        )
        .await
        .map_err(|error| {
            format!(
                "Cannot verify confirmed instance file `{}` before starting: {error}. Backup {} remains available.",
                expected.file, expected.backup_id
            )
        })?;
        if !expected.read_back_verified || current.source_sha256 != expected.result_sha256 {
            return Err(format!(
                "Confirmed instance file `{}` changed after its patch. Request a new preview before starting; backup {} remains available.",
                expected.file, expected.backup_id
            ));
        }
    }
    let (instance_lock, instance) = if let Some(expected) = expected_world_start {
        let (instance_lock, instance, validation) =
            spawn_blocking_storage_context_task(runtime_start_reservation, move || {
                let _permit = world_preview_permit;
                let validation = validate_dst_world_start_confirmation(&expected, &instance);
                (instance_lock, instance, validation)
            })
            .await
            .map_err(|error| format!("DST start confirmation task failed: {error}"))?;
        validation?;
        (instance_lock, instance)
    } else {
        (instance_lock, instance)
    };
    if source == super::commands_autostart::AUTOSTART_SOURCE
        && (!instance.summary.autostart || !state.autostart.is_eligible(&instance_id)?)
    {
        return Err(String::from(super::commands_autostart::AUTOSTART_CANCELLED));
    }
    let descriptors =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    let descriptor = find_descriptor(&descriptors, &instance.summary.module_id)?;
    ensure_instance_bind_policy_allowed(&instance, &descriptor.runtime.bind_address)?;
    let port_groups =
        super::commands_runtime_ark::port_groups(&instance, &descriptor.runtime.port_groups)?;
    let remapped_ports = app_runtime::remap_taken_port_bindings_for_module(
        &instance.summary.module_id,
        &instance.summary.bind_ip,
        &instance.ports,
        &port_groups,
    )
    .map_err(|error| format!("failed to prepare startup ports for `{instance_id}`: {error}"))?;
    // The snapshot binds the confirmed configuration. Automatic conflict
    // recovery must not change its ports after CAS and before the actual start.
    if preserve_confirmed_ports && remapped_ports.is_some() {
        return Err(String::from(
            "The confirmed ports are no longer available without remapping. Request a new configuration preview before starting.",
        ));
    }
    // A pending native bootstrap may contain partial JSON. Acquire its recovery
    // session before ordinary rendering can parse or overwrite that evidence.
    let windrose_bootstrap = Box::pin(windrose_lifecycle::prepare_if_required(
        &storage.paths,
        &instance,
    ))
    .await?;
    let defer_materialization = windrose_bootstrap.is_some();
    let deferred_ports = if defer_materialization {
        remapped_ports.clone()
    } else {
        None
    };
    let mut program_guards = (instance_lock, shared_install_guard);
    let private_root = private_runtime_install_root(&instance)?;
    let module = map_module_details_with_install_state(
        &storage.settings,
        descriptor,
        Some(private_root.as_str()),
    );
    let package_module = module.clone();
    let startup_log_stamp = current_unix_ms();
    let prestart_update_log_path = if should_run_prestart_update(&package_module) {
        let mut update_log_plans =
            build_process_launch_plans_for_instance(&storage.settings, &module, &instance)?;
        assign_process_log_paths_with_stamp(&instance, &mut update_log_plans, startup_log_stamp);
        update_log_plans.first().map(|plan| plan.log_path.clone())
    } else {
        None
    };
    if let Some(log_path) = prestart_update_log_path.as_deref() {
        publish_pending_start_console_log_path(
            app_handle,
            state,
            &instance_id,
            log_path,
            None,
            Some("Startup"),
            None,
        )?;
        let _ = append_startup_console_line(
            log_path,
            &format!("Preparing {} startup...", instance.summary.name),
            "",
        );
    }
    let install_guard = program_guards
        .1
        .take()
        .ok_or("startup program lifecycle lock is missing")?;
    // Keep the installer state machine out of nested confirmation/start frames
    // so the default Windows thread stack can enter automatic update checks.
    program_guards.1 = Some(
        Box::pin(run_prestart_update_if_needed(
            state,
            storage,
            PrestartUpdateRequest {
                descriptor,
                module: &package_module,
                instance: &instance,
                install_guard,
                reservation: runtime_start_reservation,
                context: PrestartUpdateLogContext {
                    source,
                    console_log_path: prestart_update_log_path.as_deref(),
                },
            },
        ))
        .await?,
    );
    let materialization_paths = storage.paths.clone();
    let materialization_instance_id = instance_id.clone();
    let (guards, materialized_instance) =
        spawn_storage_context_task(runtime_start_reservation, async move {
            let result = if defer_materialization {
                Ok(instance)
            } else {
                materialize_runtime_start_configuration(
                    materialization_paths,
                    instance,
                    remapped_ports,
                    materialization_instance_id,
                )
                .await
            };
            (program_guards, result)
        })
        .await
        .map_err(|error| format!("runtime start materialization task failed: {error}"))?;
    let mut program_guards = guards;
    let mut instance = materialized_instance?;

    let private_root = private_runtime_install_root(&instance)?;
    app_steamcmd::read_program_install_revision(
        Path::new(&storage.settings.servers_root),
        &instance.summary.module_id,
        Path::new(&private_root),
    )
    .map_err(|error| steamcmd_error_message(&error))?;
    let module = map_module_details_with_install_state(
        &storage.settings,
        descriptor,
        Some(private_root.as_str()),
    );
    ensure_module_installed_for_start(&module)?;
    crate::astroneer_console::validate_start_settings(&instance)?;
    super::commands_theforest_control::prepare(storage, &instance).await?;
    let mut launch_plans =
        build_process_launch_plans_for_instance(&storage.settings, &module, &instance)?;
    assign_process_log_paths_with_stamp(&instance, &mut launch_plans, startup_log_stamp);
    if let Some(primary_plan) = launch_plans.first() {
        publish_pending_start_console_log_path(
            app_handle,
            state,
            &instance_id,
            &primary_plan.log_path,
            Some(&primary_plan.process_key),
            Some(&primary_plan.display_name),
            None,
        )?;
        if prestart_update_log_path.as_deref() != Some(primary_plan.log_path.as_str()) {
            let _ = append_startup_console_line(
                &primary_plan.log_path,
                &format!("Preparing {} startup...", instance.summary.name),
                "",
            );
        }
    }
    // Generation happens before run records are published. Attach every shard
    // now so a failed secondary world remains visible in the startup console.
    for plan in launch_plans.iter().skip(1) {
        start_runtime_log_stream(
            app_handle,
            state,
            &instance_id,
            Some(&plan.process_key),
            Some(&plan.display_name),
            None,
            &plan.log_path,
        )?;
    }
    ensure_launch_plans_ready(&launch_plans)?;
    let resource_limits = &launch_plans
        .first()
        .ok_or("Instance has no process launch plans")?
        .launch_plan
        .performance_policy
        .resource_limits;
    if launch_plans
        .iter()
        .any(|plan| &plan.launch_plan.performance_policy.resource_limits != resource_limits)
    {
        return Err("All worlds in an instance must use the same resource limits".into());
    }
    let resource_group = state
        .runtime_resource_admission
        .reserve(&instance_id, resource_limits)
        .map_err(|error| error.to_string())?;
    append_desktop_app_log(
        storage,
        "info",
        "instance.resources.reserved",
        "Reserved the instance process-group resource budget",
        json!({
            "instance_id": instance_id.as_str(), "resource_limits": resource_limits,
            "process_count": launch_plans.len(),
        }),
    );
    #[cfg(all(windows, feature = "desktop-reliability"))]
    let fixture_firewall_boundary = crate::runtime_service::try_fixture_firewall_boundary(
        app_handle,
        &storage.paths,
        &instance,
        &launch_plans,
    )
    .await?;
    #[cfg(not(all(windows, feature = "desktop-reliability")))]
    let fixture_firewall_boundary = false;
    if !fixture_firewall_boundary && !defer_materialization {
        ensure_firewall_rules_for_start(storage, &instance, source).await?;
    }
    let active_runs = list_active_instance_runs(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    let (running_instance_count, active_process_count) =
        summarize_active_runtime_entries(&active_runs);
    let startup_schedule = reserve_runtime_startup_slot(
        state,
        &launch_plans,
        running_instance_count,
        active_process_count,
    )?;
    append_desktop_app_log(
        storage,
        "info",
        "instance.startup.scheduled",
        &startup_schedule.reason,
        json!({
            "source": source,
            "instance_id": instance_id.as_str(),
            "instance_name": instance.summary.name.as_str(),
            "delay_ms": startup_schedule.delay_ms,
            "instance_stagger_ms": startup_schedule.instance_stagger_ms,
            "effective_stagger_ms": startup_schedule.effective_stagger_ms,
            "child_process_stagger_ms": startup_schedule.child_process_stagger_ms,
            "running_instance_count": startup_schedule.running_instance_count,
            "active_process_count": startup_schedule.active_process_count,
            "process_count": startup_schedule.process_count,
            "queued_start_count": startup_schedule.queued_start_count,
        }),
    );
    if startup_schedule.delay_ms > 0 {
        wait_for_runtime_start_delay(
            runtime_start_reservation,
            Duration::from_millis(startup_schedule.delay_ms),
        )
        .await;
    }
    let session_id = Some(generate_session_id(&instance_id));

    let mut spawned_processes = Vec::<(ProcessLaunchPlan, app_runtime::SpawnedProcess)>::new();
    let mut failed_bind_message = None;
    let mut _windrose_bootstrap_guard = None;
    if let Some(prepared) = windrose_bootstrap {
        ensure_runtime_start_allowed(
            state,
            storage,
            &instance_id,
            source,
            runtime_start_reservation,
        )?;
        let primary = launch_plans.first().ok_or("Windrose has no launch plan")?;
        let (guards, bootstrap) = Box::pin(windrose_lifecycle::initialize_if_required(
            app_handle,
            state,
            &instance,
            primary,
            runtime_start_reservation,
            &resource_group,
            windrose_lifecycle::BootstrapAdmission {
                prepared,
                program_guards,
            },
        ))
        .await?;
        program_guards = guards;
        match bootstrap {
            windrose_lifecycle::BootstrapOutcome::Ready => {
                let paths = storage.paths.clone();
                let id = instance_id.clone();
                let (guards, result) =
                    spawn_storage_context_task(runtime_start_reservation, async move {
                        let result = materialize_runtime_start_configuration(
                            paths,
                            instance,
                            deferred_ports,
                            id,
                        )
                        .await;
                        (program_guards, result)
                    })
                    .await
                    .map_err(|error| {
                        format!("Windrose final configuration worker failed: {error}")
                    })?;
                program_guards = guards;
                instance = result?;
                launch_plans =
                    build_process_launch_plans_for_instance(&storage.settings, &module, &instance)?;
                assign_process_log_paths_with_stamp(
                    &instance,
                    &mut launch_plans,
                    startup_log_stamp,
                );
                ensure_launch_plans_ready(&launch_plans)?;
                // Bootstrap deliberately postpones port remapping. Apply rules
                // only to the persisted formal-start ports, never its old snapshot.
                if !fixture_firewall_boundary {
                    ensure_firewall_rules_for_start(storage, &instance, source).await?;
                }
                let primary = launch_plans
                    .first()
                    .ok_or("Windrose has no final launch plan")?;
                publish_pending_start_console_log_path(
                    app_handle,
                    state,
                    &instance_id,
                    &primary.log_path,
                    Some(&primary.process_key),
                    Some(&primary.display_name),
                    None,
                )?;
            }
            windrose_lifecycle::BootstrapOutcome::CleanupPending(pending) => {
                let windrose_lifecycle::PendingBootstrapCleanup {
                    plan,
                    spawned,
                    guard,
                    message,
                } = *pending;
                // Register the retained Job through the normal failure path;
                // configuration remains locked until active ownership is saved.
                _windrose_bootstrap_guard = Some(guard);
                spawned_processes.push((plan, spawned));
                failed_bind_message = Some(message);
            }
        }
    }
    for (index, plan) in launch_plans.iter().enumerate() {
        if failed_bind_message.is_some() {
            break;
        }
        if index > 0 && startup_schedule.child_process_stagger_ms > 0 {
            wait_for_runtime_start_delay(
                runtime_start_reservation,
                Duration::from_millis(startup_schedule.child_process_stagger_ms),
            )
            .await;
        }
        if let Err(error) = ensure_runtime_start_allowed(
            state,
            storage,
            &instance_id,
            source,
            runtime_start_reservation,
        ) {
            let cleanup = stop_spawned_processes(&mut spawned_processes);
            return Err(failed_start_error_with_cleanup(error, &cleanup));
        }
        let spawn_result = if plan.launch_plan.requires_admin || !cfg!(windows) {
            app_runtime::spawn_launch_plan_in_resource_group(
                &plan.launch_plan,
                &plan.log_path,
                None,
                &resource_group,
            )
            .map_err(|error| error.to_string())
        } else {
            app_storage::managed_console_log::ManagedConsoleLog::open(&plan.log_path)
                .map_err(|error| {
                    format!(
                        "Failed to open managed console log {}: {error}",
                        plan.log_path
                    )
                })
                .and_then(|writer| {
                    app_runtime::spawn_launch_plan_in_resource_group(
                        &plan.launch_plan,
                        &plan.log_path,
                        Some(Box::new(writer)),
                        &resource_group,
                    )
                    .map_err(|error| error.to_string())
                })
        };
        match spawn_result {
            Ok(spawned) => spawned_processes.push((plan.clone(), spawned)),
            Err(error) => {
                let cleanup = stop_spawned_processes(&mut spawned_processes);
                return Err(failed_start_error_with_cleanup(error.to_string(), &cleanup));
            }
        }
    }
    resource_group.mark_started();
    try_startup_window_guard_background_windows(
        storage,
        &instance,
        &module,
        build_window_inspection_targets_from_spawned_processes(&spawned_processes),
    );

    let startup_exit = if failed_bind_message.is_none() {
        match detect_startup_exit(&mut spawned_processes, Duration::from_millis(1500)) {
            Ok(exit) => exit,
            Err(error) => {
                let cleanup = stop_spawned_processes(&mut spawned_processes);
                return Err(failed_start_error_with_cleanup(error, &cleanup));
            }
        }
    } else {
        None
    };
    if let Some((failed_index, failed_exit_code)) = startup_exit {
        let failed_plan = &launch_plans[failed_index];
        let exit_label = failed_exit_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| String::from("unknown"));
        let startup_error = format!(
            "instance `{instance_id}` process `{}` exited during startup with code {exit_label}; see log {}",
            failed_plan.display_name, failed_plan.log_path
        );
        let cleanup = stop_spawned_processes(&mut spawned_processes);
        return Err(failed_start_error_with_cleanup(startup_error, &cleanup));
    }

    if failed_bind_message.is_none()
        && let Err(error) = verify_spawned_process_bind_address(
            state,
            storage,
            &instance,
            &module.runtime.bind_address,
            &mut spawned_processes,
            source,
            runtime_start_reservation,
        )
        .await
    {
        let bind_error =
            format!("instance `{instance_id}` failed strict bind-address verification: {error}");
        let cleanup = stop_spawned_processes(&mut spawned_processes);
        if !cleanup.has_survivors() {
            return Err(failed_start_error_with_cleanup(bind_error, &cleanup));
        }

        let statuses = cleanup.statuses;
        spawned_processes = std::mem::take(&mut spawned_processes)
            .into_iter()
            .zip(statuses.iter())
            .filter_map(|(process, status)| status.still_running.then_some(process))
            .collect();
        let cleanup_issue = statuses
            .iter()
            .filter(|status| status.still_running)
            .map(|status| format!("{} pid {}", status.process_key, status.pid))
            .collect::<Vec<_>>()
            .join(", ");
        failed_bind_message = Some(format!(
            "{bind_error}; emergency cleanup could not stop {cleanup_issue}. The surviving process remains registered as running so it can be stopped from the server controls."
        ));
    }

    if module.summary.id == "dontstarve"
        && failed_bind_message.is_none()
        && let Err(error) = dst_lifecycle::wait_until_ready(
            state,
            storage,
            &instance,
            &mut spawned_processes,
            source,
            runtime_start_reservation,
        )
        .await
    {
        let cleanup = stop_spawned_processes(&mut spawned_processes);
        if !cleanup.has_survivors() {
            return Err(failed_start_error_with_cleanup(error, &cleanup));
        }
        failed_bind_message = Some(failed_start_error_with_cleanup(error, &cleanup));
        spawned_processes = std::mem::take(&mut spawned_processes)
            .into_iter()
            .zip(cleanup.statuses.iter())
            .filter_map(|(process, status)| status.still_running.then_some(process))
            .collect();
    }

    let performance_applications = spawned_processes
        .iter()
        .map(|(plan, spawned)| {
            (
                plan.process_key.clone(),
                apply_runtime_performance_policy(
                    spawned.pid,
                    &spawned.process_identity,
                    &plan.launch_plan.performance_policy,
                ),
            )
        })
        .collect::<HashMap<_, _>>();
    for (process_key, application) in &performance_applications {
        append_desktop_app_log(
            storage,
            "info",
            "instance.runtime_performance.applied",
            "Runtime performance policy applied to started process tree",
            json!({
                "source": source,
                "instance_id": instance_id.as_str(),
                "instance_name": instance.summary.name.as_str(),
                "process_key": process_key,
                "pid": application.pid,
                "priority_class": &application.priority_class,
                "cpu_affinity_mask": application.cpu_affinity_mask,
                "apply_to_child_processes": application.apply_to_child_processes,
                "targeted_process_count": application.targeted_process_count,
                "priority_applied_count": application.priority_applied_count,
                "affinity_applied_count": application.affinity_applied_count,
                "warnings": &application.warnings,
            }),
        );
    }

    let mut registered_processes = Vec::with_capacity(spawned_processes.len());
    for (index, (plan, spawned)) in spawned_processes.iter_mut().enumerate() {
        match mark_instance_process_started_with_identity(
            &storage.paths,
            &StartedInstanceProcess {
                instance_id: &instance_id,
                session_id: session_id.as_deref(),
                process_key: &plan.process_key,
                display_name: &plan.display_name,
                pid: spawned.pid,
                log_path: &spawned.log_path,
                is_primary: index == 0,
            },
            Some(&spawned.process_identity),
        )
        .await
        {
            Ok(process) => {
                start_runtime_log_stream(
                    app_handle,
                    state,
                    &instance_id,
                    Some(&plan.process_key),
                    Some(&plan.display_name),
                    Some(process.run_id),
                    &spawned.log_path,
                )?;
                registered_processes.push(process);
            }
            Err(error) => {
                let cleanup = stop_spawned_processes(&mut spawned_processes);
                for (process, status) in registered_processes.iter().zip(cleanup.statuses.iter()) {
                    if status.still_running {
                        continue;
                    }
                    let _ = mark_instance_process_stopped(
                        &storage.paths,
                        &instance_id,
                        process.run_id,
                        status.exit_code,
                        true,
                    )
                    .await;
                }
                let instances = list_instances(&storage.paths)
                    .await
                    .map_err(|load_error| load_error.to_string())?;
                update_state_instances(state, instances)?;
                return Err(failed_start_error_with_cleanup(error.to_string(), &cleanup));
            }
        }
    }

    let instances = list_instances(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    let summary =
        find_instance_summary(&instances, &instance_id).unwrap_or_else(|| InstanceSummary {
            id: instance.summary.id.clone(),
            name: instance.summary.name.clone(),
            module_id: instance.summary.module_id.clone(),
            status: InstanceStatus::Running,
            active_process_count: registered_processes.len(),
            bind_ip: instance.summary.bind_ip.clone(),
            port_count: instance.summary.port_count,
            autostart: instance.summary.autostart,
        });

    let started_processes = registered_processes
        .iter()
        .filter_map(|process| {
            let pid = process.pid?;
            Some(app_core::StartedProcess {
                run_id: process.run_id,
                process_key: process.process_key.clone(),
                display_name: process.display_name.clone(),
                pid,
                log_path: process.log_path.clone()?,
                performance: performance_applications
                    .get(&process.process_key)
                    .cloned()
                    .unwrap_or_else(|| skipped_runtime_performance_application(pid)),
            })
        })
        .collect::<Vec<_>>();
    let primary_process = started_processes
        .first()
        .cloned()
        .ok_or_else(|| String::from("no started processes were recorded"))?;
    let managed_processes = spawned_processes
        .into_iter()
        .zip(registered_processes.iter())
        .map(|((plan, spawned), process)| {
            let last_performance_target_count = performance_applications
                .get(&plan.process_key)
                .map(|application| application.targeted_process_count);
            let last_performance_application =
                performance_applications.get(&plan.process_key).cloned();
            ManagedProcess {
                run_id: process.run_id,
                process_key: plan.process_key,
                display_name: plan.display_name,
                pid: spawned.pid,
                process_identity: spawned.process_identity,
                root_process_identity: spawned.root_process_identity,
                log_path: spawned.log_path,
                is_primary: process.is_primary,
                uses_script_entrypoint: spawned.uses_script_entrypoint,
                performance_policy: plan.launch_plan.performance_policy,
                last_performance_refresh: Some(Instant::now()),
                last_performance_target_count,
                last_performance_application,
                child: spawned.child,
                hidden_desktop: spawned.hidden_desktop,
            }
        })
        .collect::<Vec<_>>();

    {
        let mut runtime = state
            .runtime_supervisor
            .lock()
            .map_err(|_| String::from("runtime supervisor lock poisoned"))?;
        runtime.insert_running(summary.clone(), session_id.clone(), managed_processes);
    }

    update_state_instances(state, instances)?;
    if let Some(message) = failed_bind_message {
        append_desktop_app_log(
            storage,
            "error",
            "instance.bind_address.cleanup_survivor_tracked",
            &message,
            json!({
                "source": source,
                "instance_id": instance_id.as_str(),
                "instance_name": instance.summary.name.as_str(),
                "started_processes": &started_processes,
            }),
        );
        return Err(message);
    }
    try_auto_suppress_background_windows(storage, &instance, &module, &registered_processes);
    if source == "manual" {
        cancel_pending_runtime_restart(state, storage, &instance_id, &instance.summary.name);
    }
    drop(program_guards);
    Ok(StartInstanceResult {
        summary,
        run_id: primary_process.run_id,
        session_id,
        pid: primary_process.pid,
        log_path: primary_process.log_path.clone(),
        launch_plan: launch_plans[0].launch_plan.clone(),
        process_count: started_processes.len(),
        processes: started_processes,
        launch_plans,
        startup_schedule,
    })
}

async fn materialize_runtime_start_configuration(
    paths: app_storage::StoragePaths,
    instance: InstanceDetails,
    remapped_ports: Option<Vec<PortBinding>>,
    instance_id: String,
) -> Result<InstanceDetails, String> {
    let previous_ports = if let Some(remapped_ports) = remapped_ports {
        let previous_ports = instance.ports.clone();
        update_instance_ports(&paths, &instance.summary.id, &remapped_ports)
            .await
            .map_err(|error| {
                format!("failed to persist startup port remap for `{instance_id}`: {error}")
            })?;
        Some(previous_ports)
    } else {
        None
    };

    match materialize_instance_configuration_for_start(&paths, &instance.summary.id).await {
        Ok(materialized) => Ok(materialized),
        Err(error) => {
            let Some(previous_ports) = previous_ports else {
                return Err(format!(
                    "failed to materialize startup configuration for `{instance_id}`: {error}"
                ));
            };
            let rollback =
                match update_instance_ports(&paths, &instance.summary.id, &previous_ports).await {
                    Ok(_) => materialize_instance_configuration(&paths, &instance.summary.id)
                        .await
                        .map(|_| ()),
                    Err(rollback_error) => Err(rollback_error),
                };
            Err(match rollback {
                Ok(()) => {
                    format!("failed to render remapped startup ports for `{instance_id}`: {error}")
                }
                Err(rollback_error) => format!(
                    "failed to render remapped startup ports for `{instance_id}`: {error}; rollback failed: {rollback_error}"
                ),
            })
        }
    }
}

pub(super) fn reserve_runtime_instance_start(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    instance_id: &str,
    source: &str,
) -> Result<RuntimeStartReservationLease, String> {
    match state.try_reserve_runtime_start(instance_id, source)? {
        RuntimeStartReservationAttempt::Reserved(reservation) => Ok(reservation),
        RuntimeStartReservationAttempt::ShutdownInProgress => Err(
            runtime_start_blocked_by_app_exit_error(storage, instance_id, source),
        ),
        RuntimeStartReservationAttempt::Conflict(conflict) => {
            if source == super::commands_autostart::AUTOSTART_SOURCE {
                return Err(String::from(
                    super::commands_autostart::AUTOSTART_ALREADY_ACTIVE,
                ));
            }
            let message = format!(
                "instance `{}` is already starting from `{}` ({} ms pending)",
                conflict.instance_id, conflict.source, conflict.pending_ms
            );
            append_desktop_app_log(
                storage,
                "warn",
                "instance.startup.blocked_pending_start",
                &message,
                json!({
                    "instance_id": instance_id,
                    "source": source,
                    "pending_source": conflict.source.as_str(),
                    "pending_ms": conflict.pending_ms,
                }),
            );
            Err(message)
        }
    }
}

fn ensure_runtime_start_allowed(
    state: &DesktopState,
    storage: &StorageBootstrap,
    instance_id: &str,
    source: &str,
    reservation: &RuntimeStartReservationLease,
) -> Result<(), String> {
    // A failed shutdown reopens admission, but must never revive its old starts.
    if state.shutdown_in_progress.load(Ordering::SeqCst) || reservation.is_cancelled() {
        return Err(runtime_start_blocked_by_app_exit_error(
            storage,
            instance_id,
            source,
        ));
    }
    if source == "auto_restart"
        && !state
            .runtime_restart_scheduler
            .lock()
            .map_err(|_| String::from("runtime restart scheduler lock poisoned"))?
            .restart_allowed(instance_id)
    {
        return Err(String::from(
            "Automatic restart was cancelled by a newer runtime action.",
        ));
    }
    if source == super::commands_autostart::AUTOSTART_SOURCE
        && !state.autostart.is_eligible(instance_id)?
    {
        return Err(String::from(super::commands_autostart::AUTOSTART_CANCELLED));
    }
    Ok(())
}

fn runtime_start_blocked_by_app_exit_error(
    storage: &StorageBootstrap,
    instance_id: &str,
    source: &str,
) -> String {
    let message =
        format!("instance `{instance_id}` cannot start because application shutdown was requested");
    append_desktop_app_log(
        storage,
        "warn",
        "instance.startup.blocked_app_exit",
        &message,
        json!({
            "instance_id": instance_id,
            "source": source,
        }),
    );
    message
}

pub(super) fn publish_pending_start_console_log_path(
    app_handle: Option<&tauri::AppHandle>,
    state: &tauri::State<'_, DesktopState>,
    instance_id: &str,
    log_path: &str,
    process_key: Option<&str>,
    display_name: Option<&str>,
    run_id: Option<i64>,
) -> Result<(), String> {
    state
        .runtime_start_reservations
        .lock()
        .map_err(|_| String::from("runtime start reservation lock poisoned"))?
        .set_console_log_path(instance_id, log_path);
    start_runtime_log_stream(
        app_handle,
        state,
        instance_id,
        process_key,
        display_name,
        run_id,
        log_path,
    )?;
    Ok(())
}

pub(super) use runtime_logs::{
    finish_runtime_log_stream_for_process, start_runtime_game_log_stream, start_runtime_log_stream,
    stop_runtime_log_stream_for_process, stop_runtime_log_streams_for_instance,
};

pub(super) fn pending_start_console_log_path(
    state: &tauri::State<'_, DesktopState>,
    instance_id: &str,
) -> Option<String> {
    state
        .runtime_start_reservations
        .lock()
        .ok()
        .and_then(|reservations| reservations.pending_console_log_path(instance_id))
}

pub(super) fn pending_start_console_log_snapshot(
    state: &tauri::State<'_, DesktopState>,
    instance_id: &str,
    max_lines: usize,
) -> Option<LogTailSnapshot> {
    pending_start_console_log_path(state, instance_id)
        .map(|log_path| read_log_path_snapshot(log_path, max_lines))
}

/// The restart coordinator owns the instance lock and the recovery ticket.
/// Failed save confirmation leaves the survivor managed; never force cleanup.
pub(super) async fn stop_dst_survivors_for_restart(
    state: &DesktopState,
    storage: &StorageBootstrap,
    instance_id: &str,
    active: &ActiveInstanceRun,
) -> Result<(), String> {
    let details = read_instance_details(&storage.paths, instance_id)
        .await
        .map_err(|error| error.to_string())?;
    if details.summary.module_id != "dontstarve" {
        return Err(String::from(
            "Shard recovery is only supported for DST instances.",
        ));
    }
    dst_lifecycle::save_and_shutdown(state, storage, &details).await?;
    dst_lifecycle::finish_confirmed_stop(state, storage, instance_id, active).await?;
    Ok(())
}

#[tauri::command]
pub async fn stop_instance_process(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
) -> Result<StopInstanceResult, String> {
    state
        .runtime_restart_scheduler
        .lock()
        .map_err(|_| String::from("runtime restart scheduler lock poisoned"))?
        .request_stop(&instance_id);
    state.autostart.cancel(&instance_id)?;
    stop_instance_process_with_precondition(state, instance_id, None).await
}

pub(super) async fn stop_instance_process_with_precondition(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    expected_instance: Option<InstanceDetails>,
) -> Result<StopInstanceResult, String> {
    let storage_context_operation = state.begin_storage_context_operation("runtime stop")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;
    let _instance_lock = state.acquire_instance_mutation(&instance_id).await;

    let active_run = read_active_instance_run(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| InstanceRunConflict::NotRunning.into_error(&instance_id))?;
    let details = read_instance_details(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?;
    if let Some(expected) = expected_instance {
        if serde_json::to_value(&details).map_err(|error| error.to_string())?
            != serde_json::to_value(&expected).map_err(|error| error.to_string())?
        {
            return Err(
                "The server changed after the stop preview. Request a new confirmation.".into(),
            );
        }
        state
            .runtime_restart_scheduler
            .lock()
            .map_err(|_| String::from("runtime restart scheduler lock poisoned"))?
            .request_stop(&instance_id);
        state.autostart.cancel(&instance_id)?;
    }
    let shutdown_by_module = load_module_shutdown_specs(&storage, InstanceShutdownSource::Manual);
    let shutdown = shutdown_by_module.get(&details.summary.module_id);

    append_desktop_app_log(
        &storage,
        "info",
        "instance.stop.started",
        "Stopping managed instance with its declared shutdown strategy.",
        json!({
            "instance_id": instance_id.as_str(),
            "instance_name": details.summary.name.as_str(),
            "module_id": details.summary.module_id.as_str(),
            "has_shutdown_strategy": shutdown.is_some(),
        }),
    );

    let stop_result = super::commands_instance_stop::stop_run(
        &state,
        &storage,
        &storage_context_operation,
        &details,
        &active_run,
        shutdown,
        InstanceShutdownSource::Manual,
    )
    .await;
    let stopped_processes = match stop_result {
        Ok(stopped) => stopped,
        Err(error) => {
            append_desktop_app_log(
                &storage,
                "error",
                "instance.stop.failed",
                &error,
                json!({
                    "instance_id": instance_id.as_str(),
                    "instance_name": details.summary.name.as_str(),
                    "module_id": details.summary.module_id.as_str(),
                }),
            );
            return Err(error);
        }
    };

    let instances = list_instances(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    let summary =
        find_instance_summary(&instances, &instance_id).unwrap_or_else(|| InstanceSummary {
            id: instance_id.clone(),
            name: instance_id.clone(),
            module_id: String::new(),
            status: InstanceStatus::Stopped,
            active_process_count: 0,
            bind_ip: String::from("0.0.0.0"),
            port_count: 0,
            autostart: false,
        });

    update_state_instances(&state, instances)?;

    super::commands_instance_stop::backup_after_stop(
        &storage,
        &details,
        InstanceShutdownSource::Manual,
    )
    .await;

    let primary_process = stopped_processes
        .iter()
        .find(|process| process.run_id == active_run.run_id)
        .or_else(|| stopped_processes.first())
        .cloned();

    let result = StopInstanceResult {
        summary,
        run_id: active_run.run_id,
        session_id: active_run.session_id.clone(),
        pid: primary_process.as_ref().and_then(|process| process.pid),
        log_path: primary_process
            .as_ref()
            .and_then(|process| process.log_path.clone()),
        exit_code: primary_process
            .as_ref()
            .and_then(|process| process.exit_code),
        process_count: stopped_processes.len(),
        processes: stopped_processes,
    };

    append_desktop_app_log(
        &storage,
        "info",
        "instance.stop.completed",
        "Managed instance stopped successfully.",
        json!({
            "instance_id": instance_id.as_str(),
            "instance_name": details.summary.name.as_str(),
            "module_id": details.summary.module_id.as_str(),
            "process_count": result.process_count,
        }),
    );

    Ok(result)
}

#[derive(Clone, Copy)]
pub(super) enum InstanceShutdownSource {
    Manual,
    AppExit,
}

impl InstanceShutdownSource {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::AppExit => "app_exit",
        }
    }
}

pub(super) fn load_module_shutdown_specs(
    storage: &StorageBootstrap,
    source: InstanceShutdownSource,
) -> HashMap<String, ModuleShutdownSpec> {
    match discover_modules(&storage.paths.modules_root) {
        Ok(descriptors) => descriptors
            .into_iter()
            .filter_map(|descriptor| {
                descriptor
                    .runtime
                    .shutdown
                    .map(|shutdown| (descriptor.summary.id, shutdown))
            })
            .collect(),
        Err(error) => {
            append_desktop_app_log(
                storage,
                "warning",
                "instance.shutdown.strategy_load_failed",
                &error.to_string(),
                json!({
                    "source": source.as_str(),
                    "modules_root": storage.paths.modules_root.to_string_lossy(),
                }),
            );
            HashMap::new()
        }
    }
}

pub(super) async fn join_lan_host_for_app_exit(state: &DesktopState) -> Result<(), String> {
    let Some(host_thread) = state.take_lan_host_thread()? else {
        return Ok(());
    };

    match tokio::task::spawn_blocking(move || host_thread.join()).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) => Err(String::from("LAN host thread panicked during app exit")),
        Err(error) => Err(format!("failed to join LAN host thread: {error}")),
    }
}

pub(super) async fn join_lan_directory_for_app_exit(
    state: &DesktopState,
) -> Result<Option<String>, String> {
    let Some(directory_worker) = state.take_lan_directory_worker()? else {
        return Ok(None);
    };
    let directory_thread = directory_worker.cancel_and_take_thread();

    match tokio::task::spawn_blocking(move || directory_thread.join()).await {
        Ok(Ok(())) => Ok(None),
        // Joining a panicked discovery worker still proves that it has exited.
        // Preserve its cause as a diagnostic; it owns no server or save data.
        Ok(Err(payload)) => {
            let cause = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("non-text panic payload");
            Ok(Some(format!(
                "LanGame LAN directory worker panicked: {cause}"
            )))
        }
        Err(error) => Err(format!(
            "failed to join LanGame LAN directory thread: {error}"
        )),
    }
}

#[derive(Debug, Clone, Copy)]
enum AppShutdownCompletion {
    Exit,
    #[cfg(windows)]
    FinalExit,
    #[cfg(not(windows))]
    Restart,
}

impl AppShutdownCompletion {
    fn as_str(self) -> &'static str {
        match self {
            Self::Exit => "exit",
            #[cfg(windows)]
            Self::FinalExit => "tray_exit",
            #[cfg(not(windows))]
            Self::Restart => "restart",
        }
    }

    fn is_final(self) -> bool {
        match self {
            #[cfg(windows)]
            Self::FinalExit => true,
            _ => false,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum AppShutdownDecision {
    Complete,
    Retry(String),
    AwaitDeadline(String),
}

pub fn request_app_exit_shutdown(app_handle: tauri::AppHandle) {
    request_app_shutdown(app_handle, AppShutdownCompletion::Exit);
}

#[cfg(not(windows))]
pub fn request_app_restart_shutdown(app_handle: tauri::AppHandle) {
    request_app_shutdown(app_handle, AppShutdownCompletion::Restart);
}

fn request_app_shutdown(app_handle: tauri::AppHandle, completion: AppShutdownCompletion) {
    tauri::async_runtime::spawn(async move {
        if prepare_app_shutdown(&app_handle, completion).await.is_ok() {
            match completion {
                AppShutdownCompletion::Exit => app_handle.exit(0),
                #[cfg(windows)]
                AppShutdownCompletion::FinalExit => app_handle.exit(0),
                #[cfg(not(windows))]
                AppShutdownCompletion::Restart => app_handle.restart(),
            }
        }
    });
}

/// The runtime service sends the completed result before exiting its event loop.
/// This future belongs to the service connection task, not the desktop client.
#[cfg(windows)]
pub(crate) async fn prepare_runtime_service_shutdown(
    app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    prepare_app_shutdown(app_handle, AppShutdownCompletion::Exit).await
}

#[cfg(windows)]
pub(crate) async fn prepare_runtime_service_tray_exit(
    app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    prepare_app_shutdown(app_handle, AppShutdownCompletion::FinalExit).await
}

async fn prepare_app_shutdown(
    app_handle: &tauri::AppHandle,
    completion: AppShutdownCompletion,
) -> Result<(), String> {
    let state = app_handle.state::<DesktopState>();
    if !state.begin_app_shutdown(completion.is_final())? {
        return Err("Application shutdown is already in progress".into());
    }

    // Reserve stable paths before drain can take exclusivity. The unified stop
    // owns a normal lease and each instance lock, so unrelated admitted writes
    // can settle concurrently without delaying every game's save command.
    let shutdown_work = state
        .reserve_storage_shutdown_operation(APP_STORAGE_DRAIN_TIMEOUT)
        .await;
    let (instance_result, storage_result) = match shutdown_work {
        Ok(lease) => tokio::join!(
            async {
                let result = shutdown_all_running_instances_for_app_exit(app_handle, &lease).await;
                drop(lease);
                result
            },
            state.drain_storage_operations_for_shutdown(APP_STORAGE_DRAIN_TIMEOUT),
        ),
        Err(error) => (Ok(()), Err(error)),
    };
    let storage_shutdown = match storage_result {
        Ok(storage_shutdown) => storage_shutdown,
        Err(error) => {
            let decision = decide_app_shutdown_completion(
                &state,
                Err(match instance_result {
                    Ok(()) => {
                        format!("application shutdown could not seal storage operations: {error}")
                    }
                    Err(stop_error) => format!(
                        "{stop_error}; application shutdown could not seal storage operations: {error}"
                    ),
                }),
                completion.is_final(),
            );
            if let AppShutdownDecision::AwaitDeadline(error) = decision {
                return Err(error);
            }
            if let AppShutdownDecision::Retry(error) = decision {
                report_app_shutdown_failure(app_handle, &state, completion, &error);
                return Err(error);
            }
            return Err("Application shutdown could not acquire storage exclusivity".into());
        }
    };

    // Discovery and LAN joins must not delay delivery of game save commands.
    let (lan_directory_result, lan_host_result) = tokio::join!(
        join_lan_directory_for_app_exit(&state),
        join_lan_host_for_app_exit(&state),
    );
    if let Ok(Some(warning)) = &lan_directory_result {
        crate::lan_directory::report_shutdown_warning(warning);
    }
    let attempt_result = aggregate_app_shutdown_phase_results(
        lan_directory_result.map(|_| ()),
        lan_host_result,
        instance_result,
    );

    match attempt_result {
        Ok(()) => {
            runtime_logs::wait_for_shutdown(app_handle).await;
            state.knowledge.shutdown().await;
            let decision = decide_app_shutdown_completion(&state, Ok(()), completion.is_final());
            debug_assert_eq!(decision, AppShutdownDecision::Complete);
            storage_shutdown.commit_for_process_exit();
            Ok(())
        }
        Err(error) => {
            if completion.is_final() || state.is_final_exit_requested() {
                // Keep admission sealed until the independent watchdog finishes.
                // This commits exclusivity only, never a successful save receipt.
                storage_shutdown.commit_for_process_exit();
                return match decide_app_shutdown_completion(&state, Err(error), true) {
                    AppShutdownDecision::AwaitDeadline(error)
                    | AppShutdownDecision::Retry(error) => Err(error),
                    AppShutdownDecision::Complete => Ok(()),
                };
            }
            drop(storage_shutdown);
            let error = recover_lan_directory_after_failed_shutdown(&state, error, || {
                crate::lan_directory::spawn_lan_directory_broadcaster(app_handle.clone())
            });
            let decision = decide_app_shutdown_completion(&state, Err(error), false);
            if let AppShutdownDecision::Retry(error) = decision {
                report_app_shutdown_failure(app_handle, &state, completion, &error);
                return Err(error);
            }
            if let AppShutdownDecision::AwaitDeadline(error) = decision {
                return Err(error);
            }
            Err("Application shutdown did not complete".into())
        }
    }
}

fn recover_lan_directory_after_failed_shutdown(
    state: &DesktopState,
    shutdown_error: String,
    restart: impl FnOnce() -> Result<(), String>,
) -> String {
    if state.is_final_exit_requested() {
        return shutdown_error;
    }
    match restart() {
        Ok(()) => shutdown_error,
        Err(restart_error) => {
            format!("{shutdown_error}; LanGame LAN directory recovery failed: {restart_error}")
        }
    }
}

fn aggregate_app_shutdown_phase_results(
    lan_directory_result: Result<(), String>,
    lan_host_result: Result<(), String>,
    instance_result: Result<(), String>,
) -> Result<(), String> {
    let mut failures = Vec::new();
    if let Err(error) = lan_directory_result {
        failures.push(format!("LanGame LAN directory shutdown: {error}"));
    }
    if let Err(error) = lan_host_result {
        failures.push(format!("LAN host shutdown: {error}"));
    }
    if let Err(error) = instance_result {
        failures.push(format!("managed instance shutdown: {error}"));
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "application shutdown did not complete ({})",
            failures.join("; ")
        ))
    }
}

fn decide_app_shutdown_completion(
    state: &DesktopState,
    attempt_result: Result<(), String>,
    final_exit: bool,
) -> AppShutdownDecision {
    if final_exit {
        state.request_final_exit();
    }
    match attempt_result {
        Ok(()) => {
            state.shutdown_completed.store(true, Ordering::SeqCst);
            AppShutdownDecision::Complete
        }
        Err(error) => {
            state.shutdown_completed.store(false, Ordering::SeqCst);
            match state.release_app_shutdown_for_retry() {
                Ok(true) => AppShutdownDecision::Retry(error),
                Ok(false) => AppShutdownDecision::AwaitDeadline(error),
                Err(coordination_error) => {
                    AppShutdownDecision::AwaitDeadline(format!("{error}; {coordination_error}"))
                }
            }
        }
    }
}

fn report_app_shutdown_failure(
    app_handle: &tauri::AppHandle,
    state: &DesktopState,
    completion: AppShutdownCompletion,
    error: &str,
) {
    if state.is_final_exit_requested() {
        return;
    }
    record_app_shutdown_failure(state, completion, error);
    if state.is_final_exit_requested() {
        return;
    }
    if let Some(window) = app_handle.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    if let Err(emit_error) = app_handle.emit(
        APP_SHUTDOWN_FAILED_EVENT,
        json!({
            "completion": completion.as_str(),
            "message": error,
        }),
    ) {
        eprintln!(
            "failed to surface aborted application {}: {error}; event emission failed: {emit_error}",
            completion.as_str()
        );
    }
}

fn record_app_shutdown_failure(
    state: &DesktopState,
    completion: AppShutdownCompletion,
    error: &str,
) {
    let _storage_context_operation = match state
        .begin_storage_context_operation("app shutdown failure logging")
    {
        Ok(operation) => operation,
        Err(coordinator_error) => {
            eprintln!(
                "failed to record aborted application {}: {error}; storage coordinator rejected failure logging: {coordinator_error}",
                completion.as_str()
            );
            return;
        }
    };
    match bootstrap_storage() {
        Ok(storage) => append_desktop_app_log(
            &storage,
            "error",
            "app.exit.shutdown_aborted",
            error,
            json!({
                "requested_completion": completion.as_str(),
            }),
        ),
        Err(storage_error) => eprintln!(
            "failed to record aborted application {}: {error}; storage bootstrap failed: {storage_error}",
            completion.as_str()
        ),
    }
}

pub fn app_exit_shutdown_completed(app_handle: &tauri::AppHandle) -> bool {
    app_handle
        .state::<DesktopState>()
        .shutdown_completed
        .load(Ordering::SeqCst)
}

pub(super) async fn shutdown_all_running_instances_for_app_exit<L: StorageContextTaskLease>(
    app_handle: &tauri::AppHandle,
    storage_lease: &L,
) -> Result<(), String> {
    let state = app_handle.state::<DesktopState>();
    // Capture reservations before active runs: a start may publish its run and
    // release its reservation between these reads, but cannot evade both.
    // Shutdown admission prevents registration after this reservation snapshot.
    let pending_start_instance_ids = state.pending_runtime_start_instance_ids()?;
    // Shutdown admission is sealed and the caller retains the stable storage paths.
    // A desktop that never initialized storage and owns no processes has no instance cleanup.
    if pending_start_instance_ids.is_empty() && !app_exit_requires_instance_storage(&state)? {
        return Ok(());
    }
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;

    let mut active_instance_ids = list_active_instance_runs(&storage.paths)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|active_run| active_run.instance_id)
        .collect::<Vec<_>>();
    active_instance_ids.sort();
    active_instance_ids.dedup();
    let running_instance_count = active_instance_ids.len();
    // Existing running servers receive stop slots before starts still settling.
    // Each stop worker waits only for its own instance mutation lock.
    for instance_id in &pending_start_instance_ids {
        if !active_instance_ids.contains(instance_id) {
            active_instance_ids.push(instance_id.clone());
        }
    }
    if active_instance_ids.is_empty() {
        append_desktop_app_log(
            &storage,
            "info",
            "app.exit.no_running_instances",
            "LanGame is exiting with no running managed instances.",
            json!({}),
        );
        return Ok(());
    }

    let shutdown_by_module = load_module_shutdown_specs(&storage, InstanceShutdownSource::AppExit);

    append_desktop_app_log(
        &storage,
        "info",
        "app.exit.shutdown_started",
        "LanGame is stopping all running managed instances before exit.",
        json!({
            "running_instance_count": running_instance_count,
            "pending_start_count": pending_start_instance_ids.len(),
        }),
    );

    let shutdown_failures = shutdown_batch::run(
        app_handle,
        &storage,
        storage_lease,
        active_instance_ids,
        shutdown_by_module,
    )
    .await;

    let state_refresh_error = match list_instances(&storage.paths).await {
        Ok(instances) => update_desktop_state_instances(&state, instances).err(),
        Err(error) => Some(error.to_string()),
    };
    let residual_active_run_error = match list_active_instance_runs(&storage.paths).await {
        Ok(active_runs) if active_runs.is_empty() => None,
        Ok(active_runs) => Some(format!(
            "{} active process record(s) remain: {}",
            active_runs.len(),
            active_runs
                .iter()
                .map(|run| format!(
                    "{} run {} session {} process {}",
                    run.instance_id,
                    run.run_id,
                    run.session_id.as_deref().unwrap_or("unknown"),
                    run.process_key
                ))
                .collect::<Vec<_>>()
                .join(", ")
        )),
        Err(error) => Some(format!("final active-run verification failed: {error}")),
    };
    aggregate_app_exit_shutdown_failures(
        &shutdown_failures,
        residual_active_run_error.as_deref(),
        state_refresh_error.as_deref(),
    )?;

    append_desktop_app_log(
        &storage,
        "info",
        "app.exit.shutdown_completed",
        "LanGame finished stopping managed instances before exit.",
        json!({}),
    );

    Ok(())
}

fn app_exit_requires_instance_storage(state: &DesktopState) -> Result<bool, String> {
    let storage_initialized = state
        .app_state
        .read()
        .map_err(|_| String::from("desktop state lock poisoned"))?
        .storage
        .migrations_applied;
    // Successful initialization remains recorded even if the database later becomes unavailable.
    if storage_initialized {
        return Ok(true);
    }

    let runtime = state
        .runtime_supervisor
        .lock()
        .map_err(|_| String::from("runtime supervisor lock poisoned"))?;
    Ok(!runtime.tracked_instances().is_empty())
}

fn aggregate_app_exit_shutdown_failures(
    shutdown_failures: &[(String, String)],
    residual_active_run_error: Option<&str>,
    state_refresh_error: Option<&str>,
) -> Result<(), String> {
    let mut failures = Vec::new();
    if !shutdown_failures.is_empty() {
        failures.push(format!(
            "failed to stop {} managed instance(s): {}",
            shutdown_failures.len(),
            shutdown_failures
                .iter()
                .map(|(instance_id, error)| format!("{instance_id}: {error}"))
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    if let Some(error) = residual_active_run_error {
        failures.push(format!("active runtime records remain: {error}"));
    }
    if let Some(error) = state_refresh_error {
        failures.push(format!("desktop state refresh failed: {error}"));
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

pub(super) async fn shutdown_one_running_instance_for_app_exit<L: StorageContextTaskLease>(
    state: &DesktopState,
    storage: &StorageBootstrap,
    storage_lease: &L,
    instance_id: &str,
    shutdown_by_module: &HashMap<String, ModuleShutdownSpec>,
) -> Result<Option<InstanceDetails>, String> {
    let _instance_lock = state.acquire_instance_mutation(instance_id).await;
    let mut next_active_run = read_active_instance_run(&storage.paths, instance_id)
        .await
        .map_err(|error| error.to_string())?;
    if next_active_run.is_none() {
        return Ok(None);
    }
    let details = read_instance_details(&storage.paths, instance_id)
        .await
        .map_err(|error| error.to_string())?;
    let shutdown = shutdown_by_module.get(&details.summary.module_id);
    let mut stopped_process_count = 0usize;
    let mut previous_active_run = None;
    for attempt in 0..APP_EXIT_MAX_ACTIVE_RUN_DRAIN_ATTEMPTS {
        let Some(active_run) = next_active_run.take() else {
            break;
        };
        let active_run_key = (active_run.run_id, active_run.session_id.clone());
        if previous_active_run.as_ref() == Some(&active_run_key) {
            return Err(format!(
                "active run {} for instance `{instance_id}` made no progress during app-exit shutdown",
                active_run.run_id
            ));
        }
        previous_active_run = Some(active_run_key);

        let stopped_processes = super::commands_instance_stop::stop_run(
            state,
            storage,
            storage_lease,
            &details,
            &active_run,
            shutdown,
            InstanceShutdownSource::AppExit,
        )
        .await?;
        stopped_process_count = stopped_process_count.saturating_add(stopped_processes.len());

        next_active_run = read_active_instance_run(&storage.paths, instance_id)
            .await
            .map_err(|error| error.to_string())?;
        if attempt + 1 == APP_EXIT_MAX_ACTIVE_RUN_DRAIN_ATTEMPTS && next_active_run.is_some() {
            return Err(format!(
                "instance `{instance_id}` still has active runs after {APP_EXIT_MAX_ACTIVE_RUN_DRAIN_ATTEMPTS} shutdown attempts"
            ));
        }
    }

    if stopped_process_count == 0 {
        return Ok(None);
    }

    append_desktop_app_log(
        storage,
        "info",
        "app.exit.instance_stopped",
        "Managed instance stopped during app exit.",
        json!({
            "instance_id": instance_id,
            "process_count": stopped_process_count,
        }),
    );

    Ok(Some(details))
}

pub(super) async fn request_instance_graceful_shutdown(
    state: &DesktopState,
    storage: &StorageBootstrap,
    details: &InstanceDetails,
    shutdown: Option<&ModuleShutdownSpec>,
    source: InstanceShutdownSource,
) -> Result<(), String> {
    if shutdown_wait::wait_for_instance_exit(state, &details.summary.id, Duration::ZERO).await? {
        return Ok(());
    }
    if details.summary.module_id == "dontstarve" {
        let result = dst_lifecycle::save_and_shutdown(state, storage, details).await;
        if let Err(error) = &result {
            append_desktop_app_log(
                storage,
                "error",
                "instance.dst.stop_unconfirmed",
                error,
                json!({"instance_id": details.summary.id, "source": source.as_str()}),
            );
        }
        return result;
    }
    let Some(shutdown) = shutdown else {
        append_desktop_app_log(
            storage,
            "warning",
            "instance.shutdown.strategy_missing",
            "Module has no declared shutdown strategy; managed process ownership is retained without forcing the instance to stop.",
            json!({
                "source": source.as_str(),
                "instance_id": details.summary.id.as_str(),
                "module_id": details.summary.module_id.as_str(),
            }),
        );
        return Err("Module has no declared shutdown strategy; managed process ownership is retained and no forced stop was attempted.".into());
    };

    let mapped_shutdown = if app_core::ark_maps::is_ark(&details.summary.module_id) {
        Some(super::commands_runtime_ark::shutdown_for_running_maps(
            details, shutdown,
        )?)
    } else {
        None
    };
    dispatch_instance_shutdown_commands(
        state,
        storage,
        details,
        mapped_shutdown.as_ref().unwrap_or(shutdown),
        source,
    )
    .await?;
    if !shutdown_wait::wait_for_instance_exit(
        state,
        &details.summary.id,
        Duration::from_millis(shutdown.grace_period_ms),
    )
    .await?
    {
        return Err("The complete managed process tree did not exit after the shutdown commands; managed process ownership is retained and no forced stop was attempted.".into());
    }
    Ok(())
}

pub(super) async fn dispatch_instance_shutdown_commands(
    state: &DesktopState,
    storage: &StorageBootstrap,
    details: &InstanceDetails,
    shutdown: &ModuleShutdownSpec,
    source: InstanceShutdownSource,
) -> Result<(), String> {
    let shutdown = rust_shutdown::resolve(details, shutdown).inspect_err(|error| {
        append_desktop_app_log(
            storage,
            "error",
            "instance.shutdown.configuration_invalid",
            error,
            json!({
                "source": source.as_str(),
                "instance_id": details.summary.id,
                "module_id": details.summary.module_id,
            }),
        );
    })?;
    for command in &shutdown.commands {
        let normalized = match normalize_runtime_command_input(&command.command) {
            Ok(command) => command,
            Err(error) => {
                append_desktop_app_log(
                    storage,
                    "error",
                    "instance.shutdown.command_invalid",
                    &error,
                    json!({
                        "source": source.as_str(),
                        "instance_id": details.summary.id,
                        "module_id": details.summary.module_id,
                        "command": command.command,
                    }),
                );
                return Err(format!(
                    "Invalid shutdown command: {error}. Server processes were not forcibly stopped."
                ));
            }
        };

        let mut transport_used = command.transport.clone();
        let mut primary_transport_error = None;
        let mut rcon_delivery_attempted = None;
        let primary_result = if command.transport.eq_ignore_ascii_case("source_rcon") {
            let projected = super::commands_runtime_ark::project_running_command(
                details,
                command.process_key.as_deref(),
            )?;
            super::commands_assistant_ops::dispatch_source_rcon_shutdown_command(
                projected.as_ref().unwrap_or(details),
                &normalized,
                command.port_name.as_deref(),
                command.password_setting_key.as_deref(),
                command.enabled_setting_key.as_deref(),
            )
            .await
            .map(|_| ())
            .map_err(|error| {
                rcon_delivery_attempted = Some(error.command_may_have_been_sent);
                error.message
            })
        } else {
            dispatch_instance_runtime_transport(
                state,
                details,
                &RuntimeTransportRequest {
                    command: &normalized,
                    transport: &command.transport,
                    process_key: command.process_key.as_deref(),
                    port_name: command.port_name.as_deref(),
                    password_setting_key: command.password_setting_key.as_deref(),
                    enabled_setting_key: command.enabled_setting_key.as_deref(),
                },
            )
            .await
        };
        let result = match primary_result {
            Ok(()) => Ok(()),
            Err(error) => {
                // Once RCON delivery was attempted, a lost response is not
                // permission to send another shutdown through another channel.
                let observation_ms = match rcon_delivery_attempted {
                    Some(true) => shutdown.grace_period_ms,
                    Some(false) => 0,
                    None => command.wait_after_ms,
                };
                let exited = shutdown_wait::wait_for_instance_exit(
                    state,
                    &details.summary.id,
                    Duration::from_millis(observation_ms),
                )
                .await?;
                if exited {
                    append_desktop_app_log(
                        storage,
                        "warning",
                        "instance.shutdown.command_exit_confirmed",
                        "Shutdown response failed, but the complete managed process tree exited; the command was not replayed.",
                        json!({
                            "source": source.as_str(),
                            "instance_id": details.summary.id,
                            "module_id": details.summary.module_id,
                            "transport": command.transport,
                            "command": normalized,
                            "error": error,
                        }),
                    );
                    return Ok(());
                }
                if rcon_delivery_attempted == Some(true) {
                    append_desktop_app_log(
                        storage,
                        "error",
                        "instance.shutdown.command_failed",
                        &error,
                        json!({
                            "source": source.as_str(), "instance_id": details.summary.id,
                            "module_id": details.summary.module_id, "transport": command.transport,
                            "command": normalized, "command_may_have_been_sent": true,
                            "owned_tree_exit_confirmed": false,
                        }),
                    );
                    return Err(format!(
                        "Shutdown command `{normalized}` may have been sent, but the complete managed process tree did not exit during its grace period: {error}. Managed process ownership is retained; no fallback or forced stop was attempted."
                    ));
                }
                let fallback_transport = command
                    .fallback_transport
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                if let Some(fallback_transport) = fallback_transport {
                    primary_transport_error = Some(error.clone());
                    transport_used = fallback_transport.to_string();
                    append_desktop_app_log(
                        storage,
                        "warning",
                        "instance.shutdown.command_fallback",
                        "Primary shutdown command transport failed; trying fallback transport.",
                        json!({
                            "source": source.as_str(),
                            "instance_id": details.summary.id.as_str(),
                            "module_id": details.summary.module_id.as_str(),
                            "primary_transport": command.transport.as_str(),
                            "fallback_transport": fallback_transport,
                            "command": normalized.as_str(),
                            "error": error.as_str(),
                        }),
                    );
                    dispatch_instance_runtime_transport(
                        state,
                        details,
                        &RuntimeTransportRequest {
                            command: &normalized,
                            transport: fallback_transport,
                            process_key: command.process_key.as_deref(),
                            port_name: command.port_name.as_deref(),
                            password_setting_key: command.password_setting_key.as_deref(),
                            enabled_setting_key: command.enabled_setting_key.as_deref(),
                        },
                    )
                    .await
                } else {
                    Err(error)
                }
            }
        };

        match result {
            Ok(()) => append_desktop_app_log(
                storage,
                "info",
                "instance.shutdown.command_sent",
                "Shutdown command sent before stopping managed instance.",
                json!({
                    "source": source.as_str(),
                    "instance_id": details.summary.id.as_str(),
                    "module_id": details.summary.module_id.as_str(),
                    "transport": transport_used.as_str(),
                    "primary_transport_error": primary_transport_error.as_deref(),
                    "command": normalized.as_str(),
                }),
            ),
            Err(error) => {
                append_desktop_app_log(
                    storage,
                    "error",
                    "instance.shutdown.command_failed",
                    &error,
                    json!({
                        "source": source.as_str(),
                        "instance_id": details.summary.id.as_str(),
                        "module_id": details.summary.module_id.as_str(),
                        "transport": transport_used.as_str(),
                        "primary_transport_error": primary_transport_error.as_deref(),
                        "command": normalized.as_str(),
                    }),
                );
                // The fallback may race the final pipe closure too. Spend only
                // the existing final grace budget, without another delivery.
                if shutdown_wait::wait_for_instance_exit(
                    state,
                    &details.summary.id,
                    Duration::from_millis(shutdown.grace_period_ms),
                )
                .await?
                {
                    append_desktop_app_log(
                        storage,
                        "warning",
                        "instance.shutdown.command_exit_confirmed",
                        "Shutdown transport reported an error, but the complete managed process tree exited during its grace period.",
                        json!({
                            "source": source.as_str(),
                            "instance_id": details.summary.id,
                            "module_id": details.summary.module_id,
                            "transport": transport_used,
                            "command": normalized,
                            "error": error,
                        }),
                    );
                    return Ok(());
                }
                return Err(format!(
                    "Shutdown command `{normalized}` failed: {error}. Server processes were not forcibly stopped; check the server console and retry."
                ));
            }
        }

        if shutdown_wait::wait_for_instance_exit(
            state,
            &details.summary.id,
            Duration::from_millis(command.wait_after_ms),
        )
        .await?
        {
            break;
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub(super) struct RuntimeTransportRequest<'a> {
    pub command: &'a str,
    pub transport: &'a str,
    pub process_key: Option<&'a str>,
    pub port_name: Option<&'a str>,
    pub password_setting_key: Option<&'a str>,
    pub enabled_setting_key: Option<&'a str>,
}

pub(super) async fn dispatch_instance_runtime_transport(
    state: &DesktopState,
    details: &InstanceDetails,
    request: &RuntimeTransportRequest<'_>,
) -> Result<(), String> {
    let RuntimeTransportRequest {
        command,
        transport,
        process_key,
        port_name,
        password_setting_key,
        enabled_setting_key,
    } = *request;
    if transport.eq_ignore_ascii_case("palworld_rest") {
        return crate::live_players::palworld_rest::execute(details, command)
            .await
            .map(|_| ());
    }
    if transport.eq_ignore_ascii_case("stdin") {
        return dispatch_managed_stdin_command(
            state,
            &details.summary.id,
            process_key,
            command,
            details.active_run.as_ref().map(|run| run.run_id),
        )
        .await
        .map(|_| ());
    }

    if transport.eq_ignore_ascii_case("console_ctrl_c") || transport.eq_ignore_ascii_case("ctrl_c")
    {
        return state
            .runtime_supervisor
            .lock()
            .map_err(|_| String::from("runtime supervisor lock poisoned"))
            .and_then(|mut runtime| {
                runtime
                    .request_console_interrupt(&details.summary.id, process_key)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            });
    }

    if transport.eq_ignore_ascii_case("unreal_console") {
        if details.summary.module_id != "windrose" {
            return Err("Unreal GUI console is not declared for this module".into());
        }
        let runtime = state.runtime_supervisor.clone();
        let instance_id = details.summary.id.clone();
        let process_key = process_key.map(str::to_owned);
        let command = normalize_runtime_command_input(command)?;
        return tokio::task::spawn_blocking(move || {
            runtime
                .lock()
                .map_err(|_| String::from("runtime supervisor lock poisoned"))?
                .request_unreal_console_command(&instance_id, process_key.as_deref(), &command)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| format!("Unreal console worker failed: {error}"))?;
    }

    if transport.eq_ignore_ascii_case("window_close") {
        if command != "WM_CLOSE" {
            return Err("window_close transport only accepts WM_CLOSE".into());
        }
        return state
            .runtime_supervisor
            .lock()
            .map_err(|_| String::from("runtime supervisor lock poisoned"))
            .and_then(|mut runtime| {
                runtime
                    .request_window_close(&details.summary.id, process_key)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            });
    }

    if transport.eq_ignore_ascii_case("source_rcon") {
        let projected = super::commands_runtime_ark::project_running_command(details, process_key)?;
        return dispatch_source_rcon_command(
            projected.as_ref().unwrap_or(details),
            command,
            port_name,
            password_setting_key,
            enabled_setting_key,
        )
        .await
        .map(|_| ());
    }

    if transport.eq_ignore_ascii_case("websocket_rcon") {
        return dispatch_websocket_rcon_command(
            details,
            command,
            port_name,
            password_setting_key,
            enabled_setting_key,
        )
        .await
        .map(|_| ());
    }

    if transport.eq_ignore_ascii_case("battleye_rcon") {
        return dispatch_battleye_rcon_command(
            details,
            command,
            port_name,
            password_setting_key,
            enabled_setting_key,
        )
        .await
        .map(|_| ());
    }

    if transport.eq_ignore_ascii_case("telnet") {
        return dispatch_telnet_command(
            details,
            command,
            port_name,
            password_setting_key,
            enabled_setting_key,
        )
        .await
        .map(|_| ());
    }

    Err(format!(
        "unsupported runtime command transport `{transport}`"
    ))
}

pub(super) async fn stop_active_instance_processes<L>(
    state: &DesktopState,
    storage: &StorageBootstrap,
    storage_context_lease: &L,
    instance_id: &str,
    active_run: &ActiveInstanceRun,
    source: InstanceShutdownSource,
) -> Result<Vec<app_core::StoppedProcess>, String>
where
    L: StorageContextTaskLease,
{
    // A stop attempt can change process identity even when later persistence or cleanup fails.
    state.live_player_registry.invalidate_instance(instance_id);
    let managed = {
        let mut runtime = state
            .runtime_supervisor
            .lock()
            .map_err(|_| String::from("runtime supervisor lock poisoned"))?;
        if !runtime.is_tracked(instance_id) {
            ensure_untracked_running_process_identities(instance_id, active_run)?;
        }
        // This is normal-stop finalization. Check while ownership is still held;
        // the low-level cleanup also serves explicitly forceful recovery paths.
        if runtime
            .instance_process_tree_is_running(instance_id)
            .map_err(|error| error.to_string())?
            != Some(false)
        {
            return Err("The complete managed process tree has not been confirmed exited; managed process ownership is retained and normal-stop finalization was not attempted.".into());
        }
        runtime.take_running_for_stop(instance_id)
    };

    if let Some(managed) = managed {
        let (managed, stop_result, all_tracked_processes_stopped) =
            stop_managed_instance_on_blocking_thread(storage_context_lease, managed).await?;
        let stopped = match stop_result {
            Ok(stopped) => stopped
                .into_iter()
                .map(stopped_managed_process_result)
                .collect::<Vec<_>>(),
            Err(error) if all_tracked_processes_stopped => {
                append_desktop_app_log(
                    storage,
                    "warning",
                    "instance.stop.process_cleanup_reconciled",
                    "Process cleanup reported an error after every tracked process had already exited; treating the stop as completed.",
                    json!({
                        "source": source.as_str(),
                        "instance_id": instance_id,
                        "error": error.as_str(),
                    }),
                );
                managed
                    .processes
                    .iter()
                    .map(|process| app_core::StoppedProcess {
                        run_id: process.run_id,
                        process_key: process.process_key.clone(),
                        display_name: process.display_name.clone(),
                        pid: Some(process.pid),
                        log_path: Some(process.log_path.clone()),
                        exit_code: None,
                    })
                    .collect::<Vec<_>>()
            }
            Err(error) => {
                let restored = state
                    .runtime_supervisor
                    .lock()
                    .map_err(|_| String::from("runtime supervisor lock poisoned"))?
                    .restore_running_after_failed_stop(managed);
                return Err(if restored {
                    error
                } else {
                    format!(
                        "{error}; failed to restore runtime supervisor ownership for `{}`",
                        instance_id
                    )
                });
            }
        };

        // Closing any remaining output owner joins its pipe/ConPTY reader.
        // Do not publish EOF while the producer can still append its crash tail.
        spawn_blocking_storage_context_task(storage_context_lease, move || drop(managed))
            .await
            .map_err(|error| format!("managed output cleanup task failed: {error}"))?;
        let mut crashes = Vec::new();
        for process in &stopped {
            if let Some(path) = process.log_path.as_deref() {
                finish_runtime_log_stream_for_process(state, instance_id, path);
            }
            let crash_reason = super::runtime_exit::windows_crash_exit_reason(process.exit_code);
            mark_instance_process_stopped(
                &storage.paths,
                instance_id,
                process.run_id,
                process.exit_code,
                crash_reason.is_some(),
            )
            .await
            .map_err(|error| error.to_string())?;
            if let (Some(code), Some(reason)) = (process.exit_code, crash_reason) {
                crashes.push(format!(
                    "{}: {reason} (0x{:08X}, {code})",
                    process.process_key, code as u32
                ));
            }
        }
        if !crashes.is_empty() {
            // The entire tree is already gone: release ownership and retain the
            // actual crash result, rather than reporting a successful stop or
            // leaving the desktop's cached summary looking alive after Err.
            let error = format!("native_shutdown_crash: {}", crashes.join("; "));
            let instances = list_instances(&storage.paths).await.map_err(|refresh| {
                format!("{error}; failed to refresh finalized state: {refresh}")
            })?;
            update_desktop_state_instances(state, instances).map_err(|refresh| {
                format!("{error}; failed to refresh finalized state: {refresh}")
            })?;
            return Err(error);
        }
        return Ok(stopped);
    }

    Err(format!(
        "Managed process ownership for `{instance_id}` is unavailable; the active runtime record was preserved and normal-stop finalization was not attempted."
    ))
}

fn ensure_untracked_running_process_identities(
    instance_id: &str,
    active_run: &ActiveInstanceRun,
) -> Result<(), String> {
    for process in active_run
        .processes
        .iter()
        .filter(|process| process.status == "running")
    {
        let Some(pid) = process.pid else {
            return Err(format!(
                "cannot safely stop untracked process `{}` for instance `{instance_id}`: the running process has no recorded PID; the active runtime record was preserved",
                process.display_name
            ));
        };
        if process.process_identity.is_none() {
            return Err(format!(
                "cannot safely stop untracked process `{}` for instance `{instance_id}`: PID {pid} has no recorded process identity; the active runtime record was preserved",
                process.display_name
            ));
        }
    }
    Ok(())
}

fn stopped_managed_process_result(process: StoppedManagedProcess) -> app_core::StoppedProcess {
    app_core::StoppedProcess {
        run_id: process.run_id,
        process_key: process.process_key,
        display_name: process.display_name,
        pid: Some(process.pid),
        log_path: Some(process.log_path),
        exit_code: process.exit_code,
    }
}

async fn stop_managed_instance_on_blocking_thread<L>(
    storage_context_lease: &L,
    mut managed: ManagedInstance,
) -> Result<
    (
        ManagedInstance,
        Result<Vec<StoppedManagedProcess>, String>,
        bool,
    ),
    String,
>
where
    L: StorageContextTaskLease,
{
    spawn_blocking_storage_context_task(storage_context_lease, move || {
        let stop_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            stop_managed_instance(&mut managed)
        }))
        .map_err(|_| String::from("managed process cleanup panicked"))
        .and_then(|result| result.map_err(|error| error.to_string()));
        let all_tracked_processes_stopped = stop_result.is_err()
            && managed.processes.iter().all(|process| {
                matches!(
                    process_matches_identity(process.pid, &process.process_identity),
                    Ok(false)
                )
            });
        (managed, stop_result, all_tracked_processes_stopped)
    })
    .await
    .map_err(|error| format!("managed process cleanup task failed: {error}"))
}

pub(super) fn normalize_runtime_command_input(command: &str) -> Result<String, String> {
    let normalized = command.replace("\r\n", "\n").replace('\r', "\n");
    let lines = normalized
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();

    if lines.is_empty() {
        return Err(String::from("Enter one runtime command line first."));
    }

    if lines.len() > 1 {
        return Err(String::from(
            "Send one runtime command at a time. Multiline command batches are not supported.",
        ));
    }

    let line = lines[0];
    if line.len() > 512 {
        return Err(String::from(
            "Runtime commands are limited to 512 characters per dispatch.",
        ));
    }

    Ok(line.to_string())
}

#[cfg(test)]
#[path = "commands_app_exit_tests.rs"]
mod app_exit_tests;

#[cfg(all(test, windows))]
#[path = "commands_normal_stop_tests.rs"]
mod normal_stop_tests;
#[cfg(test)]
#[path = "commands_shutdown_commands_tests.rs"]
mod shutdown_commands_tests;
