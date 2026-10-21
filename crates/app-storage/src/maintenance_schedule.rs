use app_core::maintenance_schedule::{
    DailyMaintenanceAction, DailyMaintenanceRun, DailyMaintenanceSchedule,
    UpdateDailyMaintenanceScheduleInput, valid_daily_time,
};
use sqlx::{Row, sqlite::SqliteRow};

use crate::storage_db::connect_pool;
use crate::{StorageError, StoragePaths, current_unix_ms};

#[derive(Debug, thiserror::Error)]
pub enum MaintenanceScheduleError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error("{0}")]
    Invalid(String),
}

#[derive(Debug, Clone)]
pub struct ClaimedDailyMaintenance {
    pub schedule: DailyMaintenanceSchedule,
    pub run_token: String,
}

fn from_row(row: SqliteRow) -> Result<DailyMaintenanceSchedule, MaintenanceScheduleError> {
    let action: String = row.try_get("action")?;
    let action = match action.as_str() {
        "backup" => DailyMaintenanceAction::Backup,
        "restart" => DailyMaintenanceAction::Restart,
        _ => {
            return Err(MaintenanceScheduleError::Invalid(
                "Unknown daily maintenance action".into(),
            ));
        }
    };
    let date: Option<String> = row.try_get("last_local_date")?;
    Ok(DailyMaintenanceSchedule {
        instance_id: row.try_get("instance_id")?,
        action,
        enabled: row.try_get("enabled")?,
        time_local: row.try_get("time_local")?,
        revision: row.try_get("revision")?,
        last_run: date
            .map(|local_date| -> Result<_, sqlx::Error> {
                Ok(DailyMaintenanceRun {
                    local_date,
                    status: row.try_get("run_status")?,
                    started_at_unix_ms: row.try_get::<i64, _>("started_at_unix_ms")? as u64,
                    completed_at_unix_ms: row
                        .try_get::<Option<i64>, _>("completed_at_unix_ms")?
                        .map(|v| v as u64),
                    detail: row.try_get("detail")?,
                })
            })
            .transpose()?,
    })
}

pub async fn read_daily_maintenance_schedule(
    paths: &StoragePaths,
    instance_id: &str,
    action: DailyMaintenanceAction,
) -> Result<DailyMaintenanceSchedule, MaintenanceScheduleError> {
    let pool = connect_pool(paths).await?;
    let result = async {
        sqlx::query_scalar::<_, String>("SELECT id FROM instances WHERE id=?1")
            .bind(instance_id)
            .fetch_one(&pool)
            .await?;
        let row = sqlx::query(
            "SELECT * FROM instance_maintenance_schedules WHERE instance_id=?1 AND action=?2",
        )
        .bind(instance_id)
        .bind(action.as_str())
        .fetch_optional(&pool)
        .await?;
        row.map(from_row)
            .unwrap_or_else(|| Ok(DailyMaintenanceSchedule::disabled(instance_id, action)))
    }
    .await;
    pool.close().await;
    result
}

pub async fn update_daily_maintenance_schedule(
    paths: &StoragePaths,
    input: &UpdateDailyMaintenanceScheduleInput,
) -> Result<DailyMaintenanceSchedule, MaintenanceScheduleError> {
    if !valid_daily_time(&input.time_local) || input.expected_revision < 0 {
        return Err(MaintenanceScheduleError::Invalid(
            "Daily maintenance time must use HH:mm (00:00–23:59).".into(),
        ));
    }
    let pool = connect_pool(paths).await?;
    let result = async {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query_scalar::<_, String>("SELECT id FROM instances WHERE id=?1")
            .bind(&input.instance_id).fetch_one(&mut *tx).await?;
        let current: Option<i64> = sqlx::query_scalar("SELECT revision FROM instance_maintenance_schedules WHERE instance_id=?1 AND action=?2")
            .bind(&input.instance_id).bind(input.action.as_str()).fetch_optional(&mut *tx).await?;
        if current.unwrap_or(0) != input.expected_revision {
            return Err(MaintenanceScheduleError::Invalid("Daily maintenance policy changed elsewhere. Reload it before saving.".into()));
        }
        sqlx::query("INSERT INTO instance_maintenance_schedules(instance_id,action,enabled,time_local,revision) VALUES(?1,?2,?3,?4,1) ON CONFLICT(instance_id,action) DO UPDATE SET enabled=excluded.enabled,time_local=excluded.time_local,revision=instance_maintenance_schedules.revision+1")
            .bind(&input.instance_id).bind(input.action.as_str()).bind(input.enabled).bind(&input.time_local)
            .execute(&mut *tx).await?;
        let row = sqlx::query("SELECT * FROM instance_maintenance_schedules WHERE instance_id=?1 AND action=?2")
            .bind(&input.instance_id).bind(input.action.as_str()).fetch_one(&mut *tx).await?;
        let updated = from_row(row)?;
        tx.commit().await?;
        Ok(updated)
    }.await;
    pool.close().await;
    result
}

/// Recover receipts from a prior manager process without repeating any action.
pub async fn interrupt_abandoned_daily_maintenance(
    paths: &StoragePaths,
    owner_session: &str,
) -> Result<(), MaintenanceScheduleError> {
    let pool = connect_pool(paths).await?;
    let result = sqlx::query("UPDATE instance_maintenance_schedules SET run_status='interrupted',completed_at_unix_ms=?1,detail='The manager stopped before the operation result was recorded. Check the instance and backups; this occurrence will not be repeated.' WHERE run_status='running' AND (owner_session IS NULL OR owner_session<>?2)")
        .bind(current_unix_ms() as i64).bind(owner_session).execute(&pool).await;
    pool.close().await;
    result?;
    Ok(())
}

/// Claim all actions in the current local minute before doing any external work.
/// Dates only move forward, so a repeated minute/day after clock changes cannot replay work.
pub async fn claim_due_daily_maintenance(
    paths: &StoragePaths,
    local_date: &str,
    due_minutes: &[String],
    owner_session: &str,
) -> Result<Vec<ClaimedDailyMaintenance>, MaintenanceScheduleError> {
    if local_date.len() != 10
        || due_minutes.len() > 1440
        || due_minutes.iter().any(|time| !valid_daily_time(time))
        || owner_session.is_empty()
    {
        return Err(MaintenanceScheduleError::Invalid(
            "Invalid daily maintenance clock or owner.".into(),
        ));
    }
    let pool = connect_pool(paths).await?;
    let result = async {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        let rows = sqlx::query("SELECT * FROM instance_maintenance_schedules WHERE enabled=1 AND time_local IN (SELECT value FROM json_each(?1)) AND (last_local_date IS NULL OR last_local_date<?2) AND (run_status IS NULL OR run_status<>'running') ORDER BY time_local,instance_id,action")
            .bind(serde_json::to_string(due_minutes).map_err(StorageError::from)?).bind(local_date).fetch_all(&mut *tx).await?;
        let mut claimed = Vec::with_capacity(rows.len());
        for row in rows {
            let schedule = from_row(row)?;
            let run_token = uuid::Uuid::new_v4().to_string();
            sqlx::query("UPDATE instance_maintenance_schedules SET last_local_date=?1,run_token=?2,owner_session=?3,run_status='running',started_at_unix_ms=?4,completed_at_unix_ms=NULL,detail=NULL WHERE instance_id=?5 AND action=?6")
                .bind(local_date).bind(&run_token).bind(owner_session).bind(current_unix_ms() as i64)
                .bind(&schedule.instance_id).bind(schedule.action.as_str()).execute(&mut *tx).await?;
            claimed.push(ClaimedDailyMaintenance { schedule, run_token });
        }
        tx.commit().await?;
        Ok(claimed)
    }.await;
    pool.close().await;
    result
}

pub async fn complete_daily_maintenance(
    paths: &StoragePaths,
    claim: &ClaimedDailyMaintenance,
    status: &str,
    detail: &str,
) -> Result<(), MaintenanceScheduleError> {
    if !matches!(status, "succeeded" | "failed" | "skipped") {
        return Err(MaintenanceScheduleError::Invalid(
            "Invalid daily maintenance result.".into(),
        ));
    }
    let pool = connect_pool(paths).await?;
    let result = sqlx::query("UPDATE instance_maintenance_schedules SET run_status=?1,completed_at_unix_ms=?2,detail=?3 WHERE instance_id=?4 AND action=?5 AND run_token=?6 AND run_status='running'")
        .bind(status).bind(current_unix_ms() as i64).bind(detail).bind(&claim.schedule.instance_id)
        .bind(claim.schedule.action.as_str()).bind(&claim.run_token).execute(&pool).await;
    pool.close().await;
    if result?.rows_affected() != 1 {
        return Err(MaintenanceScheduleError::Invalid(
            "Daily maintenance result no longer owns its persisted receipt.".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "maintenance_schedule_tests.rs"]
mod tests;
