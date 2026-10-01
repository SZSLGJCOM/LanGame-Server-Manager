use super::*;
use crate::{adopt_library_install_for_instance, initialize_database};
use sqlx::SqlitePool;

struct Fixture {
    root: PathBuf,
    paths: StoragePaths,
    pool: SqlitePool,
    source: PathBuf,
    instance: PathBuf,
}

impl Fixture {
    async fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "langame-adoption-recovery-{}",
            uuid::Uuid::new_v4()
        ));
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
        sqlx::query("INSERT INTO modules (id,name,version) VALUES ('game','Game','1'),('other','Other','1')")
            .execute(&pool).await.unwrap();
        let source = root.join("custom-library");
        let instance = paths.instances_root.join("first");
        fs::create_dir_all(source.join("Saved")).unwrap();
        fs::create_dir_all(instance.join("config")).unwrap();
        fs::write(source.join("server.bin"), b"downloaded original program").unwrap();
        fs::write(source.join("Saved/world.sav"), b"original world").unwrap();
        fs::write(
            instance.join("operator-note.txt"),
            b"unrecognized instance data must survive",
        )
        .unwrap();
        sqlx::query("INSERT INTO game_installs (id,module_id,install_root,install_state) VALUES (1,'game',?1,'installed')")
            .bind(source.to_string_lossy().as_ref()).execute(&pool).await.unwrap();
        Self {
            root,
            paths,
            pool,
            source,
            instance,
        }
    }

    fn adopt(&self) {
        ProgramAdoption::begin(
            &self.source,
            &self.instance,
            &[self.source.join("Saved")],
            false,
            crate::program_runtime::ProgramFileSelection::Automatic,
            None,
        )
        .unwrap()
        .expect("same-volume fixture adoption");
    }

    async fn relocate_program_to(&mut self, source: PathBuf, module_id: &str) -> PathBuf {
        let library = self.source.clone();
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::rename(&library, &source).unwrap();
        fs::create_dir(&library).unwrap();
        fs::write(library.join("server.bin"), b"retained library program").unwrap();
        sqlx::query("UPDATE game_installs SET module_id=?1,install_root=?2 WHERE id=1")
            .bind(module_id)
            .bind(source.to_string_lossy().as_ref())
            .execute(&self.pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO game_installs (id,module_id,install_root,install_state) VALUES (2,'game',?1,'installed')")
            .bind(library.to_string_lossy().as_ref()).execute(&self.pool).await.unwrap();
        self.source = source;
        library
    }

    async fn insert_instance(&self, id: &str, root: &Path) {
        fs::create_dir_all(root.join("config")).unwrap();
        sqlx::query("INSERT INTO instances (id,name,module_id,config_path,data_path,logs_path,saves_path) VALUES (?1,?1,'game',?2,?3,?4,?5)")
            .bind(id).bind(root.join("config").to_string_lossy().as_ref())
            .bind(root.join("data").to_string_lossy().as_ref()).bind(root.join("logs").to_string_lossy().as_ref())
            .bind(root.join("data/save").to_string_lossy().as_ref()).execute(&self.pool).await.unwrap();
    }

    async fn recover(&self) -> Result<(), StorageError> {
        recover_interrupted_program_adoptions(&self.paths, "game", &self.source).await
    }

    fn assert_uncommitted_preserved(&self) {
        assert!(!self.source.exists());
        assert!(self.instance.join(ADOPTION_JOURNAL).exists());
        assert_eq!(
            fs::read(self.instance.join("runtime/server.bin")).unwrap(),
            b"downloaded original program"
        );
        assert_eq!(
            fs::read(self.instance.join("installation-retained/Saved/world.sav")).unwrap(),
            b"original world"
        );
    }

    async fn close(self) {
        self.pool.close().await;
        assert!(self.root.starts_with(std::env::temp_dir()));
        fs::remove_dir_all(self.root).expect("remove only this adoption recovery fixture");
    }
}

#[tokio::test]
async fn restart_before_database_commit_restores_the_download_and_preserves_unknown_instance_data()
{
    let f = Fixture::new().await;
    f.adopt();
    // A process can stop immediately after the rename, before the marker write.
    fs::remove_file(f.instance.join("runtime/.langame-private-runtime")).unwrap();
    f.recover().await.unwrap();
    assert_eq!(
        fs::read(f.source.join("server.bin")).unwrap(),
        b"downloaded original program"
    );
    assert_eq!(
        fs::read(f.source.join("Saved/world.sav")).unwrap(),
        b"original world"
    );
    assert_eq!(
        fs::read(f.instance.join("operator-note.txt")).unwrap(),
        b"unrecognized instance data must survive"
    );
    assert!(!f.instance.join(ADOPTION_JOURNAL).exists());
    assert!(!f.instance.join("runtime").exists());
    f.recover().await.unwrap();
    f.close().await;
}

#[tokio::test]
async fn restart_after_ownership_commit_keeps_the_runtime_and_clears_only_the_journal() {
    let f = Fixture::new().await;
    f.adopt();
    f.insert_instance("first", &f.instance).await;
    let mut tx = f.pool.begin().await.unwrap();
    adopt_library_install_for_instance(&mut tx, "game", 1, "first", &f.instance.join("runtime"))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let original = fs::read(f.instance.join("runtime/server.bin")).unwrap();
    f.recover().await.unwrap();
    assert!(!f.source.exists());
    assert_eq!(
        fs::read(f.instance.join("runtime/server.bin")).unwrap(),
        original
    );
    assert!(crate::resolve_instance_private_runtime_root(&f.instance).is_ok());
    assert_eq!(
        fs::read(f.instance.join("installation-retained/Saved/world.sav")).unwrap(),
        b"original world"
    );
    assert!(!f.instance.join(ADOPTION_JOURNAL).exists());
    f.recover().await.unwrap();
    f.close().await;
}

#[tokio::test]
async fn committed_instance_acquisition_is_recovered_through_the_retained_library() {
    let mut f = Fixture::new().await;
    let source = crate::program_instance_acquisition::module_root(&f.paths, "game")
        .unwrap()
        .join(uuid::Uuid::new_v4().to_string());
    let library = f.relocate_program_to(source, "game").await;
    f.adopt();
    f.insert_instance("first", &f.instance).await;
    let mut tx = f.pool.begin().await.unwrap();
    adopt_library_install_for_instance(&mut tx, "game", 1, "first", &f.instance.join("runtime"))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let selected = crate::read_library_program_install(&f.paths, "game")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(selected.id, 2);
    let runtime_before =
        crate::test_file_snapshot::tree_snapshot(&f.instance.join("runtime")).unwrap();
    let library_before = crate::test_file_snapshot::tree_snapshot(&library).unwrap();
    for _ in 0..2 {
        recover_interrupted_program_adoptions(&f.paths, "game", &selected.install_root)
            .await
            .unwrap();
    }
    assert!(!f.source.exists());
    assert!(!f.instance.join(ADOPTION_JOURNAL).exists());
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&f.instance.join("runtime")).unwrap(),
        runtime_before
    );
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&library).unwrap(),
        library_before
    );
    let owned = crate::read_instance_program_install(&f.paths, "first")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(owned.install.id, 1);
    assert_eq!(owned.install.scope, crate::ProgramInstallScope::Instance);
    assert_eq!(owned.install.owner_instance_id.as_deref(), Some("first"));
    f.close().await;
}

#[tokio::test]
async fn uncommitted_instance_acquisition_returns_to_its_own_source() {
    let mut f = Fixture::new().await;
    let source = crate::program_instance_acquisition::module_root(&f.paths, "game")
        .unwrap()
        .join(uuid::Uuid::new_v4().to_string());
    let library = f.relocate_program_to(source, "game").await;
    let source_before = crate::test_file_snapshot::tree_snapshot(&f.source).unwrap();
    let library_before = crate::test_file_snapshot::tree_snapshot(&library).unwrap();
    f.adopt();
    fs::remove_file(f.instance.join("runtime/.langame-private-runtime")).unwrap();
    recover_interrupted_program_adoptions(&f.paths, "game", &library)
        .await
        .unwrap();
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&f.source).unwrap(),
        source_before
    );
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&library).unwrap(),
        library_before
    );
    assert!(!f.instance.join(ADOPTION_JOURNAL).exists());
    assert!(!f.instance.join("runtime").exists());
    assert_eq!(
        fs::read(f.instance.join("operator-note.txt")).unwrap(),
        b"unrecognized instance data must survive"
    );
    f.close().await;
}

#[tokio::test]
async fn acquisition_recovery_does_not_claim_other_modules_or_nested_sources() {
    for (module_id, nested) in [("other", false), ("game", true)] {
        let mut f = Fixture::new().await;
        let mut source =
            crate::program_instance_acquisition::module_root(&f.paths, module_id).unwrap();
        if nested {
            source = source.join("unrecognized-parent");
        }
        source = source.join(uuid::Uuid::new_v4().to_string());
        let library = f.relocate_program_to(source, module_id).await;
        f.adopt();
        let before = crate::test_file_snapshot::tree_snapshot(&f.instance).unwrap();
        recover_interrupted_program_adoptions(&f.paths, "game", &library)
            .await
            .unwrap();
        assert_eq!(
            crate::test_file_snapshot::tree_snapshot(&f.instance).unwrap(),
            before
        );
        f.assert_uncommitted_preserved();
        f.close().await;
    }
}

#[tokio::test]
async fn conflicting_acquisition_owner_keeps_the_runtime_and_recovery_journal() {
    let mut f = Fixture::new().await;
    let source = crate::program_instance_acquisition::module_root(&f.paths, "game")
        .unwrap()
        .join(uuid::Uuid::new_v4().to_string());
    let library = f.relocate_program_to(source, "game").await;
    f.adopt();
    f.insert_instance("other-owner", &f.paths.instances_root.join("other-owner"))
        .await;
    sqlx::query(
        "UPDATE game_installs SET scope='instance',owner_instance_id='other-owner' WHERE id=1",
    )
    .execute(&f.pool)
    .await
    .unwrap();
    let instance_before = crate::test_file_snapshot::tree_snapshot(&f.instance).unwrap();
    let library_before = crate::test_file_snapshot::tree_snapshot(&library).unwrap();
    let error = recover_interrupted_program_adoptions(&f.paths, "game", &library)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("another installation owns"));
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&f.instance).unwrap(),
        instance_before
    );
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&library).unwrap(),
        library_before
    );
    f.assert_uncommitted_preserved();
    f.close().await;
}

#[tokio::test]
async fn another_modules_valid_journal_is_not_recovered_into_this_library() {
    let f = Fixture::new().await;
    f.adopt();
    recover_interrupted_program_adoptions(&f.paths, "other", &f.root.join("different-library"))
        .await
        .unwrap();
    f.assert_uncommitted_preserved();
    f.close().await;
}

#[tokio::test]
async fn unverified_database_ownership_preserves_both_the_program_and_journal() {
    let f = Fixture::new().await;
    f.adopt();
    f.insert_instance("first", &f.instance).await;
    assert!(f.recover().await.is_err());
    f.assert_uncommitted_preserved();
    f.close().await;
}

#[tokio::test]
async fn a_different_owner_of_the_source_prevents_rollback() {
    let f = Fixture::new().await;
    f.adopt();
    let other = f.paths.instances_root.join("other-owner");
    f.insert_instance("other-owner", &other).await;
    sqlx::query("UPDATE game_installs SET scope = 'instance',owner_instance_id = 'other-owner' WHERE id = 1")
        .execute(&f.pool).await.unwrap();
    assert!(f.recover().await.is_err());
    f.assert_uncommitted_preserved();
    f.close().await;
}

#[tokio::test]
async fn a_recreated_source_is_never_overwritten_by_recovery() {
    let f = Fixture::new().await;
    f.adopt();
    fs::create_dir(&f.source).unwrap();
    fs::write(f.source.join("server.bin"), b"other directory owner").unwrap();
    assert!(f.recover().await.is_err());
    assert_eq!(
        fs::read(f.source.join("server.bin")).unwrap(),
        b"other directory owner"
    );
    assert_eq!(
        fs::read(f.instance.join("runtime/server.bin")).unwrap(),
        b"downloaded original program"
    );
    assert!(f.instance.join(ADOPTION_JOURNAL).exists());
    f.close().await;
}

#[tokio::test]
async fn a_journal_with_another_instance_root_cannot_move_or_delete_either_directory() {
    let f = Fixture::new().await;
    f.adopt();
    let path = f.instance.join(ADOPTION_JOURNAL);
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["instance_root"] = serde_json::json!(f.paths.instances_root.join("unrecognized"));
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(f.recover().await.is_err());
    f.assert_uncommitted_preserved();
    f.close().await;
}

#[tokio::test]
async fn malformed_or_oversized_journals_are_retained_without_touching_program_bytes() {
    let f = Fixture::new().await;
    f.adopt();
    let path = f.instance.join(ADOPTION_JOURNAL);
    let valid = fs::read(&path).unwrap();
    for bytes in [b"invalid json".to_vec(), vec![b' '; 65_537]] {
        fs::write(&path, &bytes).unwrap();
        assert!(f.recover().await.is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        f.assert_uncommitted_preserved();
    }
    let mut value: serde_json::Value = serde_json::from_slice(&valid).unwrap();
    value["version"] = serde_json::json!(2);
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(f.recover().await.is_err());
    f.assert_uncommitted_preserved();
    f.close().await;
}

#[tokio::test]
async fn traversal_in_a_retained_data_key_is_rejected_before_restoring_anything() {
    let f = Fixture::new().await;
    f.adopt();
    let path = f.instance.join(ADOPTION_JOURNAL);
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["exclusions"] = serde_json::json!(["../operator-data"]);
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(f.recover().await.is_err());
    f.assert_uncommitted_preserved();
    f.close().await;
}

#[tokio::test]
async fn directories_without_a_journal_are_untouched() {
    let f = Fixture::new().await;
    let unknown = f.paths.instances_root.join("unknown");
    fs::create_dir(&unknown).unwrap();
    fs::write(unknown.join("data"), b"unrecognized owner").unwrap();
    f.recover().await.unwrap();
    assert_eq!(
        fs::read(unknown.join("data")).unwrap(),
        b"unrecognized owner"
    );
    assert_eq!(
        fs::read(f.source.join("server.bin")).unwrap(),
        b"downloaded original program"
    );
    f.close().await;
}
