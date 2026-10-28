use super::*;
use app_core::scum_custom_zones::{ScumNpcVehicleDamage, ScumZoneHandling};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::fs;
use std::path::Path;

const ID: &str = "scum-zone-test";
const ORIGINAL: i64 = (1i64 << 55) | 0x155;

struct Fixture {
    root: PathBuf,
    paths: StoragePaths,
    context: ZoneContext,
}

impl Fixture {
    async fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lgsm-scum-zones-{}", uuid::Uuid::new_v4()));
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let paths = StoragePaths {
            app_data_root: root.join("appdata"),
            settings_path: root.join("appdata/settings.json"),
            database_path: root.join("appdata/database.sqlite"),
            logs_root: root.join("logs"),
            modules_root: repo.join("modules"),
            migrations_root: repo.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances/.trash"),
        };
        let instance = paths.instances_root.join(ID);
        let install = instance.join("runtime");
        let saves = install.join("SCUM/Saved");
        let database = saves.join("SaveFiles/SCUM.db");
        for directory in [
            &paths.app_data_root,
            &paths.games_root,
            &instance.join("config"),
            database.parent().unwrap(),
        ] {
            fs::create_dir_all(directory).unwrap();
        }
        fs::write(install.join(".langame-private-runtime"), b"managed\n").unwrap();
        fs::write(
            instance.join("config/instance.json"),
            br#"{"settings":{},"ports":[]}"#,
        )
        .unwrap();
        crate::initialize_database(&paths).await.unwrap();
        let descriptor = app_modules::discover_modules(&paths.modules_root)
            .unwrap()
            .into_iter()
            .find(|module| module.summary.id == "scum")
            .unwrap();
        crate::sync_modules(&paths, &[descriptor]).await.unwrap();
        let pool = connect_pool(&paths).await.unwrap();
        sqlx::query("INSERT INTO instances(id,name,module_id,data_path,config_path,logs_path,saves_path,runtime_mode,status) VALUES(?1,'Zones','scum',?2,?3,?4,?5,'independent','stopped')")
            .bind(ID).bind(instance.join("data").to_string_lossy().as_ref())
            .bind(instance.join("config").to_string_lossy().as_ref())
            .bind(instance.join("logs").to_string_lossy().as_ref()).bind(saves.to_string_lossy().as_ref())
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO game_installs(module_id,install_root,install_state,scope,owner_instance_id) VALUES('scum',?1,'installed','instance',?2)")
            .bind(install.to_string_lossy().as_ref()).bind(ID).execute(&pool).await.unwrap();
        sqlx::query("UPDATE instances SET install_id=(SELECT id FROM game_installs WHERE owner_instance_id=?1) WHERE id=?1")
            .bind(ID).execute(&pool).await.unwrap();
        pool.close().await;
        let mut native = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(&database)
                .create_if_missing(true),
        )
        .await
        .unwrap();
        sqlx::raw_sql(include_str!(
            "../../../modules/scum/custom-zones/fixture.sql"
        ))
        .execute(&mut native)
        .await
        .unwrap();
        native.close().await.unwrap();
        Self {
            root,
            paths,
            context: ZoneContext {
                database,
                install,
                stopped: true,
            },
        }
    }

    async fn connection(&self) -> SqliteConnection {
        SqliteConnection::connect_with(
            &SqliteConnectOptions::new().filename(&self.context.database),
        )
        .await
        .unwrap()
    }

    async fn input(&self) -> WriteScumCustomZoneDamageInput {
        let snapshot = read_snapshot(ID, &self.context).await.unwrap();
        WriteScumCustomZoneDamageInput {
            instance_id: ID.into(),
            database_identity: snapshot.database_identity.unwrap(),
            expected_revision: snapshot.revision.unwrap(),
            map_id: 1,
            configuration_id: 1,
            damage: ScumNpcVehicleDamage {
                melee: ScumZoneHandling::Block,
                melee_weapon: ScumZoneHandling::Ignore,
                projectile: ScumZoneHandling::Allow,
            },
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn sqlite_write_changes_only_three_slots_and_preserves_unknown_world_and_regions() {
    let fixture = Fixture::new().await;
    verify_stopped_write_and_backup(&fixture).await;
}

#[cfg(windows)]
#[tokio::test]
async fn sqlite_write_backs_up_case_variant_save_directory() {
    let fixture = Fixture::new().await;
    // Imported saves can retain a different spelling on the same Windows path.
    // Change the real directory entry while keeping the stored instance path.
    let directory = fixture.context.database.parent().unwrap();
    let intermediate = directory.with_file_name("save-directory-case-change");
    fs::rename(directory, &intermediate).unwrap();
    fs::rename(&intermediate, directory.with_file_name("savefiles")).unwrap();
    verify_stopped_write_and_backup(&fixture).await;
}

#[cfg(windows)]
#[tokio::test]
async fn sqlite_connections_preserve_committed_wal_at_a_long_windows_path() {
    use std::os::windows::ffi::OsStrExt;

    let fixture = Fixture::new().await;
    let database = fixture
        .root
        .join("a".repeat(120))
        .join("b".repeat(120))
        .join("SCUM.db");
    let path_characters = database.as_os_str().encode_wide().count();
    assert!(path_characters > 260);
    fs::create_dir_all(database.parent().unwrap()).unwrap();
    fs::copy(&fixture.context.database, &database).unwrap();
    assert!(!fs::read(&database).unwrap().is_empty());
    let anchor = DatabaseAnchor::open(&database, true).unwrap();
    let mut writer = db::connect(&anchor, true).await.unwrap_or_else(|error| {
        panic!("SQLite rejected a {path_characters}-character path readable by std::fs: {error}")
    });
    sqlx::query("INSERT INTO retained_world VALUES(3,X'1234')")
        .execute(&mut writer)
        .await
        .unwrap();
    assert!(
        fs::metadata(database.with_file_name("SCUM.db-wal"))
            .unwrap()
            .len()
            > 0
    );
    let anchor = DatabaseAnchor::open(&database, false).unwrap();
    let mut reader = db::connect(&anchor, false).await.unwrap();
    let committed: Vec<u8> =
        sqlx::query_scalar("SELECT unknown_data FROM retained_world WHERE id=3")
            .fetch_one(&mut reader)
            .await
            .unwrap();
    assert_eq!(committed, [18, 52]);
    reader.close().await.unwrap();
    writer.close().await.unwrap();
}

async fn verify_stopped_write_and_backup(fixture: &Fixture) {
    // Keep a real WAL connection alive: the backup must retain committed WAL
    // content while omitting only SQLite's locked, reconstructable WAL index.
    let mut retained = fixture.connection().await;
    sqlx::query("INSERT INTO retained_world VALUES(3,X'1234')")
        .execute(&mut retained)
        .await
        .unwrap();
    let unrelated_sidecar = fixture.context.database.with_file_name("audit.db-shm");
    fs::write(&unrelated_sidecar, b"retained unrelated sidecar").unwrap();
    let nested_sidecar = fixture
        .context
        .database
        .parent()
        .unwrap()
        .join("nested/SCUM.db-shm");
    fs::create_dir_all(nested_sidecar.parent().unwrap()).unwrap();
    fs::write(&nested_sidecar, b"retained nested sidecar").unwrap();
    let input = fixture.input().await;
    let result = write_stopped(&fixture.paths, &fixture.context, input)
        .await
        .unwrap();
    let mut connection = fixture.connection().await;
    let value: i64 = sqlx::query_scalar("SELECT damage_handling_methods FROM custom_zone_configuration_damage_handling_methods WHERE id=1").fetch_one(&mut connection).await.unwrap();
    assert_eq!(value, (ORIGINAL & !0x3f0) | 0x120);
    assert_eq!(value & !0x3f0, ORIGINAL & !0x3f0);
    let other: Vec<(i64, i64)> = sqlx::query_as("SELECT id,damage_handling_methods FROM custom_zone_configuration_damage_handling_methods WHERE id<>1 ORDER BY id").fetch_all(&mut connection).await.unwrap();
    assert_eq!(other, vec![(2, 357913941), (3, 341), (4, 341), (5, 341)]);
    let native: (i64, Vec<u8>) = sqlx::query_as("SELECT num_damage_handling_methods,future_column FROM custom_zone_configuration_damage_handling_methods WHERE id=1").fetch_one(&mut connection).await.unwrap();
    assert_eq!(native, (5, vec![0, 1, 2, 255]));
    let region: (f64, f64, String) = sqlx::query_as(
        "SELECT location_x,location_y,future_column FROM custom_zone_region WHERE id=1",
    )
    .fetch_one(&mut connection)
    .await
    .unwrap();
    assert_eq!(region, (123.5, -456.25, "preserve-region".into()));
    let world: Vec<(i64, Vec<u8>)> =
        sqlx::query_as("SELECT id,unknown_data FROM retained_world ORDER BY id")
            .fetch_all(&mut connection)
            .await
            .unwrap();
    assert_eq!(
        world,
        vec![
            (1, vec![0, 1, 254, 255]),
            (2, vec![85, 102, 119, 136]),
            (3, vec![18, 52])
        ]
    );
    connection.close().await.unwrap();
    let reloaded = read_snapshot(ID, &fixture.context).await.unwrap();
    assert_eq!(result.revision, reloaded.revision);
    assert_eq!(
        result.configurations[0].regions,
        ["Workshop north", "Workshop south"]
    );
    let backups = crate::list_instance_backups(&fixture.paths, ID)
        .await
        .unwrap();
    assert_eq!(backups.len(), 1);
    assert_eq!(result.backup_id.as_ref(), Some(&backups[0].backup_id));
    // Read the real pre-change WAL-backed backup, not a reconstructed mock.
    let saved = Path::new(&backups[0].backup_path).join("saves/SaveFiles/SCUM.db");
    assert!(!saved.with_file_name("SCUM.db-shm").exists());
    assert!(
        fs::metadata(saved.with_file_name("SCUM.db-wal"))
            .unwrap()
            .len()
            > 0
    );
    assert_eq!(
        fs::read(saved.with_file_name("audit.db-shm")).unwrap(),
        b"retained unrelated sidecar"
    );
    assert_eq!(
        fs::read(saved.parent().unwrap().join("nested/SCUM.db-shm")).unwrap(),
        b"retained nested sidecar"
    );
    let anchor = DatabaseAnchor::open(&saved, false).unwrap();
    let mut backup = db::connect(&anchor, false).await.unwrap();
    let original: i64 = sqlx::query_scalar("SELECT damage_handling_methods FROM custom_zone_configuration_damage_handling_methods WHERE id=1").fetch_one(&mut backup).await.unwrap();
    assert_eq!(original, ORIGINAL);
    let wal_only: Vec<u8> =
        sqlx::query_scalar("SELECT unknown_data FROM retained_world WHERE id=3")
            .fetch_one(&mut backup)
            .await
            .unwrap();
    assert_eq!(wal_only, [18, 52]);
    backup.close().await.unwrap();
    retained.close().await.unwrap();
}

#[tokio::test]
async fn stale_revision_or_replaced_database_is_rejected_before_backup() {
    let fixture = Fixture::new().await;
    let input = fixture.input().await;
    let mut connection = fixture.connection().await;
    sqlx::query("UPDATE custom_zone_configuration_damage_handling_methods SET damage_handling_methods=9 WHERE id=2").execute(&mut connection).await.unwrap();
    connection.close().await.unwrap();
    assert!(
        write_stopped(&fixture.paths, &fixture.context, input)
            .await
            .is_err()
    );
    let input = fixture.input().await;
    let path = &fixture.context.database;
    fs::rename(path, path.with_extension("original-db")).unwrap();
    fs::copy(path.with_extension("original-db"), path).unwrap();
    assert!(
        write_stopped(&fixture.paths, &fixture.context, input)
            .await
            .is_err()
    );
    assert!(
        crate::list_instance_backups(&fixture.paths, ID)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn unchanged_values_do_not_create_a_backup() {
    let fixture = Fixture::new().await;
    let mut input = fixture.input().await;
    input.damage = ScumNpcVehicleDamage {
        melee: ScumZoneHandling::Allow,
        melee_weapon: ScumZoneHandling::Allow,
        projectile: ScumZoneHandling::Allow,
    };
    let result = write_stopped(&fixture.paths, &fixture.context, input)
        .await
        .unwrap();
    assert!(result.backup_id.is_none());
    assert!(
        crate::list_instance_backups(&fixture.paths, ID)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn readonly_corrupt_short_and_unknown_formats_preserve_data() {
    let fixture = Fixture::new().await;
    let input = fixture.input().await;
    let permissions = fs::metadata(&fixture.context.database)
        .unwrap()
        .permissions();
    let mut readonly = permissions.clone();
    readonly.set_readonly(true);
    fs::set_permissions(&fixture.context.database, readonly).unwrap();
    assert!(
        write_stopped(&fixture.paths, &fixture.context, input)
            .await
            .is_err()
    );
    fs::set_permissions(&fixture.context.database, permissions).unwrap();
    let mut connection = fixture.connection().await;
    sqlx::query("UPDATE custom_zone_configuration_damage_handling_methods SET num_damage_handling_methods=2 WHERE id=1").execute(&mut connection).await.unwrap();
    connection.close().await.unwrap();
    assert!(read_snapshot(ID, &fixture.context).await.is_err());
    let mut connection = fixture.connection().await;
    sqlx::query("PRAGMA user_version=74")
        .execute(&mut connection)
        .await
        .unwrap();
    connection.close().await.unwrap();
    assert!(read_snapshot(ID, &fixture.context).await.is_err());
    fs::write(&fixture.context.database, b"not a SQLite database").unwrap();
    assert!(read_snapshot(ID, &fixture.context).await.is_err());
    assert_eq!(
        fs::read(&fixture.context.database).unwrap(),
        b"not a SQLite database"
    );
    assert!(
        crate::list_instance_backups(&fixture.paths, ID)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn running_instances_and_unverified_images_are_not_admitted() {
    let fixture = Fixture::new().await;
    let input = fixture.input().await;
    assert_eq!(
        read_scum_custom_zones(&fixture.paths, ID)
            .await
            .unwrap()
            .status,
        ScumCustomZonesStatus::Unsupported
    );
    assert!(
        write_scum_custom_zone_damage(&fixture.paths, input)
            .await
            .is_err()
    );
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("UPDATE instances SET status='running' WHERE id=?1")
        .bind(ID)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    assert_eq!(
        read_scum_custom_zones(&fixture.paths, ID)
            .await
            .unwrap()
            .status,
        ScumCustomZonesStatus::Running
    );
    assert!(
        crate::list_instance_backups(&fixture.paths, ID)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
#[ignore = "opt-in: copies a stopped disposable native world and verified executable; never starts a server or edits the source"]
async fn native_schema_roundtrip_on_owned_copy() {
    let source = PathBuf::from(
        std::env::var_os("LANGAME_SCUM_ZONE_NATIVE_ROOT").expect("owned native fixture root"),
    );
    assert!(source.is_absolute());
    let source_db = source.join("SCUM/Saved/SaveFiles/SCUM.db");
    let source_wal = source_db.with_file_name("SCUM.db-wal");
    let original_db = crate::instance_file_patch::sha256(&fs::read(&source_db).unwrap());
    let original_wal = source_wal
        .try_exists()
        .unwrap()
        .then(|| crate::instance_file_patch::sha256(&fs::read(&source_wal).unwrap()));
    let fixture = Fixture::new().await;
    assert!(!fixture.root.starts_with(&source) && !source.starts_with(&fixture.root));
    fs::remove_file(&fixture.context.database).unwrap();
    fs::copy(&source_db, &fixture.context.database).unwrap();
    if source_wal.exists() {
        fs::copy(
            &source_wal,
            fixture.context.database.with_file_name("SCUM.db-wal"),
        )
        .unwrap();
    }
    let image = fixture
        .context
        .install
        .join("SCUM/Binaries/Win64/SCUMServer.exe");
    fs::create_dir_all(image.parent().unwrap()).unwrap();
    fs::copy(source.join("SCUM/Binaries/Win64/SCUMServer.exe"), image).unwrap();

    // Exercise public instance/path/stopped-state and exact native image gates.
    let before = read_scum_custom_zones(&fixture.paths, ID).await.unwrap();
    assert_eq!(before.status, ScumCustomZonesStatus::Ready);
    assert!(before.writable);
    let zone = before
        .configurations
        .iter()
        .find(|entry| !entry.global)
        .unwrap();
    let rotate = |value| match value {
        ScumZoneHandling::Ignore => ScumZoneHandling::Allow,
        ScumZoneHandling::Allow => ScumZoneHandling::Block,
        ScumZoneHandling::Block => ScumZoneHandling::Ignore,
    };
    let input = WriteScumCustomZoneDamageInput {
        instance_id: ID.into(),
        database_identity: before.database_identity.clone().unwrap(),
        expected_revision: before.revision.clone().unwrap(),
        map_id: zone.map_id,
        configuration_id: zone.configuration_id,
        damage: ScumNpcVehicleDamage {
            melee: rotate(zone.damage.melee),
            melee_weapon: rotate(zone.damage.melee_weapon),
            projectile: rotate(zone.damage.projectile),
        },
    };
    let mut expected = before.configurations.clone();
    expected
        .iter_mut()
        .find(|entry| {
            entry.map_id == input.map_id && entry.configuration_id == input.configuration_id
        })
        .unwrap()
        .damage = input.damage;
    let after = write_scum_custom_zone_damage(&fixture.paths, input)
        .await
        .unwrap();
    assert_eq!(after.configurations, expected);
    assert_ne!(after.revision, before.revision);
    assert!(after.backup_id.is_some());
    let reloaded = read_scum_custom_zones(&fixture.paths, ID).await.unwrap();
    assert_eq!(reloaded.configurations, expected);
    let backups = crate::list_instance_backups(&fixture.paths, ID)
        .await
        .unwrap();
    assert_eq!(backups.len(), 1);
    let saved = Path::new(&backups[0].backup_path).join("saves/SaveFiles/SCUM.db");
    let anchor = DatabaseAnchor::open(&saved, false).unwrap();
    let mut backup = db::connect(&anchor, false).await.unwrap();
    let restored = db::read(&mut backup, &anchor)
        .await
        .unwrap()
        .snapshot(ID, &anchor)
        .unwrap();
    assert_eq!(restored.configurations, before.configurations);
    assert_eq!(restored.revision, before.revision);
    backup.close().await.unwrap();
    assert_eq!(
        crate::instance_file_patch::sha256(&fs::read(source_db).unwrap()),
        original_db
    );
    let remaining_wal = source_wal
        .try_exists()
        .unwrap()
        .then(|| crate::instance_file_patch::sha256(&fs::read(source_wal).unwrap()));
    assert_eq!(remaining_wal, original_wal);
    println!(
        "SCUM_NATIVE_ZONES roundtrip=verified configurations={} backup_before_change=true source=read_only server_started=false",
        after.configurations.len()
    );
}
