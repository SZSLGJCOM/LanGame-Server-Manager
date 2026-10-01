use super::commands_install_progress::{
    InstallationJobLease, apply_install_progress, complete_install_progress, fail_install_progress,
    finish_install_error, queued_install_progress,
};
use super::*;
use app_storage::InstanceProgramMode;

use super::commands_module_mutation_locks;

#[path = "commands_library_baseline.rs"]
mod baseline;
pub(super) use baseline::LibraryBaselineRecorder;

#[path = "commands_program_update.rs"]
mod update;
pub(super) use update::{ProgramUpdateRequest, install_program_with_baseline};

pub(super) fn instance_root(instance: &InstanceDetails) -> Result<&Path, String> {
    Path::new(&instance.config_file_path)
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| String::from("instance config path has no instance root"))
}

pub(super) fn program_mode(instance: &InstanceDetails) -> Result<InstanceProgramMode, String> {
    app_storage::instance_program_mode(instance_root(instance)?).map_err(|error| error.to_string())
}

/// Instance mutation locks precede the install lock everywhere, including
/// startup. Retain both until installation and its database result are settled.
pub(super) struct LibraryProgramMutation {
    pub install: app_steamcmd::GameInstallLifecycleGuard,
    _instances: Vec<tokio::sync::OwnedMutexGuard<()>>,
}

enum InstanceProgramMutation {
    Shared(LibraryProgramMutation),
    Independent {
        install: app_steamcmd::GameInstallLifecycleGuard,
        _instance: tokio::sync::OwnedMutexGuard<()>,
    },
}

impl InstanceProgramMutation {
    fn install(&self) -> &app_steamcmd::GameInstallLifecycleGuard {
        match self {
            Self::Shared(guard) => &guard.install,
            Self::Independent { install, .. } => install,
        }
    }
}

pub(super) async fn acquire_library_program_mutation(
    state: &DesktopState,
    storage: &StorageBootstrap,
    module_id: &str,
    install_root: &Path,
) -> Result<LibraryProgramMutation, String> {
    let instances = commands_module_mutation_locks::acquire_module_instance_mutations(
        state,
        &storage.paths,
        module_id,
    )
    .await?;
    let install =
        app_steamcmd::acquire_game_install_lifecycle(module_id, &[install_root.to_owned()])
            .await
            .map_err(|error| steamcmd_error_message(&error))?;
    app_storage::ensure_library_program_target_available(&storage.paths, install_root)
        .await
        .map_err(|error| error.to_string())?;
    app_storage::recover_interrupted_program_adoptions(&storage.paths, module_id, install_root)
        .await
        .map_err(|error| error.to_string())?;
    if let Some(record) = app_storage::read_program_install_owner(&storage.paths, install_root)
        .await
        .map_err(|error| error.to_string())?
        && (record.scope != app_storage::ProgramInstallScope::Library
            || record.module_id != module_id)
    {
        return Err(String::from(
            "此程序目录已属于其他游戏或独立实例；请从对应实例维护入口更新。",
        ));
    }
    ensure_shared_program_unused(state, storage, module_id, install_root, None).await?;
    app_storage::ensure_program_archive_dependencies(&storage.paths, install_root)
        .await
        .map_err(|error| error.to_string())?;
    Ok(LibraryProgramMutation {
        install,
        _instances: instances,
    })
}

pub(super) async fn shared_program_references(
    paths: &app_storage::StoragePaths,
    module_id: &str,
    install_root: &Path,
) -> Result<Vec<InstanceDetails>, String> {
    let mut references = Vec::new();
    let expected = if install_root.exists() {
        fs::canonicalize(install_root).map_err(|error| error.to_string())?
    } else {
        install_root.to_owned()
    };
    for summary in list_instances(paths)
        .await
        .map_err(|error| error.to_string())?
    {
        if summary.module_id != module_id {
            continue;
        }
        let instance = read_instance_details(paths, &summary.id)
            .await
            .map_err(|error| error.to_string())?;
        if !app_storage::instance_uses_library_program(instance_root(&instance)?)
            .map_err(|error| error.to_string())?
        {
            continue;
        }
        let root = app_storage::resolve_instance_runtime_root(instance_root(&instance)?)
            .map_err(|error| error.to_string())?;
        let actual = fs::canonicalize(&root).map_err(|error| error.to_string())?;
        if actual == expected {
            references.push(instance);
        }
    }
    Ok(references)
}

/// Call while holding the actual program root's install lifecycle lock. Starts
/// hold that same lock until process/run publication, so another pending start
/// cannot use the program yet. Maintenance still respects every start reservation.
pub(super) async fn ensure_shared_program_unused(
    state: &DesktopState,
    storage: &StorageBootstrap,
    module_id: &str,
    install_root: &Path,
    starting_instance: Option<&str>,
) -> Result<(), String> {
    let pending = state.pending_runtime_start_instance_ids()?;
    for instance in shared_program_references(&storage.paths, module_id, install_root).await? {
        ensure_instance_program_update_allowed(&instance)?;
        let id = instance.summary.id.as_str();
        if starting_instance == Some(id) {
            continue;
        }
        let active = read_active_instance_run(&storage.paths, id)
            .await
            .map_err(|error| error.to_string())?
            .is_some();
        let tracked = state
            .runtime_supervisor
            .lock()
            .map_err(|_| String::from("runtime supervisor lock poisoned"))?
            .is_tracked(id);
        if active
            || tracked
            || (starting_instance.is_none() && pending.iter().any(|pending| pending == id))
        {
            return Err(format!(
                "请先停止或取消正在启动的实例“{}”，再更新它使用的服务器程序。",
                instance.summary.name
            ));
        }
    }
    Ok(())
}

fn ensure_instance_program_update_allowed(instance: &InstanceDetails) -> Result<(), String> {
    if app_core::InstanceProgramUpdatePolicy::from_settings_json(&instance.settings_json)?
        == app_core::InstanceProgramUpdatePolicy::Pinned
    {
        return Err(format!(
            "实例“{}”的当前版本已固定，请先切换到启动前更新，再更新它使用的服务器程序。",
            instance.summary.name
        ));
    }
    Ok(())
}

#[path = "commands_program_creation.rs"]
mod creation;
pub(super) use creation::{CreationProgramPreparation, prepare_creation_program};

#[path = "commands_program_repair.rs"]
mod repair;
pub(super) use repair::{CreationProgramRequest, create_with_program_repair, report_progress};

/// The caller owns the stopped instance lock. Keep it with the detach worker
/// so a dropped IPC waiter cannot expose a half-copied runtime to startup.
pub(super) struct ModProgramMutation {
    _instance: tokio::sync::OwnedMutexGuard<()>,
    pub(super) install: app_steamcmd::GameInstallLifecycleGuard,
}

pub(super) async fn prepare_instance_mod_program(
    storage: &StorageBootstrap,
    operation: &StorageContextOperationGuard,
    instance_id: &str,
    instance_lock: tokio::sync::OwnedMutexGuard<()>,
) -> Result<ModProgramMutation, String> {
    let instance = read_instance_details(&storage.paths, instance_id)
        .await
        .map_err(|error| error.to_string())?;
    let root = app_storage::resolve_instance_runtime_root(instance_root(&instance)?)
        .map_err(|error| error.to_string())?;
    // Detach reads the current program and publishes into this instance's
    // private runtime. Reserve both before changing its program binding.
    let private_root = instance_root(&instance)?.join("runtime");
    let install_guard = app_steamcmd::acquire_game_install_lifecycle(
        &instance.summary.module_id,
        &[root.clone(), private_root],
    )
    .await
    .map_err(|error| steamcmd_error_message(&error))?;
    let current_root = app_storage::resolve_instance_runtime_root(instance_root(&instance)?)
        .map_err(|error| error.to_string())?;
    install_guard
        .ensure_scope(&instance.summary.module_id, &current_root)
        .map_err(|error| steamcmd_error_message(&error))?;
    app_storage::ensure_program_archive_dependencies(&storage.paths, &current_root)
        .await
        .map_err(|error| error.to_string())?;
    if program_mode(&instance)? == InstanceProgramMode::Independent {
        return Ok(ModProgramMutation {
            _instance: instance_lock,
            install: install_guard,
        });
    }
    let descriptors =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    let descriptor = find_descriptor(&descriptors, &instance.summary.module_id)?.clone();
    let paths = storage.paths.clone();
    let id = instance_id.to_owned();
    let cancellation = operation.cancellation_token();
    spawn_storage_context_task(operation, async move {
        app_storage::detach_instance_program(&paths, &descriptor, &id, Some(cancellation))
            .await
            .map_err(|error| error.to_string())?;
        Ok(ModProgramMutation {
            _instance: instance_lock,
            install: install_guard,
        })
    })
    .await
    .map_err(|error| format!("instance program detach task failed: {error}"))?
}

pub(super) fn program_directory_is_empty(root: &Path) -> Result<bool, String> {
    match fs::read_dir(root) {
        Ok(mut entries) => entries
            .next()
            .transpose()
            .map(|entry| entry.is_none())
            .map_err(|error| error.to_string()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error.to_string()),
    }
}

#[tauri::command]
pub async fn update_instance_program<R: tauri::Runtime>(
    app_handle: tauri::AppHandle<R>,
    instance_id: String,
    validate: bool,
) -> Result<ModuleInstallResult, String> {
    let operation = app_handle
        .state::<DesktopState>()
        .begin_storage_context_operation("instance program update")?;
    let worker_operation = operation.clone();
    spawn_storage_context_task(&operation, async move {
        update_instance_program_inner(
            app_handle.state::<DesktopState>(),
            instance_id,
            validate,
            worker_operation,
        )
        .await
    })
    .await
    .map_err(|error| format!("instance program update task failed: {error}"))?
}

async fn update_instance_program_inner(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    validate: bool,
    _operation: StorageContextOperationGuard,
) -> Result<ModuleInstallResult, String> {
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    let before = read_instance_details(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?;
    let mode = program_mode(&before)?;
    let uses_library = app_storage::instance_uses_library_program(instance_root(&before)?)
        .map_err(|error| error.to_string())?;
    let expected_root = app_storage::resolve_instance_runtime_root(instance_root(&before)?)
        .map_err(|error| error.to_string())?;
    let guard = if uses_library {
        InstanceProgramMutation::Shared(
            acquire_library_program_mutation(
                &state,
                &storage,
                &before.summary.module_id,
                &expected_root,
            )
            .await?,
        )
    } else {
        let instance = state.acquire_instance_mutation(&instance_id).await;
        let install = app_steamcmd::acquire_game_install_lifecycle(
            &before.summary.module_id,
            std::slice::from_ref(&expected_root),
        )
        .await
        .map_err(|error| steamcmd_error_message(&error))?;
        InstanceProgramMutation::Independent {
            install,
            _instance: instance,
        }
    };
    let instance = read_instance_details(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?;
    ensure_instance_program_update_allowed(&instance)?;
    let root = app_storage::resolve_instance_runtime_root(instance_root(&instance)?)
        .map_err(|error| error.to_string())?;
    if program_mode(&instance)? != mode
        || instance.summary.module_id != before.summary.module_id
        || fs::canonicalize(&root).map_err(|error| error.to_string())?
            != fs::canonicalize(&expected_root).map_err(|error| error.to_string())?
    {
        return Err(String::from("实例程序位置已改变，请刷新后重试。"));
    }
    let active = read_active_instance_run(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?
        .is_some();
    let tracked = state
        .runtime_supervisor
        .lock()
        .map_err(|_| String::from("runtime supervisor lock poisoned"))?
        .is_tracked(&instance_id);
    if active
        || tracked
        || state
            .pending_runtime_start_instance_ids()?
            .contains(&instance_id)
    {
        return Err(String::from(
            "请先停止实例并取消待启动操作，再更新服务器程序。",
        ));
    }
    let binding = app_storage::read_instance_program_install(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| String::from("instance program ownership is missing"))?;
    let ownership_matches = if uses_library {
        binding.install.scope == app_storage::ProgramInstallScope::Library
            && binding.install.owner_instance_id.is_none()
    } else {
        binding.install.scope == app_storage::ProgramInstallScope::Instance
            && binding.install.owner_instance_id.as_deref() == Some(instance_id.as_str())
    };
    if !ownership_matches
        || binding.install.module_id != instance.summary.module_id
        || fs::canonicalize(&binding.install.install_root).map_err(|error| error.to_string())?
            != fs::canonicalize(&root).map_err(|error| error.to_string())?
    {
        return Err(String::from(
            "instance program ownership does not match its runtime",
        ));
    }
    app_storage::ensure_program_archive_dependencies(&storage.paths, &root)
        .await
        .map_err(|error| error.to_string())?;
    let descriptors =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    let descriptor = find_descriptor(&descriptors, &instance.summary.module_id)?;
    if descriptor
        .install
        .as_ref()
        .is_some_and(|install| install.download_url_windows.is_some())
    {
        return Err(String::from(
            "此游戏使用整包替换安装；为保留实例中的配置和Mod，不能直接覆盖独立程序目录。",
        ));
    }
    let root_text = root.to_string_lossy();
    let module =
        map_module_details_with_install_state(&storage.settings, descriptor, Some(&root_text));
    let job_id = new_background_job_id("instance-program-update", &instance_id);
    let lease = InstallationJobLease::begin(&state, job_id.clone())?;
    insert_background_job(
        &state,
        &storage,
        BackgroundJob {
            id: job_id.clone(),
            kind: JobKind::ValidateGame,
            label: format!("更新 {} 的程序", instance.summary.name),
            status: JobStatus::Pending,
            progress_percent: 1.0,
            install_progress: Some(queued_install_progress()),
            cancellable: true,
            cancel_requested: false,
            target_id: Some(instance_id.clone()),
            detail: Some(String::from("准备更新服务器程序…")),
            output_excerpt: None,
        },
    )?;
    let result = install_program_with_baseline(
        ProgramUpdateRequest {
            storage: &storage,
            descriptor,
            module: &module,
            root: &root,
            operation: &_operation,
            guard,
            validate,
            cancellation: lease.cancellation(),
        },
        |update| {
            let _ =
                update_background_job(&state, &job_id, |job| apply_install_progress(job, &update));
        },
    )
    .await;
    match result {
        Ok((result, _guard)) => {
            let saved = async {
                let record = GameInstallSyncRecord {
                    module_id: result.module_id.clone(),
                    install_root: result.install_root.clone(),
                    install_state: result.install_state.clone(),
                    current_version: result.current_version.clone(),
                    mark_verified: true,
                };
                if uses_library {
                    sync_game_installs(&storage.paths, &[record])
                        .await
                        .map_err(|error| error.to_string())
                } else {
                    app_storage::sync_instance_game_install(&storage.paths, &instance_id, &record)
                        .await
                        .map_err(|error| error.to_string())
                }
            }
            .await;
            if let Err(message) = saved {
                update_background_job(&state, &job_id, |job| {
                    fail_install_progress(job, 99.0);
                    job.detail = Some(message.clone());
                    job.output_excerpt = Some(result.output_excerpt.clone());
                })?;
                return Err(message);
            }
            update_background_job(&state, &job_id, |job| {
                complete_install_progress(job);
                job.detail = Some(String::from("服务器程序更新完成"));
            })?;
            Ok(result)
        }
        Err(error) => {
            let message = steamcmd_error_message(&error);
            update_background_job(&state, &job_id, |job| {
                finish_install_error(job, &error);
                job.detail = Some(message.clone());
                job.output_excerpt = steamcmd_error_excerpt(&error);
            })?;
            Err(message)
        }
    }
}
