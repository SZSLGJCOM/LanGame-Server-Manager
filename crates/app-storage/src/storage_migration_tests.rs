//! These tests exclusively own UUID fixtures; none resolve default/PLAP storage.
use super::*;
use std::fs;

struct Fixture {
    root: PathBuf,
    paths: StoragePaths,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("lgsm-migration-commit-{}", uuid::Uuid::new_v4()));
        let defaults = StoragePaths::default();
        let state = root.join("state");
        let instances = root.join("instances");
        let paths = StoragePaths {
            app_data_root: state.clone(),
            database_path: state.join("db/lgs.db"),
            settings_path: state.join("settings.json"),
            logs_root: state.join("logs"),
            modules_root: defaults.modules_root,
            migrations_root: defaults.migrations_root,
            instances_root: instances.clone(),
            archives_root: instances.join(".trash"),
            games_root: root.join("games"),
            steamcmd_root: root.join("steamcmd"),
        };
        crate::bootstrap_storage_with_paths(paths.clone()).unwrap();
        Self { root, paths }
    }
    fn next(&self) -> AppSettings {
        let mut settings = self.paths.settings();
        settings.servers_root = self
            .root
            .join("moved-instances")
            .to_string_lossy()
            .into_owned();
        settings.archives_root = self
            .root
            .join("moved-instances/.trash")
            .to_string_lossy()
            .into_owned();
        settings
    }
    fn write(&self, relative: &str, bytes: &[u8]) {
        let path = self.paths.instances_root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    async fn preview(&self) -> PreparedMigration {
        preview(
            &self.paths,
            self.next(),
            uuid::Uuid::new_v4().to_string(),
            vec![],
            Arc::new(AtomicBool::new(false)),
            Arc::new(|_| {}),
        )
        .await
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(
            self.root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("lgsm-migration-commit-")
        );
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[tokio::test]
async fn migration_replaces_the_whole_target_and_removes_the_old_root() {
    let fixture = Fixture::new();
    fixture.write("alpha/saves/world.bin", b"world");
    fixture.write("alpha/backups/world.bin", b"backup world");
    fixture.write(".langame/locks/instance-inventory.lock", b"");
    let target = PathBuf::from(fixture.next().servers_root);
    fs::create_dir_all(target.join("alpha/saves")).unwrap();
    fs::write(target.join("alpha/saves/world.bin"), b"world").unwrap();
    fs::write(target.join("existing-target.txt"), b"existing").unwrap();
    fs::write(target.join(".trash"), b"old target archive name is a file").unwrap();
    let prepared = fixture.preview().await;
    assert_eq!(prepared.status().reused_files, 0);
    assert_eq!(prepared.status().phase, Phase::Ready);
    assert!(
        fixture
            .paths
            .instances_root
            .join("alpha/saves/world.bin")
            .exists()
    );
    let status = execute(
        &fixture.paths,
        prepared,
        Arc::new(AtomicBool::new(false)),
        Arc::new(|_| {}),
    )
    .await
    .unwrap();
    assert_eq!(status.phase, Phase::Completed);
    assert!(
        status.cleanup_warnings.is_empty(),
        "{:?}",
        status.cleanup_warnings
    );
    assert_eq!(status.preserved_destinations.len(), 1);
    let backup = PathBuf::from(&status.preserved_destinations[0].backup_path);
    assert_eq!(
        fs::read(backup.join("existing-target.txt")).unwrap(),
        b"existing"
    );
    assert_eq!(
        fs::read(backup.join(".trash")).unwrap(),
        b"old target archive name is a file"
    );
    assert!(!target.join("existing-target.txt").exists());
    assert!(target.join(".trash").is_dir());
    assert!(
        target
            .join(".langame/locks/instance-inventory.lock")
            .is_file()
    );
    assert_eq!(
        fs::read(target.join("alpha/saves/world.bin")).unwrap(),
        b"world"
    );
    assert_eq!(
        fs::read(target.join("alpha/backups/world.bin")).unwrap(),
        b"backup world"
    );
    assert!(!fixture.paths.instances_root.exists());
    let reboot = crate::bootstrap_storage_with_paths(fixture.paths.clone()).unwrap();
    assert_eq!(reboot.settings.servers_root, target.to_string_lossy());
    recover_pending(&fixture.paths).unwrap();
    assert_eq!(
        read_status(&fixture.paths).unwrap().unwrap().phase,
        Phase::Completed
    );
}

#[tokio::test]
async fn a_stale_destination_is_rechecked_then_a_new_choice_replaces_its_contents() {
    let fixture = Fixture::new();
    assert!(program_roots(&fixture.paths).await.unwrap().is_empty());
    assert!(!fixture.paths.database_path.exists());
    fixture.write("world.bin", b"source");
    let prepared = fixture.preview().await;
    let target = PathBuf::from(fixture.next().servers_root);
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("world.bin"), b"different target").unwrap();
    assert!(
        execute(
            &fixture.paths,
            prepared,
            Arc::new(AtomicBool::new(false)),
            Arc::new(|_| {})
        )
        .await
        .is_err()
    );
    let fresh = fixture.preview().await;
    assert_eq!(fresh.status().conflict_count, 0);
    assert!(read_prepared(&fixture.paths, &fresh.status().operation_id).is_ok());
    assert_eq!(
        fs::read(fixture.paths.instances_root.join("world.bin")).unwrap(),
        b"source"
    );
    assert_eq!(
        fs::read(target.join("world.bin")).unwrap(),
        b"different target"
    );
    assert!(!fixture.paths.settings_path.exists());
    let completed = execute(
        &fixture.paths,
        fresh,
        Arc::new(AtomicBool::new(false)),
        Arc::new(|_| {}),
    )
    .await
    .unwrap();
    assert!(completed.cleanup_warnings.is_empty());
    assert_eq!(fs::read(target.join("world.bin")).unwrap(), b"source");
    assert_eq!(
        fs::read(Path::new(&completed.preserved_destinations[0].backup_path).join("world.bin"))
            .unwrap(),
        b"different target"
    );
    assert!(!fixture.paths.instances_root.exists());
}

#[tokio::test]
async fn cancelled_copy_keeps_settings_and_sources_and_records_terminal_cancellation() {
    let fixture = Fixture::new();
    fixture.write("world.bin", b"world");
    let prepared = fixture.preview().await;
    let cancellation = Arc::new(AtomicBool::new(false));
    let stop = cancellation.clone();
    let progress: Progress = Arc::new(move |status| {
        if status.phase == Phase::Copying {
            stop.store(true, Ordering::Release);
        }
    });
    assert!(matches!(
        execute(&fixture.paths, prepared, cancellation, progress).await,
        Err(StorageError::StorageMigrationCancelled)
    ));
    assert!(fixture.paths.instances_root.join("world.bin").exists());
    assert!(!fixture.paths.settings_path.exists());
    assert_eq!(
        read_status(&fixture.paths).unwrap().unwrap().phase,
        Phase::Cancelled
    );
}

#[tokio::test]
async fn failed_settings_publication_preserves_source_and_replays_on_explicit_recovery() {
    let fixture = Fixture::new();
    fixture.write("world.bin", b"world");
    let destination = PathBuf::from(fixture.next().servers_root);
    fs::create_dir_all(&destination).unwrap();
    fs::write(destination.join("world.bin"), b"old destination world").unwrap();
    let prepared = fixture.preview().await;
    crate::atomic_file::fail_next_atomic_write_for_test(&fixture.paths.settings_path);
    assert!(
        execute(
            &fixture.paths,
            prepared,
            Arc::new(AtomicBool::new(false)),
            Arc::new(|_| {})
        )
        .await
        .is_err()
    );
    assert_eq!(
        read_status(&fixture.paths).unwrap().unwrap().phase,
        Phase::Applying
    );
    assert_eq!(
        fs::read(fixture.paths.instances_root.join("world.bin")).unwrap(),
        b"world"
    );
    assert!(!fixture.paths.settings_path.exists());
    let interrupted = read_status(&fixture.paths).unwrap().unwrap();
    assert_eq!(interrupted.preserved_destinations.len(), 1);
    let preserved = PathBuf::from(&interrupted.preserved_destinations[0].backup_path);
    assert_eq!(
        fs::read(preserved.join("world.bin")).unwrap(),
        b"old destination world"
    );
    let reboot = crate::bootstrap_storage_with_paths(fixture.paths.clone()).unwrap();
    assert_eq!(reboot.settings.servers_root, fixture.next().servers_root);
    assert_eq!(
        read_status(&fixture.paths).unwrap().unwrap().phase,
        Phase::Completed
    );
    assert!(!fixture.paths.instances_root.join("world.bin").exists());
    assert_eq!(
        fs::read(Path::new(&fixture.next().servers_root).join("world.bin")).unwrap(),
        b"world"
    );
    assert!(!fixture.paths.instances_root.exists());
    assert_eq!(
        fs::read(preserved.join("world.bin")).unwrap(),
        b"old destination world"
    );
}

#[tokio::test]
async fn a_target_cannot_take_over_another_live_storage_role() {
    let fixture = Fixture::new();
    fixture.write("world.bin", b"world");
    fs::write(
        fixture.paths.games_root.join("program.bin"),
        b"live program",
    )
    .unwrap();
    for target in [
        &fixture.paths.games_root,
        &fixture.paths.games_root.join("nested"),
    ] {
        let mut next = fixture.next();
        next.servers_root = target.to_string_lossy().into_owned();
        next.archives_root = target.join(".trash").to_string_lossy().into_owned();
        let error = preview(
            &fixture.paths,
            next,
            uuid::Uuid::new_v4().to_string(),
            vec![],
            Arc::new(AtomicBool::new(false)),
            Arc::new(|_| {}),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("仍在使用"), "{error}");
        assert_eq!(
            fs::read(fixture.paths.games_root.join("program.bin")).unwrap(),
            b"live program"
        );
        assert!(fixture.paths.instances_root.join("world.bin").exists());
        assert!(!fixture.paths.settings_path.exists());
    }
}

#[tokio::test]
async fn interrupted_copy_recovery_only_marks_failure_and_keeps_both_copies() {
    let fixture = Fixture::new();
    fixture.write("world.bin", b"world");
    let mut prepared = fixture.preview().await;
    files::copy_verified(&prepared.files, None, &mut |_| {}).unwrap();
    prepared.status.phase = Phase::Copying;
    journal::save(
        &fixture.paths,
        &journal::Journal {
            version: 1,
            prepared,
            metadata: None,
        },
    )
    .unwrap();
    recover_pending(&fixture.paths).unwrap();
    assert_eq!(
        read_status(&fixture.paths).unwrap().unwrap().phase,
        Phase::Failed
    );
    assert!(fixture.paths.instances_root.join("world.bin").exists());
    assert!(
        Path::new(&fixture.next().servers_root)
            .join("world.bin")
            .exists()
    );
    assert!(!fixture.paths.settings_path.exists());
}

#[test]
fn migration_file_lease_prevents_another_executor() {
    let fixture = Fixture::new();
    let lease = journal::acquire(&fixture.paths).unwrap();
    assert!(journal::acquire(&fixture.paths).is_err());
    drop(lease);
    assert!(journal::acquire(&fixture.paths).is_ok());
}

#[tokio::test]
async fn a_nested_archive_must_move_with_its_workspace_before_any_target_is_changed() {
    let fixture = Fixture::new();
    fixture.write("world.bin", b"world");
    let mut next = fixture.next();
    next.archives_root = fixture.paths.archives_root.to_string_lossy().into_owned();
    let mappings = path_mappings(&fixture.paths, &next).unwrap();
    assert_eq!(mappings[1].1, PathBuf::from(&next.servers_root));
    assert_eq!(
        map_path(&fixture.paths.archives_root, &mappings).unwrap(),
        normalize_path(&fixture.paths.archives_root).unwrap()
    );
    let error = preview(
        &fixture.paths,
        next.clone(),
        uuid::Uuid::new_v4().to_string(),
        vec![],
        Arc::new(AtomicBool::new(false)),
        Arc::new(|_| {}),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("未一起迁移"));
    assert!(fixture.paths.archives_root.is_dir());
    assert!(fixture.paths.instances_root.join("world.bin").is_file());
    assert!(!Path::new(&next.servers_root).exists());
    assert!(!fixture.paths.settings_path.exists());
}

#[test]
fn shared_source_roles_with_different_targets_are_rejected_without_guessing() {
    let fixture = Fixture::new();
    let mut ambiguous = fixture.paths.clone();
    ambiguous.games_root = ambiguous.instances_root.clone();
    let next = fixture.next();
    assert!(path_mappings(&ambiguous, &next).is_err());
    assert!(!Path::new(&next.servers_root).exists());
}

#[tokio::test]
async fn cleanup_recovery_rechecks_metadata_outside_the_copied_roots() {
    use crate::storage_migration_metadata::{FilePatch, MetadataPlan};
    let fixture = Fixture::new();
    fixture.write("world.bin", b"world");
    let binding = fixture.paths.games_root.join("binding.json");
    fs::write(&binding, b"old reference").unwrap();
    let mut prepared = fixture.preview().await;
    files::copy_verified(&prepared.files, None, &mut |_| {}).unwrap();
    crate::save_app_settings_with_paths(fixture.next(), &fixture.paths).unwrap();
    prepared.status.phase = Phase::Cleaning;
    let patch = FilePatch {
        path: binding.clone(),
        before: b"old reference".to_vec(),
        before_sha256: crate::instance_archive_store::digest(b"old reference"),
        after: b"new reference".to_vec(),
    };
    journal::save(
        &fixture.paths,
        &journal::Journal {
            version: 1,
            prepared,
            metadata: Some(MetadataPlan {
                files: vec![patch],
                rows: vec![],
            }),
        },
    )
    .unwrap();
    fs::write(&binding, b"external edit").unwrap();
    assert!(recover_pending(&fixture.paths).is_err());
    assert_eq!(fs::read(&binding).unwrap(), b"external edit");
    assert!(fixture.paths.instances_root.join("world.bin").exists());
    assert_eq!(
        read_status(&fixture.paths).unwrap().unwrap().phase,
        Phase::Cleaning
    );
    // The fixture owner resolves the conflict; the same durable operation resumes.
    fs::write(&binding, b"old reference").unwrap();
    recover_pending(&fixture.paths).unwrap();
    assert_eq!(fs::read(&binding).unwrap(), b"new reference");
    assert_eq!(
        read_status(&fixture.paths).unwrap().unwrap().phase,
        Phase::Completed
    );
    assert!(!fixture.paths.instances_root.join("world.bin").exists());
}

#[tokio::test]
async fn superseded_preview_cannot_execute_even_when_its_payload_is_unchanged() {
    let fixture = Fixture::new();
    fixture.write("world.bin", b"world");
    let previous = fixture.preview().await;
    let current = fixture.preview().await;
    assert_ne!(
        previous.status().operation_id,
        current.status().operation_id
    );
    assert!(
        execute(
            &fixture.paths,
            previous,
            Arc::new(AtomicBool::new(false)),
            Arc::new(|_| {})
        )
        .await
        .is_err()
    );
    assert_eq!(
        read_status(&fixture.paths).unwrap().unwrap().operation_id,
        current.status().operation_id
    );
    assert!(fixture.paths.instances_root.join("world.bin").exists());
    assert!(!Path::new(&fixture.next().servers_root).exists());
}

#[tokio::test]
async fn startup_keeps_a_ready_preview_inert_even_while_its_lease_is_held() {
    let fixture = Fixture::new();
    fixture.write("world.bin", b"world");
    let prepared = fixture.preview().await;
    let lease = journal::acquire(&fixture.paths).unwrap();
    let reboot = crate::bootstrap_storage_with_paths(fixture.paths.clone()).unwrap();
    assert_eq!(
        reboot.settings.servers_root,
        fixture.paths.settings().servers_root
    );
    assert_eq!(
        read_status(&fixture.paths).unwrap().unwrap().operation_id,
        prepared.status().operation_id
    );
    assert_eq!(
        read_status(&fixture.paths).unwrap().unwrap().phase,
        Phase::Ready
    );
    assert!(fixture.paths.instances_root.join("world.bin").exists());
    assert!(!Path::new(&fixture.next().servers_root).exists());
    drop(lease);
}

#[tokio::test]
async fn publication_is_reported_non_cancellable_before_settings_and_cleanup() {
    let fixture = Fixture::new();
    fixture.write("world.bin", b"world");
    let prepared = fixture.preview().await;
    let phases = Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed = phases.clone();
    let settings = fixture.paths.settings_path.clone();
    let source = fixture.paths.instances_root.join("world.bin");
    let progress: Progress = Arc::new(move |status| {
        if status.phase == Phase::Applying {
            assert!(!status.can_cancel);
            assert!(!settings.exists());
            assert!(source.exists());
        }
        observed.lock().unwrap().push(status.phase);
    });
    execute(
        &fixture.paths,
        prepared,
        Arc::new(AtomicBool::new(false)),
        progress,
    )
    .await
    .unwrap();
    let phases = phases.lock().unwrap();
    let applying = phases
        .iter()
        .position(|phase| *phase == Phase::Applying)
        .unwrap();
    let cleaning = phases
        .iter()
        .position(|phase| *phase == Phase::Cleaning)
        .unwrap();
    let completed = phases
        .iter()
        .position(|phase| *phase == Phase::Completed)
        .unwrap();
    assert!(applying < cleaning && cleaning < completed);
}
