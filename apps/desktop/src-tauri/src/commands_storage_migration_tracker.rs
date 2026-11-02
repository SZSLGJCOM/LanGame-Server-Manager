use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use app_core::{AppPathSettingsInput, StorageMigrationPhase as Phase, StorageMigrationStatus};

#[derive(Clone)]
pub(super) struct Run {
    pub id: String,
    generation: u64,
    pub cancellation: Arc<AtomicBool>,
}

pub(super) enum Claim {
    Started(Run, StorageMigrationStatus),
    Observe(StorageMigrationStatus),
}

struct Record {
    status: StorageMigrationStatus,
    generation: u64,
    running: bool,
    cancellation: Arc<AtomicBool>,
    publication_started: bool,
}

#[derive(Default)]
struct TrackerState {
    next_generation: u64,
    record: Option<Record>,
}

/// One bounded operation. A separate generation distinguishes the preview and
/// execution workers sharing its ID, so delayed callbacks cannot rewind state.
#[derive(Default)]
pub(crate) struct MigrationTracker {
    state: Mutex<TrackerState>,
}

impl MigrationTracker {
    pub(super) fn snapshot(&self) -> Result<Option<StorageMigrationStatus>, String> {
        Ok(self
            .state
            .lock()
            .map_err(|_| lock_error())?
            .record
            .as_ref()
            .map(|record| record.status.clone()))
    }

    pub(super) fn begin_preview(&self, status: StorageMigrationStatus) -> Result<Claim, String> {
        let mut state = self.state.lock().map_err(|_| lock_error())?;
        if let Some(current) = &state.record {
            if current.status.recovery_required {
                return Err(String::from(
                    "存储迁移需要重启恢复，恢复完成前不能开始新迁移。",
                ));
            }
            if current.running {
                return if same_input(&current.status.input, &status.input) {
                    Ok(Claim::Observe(current.status.clone()))
                } else {
                    Err(String::from(
                        "已有存储迁移任务正在运行，请等待完成或取消后重试。",
                    ))
                };
            }
        }
        start(&mut state, status)
    }

    pub(super) fn begin_execute(&self, id: &str) -> Result<Claim, String> {
        let mut state = self.state.lock().map_err(|_| lock_error())?;
        let current = state
            .record
            .as_ref()
            .filter(|record| record.status.operation_id == id)
            .ok_or_else(|| String::from("找不到该存储迁移任务，请重新预检。"))?;
        if current.running || current.status.phase != Phase::Ready {
            return Ok(Claim::Observe(current.status.clone()));
        }
        if current.status.recovery_required || current.status.conflict_count != 0 {
            return Err(String::from("迁移尚有文件冲突或待恢复操作，不能执行。"));
        }
        let mut status = current.status.clone();
        status.phase = Phase::Verifying;
        status.can_cancel = true;
        status.error = None;
        start(&mut state, status)
    }

    pub(super) fn update(
        &self,
        run: &Run,
        mut status: StorageMigrationStatus,
    ) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|_| lock_error())?;
        let Some(record) = matching(&mut state, run).filter(|record| record.running) else {
            return Ok(());
        };
        if status.operation_id != run.id {
            return Err(String::from("迁移进度返回了不匹配的操作标识。"));
        }
        if recovery_phase(&status.phase) {
            record.publication_started = true;
        }
        // The supervising task publishes a terminal/Ready result only after it
        // has observed the worker and completed host state synchronization.
        if !status.is_active() {
            status.phase = record.status.phase.clone();
            status.can_cancel = false;
        }
        if record.publication_started && !recovery_phase(&status.phase) {
            status.phase = record.status.phase.clone();
        }
        if record.publication_started || run.cancellation.load(Ordering::Acquire) {
            status.can_cancel = false;
        }
        record.status = status;
        Ok(())
    }

    pub(super) fn finish(
        &self,
        run: &Run,
        mut status: StorageMigrationStatus,
        recovery_required: bool,
    ) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|_| lock_error())?;
        if let Some(record) = matching(&mut state, run).filter(|record| record.running) {
            if status.operation_id != run.id {
                return Err(String::from("迁移结果返回了不匹配的操作标识。"));
            }
            status.can_cancel = false;
            status.recovery_required = recovery_required;
            record.status = status;
            record.running = false;
        }
        Ok(())
    }

    pub(super) fn request_cancel(&self, id: &str) -> Result<StorageMigrationStatus, String> {
        let mut state = self.state.lock().map_err(|_| lock_error())?;
        let record = state
            .record
            .as_mut()
            .filter(|record| record.status.operation_id == id)
            .ok_or_else(|| String::from("找不到该存储迁移任务。"))?;
        if record.running && record.status.can_cancel && !record.publication_started {
            record.cancellation.store(true, Ordering::Release);
            record.status.can_cancel = false;
        }
        Ok(record.status.clone())
    }

    pub(super) fn publication_started(&self, run: &Run) -> Result<bool, String> {
        let mut state = self.state.lock().map_err(|_| lock_error())?;
        matching(&mut state, run)
            .map(|record| record.publication_started)
            .ok_or_else(|| String::from("存储迁移任务已经改变。"))
    }

    /// Hydrate once without overwriting a concurrently admitted in-memory run.
    /// Active disk phases have no worker after restart and need explicit recovery.
    pub(super) fn restore_if_empty(
        &self,
        mut status: StorageMigrationStatus,
        recovery_required: bool,
    ) -> Result<(StorageMigrationStatus, bool), String> {
        let mut state = self.state.lock().map_err(|_| lock_error())?;
        if let Some(record) = &state.record {
            return Ok((record.status.clone(), false));
        }
        status.recovery_required = recovery_required;
        let record = Record {
            status: status.clone(),
            generation: 0,
            running: false,
            cancellation: Arc::new(AtomicBool::new(false)),
            publication_started: recovery_required,
        };
        state.record = Some(record);
        Ok((status, true))
    }
}

fn start(state: &mut TrackerState, mut status: StorageMigrationStatus) -> Result<Claim, String> {
    status.recovery_required = false;
    state.next_generation = state
        .next_generation
        .checked_add(1)
        .ok_or_else(|| String::from("存储迁移任务序号已耗尽，请重启应用。"))?;
    let run = Run {
        id: status.operation_id.clone(),
        generation: state.next_generation,
        cancellation: Arc::new(AtomicBool::new(false)),
    };
    state.record = Some(Record {
        status: status.clone(),
        generation: run.generation,
        running: true,
        cancellation: run.cancellation.clone(),
        publication_started: false,
    });
    Ok(Claim::Started(run, status))
}

fn matching<'a>(state: &'a mut TrackerState, run: &Run) -> Option<&'a mut Record> {
    state.record.as_mut().filter(|record| {
        record.status.operation_id == run.id && record.generation == run.generation
    })
}

pub(super) fn recovery_phase(phase: &Phase) -> bool {
    matches!(phase, Phase::Applying | Phase::Cleaning)
}

fn same_input(left: &AppPathSettingsInput, right: &AppPathSettingsInput) -> bool {
    left.servers_root == right.servers_root
        && left.archives_root == right.archives_root
        && left.games_root == right.games_root
        && left.steamcmd_root == right.steamcmd_root
}

fn lock_error() -> String {
    String::from("存储迁移进度锁已损坏，请重启应用检查恢复状态。")
}

#[cfg(test)]
#[path = "commands_storage_migration_tracker_tests.rs"]
mod tests;
