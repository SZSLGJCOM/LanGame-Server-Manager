use super::*;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("dst-import-mods-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn shard(&self, key: &str, overrides: Option<&str>, index: &str) -> PathBuf {
        let root = self.0.join(key);
        fs::create_dir_all(root.join("save")).unwrap();
        fs::write(root.join("save/shardindex"), index).unwrap();
        if let Some(overrides) = overrides {
            fs::write(root.join("modoverrides.lua"), overrides).unwrap();
        }
        root
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn dst_import_mods_replaces_target_enablement_preserves_source_options_and_merges_downloads() {
    let fixture = Fixture::new();
    let raw = "-- original source options\nreturn {['workshop-1505270912']={enabled=true,configuration_options={region='islands',nested={[4]='four',false}}},client_mods_disabled=true}";
    let master = fixture.shard("master", Some(raw), "return { enabled_mods = {} }");
    let caves = fixture.shard("caves", Some("return {['workshop-1392778117']={enabled=true,configuration_options={Language='chinese'}},['workshop-111']={enabled=false}}"), "return {}");
    let plan = read_import_mods(&[("master", &master), ("caves", &caves)]).unwrap();
    assert_eq!(plan.workshop_ids, ["1392778117", "1505270912"]);
    let mut settings = json!({
        "cluster_name": "Keep server", "world_day": "longday",
        "shared_workshop_mod_ids": "222;1505270912",
        "master_enabled_workshop_mod_ids": "222", "caves_enabled_workshop_mod_ids": "333",
        "master_mod_configuration_options": {"222": {"wrong": true}},
        "caves_mod_configuration_options": {"333": {"wrong": false}},
        "islands_modoverrides_lua": "return {['workshop-444']={enabled=true}}",
        "volcano_enabled_workshop_mod_ids": "555",
        "dst_removed_workshop_mod_ids": ["1505270912", "999"]
    })
    .as_object()
    .unwrap()
    .clone();
    plan.apply(&mut settings).unwrap();
    assert_eq!(settings["master_modoverrides_lua"], raw);
    assert_eq!(
        settings["shared_workshop_mod_ids"],
        "1392778117\n1505270912\n222"
    );
    assert_eq!(settings["master_enabled_workshop_mod_ids"], "");
    assert_eq!(settings["caves_enabled_workshop_mod_ids"], "");
    assert!(!settings.contains_key("master_mod_configuration_options"));
    assert!(!settings.contains_key("caves_mod_configuration_options"));
    assert_eq!(settings["islands_modoverrides_lua"], "");
    assert_eq!(settings["volcano_enabled_workshop_mod_ids"], "");
    assert_eq!(settings["dst_removed_workshop_mod_ids"], json!(["999"]));
    assert_eq!(settings["cluster_name"], "Keep server");
    assert_eq!(settings["world_day"], "longday");
    plan.verify_unchanged().unwrap();
}

#[test]
fn dst_import_mods_uses_each_shards_saved_enabled_mods_when_overrides_are_missing() {
    let fixture = Fixture::new();
    let master = fixture.shard("master", None, "return {session_id='a',world={options={}},enabled_mods={['workshop-123']={enabled=true,configuration_options={seed=3,keep=false,tag='中文'}}}}");
    let caves_index = "return {session_id='b',enabled_mods={['workshop-456']={enabled=true,configuration_options={cave=true}}}}";
    let caves = fixture.shard("caves", None, caves_index);
    let plan = read_import_mods(&[("master", &master), ("caves", &caves)]).unwrap();
    assert_eq!(plan.workshop_ids, ["123", "456"]);
    assert!(plan.sources[0].raw.contains("keep=false"));
    assert!(plan.sources[0].raw.contains("中文"));
    assert!(plan.sources[1].raw.contains("cave=true"));
    let snapshot = plan
        .snapshots
        .iter()
        .find(|snapshot| snapshot.path == caves.join("save/shardindex"))
        .expect("the recovered cave Mod options retain their source snapshot");
    assert_eq!(snapshot.content.as_deref(), Some(caves_index.as_bytes()));
}

#[test]
fn dst_import_mods_recovers_four_shards_without_merging_different_options() {
    let fixture = Fixture::new();
    let roots = SHARD_KEYS.iter().enumerate().map(|(index, key)| fixture.shard(key, None,
        &format!("return {{ enabled_mods = {{ ['workshop-123'] = {{ enabled = true, configuration_options = {{ region='{key}', number={index} }} }} }} }}"))).collect::<Vec<_>>();
    let shards = SHARD_KEYS
        .iter()
        .zip(&roots)
        .map(|(key, root)| (*key, root.as_path()))
        .collect::<Vec<_>>();
    let plan = read_import_mods(&shards).unwrap();
    let mut settings = Map::new();
    plan.apply(&mut settings).unwrap();
    assert_eq!(plan.workshop_ids, ["123"]);
    for key in SHARD_KEYS {
        assert!(
            settings
                .get(&format!("{key}_modoverrides_lua"))
                .unwrap()
                .as_str()
                .unwrap()
                .contains(&format!("region='{key}'"))
        );
    }
}

#[test]
fn dst_import_mods_rejects_incomplete_dynamic_or_local_mod_configuration() {
    let fixture = Fixture::new();
    let cases = [
        (
            "missing",
            None,
            "return {session_id='a'}",
            "no enabled_mods",
        ),
        (
            "dynamic",
            Some("local x={}; return x"),
            "return {enabled_mods={}}",
            "static Lua",
        ),
        (
            "local",
            Some("return {my_local_mod={enabled=true}}"),
            "return {}",
            "local Mod my_local_mod",
        ),
        (
            "unspecified",
            Some("return {['workshop-123']={configuration_options={}}}"),
            "return {}",
            "declare enabled",
        ),
        (
            "invalid-id",
            Some("return {['workshop-00123']={enabled=true}}"),
            "return {}",
            "invalid Workshop",
        ),
        (
            "dynamic-index",
            None,
            "return {enabled_mods=get_mods()}",
            "Cannot recover",
        ),
    ];
    for (label, raw, index, expected) in cases {
        let root = fixture.shard(label, raw, index);
        let error = read_import_mods(&[("master", &root)]).unwrap_err();
        assert!(error.contains(expected), "{label}: {error}");
    }
}

#[test]
fn dst_import_mods_accepts_explicit_vanilla_index_and_utf8_bom() {
    let fixture = Fixture::new();
    let vanilla = fixture.shard("vanilla", None, "return {session_id='a',enabled_mods={}}");
    assert!(
        read_import_mods(&[("master", &vanilla)])
            .unwrap()
            .workshop_ids
            .is_empty()
    );
    let bom = fixture.shard(
        "bom",
        Some("\u{feff}return {['workshop-123']={enabled=true}}"),
        "return {}",
    );
    assert_eq!(
        read_import_mods(&[("master", &bom)]).unwrap().workshop_ids,
        ["123"]
    );
}

#[test]
fn dst_import_mods_reads_the_uncompressed_klei_text_envelope_without_lua_execution() {
    let fixture = Fixture::new();
    let root = fixture.shard("master", None, "KLEI     1 return {enabled_mods={['workshop-1505270912']={enabled=true,configuration_options={custom={[5]='five',false}}}}}");
    let plan = read_import_mods(&[("master", &root)]).unwrap();
    assert_eq!(plan.workshop_ids, ["1505270912"]);
    assert!(plan.sources[0].raw.contains("[5]='five',false"));
    fs::write(
        root.join("save/shardindex"),
        "KLEI     2 return {enabled_mods={}}",
    )
    .unwrap();
    assert!(read_import_mods(&[("master", &root)]).is_err());
}

#[test]
fn dst_import_mods_detects_configuration_changed_or_added_during_download() {
    let fixture = Fixture::new();
    let root = fixture.shard("master", None, "return {enabled_mods={}}");
    let plan = read_import_mods(&[("master", &root)]).unwrap();
    fs::write(
        root.join("modoverrides.lua"),
        "return {['workshop-123']={enabled=true}}",
    )
    .unwrap();
    assert!(plan.verify_unchanged().unwrap_err().contains("changed"));
    fs::remove_file(root.join("modoverrides.lua")).unwrap();
    fs::write(
        root.join("save/shardindex"),
        "return {enabled_mods={['workshop-456']={enabled=true}}}",
    )
    .unwrap();
    assert!(plan.verify_unchanged().unwrap_err().contains("changed"));
}

#[test]
fn dst_import_mods_rejects_limits_duplicates_and_unsafe_file_types() {
    let fixture = Fixture::new();
    let root = fixture.shard(
        "master",
        Some(&" ".repeat(MAX_CONFIGURATION_BYTES + 1)),
        "return {}",
    );
    assert!(
        read_import_mods(&[("master", &root)])
            .unwrap_err()
            .contains("recovery limit")
    );
    fs::remove_file(root.join("modoverrides.lua")).unwrap();
    fs::create_dir(root.join("modoverrides.lua")).unwrap();
    assert!(
        read_import_mods(&[("master", &root)])
            .unwrap_err()
            .contains("plain file")
    );
    fs::remove_dir(root.join("modoverrides.lua")).unwrap();
    fs::write(root.join("modoverrides.lua"), "return {}").unwrap();
    assert!(
        read_import_mods(&[("master", &root), ("master", &root)])
            .unwrap_err()
            .contains("duplicate")
    );
    assert!(
        read_import_mods(&[("unrecognized", &root)])
            .unwrap_err()
            .contains("Unsupported")
    );
}

#[test]
fn dst_import_mods_does_not_partially_change_settings_when_existing_download_list_is_invalid() {
    let fixture = Fixture::new();
    let root = fixture.shard(
        "master",
        Some("return {['workshop-123']={enabled=true}}"),
        "return {}",
    );
    let plan = read_import_mods(&[("master", &root)]).unwrap();
    let mut settings =
        json!({"shared_workshop_mod_ids": "not-an-id", "master_enabled_workshop_mod_ids": "222"})
            .as_object()
            .unwrap()
            .clone();
    let before = settings.clone();
    assert!(plan.apply(&mut settings).is_err());
    assert_eq!(settings, before);
}
