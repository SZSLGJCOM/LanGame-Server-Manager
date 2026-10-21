use super::*;
use app_storage::StoragePaths;
use std::future::{Future, poll_fn};
use std::task::Poll;

struct Fixture {
    root: PathBuf,
    storage: StorageBootstrap,
    state: DesktopState,
}

impl Fixture {
    async fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("lg-maintenance-gate-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let paths = StoragePaths {
            app_data_root: root.clone(),
            settings_path: root.join("settings.json"),
            database_path: root.join("fixture.db"),
            logs_root: root.join("logs"),
            modules_root: root.join("modules"),
            migrations_root: root.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("archives"),
        };
        initialize_database(&paths).await.unwrap();
        let pool = sqlx::SqlitePool::connect_with(
            sqlx::sqlite::SqliteConnectOptions::new().filename(&paths.database_path),
        )
        .await
        .unwrap();
        sqlx::query("INSERT INTO modules(id,name,version) VALUES('fixture','fixture','1')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO instances(id,name,module_id,data_path,config_path,logs_path,saves_path) VALUES('server','server','fixture','data','config','logs','saves')")
            .execute(&pool).await.unwrap();
        pool.close().await;
        let storage = StorageBootstrap {
            settings: paths.settings(),
            storage_status: paths.probe_status(),
            paths,
        };
        let state = DesktopState::from_storage(&storage);
        Self {
            root,
            storage,
            state,
        }
    }

    async fn claim(&self, action: DailyMaintenanceAction) -> ClaimedDailyMaintenance {
        let schedule = schedules::update_daily_maintenance_schedule(
            &self.storage.paths,
            &UpdateDailyMaintenanceScheduleInput {
                instance_id: "server".into(),
                action,
                enabled: true,
                time_local: "04:00".into(),
                expected_revision: 0,
            },
        )
        .await
        .unwrap();
        ClaimedDailyMaintenance {
            schedule,
            run_token: "fixture".into(),
        }
    }

    fn worker(&self) -> crate::state::DailyMaintenanceWorker {
        let (date, time) = local_clock().unwrap();
        self.state
            .daily_maintenance
            .try_start(&date, &time)
            .unwrap()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[tokio::test]
async fn daily_maintenance_rechecks_disabled_and_edited_policies_after_waiting_for_the_instance_lock()
 {
    for action in [
        DailyMaintenanceAction::Backup,
        DailyMaintenanceAction::Restart,
    ] {
        for disabled in [true, false] {
            let fixture = Fixture::new().await;
            let claim = fixture.claim(action).await;
            let worker = fixture.worker();
            let existing_mutation = fixture.state.acquire_instance_mutation("server").await;
            let execution =
                acquire_claim_execution(&fixture.state, &fixture.storage, &claim, &worker);
            tokio::pin!(execution);
            poll_fn(|context| {
                assert!(
                    execution.as_mut().poll(context).is_pending(),
                    "The existing save must retain its instance lock"
                );
                Poll::Ready(())
            })
            .await;
            schedules::update_daily_maintenance_schedule(
                &fixture.storage.paths,
                &UpdateDailyMaintenanceScheduleInput {
                    instance_id: "server".into(),
                    action,
                    enabled: !disabled,
                    time_local: "05:00".into(),
                    expected_revision: claim.schedule.revision,
                },
            )
            .await
            .unwrap();
            drop(existing_mutation);
            assert!(
                matches!(execution.await, Err(MaintenanceOutcome::Skipped(detail)) if detail.contains("policy changed")),
                "Neither scheduled backup nor restart may use the policy captured before the lock wait"
            );
        }
    }
}

#[tokio::test]
async fn daily_maintenance_rechecks_the_observation_window_after_waiting_for_the_instance_lock() {
    for action in [
        DailyMaintenanceAction::Backup,
        DailyMaintenanceAction::Restart,
    ] {
        let fixture = Fixture::new().await;
        let claim = fixture.claim(action).await;
        let worker = fixture.worker();
        let existing_mutation = fixture.state.acquire_instance_mutation("server").await;
        let execution = acquire_claim_execution(&fixture.state, &fixture.storage, &claim, &worker);
        tokio::pin!(execution);
        poll_fn(|context| {
            assert!(execution.as_mut().poll(context).is_pending());
            Poll::Ready(())
        })
        .await;
        // The same generation invalidation is used for a changed local date
        // and a suspension gap; neither may replay a previously queued action.
        fixture
            .state
            .daily_maintenance
            .try_start("1900-01-01", "10:00")
            .unwrap();
        drop(existing_mutation);
        assert!(
            matches!(execution.await, Err(MaintenanceOutcome::Skipped(detail)) if detail.contains("suspended"))
        );
    }
}

#[tokio::test]
async fn daily_maintenance_execution_keeps_the_instance_lock_until_the_action_finishes() {
    let fixture = Fixture::new().await;
    let claim = fixture.claim(DailyMaintenanceAction::Backup).await;
    let worker = fixture.worker();
    let executing = acquire_claim_execution(&fixture.state, &fixture.storage, &claim, &worker)
        .await
        .unwrap();
    let later_mutation = fixture.state.acquire_instance_mutation("server");
    tokio::pin!(later_mutation);
    poll_fn(|context| {
        assert!(later_mutation.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(executing);
    drop(later_mutation.await);
}
