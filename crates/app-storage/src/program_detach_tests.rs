// Included by program_detach.rs to exercise durable publication boundaries.
use super::*;
use crate::program_runtime::prepare_shared_program_reference;
use crate::{
    GameInstallSyncRecord, ProgramInstallScope, bind_shared_install_to_instance,
    initialize_database, read_instance_program_install, read_library_program_install,
    register_instance_install, resolve_instance_runtime_root, sync_game_installs,
};
use sqlx::SqlitePool;
use std::sync::atomic::Ordering;

struct Fixture {
    root: PathBuf,
    paths: StoragePaths,
    pool: SqlitePool,
    descriptor: ModuleDescriptor,
    program: PathBuf,
    library_id: i64,
}

impl Fixture {
    async fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("langame-program-detach-{}", uuid::Uuid::new_v4()));
        let paths = StoragePaths {
            app_data_root: root.join("app-data"),
            settings_path: root.join("app-data/settings.json"),
            database_path: root.join("app-data/db/test.db"),
            logs_root: root.join("logs"),
            modules_root: root.join("modules"),
            migrations_root: root.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances").join(".trash"),
        };
        fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
        initialize_database(&paths).await.unwrap();
        let pool = connect_pool(&paths).await.unwrap();
        let descriptor = app_modules::discover_modules(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules"),
        )
        .unwrap()
        .into_iter()
        .find(|module| module.summary.id == "minecraft")
        .unwrap();
        sqlx::query("INSERT INTO modules (id,name,version) VALUES ('minecraft','Minecraft','1')")
            .execute(&pool)
            .await
            .unwrap();
        let program = paths.games_root.join("minecraft");
        fs::create_dir_all(program.join("libraries")).unwrap();
        fs::write(program.join("server.jar"), b"original server program").unwrap();
        fs::write(
            program.join("libraries/dependency.jar"),
            b"original dependency",
        )
        .unwrap();
        sync_game_installs(
            &paths,
            &[GameInstallSyncRecord {
                module_id: "minecraft".into(),
                install_root: program.to_string_lossy().into_owned(),
                install_state: InstallState::Installed,
                current_version: Some("fixture-version".into()),
                mark_verified: true,
            }],
        )
        .await
        .unwrap();
        let library_id = read_library_program_install(&paths, "minecraft")
            .await
            .unwrap()
            .unwrap()
            .id;
        Self {
            root,
            paths,
            pool,
            descriptor,
            program,
            library_id,
        }
    }

    async fn shared(&self, id: &str) -> PathBuf {
        let instance = self.paths.instances_root.join(id);
        fs::create_dir_all(instance.join("config")).unwrap();
        fs::create_dir_all(instance.join("data/world")).unwrap();
        fs::write(instance.join("data/world/level.dat"), id.as_bytes()).unwrap();
        fs::write(instance.join("config/server.properties"), id.as_bytes()).unwrap();
        prepare_shared_program_reference(&self.program, &instance, "minecraft").unwrap();
        let mut tx = self.pool.begin().await.unwrap();
        sqlx::query("INSERT INTO instances (id,name,module_id,config_path,data_path,logs_path,saves_path) VALUES (?1,?1,'minecraft',?2,?3,?4,?5)")
            .bind(id).bind(instance.join("config").to_string_lossy().as_ref())
            .bind(instance.join("data").to_string_lossy().as_ref())
            .bind(instance.join("logs").to_string_lossy().as_ref())
            .bind(instance.join("data/world").to_string_lossy().as_ref())
            .execute(&mut *tx).await.unwrap();
        bind_shared_install_to_instance(&mut tx, "minecraft", self.library_id, id)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        instance
    }

    async fn assert_shared(&self, id: &str, instance: &Path) {
        assert_eq!(
            instance_program_mode(instance).unwrap(),
            InstanceProgramMode::Shared
        );
        assert_eq!(
            resolve_instance_runtime_root(instance).unwrap(),
            fs::canonicalize(&self.program).unwrap()
        );
        let record = read_instance_program_install(&self.paths, id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.runtime_mode, "shared");
        assert_eq!(record.install.id, self.library_id);
        assert_eq!(record.install.scope, ProgramInstallScope::Library);
        assert!(record.install.owner_instance_id.is_none());
        assert_eq!(
            fs::read(self.program.join("server.jar")).unwrap(),
            b"original server program"
        );
    }

    async fn committed_mode(&self, id: &str) -> String {
        sqlx::query_scalar("SELECT runtime_mode FROM instances WHERE id = ?1")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    async fn close(self) {
        self.pool.close().await;
        assert!(self.root.starts_with(std::env::temp_dir()));
        fs::remove_dir_all(self.root).expect("remove only this detach fixture");
    }
}

#[tokio::test]
async fn detaching_one_instance_gives_it_writable_program_ownership_without_changing_its_peer() {
    let f = Fixture::new().await;
    let first = f.shared("first").await;
    let second = f.shared("second").await;
    let runtime = detach_instance_program(&f.paths, &f.descriptor, "first", None)
        .await
        .unwrap();
    assert_eq!(runtime, first.join("runtime"));
    assert_eq!(
        instance_program_mode(&first).unwrap(),
        InstanceProgramMode::Independent
    );
    let owned = read_instance_program_install(&f.paths, "first")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(owned.runtime_mode, "independent");
    assert_eq!(owned.install.scope, ProgramInstallScope::Instance);
    assert_eq!(owned.install.owner_instance_id.as_deref(), Some("first"));
    assert_eq!(owned.install.install_root, runtime);
    assert_eq!(
        owned.install.current_version.as_deref(),
        Some("fixture-version")
    );
    assert_ne!(owned.install.id, f.library_id);
    fs::write(runtime.join("server.jar"), b"first instance custom program").unwrap();
    fs::write(
        runtime.join("libraries/dependency.jar"),
        b"first custom dependency",
    )
    .unwrap();
    f.assert_shared("second", &second).await;
    assert_eq!(
        fs::read(f.program.join("libraries/dependency.jar")).unwrap(),
        b"original dependency"
    );
    for (id, instance) in [("first", &first), ("second", &second)] {
        assert_eq!(
            fs::read(instance.join("data/world/level.dat")).unwrap(),
            id.as_bytes()
        );
        assert_eq!(
            fs::read(instance.join("config/server.properties")).unwrap(),
            id.as_bytes()
        );
    }
    assert!(!first.join(STAGING).exists());
    assert!(!first.join(SHARED_ROLLBACK).exists());
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&f.pool)
            .await
            .unwrap()
            .is_empty()
    );
    f.close().await;
}

#[tokio::test]
async fn active_status_or_an_active_process_rejects_detachment_before_any_copy() {
    let f = Fixture::new().await;
    let instance = f.shared("active").await;
    for status in ["starting", "running", "stopping"] {
        sqlx::query("UPDATE instances SET status = ?1 WHERE id = 'active'")
            .bind(status)
            .execute(&f.pool)
            .await
            .unwrap();
        assert!(
            detach_instance_program(&f.paths, &f.descriptor, "active", None)
                .await
                .is_err()
        );
        f.assert_shared("active", &instance).await;
        assert!(!instance.join(STAGING).exists());
        assert!(!instance.join(SHARED_ROLLBACK).exists());
    }
    sqlx::query("UPDATE instances SET status = 'stopped' WHERE id = 'active'")
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO instance_runs (instance_id,pid,status) VALUES ('active',12345,'running')",
    )
    .execute(&f.pool)
    .await
    .unwrap();
    assert!(
        detach_instance_program(&f.paths, &f.descriptor, "active", None)
            .await
            .is_err()
    );
    f.assert_shared("active", &instance).await;
    assert!(!instance.join(STAGING).exists());
    f.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_during_the_real_copy_cleans_the_partial_payload_and_keeps_shared_ownership() {
    use crate::instance_creation_io::test_gate::{PausePoint, pause_at};
    use crate::test_file_snapshot::tree_snapshot;
    let f = Fixture::new().await;
    let instance = f.shared("cancelled").await;
    let source_before = tree_snapshot(&f.program).unwrap();
    let instance_before = tree_snapshot(&instance).unwrap();
    let token = Arc::new(AtomicBool::new(false));
    let mut gate = pause_at(&token, PausePoint::Copy);
    let paths = f.paths.clone();
    let descriptor = f.descriptor.clone();
    let worker_token = token.clone();
    let worker = tokio::spawn(async move {
        detach_instance_program(&paths, &descriptor, "cancelled", Some(worker_token)).await
    });
    gate.reached().await;
    let partial = tree_snapshot(&instance.join(STAGING).join("runtime.staging")).unwrap();
    assert!(partial.values().flatten().any(|bytes| !bytes.is_empty()));
    assert!(partial.values().flatten().count() < source_before.values().flatten().count());
    assert_eq!(tree_snapshot(&f.program).unwrap(), source_before);
    token.store(true, Ordering::Release);
    drop(gate);
    assert!(matches!(
        worker.await.unwrap(),
        Err(StorageError::InstanceCreationCancelled)
    ));
    f.assert_shared("cancelled", &instance).await;
    assert!(!instance.join(STAGING).exists());
    assert!(!instance.join(SHARED_ROLLBACK).exists());
    assert_eq!(tree_snapshot(&f.program).unwrap(), source_before);
    assert_eq!(tree_snapshot(&instance).unwrap(), instance_before);
    f.close().await;
}

#[tokio::test]
async fn registration_failure_after_publication_restores_the_shared_reference_and_database() {
    let f = Fixture::new().await;
    let instance = f.shared("failed").await;
    sqlx::raw_sql("CREATE TRIGGER fail_fixture_registration BEFORE INSERT ON game_installs WHEN NEW.scope = 'instance' BEGIN SELECT RAISE(ABORT, 'fixture ownership write failure'); END;")
        .execute(&f.pool).await.unwrap();
    let error = detach_instance_program(&f.paths, &f.descriptor, "failed", None)
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("fixture ownership write failure"),
        "{error}"
    );
    f.assert_shared("failed", &instance).await;
    assert!(!instance.join(STAGING).exists());
    assert!(!instance.join(SHARED_ROLLBACK).exists());
    f.close().await;
}

#[tokio::test]
async fn crash_before_ownership_commit_recovers_the_shared_layout_from_the_database() {
    let f = Fixture::new().await;
    let instance = f.shared("precommit").await;
    prepare_detach(&f.program, &instance, None).unwrap();
    publish_detach(&instance).unwrap();
    assert_eq!(
        instance_program_mode(&instance).unwrap(),
        InstanceProgramMode::Independent
    );
    let mode = f.committed_mode("precommit").await;
    assert_eq!(mode, "shared");
    recover_detach(&instance, &mode).unwrap();
    f.assert_shared("precommit", &instance).await;
    assert!(!instance.join(STAGING).exists());
    assert!(!instance.join(SHARED_ROLLBACK).exists());
    f.close().await;
}

#[tokio::test]
async fn crash_after_ownership_commit_keeps_the_independent_program_and_removes_only_the_old_reference()
 {
    let f = Fixture::new().await;
    let instance = f.shared("postcommit").await;
    let other = f.shared("other").await;
    prepare_detach(&f.program, &instance, None).unwrap();
    publish_detach(&instance).unwrap();
    let mut tx = f.pool.begin().await.unwrap();
    register_instance_install(
        &mut tx,
        "minecraft",
        "postcommit",
        &instance.join("runtime"),
        InstallState::Installed,
        Some("fixture-version"),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    fs::write(
        instance.join("runtime/server.jar"),
        b"customized committed program",
    )
    .unwrap();
    let mode = f.committed_mode("postcommit").await;
    assert_eq!(mode, "independent");
    recover_detach(&instance, &mode).unwrap();
    assert_eq!(
        instance_program_mode(&instance).unwrap(),
        InstanceProgramMode::Independent
    );
    assert_eq!(
        fs::read(instance.join("runtime/server.jar")).unwrap(),
        b"customized committed program"
    );
    f.assert_shared("other", &other).await;
    assert!(!instance.join(STAGING).exists());
    assert!(!instance.join(SHARED_ROLLBACK).exists());
    f.close().await;
}

#[tokio::test]
async fn foreign_staging_is_preserved_and_blocks_detachment() {
    let f = Fixture::new().await;
    let instance = f.shared("foreign-stage").await;
    let staging = instance.join(STAGING);
    fs::create_dir(&staging).unwrap();
    fs::write(staging.join("operator-file"), b"do not delete").unwrap();
    assert!(
        detach_instance_program(&f.paths, &f.descriptor, "foreign-stage", None)
            .await
            .is_err()
    );
    assert_eq!(
        fs::read(staging.join("operator-file")).unwrap(),
        b"do not delete"
    );
    f.assert_shared("foreign-stage", &instance).await;
    f.close().await;
}

#[tokio::test]
async fn foreign_rollback_is_preserved_and_blocks_recovery() {
    let f = Fixture::new().await;
    let instance = f.shared("foreign-rollback").await;
    let rollback = instance.join(SHARED_ROLLBACK);
    fs::create_dir(&rollback).unwrap();
    fs::write(rollback.join("operator-file"), b"keep rollback contents").unwrap();
    assert!(recover_detach(&instance, "shared").is_err());
    assert_eq!(
        fs::read(rollback.join("operator-file")).unwrap(),
        b"keep rollback contents"
    );
    f.assert_shared("foreign-rollback", &instance).await;
    f.close().await;
}

#[tokio::test]
async fn a_foreign_file_named_like_a_shared_binding_is_never_deleted_during_recovery() {
    let f = Fixture::new().await;
    let instance = f.shared("invalid-binding").await;
    let valid = fs::read(instance.join("runtime").join(SHARED_RUNTIME_BINDING)).unwrap();
    detach_instance_program(&f.paths, &f.descriptor, "invalid-binding", None)
        .await
        .unwrap();
    let rollback = instance.join(SHARED_ROLLBACK);
    fs::create_dir(&rollback).unwrap();
    let binding = rollback.join(SHARED_RUNTIME_BINDING);
    let mut different_identity: serde_json::Value = serde_json::from_slice(&valid).unwrap();
    different_identity["program_id"] = serde_json::json!("unrelated-program-owner");
    for original in [
        b"operator document, not a binding".to_vec(),
        serde_json::to_vec(&different_identity).unwrap(),
        vec![b' '; 16_385],
    ] {
        fs::write(&binding, &original).unwrap();
        assert!(recover_detach(&instance, "independent").is_err());
        assert_eq!(fs::read(&binding).unwrap(), original);
        assert_eq!(
            fs::read(instance.join("runtime/server.jar")).unwrap(),
            b"original server program"
        );
        assert_eq!(
            fs::read(f.program.join("server.jar")).unwrap(),
            b"original server program"
        );
        assert_eq!(f.committed_mode("invalid-binding").await, "independent");
    }
    f.close().await;
}

#[tokio::test]
async fn interrupted_detachment_never_moves_a_program_into_foreign_staging() {
    let f = Fixture::new().await;
    let instance = f.shared("replaced-stage").await;
    prepare_detach(&f.program, &instance, None).unwrap();
    publish_detach(&instance).unwrap();
    let staging = instance.join(STAGING);
    fs::remove_file(staging.join(STAGING_MARKER)).unwrap();
    fs::write(staging.join("operator-file"), b"foreign directory contents").unwrap();
    assert!(recover_detach(&instance, "shared").is_err());
    assert_eq!(
        fs::read(staging.join("operator-file")).unwrap(),
        b"foreign directory contents"
    );
    assert!(!staging.join("runtime").exists());
    assert_eq!(
        fs::read(instance.join("runtime/server.jar")).unwrap(),
        b"original server program"
    );
    assert!(
        instance
            .join(SHARED_ROLLBACK)
            .join(SHARED_RUNTIME_BINDING)
            .is_file()
    );
    assert_eq!(f.committed_mode("replaced-stage").await, "shared");
    f.close().await;
}

#[tokio::test]
async fn an_unknown_database_mode_preserves_the_prepared_program_for_recovery() {
    let f = Fixture::new().await;
    let instance = f.shared("unknown-mode").await;
    prepare_detach(&f.program, &instance, None).unwrap();
    assert!(recover_detach(&instance, "unrecognized").is_err());
    assert_eq!(
        fs::read(instance.join(STAGING).join("runtime/server.jar")).unwrap(),
        b"original server program"
    );
    f.assert_shared("unknown-mode", &instance).await;
    f.close().await;
}

#[tokio::test]
async fn unexpected_files_beside_the_shared_binding_are_never_deleted_by_detachment() {
    let f = Fixture::new().await;
    let instance = f.shared("additional-file").await;
    fs::write(
        instance.join("runtime/operator-file"),
        b"private added data",
    )
    .unwrap();
    assert!(
        detach_instance_program(&f.paths, &f.descriptor, "additional-file", None)
            .await
            .is_err()
    );
    assert_eq!(
        fs::read(instance.join("runtime/operator-file")).unwrap(),
        b"private added data"
    );
    f.assert_shared("additional-file", &instance).await;
    assert!(!instance.join(STAGING).exists());
    f.close().await;
}
