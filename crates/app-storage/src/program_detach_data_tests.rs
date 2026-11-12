use super::*;
use crate::program_runtime::prepare_shared_program_reference;
use crate::{
    GameInstallSyncRecord, bind_shared_install_to_instance, initialize_database,
    read_library_program_install, record_library_program_baseline, sync_game_installs,
};

struct DataFixture {
    root: PathBuf,
    paths: StoragePaths,
    pool: sqlx::SqlitePool,
    descriptor: ModuleDescriptor,
    program: PathBuf,
    library_id: i64,
}

impl DataFixture {
    async fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("langame-detach-data-{}", uuid::Uuid::new_v4()));
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
            archives_root: root.join("instances/.trash"),
        };
        fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
        initialize_database(&paths).await.unwrap();
        let pool = connect_pool(&paths).await.unwrap();
        let descriptor = app_modules::discover_modules(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules"),
        )
        .unwrap()
        .into_iter()
        .find(|module| module.summary.id == "palworld")
        .unwrap();
        sqlx::query("INSERT INTO modules (id,name,version) VALUES ('palworld','Palworld','1')")
            .execute(&pool)
            .await
            .unwrap();
        let program = paths.games_root.join("palworld");
        put(&program, "PalServer.exe", b"official executable");
        record_library_program_baseline(&program, &descriptor, true, None).unwrap();
        sync_game_installs(
            &paths,
            &[GameInstallSyncRecord {
                module_id: "palworld".into(),
                install_root: program.to_string_lossy().into_owned(),
                install_state: InstallState::Installed,
                current_version: Some("fixture-build".into()),
                mark_verified: true,
            }],
        )
        .await
        .unwrap();
        let library_id = read_library_program_install(&paths, "palworld")
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
        put(&instance, "config/instance.json", br#"{"settings":{}}"#);
        prepare_shared_program_reference(&self.program, &instance, "palworld").unwrap();
        let mut tx = self.pool.begin().await.unwrap();
        sqlx::query("INSERT INTO instances (id,name,module_id,config_path,data_path,logs_path,saves_path) VALUES (?1,?1,'palworld',?2,?3,?4,?5)")
            .bind(id).bind(instance.join("config").to_string_lossy().as_ref())
            .bind(instance.join("data").to_string_lossy().as_ref())
            .bind(instance.join("logs").to_string_lossy().as_ref())
            .bind(self.program.join(save_relative(id)).to_string_lossy().as_ref())
            .execute(&mut *tx).await.unwrap();
        bind_shared_install_to_instance(&mut tx, "palworld", self.library_id, id)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        put(
            &self.program,
            &format!("{}/world.sentinel", save_relative(id)),
            id.as_bytes(),
        );
        instance
    }

    async fn close(self) {
        self.pool.close().await;
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        fs::remove_dir_all(self.root).unwrap();
    }
}

fn save_relative(id: &str) -> String {
    format!("Pal/Saved/SaveGames/0/{id}")
}

fn put(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

#[tokio::test]
async fn detach_migrates_exact_world_and_native_configuration_without_moving_source_data() {
    let f = DataFixture::new().await;
    let instance = f.shared("first").await;
    for (path, bytes) in [
        (
            "Pal/Saved/Config/WindowsServer/custom.ini",
            b"operator config".as_slice(),
        ),
        (
            "Pal/Binaries/Win64/Mods/PalModSettings.ini",
            b"selected mods".as_slice(),
        ),
        (
            "Pal/Saved/SaveGames/0/retired/world.sentinel",
            b"unrelated old world".as_slice(),
        ),
        (
            "Pal/Binaries/Win64/Mods/Workshop/other/mod.pak",
            b"unselected mod cache".as_slice(),
        ),
    ] {
        put(&f.program, path, bytes);
    }
    let source_before = crate::test_file_snapshot::tree_snapshot(&f.program).unwrap();
    let config_before = fs::read(instance.join("config/instance.json")).unwrap();
    let runtime = detach_instance_program(&f.paths, &f.descriptor, "first", None)
        .await
        .unwrap();
    assert_eq!(
        fs::read(runtime.join(save_relative("first")).join("world.sentinel")).unwrap(),
        b"first"
    );
    assert_eq!(
        fs::read(runtime.join("Pal/Saved/Config/WindowsServer/custom.ini")).unwrap(),
        b"operator config"
    );
    assert_eq!(
        fs::read(runtime.join("Pal/Binaries/Win64/Mods/PalModSettings.ini")).unwrap(),
        b"selected mods"
    );
    assert!(!runtime.join(save_relative("retired")).exists());
    assert!(!runtime.join("Pal/Binaries/Win64/Mods/Workshop").exists());
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&f.program).unwrap(),
        source_before
    );
    assert_eq!(
        fs::read(instance.join("config/instance.json")).unwrap(),
        config_before
    );
    let recorded: String = sqlx::query_scalar("SELECT saves_path FROM instances WHERE id='first'")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        normalize_path(Path::new(&recorded)).unwrap(),
        normalize_path(&runtime.join(save_relative("first"))).unwrap()
    );
    assert!(crate::library_program_is_pristine(&runtime, &f.descriptor, None).unwrap());
    f.close().await;
}

#[tokio::test]
async fn detach_copies_only_the_selected_instance_world_from_a_shared_source() {
    let f = DataFixture::new().await;
    f.shared("first").await;
    let second = f.shared("second").await;
    let source_before = crate::test_file_snapshot::tree_snapshot(&f.program).unwrap();
    let runtime = detach_instance_program(&f.paths, &f.descriptor, "first", None)
        .await
        .unwrap();
    assert_eq!(
        fs::read(runtime.join(save_relative("first")).join("world.sentinel")).unwrap(),
        b"first"
    );
    assert!(!runtime.join(save_relative("second")).exists());
    assert_eq!(
        instance_program_mode(&second).unwrap(),
        InstanceProgramMode::Shared
    );
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&f.program).unwrap(),
        source_before
    );
    f.close().await;
}

#[tokio::test]
async fn detach_rejects_ambiguous_native_data_instead_of_copying_a_peers_configuration() {
    let f = DataFixture::new().await;
    let first = f.shared("first").await;
    f.shared("second").await;
    put(
        &f.program,
        "Pal/Saved/Config/WindowsServer/custom.ini",
        b"ambiguous live config",
    );
    let source_before = crate::test_file_snapshot::tree_snapshot(&f.program).unwrap();
    let error = detach_instance_program(&f.paths, &f.descriptor, "first", None)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("another instance"), "{error}");
    assert_eq!(
        instance_program_mode(&first).unwrap(),
        InstanceProgramMode::Shared
    );
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&f.program).unwrap(),
        source_before
    );
    assert!(!first.join(STAGING).exists());
    f.close().await;
}

#[tokio::test]
async fn detach_data_registration_failure_restores_binding_and_saved_path() {
    let f = DataFixture::new().await;
    let instance = f.shared("first").await;
    let source_before = crate::test_file_snapshot::tree_snapshot(&f.program).unwrap();
    sqlx::raw_sql("CREATE TRIGGER fail_detach_data BEFORE UPDATE OF saves_path ON instances BEGIN SELECT RAISE(ABORT, 'fixture save-path failure'); END;")
        .execute(&f.pool).await.unwrap();
    let error = detach_instance_program(&f.paths, &f.descriptor, "first", None)
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("fixture save-path failure"),
        "{error}"
    );
    assert_eq!(
        instance_program_mode(&instance).unwrap(),
        InstanceProgramMode::Shared
    );
    let recorded: String = sqlx::query_scalar("SELECT saves_path FROM instances WHERE id='first'")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        normalize_path(Path::new(&recorded)).unwrap(),
        normalize_path(&f.program.join(save_relative("first"))).unwrap()
    );
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&f.program).unwrap(),
        source_before
    );
    assert!(!instance.join(STAGING).exists());
    assert!(!instance.join(SHARED_ROLLBACK).exists());
    f.close().await;
}
