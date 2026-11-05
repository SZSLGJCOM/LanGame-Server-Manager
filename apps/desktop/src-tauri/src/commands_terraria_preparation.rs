use super::*;
use crate::commands::commands_install_progress::{
    InstallationJobLease, apply_install_progress, complete_install_progress, fail_install_progress,
    finish_install_error, queued_install_progress,
};
use app_core::terraria::{TmodLoaderInstallation, enabled_mod_names, installation};

fn has_enabled_mods(settings: &Value) -> bool {
    settings
        .as_object()
        .is_some_and(|values| enabled_mod_names(values).next().is_some())
}

/// Called after the successful settings write, with the instance editor lock
/// still held. Only the empty -> nonempty transition can enqueue a download.
pub(super) fn on_saved(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    previous: &InstanceDetails,
    saved: &InstanceDetails,
) -> Result<(), String> {
    if saved.summary.module_id != "terraria" {
        return Ok(());
    }
    let settings: Value =
        serde_json::from_str(&saved.settings_json).map_err(|error| error.to_string())?;
    if !has_enabled_mods(&settings) {
        return Ok(());
    }
    let before: Value =
        serde_json::from_str(&previous.settings_json).map_err(|error| error.to_string())?;
    let instance_root = crate::commands::commands_program_storage::instance_root(saved)?;
    if has_enabled_mods(&before)
        || matches!(
            installation(instance_root, &settings),
            TmodLoaderInstallation::Custom(_)
        )
    {
        return Ok(());
    }
    let instance_id = &saved.summary.id;
    let job_prefix = format!("instance-tmodloader:{instance_id}:");
    let preparing = state
        .app_state
        .read()
        .map_err(|_| "desktop state lock poisoned")?
        .jobs
        .iter()
        .any(|job| {
            job.id.starts_with(&job_prefix)
                && matches!(job.status, JobStatus::Pending | JobStatus::Running)
        });
    if preparing {
        return Ok(());
    }
    let operation = state.begin_storage_context_operation("tModLoader preparation")?;
    let job_id = new_background_job_id("instance-tmodloader", instance_id);
    let mut job = BackgroundJob {
        id: job_id.clone(),
        label: format!("为 {} 准备 tModLoader", saved.summary.name),
        kind: JobKind::ValidateGame,
        target_id: Some(instance_id.clone()),
        status: JobStatus::Pending,
        cancellable: true,
        cancel_requested: false,
        progress_percent: 0.0,
        install_progress: Some(queued_install_progress()),
        detail: Some("已启用 Mod，正在准备开服所需的 tModLoader。".into()),
        output_excerpt: None,
        warnings: Vec::new(),
    };
    let lease = match InstallationJobLease::begin(state, job_id) {
        Ok(lease) => lease,
        Err(error) => {
            fail_install_progress(&mut job, 0.0);
            job.detail = Some(format!("Mod 已启用，后台准备未开始，开服时会再试：{error}"));
            insert_background_job(state, storage, job)?;
            return Err(error);
        }
    };
    insert_background_job(state, storage, job)?;
    let locks = state.install_locks.clone();
    let root = instance_root.to_owned();
    let storage = storage.clone();
    let id = instance_id.clone();
    let worker_operation = operation.clone();
    drop(spawn_storage_context_task(&operation, async move {
        let cancellation = lease.cancellation().clone();
        let mut progress_error = None;
        let result = {
            let preparation = app_steamcmd::prepare_tmodloader_runtime(
                &locks,
                &root,
                cancellation.clone(),
                |update| {
                    if progress_error.is_none()
                        && let Err(error) = lease.update(|job| apply_install_progress(job, &update))
                    {
                        progress_error = Some(error);
                        cancellation.cancel();
                    }
                },
            );
            tokio::pin!(preparation);
            tokio::select! {
                biased;
                _ = worker_operation.cancelled() => {
                    cancellation.cancel();
                    preparation.await
                },
                result = &mut preparation => result,
            }
        };
        let outcome = lease.update(|job| {
            if let Some(error) = &progress_error {
                fail_install_progress(job, 1.0);
                job.detail = Some(error.clone());
                return;
            }
            match &result {
                Ok(_) => {
                    complete_install_progress(job);
                    job.detail = Some("tModLoader 已准备好。".into());
                }
                Err(error) => {
                    finish_install_error(job, error);
                    job.detail = Some(error.to_string());
                }
            }
        });
        if let Some(message) = outcome
            .err()
            .or(progress_error)
            .or_else(|| result.err().map(|error| error.to_string()))
        {
            append_desktop_app_log(
                &storage,
                "warn",
                "instance.tmodloader.preparation",
                &message,
                json!({ "instance_id": id }),
            );
        }
    }));
    Ok(())
}
