use super::*;
use std::path::PathBuf;

struct Fixture {
    root: PathBuf,
    paths: StoragePaths,
    instance: PathBuf,
    saves: PathBuf,
}

impl Fixture {
    async fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("lgsm-astroneer-saves-{}", uuid::Uuid::new_v4()));
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
        let instance = paths.instances_root.join("astroneer-fixture");
        let saves = instance.join("runtime/Astro/Saved/SaveGames");
        for path in [
            &paths.app_data_root,
            &paths.games_root,
            &instance.join("config"),
            &saves,
        ] {
            fs::create_dir_all(path).unwrap();
        }
        fs::write(
            instance.join("runtime/.langame-private-runtime"),
            b"managed\n",
        )
        .unwrap();
        fs::write(
            instance.join("config/instance.json"),
            br#"{"settings":{"active_save_file_name":"Missing Existing Draft"},"ports":[]}"#,
        )
        .unwrap();
        crate::initialize_database(&paths).await.unwrap();
        let descriptor = app_modules::discover_modules(&paths.modules_root)
            .unwrap()
            .into_iter()
            .find(|module| module.summary.id == "astroneer")
            .unwrap();
        crate::sync_modules(&paths, &[descriptor]).await.unwrap();
        let pool = connect_pool(&paths).await.unwrap();
        sqlx::query("INSERT INTO instances(id,name,module_id,data_path,config_path,logs_path,saves_path,runtime_mode,status) VALUES('astroneer-fixture','Fixture','astroneer',?1,?2,?3,?4,'independent','stopped')")
            .bind(instance.join("data").to_string_lossy().as_ref())
            .bind(instance.join("config").to_string_lossy().as_ref())
            .bind(instance.join("logs").to_string_lossy().as_ref())
            .bind(saves.to_string_lossy().as_ref()).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO game_installs(module_id,install_root,install_state,scope,owner_instance_id) VALUES('astroneer',?1,'installed','instance','astroneer-fixture')")
            .bind(instance.join("runtime").to_string_lossy().as_ref()).execute(&pool).await.unwrap();
        sqlx::query("UPDATE instances SET install_id=(SELECT id FROM game_installs WHERE owner_instance_id='astroneer-fixture') WHERE id='astroneer-fixture'")
            .execute(&pool).await.unwrap();
        pool.close().await;
        Self {
            root,
            paths,
            instance,
            saves,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn native_filename_contract_validates_names_calendar_and_clock() {
    for filename in [
        "SAVE_1$2026.10.09-23.59.59.savegame",
        "My Custom Game$2024.02.29-00.00.00.SAVEGAME",
        "异星世界$2000.02.29-01.02.03.savegame",
    ] {
        assert!(parse_save_filename(filename).is_some(), "{filename}");
    }
    for filename in [
        "SAVE_1.savegame",
        "world.sav",
        "SAVE_1$2026.10.09-23.59.59.savegame.bak",
        "$2026.10.09-23.59.59.savegame",
        " $2026.10.09-23.59.59.savegame",
        "A$B$2026.10.09-23.59.59.savegame",
        "A|B$2026.10.09-23.59.59.savegame",
        "A\nB$2026.10.09-23.59.59.savegame",
        "A/B$2026.10.09-23.59.59.savegame",
        "SAVE_1$0000.10.09-23.59.59.savegame",
        "SAVE_1$1900.02.29-00.00.00.savegame",
        "SAVE_1$2026.02.29-00.00.00.savegame",
        "SAVE_1$2026.04.31-00.00.00.savegame",
        "SAVE_1$2026.13.01-00.00.00.savegame",
        "SAVE_1$2026.01.00-00.00.00.savegame",
        "SAVE_1$2026.10.09-24.00.00.savegame",
        "SAVE_1$2026.10.09-00.60.00.savegame",
        "SAVE_1$2026.10.09-00.00.60.savegame",
        "SAVE_1$2026-10-09-00.00.00.savegame",
    ] {
        assert!(parse_save_filename(filename).is_none(), "{filename}");
    }
}

#[tokio::test]
async fn catalog_groups_native_versions_preserves_existing_draft_and_changes_no_files() {
    let fixture = Fixture::new().await;
    let native = [
        (
            "Custom World$2026.10.08-12.00.00.savegame",
            b"old".as_slice(),
        ),
        (
            "Custom World$2026.10.09-12.00.00.savegame",
            b"latest".as_slice(),
        ),
        (
            "Adventure$2026.10.09-13.00.00.savegame",
            b"other".as_slice(),
        ),
    ];
    for (name, bytes) in native {
        fs::write(fixture.saves.join(name), bytes).unwrap();
    }
    for name in [
        "unknown.savegame",
        "SAVE$2026.02.29-00.00.00.savegame",
        "notes.txt",
    ] {
        fs::write(fixture.saves.join(name), b"unrecognized").unwrap();
    }
    fs::write(
        fixture.saves.join("EMPTY$2026.10.09-12.00.00.savegame"),
        b"",
    )
    .unwrap();
    fs::create_dir(fixture.saves.join("Directory$2026.10.09-12.00.00.savegame")).unwrap();
    let config = fs::read(fixture.instance.join("config/instance.json")).unwrap();
    let catalog = read_astroneer_save_catalog(&fixture.paths, "astroneer-fixture")
        .await
        .unwrap();
    assert_eq!(catalog.instance_id, "astroneer-fixture");
    assert_eq!(catalog.configured_name, "Missing Existing Draft");
    assert_eq!(
        catalog.entries,
        vec![
            AstroneerSaveEntry {
                descriptive_name: "Adventure".into(),
                latest_saved_at: "2026.10.09-13.00.00".into(),
                versions: 1,
                total_bytes: 5
            },
            AstroneerSaveEntry {
                descriptive_name: "Custom World".into(),
                latest_saved_at: "2026.10.09-12.00.00".into(),
                versions: 2,
                total_bytes: 9
            },
        ]
    );
    assert_eq!(
        fs::read(fixture.instance.join("config/instance.json")).unwrap(),
        config
    );
    for (name, bytes) in native {
        assert_eq!(fs::read(fixture.saves.join(name)).unwrap(), bytes);
    }
    assert!(!fixture.instance.join("backups").exists());
}

#[tokio::test]
async fn missing_native_directory_is_empty_without_creation_but_malformed_context_fails() {
    let fixture = Fixture::new().await;
    fs::remove_dir(&fixture.saves).unwrap();
    assert!(
        read_astroneer_save_catalog(&fixture.paths, "astroneer-fixture")
            .await
            .unwrap()
            .entries
            .is_empty()
    );
    assert!(!fixture.saves.exists());
    fs::write(&fixture.saves, b"not a directory").unwrap();
    assert!(
        read_astroneer_save_catalog(&fixture.paths, "astroneer-fixture")
            .await
            .is_err()
    );
    fs::remove_file(&fixture.saves).unwrap();
    fs::write(
        fixture.instance.join("config/instance.json"),
        br#"{"settings":{"active_save_file_name":true}}"#,
    )
    .unwrap();
    assert!(
        read_astroneer_save_catalog(&fixture.paths, "astroneer-fixture")
            .await
            .is_err()
    );
    assert!(
        read_astroneer_save_catalog(&fixture.paths, "missing-instance")
            .await
            .is_err()
    );
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("INSERT INTO modules(id,name,version) VALUES('other-game','Other','1')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE instances SET module_id='other-game' WHERE id='astroneer-fixture'")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    assert!(
        read_astroneer_save_catalog(&fixture.paths, "astroneer-fixture")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn ordinary_directories_and_catalog_capacity_are_enforced() {
    let fixture = Fixture::new().await;
    let outside = fixture.root.join("outside");
    fs::create_dir(&outside).unwrap();
    // cmd's mklink has a shorter path limit than std::fs. The same scanner is
    // tested with a compact owned directory; capacity tests keep the real root.
    let linked_catalog = fixture.root.join("links");
    fs::create_dir(&linked_catalog).unwrap();
    let link = linked_catalog.join("linked");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let result = std::process::Command::new("cmd")
            .creation_flags(0x08000000)
            .args(["/d", "/c", "mklink", "/J"])
            // mklink parses forward slashes as switches, unlike std::fs.
            .arg(link.to_string_lossy().replace('/', "\\"))
            .arg(outside.to_string_lossy().replace('/', "\\"))
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "junction fixture failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    assert!(scan_slots(&linked_catalog).is_err());
    assert!(scan_slots(&link.join("missing")).is_err());
    #[cfg(windows)]
    fs::remove_dir(&link).unwrap();
    #[cfg(unix)]
    fs::remove_file(&link).unwrap();
    for index in 0..=MAX_DESCRIPTIVE_SLOTS {
        fs::write(
            fixture
                .saves
                .join(format!("SLOT_{index}$2026.10.09-00.00.00.savegame")),
            b"fixture",
        )
        .unwrap();
    }
    assert!(scan_slots(&fixture.saves).is_err());
    for entry in fs::read_dir(&fixture.saves).unwrap() {
        fs::remove_file(entry.unwrap().path()).unwrap();
    }
    for index in 0..=MAX_DIRECTORY_ENTRIES {
        fs::write(fixture.saves.join(format!("ignored_{index}")), b"").unwrap();
    }
    assert!(scan_slots(&fixture.saves).is_err());
}
