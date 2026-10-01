use super::*;

#[test]
fn dst_caves_inherit_all_events_and_master_options_before_runtime_sync() {
    let mut settings = dst_world_schema_defaults();
    for (key, value) in settings.iter_mut() {
        if key.starts_with("world_year_of_the_")
            || [
                "world_crow_carnival",
                "world_hallowed_nights",
                "world_winters_feast",
            ]
            .contains(&key.as_str())
        {
            *value = json!("enabled");
        }
    }
    for (key, value) in [
        ("world_specialevent", "none"),
        ("world_extrastartingitems", "0"),
        ("world_healthpenalty", "none"),
        ("master_day", "onlynight"),
        ("master_beefaloheat", "always"),
        ("master_krampus", "never"),
    ] {
        settings.insert(key.to_string(), json!(value));
    }
    let caves = render_dst_worldgenoverride(&settings, "caves");
    for (key, value) in &settings {
        if value == "enabled" && key.starts_with("world_") {
            assert!(
                caves.contains(&format!("{} = \"enabled\"", &key[6..])),
                "{key}"
            );
        }
    }
    for expected in [
        "specialevent = \"none\"",
        "extrastartingitems = \"0\"",
        "healthpenalty = \"none\"",
        "day = \"onlynight\"",
        "beefaloheat = \"always\"",
        "krampus = \"never\"",
    ] {
        assert!(caves.contains(expected), "missing {expected}");
    }
}

#[test]
fn dst_caves_inherit_master_preset_values_without_surface_generation_options() {
    for (preset, expected) in [
        ("RELAXED", "hunger = \"nonlethal\""),
        ("ENDLESS", "portalresurection = \"always\""),
        ("WILDERNESS", "spawnmode = \"scatter\""),
        ("LIGHTS_OUT", "day = \"onlynight\""),
    ] {
        let mut settings = dst_world_schema_defaults();
        settings.insert(String::from("master_settings_preset"), json!(preset));
        settings.insert(String::from("master_world_size"), json!("huge"));
        settings.insert(String::from("master_season_start"), json!("winter"));
        settings.insert(String::from("world_winter"), json!("noseason"));
        let caves = render_dst_worldgenoverride(&settings, "caves");
        assert!(caves.contains(expected), "{preset}: {expected}");
        assert!(caves.contains("world_size = \"default\""));
        assert!(!caves.contains("season_start ="));
        assert!(!caves.contains("winter ="));
    }
}

#[test]
fn dst_cave_preset_does_not_suppress_master_values() {
    let mut settings = dst_world_schema_defaults();
    settings.insert(String::from("caves_settings_preset"), json!("custom_caves"));
    let caves = render_dst_worldgenoverride(&settings, "caves");
    assert!(caves.contains("healthpenalty = \"always\""));
    assert!(caves.contains("specialevent = \"default\""));
    assert!(!caves.contains("world_size ="));
}

#[test]
fn dst_caves_inherit_explicit_master_lua_values_over_preset_and_guided_defaults() {
    let mut settings = dst_world_schema_defaults();
    settings.insert(String::from("master_settings_preset"), json!("LIGHTS_OUT"));
    settings.insert(String::from("master_day"), json!("onlyday"));
    settings.insert(
        String::from("master_world_overrides_extra"),
        json!("day = 'default', year_of_the_snake = 'enabled',"),
    );
    let caves = render_dst_worldgenoverride(&settings, "caves");
    assert!(caves.contains("day = \"default\""));
    assert!(caves.contains("year_of_the_snake = \"enabled\""));

    let raw =
        "return {override_enabled=true,settings_preset='RELAXED',overrides={day='onlydusk'}}\n";
    settings.insert(String::from("master_worldgenoverride_lua"), json!(raw));
    let caves = render_dst_worldgenoverride(&settings, "caves");
    assert!(caves.contains("day = \"onlydusk\""));
    assert!(caves.contains("hunger = \"nonlethal\""));
    assert!(caves.contains("year_of_the_snake = \"default\""));
    assert_eq!(render_dst_worldgenoverride(&settings, "master"), raw);
}

#[test]
fn dst_unknown_master_preset_and_opaque_lua_do_not_invent_inherited_defaults() {
    let mut settings = dst_world_schema_defaults();
    settings.insert(
        String::from("master_settings_preset"),
        json!("custom_master"),
    );
    settings.insert(String::from("world_hallowed_nights"), json!("enabled"));
    let caves = render_dst_worldgenoverride(&settings, "caves");
    assert!(caves.contains("hallowed_nights = \"enabled\""));
    assert!(!caves.contains("healthpenalty ="));
    settings.insert(
        String::from("master_worldgenoverride_lua"),
        json!("return build_world()"),
    );
    let caves = render_dst_worldgenoverride(&settings, "caves");
    assert!(!caves.contains("hallowed_nights ="));
    assert!(!caves.contains("healthpenalty ="));
}

#[test]
fn dst_cave_expert_overrides_retain_authority_over_inherited_master_values() {
    let mut settings = dst_world_schema_defaults();
    settings.insert(String::from("master_day"), json!("onlyday"));
    settings.insert(
        String::from("caves_world_overrides_extra"),
        json!("day = 'onlynight',"),
    );
    let caves = render_dst_worldgenoverride(&settings, "caves");
    assert!(caves.contains("day = 'onlynight'"));
    assert!(caves.contains(
        "if configuration.overrides.day == nil then configuration.overrides.day = \"onlyday\" end"
    ));
    let raw = "return {override_enabled=true,preset='DST_CAVE',overrides={day='onlydusk'}}\n";
    settings.insert(String::from("caves_worldgenoverride_lua"), json!(raw));
    assert_eq!(render_dst_worldgenoverride(&settings, "caves"), raw);
}
