use super::*;
use std::path::PathBuf;

struct Fixture {
    root: PathBuf,
    paths: StoragePaths,
}
impl Fixture {
    async fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lg-daily-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let paths = StoragePaths {
            app_data_root: root.clone(),
            database_path: root.join("fixture.db"),
            ..StoragePaths::default()
        };
        let pool = connect_pool(&paths).await.unwrap();
        sqlx::query("INSERT INTO modules(id,name,version) VALUES('fixture','fixture','1')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO instances(id,name,module_id,data_path,config_path,logs_path,saves_path) VALUES('server','server','fixture','data','config','logs','saves')").execute(&pool).await.unwrap();
        pool.close().await;
        Self { root, paths }
    }
    async fn enable(&self, action: DailyMaintenanceAction) -> DailyMaintenanceSchedule {
        update_daily_maintenance_schedule(
            &self.paths,
            &UpdateDailyMaintenanceScheduleInput {
                instance_id: "server".into(),
                action,
                enabled: true,
                time_local: "04:00".into(),
                expected_revision: 0,
            },
        )
        .await
        .unwrap()
    }
    async fn claim(&self, date: &str, times: &[&str], owner: &str) -> Vec<ClaimedDailyMaintenance> {
        claim_due_daily_maintenance(
            &self.paths,
            date,
            &times.iter().map(|v| (*v).to_owned()).collect::<Vec<_>>(),
            owner,
        )
        .await
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

#[tokio::test]
async fn daily_maintenance_defaults_off_validates_and_preserves_independent_policies() {
    let fixture = Fixture::new().await;
    let initial =
        read_daily_maintenance_schedule(&fixture.paths, "server", DailyMaintenanceAction::Restart)
            .await
            .unwrap();
    assert!(!initial.enabled);
    assert!(
        fixture
            .claim("2026-10-18", &["04:00"], "owner")
            .await
            .is_empty()
    );
    fixture.enable(DailyMaintenanceAction::Backup).await;
    assert_eq!(
        read_daily_maintenance_schedule(&fixture.paths, "server", DailyMaintenanceAction::Restart)
            .await
            .unwrap(),
        initial
    );
    let mut input = UpdateDailyMaintenanceScheduleInput {
        instance_id: "server".into(),
        action: DailyMaintenanceAction::Backup,
        enabled: false,
        time_local: "04:00".into(),
        expected_revision: 0,
    };
    assert!(
        update_daily_maintenance_schedule(&fixture.paths, &input)
            .await
            .unwrap_err()
            .to_string()
            .contains("changed elsewhere")
    );
    input.expected_revision = 1;
    input.time_local = "24:00".into();
    assert!(
        update_daily_maintenance_schedule(&fixture.paths, &input)
            .await
            .is_err()
    );
    let saved =
        read_daily_maintenance_schedule(&fixture.paths, "server", DailyMaintenanceAction::Backup)
            .await
            .unwrap();
    assert!(saved.enabled);
    assert_eq!(saved.revision, 1);
    assert_eq!(saved.time_local, "04:00");
}

#[tokio::test]
async fn daily_maintenance_claims_backup_before_restart_once_across_concurrent_scans_and_clock_rollback()
 {
    let fixture = Fixture::new().await;
    fixture.enable(DailyMaintenanceAction::Restart).await;
    fixture.enable(DailyMaintenanceAction::Backup).await;
    assert!(
        fixture
            .claim("2026-10-18", &["03:59"], "owner")
            .await
            .is_empty()
    );
    let (a, b) = tokio::join!(
        fixture.claim("2026-10-18", &["04:00"], "owner"),
        fixture.claim("2026-10-18", &["04:00"], "owner")
    );
    assert_eq!(a.len() + b.len(), 2);
    let claims = if a.is_empty() { b } else { a };
    assert_eq!(
        claims.iter().map(|c| c.schedule.action).collect::<Vec<_>>(),
        [
            DailyMaintenanceAction::Backup,
            DailyMaintenanceAction::Restart
        ]
    );
    complete_daily_maintenance(&fixture.paths, &claims[0], "succeeded", "fixture backup")
        .await
        .unwrap();
    complete_daily_maintenance(&fixture.paths, &claims[1], "failed", "fixture stop failed")
        .await
        .unwrap();
    assert!(
        complete_daily_maintenance(&fixture.paths, &claims[0], "failed", "duplicate callback")
            .await
            .is_err()
    );
    assert!(
        fixture
            .claim("2026-10-18", &["04:00"], "owner")
            .await
            .is_empty()
    );
    assert!(
        fixture
            .claim("2026-10-17", &["04:00"], "owner")
            .await
            .is_empty()
    );
    assert_eq!(
        fixture.claim("2026-10-19", &["04:00"], "owner").await.len(),
        2
    );
}

#[tokio::test]
async fn daily_maintenance_process_restart_retains_unknown_result_without_repeating_occurrence() {
    let fixture = Fixture::new().await;
    fixture.enable(DailyMaintenanceAction::Restart).await;
    let claimed = fixture.claim("2026-10-18", &["04:00"], "old-manager").await;
    interrupt_abandoned_daily_maintenance(&fixture.paths, "old-manager")
        .await
        .unwrap();
    let active =
        read_daily_maintenance_schedule(&fixture.paths, "server", DailyMaintenanceAction::Restart)
            .await
            .unwrap();
    assert_eq!(active.last_run.unwrap().status, "running");
    interrupt_abandoned_daily_maintenance(&fixture.paths, "new-manager")
        .await
        .unwrap();
    let recovered =
        read_daily_maintenance_schedule(&fixture.paths, "server", DailyMaintenanceAction::Restart)
            .await
            .unwrap();
    let run = recovered.last_run.unwrap();
    assert_eq!(run.status, "interrupted");
    assert!(run.completed_at_unix_ms.is_some());
    assert!(
        fixture
            .claim("2026-10-18", &["04:00"], "new-manager")
            .await
            .is_empty()
    );
    assert!(
        complete_daily_maintenance(
            &fixture.paths,
            &claimed[0],
            "succeeded",
            "late previous manager"
        )
        .await
        .is_err()
    );
}
