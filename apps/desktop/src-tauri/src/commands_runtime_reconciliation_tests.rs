use super::*;
use app_core::{CreateInstanceInput, InstanceStatus, InstanceSummary};
use app_modules::discover_modules;
use app_storage::{
    StartedInstanceProcess, StoragePaths, create_instance, initialize_database,
    mark_instance_process_started_with_identity, read_instance_details,
    read_instance_runtime_overview, sync_modules,
};
use std::fs;
use std::path::{Path, PathBuf};

struct Fixture {
    root: PathBuf,
    storage: StorageBootstrap,
    summary: InstanceSummary,
}

impl Fixture {
    async fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lg-exit-reconcile-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("db")).unwrap();
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let paths = StoragePaths {
            app_data_root: root.clone(),
            settings_path: root.join("settings.json"),
            database_path: root.join("db/lgs.db"),
            logs_root: root.join("logs"),
            modules_root: workspace.join("modules"),
            migrations_root: workspace.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances").join(".trash"),
        };
        initialize_database(&paths).await.unwrap();
        let descriptors = discover_modules(&paths.modules_root).unwrap();
        let descriptor = descriptors
            .iter()
            .find(|module| module.summary.id == "dontstarve")
            .unwrap();
        sync_modules(&paths, std::slice::from_ref(descriptor))
            .await
            .unwrap();
        crate::commands::tests::prepare_fake_registered_program(&paths, descriptor)
            .await
            .unwrap();
        let created = create_instance(
            &paths,
            descriptor,
            CreateInstanceInput {
                name: String::from("Exit reconciliation fixture"),
                module_id: String::from("dontstarve"),
            },
        )
        .await
        .unwrap();
        Self {
            root,
            summary: created.summary,
            storage: StorageBootstrap {
                settings: paths.settings(),
                storage_status: paths.probe_status(),
                paths,
            },
        }
    }

    async fn process(&self, session: Option<&str>, key: &str) -> ExitedManagedProcess {
        let process = mark_instance_process_started_with_identity(
            &self.storage.paths,
            &StartedInstanceProcess {
                instance_id: &self.summary.id,
                session_id: session,
                process_key: key,
                display_name: key,
                pid: 1234,
                log_path: "isolated-exit-fixture.log",
                is_primary: key == "master",
            },
            None,
        )
        .await
        .unwrap();
        ExitedManagedProcess {
            summary: self.summary.clone(),
            session_id: session.map(String::from),
            run_id: process.run_id,
            process_key: key.into(),
            display_name: key.into(),
            pid: 1234,
            log_path: "isolated-exit-fixture.log".into(),
            is_primary: key == "master",
            exit_code: Some(7),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("remove isolated exit reconciliation fixture");
    }
}

#[tokio::test]
async fn runtime_reconciliation_persists_the_current_exit() {
    let fixture = Fixture::new().await;
    let exited = fixture.process(Some("current"), "master").await;
    assert!(
        persist_current_exit(&fixture.storage, &exited, true)
            .await
            .unwrap()
    );
    assert!(
        list_active_instance_runs(&fixture.storage.paths)
            .await
            .unwrap()
            .is_empty()
    );
    let overview = read_instance_runtime_overview(&fixture.storage.paths, &fixture.summary.id)
        .await
        .unwrap();
    let process = &overview.recent_runs[0].processes[0];
    assert_eq!(process.run_id, exited.run_id);
    assert_eq!(process.exit_code, Some(7));
    assert!(process.crash_flag);
}

#[tokio::test]
async fn runtime_reconciliation_does_not_rewrite_an_already_persisted_exit() {
    let fixture = Fixture::new().await;
    let exited = fixture.process(Some("current"), "master").await;
    assert!(
        persist_current_exit(&fixture.storage, &exited, true)
            .await
            .unwrap()
    );
    let before = read_instance_runtime_overview(&fixture.storage.paths, &fixture.summary.id)
        .await
        .unwrap();
    assert!(
        !persist_current_exit(&fixture.storage, &exited, true)
            .await
            .unwrap()
    );
    let mut conflicting_replay = exited.clone();
    conflicting_replay.exit_code = Some(0);
    assert!(
        !persist_current_exit(&fixture.storage, &conflicting_replay, false)
            .await
            .unwrap()
    );
    let after = read_instance_runtime_overview(&fixture.storage.paths, &fixture.summary.id)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(after.recent_runs).unwrap(),
        serde_json::to_value(before.recent_runs).unwrap()
    );
}

#[tokio::test]
async fn runtime_reconciliation_old_exit_does_not_overwrite_a_new_session() {
    let fixture = Fixture::new().await;
    let old = fixture.process(Some("old"), "master").await;
    let current = fixture.process(Some("replacement"), "master").await;
    assert!(
        !persist_current_exit(&fixture.storage, &old, true)
            .await
            .unwrap()
    );
    let active = list_active_instance_runs(&fixture.storage.paths)
        .await
        .unwrap();
    assert!(active.iter().any(|run| run.run_id == old.run_id));
    assert!(active.iter().any(|run| run.run_id == current.run_id));
    let details = read_instance_details(&fixture.storage.paths, &fixture.summary.id)
        .await
        .unwrap();
    assert!(matches!(details.summary.status, InstanceStatus::Running));
    assert_eq!(
        details.active_run.unwrap().session_id.as_deref(),
        Some("replacement")
    );
}

#[tokio::test]
async fn runtime_reconciliation_allows_other_shards_in_the_same_session() {
    let fixture = Fixture::new().await;
    let master = fixture.process(Some("shared"), "master").await;
    let caves = fixture.process(Some("shared"), "caves").await;
    assert!(
        persist_current_exit(&fixture.storage, &caves, true)
            .await
            .unwrap()
    );
    let active = list_active_instance_runs(&fixture.storage.paths)
        .await
        .unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].run_id, master.run_id);
}

#[tokio::test]
async fn runtime_reconciliation_without_a_session_uses_exact_run_identity() {
    let fixture = Fixture::new().await;
    let first = fixture.process(None, "master").await;
    assert!(
        persist_current_exit(&fixture.storage, &first, true)
            .await
            .unwrap()
    );
    let old = fixture.process(None, "master").await;
    let current = fixture.process(None, "master").await;
    assert!(
        !persist_current_exit(&fixture.storage, &old, true)
            .await
            .unwrap()
    );
    let active = list_active_instance_runs(&fixture.storage.paths)
        .await
        .unwrap();
    assert!(active.iter().any(|run| run.run_id == current.run_id));
}
