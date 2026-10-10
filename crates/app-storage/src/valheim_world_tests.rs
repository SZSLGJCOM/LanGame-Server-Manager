use super::*;
use std::path::PathBuf;

fn metadata(version: i32, keys: &[&str]) -> Vec<u8> {
    fn string(bytes: &mut Vec<u8>, text: &str) {
        let mut size = text.len();
        while size >= 128 {
            bytes.push((size as u8 & 127) | 128);
            size >>= 7;
        }
        bytes.push(size as u8);
        bytes.extend_from_slice(text.as_bytes());
    }
    let mut package = Vec::new();
    package.extend_from_slice(&version.to_le_bytes());
    string(&mut package, "Synthetic world");
    string(&mut package, "Synthetic seed");
    package.extend_from_slice(&123_i32.to_le_bytes());
    package.extend_from_slice(&456_i64.to_le_bytes());
    package.extend_from_slice(&2_i32.to_le_bytes());
    if version >= 30 {
        package.push(1);
    }
    if version >= 32 {
        package.extend_from_slice(&(keys.len() as i32).to_le_bytes());
        for key in keys {
            string(&mut package, key);
        }
    }
    if version >= 41 {
        package.extend_from_slice(&0_i32.to_le_bytes());
    }
    let mut bytes = (package.len() as i32).to_le_bytes().to_vec();
    bytes.extend(package);
    bytes
}

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let unique = uuid::Uuid::new_v4().simple().to_string();
        let root = std::env::temp_dir().join(format!("vw-{}", &unique[..16]));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn save(&self, relative: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn world_metadata_decodes_native_versions_and_preserves_unknown_rule_values() {
    for version in [26, 29, 30, 31, 32, 40, 41] {
        let input = metadata(version, &["enemydamage 150", "nomap", "futurecustom 77"]);
        let original = input.clone();
        let (actual_version, keys, needs_db) = codec::decode_keys(&input).unwrap();
        assert_eq!(actual_version, version);
        assert_eq!(needs_db, version >= 30);
        assert_eq!(
            keys,
            if version >= 32 {
                vec!["enemydamage 150", "nomap", "futurecustom 77"]
            } else {
                vec![]
            }
        );
        assert_eq!(input, original);
    }
}

#[test]
fn world_metadata_rejects_truncation_future_versions_and_unbounded_strings() {
    let valid = metadata(41, &["nomap"]);
    for length in 0..valid.len() {
        let mut truncated = valid[..length].to_vec();
        if length >= 4 {
            truncated[..4].copy_from_slice(&((length - 4) as i32).to_le_bytes());
        }
        assert!(codec::decode_keys(&truncated).is_err(), "length={length}");
    }
    for version in [25, 42, i32::MAX] {
        assert!(codec::decode_keys(&metadata(version, &[])).is_err());
    }
    assert!(codec::decode_keys(&metadata(41, &["nomap\nsecret"])).is_err());
    assert!(codec::decode_keys(&metadata(41, &[&"x".repeat(4097)])).is_err());
    let mut oversized = metadata(41, &[]);
    oversized[4 + 4] = 0xff;
    oversized[4 + 5] = 0xff;
    oversized[4 + 6] = 0xff;
    oversized[4 + 7] = 0xff;
    oversized[4 + 8] = 0xff;
    assert!(codec::decode_keys(&oversized).is_err());
}

#[test]
fn version_41_requires_complete_history_records_and_no_trailing_data() {
    let valid = metadata(41, &["nomap"]);
    let mut without_history_count = valid[..valid.len() - 4].to_vec();
    let length = (without_history_count.len() - 4) as i32;
    without_history_count[..4].copy_from_slice(&length.to_le_bytes());
    assert!(codec::decode_keys(&without_history_count).is_err());
    let mut record = valid.clone();
    let count_offset = record.len() - 4;
    record[count_offset..].copy_from_slice(&1_i32.to_le_bytes());
    record.extend_from_slice(b"\x07Steam_1\x09Synthetic\x04Name\x03abc");
    let length = (record.len() - 4) as i32;
    record[..4].copy_from_slice(&length.to_le_bytes());
    assert_eq!(codec::decode_keys(&record).unwrap().1, vec!["nomap"]);
    for length in count_offset..record.len() {
        let mut truncated = record[..length].to_vec();
        truncated[..4].copy_from_slice(&((length - 4) as i32).to_le_bytes());
        assert!(
            codec::decode_keys(&truncated).is_err(),
            "history truncation at {length}"
        );
    }
    let mut extra = valid;
    extra.push(0);
    let length = (extra.len() - 4) as i32;
    extra[..4].copy_from_slice(&length.to_le_bytes());
    assert!(codec::decode_keys(&extra).is_err());
}

#[test]
fn read_selects_latest_committed_chunked_metadata_without_changing_files() {
    let fixture = Fixture::new();
    let original = metadata(41, &["nomap", "resourcerate 300"]);
    let old = fixture.save(
        "worlds_local/World/_main.1.fwl2",
        &metadata(40, &["teleportall"]),
    );
    fixture.save(
        "worlds_local/World/_main.1.db2",
        b"owned synthetic database",
    );
    fixture.save("worlds_local/World/_main.1.ok", b"committed");
    let current = fixture.save("worlds_local/World/_main.2.fwl2", &original);
    fixture.save(
        "worlds_local/World/_main.2.db2",
        b"owned synthetic database",
    );
    fixture.save("worlds_local/World/_main.2.ok", b"committed");
    fixture.save(
        "worlds_local/World/_main.3.fwl2",
        &metadata(41, &["nobuildcost"]),
    );
    let rules = scan_world(&fixture.0, "World").unwrap();
    assert_eq!(
        rules,
        (
            ValheimWorldRuleSource::Saved,
            Some(41),
            vec!["nomap".into(), "resourcerate 300".into()]
        )
    );
    assert_eq!(fs::read(current).unwrap(), original);
    assert_eq!(fs::read(old).unwrap(), metadata(40, &["teleportall"]));
    fixture.save(
        "worlds_local/World/_main.4.ok",
        b"committed but metadata missing",
    );
    assert_eq!(
        scan_world(&fixture.0, "World").unwrap().0,
        ValheimWorldRuleSource::MissingMetadata
    );
}

#[test]
fn legacy_metadata_missing_world_and_missing_rules_remain_distinct() {
    let fixture = Fixture::new();
    assert_eq!(
        scan_world(&fixture.0, "Fresh").unwrap().0,
        ValheimWorldRuleSource::NewWorld
    );
    assert!(!fixture.0.join("worlds_local").exists());
    fixture.save("worlds_local/Old.fwl", &metadata(32, &["passivemobs"]));
    assert_eq!(
        scan_world(&fixture.0, "Old").unwrap().0,
        ValheimWorldRuleSource::MissingMetadata
    );
    fixture.save("worlds_local/Old.db", b"owned synthetic database");
    assert_eq!(
        scan_world(&fixture.0, "Old").unwrap().2,
        vec!["passivemobs"]
    );
    fixture.save("worlds_local/MetadataMissing.db", b"owned database");
    assert_eq!(
        scan_world(&fixture.0, "MetadataMissing").unwrap().0,
        ValheimWorldRuleSource::MissingMetadata
    );
    for name in [
        "",
        ".",
        "..",
        "../Outside",
        "Outside\\World",
        "name.",
        "name ",
        "name:stream",
        "a\nb",
    ] {
        assert!(validate_world_name(name).is_err(), "{name}");
    }
    assert!(validate_world_name("瓦纳海姆 世界").is_ok());
}

#[test]
fn metadata_files_cannot_exceed_the_read_limit() {
    let fixture = Fixture::new();
    fixture.save("worlds_local/Huge.fwl", &vec![0; 64 * 1024 + 1]);
    assert!(scan_world(&fixture.0, "Huge").is_err());
}

#[test]
#[ignore = "Requires explicit read-only access to an installed Valheim world's save root."]
fn installed_world_metadata_read_only_probe() {
    let root = PathBuf::from(
        std::env::var_os("LGSM_VALHEIM_RULES_PROBE_SAVE_ROOT")
            .expect("LGSM_VALHEIM_RULES_PROBE_SAVE_ROOT is required"),
    );
    let name = std::env::var("LGSM_VALHEIM_RULES_PROBE_WORLD_NAME")
        .expect("LGSM_VALHEIM_RULES_PROBE_WORLD_NAME is required");
    validate_world_name(&name).unwrap();
    let (source, version, keys) = scan_world(&root, &name).unwrap();
    assert_eq!(source, ValheimWorldRuleSource::Saved);
    assert!(matches!(version, Some(26..=41)));
    println!(
        "VALHEIM_NATIVE_RULES_READBACK world_version={} rule_count={} read_only=true",
        version.unwrap(),
        keys.len()
    );
}

struct DatabaseFixture {
    files: Fixture,
    paths: StoragePaths,
    instance: PathBuf,
}

impl DatabaseFixture {
    async fn new() -> Self {
        let files = Fixture::new();
        let root = &files.0;
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
        // Keep the actual managed root compact for cmd's mklink path limit.
        let instance = paths.instances_root.join("v");
        for path in [
            &paths.app_data_root,
            &paths.games_root,
            &instance.join("config"),
            &instance.join("runtime"),
            &instance.join("saves"),
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
            br#"{"settings":{"world_name":"Committed","future_setting":"retained"},"ports":[]}"#,
        )
        .unwrap();
        crate::initialize_database(&paths).await.unwrap();
        let descriptor = app_modules::discover_modules(&paths.modules_root)
            .unwrap()
            .into_iter()
            .find(|module| module.summary.id == "valheim")
            .unwrap();
        crate::sync_modules(&paths, &[descriptor]).await.unwrap();
        let pool = connect_pool(&paths).await.unwrap();
        sqlx::query("INSERT INTO instances(id,name,module_id,data_path,config_path,logs_path,saves_path,runtime_mode,status) VALUES('valheim-fixture','Default Selected World','valheim',?1,?2,?3,?4,'independent','stopped')")
            .bind(instance.join("data").to_string_lossy().as_ref())
            .bind(instance.join("config").to_string_lossy().as_ref())
            .bind(instance.join("logs").to_string_lossy().as_ref())
            .bind(instance.join("saves").to_string_lossy().as_ref()).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO game_installs(module_id,install_root,install_state,scope,owner_instance_id) VALUES('valheim',?1,'installed','instance','valheim-fixture')")
            .bind(instance.join("runtime").to_string_lossy().as_ref()).execute(&pool).await.unwrap();
        sqlx::query("UPDATE instances SET install_id=(SELECT id FROM game_installs WHERE owner_instance_id='valheim-fixture') WHERE id='valheim-fixture'")
            .execute(&pool).await.unwrap();
        pool.close().await;
        Self {
            files,
            paths,
            instance,
        }
    }
}

#[tokio::test]
async fn public_reader_verifies_module_committed_selector_and_authoritative_default() {
    let fixture = DatabaseFixture::new().await;
    let config = fixture.instance.join("config/instance.json");
    let original = fs::read(&config).unwrap();
    let saves = fixture.instance.join("saves/worlds_local");
    fs::create_dir_all(&saves).unwrap();
    let native = metadata(41, &["nomap", "resourcerate 300"]);
    // The internal editable display name is intentionally different. Native
    // dedicated-server -world selects the disk identity, not this display name.
    fs::write(saves.join("Committed.fwl"), &native).unwrap();
    fs::write(saves.join("Committed.db"), b"owned synthetic database").unwrap();
    let result = read_valheim_world_rules(&fixture.paths, "valheim-fixture", "Committed")
        .await
        .unwrap();
    assert_eq!(result.saved_keys, vec!["nomap", "resourcerate 300"]);
    assert_eq!(fs::read(&config).unwrap(), original);
    assert_eq!(fs::read(saves.join("Committed.fwl")).unwrap(), native);
    let mut bom_config = vec![0xef, 0xbb, 0xbf];
    bom_config.extend_from_slice(&original);
    fs::write(&config, &bom_config).unwrap();
    let with_bom = read_valheim_world_rules(&fixture.paths, "valheim-fixture", "Committed")
        .await
        .unwrap();
    assert_eq!(with_bom.saved_keys, result.saved_keys);
    assert_eq!(
        fs::read(&config).unwrap(),
        bom_config,
        "A read must preserve the original encoding"
    );
    fs::write(&config, &original).unwrap();
    assert!(
        read_valheim_world_rules(&fixture.paths, "valheim-fixture", "Other")
            .await
            .is_err()
    );
    fs::write(&config, br#"{"settings":{"world_name":true}}"#).unwrap();
    assert!(
        read_valheim_world_rules(&fixture.paths, "valheim-fixture", "Committed")
            .await
            .is_err()
    );
    fs::write(&config, br#"{"settings":{}}"#).unwrap();
    assert_eq!(
        read_valheim_world_rules(&fixture.paths, "valheim-fixture", "Default Selected World")
            .await
            .unwrap()
            .source,
        ValheimWorldRuleSource::NewWorld
    );
    assert!(
        read_valheim_world_rules(&fixture.paths, "missing", "Committed")
            .await
            .is_err()
    );
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("INSERT INTO modules(id,name,version) VALUES('other-game','Other','1')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE instances SET module_id='other-game' WHERE id='valheim-fixture'")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    assert!(
        read_valheim_world_rules(&fixture.paths, "valheim-fixture", "Committed")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn public_reader_rejects_a_real_link_in_the_owned_save_directory() {
    let fixture = DatabaseFixture::new().await;
    let outside = fixture.files.0.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("_main.1.fwl2"), metadata(41, &["nomap"])).unwrap();
    fs::write(outside.join("_main.1.db2"), b"outside world").unwrap();
    fs::write(outside.join("_main.1.ok"), b"committed").unwrap();
    let parent = fixture.instance.join("saves/worlds_local");
    fs::create_dir(&parent).unwrap();
    let link = parent.join("Committed");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let result = std::process::Command::new("cmd")
            .creation_flags(0x08000000)
            .args(["/d", "/c", "mklink", "/J"])
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
    let result = read_valheim_world_rules(&fixture.paths, "valheim-fixture", "Committed").await;
    #[cfg(windows)]
    fs::remove_dir(&link).unwrap();
    #[cfg(unix)]
    fs::remove_file(&link).unwrap();
    assert!(result.is_err());
    assert_eq!(
        fs::read(outside.join("_main.1.db2")).unwrap(),
        b"outside world"
    );
}
