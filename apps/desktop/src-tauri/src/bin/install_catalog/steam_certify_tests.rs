use super::*;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("lgsm-certify-preflight-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("payload.bin"), b"program").unwrap();
        Self(root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn certification_rejects_retained_and_used_trees_without_removing_markers() {
    let fixture = Fixture::new();
    let root = &fixture.0;
    assert!(preflight(root).is_ok());
    for marker in [
        RETAINED_INSTALL_DATA_MARKER,
        ".langame-program-usage.json",
        ".langame-program-acquisition.json",
        ".langame-steam-cache-seed.json",
        ".langame-program-identity.json",
    ] {
        fs::write(root.join(marker), b"existing ownership").unwrap();
        assert!(preflight(root).is_err(), "accepted {marker}");
        assert_eq!(fs::read(root.join(marker)).unwrap(), b"existing ownership");
        fs::remove_file(root.join(marker)).unwrap();
    }
    assert_eq!(fs::read(root.join("payload.bin")).unwrap(), b"program");
}

#[test]
fn unknown_archive_entries_prevent_certification_without_being_removed() {
    let fixture = Fixture::new();
    let archive = fixture.0.join("archives");
    assert!(archive_directory_is_empty(&archive).unwrap());
    assert!(!archive.exists());
    fs::create_dir(&archive).unwrap();
    assert!(archive_directory_is_empty(&archive).unwrap());
    fs::write(archive.join("unrecognized.dat"), b"recoverable").unwrap();
    assert!(!archive_directory_is_empty(&archive).unwrap());
    assert_eq!(
        fs::read(archive.join("unrecognized.dat")).unwrap(),
        b"recoverable"
    );
}

#[cfg(windows)]
#[tokio::test]
async fn certification_reuses_registration_with_equivalent_windows_path_spelling() {
    use app_storage::initialize_database;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    let fixture = Fixture::new();
    let paths = StoragePaths {
        app_data_root: fixture.0.join("app-data"),
        settings_path: fixture.0.join("app-data/settings.json"),
        database_path: fixture.0.join("app-data/db/test.db"),
        logs_root: fixture.0.join("logs"),
        modules_root: fixture.0.join("modules"),
        migrations_root: fixture.0.join("migrations"),
        steamcmd_root: fixture.0.join("steamcmd"),
        games_root: fixture.0.join("games"),
        instances_root: fixture.0.join("instances"),
        archives_root: fixture.0.join("archives"),
    };
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    fs::create_dir_all(&paths.games_root).unwrap();
    initialize_database(&paths).await.unwrap();
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(&paths.database_path))
        .await
        .unwrap();
    sqlx::query("INSERT INTO modules (id,name,version) VALUES ('fixture','Fixture','1')")
        .execute(&pool)
        .await
        .unwrap();
    let root = steam_seed::checked_root(&paths.games_root).unwrap();
    let spelling = root.to_string_lossy().replace('\\', "/");
    assert_ne!(spelling, root.to_string_lossy());
    let mut record = GameInstallSyncRecord {
        module_id: "fixture".into(),
        install_root: spelling.clone(),
        install_state: InstallState::Installed,
        current_version: Some("old".into()),
        mark_verified: true,
    };
    sync_game_installs(&paths, &[record.clone()]).await.unwrap();
    let original = read_program_install_owner(&paths, &root)
        .await
        .unwrap()
        .unwrap();
    record.install_root = registered_root(&paths, &root).await.unwrap();
    assert_eq!(record.install_root, spelling);
    record.install_state = InstallState::Incomplete;
    record.mark_verified = false;
    sync_game_installs(&paths, &[record.clone()]).await.unwrap();
    record.install_state = InstallState::Installed;
    record.current_version = Some("validated".into());
    record.mark_verified = true;
    sync_game_installs(&paths, &[record]).await.unwrap();
    let after = read_program_install_owner(&paths, &root)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.id, original.id);
    assert_eq!(after.current_version.as_deref(), Some("validated"));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM game_installs")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    pool.close().await;
}
