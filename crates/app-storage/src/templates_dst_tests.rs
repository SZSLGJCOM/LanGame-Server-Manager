use super::*;
use serde_json::json;

#[path = "templates_dst_inheritance_tests.rs"]
mod inheritance_tests;
#[path = "templates_dst_workshop_ids_tests.rs"]
mod workshop_ids_tests;

#[test]
fn island_adventures_renders_author_worlds_and_independent_mod_options() {
    let mut settings = Map::new();
    for (shard, preset) in [
        ("islands", "SURVIVAL_SHIPWRECKED_CLASSIC"),
        ("volcano", "SURVIVAL_VOLCANO_CLASSIC"),
    ] {
        let world = render_dst_worldgenoverride(&settings, shard);
        assert!(world.contains(&format!("settings_preset = \"{preset}\"")));
        assert!(world.contains(&format!("worldgen_preset = \"{preset}\"")));
        assert!(!world.contains("cave_default"));
        assert!(!world.contains("task_set = \"default\""));
        settings.insert(
            format!("{shard}_enabled_workshop_mod_ids"),
            json!("3435352667,1467214795"),
        );
        settings.insert(
            format!("{shard}_mod_configuration_options"),
            json!({"3435352667":{"flag":true,"amount":2,"label":shard}}),
        );
        let mods = render_dst_modoverrides(&settings, shard);
        assert!(mods.contains("workshop-1467214795"));
        assert!(mods.contains("[\"flag\"] = true"));
        assert!(mods.contains("[\"amount\"] = 2"));
        assert!(mods.contains(&format!("[\"label\"] = \"{shard}\"")));
    }
    assert!(render_dst_worldgenoverride(&settings, "islands").contains("volcanoisland = \"none\""));
    settings.insert(
        "volcano_worldgenoverride_lua".into(),
        json!("return { overrides = { world_size = 'tiny' } }"),
    );
    assert_eq!(
        render_dst_worldgenoverride(&settings, "volcano"),
        "return { overrides = { world_size = 'tiny' } }\n"
    );
}

#[test]
fn saved_mod_subtable_preserves_indexed_nested_data_without_executing_lua() {
    let raw = "return { enabled_mods = { ['workshop-123']={enabled=true,configuration_options={choices={[1]='a',[2]='b'}}} }, other = false }";
    let extracted = dst_world_settings::extract_dst_static_lua_table(raw, "enabled_mods")
        .unwrap()
        .unwrap();
    assert!(extracted.contains("[1]='a',[2]='b'"));
    assert!(!extracted.contains("other"));
    assert!(
        dst_world_settings::extract_dst_static_lua_table(
            "return os.execute('bad')",
            "enabled_mods"
        )
        .is_err()
    );
    assert!(
        dst_world_settings::extract_dst_static_lua_table(
            "return {enabled_mods=false}",
            "enabled_mods"
        )
        .is_err()
    );
}

#[test]
fn dst_caves_defaults_use_cave_location_tasks_and_spawn_layout() {
    let defaults = collect_schema_defaults_from_schema_json(
        Some(include_str!("../../../modules/dontstarve/schema.json")),
        SchemaDefaultContext {
            instance_id: Some("fixture-dst"),
            instance_name: Some("Fixture DST"),
        },
    )
    .unwrap();
    for settings in [&Map::new(), &defaults] {
        let rendered = render_dst_worldgenoverride(settings, "caves");
        assert!(rendered.contains("task_set = \"cave_default\""));
        assert!(rendered.contains("start_location = \"caves\""));
        assert!(!rendered.contains("task_set = \"default\""));
        assert!(!rendered.contains("start_location = \"default\""));
    }
}

fn unique_dst_test_root() -> PathBuf {
    std::env::temp_dir().join(format!(
        "lsgm-dst-templates-test-{}",
        Uuid::new_v4().as_simple()
    ))
}

fn dst_test_storage_paths(root: &Path) -> StoragePaths {
    StoragePaths {
        app_data_root: root.join("appdata"),
        settings_path: root.join("appdata/settings.json"),
        database_path: root.join("appdata/db/lgs.db"),
        logs_root: root.join("appdata/logs"),
        modules_root: root.join("modules"),
        migrations_root: root.join("migrations"),
        steamcmd_root: root.join("programdata/steamcmd"),
        games_root: root.join("games"),
        instances_root: root.join("instances"),
        archives_root: root.join("instances").join(".trash"),
    }
}

fn dst_mod_settings(mod_id: &str) -> Map<String, Value> {
    Map::from_iter([
        (
            String::from("shared_workshop_mod_ids"),
            Value::String(String::from(mod_id)),
        ),
        (
            String::from("shared_workshop_collection_ids"),
            Value::String(String::new()),
        ),
    ])
}

fn write_dst_instance_config(
    paths: &StoragePaths,
    instance_id: &str,
    settings: &Map<String, Value>,
) -> ManagedConfigMutation {
    let instance_root = paths.instances_root.join(instance_id);
    let config_dir = instance_root.join("config");
    let install_root = instance_root.join("runtime");
    fs::create_dir_all(&config_dir).expect("instance config directory");
    fs::create_dir_all(&install_root).expect("instance runtime directory");
    fs::write(
        install_root.join(crate::private_runtime::PRIVATE_RUNTIME_MARKER),
        b"managed\n",
    )
    .expect("instance runtime marker");
    let ports = Vec::new();
    materialize_support_files_and_write_instance_config(
        &ModuleSupportMaterializationContext {
            storage_paths: paths,
            module_id: "dontstarve",
            install_root: &install_root,
            shared_install_root: &paths.games_root.join("dontstarve"),
            config_dir: &config_dir,
            saves_dir: &config_dir.join("clusters/main"),
            instance_id,
            instance_running: false,
            settings,
        },
        &config_dir.join("instance.json"),
        InstanceConfigInput {
            instance_id,
            instance_name: instance_id,
            module_id: "dontstarve",
            bind_ip: "0.0.0.0",
            autostart: false,
            settings: settings.clone(),
            ports: &ports,
        },
    )
    .expect("materialize private DST setup")
}

#[test]
fn dst_native_inventory_separates_cave_options_from_inherited_master_options() {
    use super::templates_render_dst_inventory::{
        DST_CAVES_OVERRIDE_ENTRIES, DST_MASTER_OVERRIDE_ENTRIES, DST_SHARED_OVERRIDE_ENTRIES,
    };

    assert_eq!(
        DST_SHARED_OVERRIDE_ENTRIES.len() + DST_MASTER_OVERRIDE_ENTRIES.len(),
        190
    );
    assert_eq!(DST_CAVES_OVERRIDE_ENTRIES.len(), 86);

    let master = render_dst_worldgenoverride(&Map::new(), "master");
    let caves = render_dst_worldgenoverride(&Map::new(), "caves");
    let native_entry_count = |rendered: &str| {
        rendered
            .lines()
            .filter(|line| line.starts_with("    ") && line.ends_with("\","))
            .count()
    };
    assert_eq!(native_entry_count(&master), 190);
    assert_eq!(native_entry_count(&caves), 120);
    assert!(master.contains("settings_preset = \"SURVIVAL_TOGETHER\""));
    assert!(master.contains("worldgen_preset = \"SURVIVAL_TOGETHER\""));
    assert!(caves.contains("settings_preset = \"DST_CAVE\""));
    assert!(caves.contains("worldgen_preset = \"DST_CAVE\""));
    assert!(!master.contains("toadstool ="));
    for master_only in [
        "season_start =",
        "autumn =",
        "winter =",
        "spring =",
        "summer =",
        "deerclops =",
    ] {
        assert!(!caves.contains(master_only), "unexpected {master_only}");
    }
}

#[test]
fn dst_caves_shard_id_depends_only_on_immutable_instance_id() {
    assert_eq!(
        derive_dst_caves_shard_id("instance-immutable"),
        derive_dst_caves_shard_id("instance-immutable")
    );
    assert_ne!(
        derive_dst_caves_shard_id("instance-immutable"),
        derive_dst_caves_shard_id("another-instance")
    );
    assert!(derive_dst_caves_shard_id("") >= 2);
}

fn dst_world_schema_defaults() -> Map<String, Value> {
    collect_schema_defaults_from_schema_json(
        Some(include_str!("../../../modules/dontstarve/schema.json")),
        SchemaDefaultContext::default(),
    )
    .expect("DST schema defaults")
}

#[test]
fn dst_preset_defaults_preserve_materialized_and_existing_settings() {
    let mut settings = dst_world_schema_defaults();
    for shard in ["master", "caves"] {
        let expected = render_dst_worldgenoverride(&Map::new(), shard);
        assert_eq!(render_dst_worldgenoverride(&settings, shard), expected);
        settings.remove(&format!("{shard}_settings_preset"));
        settings.remove(&format!("{shard}_worldgen_preset"));
        assert_eq!(render_dst_worldgenoverride(&settings, shard), expected);
    }
}

#[test]
fn dst_non_default_presets_inherit_base_values_and_preserve_changed_options() {
    for (shard, rule_preset, generation_preset, changed_key, changed_output, changed_value) in [
        (
            "master",
            "ENDLESS",
            "LIGHTS_OUT",
            "master_day",
            "day",
            "onlyday",
        ),
        (
            "caves",
            "custom.caves_rules-1",
            "custom_caves_generation",
            "caves_acidrain_enabled",
            "acidrain_enabled",
            "none",
        ),
    ] {
        let mut settings = dst_world_schema_defaults();
        settings.insert(format!("{shard}_settings_preset"), json!(rule_preset));
        settings.insert(format!("{shard}_worldgen_preset"), json!(generation_preset));
        settings.insert(String::from(changed_key), json!(changed_value));
        let original = settings.clone();
        let rendered = render_dst_worldgenoverride(&settings, shard);
        assert!(rendered.contains(&format!("settings_preset = \"{rule_preset}\"")));
        assert!(rendered.contains(&format!("worldgen_preset = \"{generation_preset}\"")));
        assert!(rendered.contains(&format!("{changed_output} = \"{changed_value}\"")));
        assert!(!rendered.contains("world_size ="));
        assert_eq!(rendered.contains("healthpenalty ="), shard == "caves");
        let other_shard = if shard == "master" { "caves" } else { "master" };
        let other = render_dst_worldgenoverride(&settings, other_shard);
        if shard == "master" {
            assert!(other.contains("day = \"onlyday\""));
            assert!(other.contains("portalresurection = \"always\""));
        } else {
            assert_eq!(other, render_dst_worldgenoverride(&Map::new(), other_shard));
        }
        assert_eq!(
            settings, original,
            "rendering must preserve stored settings"
        );
    }
}

#[test]
fn dst_extra_world_overrides_force_base_values_without_global_lua_functions() {
    let mut settings = dst_world_schema_defaults();
    settings.insert(String::from("master_settings_preset"), json!("ENDLESS"));
    settings.insert(String::from("master_day"), json!("onlyday"));
    settings.insert(
        String::from("master_world_overrides_extra"),
        json!("healthpenalty = \"always\",\nday = \"default\",\ncustom_rule = false,"),
    );
    let rendered = render_dst_worldgenoverride(&settings, "master");
    assert!(rendered.starts_with("local configuration = {\n"));
    assert!(rendered.contains("healthpenalty = \"always\""));
    assert!(rendered.contains("day = \"default\""));
    assert!(rendered.contains("custom_rule = false"));
    assert!(rendered.contains(
        "if configuration.overrides.day == nil then configuration.overrides.day = \"onlyday\" end"
    ));
    assert!(!rendered.contains("configuration.overrides.healthpenalty ="));
    assert!(!rendered.contains("pairs("));
    assert!(!rendered.contains("setmetatable("));
    assert!(rendered.ends_with("return configuration\n"));
    let caves = render_dst_worldgenoverride(&settings, "caves");
    assert!(caves.contains("day = \"default\""));
    assert!(caves.contains("healthpenalty = \"always\""));
    assert!(!caves.contains("custom_rule ="));
}

#[test]
fn dst_preset_identifiers_reject_unsafe_or_unbounded_input_at_storage_boundary() {
    use crate::settings_validation::{
        SettingsValidationPhase, collect_settings_schema_diagnostics,
    };
    let schema: Value =
        serde_json::from_str(include_str!("../../../modules/dontstarve/schema.json"))
            .expect("DST schema");
    for shard in ["master", "caves"] {
        for kind in ["settings", "worldgen"] {
            let key = format!("{shard}_{kind}_preset");
            for value in [
                String::new(),
                String::from("bad\"; error('injected')"),
                String::from("ENDLESS\n"),
                "a".repeat(129),
            ] {
                let mut settings = dst_world_schema_defaults();
                settings.insert(key.clone(), json!(value));
                let diagnostics = collect_settings_schema_diagnostics(
                    &schema,
                    &settings,
                    SettingsValidationPhase::Creation,
                );
                assert!(
                    diagnostics.iter().any(|diagnostic| diagnostic.field == key),
                    "accepted unsafe {key}"
                );
            }
        }
    }
}

#[test]
fn dst_raw_world_file_remains_an_explicit_full_file_override() {
    let mut settings = dst_world_schema_defaults();
    settings.insert(String::from("master_settings_preset"), json!("ENDLESS"));
    let raw = "return { override_enabled = true, settings_preset = 'RELAXED', overrides = {} }\n";
    settings.insert(String::from("master_worldgenoverride_lua"), json!(raw));
    assert_eq!(render_dst_worldgenoverride(&settings, "master"), raw);
}

#[test]
fn dst_private_runtime_never_changes_peer_or_shared_install_setup() {
    let root = unique_dst_test_root();
    let paths = dst_test_storage_paths(&root);
    let shared_setup = paths
        .games_root
        .join("dontstarve/mods/dedicated_server_mods_setup.lua");
    fs::create_dir_all(shared_setup.parent().unwrap()).unwrap();
    let shared_before = b"-- package source retained unchanged\nServerModSetup(\"9999999999\")\n";
    fs::write(&shared_setup, shared_before).unwrap();

    write_dst_instance_config(&paths, "alpha", &dst_mod_settings("1111111111")).commit();
    let alpha_root = paths.instances_root.join("alpha");
    let alpha_setup = alpha_root.join("runtime/mods/dedicated_server_mods_setup.lua");
    let alpha_config = alpha_root.join("config/instance.json");
    let setup_before = fs::read(&alpha_setup).unwrap();
    let config_before = fs::read(&alpha_config).unwrap();
    let pending = write_dst_instance_config(&paths, "alpha", &dst_mod_settings("3333333333"));
    assert!(
        fs::read_to_string(&alpha_setup)
            .unwrap()
            .contains("3333333333")
    );

    // A pending change in Alpha must not serialize or aggregate Beta's setup.
    write_dst_instance_config(&paths, "beta", &dst_mod_settings("2222222222")).commit();
    let beta_setup = paths
        .instances_root
        .join("beta/runtime/mods/dedicated_server_mods_setup.lua");
    let beta_before = fs::read(&beta_setup).unwrap();
    let beta_text = std::str::from_utf8(&beta_before).unwrap();
    assert!(beta_text.contains("ServerModSetup(\"2222222222\")"));
    assert!(!beta_text.contains("1111111111"));
    assert!(!beta_text.contains("3333333333"));
    assert!(!beta_text.contains("9999999999"));

    pending.rollback().unwrap();
    assert_eq!(fs::read(&alpha_setup).unwrap(), setup_before);
    assert_eq!(fs::read(&alpha_config).unwrap(), config_before);
    assert_eq!(fs::read(&beta_setup).unwrap(), beta_before);
    assert_eq!(fs::read(&shared_setup).unwrap(), shared_before);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn dst_private_setup_rollback_preserves_external_file_change() {
    let root = unique_dst_test_root();
    let paths = dst_test_storage_paths(&root);
    write_dst_instance_config(&paths, "alpha", &dst_mod_settings("1111111111")).commit();
    let instance_root = paths.instances_root.join("alpha");
    let setup_path = instance_root.join("runtime/mods/dedicated_server_mods_setup.lua");
    let config_path = instance_root.join("config/instance.json");
    let config_before = fs::read(&config_path).unwrap();
    let pending = write_dst_instance_config(&paths, "alpha", &dst_mod_settings("2222222222"));
    let external = b"-- Operator change after staging.\nServerModSetup(\"4999999999\")\n";
    fs::write(&setup_path, external).unwrap();
    let error = pending
        .rollback()
        .expect_err("rollback must reject a concurrent writer");
    assert!(
        error
            .to_string()
            .contains("destination changed concurrently")
    );
    assert_eq!(fs::read(&setup_path).unwrap(), external);
    assert_eq!(fs::read(&config_path).unwrap(), config_before);
    fs::remove_dir_all(root).unwrap();
}

fn materialize_support_files_and_write_instance_config(
    context: &ModuleSupportMaterializationContext<'_>,
    config_path: &Path,
    input: InstanceConfigInput<'_>,
) -> Result<ManagedConfigMutation, StorageError> {
    let render_input = ModuleTemplateRenderInput {
        config_dir: context.config_dir,
        install_root: context.install_root,
        saves_dir: context.saves_dir,
        instance_id: input.instance_id,
        instance_name: input.instance_name,
        module_id: input.module_id,
        bind_ip: input.bind_ip,
        autostart: input.autostart,
        settings: context.settings,
        ports: input.ports,
    };
    write_pending_instance_configuration(
        &context.config_dir.join("fixture-templates"),
        &render_input,
        context,
        config_path,
        input,
        None,
    )
}
