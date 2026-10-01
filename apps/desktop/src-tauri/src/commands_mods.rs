use super::commands_install_progress::{
    InstallationJobLease, apply_install_progress, begin_install_deployment,
    complete_install_progress, fail_install_progress, finish_install_error,
    queued_install_progress,
};
#[cfg(test)]
pub(super) use super::commands_mod_staging::select_thunderstore_payload_prefix;
use super::commands_mod_staging::{
    ONLINE_MOD_MANIFEST, stage_downloaded_workshop_items_into_manual_target,
    stage_manual_mod_sources, stage_online_mod_sources, stage_thunderstore_archive_to_directory,
    verify_online_mod_dependencies,
};
use super::*;
use crate::steam_workshop::validate_workshop_download_items;
#[path = "commands_mod_download.rs"]
mod mod_download;
use mod_download::{
    ModDownloadIntegrity, ModDownloadRequest, download_mod_site_file_with_limit,
    fetch_modrinth_project_names,
};
#[path = "commands_mod_inventory.rs"]
mod mod_inventory;
#[path = "commands_mod_online.rs"]
mod mod_online;
#[cfg(test)]
pub(super) use mod_inventory::{manual_mod_path_stats, read_manual_mod_inventory_items};
#[path = "commands_dst_workshop.rs"]
mod dst_workshop;
#[path = "commands_install_data.rs"]
mod install_data;
#[path = "commands_install_removal.rs"]
mod install_removal;
#[path = "commands_library_cleanup.rs"]
mod library_cleanup;
use super::commands_module_mutation_locks::acquire_module_instance_mutations;
pub(super) use library_cleanup::after_instance_deletion;
#[path = "commands_workshop_download.rs"]
mod workshop_download;
pub(super) async fn prepare_dst_import_workshop_items(
    storage: &StorageBootstrap,
    operation: &StorageContextOperationGuard,
    instance: &InstanceDetails,
    ids: &[String],
    preference: app_network::SourcePreference,
) -> Result<(), String> {
    workshop_download::prepare_locked_dst_import_items(
        storage, operation, instance, ids, preference,
    )
    .await
}
use install_data::{contains_preserved_data, declared_install_root_data_path};

#[derive(Default)]
struct DownloadedManualModSourcesCleanup {
    sources: Vec<DownloadedManualModSource>,
}

impl DownloadedManualModSourcesCleanup {
    fn from_sources(sources: Vec<DownloadedManualModSource>) -> Self {
        Self { sources }
    }

    fn push(&mut self, source: DownloadedManualModSource) {
        self.sources.push(source);
    }

    fn into_sources(mut self) -> Vec<DownloadedManualModSource> {
        std::mem::take(&mut self.sources)
    }
}

impl std::ops::Deref for DownloadedManualModSourcesCleanup {
    type Target = [DownloadedManualModSource];

    fn deref(&self) -> &Self::Target {
        &self.sources
    }
}

impl Drop for DownloadedManualModSourcesCleanup {
    fn drop(&mut self) {
        let paths = self
            .sources
            .drain(..)
            .map(|source| source.path)
            .collect::<Vec<_>>();
        if paths.is_empty() {
            return;
        }
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn_blocking(move || {
                for path in paths {
                    cleanup_downloaded_mod_source(&path);
                }
            });
        } else {
            for path in paths {
                cleanup_downloaded_mod_source(&path);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProtectedInstallDataPath {
    source: String,
    path: Option<PathBuf>,
}

#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum ModuleInstallOperation {
    Install,
    Update,
    Validate,
    Uninstall,
}

impl ModuleInstallOperation {
    fn diagnostic_label(self) -> &'static str {
        match self {
            Self::Install => "安装",
            Self::Update => "更新",
            Self::Validate => "校验或更新",
            Self::Uninstall => "卸载",
        }
    }
}

fn comparable_path_components(path: &Path) -> Option<Vec<String>> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    let mut normalized = Vec::new();
    for component in absolute.components() {
        match component {
            std::path::Component::Prefix(prefix) => normalized.push(
                prefix
                    .as_os_str()
                    .to_string_lossy()
                    .replace('\\', "/")
                    .to_ascii_lowercase(),
            ),
            std::path::Component::RootDir => normalized.push(String::from("/")),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if normalized.last().is_some_and(|component| component != "/") {
                    normalized.pop();
                }
            }
            std::path::Component::Normal(component) => normalized.push(
                component
                    .to_string_lossy()
                    .replace('\\', "/")
                    .to_ascii_lowercase(),
            ),
        }
    }
    Some(normalized)
}

fn canonicalize_with_missing_suffix(path: &Path) -> Option<PathBuf> {
    let mut existing_ancestor = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    let mut missing_suffix = Vec::new();
    while !existing_ancestor.exists() {
        missing_suffix.push(existing_ancestor.file_name()?.to_os_string());
        existing_ancestor = existing_ancestor.parent()?.to_path_buf();
    }
    let mut resolved = fs::canonicalize(existing_ancestor).ok()?;
    for component in missing_suffix.into_iter().rev() {
        resolved.push(component);
    }
    Some(resolved)
}

fn path_is_same_or_within(candidate: &Path, root: &Path) -> bool {
    fn components_are_same_or_within(candidate: &Path, root: &Path) -> bool {
        let Some(candidate) = comparable_path_components(candidate) else {
            return false;
        };
        let Some(root) = comparable_path_components(root) else {
            return false;
        };
        candidate.len() >= root.len() && candidate[..root.len()] == root
    }

    if components_are_same_or_within(candidate, root) {
        return true;
    }
    match (
        canonicalize_with_missing_suffix(candidate),
        canonicalize_with_missing_suffix(root),
    ) {
        (Some(candidate), Some(root)) => components_are_same_or_within(&candidate, &root),
        _ => false,
    }
}

async fn collect_protected_install_data_paths(
    paths: &app_storage::StoragePaths,
    descriptor: &ModuleDescriptor,
    install_root: &Path,
) -> Result<Vec<ProtectedInstallDataPath>, String> {
    let mut candidates = Vec::new();
    if path_is_same_or_within(&paths.instances_root, install_root) {
        candidates.push(ProtectedInstallDataPath {
            source: String::from("安装目录内的实例数据"),
            path: Some(paths.instances_root.clone()),
        });
    }

    let instances = list_instances(paths)
        .await
        .map_err(|error| error.to_string())?;
    for instance in instances
        .into_iter()
        .filter(|instance| instance.module_id == descriptor.summary.id)
    {
        let details = read_instance_details(paths, &instance.id)
            .await
            .map_err(|error| {
                format!(
                    "检查实例 {} ({}) 的数据路径失败，已拒绝继续：{}",
                    instance.name, instance.id, error
                )
            })?;
        let config_path = PathBuf::from(&details.config_file_path);
        if path_is_same_or_within(&config_path, install_root) {
            candidates.push(ProtectedInstallDataPath {
                source: format!("实例 {} ({}) 的配置", instance.name, instance.id),
                path: Some(config_path),
            });
        }
        let saves_path = PathBuf::from(&details.saves_path);
        if path_is_same_or_within(&saves_path, install_root) {
            candidates.push(ProtectedInstallDataPath {
                source: format!("实例 {} ({}) 的存档", instance.name, instance.id),
                path: Some(saves_path),
            });
        }

        let backups = list_instance_backups_snapshot(paths, &instance.id)
            .await
            .map_err(|error| {
                format!(
                    "检查实例 {} ({}) 的备份失败，已拒绝继续：{}",
                    instance.name, instance.id, error
                )
            })?;
        for backup in backups {
            let backup_path = PathBuf::from(&backup.backup_path);
            if path_is_same_or_within(&backup_path, install_root) {
                candidates.push(ProtectedInstallDataPath {
                    source: format!(
                        "实例 {} ({}) 的备份 {}",
                        instance.name, instance.id, backup.backup_id
                    ),
                    path: Some(backup_path),
                });
            }
        }
    }

    let descriptor = descriptor.clone();
    let install_root = install_root.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut protected =
            install_data::declared_install_root_retained_paths(&descriptor, &install_root)?;
        if let Some(declared_path) = declared_install_root_data_path(&descriptor, &install_root)? {
            protected.push(declared_path);
        }
        for candidate in candidates {
            if let Some(path) = candidate.path.as_deref()
                && contains_preserved_data(path)?
            {
                protected.push(candidate);
            }
        }
        protected.sort_by(|left, right| {
            left.source
                .cmp(&right.source)
                .then_with(|| left.path.cmp(&right.path))
        });
        protected.dedup();
        Ok(protected)
    })
    .await
    .map_err(|error| format!("检查安装目录内的存档和备份失败，已拒绝继续：{error}"))?
}

fn reject_protected_install_data(
    operation: ModuleInstallOperation,
    module_name: &str,
    install_root: &Path,
    protected: &[ProtectedInstallDataPath],
) -> Result<(), String> {
    if protected.is_empty() {
        return Ok(());
    }
    let details = protected
        .iter()
        .map(|item| match item.path.as_deref() {
            Some(path) => format!("{}：{}", item.source, path.display()),
            None => item.source.clone(),
        })
        .collect::<Vec<_>>()
        .join("；");
    Err(serde_json::json!({
        "code": "install_data_protected",
        "operation": operation,
        "module_name": module_name,
        "install_root": install_root.to_string_lossy(),
        "protected": protected.iter().map(|item| serde_json::json!({
            "source": item.source,
            "path": item.path.as_ref().map(|path| path.to_string_lossy()),
        })).collect::<Vec<_>>(),
        "message": format!(
            "为保护存档和备份，已拒绝{}“{module_name}”。安装目录 {} 内存在受保护数据：{details}。请先将这些数据迁移到实例目录或安装目录之外。",
            operation.diagnostic_label(),
            install_root.display()
        ),
    })
    .to_string())
}

fn ensure_module_unused_for_uninstall(
    module: &ModuleSummary,
    running_instances: &[String],
) -> Result<(), String> {
    if running_instances.is_empty() {
        return Ok(());
    }
    Err(serde_json::json!({
        "code": "module_in_use",
        "module_id": module.id,
        "module_name": module.name,
        "instances": running_instances,
        "message": format!(
            "无法卸载“{}”：以下实例仍在运行：{}",
            module.name,
            running_instances.join(", ")
        ),
    })
    .to_string())
}

fn direct_download_replaces_install_root(descriptor: &ModuleDescriptor) -> bool {
    descriptor
        .install
        .as_ref()
        .and_then(|install| install.download_url_windows.as_deref())
        .is_some_and(|url| !url.trim().is_empty())
}

fn install_root_has_entries(install_root: &Path) -> Result<bool, String> {
    if !install_root.exists() {
        return Ok(false);
    }
    if !install_root.is_dir() {
        return Err(serde_json::json!({
            "code": "install_path_not_directory",
            "install_root": install_root.to_string_lossy(),
            "message": format!(
                "安装路径 {} 不是目录，已拒绝整包替换",
                install_root.display()
            ),
        })
        .to_string());
    }
    fs::read_dir(install_root)
        .map_err(|error| {
            format!(
                "读取安装目录 {} 失败，已拒绝整包替换：{}",
                install_root.display(),
                error
            )
        })?
        .next()
        .transpose()
        .map(|entry| entry.is_some())
        .map_err(|error| {
            format!(
                "检查安装目录 {} 失败，已拒绝整包替换：{}",
                install_root.display(),
                error
            )
        })
}

fn reject_unsafe_direct_download_replacement(
    operation: ModuleInstallOperation,
    settings: &app_core::AppSettings,
    descriptor: &ModuleDescriptor,
) -> Result<(), String> {
    if !direct_download_replaces_install_root(descriptor) {
        return Ok(());
    }
    let install = descriptor
        .install
        .as_ref()
        .ok_or_else(|| format!("模块 `{}` 缺少安装声明", descriptor.summary.id))?;
    let install_root = PathBuf::from(&settings.games_root).join(&install.shared_game_dir);
    if !install_root_has_entries(&install_root)? {
        return Ok(());
    }
    if app_steamcmd::has_retained_install_data(&install_root)
        && probe_module_install_state_with_override(
            settings,
            &descriptor.summary.id,
            descriptor.summary.steam_app_id,
            descriptor.install.as_ref(),
            descriptor.process.as_ref(),
            None,
        )
        .install_state
            == InstallState::NotInstalled
    {
        // The ZIP publisher merges these retained files into the verified new
        // payload and keeps the original tree available for rollback.
        return Ok(());
    }
    Err(serde_json::json!({
        "code": "install_replacement_not_empty",
        "operation": operation,
        "module_id": descriptor.summary.id,
        "module_name": descriptor.summary.name,
        "install_root": install_root.to_string_lossy(),
        "message": format!(
            "为保护用户数据，已拒绝{}“{}”。该模块使用整包替换安装，现有目录 {} 非空；继续操作可能覆盖存档、配置或其他用户文件。请先迁移或备份该目录后再处理。",
            operation.diagnostic_label(),
            descriptor.summary.name,
            install_root.display()
        ),
    })
    .to_string())
}

fn merge_module_install_state(app_state: &mut AppState, refreshed: &ModuleSummary) {
    if let Some(module) = app_state
        .modules
        .iter_mut()
        .find(|module| module.id == refreshed.id)
    {
        // Program ownership counts have their own refresh lifecycle. Finishing
        // this installation must also preserve other modules' active states.
        module.install_state = refreshed.install_state.clone();
    } else {
        app_state.modules.push(refreshed.clone());
    }
}

pub(super) async fn install_module_game_inner(
    state: tauri::State<'_, DesktopState>,
    module_id: String,
) -> Result<ModuleInstallResult, String> {
    let _storage_context_operation =
        state.begin_storage_context_operation("module installation")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    let descriptors =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    sync_modules(&storage.paths, &descriptors)
        .await
        .map_err(|error| error.to_string())?;
    let descriptor = find_descriptor(&descriptors, &module_id)?;
    let install_root_override = load_module_install_root_override(&storage.paths, &module_id).await;
    let module = map_module_details_with_install_state(
        &storage.settings,
        descriptor,
        install_root_override.as_deref(),
    );
    let operation = if matches!(module.summary.install_state, InstallState::Installed) {
        ModuleInstallOperation::Update
    } else {
        ModuleInstallOperation::Install
    };
    reject_unsafe_direct_download_replacement(operation, &storage.settings, descriptor)?;
    let program_root = PathBuf::from(
        probe_module_install_state_with_override(
            &storage.settings,
            &module_id,
            descriptor.summary.steam_app_id,
            descriptor.install.as_ref(),
            descriptor.process.as_ref(),
            install_root_override.as_deref(),
        )
        .install_root,
    );
    let program_guard = super::commands_program_storage::acquire_library_program_mutation(
        &state,
        &storage,
        &module_id,
        &program_root,
    )
    .await?;
    let job_kind = JobKind::DownloadGame;
    let label = if matches!(module.summary.install_state, InstallState::Installed) {
        format!("Update {}", module.summary.name)
    } else {
        format!("Install {}", module.summary.name)
    };
    let transient_install_state = if matches!(module.summary.install_state, InstallState::Installed)
    {
        InstallState::Updating
    } else {
        InstallState::Installing
    };
    let job_id = new_background_job_id("module-install", &module.summary.id);
    let operation = InstallationJobLease::begin(&state, job_id.clone())?;

    insert_background_job(
        &state,
        &storage,
        BackgroundJob {
            id: job_id.clone(),
            kind: job_kind,
            label: label.clone(),
            status: JobStatus::Pending,
            progress_percent: 1.0,
            install_progress: Some(queued_install_progress()),
            cancellable: true,
            cancel_requested: false,
            target_id: Some(module.summary.id.clone()),
            detail: Some(format!("Preparing {}...", module.summary.name)),
            output_excerpt: None,
        },
    )?;
    set_module_install_state(&state, &module.summary.id, transient_install_state)?;

    let install_result = super::commands_program_storage::install_program_with_baseline(
        super::commands_program_storage::ProgramUpdateRequest {
            storage: &storage,
            descriptor,
            module: &module,
            root: &program_root,
            operation: &_storage_context_operation,
            guard: program_guard,
            validate: false,
            cancellation: operation.cancellation(),
        },
        |update: InstallProgressUpdate| {
            let _ = update_background_job(&state, &job_id, |job| {
                apply_install_progress(job, &update);
            });
        },
    )
    .await;

    let refreshed_module = module_summary_with_install_state(
        &storage.settings,
        descriptor,
        install_root_override.as_deref(),
    );

    match install_result {
        Ok((result, _program_guard)) => {
            persist_install_result(
                &storage,
                &result,
                matches!(result.install_state, InstallState::Installed),
            )
            .await?;
            mutate_app_state(&state, |app_state| {
                merge_module_install_state(app_state, &refreshed_module);
                if let Some(job) = app_state.jobs.iter_mut().find(|job| job.id == job_id) {
                    complete_install_progress(job);
                    job.detail = Some(format!("{} complete", label));
                    job.output_excerpt = Some(result.output_excerpt.clone());
                }
            })?;
            Ok(result)
        }
        Err(error) => {
            let error_message = steamcmd_error_message(&error);
            let output_excerpt = steamcmd_error_excerpt(&error).or_else(|| Some(error.to_string()));
            let _ =
                persist_descriptor_install_states(&storage, std::slice::from_ref(descriptor)).await;
            mutate_app_state(&state, |app_state| {
                merge_module_install_state(app_state, &refreshed_module);
                if let Some(job) = app_state.jobs.iter_mut().find(|job| job.id == job_id) {
                    finish_install_error(job, &error);
                    job.detail = Some(error_message.clone());
                    job.output_excerpt = output_excerpt.clone();
                }
            })?;
            Err(error_message)
        }
    }
}

pub(super) async fn uninstall_module_game_inner<R: tauri::Runtime>(
    app_handle: tauri::AppHandle<R>,
    module_id: String,
) -> Result<app_core::ModuleUninstallResult, String> {
    let operation = app_handle
        .state::<DesktopState>()
        .begin_storage_context_operation("module uninstallation")?;
    // The command caller may disappear while Windows releases executable
    // handles. Own the locks, transaction and final UI state until completion.
    spawn_storage_context_task(&operation, async move {
        library_cleanup::uninstall_module_game_transaction(
            app_handle.state::<DesktopState>(),
            module_id,
        )
        .await
    })
    .await
    .map_err(|error| format!("game removal transaction task failed: {error}"))?
}

pub(super) async fn download_steam_workshop_items_inner(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    ids: Vec<String>,
    missing_only: bool,
    preference: app_network::SourcePreference,
) -> Result<SteamWorkshopDownloadResult, String> {
    let storage_context_operation =
        state.begin_storage_context_operation("Steam Workshop download")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    let instance = read_instance_details(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?;
    let descriptors =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    let descriptor = find_descriptor(&descriptors, &instance.summary.module_id)?;
    let workshop = descriptor.workshop.as_ref().ok_or_else(|| {
        format!(
            "module `{}` does not declare [workshop]",
            descriptor.summary.id
        )
    })?;
    if !workshop.provider.eq_ignore_ascii_case("steam") {
        return Err(format!(
            "module `{}` declares unsupported Workshop provider `{}`",
            descriptor.summary.id, workshop.provider
        ));
    }
    let consumer_app_id = workshop.consumer_app_id.ok_or_else(|| {
        format!(
            "module `{}` does not declare workshop.consumer_app_id",
            descriptor.summary.id
        )
    })?;
    let instance_lock =
        acquire_stopped_instance_mod_mutation(&state, &storage, &instance_id).await?;
    let instance_lock = super::commands_program_storage::prepare_instance_mod_program(
        &storage,
        &storage_context_operation,
        &instance_id,
        instance_lock,
    )
    .await?;
    // Detaching a shared program changes its root. Keep the refreshed instance
    // and its mutation lease until Workshop finishes writing the cache.
    let instance = read_instance_details(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?;
    let install_root =
        PathBuf::from(super::commands_runtime_lifecycle::private_runtime_install_root(&instance)?);
    validate_workshop_download_items(consumer_app_id, &ids, preference).await?;
    let steamcmd = instance_lock
        .install
        .acquire_steamcmd(&storage.settings)
        .await
        .map_err(|error| steamcmd_error_message(&error))?;
    let mutation = std::sync::Arc::new(workshop_download::WorkshopMutation::for_instance(
        instance_lock,
        steamcmd,
    ));
    let label = format!("Download Workshop items for {}", instance.summary.name);
    let job_id = new_background_job_id("workshop-download", &instance.summary.id);

    insert_background_job(
        &state,
        &storage,
        BackgroundJob {
            id: job_id.clone(),
            kind: JobKind::DownloadWorkshop,
            label: label.clone(),
            status: JobStatus::Pending,
            progress_percent: 1.0,
            install_progress: None,
            cancellable: false,
            cancel_requested: false,
            target_id: Some(instance.summary.id.clone()),
            detail: Some(format!(
                "Preparing Steam Workshop download for {}...",
                descriptor.summary.name
            )),
            output_excerpt: None,
        },
    )?;

    let download_result = workshop_download::prepare_workshop_items(
        &storage.settings,
        consumer_app_id,
        &install_root,
        &ids,
        missing_only,
        workshop_download::WorkshopOperation {
            storage: &storage_context_operation,
            mutation: mutation.clone(),
        },
        |update: InstallProgressUpdate| {
            let _ = update_background_job(&state, &job_id, |job| {
                apply_install_progress(job, &update);
            });
        },
    )
    .await;

    let download_result = dst_workshop::finish_download(
        &instance,
        &storage_context_operation,
        mutation.clone(),
        download_result,
    )
    .await;

    match download_result {
        Ok(mut result) => {
            if should_stage_downloaded_workshop_items(&descriptor.manifest_toml) {
                update_background_job(&state, &job_id, |job| {
                    begin_install_deployment(job);
                    job.detail = Some(format!(
                        "Deploying {} Workshop item(s) into this instance...",
                        result.items.len()
                    ));
                })?;
                let stage_result: Result<ManualModStageResult, String> = async {
                    let target = resolve_manual_mod_target(instance_id.clone(), true).await?;
                    // The downloaded cache can be replaced by another Workshop job.
                    // Keep its source stable until this instance has copied the files.
                    let downloaded = result.clone();
                    let mutation = mutation.clone();
                    spawn_blocking_storage_context_task(&storage_context_operation, move || {
                        let _mutation = mutation;
                        stage_downloaded_workshop_items_into_manual_target(&downloaded, &target)
                    })
                    .await
                    .map_err(|error| format!("manual mod staging task failed: {error}"))?
                }
                .await;
                let stage_result = match stage_result {
                    Ok(stage_result) => stage_result,
                    Err(error_message) => {
                        mutate_app_state(&state, |app_state| {
                            if let Some(job) =
                                app_state.jobs.iter_mut().find(|job| job.id == job_id)
                            {
                                fail_install_progress(job, 95.0);
                                job.detail = Some(error_message.clone());
                                job.output_excerpt = Some(error_message.clone());
                            }
                        })?;
                        return Err(error_message);
                    }
                };
                result.output_excerpt = append_mod_install_note(
                    result.output_excerpt,
                    Some(&format!(
                        "Installed {} file(s) into {}.",
                        stage_result.copied_file_count, stage_result.target_label
                    )),
                );
            }
            mutate_app_state(&state, |app_state| {
                if let Some(job) = app_state.jobs.iter_mut().find(|job| job.id == job_id) {
                    complete_install_progress(job);
                    job.detail = Some(format!("{} complete", label));
                    job.output_excerpt = Some(result.output_excerpt.clone());
                }
            })?;
            Ok(result)
        }
        Err(error) => {
            let error_message = error.message;
            let output_excerpt = error.output_excerpt;
            mutate_app_state(&state, |app_state| {
                if let Some(job) = app_state.jobs.iter_mut().find(|job| job.id == job_id) {
                    fail_install_progress(job, 1.0);
                    job.detail = Some(error_message.clone());
                    job.output_excerpt = output_excerpt.clone();
                }
            })?;
            Err(error_message)
        }
    }
}

async fn acquire_stopped_instance_mod_mutation(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    instance_id: &str,
) -> Result<tokio::sync::OwnedMutexGuard<()>, String> {
    let instance_lock = state.acquire_instance_mutation(instance_id).await;
    let active_run = read_active_instance_run(&storage.paths, instance_id)
        .await
        .map_err(|error| error.to_string())?;
    let tracked = state
        .runtime_supervisor
        .lock()
        .map_err(|_| String::from("runtime supervisor lock poisoned"))?
        .is_tracked(instance_id);
    if active_run.is_some() || tracked {
        return Err(format!(
            "Stop instance {instance_id} before installing mods."
        ));
    }
    Ok(instance_lock)
}
pub(super) async fn stage_manual_mod_files_inner(
    state: &tauri::State<'_, DesktopState>,
    storage_context_operation: &StorageContextOperationGuard,
    instance_id: String,
    source_paths: Vec<String>,
) -> Result<ManualModStageResult, String> {
    let source_paths = normalize_manual_mod_source_paths(source_paths)?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    let instance_lock =
        acquire_stopped_instance_mod_mutation(state, &storage, &instance_id).await?;
    let instance_lock = super::commands_program_storage::prepare_instance_mod_program(
        &storage,
        storage_context_operation,
        &instance_id,
        instance_lock,
    )
    .await?;
    let target = resolve_manual_mod_target(instance_id, true).await?;
    spawn_blocking_storage_context_task(storage_context_operation, move || {
        let _instance_lock = instance_lock;
        stage_manual_mod_sources(target, source_paths)
    })
    .await
    .map_err(|error| format!("manual mod staging task failed: {error}"))?
}

pub(super) async fn install_manual_mod_references_inner(
    state: &tauri::State<'_, DesktopState>,
    storage_context_operation: &StorageContextOperationGuard,
    instance_id: String,
    references: Vec<String>,
) -> Result<ManualModStageResult, String> {
    let references = normalize_manual_mod_references(references)?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    let instance = read_instance_details(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?;
    let descriptors =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    let descriptor = find_descriptor(&descriptors, &instance.summary.module_id)?;
    let mods = module_mods_spec_from_manifest(&descriptor.manifest_toml).ok_or_else(|| {
        format!(
            "module `{}` does not declare a [mods] workflow",
            descriptor.summary.id
        )
    })?;

    if mods.manual_staging.is_none() {
        return Err(format!(
            "module `{}` does not declare [mods.manual_staging]",
            descriptor.summary.id
        ));
    }
    if let Some(enablement) = mods.enablement.as_ref() {
        return Err(format!(
            "module `{}` uses `{}` for mod enablement; resolve references before writing settings",
            descriptor.summary.id, enablement.setting_label
        ));
    }

    let instance_lock =
        acquire_stopped_instance_mod_mutation(state, &storage, &instance_id).await?;
    let instance_lock = super::commands_program_storage::prepare_instance_mod_program(
        &storage,
        storage_context_operation,
        &instance_id,
        instance_lock,
    )
    .await?;
    let target = resolve_manual_mod_target(instance_id.clone(), true).await?;
    let instance = read_instance_details(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?;
    let runtime_root =
        PathBuf::from(super::commands_runtime_lifecycle::private_runtime_install_root(&instance)?);
    let runtime_module = instance.summary.module_id.clone();
    let runtime_provider = mods
        .source
        .as_ref()
        .map(|source| source.provider.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let runtime_target = target.target_path.clone();
    tokio::task::spawn_blocking(move || {
        mod_online::validate_runtime(
            &runtime_module,
            &runtime_provider,
            &runtime_root,
            &runtime_target,
        )
    })
    .await
    .map_err(|error| format!("Mod runtime inspection worker failed: {error}"))??;
    let downloaded_sources = DownloadedManualModSourcesCleanup::from_sources(
        tokio::time::timeout(
            Duration::from_secs(180),
            download_manual_mod_sources_for_staging(&mods, &references, &target.target_path),
        )
        .await
        .map_err(|_| "Online Mod installation preparation exceeded its 180-second limit")??,
    );
    spawn_blocking_storage_context_task(storage_context_operation, move || {
        let _instance_lock = instance_lock;
        // Retain downloaded files until the worker settles, even if IPC closes.
        stage_online_mod_sources(target, &downloaded_sources)
    })
    .await
    .map_err(|error| format!("online mod staging task failed: {error}"))?
}

pub(super) async fn read_manual_mod_inventory_inner(
    instance_id: String,
) -> Result<ManualModInventoryResult, String> {
    let target = resolve_manual_mod_target(instance_id, false).await?;
    tokio::task::spawn_blocking(move || mod_inventory::read_inventory(target))
        .await
        .map_err(|error| format!("Mod inventory inspection worker failed: {error}"))?
}

pub(super) async fn resolve_manual_mod_references_inner(
    instance_id: String,
    references: Vec<String>,
) -> Result<ManualModReferenceResolveResult, String> {
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    let instance = read_instance_details(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?;
    let descriptors =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    let descriptor = find_descriptor(&descriptors, &instance.summary.module_id)?;
    let mods = module_mods_spec_from_manifest(&descriptor.manifest_toml).ok_or_else(|| {
        format!(
            "module `{}` does not declare a [mods] workflow",
            descriptor.summary.id
        )
    })?;
    let enablement = mods.enablement.as_ref().ok_or_else(|| {
        format!(
            "module `{}` does not declare [mods.enablement]",
            descriptor.summary.id
        )
    })?;
    let references = normalize_manual_mod_references(references)?;
    let client = reqwest::Client::builder()
        .user_agent(concat!("LanGameServerManager/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|error| format!("failed to build mod reference resolver: {error}"))?;

    let mut items = Vec::new();
    let mut resolved_ids = Vec::new();
    for reference in references {
        match resolve_one_manual_mod_reference(&client, enablement, &reference).await {
            Ok((resolved_id, title)) => {
                if !resolved_ids.iter().any(|id| id == &resolved_id) {
                    resolved_ids.push(resolved_id.clone());
                }
                items.push(ManualModReferenceItem {
                    reference,
                    status: String::from("resolved"),
                    resolved_id: Some(resolved_id),
                    title,
                    message: None,
                });
            }
            Err(message) => items.push(ManualModReferenceItem {
                reference,
                status: String::from("failed"),
                resolved_id: None,
                title: None,
                message: Some(message),
            }),
        }
    }

    if resolved_ids.is_empty() {
        let messages = items
            .iter()
            .filter_map(|item| item.message.as_deref())
            .collect::<Vec<_>>()
            .join("; ");
        return Err(if messages.trim().is_empty() {
            String::from("no mod id could be resolved")
        } else {
            messages
        });
    }

    Ok(ManualModReferenceResolveResult {
        instance_id: instance.summary.id,
        module_id: descriptor.summary.id.clone(),
        source_label: mods
            .source
            .as_ref()
            .map(|source| source.label.clone())
            .unwrap_or_else(|| descriptor.summary.name.clone()),
        setting_key: enablement.setting_key.clone(),
        setting_label: enablement.setting_label.clone(),
        items,
        resolved_ids,
    })
}

pub(super) async fn resolve_manual_mod_target(
    instance_id: String,
    require_installed: bool,
) -> Result<ResolvedManualModTarget, String> {
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    let instance = read_instance_details(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?;
    let descriptors =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    let descriptor = find_descriptor(&descriptors, &instance.summary.module_id)?;
    let mods = module_mods_spec_from_manifest(&descriptor.manifest_toml).ok_or_else(|| {
        format!(
            "module `{}` does not declare a [mods] workflow",
            descriptor.summary.id
        )
    })?;
    let staging = mods.manual_staging.as_ref().ok_or_else(|| {
        format!(
            "module `{}` does not declare [mods.manual_staging]",
            descriptor.summary.id
        )
    })?;
    let install_probe = probe_instance_mod_install(&storage.settings, descriptor, &instance)?;
    if require_installed
        && staging.target_template.contains("paths.install_root")
        && !matches!(install_probe.install_state, InstallState::Installed)
    {
        return Err(format!(
            "module `{}` is not installed yet; install the server files before staging local mods",
            descriptor.summary.id
        ));
    }

    let config_file_path = PathBuf::from(&instance.config_file_path);
    let config_dir = config_file_path
        .parent()
        .unwrap_or(config_file_path.as_path())
        .to_path_buf();
    let instance_root = config_dir
        .parent()
        .unwrap_or(config_dir.as_path())
        .to_path_buf();
    let data_dir = instance_root.join("data");
    let logs_dir = instance_root.join("logs");
    let saves_dir = PathBuf::from(&instance.saves_path);
    let settings_json =
        serde_json::from_str::<Value>(&instance.settings_json).unwrap_or(Value::Null);
    let install_root = PathBuf::from(&install_probe.install_root);
    let target_path = resolve_manual_mod_target_template(
        &staging.target_template,
        &ManualModTargetContext {
            install_root: &install_root,
            instance_root: &instance_root,
            config_dir: &config_dir,
            data_dir: &data_dir,
            logs_dir: &logs_dir,
            saves_dir: &saves_dir,
            instance: &instance,
            settings_json: &settings_json,
        },
    )?;

    Ok(ResolvedManualModTarget {
        instance_id: instance.summary.id,
        module_id: descriptor.summary.id.clone(),
        source_label: mods
            .source
            .as_ref()
            .map(|source| source.label.clone())
            .unwrap_or_else(|| descriptor.summary.name.clone()),
        target_label: staging.target_label.clone(),
        target_path,
        accepts: staging.accepts.clone(),
        id_strategy: mods
            .enablement
            .as_ref()
            .and_then(|enablement| enablement.id_strategy.clone()),
    })
}

fn probe_instance_mod_install(
    settings: &AppSettings,
    descriptor: &ModuleDescriptor,
    instance: &InstanceDetails,
) -> Result<app_steamcmd::ModuleInstallProbe, String> {
    // Package deployment and native loading must target the same instance runtime.
    let private_root = super::commands_runtime_lifecycle::private_runtime_install_root(instance)?;
    Ok(probe_module_install_state_with_override(
        settings,
        &instance.summary.module_id,
        descriptor.summary.steam_app_id,
        descriptor.install.as_ref(),
        descriptor.process.as_ref(),
        Some(private_root.as_str()),
    ))
}

pub(super) fn module_mods_spec_from_manifest(manifest_toml: &str) -> Option<ModuleModsSpec> {
    toml::from_str::<ModuleModsManifest>(manifest_toml)
        .ok()
        .and_then(|manifest| manifest.mods)
}

pub(super) fn should_stage_downloaded_workshop_items(manifest_toml: &str) -> bool {
    module_mods_spec_from_manifest(manifest_toml).is_some_and(|mods| mods.manual_staging.is_some())
}

pub(super) fn normalize_manual_mod_source_paths(
    source_paths: Vec<String>,
) -> Result<Vec<PathBuf>, String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for raw_path in source_paths {
        let trimmed = raw_path.trim();
        if trimmed.is_empty() {
            continue;
        }
        let path = PathBuf::from(trimmed);
        let key = path.to_string_lossy().to_lowercase();
        if seen.insert(key) {
            normalized.push(path);
        }
    }
    if normalized.is_empty() {
        return Err(String::from("drop a downloaded mod file or folder first"));
    }
    Ok(normalized)
}

pub(super) fn normalize_manual_mod_references(
    references: Vec<String>,
) -> Result<Vec<String>, String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for raw_reference in references {
        for candidate in extract_manual_mod_reference_candidates(&raw_reference) {
            let key = candidate.to_lowercase();
            if seen.insert(key) {
                normalized.push(candidate);
            }
        }
    }
    if normalized.is_empty() {
        return Err(String::from("drop a mod link or paste a mod id first"));
    }
    Ok(normalized)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ManualModSourcePathInputSplit {
    pub(super) source_paths: Vec<String>,
    pub(super) reference_inputs: Vec<String>,
}

pub(super) fn split_manual_mod_source_path_inputs(
    source_paths: Vec<String>,
) -> ManualModSourcePathInputSplit {
    let mut local_paths = Vec::new();
    let mut reference_inputs = Vec::new();

    for raw_path in source_paths {
        let trimmed = raw_path.trim();
        if trimmed.is_empty() {
            continue;
        }

        let references = extract_manual_mod_reference_candidates(trimmed);
        if references.is_empty() {
            local_paths.push(trimmed.to_string());
        } else {
            reference_inputs.extend(references);
        }
    }

    ManualModSourcePathInputSplit {
        source_paths: local_paths,
        reference_inputs,
    }
}

pub(super) async fn download_manual_mod_sources_for_staging(
    mods: &ModuleModsSpec,
    references: &[String],
    target: &Path,
) -> Result<Vec<DownloadedManualModSource>, String> {
    let source = mods
        .source
        .as_ref()
        .ok_or_else(|| String::from("this module does not declare a mod source"))?;
    let provider = source.provider.trim().to_ascii_lowercase();
    let client = reqwest::Client::builder()
        .user_agent(concat!("LanGameServerManager/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(45))
        .build()
        .map_err(|error| format!("failed to build mod package downloader: {error}"))?;

    match provider.as_str() {
        "thunderstore" => {
            mod_online::download_thunderstore_mod_sources(&client, source, references, target).await
        }
        "modrinth" => download_modrinth_mod_sources(&client, source, references).await,
        "nexus" => Err(String::from(
            "Nexus Mods downloads require an authenticated browser/API session; download the archive first and provide the local file path.",
        )),
        other => Err(format!(
            "module source `{other}` does not support automatic package downloads yet; download the archive first and provide a local path"
        )),
    }
}

async fn prepare_downloaded_thunderstore_archive(
    archive_path: PathBuf,
    package_name: &str,
    version: &str,
) -> Result<PathBuf, String> {
    let target_path = std::env::temp_dir().join(format!(
        "langame-mod-{}-{}-{}",
        sanitize_mod_site_filename(package_name),
        sanitize_mod_site_filename(version),
        uuid::Uuid::new_v4().simple()
    ));
    let (result_sender, result_receiver) = tokio::sync::oneshot::channel();
    let cancellation_target = target_path.clone();
    tokio::task::spawn_blocking(move || {
        let result = mod_online::reject_loader_archive(&archive_path)
            .and_then(|_| {
                stage_thunderstore_archive_to_directory(archive_path.clone(), target_path.clone())
            })
            .map(|_| target_path.clone());
        let _ = fs::remove_file(&archive_path);
        if result_sender.send(result).is_err() {
            cleanup_downloaded_mod_source(&cancellation_target);
        }
    });
    result_receiver
        .await
        .map_err(|_| String::from("Thunderstore package staging task stopped unexpectedly"))?
}

pub(super) async fn download_modrinth_mod_sources(
    client: &reqwest::Client,
    source: &app_core::ModuleModSourceSpec,
    references: &[String],
) -> Result<Vec<DownloadedManualModSource>, String> {
    let mut sources = DownloadedManualModSourcesCleanup::default();
    if source.loaders.len() != 1 || source.game_versions.len() != 1 {
        return Err(
            "Modrinth installation requires exactly one verified instance loader and game version"
                .into(),
        );
    }
    if references.len() > 32 {
        return Err("An online installation supports at most 32 packages".into());
    }
    for reference in references {
        let project = extract_modrinth_project_reference(reference)
            .ok_or_else(|| format!("Modrinth link did not identify a project: {reference}"))?;
        if (!project.loaders.is_empty() && project.loaders != source.loaders)
            || (!project.game_versions.is_empty() && project.game_versions != source.game_versions)
        {
            return Err(
                "Modrinth link filters do not match the verified instance loader and game version"
                    .into(),
            );
        }
        let loaders = if project.loaders.is_empty() {
            source.loaders.clone()
        } else {
            project.loaders.clone()
        };
        let game_versions = if project.game_versions.is_empty() {
            source.game_versions.clone()
        } else {
            project.game_versions.clone()
        };
        let versions =
            fetch_modrinth_project_versions(client, &project.project, &loaders, &game_versions)
                .await?;
        let (version, file) = select_modrinth_download_file(&versions).ok_or_else(|| {
            format!(
                "Modrinth project `{}` did not publish an installable file for the requested filters",
                project.project
            )
        })?;
        let required = version
            .dependencies
            .iter()
            .filter(|dependency| dependency.dependency_type == "required")
            .map(|dependency| {
                dependency
                    .project_id
                    .as_deref()
                    .or(dependency.version_id.as_deref())
                    .unwrap_or("unidentified dependency")
            })
            .collect::<Vec<_>>();
        if !required.is_empty() {
            return Err(format!(
                "Required Modrinth dependencies cannot be verified for this instance: {}. Import a complete compatible local package instead.",
                required.join(", ")
            ));
        }
        let historical_names =
            fetch_modrinth_project_names(client, &version.project_id, &project.project).await?;
        let source_path = download_mod_site_file(
            client,
            &file.url,
            &project.project,
            &version.version_number,
            Some(&file.filename),
            "jar",
            Some(ModDownloadIntegrity::from_modrinth_file(file)?),
        )
        .await?;
        sources.push(DownloadedManualModSource {
            path: source_path,
            identity: OnlineModIdentity {
                provider: "modrinth".into(),
                project: version.project_id.clone(),
                historical_names,
                version: version.version_number.clone(),
                dependencies: Vec::new(),
            },
            label: format!("{} {}", version.name, version.version_number),
        });
    }
    Ok(sources.into_sources())
}

pub(super) async fn fetch_modrinth_project_versions(
    client: &reqwest::Client,
    project: &str,
    loaders: &[String],
    game_versions: &[String],
) -> Result<Vec<ModrinthProjectVersion>, String> {
    let mut url = reqwest::Url::parse(&format!(
        "https://api.modrinth.com/v2/project/{project}/version"
    ))
    .map_err(|error| format!("failed to build Modrinth project URL: {error}"))?;
    if !loaders.is_empty() {
        url.query_pairs_mut().append_pair(
            "loaders",
            &serde_json::to_string(
                &loaders
                    .iter()
                    .map(|loader| loader.trim().to_ascii_lowercase())
                    .collect::<Vec<_>>(),
            )
            .map_err(|error| error.to_string())?,
        );
    }
    if !game_versions.is_empty() {
        url.query_pairs_mut().append_pair(
            "game_versions",
            &serde_json::to_string(game_versions).map_err(|error| error.to_string())?,
        );
    }
    url.query_pairs_mut()
        .append_pair("featured", "false")
        .append_pair("include_changelog", "false");

    let request = client
        .get(url)
        .build()
        .map_err(|error| format!("failed to prepare Modrinth project request: {error}"))?;
    let response = app_network::read_public_bytes(
        client,
        request,
        Duration::from_secs(12),
        8 * 1024 * 1024,
        app_network::SourcePreference::InternationalFirst,
    )
    .await
    .map_err(|error| format!("failed to resolve Modrinth project: {error}"))?;
    let versions = serde_json::from_slice::<Vec<ModrinthProjectVersion>>(&response.bytes)
        .map_err(|error| format!("failed to read Modrinth project metadata: {error}"))?;
    app_network::record_success(response.url.as_str());
    Ok(versions)
}

pub(super) async fn fetch_thunderstore_package_metadata(
    client: &reqwest::Client,
    namespace: &str,
    package: &str,
) -> Result<ThunderstorePackageMetadata, String> {
    let url = format!("https://thunderstore.io/api/experimental/package/{namespace}/{package}/");
    let request = client
        .get(url)
        .build()
        .map_err(|error| format!("failed to prepare Thunderstore package request: {error}"))?;
    let response = app_network::read_public_bytes(
        client,
        request,
        Duration::from_secs(12),
        8 * 1024 * 1024,
        app_network::SourcePreference::InternationalFirst,
    )
    .await
    .map_err(|error| format!("failed to resolve Thunderstore package: {error}"))?;
    let metadata = serde_json::from_slice::<ThunderstorePackageMetadata>(&response.bytes)
        .map_err(|error| format!("failed to read Thunderstore package metadata: {error}"))?;
    app_network::record_success(response.url.as_str());
    Ok(metadata)
}

pub(super) async fn download_mod_site_archive(
    client: &reqwest::Client,
    download_url: &str,
    package_name: &str,
    version: &str,
) -> Result<PathBuf, String> {
    download_mod_site_file(
        client,
        download_url,
        package_name,
        version,
        None,
        "zip",
        None,
    )
    .await
}

async fn download_mod_site_file(
    client: &reqwest::Client,
    download_url: &str,
    package_name: &str,
    version: &str,
    preferred_filename: Option<&str>,
    fallback_extension: &str,
    integrity: Option<ModDownloadIntegrity>,
) -> Result<PathBuf, String> {
    download_mod_site_file_with_limit(
        client,
        ModDownloadRequest {
            download_url,
            package_name,
            version,
            preferred_filename,
            fallback_extension,
            integrity,
            max_bytes: MANUAL_MOD_SITE_DOWNLOAD_MAX_BYTES,
            download_root: &std::env::temp_dir(),
        },
    )
    .await
}

pub(super) fn select_modrinth_download_file(
    versions: &[ModrinthProjectVersion],
) -> Option<(&ModrinthProjectVersion, &ModrinthVersionFile)> {
    versions
        .iter()
        .filter(|version| {
            version
                .status
                .as_deref()
                .is_none_or(|status| status.eq_ignore_ascii_case("listed"))
        })
        .filter(|version| {
            version
                .version_type
                .as_deref()
                .is_none_or(|version_type| version_type.eq_ignore_ascii_case("release"))
        })
        .find_map(|version| select_modrinth_version_file(version).map(|file| (version, file)))
        .or_else(|| {
            versions
                .iter()
                .filter(|version| {
                    version
                        .status
                        .as_deref()
                        .is_none_or(|status| status.eq_ignore_ascii_case("listed"))
                })
                .find_map(|version| {
                    select_modrinth_version_file(version).map(|file| (version, file))
                })
        })
}

pub(super) fn select_modrinth_version_file(
    version: &ModrinthProjectVersion,
) -> Option<&ModrinthVersionFile> {
    version
        .files
        .iter()
        .find(|file| file.primary && modrinth_file_is_installable(file))
        .or_else(|| {
            version
                .files
                .iter()
                .find(|file| modrinth_file_is_installable(file))
        })
}

pub(super) fn modrinth_file_is_installable(file: &ModrinthVersionFile) -> bool {
    let file_type = file.file_type.as_deref().unwrap_or_default();
    if matches!(
        file_type,
        "sources-jar" | "dev-jar" | "javadoc-jar" | "signature"
    ) {
        return false;
    }
    let filename = file.filename.to_ascii_lowercase();
    !(filename.ends_with("-sources.jar")
        || filename.ends_with("-dev.jar")
        || filename.ends_with("-javadoc.jar")
        || filename.ends_with(".asc")
        || filename.ends_with(".sig"))
}

pub(super) fn sanitize_mod_site_filename(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    if sanitized.is_empty() {
        String::from("package")
    } else {
        sanitized
    }
}

pub(super) fn cleanup_downloaded_mod_source(path: &Path) {
    if path.is_dir() {
        let _ = fs::remove_dir_all(path);
    } else {
        let _ = fs::remove_file(path);
    }
}

pub(super) fn append_mod_install_note(message: String, note: Option<&str>) -> String {
    match note.map(str::trim).filter(|value| !value.is_empty()) {
        Some(note) => format!("{message} {note}"),
        None => message,
    }
}

pub(super) fn extract_manual_mod_reference_candidates(text: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    for token in text
        .replace('\r', "\n")
        .split(|character: char| {
            character.is_whitespace()
                || character == '"'
                || character == '\''
                || character == '<'
                || character == '>'
        })
        .map(|token| {
            token.trim_matches(|character: char| {
                character == ','
                    || character == ';'
                    || character == ')'
                    || character == ']'
                    || character == '}'
            })
        })
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        let candidate = normalize_manual_mod_reference_candidate(token);
        if candidate.starts_with("http://")
            || candidate.starts_with("https://")
            || candidate.to_ascii_lowercase().starts_with("modrinth:")
            || candidate.to_ascii_lowercase().starts_with("mr:")
            || candidate
                .chars()
                .all(|character| character.is_ascii_digit())
        {
            candidates.push(candidate);
        }
    }
    candidates
}

pub(super) fn normalize_manual_mod_reference_candidate(token: &str) -> String {
    let decoded = decode_basic_html_entities(token.trim());
    if decoded.starts_with("http://") || decoded.starts_with("https://") {
        unwrap_manual_mod_redirect_reference(&decoded)
    } else {
        decoded
    }
}

pub(super) fn decode_basic_html_entities(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&AMP;", "&")
        .replace("&quot;", "\"")
        .replace("&QUOT;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&APOS;", "'")
        .replace("&lt;", "<")
        .replace("&LT;", "<")
        .replace("&gt;", ">")
        .replace("&GT;", ">")
}

pub(super) fn unwrap_manual_mod_redirect_reference(reference: &str) -> String {
    let mut current = reference.to_string();
    for _ in 0..4 {
        let Ok(url) = reqwest::Url::parse(&current) else {
            return current;
        };
        let next = url.query_pairs().find_map(|(key, value)| {
            if matches!(
                key.as_ref(),
                "u" | "url" | "q" | "target" | "redirect" | "redirect_url"
            ) && (value.starts_with("http://") || value.starts_with("https://"))
            {
                Some(value.into_owned())
            } else {
                None
            }
        });
        let Some(next) = next else {
            return current;
        };
        if next == current {
            return current;
        }
        current = next;
    }
    current
}

pub(super) async fn resolve_one_manual_mod_reference(
    client: &reqwest::Client,
    enablement: &app_core::ModuleModEnablementSpec,
    reference: &str,
) -> Result<(String, Option<String>), String> {
    if let Some(id) = resolve_direct_manual_mod_reference(enablement, reference) {
        return Ok((id, None));
    }
    match enablement.reference_strategy.as_deref() {
        Some("curseforge_project_id") => {
            resolve_curseforge_project_reference(client, enablement, reference).await
        }
        Some(strategy) => Err(format!("unsupported mod reference strategy `{strategy}`")),
        None => Err(String::from(
            "this module does not declare a mod reference resolver",
        )),
    }
}

pub(super) fn resolve_direct_manual_mod_reference(
    enablement: &app_core::ModuleModEnablementSpec,
    reference: &str,
) -> Option<String> {
    match enablement.reference_strategy.as_deref() {
        Some("steam_workshop_id") => extract_steam_workshop_item_id(reference),
        Some("plain_id") => normalize_plain_manual_mod_id(reference),
        _ => infer_numeric_prefix(reference),
    }
}

pub(super) fn extract_steam_workshop_item_id(reference: &str) -> Option<String> {
    let trimmed = reference.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(url) = reqwest::Url::parse(trimmed) {
        for (key, value) in url.query_pairs() {
            if key.eq_ignore_ascii_case("id") && is_steam_workshop_item_id(&value) {
                return Some(value.into_owned());
            }
        }
    }

    if is_steam_workshop_item_id(trimmed) {
        return Some(trimmed.to_string());
    }

    let lowercase = trimmed.to_ascii_lowercase();
    if !lowercase.contains("workshop")
        && !lowercase.contains("sharedfiles")
        && !lowercase.contains("filedetails")
    {
        return None;
    }

    extract_last_digit_run(trimmed).filter(|id| is_steam_workshop_item_id(id))
}

fn is_steam_workshop_item_id(value: &str) -> bool {
    value.len() >= 5 && value.chars().all(|character| character.is_ascii_digit())
}

fn extract_last_digit_run(value: &str) -> Option<String> {
    let mut current = String::new();
    let mut last = None;
    for character in value.chars() {
        if character.is_ascii_digit() {
            current.push(character);
            continue;
        }
        if is_steam_workshop_item_id(&current) {
            last = Some(std::mem::take(&mut current));
        } else {
            current.clear();
        }
    }
    if is_steam_workshop_item_id(&current) {
        Some(current)
    } else {
        last
    }
}

pub(super) async fn resolve_curseforge_project_reference(
    client: &reqwest::Client,
    enablement: &app_core::ModuleModEnablementSpec,
    reference: &str,
) -> Result<(String, Option<String>), String> {
    if let Some(id) = extract_curseforge_project_id_from_reference(reference) {
        return Ok((id, None));
    }
    let game_id = enablement
        .reference_game_id
        .ok_or_else(|| String::from("module does not declare a CurseForge game id"))?;
    let slug = extract_curseforge_mod_slug(reference)
        .ok_or_else(|| String::from("CurseForge link did not include a project id or mod slug"))?;
    let url = format!("https://api.curse.tools/v1/cf/mods/search?gameId={game_id}&slug={slug}");
    let request = client
        .get(url)
        .build()
        .map_err(|error| format!("failed to prepare CurseForge mod link request: {error}"))?;
    let response = app_network::read_public_bytes(
        client,
        request,
        Duration::from_secs(12),
        4 * 1024 * 1024,
        app_network::SourcePreference::InternationalFirst,
    )
    .await
    .map_err(|error| format!("failed to resolve CurseForge mod link: {error}"))?;
    let body = serde_json::from_slice::<CurseToolsSearchResponse>(&response.bytes)
        .map_err(|error| format!("failed to read CurseForge mod metadata: {error}"))?;
    let item = body
        .data
        .into_iter()
        .next()
        .ok_or_else(|| format!("no CurseForge mod matched `{slug}`"))?;
    Ok((item.id.to_string(), item.name))
}

pub(super) fn extract_curseforge_project_id_from_reference(reference: &str) -> Option<String> {
    let url = reqwest::Url::parse(reference).ok()?;
    let mut previous = "";
    for segment in url.path_segments()? {
        if previous.eq_ignore_ascii_case("projects")
            && segment.chars().all(|character| character.is_ascii_digit())
        {
            return Some(segment.to_string());
        }
        previous = segment;
    }
    for (key, value) in url.query_pairs() {
        if matches!(
            key.as_ref(),
            "projectId" | "projectID" | "project_id" | "modId" | "modID"
        ) && value.chars().all(|character| character.is_ascii_digit())
        {
            return Some(value.into_owned());
        }
    }
    None
}

pub(super) fn extract_curseforge_mod_slug(reference: &str) -> Option<String> {
    let url = reqwest::Url::parse(reference).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    if !host.ends_with("curseforge.com") {
        return None;
    }
    let mut previous = "";
    for segment in url.path_segments()? {
        if previous.eq_ignore_ascii_case("mods") && !segment.trim().is_empty() {
            return Some(segment.to_string());
        }
        previous = segment;
    }
    None
}

pub(super) fn extract_modrinth_project_reference(
    reference: &str,
) -> Option<ModrinthProjectReference> {
    let trimmed = reference.trim();
    for prefix in ["modrinth:", "mr:"] {
        if trimmed.len() > prefix.len()
            && trimmed
                .get(..prefix.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        {
            let project = trimmed[prefix.len()..].trim();
            if is_modrinth_project_token(project) {
                return Some(ModrinthProjectReference {
                    project: project.to_string(),
                    loaders: Vec::new(),
                    game_versions: Vec::new(),
                });
            }
        }
    }

    let url = reqwest::Url::parse(trimmed).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    if !host.ends_with("modrinth.com") {
        return None;
    }
    let segments = url
        .path_segments()?
        .filter(|segment| !segment.trim().is_empty())
        .collect::<Vec<_>>();
    let project = if host == "api.modrinth.com" {
        segments
            .windows(2)
            .find(|window| window[0].eq_ignore_ascii_case("project"))
            .map(|window| window[1].to_string())?
    } else {
        let mut found = None;
        for (index, segment) in segments.iter().enumerate() {
            if matches!(
                segment.to_ascii_lowercase().as_str(),
                "mod" | "plugin" | "datapack" | "resourcepack" | "modpack" | "project"
            ) && index + 1 < segments.len()
            {
                found = Some(segments[index + 1].to_string());
                break;
            }
        }
        found?
    };
    if !is_modrinth_project_token(&project) {
        return None;
    }

    let mut loaders = Vec::new();
    let mut game_versions = Vec::new();
    for (key, value) in url.query_pairs() {
        match key.to_ascii_lowercase().as_str() {
            "loader" | "loaders" | "modloader" | "mod_loader" => {
                append_modrinth_filter_values(&mut loaders, &value)
            }
            "version" | "game_version" | "game_versions" | "minecraft" | "mc" => {
                append_modrinth_filter_values(&mut game_versions, &value)
            }
            _ => {}
        }
    }
    dedupe_case_insensitive(&mut loaders);
    dedupe_case_insensitive(&mut game_versions);

    Some(ModrinthProjectReference {
        project,
        loaders,
        game_versions,
    })
}

pub(super) fn append_modrinth_filter_values(values: &mut Vec<String>, raw_value: &str) {
    let trimmed = raw_value.trim();
    if trimmed.is_empty() {
        return;
    }
    if let Ok(array) = serde_json::from_str::<Vec<String>>(trimmed) {
        values.extend(
            array
                .into_iter()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
        );
        return;
    }
    values.extend(
        trimmed
            .split(',')
            .map(str::trim)
            .map(|value| value.trim_matches('"').trim_matches('\'').to_string())
            .filter(|value| !value.is_empty()),
    );
}

pub(super) fn dedupe_case_insensitive(values: &mut Vec<String>) {
    let mut seen = HashSet::new();
    values.retain(|value| seen.insert(value.to_ascii_lowercase()));
}

pub(super) fn is_modrinth_project_token(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.len() >= 2
        && trimmed.len() <= 64
        && trimmed
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

pub(super) fn extract_thunderstore_package_reference(
    reference: &str,
) -> Option<ThunderstorePackageReference> {
    let url = reqwest::Url::parse(reference).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    if host != "thunderstore.io" && !host.ends_with(".thunderstore.io") {
        return None;
    }
    let segments = url
        .path_segments()?
        .filter(|segment| !segment.trim().is_empty())
        .collect::<Vec<_>>();

    for (index, segment) in segments.iter().enumerate() {
        if (segment.eq_ignore_ascii_case("p") || segment.eq_ignore_ascii_case("package"))
            && index + 2 < segments.len()
            && !segments[index + 1].eq_ignore_ascii_case("download")
        {
            if !segments[index + 1..=index + 2].iter().all(|value| {
                !value.is_empty()
                    && value.len() <= 128
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            }) {
                return None;
            }
            let suffix = &segments[index + 3..];
            let version = match suffix {
                [] | ["versions"] => None,
                [version] if mod_online::valid_thunderstore_version(version) => {
                    Some((*version).to_string())
                }
                _ => return None,
            };
            return Some(ThunderstorePackageReference {
                namespace: segments[index + 1].to_string(),
                package: segments[index + 2].to_string(),
                version,
            });
        }
    }

    None
}

pub(super) fn infer_manual_mod_inventory_id_from_path(
    id_strategy: Option<&str>,
    name: &str,
    path: &Path,
) -> Option<String> {
    match id_strategy {
        Some("palworld_package_name") => infer_palworld_package_name(path),
        _ => infer_manual_mod_inventory_id(id_strategy, name),
    }
}

pub(super) fn infer_palworld_package_name(path: &Path) -> Option<String> {
    const MAX_INFO_BYTES: u64 = 1024 * 1024;
    let info_path = path.join("Info.json");
    mod_inventory::validate_root_chain(&info_path).ok()?;
    let metadata = fs::symlink_metadata(&info_path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_INFO_BYTES {
        return None;
    }
    let mut document = String::new();
    fs::File::open(info_path)
        .ok()?
        .take(MAX_INFO_BYTES + 1)
        .read_to_string(&mut document)
        .ok()?;
    if document.len() as u64 > MAX_INFO_BYTES {
        return None;
    }
    let parsed = serde_json::from_str::<Value>(&document).ok()?;
    parsed
        .get("PackageName")
        .and_then(Value::as_str)
        .and_then(normalize_plain_manual_mod_id)
}

pub(super) fn infer_manual_mod_inventory_id(
    id_strategy: Option<&str>,
    name: &str,
) -> Option<String> {
    match id_strategy {
        Some("numeric_prefix") => infer_numeric_prefix(name),
        Some("folder_name") => normalize_plain_manual_mod_id(name),
        _ => None,
    }
}

pub(super) fn normalize_plain_manual_mod_id(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
    {
        return None;
    }

    if trimmed.chars().any(|character| {
        character.is_control()
            || character.is_whitespace()
            || matches!(
                character,
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
            )
    }) {
        return None;
    }

    Some(String::from(trimmed))
}

pub(super) fn infer_numeric_prefix(name: &str) -> Option<String> {
    let prefix = name
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect::<String>();
    if prefix.len() >= 5 {
        Some(prefix)
    } else {
        None
    }
}

pub(super) struct ManualModTargetContext<'a> {
    pub install_root: &'a Path,
    pub instance_root: &'a Path,
    pub config_dir: &'a Path,
    pub data_dir: &'a Path,
    pub logs_dir: &'a Path,
    pub saves_dir: &'a Path,
    pub instance: &'a InstanceDetails,
    pub settings_json: &'a Value,
}

pub(super) fn resolve_manual_mod_target_template(
    template: &str,
    context: &ManualModTargetContext<'_>,
) -> Result<PathBuf, String> {
    let mut output = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        output.push_str(&rest[..start]);
        let token_start = start + 2;
        let Some(relative_end) = rest[token_start..].find("}}") else {
            return Err(format!("invalid mod target template `{template}`"));
        };
        let token_end = token_start + relative_end;
        let token = rest[token_start..token_end].trim();
        let value = resolve_manual_mod_target_token(token, context)
            .ok_or_else(|| format!("unsupported mod target template token `{token}`"))?;
        output.push_str(&value);
        rest = &rest[token_end + 2..];
    }
    output.push_str(rest);

    let trimmed = output.trim();
    if trimmed.is_empty() {
        return Err(String::from(
            "mod target template resolved to an empty path",
        ));
    }
    // Canonical Windows roots retain the verbatim prefix, which requires native separators.
    #[cfg(windows)]
    let trimmed = trimmed.replace('/', "\\");
    Ok(PathBuf::from(trimmed))
}

pub(super) fn resolve_manual_mod_target_token(
    token: &str,
    context: &ManualModTargetContext<'_>,
) -> Option<String> {
    match token {
        "paths.install_root" => Some(context.install_root.to_string_lossy().into_owned()),
        "paths.instance_root" => Some(context.instance_root.to_string_lossy().into_owned()),
        "paths.config_dir" => Some(context.config_dir.to_string_lossy().into_owned()),
        "paths.data_dir" => Some(context.data_dir.to_string_lossy().into_owned()),
        "paths.logs_dir" => Some(context.logs_dir.to_string_lossy().into_owned()),
        "paths.saves_dir" => Some(context.saves_dir.to_string_lossy().into_owned()),
        "instance.id" => Some(context.instance.summary.id.clone()),
        "instance.name" => Some(context.instance.summary.name.clone()),
        _ => token
            .strip_prefix("settings.")
            .and_then(|path| lookup_json_path(context.settings_json, path))
            .map(json_value_to_template_string),
    }
}

pub(super) fn lookup_json_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for segment in path.split('.') {
        current = current.get(segment)?;
    }
    Some(current)
}

pub(super) fn json_value_to_template_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

pub(super) async fn validate_module_game_inner(
    state: tauri::State<'_, DesktopState>,
    module_id: String,
) -> Result<ModuleInstallResult, String> {
    let _storage_context_operation = state.begin_storage_context_operation("module validation")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    let descriptors =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    sync_modules(&storage.paths, &descriptors)
        .await
        .map_err(|error| error.to_string())?;
    let descriptor = find_descriptor(&descriptors, &module_id)?;
    let install_root_override = load_module_install_root_override(&storage.paths, &module_id).await;
    let module = map_module_details_with_install_state(
        &storage.settings,
        descriptor,
        install_root_override.as_deref(),
    );
    reject_unsafe_direct_download_replacement(
        ModuleInstallOperation::Validate,
        &storage.settings,
        descriptor,
    )?;
    let program_root = PathBuf::from(
        probe_module_install_state_with_override(
            &storage.settings,
            &module_id,
            descriptor.summary.steam_app_id,
            descriptor.install.as_ref(),
            descriptor.process.as_ref(),
            install_root_override.as_deref(),
        )
        .install_root,
    );
    let program_guard = super::commands_program_storage::acquire_library_program_mutation(
        &state,
        &storage,
        &module_id,
        &program_root,
    )
    .await?;
    let transient_install_state =
        if matches!(module.summary.install_state, InstallState::NotInstalled) {
            InstallState::Installing
        } else {
            InstallState::Updating
        };
    let job_id = new_background_job_id("module-validate", &module.summary.id);
    let operation = InstallationJobLease::begin(&state, job_id.clone())?;
    let label = format!("Check updates for {}", module.summary.name);

    insert_background_job(
        &state,
        &storage,
        BackgroundJob {
            id: job_id.clone(),
            kind: JobKind::ValidateGame,
            label: label.clone(),
            status: JobStatus::Pending,
            progress_percent: 1.0,
            install_progress: Some(queued_install_progress()),
            cancellable: true,
            cancel_requested: false,
            target_id: Some(module.summary.id.clone()),
            detail: Some(format!(
                "Preparing SteamCMD update check for {}...",
                module.summary.name
            )),
            output_excerpt: None,
        },
    )?;
    set_module_install_state(&state, &module.summary.id, transient_install_state)?;

    let validate_result = super::commands_program_storage::install_program_with_baseline(
        super::commands_program_storage::ProgramUpdateRequest {
            storage: &storage,
            descriptor,
            module: &module,
            root: &program_root,
            operation: &_storage_context_operation,
            guard: program_guard,
            validate: true,
            cancellation: operation.cancellation(),
        },
        |update: InstallProgressUpdate| {
            let _ = update_background_job(&state, &job_id, |job| {
                apply_install_progress(job, &update);
            });
        },
    )
    .await;

    let refreshed_module = module_summary_with_install_state(
        &storage.settings,
        descriptor,
        install_root_override.as_deref(),
    );

    match validate_result {
        Ok((result, _program_guard)) => {
            persist_install_result(&storage, &result, true).await?;
            mutate_app_state(&state, |app_state| {
                merge_module_install_state(app_state, &refreshed_module);
                if let Some(job) = app_state.jobs.iter_mut().find(|job| job.id == job_id) {
                    complete_install_progress(job);
                    job.detail = Some(format!("{} complete", label));
                    job.output_excerpt = Some(result.output_excerpt.clone());
                }
            })?;
            Ok(result)
        }
        Err(error) => {
            let error_message = steamcmd_error_message(&error);
            let output_excerpt = steamcmd_error_excerpt(&error).or_else(|| Some(error.to_string()));
            let _ =
                persist_descriptor_install_states(&storage, std::slice::from_ref(descriptor)).await;
            mutate_app_state(&state, |app_state| {
                merge_module_install_state(app_state, &refreshed_module);
                if let Some(job) = app_state.jobs.iter_mut().find(|job| job.id == job_id) {
                    finish_install_error(job, &error);
                    job.detail = Some(error_message.clone());
                    job.output_excerpt = output_excerpt.clone();
                }
            })?;
            Err(error_message)
        }
    }
}

#[cfg(test)]
#[path = "commands_module_lifecycle_tests.rs"]
mod lifecycle_safety_tests;

#[cfg(test)]
#[path = "commands_mod_target_tests.rs"]
mod mod_target_tests;

#[cfg(test)]
#[path = "commands_mod_inventory_tests.rs"]
mod mod_inventory_tests;
