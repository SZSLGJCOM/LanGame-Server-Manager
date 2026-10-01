use super::*;
use crate::templates::rimworld::{
    preserve_retired_settings, render_rimworld_password_json, validate_before_start,
};

struct RimWorldFixture {
    root: PathBuf,
    paths: StoragePaths,
    config: PathBuf,
    install: PathBuf,
}

impl RimWorldFixture {
    fn new() -> Self {
        let root = unique_test_root();
        let paths = test_storage_paths(&root);
        let config = root.join("instance/config");
        let install = root.join("instance/runtime");
        fs::create_dir_all(&config).unwrap();
        fs::create_dir_all(install.join("Configs")).unwrap();
        fs::write(
            config.join("ServerConfig.json"),
            b"{\"UseClientSave\":false}",
        )
        .unwrap();
        Self {
            root,
            paths,
            config,
            install,
        }
    }

    fn context<'a>(
        &'a self,
        settings: &'a Map<String, Value>,
    ) -> ModuleSupportMaterializationContext<'a> {
        ModuleSupportMaterializationContext {
            storage_paths: &self.paths,
            module_id: "rimworld",
            install_root: &self.install,
            shared_install_root: &self.install,
            config_dir: &self.config,
            saves_dir: &self.root,
            instance_id: "rimworld-password",
            instance_running: false,
            settings,
        }
    }
}

impl Drop for RimWorldFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn password_matches_official_ascii_sha256_format() {
    let settings = json!({"server_password":"abc"})
        .as_object()
        .unwrap()
        .clone();
    assert_eq!(
        render_rimworld_password_json(&settings),
        "\"BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD\""
    );
    assert_eq!(render_rimworld_password_json(&Map::new()), "\"\"");
}

#[test]
fn partial_updates_keep_historical_rosters_restrictions_and_password() {
    let persisted =
        json!({"use_whitelist":true,"whitelisted_users":["private-user"],"server_password":"abc"})
            .as_object()
            .unwrap()
            .clone();
    let mut incoming = json!({"extra_launch_args":"-test","use_whitelist":false})
        .as_object()
        .unwrap()
        .clone();
    preserve_retired_settings(&persisted, &mut incoming);
    assert_eq!(incoming["use_whitelist"], true);
    assert_eq!(
        incoming["whitelisted_users"],
        persisted["whitelisted_users"]
    );
    assert_eq!(incoming["server_password"], "abc");
    incoming.insert("server_password".into(), json!(""));
    preserve_retired_settings(&persisted, &mut incoming);
    assert_eq!(incoming["server_password"], "");
}

#[test]
fn old_whitelist_blocks_start_until_password_materialized_and_keeps_lists() {
    let fixture = RimWorldFixture::new();
    let old = b"{\"UseWhitelist\":true,\"WhitelistedUsers\":[\"private-user\"]}\n";
    fs::write(fixture.install.join("Configs/WhitelistConfig.json"), old).unwrap();
    fs::write(fixture.config.join("WhitelistConfig.json"), old).unwrap();
    let empty = Map::new();
    materialize_module_support_files(&fixture.context(&empty)).unwrap();
    let error = validate_before_start(&fixture.context(&empty))
        .unwrap_err()
        .to_string();
    assert!(error.contains("server password"));
    assert!(!error.contains("private-user"));
    let settings = json!({"server_password":"abc"})
        .as_object()
        .unwrap()
        .clone();
    materialize_module_support_files(&fixture.context(&settings)).unwrap();
    validate_before_start(&fixture.context(&settings)).unwrap();
    assert_eq!(
        fs::read(fixture.install.join("Configs/WhitelistConfig.json")).unwrap(),
        old
    );
    assert_eq!(
        fs::read(fixture.config.join("WhitelistConfig.json")).unwrap(),
        old
    );
    let password: Value = serde_json::from_slice(
        &fs::read(fixture.install.join("Configs/PasswordConfig.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        password["Password"],
        "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD"
    );
}

#[test]
fn unset_password_preserves_native_state_and_explicit_clear_restores_gate() {
    let fixture = RimWorldFixture::new();
    let native = b"{\"Password\":\"BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD\",\"Future\":7}\n";
    let path = fixture.install.join("Configs/PasswordConfig.json");
    fs::write(&path, native).unwrap();
    let settings = json!({"use_whitelist":true}).as_object().unwrap().clone();
    materialize_module_support_files(&fixture.context(&settings)).unwrap();
    validate_before_start(&fixture.context(&settings)).unwrap();
    assert_eq!(fs::read(&path).unwrap(), native);
    let settings = json!({"use_whitelist":true,"server_password":""})
        .as_object()
        .unwrap()
        .clone();
    materialize_module_support_files(&fixture.context(&settings)).unwrap();
    assert!(validate_before_start(&fixture.context(&settings)).is_err());
    let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["Password"], "");
    assert_eq!(saved["Future"], 7);
}

#[test]
fn new_unrestricted_instance_starts_without_password() {
    let fixture = RimWorldFixture::new();
    materialize_module_support_files(&fixture.context(&Map::new())).unwrap();
    validate_before_start(&fixture.context(&Map::new())).unwrap();
    assert!(
        !fixture
            .install
            .join("Configs/WhitelistConfig.json")
            .exists()
    );
}

#[test]
fn malformed_native_password_is_not_overwritten() {
    let fixture = RimWorldFixture::new();
    let path = fixture.install.join("Configs/PasswordConfig.json");
    fs::write(&path, b"private-malformed-content").unwrap();
    let settings = json!({"server_password":"abc"})
        .as_object()
        .unwrap()
        .clone();
    let error = materialize_module_support_files(&fixture.context(&settings))
        .unwrap_err()
        .to_string();
    assert!(!error.contains("private-malformed-content"));
    assert_eq!(fs::read(path).unwrap(), b"private-malformed-content");
}

#[test]
fn native_overrides_apply_with_unset_password_and_preserve_nested_unknown_values() {
    let fixture = RimWorldFixture::new();
    let password = b"{\"Password\":\"synthetic-fixture\"}\n";
    fs::write(
        fixture.install.join("Configs/PasswordConfig.json"),
        password,
    )
    .unwrap();
    fs::create_dir_all(fixture.install.join("Configs/Actions")).unwrap();
    fs::write(
        fixture.install.join("Configs/ChatConfig.json"),
        b"{\"EnableMoTD\":false,\"Future\":19}",
    )
    .unwrap();
    fs::write(
        fixture.install.join("Configs/Actions/Road.json"),
        b"{\"RoadValues\":{\"DirtRoadCost\":50,\"Future\":17},\"Future\":23}",
    )
    .unwrap();
    let settings = json!({"chat_enable_mo_td":true, "difficulty_threat_scale":1.75,
        "action_road_dirt_road_cost":42})
    .as_object()
    .unwrap()
    .clone();
    materialize_module_support_files(&fixture.context(&settings)).unwrap();
    assert_eq!(
        fs::read(fixture.install.join("Configs/PasswordConfig.json")).unwrap(),
        password
    );
    let read = |relative: &str| -> Value {
        serde_json::from_slice(&fs::read(fixture.install.join(relative)).unwrap()).unwrap()
    };
    assert_eq!(
        read("Configs/ChatConfig.json"),
        json!({"EnableMoTD":true,"Future":19})
    );
    assert_eq!(
        read("Configs/DifficultyConfig.json"),
        json!({"ThreatScale":1.75})
    );
    assert_eq!(
        read("Configs/Actions/Road.json"),
        json!({"RoadValues":{"DirtRoadCost":42,"Future":17},"Future":23})
    );
    assert!(!fixture.install.join("Configs/ScenarioConfig.json").exists());
    assert!(!fixture.install.join("Assets/WorldValuesFile.json").exists());
}

#[test]
fn external_native_edit_after_read_is_not_overwritten() {
    let fixture = RimWorldFixture::new();
    let path = fixture.install.join("Configs/ChatConfig.json");
    fs::write(&path, b"{\"EnableMoTD\":false}").unwrap();
    let settings = json!({"chat_enable_mo_td":true})
        .as_object()
        .unwrap()
        .clone();
    let plans =
        crate::templates::rimworld::native::plan_overrides(&fixture.context(&settings)).unwrap();
    fs::write(&path, b"{\"EnableMoTD\":false,\"External\":19}").unwrap();
    let mut mutation = ManagedConfigMutation::new("rimworld");
    assert!(mutation.apply(plans).is_err());
    mutation.rollback().unwrap();
    assert_eq!(
        fs::read(path).unwrap(),
        b"{\"EnableMoTD\":false,\"External\":19}"
    );
}

#[test]
fn server_config_preserves_unknown_fields_across_real_template_materialization() {
    let fixture = RimWorldFixture::new();
    let path = fixture.install.join("Configs/ServerConfig.json");
    let unknown = json!({
        "nested": {"enabled": false, "items": [null, 0, "", {"value": 17}]}
    });
    fs::write(
        &path,
        serde_json::to_vec(&json!({
            "Name": "Native name",
            "UseClientSave": true,
            "MaxPlayers": 99,
            "FutureNative": unknown,
            "FutureScalar": 0
        }))
        .unwrap(),
    )
    .unwrap();
    let mut settings = collect_schema_defaults_from_schema_json(
        Some(include_str!("../../../modules/rimworld/schema.json")),
        SchemaDefaultContext {
            instance_id: Some("rimworld-password"),
            instance_name: Some("RimWorld regression"),
        },
    )
    .unwrap();
    let ports = [PortBinding {
        name: "game".into(),
        protocol: "tcp".into(),
        port: 25555,
    }];
    let templates = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules/rimworld/templates");
    for (name, use_client_save, max_players) in
        [("First save", false, 17), ("Second save", true, 23)]
    {
        settings.insert("server_name".into(), json!(name));
        settings.insert("sync_local_save".into(), json!(use_client_save));
        settings.insert("max_players".into(), json!(max_players));
        render_module_templates(
            &templates,
            &ModuleTemplateRenderInput {
                config_dir: &fixture.config,
                install_root: &fixture.install,
                saves_dir: &fixture.root,
                instance_id: "rimworld-password",
                instance_name: "RimWorld regression",
                module_id: "rimworld",
                bind_ip: "127.0.0.1",
                autostart: false,
                settings: &settings,
                ports: &ports,
            },
        )
        .unwrap();
        materialize_module_support_files(&fixture.context(&settings)).unwrap();
        let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["Name"], name);
        assert_eq!(saved["UseClientSave"], use_client_save);
        assert_eq!(saved["MaxPlayers"], max_players);
        assert_eq!(saved["Port"], 25555);
        assert_eq!(saved["FutureNative"], unknown);
        assert_eq!(saved["FutureScalar"], 0);
    }
}

#[test]
fn malformed_server_config_is_preserved_and_rolls_back_other_pending_writes() {
    let fixture = RimWorldFixture::new();
    let path = fixture.install.join("Configs/ServerConfig.json");
    let original = b"{\"FutureNative\":unfinished";
    fs::write(&path, original).unwrap();
    let result = materialize_module_support_files(&fixture.context(&Map::new()));
    assert!(
        result.is_err(),
        "malformed native data must not be replaced"
    );
    assert_eq!(fs::read(path).unwrap(), original);
    assert!(!fixture.install.join("Configs/PasswordConfig.json").exists());
}
