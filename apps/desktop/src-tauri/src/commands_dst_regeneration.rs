//! One confirmed DST world regeneration: save and stop, delete every shard's
//! world save (cluster and shard configuration, mods and settings stay), then
//! start so the server generates new worlds. One background job reports it.
use super::commands_dst_import_validation::{
    dontstarve_cluster_root_from_config_file_path, is_link_or_reparse,
};
use super::commands_dst_world_state::{
    DstWorldStartPreview, DstWorldState, build_dst_world_start_preview,
};
use crate::state::DesktopState;
use app_core::{AppState, BackgroundJob, InstanceDetails, JobKind, JobStatus, StartInstanceResult};
use std::fs;
use std::future::Future;
use std::io::ErrorKind;
use std::path::Path;
use std::sync::{Mutex, RwLock};

/// The steps shown in the activity bar, published as the job's `detail`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    Stopping,
    Generating,
    Done,
    Failed,
}

impl Phase {
    fn code(self) -> &'static str {
        match self {
            Self::Stopping => "stopping",
            Self::Generating => "generating",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }
}

/// Publishing progress never fails the world operation it describes.
fn report(jobs: &RwLock<AppState>, instance_id: &str, name: &str, phase: Phase) {
    let Ok(mut state) = jobs.write() else { return };
    let (status, percent) = match phase {
        Phase::Stopping => (JobStatus::Running, 0.0),
        Phase::Generating => (JobStatus::Running, 50.0),
        Phase::Done => (JobStatus::Completed, 100.0),
        Phase::Failed => (JobStatus::Failed, 100.0),
    };
    super::commands_runtime_supervision::publish_background_job(
        &mut state,
        BackgroundJob {
            warnings: Vec::new(),
            id: format!("dst-world-regeneration:{instance_id}"),
            kind: JobKind::RegenerateWorld,
            label: name.to_owned(),
            status,
            progress_percent: percent,
            install_progress: None,
            cancellable: false,
            cancel_requested: false,
            target_id: Some(instance_id.to_owned()),
            detail: Some(phase.code().to_owned()),
            output_excerpt: None,
        },
    );
}

/// Stop, delete, then start; a failed step ends the job without running later ones.
pub(super) async fn sequence<T, Stop, Delete, Start>(
    report: impl Fn(Phase),
    stop: impl FnOnce() -> Stop,
    delete: impl FnOnce() -> Delete,
    start: impl FnOnce(DstWorldStartPreview) -> Start,
) -> Result<T, String>
where
    Stop: Future<Output = Result<(), String>>,
    Delete: Future<Output = Result<DstWorldStartPreview, String>>,
    Start: Future<Output = Result<T, String>>,
{
    report(Phase::Stopping);
    let result = async {
        stop().await?;
        let preview = delete().await?;
        report(Phase::Generating);
        start(preview).await
    }
    .await;
    report(if result.is_ok() {
        Phase::Done
    } else {
        Phase::Failed
    });
    result
}

fn plain_directory(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !is_link_or_reparse(&metadata) => Ok(true),
        Ok(_) => Err(format!("Unsafe world-data entry: {}", path.display())),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("Cannot inspect {}: {error}", path.display())),
    }
}

/// Deletes each shard's `save` directory and returns the new-world start
/// preview. Everything else in the cluster and shard directories is kept.
pub(super) fn delete_world_saves(
    details: &InstanceDetails,
) -> Result<DstWorldStartPreview, String> {
    let cluster = dontstarve_cluster_root_from_config_file_path(&details.config_file_path);
    if plain_directory(&cluster)? {
        for shard in build_dst_world_start_preview(details)?.shards {
            let shard_root = cluster.join(&shard.shard);
            let save = shard_root.join("save");
            if plain_directory(&shard_root)? && plain_directory(&save)? {
                fs::remove_dir_all(&save)
                    .map_err(|error| format!("Cannot delete {}: {error}", save.display()))?;
            }
        }
    }
    let preview = build_dst_world_start_preview(details)?;
    if preview
        .shards
        .iter()
        .any(|shard| shard.state == DstWorldState::Existing)
    {
        return Err("A world save remained after deletion.".into());
    }
    Ok(preview)
}

fn running(
    runtime: &Mutex<app_runtime::RuntimeSupervisor>,
    details: &InstanceDetails,
) -> Result<bool, String> {
    Ok(details.active_run.is_some()
        || runtime
            .lock()
            .map_err(|_| "Runtime supervisor lock poisoned")?
            .is_tracked(&details.summary.id))
}

#[tauri::command]
pub async fn regenerate_dontstarve_world(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
) -> Result<StartInstanceResult, String> {
    let storage = state.bootstrap_storage()?;
    let details = app_storage::read_instance_details(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?;
    if details.summary.module_id != "dontstarve" {
        return Err("World regeneration is only available for Don't Starve Together.".into());
    }
    let name = details.summary.name.clone();
    let jobs = state.app_state.clone();
    sequence(
        |phase| report(&jobs, &instance_id, &name, phase),
        || async {
            if running(&state.runtime_supervisor, &details)? {
                super::commands_runtime_lifecycle::stop_instance_process(
                    state.clone(),
                    instance_id.clone(),
                )
                .await?;
            }
            Ok(())
        },
        || delete_stopped(&state, &instance_id),
        |preview| {
            super::commands_runtime_lifecycle::start_instance_process_inner(
                app_handle,
                state.clone(),
                instance_id.clone(),
                Some(preview),
            )
        },
    )
    .await
}

async fn delete_stopped(
    state: &tauri::State<'_, DesktopState>,
    instance_id: &str,
) -> Result<DstWorldStartPreview, String> {
    let operation = state.begin_storage_context_operation("DST world regeneration")?;
    let storage = state.bootstrap_storage()?;
    let id = instance_id.to_owned();
    let runtime = state.runtime_supervisor.clone();
    super::commands_storage::run_instance_mutation_to_completion(
        state,
        &operation,
        instance_id,
        move || async move {
            let details = app_storage::read_instance_details(&storage.paths, &id)
                .await
                .map_err(|error| error.to_string())?;
            if running(&runtime, &details)? {
                return Err("The server is still running; its world was not deleted.".into());
            }
            tokio::task::spawn_blocking(move || delete_world_saves(&details))
                .await
                .map_err(|error| format!("DST world deletion task failed: {error}"))?
        },
    )
    .await
}

#[cfg(test)]
#[path = "commands_dst_regeneration_tests.rs"]
mod tests;
