use super::*;

// Exact text emitted by build 26335402 through SetPersistentString(..., false)
// after saving an isolated small Master world. The native file has no final LF.
const NATIVE_MASTER: &str = "KLEI     1 return {\n\toverride_enabled = true,\n\tworldgen_preset = \"SURVIVAL_TOGETHER\",\n\tsettings_preset = \"SURVIVAL_TOGETHER\",\n\toverrides = {\n\t\tworld_size = \"small\",\n\t},\n}";

#[tokio::test]
async fn dst_backup_restore_recovers_native_world_options_before_rematerialization() {
    let mut fixture = Fixture::new().await;
    fixture.persist_mod("123456", "original").await;
    write_saved_world(&fixture.target(), "OLD");
    let world_path = fixture.target().join("Master/worldgenoverride.lua");
    fs::write(&world_path, NATIVE_MASTER).unwrap();
    let saved_world = fixture.target().join("Master/save/session/OLD/0000000001");
    let saved_bytes = fs::read(&saved_world).unwrap();
    let backup =
        app_storage::create_instance_backup(&fixture.storage.paths, &fixture.instance.summary.id)
            .await
            .unwrap();
    let backup_root = Path::new(&backup.backup_path).join("saves");
    assert_eq!(
        fs::read(backup_root.join("Master/worldgenoverride.lua")).unwrap(),
        NATIVE_MASTER.as_bytes()
    );
    fs::write(&saved_world, b"changed world after backup").unwrap();
    fs::write(
        &world_path,
        "return {override_enabled=true, overrides={world_size='huge'}}",
    )
    .unwrap();
    let prepared = app_storage::prepare_instance_backup_restore(
        &fixture.storage.paths,
        &fixture.instance.summary.id,
        &backup.backup_id,
    )
    .await
    .unwrap();
    let restored = fixture
        .replace(backup_root.clone(), Some(prepared))
        .await
        .unwrap();
    assert!(restored.result.imported_master);
    assert_eq!(fs::read(&saved_world).unwrap(), saved_bytes);
    assert_eq!(
        fs::read(backup_root.join("Master/worldgenoverride.lua")).unwrap(),
        NATIVE_MASTER.as_bytes(),
        "normalizing restored configuration must not modify the selected backup"
    );
    let details = read_instance_details(&fixture.storage.paths, &fixture.instance.summary.id)
        .await
        .unwrap();
    let settings: Value = serde_json::from_str(&details.settings_json).unwrap();
    let body = NATIVE_MASTER.strip_prefix("KLEI     1 ").unwrap();
    assert_eq!(settings["master_worldgenoverride_lua"], body);
    let rendered = fs::read(&world_path).unwrap();
    // The renderer terminates its Lua body, then the HBS file contributes LF.
    assert_eq!(rendered, format!("{body}\n\n").as_bytes());
    app_storage::materialize_instance_configuration(&fixture.storage.paths, &details.summary.id)
        .await
        .unwrap();
    assert_eq!(fs::read(&world_path).unwrap(), rendered);
    assert_eq!(fs::read(&saved_world).unwrap(), saved_bytes);
}

#[tokio::test]
async fn dst_restore_rejects_unsafe_world_options_before_publishing_world_or_settings() {
    let mut fixture = Fixture::new().await;
    fixture.persist_mod("123456", "original").await;
    write_saved_world(&fixture.target(), "OLD");
    let world_path = fixture.target().join("Master/worldgenoverride.lua");
    let world_bytes =
        fs::read(fixture.target().join("Master/save/session/OLD/0000000001")).unwrap();
    let details = read_instance_details(&fixture.storage.paths, &fixture.instance.summary.id)
        .await
        .unwrap();
    let safe = "return {override_enabled=true,overrides={world_size='small'}}";
    for unsafe_source in [
        "KLEI     2 return {overrides={}}",
        "KLEI     10 return {overrides={}}",
        "KLEI 1 return {overrides={}}",
        "KLEI     1 return build_world()",
        "KLEI     1 local x={}; return x",
        "KLEI     1 return {overrides={world_size=os.getenv('SIZE')}}",
        "KLEI     1 return {overrides={}}; os.execute('unexpected')",
        "KLEI     1 return {overrides={world_size='small',world_size='huge'}}",
    ] {
        fs::write(&world_path, unsafe_source).unwrap();
        let backup = app_storage::create_instance_backup(
            &fixture.storage.paths,
            &fixture.instance.summary.id,
        )
        .await
        .unwrap();
        fs::write(&world_path, safe).unwrap();
        let prepared = app_storage::prepare_instance_backup_restore(
            &fixture.storage.paths,
            &fixture.instance.summary.id,
            &backup.backup_id,
        )
        .await
        .unwrap();
        let error = fixture
            .replace(Path::new(&backup.backup_path).join("saves"), Some(prepared))
            .await
            .err()
            .expect("unsafe saved Lua must be rejected");
        assert!(error.contains("bounded static return table"), "{error}");
        assert_eq!(fs::read_to_string(&world_path).unwrap(), safe);
        assert_eq!(
            fs::read(fixture.target().join("Master/save/session/OLD/0000000001")).unwrap(),
            world_bytes
        );
        assert_eq!(
            read_instance_details(&fixture.storage.paths, &fixture.instance.summary.id)
                .await
                .unwrap()
                .settings_json,
            details.settings_json
        );
    }
}

fn cluster_bytes(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    let mut files = std::collections::BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    files
}

fn captured_native_ini(shard: &str, managed: &[u8]) -> Vec<u8> {
    let capture: Value = serde_json::from_str(include_str!(
        "../../../../modules/dontstarve/reference-fixtures/2026-10-31-native_server_ini_761239.json"
    ))
    .unwrap();
    let sample = &capture["samples"][shard];
    let baseline = sample["manager"]["utf8"].as_str().unwrap();
    let mut native = sample["stopped_native"]["utf8"]
        .as_str()
        .unwrap()
        .to_owned();
    let managed = std::str::from_utf8(managed).unwrap();
    assert_eq!(baseline.lines().count(), managed.lines().count());
    for (captured, actual) in baseline.lines().zip(managed.lines()) {
        if captured == actual {
            continue;
        }
        // Substitute only this disposable instance's numeric identities into
        // the captured native bytes; do not reproduce the oracle's formatting.
        let (key, _) = captured.split_once(" = ").unwrap();
        let (actual_key, value) = actual.split_once(" = ").unwrap();
        assert_eq!(key, actual_key);
        assert!(matches!(
            key,
            "server_port" | "master_server_port" | "authentication_port" | "id"
        ));
        assert!(!value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()));
        let captured_line = format!("\r\n{captured}\r\n");
        assert_eq!(native.matches(captured_line.as_str()).count(), 1);
        native = native.replace(&captured_line, &format!("\r\n{actual}\r\n"));
    }
    native.into_bytes()
}

#[tokio::test]
async fn dst_no_mods_restore_rematerializes_ini_and_preserves_world_and_operator_files() {
    let mut fixture = Fixture::new().await;
    let mut settings: Value = serde_json::from_str(&fixture.instance.settings_json).unwrap();
    settings["offline_cluster"] = json!(true);
    settings["enable_caves"] = json!(true);
    settings["master_world_size"] = json!("small");
    settings["caves_world_size"] = json!("small");
    fixture.instance = app_storage::update_instance_if_current(
        &fixture.storage.paths,
        UpdateInstanceInput {
            id: fixture.instance.summary.id.clone(),
            bind_ip: fixture.instance.summary.bind_ip.clone(),
            auto_backup_on_stop: false,
            backup_retention_count: 8,
            settings_json: settings.to_string(),
            ports: fixture.instance.ports.clone(),
        },
        &fixture.instance.settings_json,
    )
    .await
    .unwrap();
    let root = fixture.target();
    let managed_ini = ["Master", "Caves", "Islands", "Volcano"]
        .map(|shard| {
            let relative = Path::new(shard).join("server.ini");
            let bytes = fs::read(root.join(&relative)).unwrap();
            (relative, bytes)
        })
        .into_iter()
        .collect::<std::collections::BTreeMap<_, _>>();
    for (shard, preset) in [("Master", "SURVIVAL_TOGETHER"), ("Caves", "DST_CAVE")] {
        let save = root.join(shard).join("save");
        fs::create_dir_all(save.join("session/OLD")).unwrap();
        fs::write(
            save.join("shardindex"),
            "return {session_id='OLD',enabled_mods={}}",
        )
        .unwrap();
        let relative = Path::new(shard).join("server.ini");
        fs::write(
            root.join(&relative),
            captured_native_ini(shard, &managed_ini[&relative]),
        )
        .unwrap();
        fs::write(save.join("session/OLD/0000000001"), b"saved world\0\x01").unwrap();
        fs::write(save.join("session/OLD/0000000001.meta"), b"world metadata").unwrap();
        fs::write(
            root.join(shard).join("worldgenoverride.lua"),
            NATIVE_MASTER.replace("SURVIVAL_TOGETHER", preset),
        )
        .unwrap();
    }
    let before = cluster_bytes(&root);
    let config_before: Value =
        serde_json::from_slice(&fs::read(&fixture.instance.config_file_path).unwrap()).unwrap();
    let backup =
        app_storage::create_instance_backup(&fixture.storage.paths, &fixture.instance.summary.id)
            .await
            .unwrap();
    let backup_root = Path::new(&backup.backup_path).join("saves");
    fs::write(
        root.join("Master/save/session/OLD/0000000001"),
        b"changed native world",
    )
    .unwrap();
    fs::write(
        root.join("Master/save/post-backup-world-file"),
        b"discard during restore",
    )
    .unwrap();
    let retained = PathBuf::from("post-backup-operator-file.txt");
    let retained_bytes = b"operator data outside shard world ownership";
    fs::write(root.join(&retained), retained_bytes).unwrap();
    let prepared = app_storage::prepare_instance_backup_restore(
        &fixture.storage.paths,
        &fixture.instance.summary.id,
        &backup.backup_id,
    )
    .await
    .unwrap();
    let restored = fixture
        .replace(backup_root.clone(), Some(prepared))
        .await
        .unwrap();
    let copied_bytes: u64 = before
        .iter()
        .filter(|(path, _)| path.starts_with("Master/save") || path.starts_with("Caves/save"))
        .map(|(_, bytes)| bytes.len() as u64)
        .sum();
    assert_eq!(
        restored.result.copied_file_count, 6,
        "two three-file saved worlds; retained configs are not copied world data"
    );
    assert_eq!(restored.result.copied_total_bytes, copied_bytes);
    assert!(restored.result.imported_master && restored.result.imported_caves);
    let mut expected = before.clone();
    expected.insert(retained, retained_bytes.to_vec());
    expected.extend(managed_ini);
    let restored_settings: Value = serde_json::from_str(
        &read_instance_details(&fixture.storage.paths, &fixture.instance.summary.id)
            .await
            .unwrap()
            .settings_json,
    )
    .unwrap();
    for (key, directory) in [
        ("master", "Master"),
        ("caves", "Caves"),
        ("islands", "Islands"),
        ("volcano", "Volcano"),
    ] {
        let relative = Path::new(directory).join("worldgenoverride.lua");
        let raw = std::str::from_utf8(&before[&relative]).unwrap();
        let body = raw.strip_prefix("KLEI     1 ").unwrap_or(raw);
        let rendered = if body.ends_with('\n') {
            format!("{body}\n")
        } else {
            format!("{body}\n\n")
        };
        expected.insert(relative, rendered.into_bytes());
        assert_eq!(
            before[&Path::new(directory).join("modoverrides.lua")],
            b"return {\n}\n\n"
        );
        assert_eq!(
            restored_settings[format!("{key}_worldgenoverride_lua")],
            body
        );
        assert_eq!(
            restored_settings[format!("{key}_modoverrides_lua")],
            "return {\n}\n\n"
        );
    }
    let after = cluster_bytes(&root);
    assert_eq!(
        after.keys().collect::<Vec<_>>(),
        expected.keys().collect::<Vec<_>>()
    );
    for (path, bytes) in &expected {
        assert!(
            after.get(path) == Some(bytes),
            "unexpected restored bytes at {path:?}"
        );
    }
    assert_eq!(
        cluster_bytes(&backup_root),
        before,
        "selected backup remains untouched"
    );
    let config_after: Value =
        serde_json::from_slice(&fs::read(&fixture.instance.config_file_path).unwrap()).unwrap();
    let changed_keys = config_before["settings"]
        .as_object()
        .unwrap()
        .keys()
        .chain(config_after["settings"].as_object().unwrap().keys())
        .filter(|key| config_before["settings"][*key] != config_after["settings"][*key])
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let expected_keys = ["master", "caves", "islands", "volcano"]
        .into_iter()
        .flat_map(|shard| {
            [
                format!("{shard}_worldgenoverride_lua"),
                format!("{shard}_modoverrides_lua"),
            ]
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        changed_keys, expected_keys,
        "all unrelated canonical settings remain equal"
    );
    assert_eq!(
        config_before
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        config_after.as_object().unwrap().keys().collect::<Vec<_>>()
    );
    for (key, value) in config_before.as_object().unwrap() {
        if key != "settings" && key != "generated_at_unix_ms" {
            assert_eq!(&config_after[key], value, "unrelated canonical field {key}");
        }
    }
    println!(
        "DST_RESTORE_CONTRACT copied_world_files=6 normalized_worldgen_files=4 managed_ini_baseline_files=4 unchanged_mod_files=4 retained_cluster_root_additions=1"
    );
}
