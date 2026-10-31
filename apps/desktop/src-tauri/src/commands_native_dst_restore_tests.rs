use super::*;
use serde_json::json;
use std::time::Duration;

const BODY: &str = "return {override_enabled=true,overrides={world_size='small'}}";

fn ini_samples() -> Value {
    serde_json::from_str(include_str!(
        "../../../../modules/dontstarve/reference-fixtures/2026-10-31-native_server_ini_761239.json"
    ))
    .unwrap()
}

struct Fixture {
    root: PathBuf,
    configuration: Value,
    original: BTreeMap<PathBuf, (u64, u64)>,
    managed_ini: BTreeMap<PathBuf, Vec<u8>>,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("native-dst-restore-{}", uuid::Uuid::new_v4()));
        let mut configuration = json!({
            "generated_at_unix_ms": 1,
            "instance_id": "fixture",
            "module_id": "dontstarve",
            "settings": {
                "shard_layout": "standard", "enable_caves": true,
                "shared_workshop_mod_ids": "", "cluster_name": "Preserve name",
                "master_world_size": "small", "caves_world_size": "small"
            },
            "ports": [{"name":"master","port":10999,"protocol":"udp"}]
        });
        let ini = ini_samples();
        for (key, directory) in SHARDS {
            let shard = root.join("saves").join(directory);
            let template = root.join("templates").join(directory);
            fs::create_dir_all(&shard).unwrap();
            fs::create_dir_all(&template).unwrap();
            let world = if matches!(key, "master" | "caves") {
                format!("KLEI     1 {BODY}")
            } else {
                format!("{BODY}\n\n")
            };
            fs::write(shard.join("worldgenoverride.lua"), world).unwrap();
            fs::write(shard.join("modoverrides.lua"), "return {\n}\n\n").unwrap();
            fs::write(
                shard.join("server.ini"),
                ini["samples"][directory]["manager"]["utf8"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
            fs::write(
                shard.join("leveldataoverride.lua"),
                "return {overrides={}}\n",
            )
            .unwrap();
            fs::write(
                template.join("worldgenoverride.lua.hbs"),
                format!("{{{{dst.{key}_worldgenoverride}}}}\n"),
            )
            .unwrap();
            configuration["settings"][format!("{key}_worldgenoverride_lua")] = json!("old options");
            configuration["settings"][format!("{key}_modoverrides_lua")] = json!("return {\n}\n");
            configuration["settings"][format!("{key}_enabled_workshop_mod_ids")] = json!("");
            configuration["settings"][format!("{key}_mod_configuration_options")] = json!({});
        }
        fs::write(
            root.join("saves/Master/native-world.bin"),
            b"world\0\x01bytes",
        )
        .unwrap();
        let original = inventory::save_inventory(&root.join("saves")).unwrap();
        let managed_ini = capture_managed_ini(&root.join("saves")).unwrap();
        Self {
            root,
            configuration,
            original,
            managed_ini,
        }
    }

    fn capture(&self) -> ProbeResult<NoModsRestore> {
        NoModsRestore::capture(
            &self.root.join("saves"),
            &serde_json::to_vec(&self.configuration)?,
            &self.root.join("templates"),
            &self.managed_ini,
        )
    }

    fn restored_configuration(&self) -> Value {
        let mut restored = self.configuration.clone();
        restored["generated_at_unix_ms"] = json!(15);
        for (key, _) in SHARDS {
            restored["settings"][format!("{key}_worldgenoverride_lua")] =
                if matches!(key, "master" | "caves") {
                    json!(BODY)
                } else {
                    json!(format!("{BODY}\n\n"))
                };
            restored["settings"][format!("{key}_modoverrides_lua")] = json!("return {\n}\n\n");
        }
        restored
    }

    fn write_rendered_options(&self) {
        for (key, directory) in SHARDS {
            fs::write(
                self.root
                    .join("saves")
                    .join(directory)
                    .join("worldgenoverride.lua"),
                if matches!(key, "master" | "caves") {
                    format!("{BODY}\n\n")
                } else {
                    format!("{BODY}\n\n\n")
                },
            )
            .unwrap();
        }
    }
}

#[test]
fn dst_native_restore_accepts_exact_stopped_ini_and_rejects_field_changes() {
    let fixture = Fixture::new();
    let samples = ini_samples();
    let root = fixture.root.join("saves");
    for shard in ["Master", "Caves"] {
        let sample = &samples["samples"][shard];
        let manager = sample["manager"]["utf8"].as_str().unwrap();
        let native = sample["stopped_native"]["utf8"].as_str().unwrap();
        assert_eq!(
            stopped_native_ini(manager.as_bytes()).unwrap(),
            native.as_bytes()
        );
        fs::write(root.join(shard).join("server.ini"), native).unwrap();
    }
    let native_inventory = inventory::save_inventory(&root).unwrap();
    let expected = fixture.capture().unwrap();
    fixture.write_rendered_options();
    assert!(expected.verify_saves(&root, &native_inventory).is_err());
    for (relative, bytes) in &fixture.managed_ini {
        fs::write(root.join(relative), bytes).unwrap();
    }
    expected.verify_saves(&root, &native_inventory).unwrap();

    let native = samples["samples"]["Master"]["stopped_native"]["utf8"]
        .as_str()
        .unwrap();
    for changed in [
        native.replace("server_port = 10999", "server_port = 12000"),
        format!("{native}unexpected = true\r\n"),
        native.replace("name = Master\r\n", ""),
        native.replace("name = Master\r\n", "name = Master\r\nname = Master\r\n"),
        samples["samples"]["Master"]["live"]["utf8"]
            .as_str()
            .unwrap()
            .to_owned(),
    ] {
        fs::write(root.join("Master/server.ini"), changed).unwrap();
        let error = fixture
            .capture()
            .err()
            .expect("altered INI must be refused");
        assert!(error.to_string().contains("backup INI differs"), "{error}");
    }
    fs::write(
        root.join("Master/server.ini"),
        &fixture.managed_ini[Path::new("Master/server.ini")],
    )
    .unwrap();
    fs::write(
        root.join("Islands/server.ini"),
        stopped_native_ini(&fixture.managed_ini[Path::new("Islands/server.ini")]).unwrap(),
    )
    .unwrap();
    let error = fixture
        .capture()
        .err()
        .expect("inactive INI must stay exact");
    assert!(error.to_string().contains("Islands"), "{error}");
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn dst_native_restore_prediction_accepts_only_exact_normalized_options_and_preserved_data() {
    let fixture = Fixture::new();
    let expected = fixture.capture().unwrap();
    fixture.write_rendered_options();
    let root = fixture.root.join("saves");
    expected.verify_saves(&root, &fixture.original).unwrap();
    for relative in [
        "Master/worldgenoverride.lua",
        "Master/native-world.bin",
        "Caves/server.ini",
        "Caves/modoverrides.lua",
        "Islands/leveldataoverride.lua",
    ] {
        let path = root.join(relative);
        let before = fs::read(&path).unwrap();
        fs::write(&path, b"different world or unrelated data").unwrap();
        let error = expected
            .verify_saves(&root, &fixture.original)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(relative.rsplit('/').next().unwrap()),
            "{error}"
        );
        assert!(
            !error.contains("different world or unrelated data"),
            "no file content in diagnostic"
        );
        fs::remove_file(&path).unwrap();
        assert!(
            expected.verify_saves(&root, &fixture.original).is_err(),
            "missing {relative}"
        );
        fs::write(path, before).unwrap();
    }
    fs::write(root.join("extra-native-file"), b"unexpected").unwrap();
    assert!(expected.verify_saves(&root, &fixture.original).is_err());
    let mut retained = fixture.original.clone();
    retained.insert(
        PathBuf::from("extra-native-file"),
        inventory::save_inventory(&root.join("extra-native-file"))
            .unwrap()
            .remove(Path::new(""))
            .unwrap(),
    );
    expected.verify_saves(&root, &retained).unwrap();
    fs::write(root.join("extra-native-file"), b"changed operator data").unwrap();
    assert!(expected.verify_saves(&root, &retained).is_err());
}

#[test]
fn dst_native_restore_prediction_rejects_other_configuration_changes_and_invalid_timestamps() {
    let fixture = Fixture::new();
    let expected = fixture.capture().unwrap();
    let restored = fixture.restored_configuration();
    let verify = |value: &Value| {
        expected.verify_configuration(
            &serde_json::to_vec(value).unwrap(),
            UNIX_EPOCH + Duration::from_millis(10),
            UNIX_EPOCH + Duration::from_millis(20),
        )
    };
    verify(&restored).unwrap();
    for (pointer, wrong) in [
        (
            "/settings/master_worldgenoverride_lua",
            json!("return {overrides={world_size='huge'}}"),
        ),
        ("/settings/master_world_size", json!("huge")),
        ("/settings/cluster_name", json!("different server")),
        (
            "/settings/master_modoverrides_lua",
            json!("return {['workshop-123']={enabled=true}}"),
        ),
        ("/ports/0/port", json!(12000)),
        ("/generated_at_unix_ms", json!(9)),
        ("/generated_at_unix_ms", json!(21)),
        ("/generated_at_unix_ms", json!("15")),
    ] {
        let mut changed = restored.clone();
        *changed.pointer_mut(pointer).unwrap() = wrong;
        assert!(verify(&changed).is_err(), "{pointer}");
    }
    let mut changed = restored;
    changed["settings"]["unexpected_setting"] = json!(true);
    assert!(verify(&changed).is_err());
}

#[test]
fn dst_native_restore_prediction_refuses_modded_fixtures_and_changed_templates() {
    let mut fixture = Fixture::new();
    fixture.configuration["settings"]["master_enabled_workshop_mod_ids"] = json!("123");
    assert!(fixture.capture().is_err());
    fixture.configuration["settings"]["master_enabled_workshop_mod_ids"] = json!("");
    fs::write(
        fixture.root.join("saves/Master/modoverrides.lua"),
        "return {['workshop-123']={enabled=true}}",
    )
    .unwrap();
    assert!(fixture.capture().is_err());
    fs::write(
        fixture.root.join("saves/Master/modoverrides.lua"),
        "return {\n}\n\n",
    )
    .unwrap();
    fs::write(
        fixture
            .root
            .join("templates/Master/worldgenoverride.lua.hbs"),
        "changed template",
    )
    .unwrap();
    assert!(fixture.capture().is_err());
}

#[test]
fn dst_native_restore_counts_world_data_and_mutates_a_real_save_file() {
    let files = BTreeMap::from([
        (PathBuf::from("cluster.ini"), (100, 0)),
        (PathBuf::from("unrelated-root.txt"), (200, 0)),
        (PathBuf::from("Master/server.ini"), (300, 0)),
        (PathBuf::from("Master/worldgenoverride.lua"), (400, 0)),
        (PathBuf::from("Caves/modoverrides.lua"), (500, 0)),
        (PathBuf::from("Master/save/session/OLD/0000000001"), (1, 0)),
        (PathBuf::from("Caves/save/shardindex"), (2, 0)),
        (PathBuf::from("Master/leveldataoverride.lua"), (4, 0)),
        (PathBuf::from("Islands/leveldataoverride.lua"), (8, 0)),
        (PathBuf::from("Volcano/worldgenoverride.lua"), (600, 0)),
    ]);
    assert_eq!(copied_world_data(&files), (4, 15));
    for (path, _) in files {
        assert_eq!(
            is_native_save_file(&path),
            path.to_str() == Some("Master/save/session/OLD/0000000001")
        );
    }
    assert!(is_native_save_file(Path::new(
        "Caves/save/session/OLD/0000000002"
    )));
    assert!(!is_native_save_file(Path::new(
        "Caves/save/session/OLD/0000000002.meta"
    )));
}
