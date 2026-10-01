use super::*;
use crate::{GameInstallSyncRecord, initialize_database, sync_game_installs};
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use std::fs;

const INITIAL: &str = include_str!("../../../migrations/0001_initial.sql");
const OWNERSHIP: &str = include_str!("../../../migrations/0002_program_ownership.sql");

async fn old_database(configs: &[&str]) -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql(INITIAL).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO modules (id, name, version) VALUES ('game', 'Game', '1')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO game_installs (id, module_id, install_root, install_state, current_version, last_verified_at) VALUES (1, 'game', 'D:/library', 'installed', 'latest-library', CURRENT_TIMESTAMP)")
        .execute(&pool).await.unwrap();
    for (index, config) in configs.iter().enumerate() {
        sqlx::query("INSERT INTO instances (id, name, module_id, config_path, data_path, logs_path, saves_path, install_id) VALUES (?1, ?1, 'game', ?2, 'original-data', 'original-logs', 'original-saves', ?3)")
            .bind(index.to_string()).bind(config).bind((index < 2).then_some(1_i64))
            .execute(&pool).await.unwrap();
    }
    pool
}

async fn apply_ownership(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    if let Err(error) = sqlx::raw_sql(OWNERSHIP).execute(&mut *tx).await {
        tx.rollback().await?;
        return Err(error);
    }
    tx.commit().await
}

#[tokio::test]
async fn migration_registers_existing_copies_without_claiming_the_library_version() {
    let pool = old_database(&[
        r"D:\instances\first\config",
        "D:/instances/second/config",
        "/instances/third/config",
    ])
    .await;
    apply_ownership(&pool).await.unwrap();
    let rows = sqlx::query("SELECT i.id, i.runtime_mode, i.saves_path, g.* FROM instances i JOIN game_installs g ON g.id = i.install_id ORDER BY i.id")
        .fetch_all(&pool).await.unwrap();
    let roots = [
        r"D:\instances\first\runtime",
        "D:/instances/second/runtime",
        "/instances/third/runtime",
    ];
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(row.get::<String, _>("scope"), "instance");
        assert_eq!(row.get::<String, _>("runtime_mode"), "independent");
        assert_eq!(row.get::<String, _>("owner_instance_id"), index.to_string());
        assert_eq!(row.get::<String, _>("install_root"), roots[index]);
        assert_eq!(row.get::<String, _>("install_state"), "incomplete");
        assert_eq!(row.get::<String, _>("saves_path"), "original-saves");
        assert!(row.get::<Option<String>, _>("current_version").is_none());
        assert!(row.get::<Option<String>, _>("last_verified_at").is_none());
    }
    assert_eq!(rows.len(), 3);
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT current_version FROM game_installs WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap(),
        "latest-library"
    );
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&pool)
            .await
            .unwrap()
            .is_empty()
    );
    pool.close().await;
}

#[tokio::test]
async fn migration_rejects_unrecognized_paths_and_rolls_back_all_changes() {
    for configs in [
        vec!["config"],
        vec!["relative/config"],
        vec!["D:/instances/one/../config"],
        vec!["D:/instances/one/config/"],
        vec!["D:/instances/ONE/config", r"d:\instances\one\config"],
        vec!["/instances/one/./config"],
        vec!["/instances/one/config\0"],
    ] {
        let pool = old_database(&configs).await;
        let error = apply_ownership(&pool).await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("instance_config_path_must_end_in_config"),
            "{error}"
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("PRAGMA user_version")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM game_installs")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM pragma_table_info('game_installs') WHERE name = 'scope'"
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );
        let preserved: Vec<String> =
            sqlx::query_scalar("SELECT config_path FROM instances ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(preserved, configs);
        pool.close().await;
    }
}

#[tokio::test]
async fn deleting_an_instance_removes_only_its_installation_metadata() {
    let pool = old_database(&["/instances/one/config", "/instances/two/config"]).await;
    apply_ownership(&pool).await.unwrap();
    sqlx::query("DELETE FROM instances WHERE id = '0'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM game_installs WHERE scope = 'instance'")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM game_installs WHERE scope = 'library'")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&pool)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        sqlx::query("UPDATE game_installs SET scope = 'instance' WHERE id = 1")
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE instances SET runtime_mode = 'unknown'")
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(sqlx::query("INSERT INTO game_installs (module_id,install_root,scope,owner_instance_id) VALUES ('game','/duplicate','instance','1')").execute(&pool).await.is_err());
    pool.close().await;
}

struct Fixture {
    root: PathBuf,
    paths: StoragePaths,
    pool: SqlitePool,
}

impl Fixture {
    async fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "langame-program-ownership-{}",
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
        Self { root, paths, pool }
    }

    async fn instance(&self, id: &str) -> PathBuf {
        let root = self.paths.instances_root.join(id);
        fs::create_dir_all(root.join("config")).unwrap();
        sqlx::query("INSERT INTO instances (id,name,module_id,config_path,data_path,logs_path,saves_path) VALUES (?1,?1,'game',?2,?3,?4,?5)")
            .bind(id).bind(root.join("config").to_string_lossy().as_ref())
            .bind(root.join("data").to_string_lossy().as_ref()).bind(root.join("logs").to_string_lossy().as_ref())
            .bind(root.join("data/save").to_string_lossy().as_ref()).execute(&self.pool).await.unwrap();
        root.join("runtime")
    }

    async fn library(&self, name: &str) -> ProgramInstallRecord {
        let root = self.paths.games_root.join(name);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("server.bin"), b"original server payload").unwrap();
        sync_game_installs(&self.paths, &[update(&root, InstallState::Installed)])
            .await
            .unwrap();
        read_library_program_install(&self.paths, "game")
            .await
            .unwrap()
            .unwrap()
    }

    async fn close(self) {
        self.pool.close().await;
        assert!(self.root.starts_with(std::env::temp_dir()));
        fs::remove_dir_all(self.root).unwrap();
    }
}

fn update(root: &Path, install_state: InstallState) -> GameInstallSyncRecord {
    GameInstallSyncRecord {
        module_id: "game".into(),
        install_root: root.to_string_lossy().into_owned(),
        install_state,
        current_version: Some("version-1".into()),
        mark_verified: true,
    }
}

#[tokio::test]
async fn a_missing_library_keeps_its_registered_location_without_becoming_an_installed_program() {
    let f = Fixture::new().await;
    let library = f.library("custom-location").await;
    let independent = f.instance("independent").await;
    fs::create_dir(&independent).unwrap();
    let mut tx = f.pool.begin().await.unwrap();
    register_instance_install(
        &mut tx,
        "game",
        "independent",
        &independent,
        InstallState::Installed,
        Some("instance-version"),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    fs::remove_dir_all(&library.install_root).unwrap();
    sync_game_installs(
        &f.paths,
        &[update(&library.install_root, InstallState::NotInstalled)],
    )
    .await
    .unwrap();
    let recorded = read_library_program_install(&f.paths, "game")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(recorded.id, library.id);
    assert_eq!(recorded.install_root, library.install_root);
    assert_eq!(recorded.scope, ProgramInstallScope::Library);
    assert!(recorded.owner_instance_id.is_none());
    assert!(matches!(recorded.install_state, InstallState::NotInstalled));
    assert!(
        crate::storage_db::resolve_module_install_root_from_pool(&f.pool, "game")
            .await
            .unwrap()
            .is_none()
    );
    let owned = read_instance_program_install(&f.paths, "independent")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(owned.install.install_root, independent);
    assert!(matches!(
        owned.install.install_state,
        InstallState::Installed
    ));
    f.close().await;
}

#[tokio::test]
async fn library_targets_allow_separate_roots_and_reject_managed_instance_ancestors_or_children() {
    let f = Fixture::new().await;
    let library = f.library("registered-library").await;
    for target in [library.install_root, f.root.join("new-custom-library")] {
        ensure_library_program_target_available(&f.paths, &target)
            .await
            .unwrap();
    }
    for target in [
        f.paths.instances_root.clone(),
        f.paths.instances_root.join("future-instance/runtime"),
        f.root.clone(),
    ] {
        assert!(
            ensure_library_program_target_available(&f.paths, &target)
                .await
                .is_err(),
            "unsafe target accepted: {}",
            target.display()
        );
    }
    f.close().await;
}

#[tokio::test]
async fn library_targets_reject_another_modules_owned_program_outside_the_managed_instance_root() {
    let f = Fixture::new().await;
    let external_instance = f.root.join("external-owner");
    let program = external_instance.join("runtime");
    fs::create_dir_all(external_instance.join("config")).unwrap();
    fs::create_dir(&program).unwrap();
    fs::write(
        program.join("server.bin"),
        b"another module's owned program",
    )
    .unwrap();
    sqlx::query("INSERT INTO instances (id,name,module_id,config_path,data_path,logs_path,saves_path) VALUES ('external','External','other',?1,?2,?3,?4)")
        .bind(external_instance.join("config").to_string_lossy().as_ref())
        .bind(external_instance.join("data").to_string_lossy().as_ref())
        .bind(external_instance.join("logs").to_string_lossy().as_ref())
        .bind(external_instance.join("saves").to_string_lossy().as_ref()).execute(&f.pool).await.unwrap();
    let mut tx = f.pool.begin().await.unwrap();
    register_instance_install(
        &mut tx,
        "other",
        "external",
        &program,
        InstallState::Installed,
        None,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    for target in [
        &external_instance,
        &program,
        &program.join("nested-library"),
    ] {
        assert!(
            ensure_library_program_target_available(&f.paths, target)
                .await
                .is_err(),
            "another owner was accepted: {}",
            target.display()
        );
    }
    ensure_library_program_target_available(&f.paths, &f.root.join("external-owner-neighbor"))
        .await
        .unwrap();
    assert_eq!(
        fs::read(program.join("server.bin")).unwrap(),
        b"another module's owned program"
    );
    let registration = read_instance_program_install(&f.paths, "external")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(registration.install.module_id, "other");
    assert_eq!(
        registration.install.owner_instance_id.as_deref(),
        Some("external")
    );
    f.close().await;
}

#[tokio::test]
async fn library_refresh_never_rebinds_independent_or_shared_instances() {
    let f = Fixture::new().await;
    let library = f.library("original").await;
    let owned = f.instance("owned").await;
    f.instance("shared").await;
    let mut tx = f.pool.begin().await.unwrap();
    let owned_id = register_instance_install(
        &mut tx,
        "game",
        "owned",
        &owned,
        InstallState::Installed,
        Some("custom-version"),
    )
    .await
    .unwrap();
    bind_shared_install_to_instance(&mut tx, "game", library.id, "shared")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let newer = f.library("newer").await;
    sync_game_installs(
        &f.paths,
        &[update(&library.install_root, InstallState::NotInstalled)],
    )
    .await
    .unwrap();
    let independent = read_instance_program_install(&f.paths, "owned")
        .await
        .unwrap()
        .unwrap();
    let shared = read_instance_program_install(&f.paths, "shared")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(independent.install.id, owned_id);
    assert_eq!(
        independent.install.current_version.as_deref(),
        Some("custom-version")
    );
    assert_eq!(shared.install.id, library.id);
    assert_eq!(
        read_library_program_install(&f.paths, "game")
            .await
            .unwrap()
            .unwrap()
            .id,
        newer.id
    );
    assert_eq!(
        read_module_instance_installs(&f.paths, "game")
            .await
            .unwrap()
            .len(),
        2
    );
    f.close().await;
}

#[tokio::test]
async fn adoption_and_binding_are_transactional_without_touching_payload_bytes() {
    let f = Fixture::new().await;
    let library = f.library("original").await;
    let runtime = f.instance("one").await;
    fs::rename(&library.install_root, &runtime).unwrap();
    let mut tx = f.pool.begin().await.unwrap();
    adopt_library_install_for_instance(&mut tx, "game", library.id, "one", &runtime)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    assert!(
        read_instance_program_install(&f.paths, "one")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        read_library_program_install(&f.paths, "game")
            .await
            .unwrap()
            .is_some()
    );
    let mut tx = f.pool.begin().await.unwrap();
    adopt_library_install_for_instance(&mut tx, "game", library.id, "one", &runtime)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let record = read_instance_program_install(&f.paths, "one")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.runtime_mode, "independent");
    assert_eq!(record.install.owner_instance_id.as_deref(), Some("one"));
    assert!(
        read_library_program_install(&f.paths, "game")
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        fs::read(runtime.join("server.bin")).unwrap(),
        b"original server payload"
    );
    assert_eq!(
        read_program_install_owner(&f.paths, &runtime)
            .await
            .unwrap()
            .unwrap()
            .id,
        library.id
    );
    assert!(
        sync_game_installs(&f.paths, &[update(&runtime, InstallState::Installed)])
            .await
            .is_err()
    );
    f.close().await;
}

#[tokio::test]
async fn an_existing_reference_prevents_adoption_or_ownership_reassignment() {
    let f = Fixture::new().await;
    let library = f.library("original").await;
    f.instance("shared").await;
    let target = f.instance("target").await;
    let mut tx = f.pool.begin().await.unwrap();
    bind_shared_install_to_instance(&mut tx, "game", library.id, "shared")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let mut tx = f.pool.begin().await.unwrap();
    let error = adopt_library_install_for_instance(&mut tx, "game", library.id, "target", &target)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("another instance"));
    tx.rollback().await.unwrap();
    assert_eq!(
        read_instance_program_install(&f.paths, "shared")
            .await
            .unwrap()
            .unwrap()
            .install
            .id,
        library.id
    );
    assert!(
        read_instance_program_install(&f.paths, "target")
            .await
            .unwrap()
            .is_none()
    );
    f.close().await;
}

#[tokio::test]
async fn wrong_module_or_runtime_path_cannot_claim_an_installation() {
    let f = Fixture::new().await;
    let library = f.library("original").await;
    let runtime = f.instance("one").await;
    let other_runtime = f.instance("two").await;
    let mut tx = f.pool.begin().await.unwrap();
    assert!(
        register_instance_install(
            &mut tx,
            "game",
            "one",
            &other_runtime,
            InstallState::Installed,
            None
        )
        .await
        .is_err()
    );
    assert!(
        register_instance_install(
            &mut tx,
            "other",
            "one",
            &runtime,
            InstallState::Installed,
            None
        )
        .await
        .is_err()
    );
    assert!(
        bind_shared_install_to_instance(&mut tx, "other", library.id, "one")
            .await
            .is_err()
    );
    tx.rollback().await.unwrap();
    assert!(
        read_instance_program_install(&f.paths, "one")
            .await
            .unwrap()
            .is_none()
    );
    f.close().await;
}

#[tokio::test]
async fn explicit_instance_state_updates_preserve_owner_and_reject_other_roots() {
    let f = Fixture::new().await;
    let runtime = f.instance("one").await;
    let mut tx = f.pool.begin().await.unwrap();
    let id = register_instance_install(
        &mut tx,
        "game",
        "one",
        &runtime,
        InstallState::Incomplete,
        None,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert!(
        sync_instance_game_install(
            &f.paths,
            "one",
            &update(&f.paths.games_root.join("other"), InstallState::Installed)
        )
        .await
        .is_err()
    );
    sync_instance_game_install(&f.paths, "one", &update(&runtime, InstallState::Installed))
        .await
        .unwrap();
    let record = read_instance_program_install(&f.paths, "one")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.install.id, id);
    assert_eq!(record.install.scope, ProgramInstallScope::Instance);
    assert_eq!(record.install.current_version.as_deref(), Some("version-1"));
    assert_eq!(record.install.install_state, InstallState::Installed);
    f.close().await;
}

#[tokio::test]
async fn detaching_shared_registration_does_not_modify_the_library() {
    let f = Fixture::new().await;
    let library = f.library("original").await;
    let runtime = f.instance("one").await;
    let mut tx = f.pool.begin().await.unwrap();
    bind_shared_install_to_instance(&mut tx, "game", library.id, "one")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let mut tx = f.pool.begin().await.unwrap();
    let owned_id = register_instance_install(
        &mut tx,
        "game",
        "one",
        &runtime,
        InstallState::Installed,
        None,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_ne!(owned_id, library.id);
    assert_eq!(
        read_library_program_install(&f.paths, "game")
            .await
            .unwrap()
            .unwrap()
            .id,
        library.id
    );
    assert_eq!(
        fs::read(library.install_root.join("server.bin")).unwrap(),
        b"original server payload"
    );
    assert_eq!(
        read_instance_program_install(&f.paths, "one")
            .await
            .unwrap()
            .unwrap()
            .runtime_mode,
        "independent"
    );
    f.close().await;
}
