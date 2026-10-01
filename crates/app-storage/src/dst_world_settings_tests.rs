use super::*;
use serde_json::json;

fn world_schema() -> Value {
    serde_json::from_str(include_str!("../../../modules/dontstarve/schema.json")).unwrap()
}

fn defaults(schema: &Value) -> Settings {
    schema["properties"]
        .as_object()
        .unwrap()
        .iter()
        .filter_map(|(key, property)| {
            property
                .get("default")
                .map(|value| (key.clone(), value.clone()))
        })
        .collect()
}

fn raw_world(settings: &mut Settings, shard: &str, source: &str) {
    settings.insert(format!("{shard}_worldgenoverride_lua"), json!(source));
}

#[test]
fn static_raw_projects_effective_values_and_keeps_disabled_caves_sources() {
    let schema = world_schema();
    let mut settings = defaults(&schema);
    let master = "return { override_enabled=true, preset='ENDLESS', overrides={world_size='huge'}, unknown={1,2} }";
    let caves =
        "return { override_enabled=true, preset='DST_CAVE', overrides={world_size='default'} }";
    raw_world(&mut settings, "master", master);
    raw_world(&mut settings, "caves", caves);
    settings.insert(
        String::from("master_ocean_bullkelp"),
        json!("ocean_default"),
    );
    settings.insert(
        String::from("master_world_overrides_extra"),
        json!("world_size='small', custom_rule=true,"),
    );
    project_schema(&schema, &mut settings);
    assert_eq!(settings["master_world_size"], "huge");
    assert_eq!(
        settings["master_ocean_bullkelp"],
        schema["properties"]["master_ocean_bullkelp"]["default"]
    );
    assert_eq!(settings["master_settings_preset"], "ENDLESS");
    assert_eq!(settings["master_worldgen_preset"], "ENDLESS");
    assert_eq!(settings["enable_caves"], false);
    assert_eq!(settings["master_worldgenoverride_lua"], master);
    assert_eq!(settings["caves_worldgenoverride_lua"], caves);
    assert!(text(&settings, "master_world_overrides_extra").contains("custom_rule=true"));
}

#[test]
fn repeated_guided_edits_preserve_source_and_can_return_to_schema_default() {
    let schema = world_schema();
    let mut current = defaults(&schema);
    let source = "-- untouched\nreturn {override_enabled=true,preset='SURVIVAL_TOGETHER',overrides={world_size='huge',custom={x='keep'} -- comment\n}}";
    raw_world(&mut current, "master", source);
    current.insert(
        String::from("master_world_overrides_extra"),
        json!("world_size='small', custom_extra=false,"),
    );
    project_schema(&schema, &mut current);
    let caves = current["caves_worldgenoverride_lua"].clone();
    for value in ["small", "medium", "default"] {
        let mut incoming = current.clone();
        incoming.insert(String::from("master_world_size"), json!(value));
        current = reconcile_schema(&schema, current, incoming).unwrap();
        assert_eq!(current["master_world_size"], value);
        let raw = text(&current, "master_worldgenoverride_lua");
        assert!(raw.starts_with("-- untouched\n"));
        assert!(raw.contains("custom={x='keep'} -- comment"));
        let parsed = lua::parse(raw, false).unwrap();
        assert_eq!(
            parsed
                .get("overrides")
                .unwrap()
                .table()
                .unwrap()
                .get("world_size")
                .unwrap()
                .scalar(),
            Some(&json!(value))
        );
        assert!(text(&current, "master_world_overrides_extra").contains("custom_extra=false"));
        assert_eq!(current["caves_worldgenoverride_lua"], caves);
        let mut read_back = current.clone();
        project_schema(&schema, &mut read_back);
        assert_eq!(
            read_back, current,
            "saved state must equal the next optimistic baseline"
        );
    }
}

#[test]
fn raw_edits_win_over_simultaneous_guided_edits_and_remain_byte_exact() {
    let schema = world_schema();
    let current = defaults(&schema);
    let raw = "return { override_enabled = true, overrides={day='onlynight'}, settings_preset='ENDLESS' }";
    let mut incoming = current.clone();
    raw_world(&mut incoming, "master", raw);
    incoming.insert(String::from("master_day"), json!("onlyday"));
    let saved = reconcile_schema(&schema, current, incoming).unwrap();
    assert_eq!(saved["master_day"], "onlynight");
    assert_eq!(saved["master_worldgenoverride_lua"], raw);
    let mut incoming = saved.clone();
    raw_world(&mut incoming, "master", "");
    let saved = reconcile_schema(&schema, saved, incoming).unwrap();
    assert_eq!(saved["master_worldgenoverride_lua"], "");
    assert_eq!(saved["master_day"], "onlynight");
}

#[test]
fn custom_presets_allow_explicit_base_defaults_without_adding_untouched_values() {
    let schema = world_schema();
    let mut current = defaults(&schema);
    current.insert(String::from("master_settings_preset"), json!("ENDLESS"));
    current.insert(String::from("world_healthpenalty"), json!("none"));
    let mut incoming = current.clone();
    incoming.insert(String::from("world_healthpenalty"), json!("always"));
    let saved = reconcile_schema(&schema, current, incoming).unwrap();
    assert!(text(&saved, "master_world_overrides_extra").contains("healthpenalty = \"always\""));
    assert!(!text(&saved, "master_world_overrides_extra").contains("day"));
    let rendered = super::super::render_dst_worldgenoverride(&saved, "master");
    assert!(rendered.contains("healthpenalty = \"always\""));
    assert!(rendered.contains("settings_preset = \"ENDLESS\""));
}

#[test]
fn complex_or_invalid_sources_preserve_other_settings_and_reject_only_guided_mutations() {
    let schema = world_schema();
    for source in [
        "local configuration = make_world(); return configuration",
        "return {override_enabled=false,overrides={day='onlyday'}}",
        "return {overrides={day='onlyday'}}",
        "return {override_enabled=true,overrides=false}",
        "return {override_enabled=true,settings_preset=3}",
        "return {override_enabled=true,overrides={day='unsupported'}}",
    ] {
        let mut current = defaults(&schema);
        raw_world(&mut current, "master", source);
        project_schema(&schema, &mut current);
        let mut incoming = current.clone();
        incoming.insert(String::from("cluster_description"), json!("still editable"));
        let saved = reconcile_schema(&schema, current.clone(), incoming).unwrap();
        assert_eq!(saved["master_worldgenoverride_lua"], source);
        assert_eq!(saved["cluster_description"], "still editable");
        let mut incoming = saved;
        incoming.insert(String::from("master_world_size"), json!("huge"));
        assert!(
            reconcile_schema(&schema, current, incoming).is_err(),
            "must not pretend to edit {source}"
        );
    }
}

#[test]
fn nil_overrides_can_be_filled_and_nil_presets_use_fallback() {
    let schema = world_schema();
    let mut current = defaults(&schema);
    raw_world(
        &mut current,
        "master",
        "return {override_enabled=true, settings_preset=nil, preset='ENDLESS', overrides=nil}",
    );
    project_schema(&schema, &mut current);
    assert_eq!(current["master_settings_preset"], "ENDLESS");
    let mut incoming = current.clone();
    incoming.insert(String::from("master_day"), json!("onlyday"));
    let saved = reconcile_schema(&schema, current, incoming).unwrap();
    assert_eq!(saved["master_day"], "onlyday");
    assert!(lua::parse(text(&saved, "master_worldgenoverride_lua"), false).is_some());
}

#[test]
fn default_raw_with_windows_newlines_stays_generated() {
    let schema = world_schema();
    let mut settings = defaults(&schema);
    let raw = text(&settings, "master_worldgenoverride_lua").replace('\n', "\r\n");
    raw_world(&mut settings, "master", &raw);
    settings.insert(String::from("master_world_size"), json!("huge"));
    project_schema(&schema, &mut settings);
    assert_eq!(settings["master_world_size"], "huge");
    assert_eq!(settings["master_worldgenoverride_lua"], raw);
}

#[test]
fn nil_extra_inherits_the_typed_field() {
    let schema = world_schema();
    let mut generated = defaults(&schema);
    generated.insert(String::from("master_day"), json!("onlynight"));
    generated.insert(
        String::from("master_world_overrides_extra"),
        json!("day=nil,"),
    );
    project_schema(&schema, &mut generated);
    assert_eq!(generated["master_day"], "onlynight");
}

#[test]
fn frontend_fixture_outputs_remain_exact_optimistic_save_baselines() {
    let schema = world_schema();
    let fixtures: Value = serde_json::from_str(include_str!(
        "../../../apps/desktop/tests/fixtures/dontstarve-world-lua.json"
    ))
    .unwrap();
    for case in fixtures["cases"].as_array().unwrap() {
        let mut current = defaults(&schema);
        current.extend(case["settings"].as_object().unwrap().clone());
        let mut expected = current.clone();
        for shard in case["projectedShards"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
        {
            for (key, _, fallback) in entries(shard) {
                expected.insert(key.to_string(), default_value(&schema, key, fallback));
            }
            for kind in ["settings_preset", "worldgen_preset"] {
                let key = format!("{shard}_{kind}");
                expected.insert(key.clone(), schema["properties"][&key]["default"].clone());
            }
        }
        expected.extend(case["expected"].as_object().unwrap().clone());
        project_schema(&schema, &mut current);
        assert_eq!(current, expected, "initial projection: {}", case["name"]);
        for step in case["steps"].as_array().unwrap() {
            let mut pending = current.clone();
            pending.extend(step["patch"].as_object().unwrap().clone());
            pending.extend(step["expected"].as_object().unwrap().clone());
            if let Some(raw) = step.get("raw") {
                pending.insert(String::from("master_worldgenoverride_lua"), raw.clone());
            }
            if let Some(extra) = step.get("extra") {
                pending.insert(String::from("master_world_overrides_extra"), extra.clone());
            }
            let saved = reconcile_schema(&schema, current, pending.clone()).unwrap();
            assert_eq!(
                saved, pending,
                "frontend payload must remain the next baseline: {}",
                case["name"]
            );
            current = saved;
            project_schema(&schema, &mut current);
            assert_eq!(
                current, pending,
                "read projection must preserve the frontend baseline: {}",
                case["name"]
            );
        }
    }
}
