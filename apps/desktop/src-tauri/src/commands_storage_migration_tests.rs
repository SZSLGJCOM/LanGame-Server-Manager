use super::*;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[path = "commands_storage_migration_failure_tests.rs"]
mod failure_tests;

struct Fixture {
    root: PathBuf,
    storage: app_storage::StorageBootstrap,
}

impl Fixture {
    async fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("lgsm-migration-command-{}", uuid::Uuid::new_v4()));
        let defaults = StoragePaths::default();
        let state = root.join("state");
        let instances = root.join("instances");
        let paths = StoragePaths {
            app_data_root: state.clone(),
            settings_path: state.join("settings.json"),
            database_path: state.join("db/lgs.db"),
            logs_root: state.join("logs"),
            modules_root: defaults.modules_root,
            migrations_root: defaults.migrations_root,
            instances_root: instances.clone(),
            archives_root: instances.join(".trash"),
            games_root: root.join("games"),
            steamcmd_root: root.join("steamcmd"),
        };
        let mut storage = app_storage::bootstrap_storage_with_paths(paths).unwrap();
        storage.storage_status = app_storage::initialize_database(&storage.paths)
            .await
            .unwrap();
        Self { root, storage }
    }

    fn input(&self) -> AppPathSettingsInput {
        AppPathSettingsInput {
            servers_root: self
                .root
                .join("moved-instances")
                .to_string_lossy()
                .into_owned(),
            archives_root: String::new(),
            games_root: self.storage.settings.games_root.clone(),
            steamcmd_root: self.storage.settings.steamcmd_root.clone(),
        }
    }

    fn app(&self) -> tauri::App<tauri::test::MockRuntime> {
        tauri::test::mock_builder()
            .manage(DesktopState::from_storage(&self.storage))
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap()
    }

    fn logs(&self) -> Vec<serde_json::Value> {
        crate::desktop_app_log::recent_lines(
            &self.storage.paths.app_log_path(),
            None,
            100,
            128 * 1024,
        )
        .unwrap()
        .into_iter()
        .map(|line| serde_json::from_str(&line).unwrap())
        .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(
            self.root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("lgsm-migration-command-")
        );
        fs::remove_dir_all(&self.root).unwrap();
    }
}

async fn settled(app: &tauri::App<tauri::test::MockRuntime>) -> StorageMigrationStatus {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let status = read_app_storage_migration(app.state::<DesktopState>())
                .await
                .unwrap()
                .unwrap();
            if !status.is_active() {
                return status;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("isolated migration command must settle")
}

pub(super) fn tree_snapshot(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    let mut result = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path).unwrap();
        assert!(!metadata.file_type().is_symlink(), "fixture must be plain");
        let relative = path.strip_prefix(root).unwrap().to_path_buf();
        if metadata.is_dir() {
            result.insert(relative, None);
            pending.extend(
                fs::read_dir(path)
                    .unwrap()
                    .map(|entry| entry.unwrap().path()),
            );
        } else {
            assert!(metadata.is_file());
            result.insert(relative, Some(fs::read(path).unwrap()));
        }
    }
    result
}

pub(super) fn preserved_backup(
    status: &StorageMigrationStatus,
    target: &Path,
    fixture_root: &Path,
) -> PathBuf {
    let target = fs::canonicalize(target).unwrap();
    let matches: Vec<_> = status
        .preserved_destinations
        .iter()
        .filter(|entry| fs::canonicalize(&entry.original_path).unwrap() == target)
        .collect();
    assert_eq!(matches.len(), 1, "exactly one backup per outer target");
    let backup = fs::canonicalize(&matches[0].backup_path).unwrap();
    assert!(backup.starts_with(fs::canonicalize(fixture_root).unwrap()));
    assert_eq!(backup.components().next(), target.components().next());
    assert!(!backup.starts_with(&target) && !target.starts_with(&backup));
    backup
}

#[tokio::test(flavor = "current_thread")]
async fn mock_commands_preview_execute_and_read_publish_paths_before_completed() {
    let _serial = crate::commands::tests::command_smoke_lock().lock().await;
    let fixture = Fixture::new().await;
    let app = fixture.app();
    let source = fixture.storage.paths.instances_root.join("world.dat");
    fs::write(&source, b"world survives native commands").unwrap();
    let lock = ".langame/locks/instance-settings/idle.lock";
    let source_lock = fixture.storage.paths.instances_root.join(lock);
    fs::create_dir_all(source_lock.parent().unwrap()).unwrap();
    fs::write(&source_lock, []).unwrap();
    let expected_before =
        serde_json::to_value(&*app.state::<DesktopState>().app_state.read().unwrap()).unwrap();
    assert!(
        read_app_storage_migration(app.state::<DesktopState>())
            .await
            .unwrap()
            .is_none()
    );
    let started = preview_app_storage_migration(
        app.handle().clone(),
        app.state::<DesktopState>(),
        fixture.input(),
    )
    .await
    .unwrap();
    assert_eq!(started.phase, Phase::Checking);
    let ready = settled(&app).await;
    assert_eq!(ready.phase, Phase::Ready, "{:?}", ready.error);
    assert_eq!(ready.operation_id, started.operation_id);
    assert_eq!(ready.conflict_count, 0);
    assert!(source.exists());
    let executing = execute_app_storage_migration(
        app.handle().clone(),
        app.state::<DesktopState>(),
        ready.operation_id.clone(),
    )
    .await
    .unwrap();
    assert_eq!(executing.phase, Phase::Verifying);
    let repeated = execute_app_storage_migration(
        app.handle().clone(),
        app.state::<DesktopState>(),
        ready.operation_id.clone(),
    )
    .await
    .unwrap();
    assert_eq!(repeated.operation_id, ready.operation_id);
    let completed = settled(&app).await;
    assert_eq!(completed.phase, Phase::Completed, "{:?}", completed.error);
    assert!(!completed.recovery_required);
    assert!(
        completed.cleanup_warnings.is_empty(),
        "{:?}",
        completed.cleanup_warnings
    );
    let target = fixture.root.join("moved-instances");
    assert_eq!(
        fs::read(target.join("world.dat")).unwrap(),
        b"world survives native commands"
    );
    assert!(!source.exists());
    assert!(!fixture.storage.paths.instances_root.exists());
    assert!(!fixture.storage.paths.archives_root.exists());
    assert_eq!(fs::read(target.join(lock)).unwrap(), Vec::<u8>::new());
    assert!(completed.preserved_destinations.is_empty());
    let mut expected_after = expected_before;
    expected_after["settings"]["servers_root"] = serde_json::json!(target);
    expected_after["settings"]["archives_root"] = serde_json::json!(target.join(".trash"));
    assert_eq!(
        serde_json::to_value(&*app.state::<DesktopState>().app_state.read().unwrap()).unwrap(),
        expected_after
    );
    let persisted =
        app_storage::bootstrap_storage_with_paths(fixture.storage.paths.clone()).unwrap();
    assert_eq!(persisted.paths.instances_root, target);
    assert_eq!(persisted.paths.archives_root, target.join(".trash"));
    assert!(!fixture.storage.paths.instances_root.exists());
    let observed = execute_app_storage_migration(
        app.handle().clone(),
        app.state::<DesktopState>(),
        completed.operation_id.clone(),
    )
    .await
    .unwrap();
    assert_eq!(observed.phase, Phase::Completed);
    let logs = fixture.logs();
    for (action, phase) in [
        ("storage.migration.request", "checking"),
        ("storage.migration.ready", "ready"),
        ("storage.migration.completed", "completed"),
    ] {
        let matching: Vec<_> = logs
            .iter()
            .filter(|entry| {
                entry["action"] == action
                    && entry["context"]["operation_id"] == completed.operation_id
            })
            .collect();
        assert_eq!(
            matching.len(),
            1,
            "one structured {action} record per operation"
        );
        let entry = matching[0];
        assert!(entry["ts_unix_ms"].as_u64().is_some());
        assert_eq!(entry["context"]["phase"], phase);
        assert_eq!(
            entry["context"]["servers_root"],
            fixture.input().servers_root
        );
        assert_eq!(entry["context"]["needs_recovery"], false);
    }
    assert!(
        app.state::<DesktopState>()
            .begin_storage_context_operation("after migration fixture")
            .is_ok()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn mock_existing_target_is_preserved_whole_and_replaced_by_source() {
    let _serial = crate::commands::tests::command_smoke_lock().lock().await;
    let fixture = Fixture::new().await;
    let app = fixture.app();
    let source = &fixture.storage.paths.instances_root;
    let target = fixture.root.join("moved-instances");
    fs::create_dir_all(source.join("source-empty")).unwrap();
    fs::create_dir_all(target.join("destination-empty")).unwrap();
    fs::write(source.join("world.dat"), b"original source world").unwrap();
    fs::write(target.join("world.dat"), b"different destination world").unwrap();
    for root in [source, &target] {
        fs::write(root.join("identical.bin"), b"same payload in both trees").unwrap();
    }
    fs::write(source.join("source-only.bin"), b"source only").unwrap();
    fs::write(target.join("extra.bin"), b"destination only").unwrap();
    let before_source = tree_snapshot(source);
    let before_target = tree_snapshot(&target);
    let settings_path = &fixture.storage.paths.settings_path;
    let before_disk = settings_path
        .exists()
        .then(|| fs::read(settings_path).unwrap());
    let before_state =
        serde_json::to_value(&*app.state::<DesktopState>().app_state.read().unwrap()).unwrap();
    let started = preview_app_storage_migration(
        app.handle().clone(),
        app.state::<DesktopState>(),
        fixture.input(),
    )
    .await
    .unwrap();
    let ready = settled(&app).await;
    assert_eq!(ready.phase, Phase::Ready, "{:?}", ready.error);
    assert_eq!(ready.operation_id, started.operation_id);
    assert_eq!(ready.conflict_count, 0);
    assert_eq!(tree_snapshot(source), before_source);
    assert_eq!(tree_snapshot(&target), before_target);
    assert_eq!(
        settings_path
            .exists()
            .then(|| fs::read(settings_path).unwrap()),
        before_disk
    );
    assert_eq!(
        serde_json::to_value(&*app.state::<DesktopState>().app_state.read().unwrap()).unwrap(),
        before_state
    );
    execute_app_storage_migration(
        app.handle().clone(),
        app.state::<DesktopState>(),
        ready.operation_id.clone(),
    )
    .await
    .unwrap();
    let completed = settled(&app).await;
    assert_eq!(completed.phase, Phase::Completed, "{:?}", completed.error);
    assert!(completed.cleanup_warnings.is_empty());
    assert_eq!(completed.conflict_count, 0);
    assert_eq!(completed.preserved_destinations.len(), 1);
    let backup = preserved_backup(&completed, &target, &fixture.root);
    assert_eq!(tree_snapshot(&backup), before_target);
    assert_eq!(tree_snapshot(&target), before_source);
    assert!(!source.exists());
    assert!(!fixture.storage.paths.archives_root.exists());
    let reopened =
        app_storage::bootstrap_storage_with_paths(fixture.storage.paths.clone()).unwrap();
    assert_eq!(reopened.paths.instances_root, target);
    assert!(!source.exists(), "refresh must not recreate the old root");
    assert!(fixture.logs().iter().any(|entry| {
        entry["action"] == "storage.migration.ready"
            && entry["context"]["operation_id"] == ready.operation_id
            && entry["context"]["conflict_count"] == 0
    }));
}

#[tokio::test(flavor = "current_thread")]
async fn mock_changed_source_rejects_stale_preview_and_preserves_target_and_settings() {
    let fixture = Fixture::new().await;
    let app = fixture.app();
    let source = fixture.storage.paths.instances_root.join("world.dat");
    let target = fixture.root.join("moved-instances");
    fs::create_dir_all(&target).unwrap();
    fs::write(&source, b"previewed source").unwrap();
    fs::write(target.join("world.dat"), b"existing target").unwrap();
    let target_before = tree_snapshot(&target);
    let settings_path = &fixture.storage.paths.settings_path;
    let settings_before = settings_path
        .exists()
        .then(|| fs::read(settings_path).unwrap());
    let state_before =
        serde_json::to_value(&*app.state::<DesktopState>().app_state.read().unwrap()).unwrap();
    preview_app_storage_migration(
        app.handle().clone(),
        app.state::<DesktopState>(),
        fixture.input(),
    )
    .await
    .unwrap();
    let ready = settled(&app).await;
    assert_eq!(ready.phase, Phase::Ready, "{:?}", ready.error);
    assert_eq!(ready.conflict_count, 0);
    fs::write(&source, b"source changed after preview").unwrap();
    execute_app_storage_migration(
        app.handle().clone(),
        app.state::<DesktopState>(),
        ready.operation_id,
    )
    .await
    .unwrap();
    let failed = settled(&app).await;
    assert_eq!(failed.phase, Phase::Failed);
    assert!(!failed.recovery_required);
    let error = failed.error.unwrap();
    assert!(
        error.contains("Source") && error.contains("changed"),
        "{error}"
    );
    assert_eq!(fs::read(&source).unwrap(), b"source changed after preview");
    assert_eq!(tree_snapshot(&target), target_before);
    assert_eq!(
        settings_path
            .exists()
            .then(|| fs::read(settings_path).unwrap()),
        settings_before
    );
    assert_eq!(
        serde_json::to_value(&*app.state::<DesktopState>().app_state.read().unwrap()).unwrap(),
        state_before
    );
    assert!(
        app.state::<DesktopState>()
            .begin_storage_context_operation("stale preview released its guard")
            .is_ok()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn mock_command_cancel_requests_do_not_fake_a_terminal_result() {
    let fixture = Fixture::new().await;
    let app = fixture.app();
    let state = app.state::<DesktopState>();
    let initial = core::initial_status(uuid::Uuid::new_v4().to_string(), fixture.input());
    let (run, _) = match state
        .storage_migration
        .begin_preview(initial.clone())
        .unwrap()
    {
        Claim::Started(run, status) => (run, status),
        Claim::Observe(_) => panic!("isolated tracker must admit its first run"),
    };
    let transition = state.begin_storage_context_transition().unwrap();
    let cancelled = cancel_app_storage_migration(app.state::<DesktopState>(), run.id.clone())
        .await
        .unwrap();
    assert_eq!(cancelled.phase, Phase::Checking);
    assert!(!cancelled.can_cancel);
    assert!(run.cancellation.load(std::sync::atomic::Ordering::Acquire));
    assert!(
        state
            .begin_storage_context_operation("must wait for worker")
            .is_err()
    );
    let mut finished = initial;
    finished.phase = Phase::Cancelled;
    drop(transition);
    state
        .storage_migration
        .finish(&run, finished, false)
        .unwrap();
    assert_eq!(
        read_app_storage_migration(app.state::<DesktopState>())
            .await
            .unwrap()
            .unwrap()
            .phase,
        Phase::Cancelled
    );
    assert!(
        state
            .begin_storage_context_operation("worker finished")
            .is_ok()
    );
}
