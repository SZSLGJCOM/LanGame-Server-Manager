use super::super::commands_install_progress::{
    apply_install_progress, begin_install_deployment, complete_install_progress,
    fail_install_progress,
};
use super::*;

#[path = "workshop_preparation_state.rs"]
mod preparation_state;
use preparation_state::PreparedTicket;
pub(crate) use preparation_state::WorkshopPreparations;

#[derive(serde::Serialize)]
pub(crate) struct SteamWorkshopPreparation {
    ticket_id: String,
    consumer_app_id: u32,
}

pub(in crate::commands) fn workshop_attempt_job_id(
    instance_id: &str,
    token: Option<&str>,
) -> Result<String, String> {
    match token {
        None => Ok(new_background_job_id("workshop-download", instance_id)),
        Some(token) => {
            let id = uuid::Uuid::parse_str(token).map_err(|_| "Invalid Workshop attempt token")?;
            if id.to_string() != token {
                return Err("Workshop attempt token must be a canonical UUID".into());
            }
            Ok(format!("workshop-download:{instance_id}:{token}"))
        }
    }
}

pub(in crate::commands) fn register_workshop_job(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    job: BackgroundJob,
) -> Result<(), String> {
    ensure_storage_context_snapshot_current(state, storage, "Workshop download")?;
    let mut app_state = state
        .app_state
        .write()
        .map_err(|_| "desktop state lock poisoned")?;
    if app_state.jobs.iter().any(|existing| existing.id == job.id) {
        return Err("Workshop attempt has already been accepted".into());
    }
    super::super::commands_runtime_supervision::publish_background_job(&mut app_state, job);
    Ok(())
}

#[tauri::command]
pub async fn prepare_steam_workshop_items<R: tauri::Runtime>(
    app_handle: tauri::AppHandle<R>,
    instance_id: String,
    ids: Vec<String>,
    attempt_token: String,
    locale: Option<String>,
) -> Result<SteamWorkshopPreparation, String> {
    let operation = app_handle
        .state::<DesktopState>()
        .begin_storage_context_operation("Workshop download preparation")?;
    let worker_operation = operation.clone();
    spawn_storage_context_task(&operation, async move {
        prepare_inner(
            app_handle.state::<DesktopState>(),
            instance_id,
            ids,
            attempt_token,
            app_network::SourcePreference::from_locale(locale.as_deref()),
            worker_operation,
        )
        .await
    })
    .await
    .map_err(|error| format!("Workshop preparation task failed: {error}"))?
}

async fn prepare_inner(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    ids: Vec<String>,
    attempt_token: String,
    preference: app_network::SourcePreference,
    operation: StorageContextOperationGuard,
) -> Result<SteamWorkshopPreparation, String> {
    let job_id = workshop_attempt_job_id(&instance_id, Some(&attempt_token))?;
    let storage = state.bootstrap_storage()?;
    let instance = read_instance_details(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?;
    let descriptors =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    let descriptor = find_descriptor(&descriptors, &instance.summary.module_id)?;
    let workshop = descriptor
        .workshop
        .as_ref()
        .filter(|spec| spec.provider.eq_ignore_ascii_case("steam"))
        .ok_or("This instance does not support Steam Workshop downloads")?;
    let app = workshop
        .consumer_app_id
        .ok_or("Workshop consumer app ID is missing")?;
    let id = uuid::Uuid::new_v4().to_string();
    let reservation = state.workshop_preparations.reserve(&id, &instance_id)?;
    register_workshop_job(
        &state,
        &storage,
        BackgroundJob {
            warnings: vec![],
            id: job_id.clone(),
            kind: JobKind::DownloadWorkshop,
            label: format!("Download Workshop items for {}", instance.summary.name),
            status: JobStatus::Pending,
            progress_percent: 0.0,
            install_progress: None,
            cancellable: false,
            cancel_requested: false,
            target_id: Some(instance_id.clone()),
            detail: Some("Waiting for a Workshop download slot...".into()),
            output_excerpt: None,
        },
    )?;
    let mut ticket = PreparedTicket {
        instance_id: instance_id.clone(),
        module_id: instance.summary.module_id.clone(),
        config_path: instance.config_file_path.clone(),
        storage_root: storage.paths.app_data_root.clone(),
        consumer_app_id: app,
        ids: ids.clone(),
        job_id: job_id.clone(),
        prepared: None,
        operation,
        workspace: None,
        finished: None,
    };
    let result: Result<(), String> = async {
        tokio::select! {
            _ = ticket.operation.cancelled() => return Err("installation_cancelled".into()),
            result = crate::steam_workshop::validate_workshop_download_items(app, &ids, preference) => result?,
        }
        // Advisory cache inspection only; commit always rechecks under the live
        // instance/program/cache leases. Preparation never detaches a program.
        let instance_lock = tokio::select! {
            _ = ticket.operation.cancelled() => return Err("installation_cancelled".into()),
            result = acquire_stopped_instance_mod_mutation(&state, &storage, &instance_id) => result?,
        };
        let root = app_storage::resolve_instance_runtime_root(
            super::super::commands_program_storage::instance_root(&instance)?,
        )
        .map_err(|error| error.to_string())?;
        let settings = storage.settings.clone();
        let requested = ids.clone();
        let missing = spawn_blocking_storage_context_task(&ticket.operation, move || {
            let _instance_lock = instance_lock;
            let mut snapshot = inspect_workshop_items(&settings, app, &root, &requested)
                .map_err(|error| steamcmd_error_message(&error))?;
            dst_workshop::verify_cached_items(&mut snapshot, &requested);
            let installed: HashSet<_> = snapshot
                .items
                .into_iter()
                .filter(|item| item.installed)
                .map(|item| item.item_id)
                .collect();
            Ok::<_, String>(
                requested
                    .into_iter()
                    .filter(|id| !installed.contains(id))
                    .collect::<Vec<_>>(),
            )
        })
        .await
        .map_err(|error| format!("Workshop cache inspection failed: {error}"))??;
        if !missing.is_empty() {
            let mut worker = state
                .workshop_preparations
                .worker(&ticket.operation)
                .await?;
            let tools = storage
                .paths
                .steamcmd_root
                .parent()
                .ok_or("SteamCMD root has no parent")?
                .join("langame-workshop-workers");
            let runtime_root = tools.join(format!("runtime-{}", worker.id));
            let download_root = tools.join("requests").join(&id);
            ticket.workspace = Some(download_root.clone());
            let cancellation = app_steamcmd::InstallCancellation::new();
            let fetch = app_steamcmd::prefetch_workshop_items(
                &storage.settings,
                app_steamcmd::WorkshopPrefetchRequest {
                    runtime_root: &runtime_root,
                    download_root: &download_root,
                    consumer_app_id: app,
                    ids: &missing,
                },
                &state.install_locks,
                &cancellation,
                |update| {
                    if !update.is_raw_output() {
                        let _ = update_background_job(&state, &job_id, |job| {
                            apply_install_progress(job, &update)
                        });
                    }
                },
            );
            tokio::pin!(fetch);
            let downloaded = tokio::select! {
                result = &mut fetch => result,
                _ = ticket.operation.cancelled() => { cancellation.cancel(); fetch.await },
            };
            if matches!(&downloaded, Err(app_steamcmd::SteamCmdError::InstallProcessCleanupFailed { .. }
                | app_steamcmd::SteamCmdError::SteamCmdPreparationCleanupFailed { .. })) {
                worker.quarantine();
                // Preserve this attempt too: native writes may still be active.
                ticket.workspace = None;
            }
            let downloaded = downloaded.map_err(|error| steamcmd_error_message(&error))?;
            if cancellation.is_cancelled() {
                return Err("installation_cancelled".into());
            }
            ticket.prepared = Some(Arc::new(downloaded));
            // Native output is now fixed in this request's private directory;
            // the runtime slot must not be held while waiting for FIFO commit.
            drop(worker);
        }
        if ticket.operation.cancellation_token().load(Ordering::SeqCst) {
            return Err("installation_cancelled".into());
        }
        update_background_job(&state, &job_id, |job| {
            job.status = JobStatus::Running;
            job.progress_percent = 96.0;
            job.detail = Some("Download ready; waiting to deploy into this instance...".into());
        })?;
        Ok(())
    }
    .await;
    match result {
        Ok(()) => {
            reservation.publish(ticket, state.app_state.clone())?;
            Ok(SteamWorkshopPreparation {
                ticket_id: id,
                consumer_app_id: app,
            })
        }
        Err(error) => {
            finish_preparation_error(&state, &job_id, &error);
            Err(error)
        }
    }
}

fn finish_preparation_error(state: &tauri::State<'_, DesktopState>, job_id: &str, error: &str) {
    let _ = update_background_job(state, job_id, |job| {
        let progress = job.progress_percent;
        fail_install_progress(job, progress);
        job.detail = Some(error.into());
        job.output_excerpt = Some(error.into());
    });
}

#[tauri::command]
pub async fn commit_steam_workshop_items<R: tauri::Runtime>(
    app_handle: tauri::AppHandle<R>,
    instance_id: String,
    ticket_id: String,
) -> Result<SteamWorkshopDownloadResult, String> {
    let state = app_handle.state::<DesktopState>();
    let ticket = state
        .workshop_preparations
        .take(&ticket_id, &instance_id)?
        .ok_or("Workshop preparation is no longer available")?;
    let operation = ticket.operation.clone();
    spawn_storage_context_task(&operation, async move {
        let state = app_handle.state::<DesktopState>();
        let result = commit_inner(&state, &ticket).await;
        match &result {
            Ok(downloaded) => {
                update_background_job(&state, &ticket.job_id, |job| {
                    complete_install_progress(job);
                    job.detail = Some("Workshop files deployed".into());
                    job.output_excerpt = Some(downloaded.output_excerpt.clone());
                })?;
            }
            Err(error) => finish_preparation_error(&state, &ticket.job_id, error),
        }
        result
    })
    .await
    .map_err(|error| format!("Workshop commit task failed: {error}"))?
}

async fn commit_inner(
    state: &tauri::State<'_, DesktopState>,
    ticket: &PreparedTicket,
) -> Result<SteamWorkshopDownloadResult, String> {
    if ticket.operation.cancellation_token().load(Ordering::SeqCst) {
        return Err("installation_cancelled".into());
    }
    let storage = state.bootstrap_storage()?;
    if storage.paths.app_data_root != ticket.storage_root {
        return Err("Workshop storage context changed".into());
    }
    let instance_lock = tokio::select! {
        _ = ticket.operation.cancelled() => return Err("installation_cancelled".into()),
        result = acquire_stopped_instance_mod_mutation(state, &storage, &ticket.instance_id) => result?,
    };
    let instance = read_instance_details(&storage.paths, &ticket.instance_id)
        .await
        .map_err(|error| error.to_string())?;
    if instance.summary.module_id != ticket.module_id
        || instance.config_file_path != ticket.config_path
    {
        return Err("Workshop target instance changed after preparation".into());
    }
    let descriptors =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    let descriptor = find_descriptor(&descriptors, &ticket.module_id)?;
    if descriptor
        .workshop
        .as_ref()
        .and_then(|spec| spec.consumer_app_id)
        != Some(ticket.consumer_app_id)
    {
        return Err("Workshop target app changed after preparation".into());
    }
    let program = super::super::commands_program_storage::prepare_instance_mod_program(
        &state.install_locks,
        &storage,
        &ticket.operation,
        &ticket.instance_id,
        instance_lock,
    )
    .await?;
    if ticket.operation.cancellation_token().load(Ordering::SeqCst) {
        return Err("installation_cancelled".into());
    }
    let instance = read_instance_details(&storage.paths, &ticket.instance_id)
        .await
        .map_err(|error| error.to_string())?;
    let root = PathBuf::from(
        super::super::commands_runtime_lifecycle::private_runtime_install_root(&instance)?,
    );
    let steamcmd = tokio::select! {
        _ = ticket.operation.cancelled() => return Err("installation_cancelled".into()),
        result = program.install.acquire_steamcmd(&storage.settings) =>
            result.map_err(|error| steamcmd_error_message(&error))?,
    };
    update_background_job(state, &ticket.job_id, |job| {
        begin_install_deployment(job);
    })?;
    if let Some(prepared) = &ticket.prepared {
        app_steamcmd::publish_prefetched_workshop_items(
            prepared.clone(),
            root.clone(),
            &program.install,
        )
        .await
        .map_err(|error| steamcmd_error_message(&error))?;
    }
    if ticket.operation.cancellation_token().load(Ordering::SeqCst) {
        return Err("installation_cancelled".into());
    }
    let mutation = Arc::new(workshop_download::WorkshopMutation::for_instance(
        program, steamcmd,
    ));
    // Include cached members too; a complete collection is one deployment intent.
    let downloaded = workshop_download::prepare_workshop_items(
        &storage.settings,
        ticket.consumer_app_id,
        &root,
        &ticket.ids,
        true,
        workshop_download::WorkshopOperation {
            storage: &ticket.operation,
            mutation: mutation.clone(),
            account_name: None,
        },
        |update| {
            if !update.is_raw_output() {
                let _ = update_background_job(state, &ticket.job_id, |job| {
                    apply_install_progress(job, &update)
                });
            }
        },
    )
    .await;
    let mut result =
        dst_workshop::finish_download(&instance, &ticket.operation, mutation.clone(), downloaded)
            .await
            .map_err(|error| error.message)?;
    if should_stage_downloaded_workshop_items(&descriptor.manifest_toml) {
        let target = resolve_manual_mod_target(&storage, ticket.instance_id.clone(), true).await?;
        let source = result.clone();
        let staged = spawn_blocking_storage_context_task(&ticket.operation, move || {
            let _mutation = mutation;
            stage_downloaded_workshop_items_into_manual_target(&source, &target)
        })
        .await
        .map_err(|error| format!("Workshop deployment task failed: {error}"))??;
        result.output_excerpt = append_mod_install_note(
            result.output_excerpt,
            Some(&format!(
                "Installed {} file(s) into {}.",
                staged.copied_file_count, staged.target_label
            )),
        );
    }
    Ok(result)
}

#[tauri::command]
pub fn retain_steam_workshop_preparation(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    ticket_id: String,
) -> Result<(), String> {
    state.workshop_preparations.retain(&ticket_id, &instance_id)
}

#[tauri::command]
pub async fn discard_steam_workshop_preparation(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    ticket_id: String,
) -> Result<(), String> {
    if let Some(ticket) = state.workshop_preparations.take(&ticket_id, &instance_id)? {
        finish_preparation_error(
            &state,
            &ticket.job_id,
            "Workshop preparation discarded before deployment",
        );
        drop(ticket);
    }
    Ok(())
}

#[cfg(test)]
#[path = "commands_workshop_preparation_tests.rs"]
mod tests;
