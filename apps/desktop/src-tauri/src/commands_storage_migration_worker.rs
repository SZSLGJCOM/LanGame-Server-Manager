use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use app_core::{AppPathSettingsInput, AppSettings, StorageMigrationStatus};
use app_storage::{StorageError, StoragePaths, storage_migration as core};
use tauri::Manager;

use super::tracker::{Run, recovery_phase};
use crate::state::{DesktopState, StorageSource};

pub(super) enum Job {
    Preview(AppPathSettingsInput),
    Execute,
}

pub(super) struct Failure {
    pub message: String,
    pub cancelled: bool,
    pub recovery_required: bool,
}

impl Failure {
    pub(super) fn message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            cancelled: false,
            recovery_required: false,
        }
    }
    fn storage(error: StorageError) -> Self {
        Self {
            cancelled: matches!(error, StorageError::StorageMigrationCancelled),
            message: error.to_string(),
            recovery_required: false,
        }
    }
}

pub(super) async fn run<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    source: StorageSource,
    run: Run,
    job: Job,
    progress: core::Progress,
) -> Result<StorageMigrationStatus, Failure> {
    let source_for_read = source.clone();
    let saved = tokio::task::spawn_blocking(move || {
        let paths = source_for_read.resolve_paths()?;
        core::read_status(&paths).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| Failure::message(error.to_string()))?
    .map_err(Failure::message)?;
    if saved
        .as_ref()
        .is_some_and(|status| status.recovery_required || recovery_phase(&status.phase))
    {
        return Err(Failure {
            message: String::from("上次存储迁移尚未完成提交，请重启应用完成恢复。"),
            cancelled: false,
            recovery_required: true,
        });
    }
    check_cancelled(&run)?;
    let storage = tokio::task::spawn_blocking(move || source.bootstrap_storage())
        .await
        .map_err(|error| Failure::message(error.to_string()))?
        .map_err(Failure::message)?;
    let state = app
        .try_state::<DesktopState>()
        .ok_or_else(|| Failure::message("桌面运行状态已不可用。"))?;
    let blockers = super::super::collect_app_path_update_blockers(&state, &storage)
        .await
        .map_err(Failure::message)?;
    super::super::ensure_app_path_update_allowed(&blockers).map_err(Failure::message)?;
    check_cancelled(&run)?;
    match job {
        Job::Preview(input) => {
            let next = super::super::normalize_app_path_settings(&storage.settings, &input);
            let programs = core::program_roots(&storage.paths)
                .await
                .map_err(Failure::storage)?;
            let paths = storage.paths.clone();
            let next_for_moves = next.clone();
            let cancellation = run.cancellation.clone();
            let additional = tokio::task::spawn_blocking(move || {
                revision_moves(&paths, &next_for_moves, programs, &cancellation)
            })
            .await
            .map_err(|error| Failure::message(error.to_string()))??;
            check_cancelled(&run)?;
            core::preview(
                &storage.paths,
                next,
                run.id,
                additional,
                run.cancellation,
                progress,
            )
            .await
            .map(|prepared| prepared.status())
            .map_err(Failure::storage)
        }
        Job::Execute => {
            let paths = storage.paths.clone();
            let id = run.id.clone();
            let prepared = tokio::task::spawn_blocking(move || core::read_prepared(&paths, &id))
                .await
                .map_err(|error| Failure::message(error.to_string()))?
                .map_err(Failure::storage)?;
            check_cancelled(&run)?;
            core::execute(&storage.paths, prepared, run.cancellation, progress)
                .await
                .map_err(Failure::storage)
        }
    }
}

fn revision_moves(
    paths: &StoragePaths,
    next: &AppSettings,
    programs: Vec<(String, PathBuf)>,
    cancellation: &std::sync::atomic::AtomicBool,
) -> Result<Vec<(PathBuf, PathBuf)>, Failure> {
    let mappings = core::path_mappings(paths, next).map_err(Failure::storage)?;
    let mut additional = BTreeMap::new();
    for (module, old_root) in programs {
        if cancellation.load(Ordering::Acquire) {
            return Err(Failure::storage(StorageError::StorageMigrationCancelled));
        }
        let new_root = core::map_path(&old_root, &mappings).map_err(Failure::storage)?;
        let (source, target) = app_steamcmd::program_install_revision_move_paths(
            &paths.instances_root,
            &module,
            &old_root,
            Path::new(&next.servers_root),
            &new_root,
        )
        .map_err(|error| Failure::message(error.to_string()))?;
        match std::fs::symlink_metadata(&source) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(Failure::message(format!(
                    "读取程序安装代次目录失败：{error}"
                )));
            }
        }
        let normalized_source = core::map_path(&source, &[]).map_err(Failure::storage)?;
        let normalized_target =
            core::normalize_destination(&target, next).map_err(Failure::storage)?;
        let default_target = core::map_path(&source, &mappings).map_err(Failure::storage)?;
        // Moving the servers root already handles unchanged revision keys.
        if normalized_source == normalized_target || default_target == normalized_target {
            continue;
        }
        if let Some(previous) = additional.insert(normalized_source, normalized_target.clone())
            && previous != normalized_target
        {
            return Err(Failure::message(
                "同一程序代次目录映射到了不同目标，请先修复重复程序记录。",
            ));
        }
    }
    Ok(additional.into_iter().collect())
}

fn check_cancelled(run: &Run) -> Result<(), Failure> {
    if run.cancellation.load(Ordering::Acquire) {
        Err(Failure::storage(StorageError::StorageMigrationCancelled))
    } else {
        Ok(())
    }
}
