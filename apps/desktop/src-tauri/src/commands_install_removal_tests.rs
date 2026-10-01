use super::*;
use app_core::InstallState;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

struct Fixture {
    root: PathBuf,
    paths: StoragePaths,
    record: ProgramInstallRecord,
}

impl Fixture {
    async fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lg-removal-{}", uuid::Uuid::new_v4()));
        let paths = StoragePaths {
            app_data_root: root.join("appdata"),
            settings_path: root.join("appdata/settings.json"),
            database_path: root.join("appdata/db/store.db"),
            logs_root: root.join("appdata/logs"),
            modules_root: fs::canonicalize(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../modules"),
            )
            .unwrap(),
            migrations_root: root.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances/.trash"),
        };
        let game = paths.games_root.join("palworld");
        fs::create_dir_all(game.join("saves/world")).unwrap();
        fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
        fs::write(game.join("Server.exe"), b"program").unwrap();
        fs::write(game.join("saves/world/save.dat"), b"retained").unwrap();
        app_storage::initialize_database(&paths).await.unwrap();
        let descriptors = app_modules::discover_modules(&paths.modules_root).unwrap();
        app_storage::sync_modules(&paths, &descriptors)
            .await
            .unwrap();
        app_storage::sync_game_installs(
            &paths,
            &[app_storage::GameInstallSyncRecord {
                module_id: "palworld".into(),
                install_root: game.to_string_lossy().into_owned(),
                install_state: InstallState::Installed,
                current_version: Some("fixture-build".into()),
                mark_verified: true,
            }],
        )
        .await
        .unwrap();
        let record = app_storage::read_program_install_owner(&paths, &game)
            .await
            .unwrap()
            .unwrap();
        Self {
            root,
            paths,
            record,
        }
    }

    fn game(&self) -> &Path {
        &self.record.install_root
    }

    fn protected(&self) -> Vec<ProtectedInstallDataPath> {
        vec![ProtectedInstallDataPath {
            source: "world".into(),
            path: Some(self.game().join("saves")),
        }]
    }

    async fn prepare(&self, retained: Vec<PathBuf>) -> (journal::Journal, ProgramRemovalRecord) {
        let journal = journal::Journal::prepare(&self.record, retained).unwrap();
        let record = ProgramRemovalRecord {
            operation_id: journal.operation_id.clone(),
            module_id: journal.module_id.clone(),
            install_id: journal.install_id,
            source_root: journal.source.clone(),
            phase: "prepared".into(),
            journal_json: journal.encode().unwrap(),
        };
        removals_db::begin(&self.paths, &record).await.unwrap();
        (journal, record)
    }

    async fn state(&self) -> InstallState {
        app_storage::read_program_install_owner(&self.paths, self.game())
            .await
            .unwrap()
            .unwrap()
            .install_state
    }

    async fn pending(&self) -> Vec<ProgramRemovalRecord> {
        removals_db::list(&self.paths, "palworld").await.unwrap()
    }

    fn assert_original(&self) {
        assert_eq!(
            fs::read(self.game().join("Server.exe")).unwrap(),
            b"program"
        );
        assert_eq!(
            fs::read(self.game().join("saves/world/save.dat")).unwrap(),
            b"retained"
        );
        assert!(
            !self
                .game()
                .join(app_steamcmd::RETAINED_INSTALL_DATA_MARKER)
                .exists()
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn staged(journal: &journal::Journal) -> PathBuf {
    serde_json::from_value(
        serde_json::from_str::<serde_json::Value>(&journal.encode().unwrap()).unwrap()["staged"]
            .clone(),
    )
    .unwrap()
}

#[tokio::test]
async fn staged_uninstall_keeps_saves_at_their_original_path() {
    let fixture = Fixture::new().await;
    let result = remove_library_program(&fixture.paths, &fixture.record, &fixture.protected())
        .await
        .unwrap();
    assert!(!fixture.game().join("Server.exe").exists());
    assert_eq!(
        fs::read(fixture.game().join("saves/world/save.dat")).unwrap(),
        b"retained"
    );
    assert!(app_steamcmd::has_retained_install_data(fixture.game()));
    assert_eq!(result.preserved_data_paths.len(), 1);
    assert_eq!(fixture.state().await, InstallState::NotInstalled);
    assert!(fixture.pending().await.is_empty());
    assert_eq!(fs::read_dir(&fixture.paths.games_root).unwrap().count(), 1);
}

#[tokio::test]
async fn prepared_uninstall_recovers_program_and_retained_data_after_restart() {
    let fixture = Fixture::new().await;
    let (journal, _) = fixture.prepare(vec![PathBuf::from("saves")]).await;
    journal.stage().unwrap();
    let detached = staged(&journal);
    drop(journal);
    recover_library_removals(&fixture.paths, "palworld")
        .await
        .unwrap();
    fixture.assert_original();
    assert_eq!(fixture.state().await, InstallState::Installed);
    assert!(fixture.pending().await.is_empty());
    assert!(!detached.exists());
}

#[tokio::test]
async fn prepared_uninstall_before_first_rename_recovers_without_payload_changes() {
    let fixture = Fixture::new().await;
    fixture.prepare(vec![PathBuf::from("saves")]).await;
    recover_library_removals(&fixture.paths, "palworld")
        .await
        .unwrap();
    fixture.assert_original();
    assert!(fixture.pending().await.is_empty());
    assert_eq!(fs::read_dir(&fixture.paths.games_root).unwrap().count(), 1);
}

#[tokio::test]
async fn committed_uninstall_recovery_only_purges_and_never_restores_program() {
    let fixture = Fixture::new().await;
    let (journal, record) = fixture.prepare(vec![PathBuf::from("saves")]).await;
    journal.stage().unwrap();
    removals_db::commit(&fixture.paths, &record).await.unwrap();
    fs::write(
        fixture.game().join("created-after-commit.txt"),
        b"new user data",
    )
    .unwrap();
    recover_library_removals(&fixture.paths, "palworld")
        .await
        .unwrap();
    recover_library_removals(&fixture.paths, "palworld")
        .await
        .unwrap();
    assert!(!staged(&journal).exists());
    assert!(!fixture.game().join("Server.exe").exists());
    assert_eq!(
        fs::read(fixture.game().join("created-after-commit.txt")).unwrap(),
        b"new user data"
    );
    assert_eq!(
        fs::read(fixture.game().join("saves/world/save.dat")).unwrap(),
        b"retained"
    );
    assert_eq!(fixture.state().await, InstallState::NotInstalled);
    assert!(fixture.pending().await.is_empty());
}

#[tokio::test]
async fn database_commit_failure_restores_program_and_retained_data() {
    let fixture = Fixture::new().await;
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(&fixture.paths.database_path))
        .await
        .unwrap();
    sqlx::query("CREATE TRIGGER reject_uninstall BEFORE UPDATE OF install_state ON game_installs WHEN NEW.install_state='not_installed' BEGIN SELECT RAISE(ABORT,'injected uninstall commit failure'); END")
        .execute(&pool).await.unwrap();
    pool.close().await;
    let error =
        match remove_library_program(&fixture.paths, &fixture.record, &fixture.protected()).await {
            Ok(_) => panic!("database trigger must fail uninstall"),
            Err(error) => error,
        };
    assert!(
        error.contains("injected uninstall commit failure"),
        "{error}"
    );
    fixture.assert_original();
    assert_eq!(fixture.state().await, InstallState::Installed);
    assert!(fixture.pending().await.is_empty());
}

#[tokio::test]
async fn parent_and_duplicate_protections_are_moved_once() {
    let fixture = Fixture::new().await;
    let mut protected = fixture.protected();
    protected.extend(fixture.protected());
    protected.push(ProtectedInstallDataPath {
        source: "file".into(),
        path: Some(fixture.game().join("saves/world/save.dat")),
    });
    let result = remove_library_program(&fixture.paths, &fixture.record, &protected)
        .await
        .unwrap();
    assert_eq!(result.preserved_data_paths.len(), 1);
    assert_eq!(
        fs::read(fixture.game().join("saves/world/save.dat")).unwrap(),
        b"retained"
    );
}

#[tokio::test]
async fn rollback_does_not_delete_files_created_by_another_writer() {
    let fixture = Fixture::new().await;
    let (journal, _) = fixture.prepare(vec![PathBuf::from("saves")]).await;
    journal.stage().unwrap();
    fs::write(fixture.game().join("unknown.dat"), b"concurrent").unwrap();
    assert!(
        recover_library_removals(&fixture.paths, "palworld")
            .await
            .is_err()
    );
    assert_eq!(
        fs::read(fixture.game().join("unknown.dat")).unwrap(),
        b"concurrent"
    );
    assert_eq!(
        fs::read(staged(&journal).join("Server.exe")).unwrap(),
        b"program"
    );
    assert_eq!(
        fs::read(staged(&journal).join("saves/world/save.dat")).unwrap(),
        b"retained"
    );
    assert_eq!(fixture.pending().await.len(), 1);
    assert_eq!(fixture.state().await, InstallState::Installed);
}

#[tokio::test]
async fn committed_recovery_rejects_a_replaced_staging_directory() {
    let fixture = Fixture::new().await;
    let (journal, record) = fixture.prepare(vec![PathBuf::from("saves")]).await;
    journal.stage().unwrap();
    removals_db::commit(&fixture.paths, &record).await.unwrap();
    let detached = staged(&journal);
    let displaced = fixture.root.join("original-staged");
    fs::rename(&detached, &displaced).unwrap();
    fs::create_dir(&detached).unwrap();
    fs::write(detached.join("unrelated.txt"), b"keep me").unwrap();
    assert!(
        recover_library_removals(&fixture.paths, "palworld")
            .await
            .is_err()
    );
    assert_eq!(
        fs::read(detached.join("unrelated.txt")).unwrap(),
        b"keep me"
    );
    assert_eq!(fs::read(displaced.join("Server.exe")).unwrap(), b"program");
    assert_eq!(fixture.pending().await.len(), 1);
}

#[tokio::test]
async fn removal_without_retained_data_removes_the_whole_root() {
    let fixture = Fixture::new().await;
    remove_library_program(&fixture.paths, &fixture.record, &[])
        .await
        .unwrap();
    assert!(!fixture.game().exists());
    assert_eq!(fixture.state().await, InstallState::NotInstalled);
    assert!(fixture.pending().await.is_empty());
}

#[tokio::test]
async fn single_file_protection_keeps_its_bytes_and_parent_path() {
    let fixture = Fixture::new().await;
    let protected = [ProtectedInstallDataPath {
        source: "file".into(),
        path: Some(fixture.game().join("saves/world/save.dat")),
    }];
    remove_library_program(&fixture.paths, &fixture.record, &protected)
        .await
        .unwrap();
    assert_eq!(
        fs::read(fixture.game().join("saves/world/save.dat")).unwrap(),
        b"retained"
    );
    assert!(!fixture.game().join("Server.exe").exists());
    assert!(fixture.pending().await.is_empty());
}

#[tokio::test]
async fn unresolved_and_root_wide_data_never_detach_the_installation() {
    let fixture = Fixture::new().await;
    for path in [
        None,
        Some(fixture.game().to_owned()),
        Some(fixture.root.clone()),
    ] {
        let protected = [ProtectedInstallDataPath {
            source: "unresolved".into(),
            path,
        }];
        assert!(
            remove_library_program(&fixture.paths, &fixture.record, &protected)
                .await
                .is_err()
        );
        fixture.assert_original();
        assert!(fixture.pending().await.is_empty());
    }
}

#[tokio::test]
async fn recovery_rejects_a_journal_with_an_unrelated_staging_path() {
    let fixture = Fixture::new().await;
    let (journal, _) = fixture.prepare(vec![PathBuf::from("saves")]).await;
    let mut value: serde_json::Value = serde_json::from_str(&journal.encode().unwrap()).unwrap();
    value["staged"] = serde_json::to_value(fixture.root.join("unrelated")).unwrap();
    assert!(journal::Journal::decode(&value.to_string()).is_err());
    fixture.assert_original();
    recover_library_removals(&fixture.paths, "palworld")
        .await
        .unwrap();
}

#[tokio::test]
async fn removal_and_recovery_refuse_a_registration_outside_managed_program_roots() {
    let mut fixture = Fixture::new().await;
    let original_games = fixture.paths.games_root.clone();
    fixture.paths.games_root = fixture.root.join("other-library");
    assert!(
        remove_library_program(&fixture.paths, &fixture.record, &fixture.protected())
            .await
            .is_err()
    );
    fixture.assert_original();
    assert!(fixture.pending().await.is_empty());
    fixture.paths.games_root = original_games;
    let (journal, _) = fixture.prepare(vec![PathBuf::from("saves")]).await;
    journal.stage().unwrap();
    fixture.paths.games_root = fixture.root.join("other-library");
    assert!(
        recover_library_removals(&fixture.paths, "palworld")
            .await
            .is_err()
    );
    assert_eq!(
        fs::read(staged(&journal).join("Server.exe")).unwrap(),
        b"program"
    );
    assert_eq!(
        fs::read(fixture.game().join("saves/world/save.dat")).unwrap(),
        b"retained"
    );
    assert_eq!(fixture.pending().await.len(), 1);
}
