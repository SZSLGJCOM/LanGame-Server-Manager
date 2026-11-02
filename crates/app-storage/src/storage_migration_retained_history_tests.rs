//! Real file/SQLite migrations in owned fixtures; no default storage is used.
use super::*;
use crate::storage_migration as migration;
use app_core::{AppSettings, StorageMigrationPhase};
use std::sync::{Arc, atomic::AtomicBool};

const HISTORY: &str = ".langame-steam-retained-history-42-1700000000000000000-1";

#[derive(Clone, Copy, Debug)]
enum Role {
    Games,
    Instances,
    Archives,
    SteamCmd,
}

fn selection(fixture: &Fixture, role: Role) -> (AppSettings, PathBuf, PathBuf) {
    let mut settings = fixture.paths.settings();
    let (source, leaf) = match role {
        Role::Games => (&fixture.paths.games_root, "moved-games"),
        Role::Instances => (&fixture.paths.instances_root, "moved-instances"),
        Role::Archives => (&fixture.paths.archives_root, "moved-archives"),
        Role::SteamCmd => (&fixture.paths.steamcmd_root, "moved-steamcmd"),
    };
    let target = fixture.root.join(leaf);
    let selected = target.to_string_lossy().into_owned();
    match role {
        Role::Games => settings.games_root = selected,
        Role::Instances => {
            settings.servers_root = selected;
            settings.archives_root = target.join(".trash").to_string_lossy().into_owned();
        }
        Role::Archives => settings.archives_root = selected,
        Role::SteamCmd => settings.steamcmd_root = selected,
    }
    (settings, source.clone(), target)
}

fn write_history(fixture: &Fixture, program: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let root = fixture.paths.games_root.join(HISTORY);
    let owner = write_json(
        &root.join("owner.json"),
        &json!({
            "version":1,"module_id":"fixture","root":fs::canonicalize(program).unwrap(),
            "root_identity":identity(program, true).unwrap(),
        }),
    );
    let inventory = write_json(
        &root.join("inventory.json"),
        &json!([{"path":"Server.cfg","directory":false,"bytes":17,"sha256":digest(b"operator settings")}]),
    );
    let files = vec![
        (PathBuf::from("owner.json"), owner),
        (PathBuf::from("inventory.json"), inventory),
        (PathBuf::from("native-started"), b"1\n".to_vec()),
        (PathBuf::from("native-stopped"), b"success\n".to_vec()),
        (PathBuf::from("restored"), b"1\n".to_vec()),
        (
            PathBuf::from("changed/Server.cfg"),
            b"depot defaults".to_vec(),
        ),
    ];
    for (relative, bytes) in &files {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    files
}

async fn assert_retired_history_allows_migration(role: Role) {
    let fixture = Fixture::new();
    let (pool, archive_id, _) = archived_fixture(&fixture).await;
    let program = fixture.paths.games_root.join("fixture");
    fs::create_dir_all(&program).unwrap();
    fs::write(program.join("server.exe"), b"library program").unwrap();
    let program_owner = uuid::Uuid::new_v4().to_string();
    let identity_bytes = write_json(
        &program.join(".langame-program-identity.json"),
        &json!({"version":1,"id":program_owner,"module_id":"fixture"}),
    );
    write_json(
        &program.join(".langame-retained-library.json"),
        &json!({"version":1,"module_id":"fixture","program_root":program}),
    );
    let library_id = sqlx::query("INSERT INTO game_installs(module_id,install_root,install_state,scope) VALUES('fixture',?1,'installed','library')")
        .bind(program.to_string_lossy().as_ref()).execute(&pool).await.unwrap().last_insert_rowid();
    let history = write_history(&fixture, &program);
    fs::write(
        fixture.paths.instances_root.join("workspace.txt"),
        b"working data",
    )
    .unwrap();
    fs::write(
        fixture.paths.steamcmd_root.join("steamcmd.exe"),
        b"command tool",
    )
    .unwrap();
    write_json(
        &fixture
            .paths
            .steamcmd_root
            .join(".langame-steamcmd-owner.json"),
        &json!({"product":"langame-server-manager","canonical_root":fs::canonicalize(&fixture.paths.steamcmd_root).unwrap()}),
    );
    let (requested, source, target) = selection(&fixture, role);
    let expected_paths = fixture.paths.with_app_settings(&requested);
    let cancellation = Arc::new(AtomicBool::new(false));
    let progress: migration::Progress = Arc::new(|_| {});
    let prepared = migration::preview(
        &fixture.paths,
        requested,
        uuid::Uuid::new_v4().to_string(),
        Vec::new(),
        cancellation.clone(),
        progress.clone(),
    )
    .await
    .unwrap_or_else(|error| panic!("{role:?} migration rejected completed history: {error}"));
    assert_eq!(prepared.status().phase, StorageMigrationPhase::Ready);
    assert!(!target.exists(), "preflight must not publish any target");
    let completed = migration::execute(&fixture.paths, prepared, cancellation, progress)
        .await
        .unwrap();
    assert_eq!(completed.phase, StorageMigrationPhase::Completed);
    assert!(
        completed.cleanup_warnings.is_empty(),
        "{:?}",
        completed.cleanup_warnings
    );
    assert!(!source.exists(), "the selected old root must be cleaned");
    let persisted = crate::settings::load_persisted_app_settings(&fixture.paths)
        .unwrap()
        .unwrap();
    let next = fixture.paths.with_app_settings(&persisted);
    assert_eq!(next.games_root, expected_paths.games_root);
    assert_eq!(next.instances_root, expected_paths.instances_root);
    assert_eq!(next.archives_root, expected_paths.archives_root);
    assert_eq!(next.steamcmd_root, expected_paths.steamcmd_root);
    for (relative, bytes) in history {
        assert_eq!(
            fs::read(next.games_root.join(HISTORY).join(relative)).unwrap(),
            bytes
        );
    }
    assert_eq!(
        fs::read(next.games_root.join("fixture/server.exe")).unwrap(),
        b"library program"
    );
    assert_eq!(
        fs::read(
            next.games_root
                .join("fixture/.langame-program-identity.json")
        )
        .unwrap(),
        identity_bytes
    );
    let (_, owner) = read_json(
        &next
            .games_root
            .join("fixture/.langame-retained-library.json"),
    )
    .unwrap();
    assert_eq!(
        Path::new(owner["program_root"].as_str().unwrap()),
        next.games_root.join("fixture")
    );
    let registered: String =
        sqlx::query_scalar("SELECT install_root FROM game_installs WHERE id=?1")
            .bind(library_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(Path::new(&registered), next.games_root.join("fixture"));
    assert_eq!(
        fs::read(next.instances_root.join("workspace.txt")).unwrap(),
        b"working data"
    );
    assert_eq!(
        fs::read(next.steamcmd_root.join("steamcmd.exe")).unwrap(),
        b"command tool"
    );
    let (_, tool_owner) =
        read_json(&next.steamcmd_root.join(".langame-steamcmd-owner.json")).unwrap();
    assert_eq!(
        PathBuf::from(tool_owner["canonical_root"].as_str().unwrap()),
        fs::canonicalize(&next.steamcmd_root).unwrap()
    );
    let row = sqlx::query("SELECT * FROM instance_archives WHERE archive_id=?1")
        .bind(&archive_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    crate::instance_archive::validate_archived_files(&next, &store::map_archive(&row).unwrap())
        .unwrap();
    let restored = crate::restore_instance_archive(&next, &archive_id)
        .await
        .unwrap();
    assert_eq!(
        Path::new(&restored.restored_instance_root),
        next.instances_root.join("one")
    );
    assert_eq!(
        fs::read(next.instances_root.join("one/saves/world.dat")).unwrap(),
        b"preserved world payload"
    );
    assert_eq!(
        fs::read(next.instances_root.join("one/runtime/server.exe")).unwrap(),
        b"preserved server payload"
    );
    pool.close().await;
}

#[tokio::test]
async fn retired_steam_history_allows_games_migration() {
    assert_retired_history_allows_migration(Role::Games).await;
}

#[tokio::test]
async fn retired_steam_history_allows_instances_migration() {
    assert_retired_history_allows_migration(Role::Instances).await;
}

#[tokio::test]
async fn retired_steam_history_allows_archives_migration() {
    assert_retired_history_allows_migration(Role::Archives).await;
}

#[tokio::test]
async fn retired_steam_history_allows_steamcmd_migration() {
    assert_retired_history_allows_migration(Role::SteamCmd).await;
}

#[tokio::test]
async fn active_steam_retention_blocks_every_storage_role_without_moving_files() {
    for role in [Role::Games, Role::Instances, Role::Archives, Role::SteamCmd] {
        let fixture = Fixture::new();
        let journal = fixture
            .paths
            .games_root
            .join(format!(".langame-steam-retained-{}", "a".repeat(64)));
        fs::create_dir_all(&journal).unwrap();
        fs::write(journal.join("native-started"), b"1\n").unwrap();
        // Even a restored failed update retains this active slot until retry.
        fs::write(journal.join("native-stopped"), b"failed\n").unwrap();
        fs::write(journal.join("restored"), b"1\n").unwrap();
        let (requested, source, target) = selection(&fixture, role);
        let error = migration::preview(
            &fixture.paths,
            requested,
            uuid::Uuid::new_v4().to_string(),
            Vec::new(),
            Arc::new(AtomicBool::new(false)),
            Arc::new(|_| {}),
        )
        .await
        .err()
        .expect("active recovery must block migration");
        assert!(
            error
                .to_string()
                .contains("unfinished data or program recovery"),
            "{error}"
        );
        assert!(source.is_dir());
        assert!(!target.exists());
        assert!(!fixture.paths.settings_path.exists());
        assert_eq!(
            fs::read(journal.join("native-stopped")).unwrap(),
            b"failed\n"
        );
    }
}
