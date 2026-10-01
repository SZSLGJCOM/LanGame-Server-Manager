use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use super::*;

#[path = "../../config_acceptance_test_support.rs"]
mod config_acceptance_support;
use config_acceptance_support::*;
#[path = "config_acceptance_formats.rs"]
mod config_acceptance_formats;
#[path = "config_acceptance_runner.rs"]
mod config_acceptance_runner;
use config_acceptance_runner::run_storage_acceptance_repository;

#[test]
fn fixture_selection_defaults_to_all_and_rejects_explicit_empty_values() {
    let known = BTreeSet::from([String::from("minecraft"), String::from("rust")]);
    let fixtures = BTreeMap::from([
        (
            String::from("minecraft"),
            vec![PathBuf::from("minecraft.json")],
        ),
        (String::from("rust"), vec![PathBuf::from("rust.json")]),
    ]);

    assert_eq!(
        resolve_fixture_selection(&known, &fixtures, None).unwrap(),
        vec![String::from("minecraft"), String::from("rust")]
    );
    for requested in ["", "   ", ", ,"] {
        assert!(
            resolve_fixture_selection(&known, &fixtures, Some(requested))
                .unwrap_err()
                .contains("must not be empty")
        );
    }
}

#[test]
fn fixture_selection_without_target_rejects_fixtureless_modules() {
    let known = BTreeSet::from([String::from("minecraft"), String::from("rust")]);
    let fixtures = BTreeMap::from([(
        String::from("minecraft"),
        vec![PathBuf::from("minecraft.json")],
    )]);

    let error = resolve_fixture_selection(&known, &fixtures, None).unwrap_err();

    assert!(error.contains("rust"));
    assert!(error.contains("no fixtures"));
}

#[test]
fn fixture_selection_without_target_rejects_unknown_fixture_modules() {
    let known = BTreeSet::from([String::from("minecraft")]);
    let fixtures = BTreeMap::from([
        (
            String::from("minecraft"),
            vec![PathBuf::from("minecraft.json")],
        ),
        (String::from("ghost"), vec![PathBuf::from("ghost.json")]),
    ]);

    let error = resolve_fixture_selection(&known, &fixtures, None).unwrap_err();

    assert!(error.contains("ghost"));
    assert!(error.contains("unknown modules"));
}

#[test]
fn fixture_selection_rejects_duplicate_unknown_and_fixtureless_ids() {
    let known = BTreeSet::from([String::from("minecraft"), String::from("rust")]);
    let fixtures = BTreeMap::from([(
        String::from("minecraft"),
        vec![PathBuf::from("minecraft.json")],
    )]);

    assert_eq!(
        resolve_fixture_selection(&known, &fixtures, Some("minecraft")).unwrap(),
        vec![String::from("minecraft")]
    );
    assert!(
        resolve_fixture_selection(&known, &fixtures, Some("minecraft,minecraft"))
            .unwrap_err()
            .contains("duplicate")
    );
    assert!(
        resolve_fixture_selection(&known, &fixtures, Some("unknown"))
            .unwrap_err()
            .contains("unknown")
    );
    assert!(
        resolve_fixture_selection(&known, &fixtures, Some("rust"))
            .unwrap_err()
            .contains("no fixtures")
    );
}

#[test]
fn repository_discovery_requires_storage_fixture_for_every_module() {
    let modules_root = repository_root().join("modules");
    let modules =
        app_modules::discover_modules(&modules_root).expect("discover repository modules");
    let known = modules
        .iter()
        .map(|module| module.summary.id.clone())
        .collect::<BTreeSet<_>>();
    let fixtures = discover_fixture_paths(&modules).expect("discover repository fixtures");

    assert_eq!(known.len(), 32);
    assert_eq!(
        fixtures.keys().cloned().collect::<BTreeSet<_>>(),
        known,
        "every discovered module must have storage acceptance fixtures"
    );
    assert_eq!(
        resolve_fixture_selection(&known, &fixtures, None).unwrap(),
        fixtures.keys().cloned().collect::<Vec<_>>()
    );
}

#[test]
fn controlled_fixture_renders_and_validates_json_output() {
    let repository = ControlledRepository::new();
    repository.write_module();
    repository.write_fixture(
        r#"{
  "fixture_version": 1,
  "module_id": "acceptancealpha",
  "settings": {"server_name": "LanGame 配置", "required_setting": "ready"},
  "expected": {
    "files": [{
      "root": "config",
      "path": "server.json",
      "format": "json",
      "keys": {"name": "LanGame 配置", "port": 27115}
    }, {
      "root": "config",
      "path": "motd.txt",
      "format": "text",
      "fragments": ["Welcome to LanGame 配置"]
    }, {
      "root": "config",
      "path": "server.properties",
      "format": "properties",
      "keys": {"motd": "LanGame 配置", "server-port": 27115}
    }, {
      "root": "config",
      "path": "Game.ini",
      "format": "ini",
      "entries": [
        {"key": "Server.Name", "value": "LanGame 配置"},
        {"key": "Server.Option", "value": "first"},
        {"key": "Server.Option", "value": "second"},
        {"key": "Server.Port", "value": 27115}
      ]
    }],
    "launch": {"executable_suffix": null, "arguments": []}
  }
}"#,
    );

    run_storage_acceptance_repository(repository.modules_root(), None)
        .expect("controlled fixture should match rendered output");
}

#[test]
fn controlled_fixture_renders_the_instance_creation_default_bind_ip() {
    let repository = ControlledRepository::new();
    repository.write_module();
    repository.write_fixture(
        r#"{
  "fixture_version": 1,
  "module_id": "acceptancealpha",
  "settings": {"required_setting": "ready"},
  "expected": {
    "files": [{
      "root": "config",
      "path": "server.json",
      "format": "json",
      "keys": {"bind_ip": "0.0.0.0"}
    }],
    "launch": {"executable_suffix": null, "arguments": []}
  }
}"#,
    );

    run_storage_acceptance_repository(repository.modules_root(), None)
        .expect("storage acceptance must use the production instance creation bind default");
}

#[test]
fn controlled_fixture_uses_schema_defaults_and_rejects_invalid_settings() {
    let defaults = ControlledRepository::new();
    defaults.write_module();
    defaults.write_fixture(
        r#"{
  "fixture_version": 1,
  "module_id": "acceptancealpha",
  "settings": {"required_setting": "ready"},
  "expected": {
    "files": [{
      "root": "config",
      "path": "server.json",
      "format": "json",
      "keys": {"name": "Default Server"}
    }],
    "launch": {"executable_suffix": null, "arguments": []}
  }
}"#,
    );
    run_storage_acceptance_repository(defaults.modules_root(), None)
        .expect("schema default must drive native output");

    let invalid = ControlledRepository::new();
    invalid.write_module();
    invalid.write_fixture(
        r#"{
  "fixture_version": 1,
  "module_id": "acceptancealpha",
  "settings": {"server_name": 42, "required_setting": "ready"},
  "expected": {
    "files": [{
      "root": "config",
      "path": "server.json",
      "format": "json",
      "keys": {"name": 42}
    }],
    "launch": {"executable_suffix": null, "arguments": []}
  }
}"#,
    );
    assert!(
        run_storage_acceptance_repository(invalid.modules_root(), None)
            .unwrap_err()
            .contains("invalid fixture settings")
    );

    let incomplete = ControlledRepository::new();
    incomplete.write_module();
    incomplete.write_fixture(
        r#"{
  "fixture_version": 1,
  "module_id": "acceptancealpha",
  "settings": {},
  "expected": {
    "files": [{"root": "config", "path": "server.json", "format": "json", "keys": {"name": "Default Server"}}],
    "launch": {"executable_suffix": null, "arguments": []}
  }
}"#,
    );
    assert!(
        run_storage_acceptance_repository(incomplete.modules_root(), None)
            .unwrap_err()
            .contains("required_setting")
    );
}

#[test]
fn controlled_fixture_rejects_malformed_json_and_escaping_output_paths() {
    let malformed = ControlledRepository::new();
    malformed.write_module();
    malformed.write_fixture("{");
    assert!(
        run_storage_acceptance_repository(malformed.modules_root(), None)
            .unwrap_err()
            .contains("invalid fixture JSON")
    );

    let escaping = ControlledRepository::new();
    escaping.write_module();
    escaping.write_fixture(
        r#"{
  "fixture_version": 1,
  "module_id": "acceptancealpha",
  "settings": {},
  "expected": {
    "files": [{"root": "config", "path": "../secret.json", "format": "json", "keys": {}}],
    "launch": {"executable_suffix": null, "arguments": []}
  }
}"#,
    );
    assert!(
        run_storage_acceptance_repository(escaping.modules_root(), None)
            .unwrap_err()
            .contains("unsafe relative path")
    );
}

#[test]
fn repository_config_acceptance_fixtures_match_storage_outputs() {
    let requested = std::env::var("GAME_CONFIG_ACCEPTANCE_MODULES").ok();
    run_storage_acceptance_repository(repository_root().join("modules"), requested.as_deref())
        .expect("repository storage acceptance fixtures");
}

#[test]
fn barotrauma_fractional_fixture_survives_schema_and_native_output() {
    let modules_root = repository_root().join("modules");
    let modules = app_modules::discover_modules(&modules_root).unwrap();
    let module = modules
        .iter()
        .find(|module| module.summary.id == "barotrauma")
        .unwrap();
    let fixture = read_acceptance_fixture(
        &module
            .root
            .join("config-fixtures/2026-07-13-server_settings.json"),
    )
    .unwrap();
    let normalized = normalize_complete_instance_settings(
        Some(module),
        fixture.settings.clone(),
        "acceptance-barotrauma",
        CONFIG_ACCEPTANCE_INSTANCE_NAME,
        app_core::INSTANCE_CREATION_DEFAULT_BIND_IP,
    )
    .expect("fractional native settings must pass production schema validation");
    for (key, value) in [
        ("respawn_interval", 2.5),
        ("campaign_oxygen_multiplier", 1.25),
        ("campaign_crew_vitality_multiplier", 3.25),
    ] {
        assert_eq!(normalized.get(key), Some(&serde_json::json!(value)));

        let mut integer_descriptor = module.clone();
        let mut schema: serde_json::Value =
            serde_json::from_str(module.schema_json.as_deref().unwrap()).unwrap();
        schema["properties"][key]["type"] = serde_json::json!("integer");
        integer_descriptor.schema_json = Some(schema.to_string());
        let error = normalize_complete_instance_settings(
            Some(&integer_descriptor),
            fixture.settings.clone(),
            "acceptance-barotrauma",
            CONFIG_ACCEPTANCE_INSTANCE_NAME,
            app_core::INSTANCE_CREATION_DEFAULT_BIND_IP,
        )
        .expect_err("the old integer schema must reject this fractional regression fixture");
        assert!(matches!(
            error,
            crate::StorageError::InvalidModuleSetting { field, .. } if field == key
        ));
    }

    run_storage_acceptance_repository(modules_root, Some("barotrauma"))
        .expect("fractional values must survive native XML writes and disk reads");
}

#[test]
fn conanexiles_fractional_fixture_survives_schema_and_native_output() {
    let modules_root = repository_root().join("modules");
    let modules = app_modules::discover_modules(&modules_root).unwrap();
    let module = modules
        .iter()
        .find(|module| module.summary.id == "conanexiles")
        .unwrap();
    let fixture = read_acceptance_fixture(
        &module
            .root
            .join("config-fixtures/2026-07-13-steamcmd_anonymous_validate_443030.json"),
    )
    .unwrap();
    let normalized = normalize_complete_instance_settings(
        Some(module),
        fixture.settings.clone(),
        "acceptance-conanexiles",
        CONFIG_ACCEPTANCE_INSTANCE_NAME,
        app_core::INSTANCE_CREATION_DEFAULT_BIND_IP,
    )
    .expect("documented floating-point settings must pass production schema validation");
    for (key, value) in [
        ("avatar_lifetime", 600.5),
        ("chat_local_radius", 5000.5),
        ("land_claim_radius_multiplier", 1.25),
        ("item_convertion_multiplier", 1.25),
        ("thrall_corruption_removal_multiplier", 1.25),
        ("player_corruption_gain_multiplier", 1.25),
        ("player_corruption_gain_from_sorcery_multiplier", 1.25),
        ("animal_pen_crafting_time_multiplier", 1.25),
        ("feed_box_range_multiplier", 1.25),
        ("unconscious_time_seconds", 1800.5),
        ("stability_loss_multiplier", 1.25),
        ("healthbar_visibility_distance", 15000.5),
        ("thrall_decay_time", 1296000.25),
        ("player_corpse_life_time", 1800.25),
        ("thrall_scouting_time_minutes", 10.25),
        ("avatar_summon_time", 60.25),
    ] {
        assert_eq!(normalized.get(key), Some(&serde_json::json!(value)));
        let mut integer_descriptor = module.clone();
        let mut schema: serde_json::Value =
            serde_json::from_str(module.schema_json.as_deref().unwrap()).unwrap();
        schema["properties"][key]["type"] = serde_json::json!("integer");
        integer_descriptor.schema_json = Some(schema.to_string());
        let error = normalize_complete_instance_settings(
            Some(&integer_descriptor),
            fixture.settings.clone(),
            "acceptance-conanexiles",
            CONFIG_ACCEPTANCE_INSTANCE_NAME,
            app_core::INSTANCE_CREATION_DEFAULT_BIND_IP,
        )
        .expect_err("the old integer schema must reject each fractional setting");
        assert!(matches!(
            error,
            crate::StorageError::InvalidModuleSetting { field, .. } if field == key
        ));
    }
    run_storage_acceptance_repository(modules_root, Some("conanexiles"))
        .expect("all fixture fractions must survive native INI writes and disk reads");
}

struct ControlledRepository {
    root: PathBuf,
}

impl ControlledRepository {
    fn new() -> Self {
        Self {
            root: unique_system_temp_root("storage-controlled"),
        }
    }

    fn modules_root(&self) -> PathBuf {
        self.root.join("modules")
    }

    fn module_root(&self) -> PathBuf {
        self.root.join("modules").join("acceptancealpha")
    }

    fn write_module(&self) {
        let module_root = self.module_root();
        std::fs::create_dir_all(module_root.join("templates")).unwrap();
        std::fs::write(
            module_root.join("module.toml"),
            r#"id = "acceptancealpha"
name = "Acceptance Alpha"
version = "1.0.0"

[[default_ports]]
name = "game"
protocol = "udp"
port = 27115

[install]
shared_game_dir = "acceptancealpha"

[process]
executable = "server.exe"
args_template = []
"#,
        )
        .unwrap();
        std::fs::write(
            module_root.join("schema.json"),
            r#"{
  "type": "object",
  "properties": {
    "server_name": {"type":"string","default":"Default Server"},
    "required_setting": {"type":"string","x-lsgm-required-before-start":true}
  }
}"#,
        )
        .unwrap();
        std::fs::write(
            module_root.join("templates").join("server.json.hbs"),
            r#"{"name":{{json.settings.server_name}},"port":{{ports.game.port}},"bind_ip":"{{instance.bind_ip}}"}"#,
        )
        .unwrap();
        std::fs::write(
            module_root.join("templates").join("motd.txt.hbs"),
            "Welcome to {{settings.server_name}}",
        )
        .unwrap();
        std::fs::write(
            module_root.join("templates").join("server.properties.hbs"),
            "motd={{settings.server_name}}\nserver-port={{ports.game.port}}\n",
        )
        .unwrap();
        std::fs::write(
            module_root.join("templates").join("Game.ini.hbs"),
            "[Server]\nName={{settings.server_name}}\nOption=first\nOption=second\nPort={{ports.game.port}}\n",
        )
        .unwrap();
    }

    fn write_fixture(&self, contents: &str) {
        let fixture_root = self.module_root().join("config-fixtures");
        std::fs::create_dir_all(&fixture_root).unwrap();
        std::fs::write(fixture_root.join("controlled.json"), contents).unwrap();
    }
}

impl Drop for ControlledRepository {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
