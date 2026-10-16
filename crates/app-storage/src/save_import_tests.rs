use super::*;

#[path = "save_import_profile_tests.rs"]
mod profile_tests;

// Minimal independently encoded SPUD metadata plus opaque world content. These
// tests exercise native world selection and byte preservation, not game loading.
fn dragonwilds_import_save(name: &str, saved_at: i64) -> Vec<u8> {
    fn string(value: &str) -> Vec<u8> {
        assert!(value.is_ascii());
        let mut bytes = (value.len() as u32 + 1).to_le_bytes().to_vec();
        bytes.extend_from_slice(value.as_bytes());
        bytes.push(0);
        bytes
    }
    fn chunk(tag: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut bytes = tag.to_vec();
        bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
        bytes.extend_from_slice(body);
        bytes
    }
    let name = string(name);
    let mut custom = 2u32.to_le_bytes().to_vec();
    custom.extend_from_slice(&string("WorldName"));
    custom.extend_from_slice(&string("TimeOfSave"));
    custom.extend_from_slice(&2u32.to_le_bytes());
    for offset in [0, name.len() as u32, name.len() as u32 + 8] {
        custom.extend_from_slice(&offset.to_le_bytes());
    }
    custom.extend_from_slice(&name);
    custom.extend_from_slice(&saved_at.to_le_bytes());
    let mut info = 8u16.to_le_bytes().to_vec();
    info.extend_from_slice(&522u32.to_le_bytes());
    info.extend_from_slice(&1017u32.to_le_bytes());
    info.extend_from_slice(&[0, 0, 0, 0, 255, 0, 0, 0, 0]);
    info.extend_from_slice(&string("2026-10-16T00:00:00.000Z"));
    info.extend_from_slice(&chunk(b"CINF", &custom));
    let mut body = chunk(b"INFO", &info);
    body.extend_from_slice(&chunk(b"GLOB", b"opaque-world-rules-and-players"));
    chunk(b"SAVE", &body)
}

pub(super) struct Fixture {
    pub(super) root: PathBuf,
    pub(super) source: PathBuf,
    pub(super) context: Context,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lgsm-save-import-{}", Uuid::new_v4()));
        let source = root.join("external-world");
        let instance = root.join("instances").join("example");
        let target = instance.join("world");
        for directory in [&source, &target, &instance.join("config")] {
            fs::create_dir_all(directory).unwrap();
        }
        fs::create_dir(source.join("region")).unwrap();
        fs::write(source.join("level.dat"), b"new world").unwrap();
        fs::write(source.join("region/r.0.0.mca"), b"new region").unwrap();
        fs::write(target.join("old-player.dat"), b"old player").unwrap();
        fs::write(
            instance.join("config/instance.json"),
            br#"{"settings":{"level_name":"world"}}"#,
        )
        .unwrap();
        Self {
            source,
            context: Context {
                record: StoredInstanceRecord {
                    summary: app_core::InstanceSummary {
                        id: "example".into(),
                        name: "Example".into(),
                        module_id: "minecraft".into(),
                        status: InstanceStatus::Stopped,
                        active_process_count: 0,
                        bind_ip: "0.0.0.0".into(),
                        port_count: 0,
                        autostart: false,
                    },
                    config_dir: instance.join("config"),
                    saves_dir: target.clone(),
                    auto_backup_on_stop: false,
                    backup_retention_count: 1,
                    runtime_mode: "independent".into(),
                    program_install_root: Some(instance.join("runtime")),
                },
                install: instance.join("runtime"),
                target,
                settings: serde_json::from_value(json!({"level_name":"world"})).unwrap(),
                settings_json: r#"{"level_name":"world"}"#.into(),
                protected: vec![root.join("instances")],
            },
            root,
        }
    }

    pub(super) fn preview(&self) -> InstanceSaveImportPreview {
        preview(&self.context, &self.source).unwrap()
    }
    pub(super) fn import(
        &self,
        fingerprint: &str,
    ) -> Result<InstanceSaveImportResult, StorageError> {
        import_transaction(
            &self.context,
            &self.source,
            fingerprint,
            &publication::Publication::uncoordinated(),
            publish_new_directory,
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn save_import_keeps_source_and_config_and_retains_restorable_safeguard() {
    let fixture = Fixture::new();
    let source = io::inventory(&fixture.source, None).unwrap();
    let config = fs::read(fixture.context.record.config_dir.join("instance.json")).unwrap();
    let result = fixture.import(&fixture.preview().fingerprint).unwrap();
    assert_eq!(result.copied_file_count, 2);
    assert_eq!(io::inventory(&fixture.source, None).unwrap(), source);
    assert_eq!(
        io::inventory(&fixture.context.target, None).unwrap(),
        source
    );
    assert_eq!(
        fs::read(fixture.context.record.config_dir.join("instance.json")).unwrap(),
        config
    );
    let backup = Path::new(&result.safeguard_backup_path);
    assert_eq!(
        fs::read(backup.join("saves/old-player.dat")).unwrap(),
        b"old player"
    );
    assert!(backup.join("backup.json").is_file());
    assert!(!fixture.context.target.join("old-player.dat").exists());
}

#[test]
fn save_import_refuses_source_changes_and_target_changes_after_preview() {
    let fixture = Fixture::new();
    let before = fixture.preview();
    fs::write(fixture.source.join("level.dat"), b"changed source").unwrap();
    assert!(fixture.import(&before.fingerprint).is_err());
    assert_eq!(
        fs::read(fixture.context.target.join("old-player.dat")).unwrap(),
        b"old player"
    );
    let before = fixture.preview();
    fs::write(fixture.context.target.join("old-player.dat"), b"late edit").unwrap();
    assert!(fixture.import(&before.fingerprint).is_err());
    assert_eq!(
        fs::read(fixture.context.target.join("old-player.dat")).unwrap(),
        b"late edit"
    );
}

#[test]
fn save_import_valheim_preserves_target_access_lists_and_ignores_source_lists() {
    let mut fixture = Fixture::new();
    fixture.context.record.summary.module_id = "valheim".into();
    fixture.context.settings = serde_json::from_value(json!({"world_name":"World"})).unwrap();
    fs::create_dir(fixture.source.join("worlds_local")).unwrap();
    fs::write(
        fixture.source.join("worlds_local/World.fwl"),
        b"world metadata",
    )
    .unwrap();
    fs::write(fixture.source.join("worlds_local/World.db"), b"world data").unwrap();
    fs::write(
        fixture.source.join("adminlist.txt"),
        b"source administrator",
    )
    .unwrap();
    fs::write(fixture.source.join("bannedlist.txt"), b"source banned user").unwrap();
    fs::write(
        fixture.context.target.join("adminlist.txt"),
        b"target administrator",
    )
    .unwrap();
    fs::write(
        fixture.context.target.join("permittedlist.txt"),
        b"target allowed user",
    )
    .unwrap();
    fixture.import(&fixture.preview().fingerprint).unwrap();
    assert_eq!(
        fs::read(fixture.context.target.join("adminlist.txt")).unwrap(),
        b"target administrator"
    );
    assert_eq!(
        fs::read(fixture.context.target.join("permittedlist.txt")).unwrap(),
        b"target allowed user"
    );
    assert!(!fixture.context.target.join("bannedlist.txt").exists());
    assert_eq!(
        fs::read(fixture.source.join("adminlist.txt")).unwrap(),
        b"source administrator"
    );
}

#[test]
fn save_import_confirmation_binds_configuration_instance_and_destination() {
    let mut fixture = Fixture::new();
    let preview = fixture.preview();
    fixture.context.settings_json = r#"{"level_name":"second"}"#.into();
    assert!(fixture.import(&preview.fingerprint).is_err());
    fixture.context.settings_json = r#"{"level_name":"world"}"#.into();
    fixture.context.record.summary.id = "another".into();
    assert!(fixture.import(&preview.fingerprint).is_err());
    fixture.context.record.summary.id = "example".into();
    fixture.context.target = fixture.context.target.with_file_name("second");
    assert!(fixture.import(&preview.fingerprint).is_err());
}

#[test]
fn save_import_failed_publish_restores_original_directory() {
    let fixture = Fixture::new();
    let fingerprint = fixture.preview().fingerprint;
    let before = io::inventory(&fixture.context.target, None).unwrap();
    let result = import_transaction(
        &fixture.context,
        &fixture.source,
        &fingerprint,
        &publication::Publication::uncoordinated(),
        |_, target| Err(invalid(target, "injected publication failure")),
    );
    assert!(result.is_err());
    assert_eq!(
        io::inventory(&fixture.context.target, None).unwrap(),
        before
    );
    let backups = collect_instance_backups(&fixture.context.record, "example").unwrap();
    assert_eq!(backups.len(), 1);
    assert!(
        Path::new(&backups[0].backup_path)
            .join("saves/old-player.dat")
            .exists()
    );
}

#[test]
fn save_import_missing_target_still_creates_empty_recovery_backup() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.context.target.join("old-player.dat")).unwrap();
    fs::remove_dir(&fixture.context.target).unwrap();
    let result = fixture.import(&fixture.preview().fingerprint).unwrap();
    assert!(fixture.context.target.join("level.dat").exists());
    assert_eq!(
        io::inventory(
            &Path::new(&result.safeguard_backup_path).join("saves"),
            None
        )
        .unwrap()
        .files,
        0
    );
}

#[test]
fn save_import_rejects_program_config_overlap_running_and_unknown_games() {
    let mut fixture = Fixture::new();
    fs::write(fixture.source.join("server.properties"), b"config").unwrap();
    assert!(preview(&fixture.context, &fixture.source).is_err());
    fs::remove_file(fixture.source.join("server.properties")).unwrap();
    fs::write(fixture.source.join("bad.exe"), b"program").unwrap();
    assert!(preview(&fixture.context, &fixture.source).is_err());
    fs::remove_file(fixture.source.join("bad.exe")).unwrap();
    assert!(preview(&fixture.context, &fixture.context.target).is_err());
    assert!(io::validate_source_path(&fixture.root, &fixture.context.target, &[]).is_err());
    fixture.context.record.summary.status = InstanceStatus::Running;
    assert!(preview(&fixture.context, &fixture.source).is_err());
    fixture.context.record.summary.status = InstanceStatus::Stopped;
    fixture.context.record.summary.module_id = "unsupported-game".into();
    assert!(!profiles::support(&fixture.context).supported);
    assert!(preview(&fixture.context, &fixture.source).is_err());
}

#[test]
fn save_import_valheim_requires_complete_current_world_and_committed_generation() {
    let mut fixture = Fixture::new();
    fixture.context.record.summary.module_id = "valheim".into();
    fixture.context.settings = serde_json::from_value(json!({"world_name":"Named World"})).unwrap();
    let world = fixture.source.join("worlds_local");
    fs::create_dir(&world).unwrap();
    fs::write(world.join("Named World.fwl"), b"metadata").unwrap();
    assert!(preview(&fixture.context, &fixture.source).is_err());
    fs::write(world.join("Named World.db"), b"database").unwrap();
    assert!(preview(&fixture.context, &fixture.source).is_ok());
    fs::create_dir(world.join("Named World")).unwrap();
    fs::write(world.join("Named World/_main.12.fwl2"), b"metadata").unwrap();
    fs::write(world.join("Named World/_main.12.db2"), b"database").unwrap();
    assert!(preview(&fixture.context, &fixture.source).is_err());
    fs::write(world.join("Named World/_main.12.ok"), b"").unwrap();
    assert!(preview(&fixture.context, &fixture.source).is_ok());
    fs::write(world.join("Named World/_main.13.ok"), b"").unwrap();
    assert!(preview(&fixture.context, &fixture.source).is_err());
}

#[test]
fn save_import_astroneer_requires_selected_slot_and_autoload() {
    let mut fixture = Fixture::new();
    fixture.context.record.summary.module_id = "astroneer".into();
    fixture.context.settings =
        serde_json::from_value(json!({"active_save_file_name":"My Save", "load_auto_save":true}))
            .unwrap();
    fs::write(
        fixture.source.join("Other$2026.10.09-23.59.59.savegame"),
        b"world",
    )
    .unwrap();
    assert!(preview(&fixture.context, &fixture.source).is_err());
    fs::write(
        fixture.source.join("My Save$2026.10.09-23.59.59.savegame"),
        b"world",
    )
    .unwrap();
    assert!(preview(&fixture.context, &fixture.source).is_ok());
    fixture
        .context
        .settings
        .insert("load_auto_save".into(), Value::Bool(false));
    assert!(preview(&fixture.context, &fixture.source).is_err());
}

#[test]
fn save_import_abiotic_excludes_external_sandbox_config_and_preserves_current_settings() {
    let mut fixture = Fixture::new();
    fixture.context.record.summary.module_id = "abioticfactor".into();
    for file in ["WorldSave_Facility.sav", "WorldSave_MetaData.sav"] {
        fs::write(fixture.source.join(file), b"native world").unwrap();
    }
    fs::write(
        fixture.source.join("SandboxSettings.ini"),
        b"external settings",
    )
    .unwrap();
    fs::write(
        fixture.context.target.join("SandboxSettings.ini"),
        b"current settings",
    )
    .unwrap();
    let preview = fixture.preview();
    assert!(
        preview
            .warnings
            .iter()
            .any(|message| message.contains("SandboxSettings.ini is excluded"))
    );
    assert_eq!(preview.file_count, 4);
    let result = fixture.import(&preview.fingerprint).unwrap();
    assert_eq!(result.copied_file_count, 4);
    assert_eq!(
        fs::read(fixture.context.target.join("SandboxSettings.ini")).unwrap(),
        b"current settings"
    );
    assert_eq!(
        fs::read(fixture.source.join("SandboxSettings.ini")).unwrap(),
        b"external settings"
    );
    assert_eq!(
        fs::read(Path::new(&result.safeguard_backup_path).join("saves/SandboxSettings.ini"))
            .unwrap(),
        b"current settings"
    );
}

#[test]
fn save_import_minecraft_requires_region_to_be_a_directory() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.source.join("region/r.0.0.mca")).unwrap();
    fs::remove_dir(fixture.source.join("region")).unwrap();
    fs::write(fixture.source.join("region"), b"wrong node type").unwrap();
    assert!(preview(&fixture.context, &fixture.source).is_err());
}

#[cfg(windows)]
#[test]
fn save_import_palworld_coop_sentinels_follow_windows_filename_case() {
    let mut fixture = Fixture::new();
    fixture.context.record.summary.module_id = "palworld".into();
    fs::write(fixture.source.join("Level.sav"), b"world").unwrap();
    fs::write(fixture.source.join("LevelMeta.sav"), b"metadata").unwrap();
    fs::write(fixture.source.join("worldoption.sav"), b"co-op settings").unwrap();
    assert!(preview(&fixture.context, &fixture.source).is_err());
    fs::remove_file(fixture.source.join("worldoption.sav")).unwrap();
    fs::create_dir(fixture.source.join("players")).unwrap();
    fs::write(
        fixture
            .source
            .join("players/00000000000000000000000000000001.SAV"),
        b"host",
    )
    .unwrap();
    assert!(preview(&fixture.context, &fixture.source).is_err());
}

#[test]
fn save_import_rejects_excessive_tree_depth() {
    let fixture = Fixture::new();
    let mut directory = fixture.source.clone();
    for _ in 0..66 {
        directory.push("nested");
        fs::create_dir(&directory).unwrap();
    }
    assert!(preview(&fixture.context, &fixture.source).is_err());
}

async fn database_fixture(fixture: &Fixture) -> StoragePaths {
    let root = &fixture.root;
    let repo = fs::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
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
    fs::create_dir_all(&paths.app_data_root).unwrap();
    fs::create_dir_all(&paths.games_root).unwrap();
    fs::create_dir_all(&fixture.context.install).unwrap();
    fs::write(
        fixture.context.install.join(".langame-private-runtime"),
        b"managed\n",
    )
    .unwrap();
    crate::initialize_database(&paths).await.unwrap();
    let descriptor = app_modules::discover_modules(&paths.modules_root)
        .unwrap()
        .into_iter()
        .find(|module| module.summary.id == "minecraft")
        .unwrap();
    crate::sync_modules(&paths, &[descriptor]).await.unwrap();
    let pool = connect_pool(&paths).await.unwrap();
    sqlx::query("INSERT INTO instances(id,name,module_id,data_path,config_path,logs_path,saves_path,runtime_mode,status) VALUES('example','Example','minecraft',?1,?2,?3,?4,'independent','stopped')")
        .bind(instance_root(&fixture.context.record).join("data").to_string_lossy().as_ref())
        .bind(fixture.context.record.config_dir.to_string_lossy().as_ref())
        .bind(instance_root(&fixture.context.record).join("logs").to_string_lossy().as_ref())
        .bind(fixture.context.target.to_string_lossy().as_ref()).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO game_installs(module_id,install_root,install_state,scope,owner_instance_id) VALUES('minecraft',?1,'installed','instance','example')")
        .bind(fixture.context.install.to_string_lossy().as_ref()).execute(&pool).await.unwrap();
    sqlx::query("UPDATE instances SET install_id=(SELECT id FROM game_installs WHERE owner_instance_id='example') WHERE id='example'")
        .execute(&pool).await.unwrap();
    pool.close().await;
    paths
}

#[tokio::test]
async fn save_import_public_pipeline_creates_backup_accepted_by_existing_restore() {
    let fixture = Fixture::new();
    let paths = database_fixture(&fixture).await;
    assert!(
        read_instance_save_import_support(&paths, "example")
            .await
            .unwrap()
            .supported
    );
    let preview = preview_instance_save_import(&paths, "example", &fixture.source)
        .await
        .unwrap();
    let result = import_instance_save(&paths, "example", &fixture.source, &preview.fingerprint)
        .await
        .unwrap();
    assert_eq!(result.copied_file_count, 2);
    assert_eq!(
        fs::read(fixture.context.target.join("level.dat")).unwrap(),
        b"new world"
    );
    let backups = crate::list_instance_backups(&paths, "example")
        .await
        .unwrap();
    assert_eq!(backups.len(), 1);
    assert_eq!(backups[0].backup_kind, InstanceBackupKind::PreRestore);
    assert_eq!(backups[0].backup_path, result.safeguard_backup_path);
    crate::restore_instance_backup(&paths, "example", &backups[0].backup_id)
        .await
        .unwrap();
    assert_eq!(
        fs::read(fixture.context.target.join("old-player.dat")).unwrap(),
        b"old player"
    );
    assert!(!fixture.context.target.join("level.dat").exists());
    assert_eq!(
        fs::read(fixture.source.join("level.dat")).unwrap(),
        b"new world"
    );
}

#[tokio::test]
async fn save_import_public_pipeline_rejects_late_configuration_and_running_state() {
    let fixture = Fixture::new();
    let paths = database_fixture(&fixture).await;
    let preview = preview_instance_save_import(&paths, "example", &fixture.source)
        .await
        .unwrap();
    let pool = connect_pool(&paths).await.unwrap();
    sqlx::query("UPDATE instances SET status='running' WHERE id='example'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        import_instance_save(&paths, "example", &fixture.source, &preview.fingerprint)
            .await
            .is_err()
    );
    sqlx::query("UPDATE instances SET status='stopped' WHERE id='example'")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    fs::write(
        fixture.context.record.config_dir.join("instance.json"),
        br#"{"settings":{"level_name":"second-world"}}"#,
    )
    .unwrap();
    assert!(
        import_instance_save(&paths, "example", &fixture.source, &preview.fingerprint)
            .await
            .is_err()
    );
    assert_eq!(
        fs::read(fixture.context.target.join("old-player.dat")).unwrap(),
        b"old player"
    );
}

#[tokio::test]
async fn save_import_coordinator_release_failure_restores_original_saves() {
    let fixture = Fixture::new();
    let paths = database_fixture(&fixture).await;
    let preview = preview_instance_save_import(&paths, "example", &fixture.source)
        .await
        .unwrap();
    let original = io::inventory(&fixture.context.target, None).unwrap();
    publication::fail_next_release(fixture.context.target.clone());
    let error = import_instance_save(&paths, "example", &fixture.source, &preview.fingerprint)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("Safeguard backup:"));
    assert_eq!(
        io::inventory(&fixture.context.target, None).unwrap(),
        original
    );
    assert_eq!(
        crate::list_instance_backups(&paths, "example")
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        fs::read(fixture.source.join("level.dat")).unwrap(),
        b"new world"
    );
}

#[tokio::test]
async fn save_import_readback_does_not_hold_database_writer() {
    let fixture = Fixture::new();
    let paths = database_fixture(&fixture).await;
    let preview = preview_instance_save_import(&paths, "example", &fixture.source)
        .await
        .unwrap();
    let mut pause = crate::instance_archive::test_gate::register(
        &fixture.context.target,
        crate::instance_archive::test_gate::Point::SaveImportReadback,
    );
    let task_paths = paths.clone();
    let source = fixture.source.clone();
    let import = tokio::spawn(async move {
        import_instance_save(&task_paths, "example", &source, &preview.fingerprint).await
    });
    pause.reached().await;
    let pool = connect_pool(&paths).await.unwrap();
    let mut connection = pool.acquire().await.unwrap();
    sqlx::query("PRAGMA busy_timeout = 0")
        .execute(&mut *connection)
        .await
        .unwrap();
    let writer = sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *connection)
        .await;
    if writer.is_ok() {
        sqlx::query("ROLLBACK")
            .execute(&mut *connection)
            .await
            .unwrap();
    }
    pause.resume();
    let imported = import.await.unwrap();
    drop(connection);
    pool.close().await;
    imported.unwrap();
    writer.expect("save readback must not reserve the database writer");
}

#[test]
fn save_import_failed_readback_restores_original_directory() {
    let fixture = Fixture::new();
    let fingerprint = fixture.preview().fingerprint;
    let before = io::inventory(&fixture.context.target, None).unwrap();
    let source = io::inventory(&fixture.source, None).unwrap();
    let result = import_transaction(
        &fixture.context,
        &fixture.source,
        &fingerprint,
        &publication::Publication::uncoordinated(),
        |staging, target| {
            publish_new_directory(staging, target)?;
            fs::write(target.join("level.dat"), b"damaged publication")
                .map_err(|error| invalid(target, error.to_string()))
        },
    );
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("verification failed")
    );
    assert_eq!(
        io::inventory(&fixture.context.target, None).unwrap(),
        before
    );
    assert_eq!(io::inventory(&fixture.source, None).unwrap(), source);
    let backups = collect_instance_backups(&fixture.context.record, "example").unwrap();
    assert_eq!(backups.len(), 1);
    assert_eq!(
        io::inventory(&Path::new(&backups[0].backup_path).join("saves"), None).unwrap(),
        before
    );
}

#[cfg(windows)]
#[test]
fn save_import_rejects_junctions_without_reading_external_tree() {
    let fixture = Fixture::new();
    let junction = fixture.source.join("redirected");
    let output = std::process::Command::new("cmd")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&junction)
        .arg(&fixture.context.target)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(preview(&fixture.context, &fixture.source).is_err());
    fs::remove_dir(junction).unwrap();
    assert_eq!(
        fs::read(fixture.context.target.join("old-player.dat")).unwrap(),
        b"old player"
    );
}

#[test]
fn save_import_dragonwilds_uses_native_world_name_and_save_timestamp_without_rewriting() {
    let mut fixture = Fixture::new();
    fixture.context.record.summary.module_id = "runescapedragonwilds".into();
    fixture.context.settings =
        serde_json::from_value(json!({"default_world_name":"Imported World"})).unwrap();
    let older = dragonwilds_import_save("Imported World", 639028224000000000);
    let newer = dragonwilds_import_save("Imported World", 639028224000000001);
    // Write the older snapshot last: filesystem timestamps must not select it.
    fs::write(fixture.source.join("arbitrary-new.sav"), &newer).unwrap();
    fs::write(fixture.source.join("arbitrary-old.sav"), &older).unwrap();
    let selected =
        crate::dragonwilds_world_settings::io::latest_world(&fixture.source, "Imported World")
            .unwrap()
            .unwrap();
    assert_eq!(selected.file_name().unwrap(), "arbitrary-new.sav");
    let native_config = fixture
        .context
        .install
        .join("RSDragonwilds/Saved/Config/WindowsServer/DedicatedServer.ini");
    fs::create_dir_all(native_config.parent().unwrap()).unwrap();
    fs::write(&native_config, b"target-ports-password-and-owner").unwrap();
    let config = fs::read(fixture.context.record.config_dir.join("instance.json")).unwrap();
    let preview = fixture.preview();
    assert!(
        preview
            .warnings
            .iter()
            .any(|warning| warning.contains("not rewritten"))
    );
    fixture.import(&preview.fingerprint).unwrap();
    for (name, expected) in [("arbitrary-new.sav", newer), ("arbitrary-old.sav", older)] {
        assert_eq!(fs::read(fixture.source.join(name)).unwrap(), expected);
        assert_eq!(
            fs::read(fixture.context.target.join(name)).unwrap(),
            expected
        );
    }
    assert_eq!(
        fs::read(native_config).unwrap(),
        b"target-ports-password-and-owner"
    );
    assert_eq!(
        fs::read(fixture.context.record.config_dir.join("instance.json")).unwrap(),
        config
    );
}

#[test]
fn save_import_dragonwilds_rejects_wrong_internal_world_ambiguous_saves_and_source_config() {
    let mut fixture = Fixture::new();
    fixture.context.record.summary.module_id = "runescapedragonwilds".into();
    fixture.context.settings =
        serde_json::from_value(json!({"default_world_name":"Imported World"})).unwrap();
    let save = fixture.source.join("Imported World.sav");
    fs::write(
        &save,
        dragonwilds_import_save("Another World", 639028224000000000),
    )
    .unwrap();
    assert!(preview(&fixture.context, &fixture.source).is_err());
    let bytes = dragonwilds_import_save("Imported World", 639028224000000000);
    fs::write(&save, &bytes).unwrap();
    fs::write(fixture.source.join("same-timestamp.sav"), &bytes).unwrap();
    let error = preview(&fixture.context, &fixture.source).unwrap_err();
    assert!(error.to_string().contains("same native save timestamp"));
    fs::remove_file(fixture.source.join("same-timestamp.sav")).unwrap();
    let checked = fixture.preview();
    fixture
        .context
        .settings
        .insert("default_world_name".into(), json!("Another World"));
    assert!(fixture.import(&checked.fingerprint).is_err());
    fixture
        .context
        .settings
        .insert("default_world_name".into(), json!("Imported World"));
    fs::write(
        fixture.source.join("DedicatedServer.ini"),
        b"source host settings",
    )
    .unwrap();
    assert!(preview(&fixture.context, &fixture.source).is_err());
    assert_eq!(fs::read(&save).unwrap(), bytes);
    assert_eq!(
        fs::read(fixture.context.target.join("old-player.dat")).unwrap(),
        b"old player"
    );
}

#[test]
fn save_import_vrising_matches_current_pc_generation_and_keeps_complete_world_bytes() {
    let mut fixture = Fixture::new();
    fixture.context.record.summary.module_id = "vrising".into();
    fixture.context.settings = serde_json::from_value(json!({"save_name":"Castle"})).unwrap();
    let world = fixture.source.join("v4/Castle");
    fs::create_dir_all(&world).unwrap();
    fs::write(world.join("AutoSave_invalid.save.gz"), b"not a native name").unwrap();
    assert!(preview(&fixture.context, &fixture.source).is_err());
    fs::write(world.join("AutoSave_0.save.gz"), b"").unwrap();
    assert!(preview(&fixture.context, &fixture.source).is_err());
    fs::write(world.join("AutoSave_0.save.gz"), b"opaque native snapshot").unwrap();
    fs::write(
        world.join("unknown-world-metadata.json"),
        br#"{"keep":"unchanged"}"#,
    )
    .unwrap();
    fs::write(
        world.join("ServerGameSettings.json"),
        br#"{"world-local-rule":1}"#,
    )
    .unwrap();
    let host_settings = fixture
        .root
        .join("instances/example/Settings/ServerHostSettings.json");
    fs::create_dir_all(host_settings.parent().unwrap()).unwrap();
    fs::write(&host_settings, b"target host identity and ports").unwrap();
    let source = io::inventory(&fixture.source, None).unwrap();
    fixture.import(&fixture.preview().fingerprint).unwrap();
    assert_eq!(io::inventory(&fixture.source, None).unwrap(), source);
    assert_eq!(
        io::inventory(&fixture.context.target, None).unwrap(),
        source
    );
    assert_eq!(
        fs::read(host_settings).unwrap(),
        b"target host identity and ports"
    );
    // The optional native uncompressed save form uses the same v4 directory.
    fs::remove_file(world.join("AutoSave_0.save.gz")).unwrap();
    fs::write(
        world.join("AutoSave_1.save"),
        b"uncompressed native snapshot",
    )
    .unwrap();
    assert!(preview(&fixture.context, &fixture.source).is_ok());
}

#[test]
fn save_import_vrising_rejects_other_worlds_old_generations_and_host_credentials() {
    let mut fixture = Fixture::new();
    fixture.context.record.summary.module_id = "vrising".into();
    fixture.context.settings = serde_json::from_value(json!({"save_name":"Castle"})).unwrap();
    for directory in ["v3/Castle", "v4/Other", "v4/Castle/nested"] {
        let directory = fixture.source.join(directory);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("AutoSave_0.save.gz"), b"native snapshot").unwrap();
        assert!(preview(&fixture.context, &fixture.source).is_err());
    }
    let world = fixture.source.join("v4/Castle");
    fs::write(world.join("AutoSave_1.save.gz"), b"native snapshot").unwrap();
    for filename in ["ServerHostSettings.json", "ADMINLIST.TXT", "banlist.txt"] {
        fs::write(world.join(filename), b"source host credentials").unwrap();
        assert!(
            preview(&fixture.context, &fixture.source).is_err(),
            "accepted {filename}"
        );
        fs::remove_file(world.join(filename)).unwrap();
    }
    let checked = fixture.preview();
    fixture
        .context
        .settings
        .insert("save_name".into(), json!("Other Castle"));
    assert!(fixture.import(&checked.fingerprint).is_err());
    assert_eq!(
        fs::read(fixture.context.target.join("old-player.dat")).unwrap(),
        b"old player"
    );
}
