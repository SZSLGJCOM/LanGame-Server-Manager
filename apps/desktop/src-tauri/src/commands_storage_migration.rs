//! Local migration commands. Registration and host admission are owned by the
//! command registry; background workers retain the storage transition lease.
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use app_core::{AppPathSettingsInput, StorageMigrationPhase as Phase, StorageMigrationStatus};
use app_storage::{StoragePaths, storage_migration as core};
use tauri::Manager;

use crate::state::{DesktopState, StorageContextTransitionGuard, StorageSource};

#[path = "commands_storage_migration_tracker.rs"]
mod tracker;
#[path = "commands_storage_migration_worker.rs"]
mod worker;
pub(crate) use tracker::MigrationTracker;
use tracker::{Claim, Run, recovery_phase};
use worker::{Failure, Job};

#[tauri::command]
pub async fn preview_app_storage_migration<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, DesktopState>,
    input: AppPathSettingsInput,
) -> Result<StorageMigrationStatus, String> {
    if !cfg!(windows) {
        return Err(String::from("存储目录迁移目前仅支持 Windows。"));
    }
    let status = core::initial_status(uuid::Uuid::new_v4().to_string(), input.clone());
    let claim = state.storage_migration.begin_preview(status)?;
    begin_background(app, &state, claim, Job::Preview(input))
}

#[tauri::command]
pub async fn read_app_storage_migration(
    state: tauri::State<'_, DesktopState>,
) -> Result<Option<StorageMigrationStatus>, String> {
    read_current(&state).await
}

#[tauri::command]
pub async fn execute_app_storage_migration<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, DesktopState>,
    operation_id: String,
) -> Result<StorageMigrationStatus, String> {
    if !cfg!(windows) {
        return Err(String::from("存储目录迁移目前仅支持 Windows。"));
    }
    validate_id(&operation_id)?;
    read_current(&state).await?;
    let claim = state.storage_migration.begin_execute(&operation_id)?;
    begin_background(app, &state, claim, Job::Execute)
}

#[tauri::command]
pub async fn cancel_app_storage_migration(
    state: tauri::State<'_, DesktopState>,
    operation_id: String,
) -> Result<StorageMigrationStatus, String> {
    validate_id(&operation_id)?;
    read_current(&state).await?;
    state.storage_migration.request_cancel(&operation_id)
}

fn validate_id(id: &str) -> Result<(), String> {
    uuid::Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| String::from("存储迁移操作标识无效。"))
}

fn begin_background<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: &DesktopState,
    claim: Claim,
    job: Job,
) -> Result<StorageMigrationStatus, String> {
    let (run, mut status) = match claim {
        Claim::Observe(status) => return Ok(status),
        Claim::Started(run, status) => (run, status),
    };
    let source = state.storage_source();
    let anchor = match source.resolve_paths() {
        Ok(anchor) => anchor,
        Err(message) => {
            eprintln!("Storage migration could not resolve its diagnostic log anchor: {message}");
            status.phase = Phase::Failed;
            status.can_cancel = false;
            status.error = Some(message);
            state
                .storage_migration
                .finish(&run, status.clone(), false)?;
            return Ok(status);
        }
    };
    let (action, message) = match &job {
        Job::Preview(_) => (
            "storage.migration.request",
            "Checking storage migration request",
        ),
        Job::Execute => (
            "storage.migration.execute.request",
            "Executing verified storage migration",
        ),
    };
    append_migration_log(&anchor, &status, "info", action, message, false);
    let transition = match state.begin_storage_context_transition() {
        Ok(transition) => transition,
        Err(message) => {
            status.phase = Phase::Failed;
            status.can_cancel = false;
            status.error = Some(message);
            log_outcome(&anchor, &status, false);
            state
                .storage_migration
                .finish(&run, status.clone(), false)?;
            return Ok(status);
        }
    };
    let background = Background {
        app,
        source,
        anchor,
        tracker: state.storage_migration.clone(),
        run,
        initial: status.clone(),
        transition,
        job,
    };
    // Dropping the IPC request must not drop the supervisor or its transition.
    tauri::async_runtime::spawn(background.supervise());
    Ok(status)
}

async fn read_current(state: &DesktopState) -> Result<Option<StorageMigrationStatus>, String> {
    if let Some(status) = state.storage_migration.snapshot()? {
        return Ok(Some(status));
    }
    let source = state.storage_source();
    let saved = tokio::task::spawn_blocking(move || {
        let paths = source.resolve_paths()?;
        core::read_status(&paths).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("读取存储迁移状态失败：{error}"))??;
    if let Some(status) = state.storage_migration.snapshot()? {
        return Ok(Some(status));
    }
    let Some(mut status) = saved else {
        return Ok(None);
    };
    let needs_recovery = status.recovery_required || recovery_phase(&status.phase);
    if status.is_active() {
        status.phase = Phase::Failed;
        status.can_cancel = false;
        status.error = Some(if needs_recovery {
            String::from("存储迁移提交尚未完成，请重启应用完成恢复。")
        } else {
            String::from("上次存储迁移在提交前中断，原数据仍保留，请重新预检。")
        });
    }
    if needs_recovery {
        let transition = match state.begin_storage_context_transition() {
            Ok(transition) => transition,
            Err(error) => {
                return state.storage_migration.snapshot()?.map(Some).ok_or(error);
            }
        };
        transition.require_recovery(
            status
                .error
                .clone()
                .unwrap_or_else(|| String::from("存储迁移需要重启恢复。")),
        )?;
    }
    state
        .storage_migration
        .restore_if_empty(status, needs_recovery)
        .map(|(status, _)| Some(status))
}

struct Background<R: tauri::Runtime> {
    app: tauri::AppHandle<R>,
    source: StorageSource,
    anchor: StoragePaths,
    tracker: Arc<MigrationTracker>,
    run: Run,
    initial: StorageMigrationStatus,
    transition: StorageContextTransitionGuard,
    job: Job,
}

impl<R: tauri::Runtime> Background<R> {
    async fn supervise(self) {
        let Self {
            app,
            source,
            anchor,
            tracker,
            run,
            initial,
            transition,
            job,
        } = self;
        let executing = matches!(&job, Job::Execute);
        let progress_tracker = tracker.clone();
        let progress_run = run.clone();
        let progress: core::Progress = Arc::new(move |status| {
            if let Err(error) = progress_tracker.update(&progress_run, status) {
                progress_run
                    .cancellation
                    .store(true, std::sync::atomic::Ordering::Release);
                eprintln!("Storage migration progress failed: {error}");
            }
        });
        // The outer task owns the transition while the inner task may panic.
        // JoinError is an observed failure, never an implicit unlock/success.
        let outcome = tokio::spawn(worker::run(app.clone(), source, run.clone(), job, progress))
            .await
            .unwrap_or_else(|error| {
                Err(Failure::message(format!(
                    "存储迁移后台任务异常退出：{error}"
                )))
            });
        let (mut status, mut needs_recovery) = match outcome {
            Ok(status) => {
                let needs_recovery = status.recovery_required;
                (status, needs_recovery)
            }
            Err(failure) => {
                failure_status(&anchor, &tracker, &run, initial, executing, failure).await
            }
        };
        if status.phase == Phase::Completed {
            if let Err(message) = refresh_completed_settings(&app, &status) {
                needs_recovery = true;
                status
                    .cleanup_warnings
                    .push(format!("存储迁移已完成；{message}，请重启应用加载新路径。"));
            }
            if let Err(message) = super::clear_assistant_pending_operations() {
                status
                    .cleanup_warnings
                    .push(format!("存储迁移已完成，但旧助手确认未能清除：{message}"));
                eprintln!("Storage migration assistant confirmation cleanup failed: {message}");
            }
        }
        if needs_recovery {
            let message = status
                .error
                .clone()
                .unwrap_or_else(|| status.cleanup_warnings.join("\n"));
            if let Err(error) = transition.require_recovery(message) {
                let message =
                    format!("迁移恢复保护未能登记：{error}；请退出并重启应用后再操作存储。");
                status.error = Some(match status.error {
                    Some(previous) => format!("{previous}\n{message}"),
                    None => message,
                });
                eprintln!("Storage migration recovery barrier failed: {error}");
            }
        }
        status.recovery_required = needs_recovery;
        log_outcome(&anchor, &status, needs_recovery);
        drop(transition);
        if let Err(error) = tracker.finish(&run, status, needs_recovery) {
            eprintln!("Storage migration completion state failed: {error}");
        }
    }
}

async fn failure_status(
    anchor: &StoragePaths,
    tracker: &MigrationTracker,
    run: &Run,
    initial: StorageMigrationStatus,
    executing: bool,
    failure: Failure,
) -> (StorageMigrationStatus, bool) {
    let anchor = anchor.clone();
    let saved = tokio::task::spawn_blocking(move || core::read_status(&anchor))
        .await
        .map_err(|error| error.to_string())
        .and_then(|result| result.map_err(|error| error.to_string()));
    let matching_saved = saved
        .as_ref()
        .ok()
        .and_then(|saved| saved.as_ref())
        .filter(|status| status.operation_id == run.id);
    if let Some(saved) = matching_saved
        && saved.phase == Phase::Completed
        && executing
    {
        let mut completed = saved.clone();
        completed.cleanup_warnings.push(format!(
            "迁移已持久化完成，后台返回结果时发生异常：{}",
            failure.message
        ));
        return (completed, saved.recovery_required);
    }
    let mut status = match tracker.snapshot() {
        Ok(Some(status)) if status.operation_id == run.id => status,
        Ok(_) => initial,
        Err(error) => {
            eprintln!("Storage migration failure snapshot unavailable: {error}");
            initial
        }
    };
    if let Some(saved) = matching_saved {
        // Failure records discover backups after the last progress callback.
        // Import those verified locations without rewinding live byte counters.
        status
            .preserved_destinations
            .clone_from(&saved.preserved_destinations);
    }
    let publication_started = tracker.publication_started(run).unwrap_or_else(|error| {
        eprintln!("Storage migration publication state unavailable: {error}");
        executing
    });
    let needs_recovery = failure.recovery_required
        || publication_started
        || matching_saved
            .is_some_and(|status| status.recovery_required || recovery_phase(&status.phase))
        || (executing && saved.is_err());
    let mut message = failure.message;
    if let Err(error) = saved {
        message.push_str(&format!("\n无法确认磁盘迁移状态：{error}"));
    }
    if needs_recovery {
        message.push_str("\n存储提交尚未确认完成，后续存储写入已暂停，请重启应用完成恢复。");
    }
    status.phase = if failure.cancelled && !needs_recovery {
        Phase::Cancelled
    } else {
        Phase::Failed
    };
    status.can_cancel = false;
    status.recovery_required = needs_recovery;
    status.error = Some(message);
    (status, needs_recovery)
}

fn refresh_completed_settings<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    status: &StorageMigrationStatus,
) -> Result<(), String> {
    let state = app
        .try_state::<DesktopState>()
        .ok_or_else(|| String::from("桌面运行状态已不可用"))?;
    let mut current = state
        .app_state
        .write()
        .map_err(|_| String::from("桌面设置锁已损坏"))?;
    current.settings.servers_root = status.input.servers_root.clone();
    current.settings.archives_root = status.input.archives_root.clone();
    current.settings.games_root = status.input.games_root.clone();
    current.settings.steamcmd_root = status.input.steamcmd_root.clone();
    Ok(())
}

fn log_outcome(anchor: &StoragePaths, status: &StorageMigrationStatus, needs_recovery: bool) {
    let (level, action, fallback) = match status.phase {
        Phase::Ready if status.conflict_count > 0 => (
            "warn",
            "storage.migration.ready",
            "Storage migration preview found file conflicts; execution is blocked",
        ),
        Phase::Ready => (
            "info",
            "storage.migration.ready",
            "Storage migration preview is ready",
        ),
        Phase::Completed => (
            if status.cleanup_warnings.is_empty() && !needs_recovery {
                "info"
            } else {
                "warn"
            },
            "storage.migration.completed",
            "Storage migration completed",
        ),
        Phase::Cancelled => (
            "info",
            "storage.migration.cancelled",
            "Storage migration was cancelled before publication",
        ),
        _ => (
            "error",
            "storage.migration.failed",
            "Storage migration failed",
        ),
    };
    append_migration_log(
        anchor,
        status,
        level,
        action,
        status.error.as_deref().unwrap_or(fallback),
        needs_recovery,
    );
}

fn append_migration_log(
    anchor: &StoragePaths,
    status: &StorageMigrationStatus,
    level: &str,
    action: &str,
    message: &str,
    needs_recovery: bool,
) {
    let entry = serde_json::json!({
        "ts_unix_ms": SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_millis()).unwrap_or(0),
        "level": level, "action": action, "message": message,
        "context": {
            "operation_id": status.operation_id, "phase": status.phase,
            "servers_root": status.input.servers_root, "archives_root": status.input.archives_root,
            "games_root": status.input.games_root, "steamcmd_root": status.input.steamcmd_root,
            "conflict_count": status.conflict_count, "reused_files": status.reused_files,
            "needs_recovery": needs_recovery,
        },
    });
    let path = anchor.app_log_path();
    if let Err(error) = crate::desktop_app_log::append(&path, &entry) {
        eprintln!(
            "failed to persist storage migration diagnostic log {}: {error}",
            path.display()
        );
    }
}

#[cfg(all(test, windows))]
#[path = "commands_storage_migration_tests.rs"]
pub(in crate::commands) mod tests;

#[cfg(all(test, windows))]
#[path = "commands_storage_migration_acceptance_tests.rs"]
mod acceptance_tests;
