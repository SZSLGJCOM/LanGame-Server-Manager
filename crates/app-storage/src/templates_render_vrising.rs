use super::*;

pub(super) fn render_vrising_optional_host_settings_members(
    settings: &Map<String, Value>,
) -> String {
    // Official examples are not defaults. Omit unset overrides so the server
    // retains its native defaults, while preserving explicit false and zero.
    [
        ("lower_fps_when_empty", "LowerFPSWhenEmpty"),
        ("lower_fps_when_empty_value", "LowerFPSWhenEmptyValue"),
        ("lan_mode", "LanMode"),
        ("safe_reconnect_time", "SafeReconnectTime"),
        ("safe_reconnect_slots", "SafeReconnectSlots"),
    ]
    .into_iter()
    .filter_map(|(setting_key, native_key)| {
        vrising_setting_value(settings, setting_key)
            .map(|value| format!(",\n  \"{native_key}\": {value}"))
    })
    .collect()
}

#[cfg(test)]
mod optional_host_settings_tests {
    use super::*;

    #[test]
    fn omitted_values_keep_native_defaults_and_explicit_false_zero_are_written() {
        assert!(render_vrising_optional_host_settings_members(&Map::new()).is_empty());
        let settings = serde_json::json!({
            "lower_fps_when_empty": false,
            "safe_reconnect_time": 0,
            "lan_mode": null,
            "unrelated_setting": true
        });
        let members = render_vrising_optional_host_settings_members(settings.as_object().unwrap());
        let document: Value = serde_json::from_str(&format!("{{\"Name\":\"Test\"{members}}}"))
            .expect("optional members produce valid host JSON");
        assert_eq!(document["LowerFPSWhenEmpty"], false);
        assert_eq!(document["SafeReconnectTime"], 0);
        assert!(document.get("LanMode").is_none());
        assert!(document.get("LowerFPSWhenEmptyValue").is_none());
        assert!(document.get("SafeReconnectSlots").is_none());
        assert!(document.get("unrelated_setting").is_none());
    }
}

pub(super) fn render_vrising_steam64_lines(settings: &Map<String, Value>, key: &str) -> String {
    let Some(raw) = lookup_setting_text(settings, key) else {
        return String::new();
    };

    parse_steam64_values_from_text(&raw).join("\n")
}

pub(super) fn render_vrising_server_game_settings_json(
    settings: &Map<String, Value>,
    schema_defaults: &Map<String, Value>,
) -> String {
    let mut document = parse_vrising_advanced_game_settings(settings, schema_defaults);

    merge_vrising_generated_game_settings(
        &mut document,
        settings,
        Some(schema_defaults),
        VrisingGameSettingsMergeMode::Override,
    );
    merge_vrising_generated_game_settings(
        &mut document,
        schema_defaults,
        None,
        VrisingGameSettingsMergeMode::FillMissing,
    );

    serde_json::to_string_pretty(&Value::Object(document)).unwrap_or_else(|_| String::from("{\n}"))
}

#[derive(Debug, Clone, Copy)]
pub(super) enum VrisingGameSettingsMergeMode {
    Override,
    FillMissing,
}

pub(super) fn merge_vrising_generated_game_settings(
    document: &mut Map<String, Value>,
    values: &Map<String, Value>,
    defaults_for_user_filter: Option<&Map<String, Value>>,
    mode: VrisingGameSettingsMergeMode,
) {
    for (settings_key, output_key) in VRISING_TOP_LEVEL_GAME_SETTINGS {
        if let Some(value) =
            vrising_setting_value_for_merge(values, defaults_for_user_filter, settings_key)
        {
            merge_vrising_value_field(document, output_key, value, mode);
        }
    }

    let game_time =
        build_vrising_named_object(values, defaults_for_user_filter, VRISING_GAME_TIME_SETTINGS);
    merge_vrising_object_field(document, "GameTimeModifiers", game_time, mode);

    for (output_key, entries) in [
        ("VampireStatModifiers", VRISING_VAMPIRE_STAT_SETTINGS),
        (
            "UnitStatModifiers_Global",
            VRISING_GLOBAL_UNIT_STAT_SETTINGS,
        ),
        (
            "UnitStatModifiers_VBlood",
            VRISING_VBLOOD_UNIT_STAT_SETTINGS,
        ),
        (
            "EquipmentStatModifiers_Global",
            VRISING_EQUIPMENT_STAT_SETTINGS,
        ),
    ] {
        let object = build_vrising_named_object(values, defaults_for_user_filter, entries);
        merge_vrising_object_field(document, output_key, object, mode);
    }

    let mut castle_stats = build_vrising_named_object(
        values,
        defaults_for_user_filter,
        VRISING_CASTLE_STAT_SETTINGS,
    );
    let heart_limits = build_vrising_castle_heart_limits(values, defaults_for_user_filter, mode);
    merge_vrising_object_field(&mut castle_stats, "HeartLimits", heart_limits, mode);
    merge_vrising_object_field(document, "CastleStatModifiers_Global", castle_stats, mode);

    let trader_modifiers = build_vrising_named_object(
        values,
        defaults_for_user_filter,
        VRISING_TRADER_MODIFIER_SETTINGS,
    );
    merge_vrising_object_field(document, "TraderModifiers", trader_modifiers, mode);

    let mut player_interaction = Map::new();
    if let Some(value) = vrising_setting_value_for_merge(
        values,
        defaults_for_user_filter,
        "player_interaction_time_zone",
    ) {
        player_interaction.insert(String::from("TimeZone"), value);
    }
    insert_vrising_time_window(
        &mut player_interaction,
        values,
        defaults_for_user_filter,
        "VSPlayerWeekdayTime",
        "vs_player_weekday",
        mode,
    );
    insert_vrising_time_window(
        &mut player_interaction,
        values,
        defaults_for_user_filter,
        "VSPlayerWeekendTime",
        "vs_player_weekend",
        mode,
    );
    insert_vrising_time_window(
        &mut player_interaction,
        values,
        defaults_for_user_filter,
        "VSCastleWeekdayTime",
        "vs_castle_weekday",
        mode,
    );
    insert_vrising_time_window(
        &mut player_interaction,
        values,
        defaults_for_user_filter,
        "VSCastleWeekendTime",
        "vs_castle_weekend",
        mode,
    );
    merge_vrising_object_field(
        document,
        "PlayerInteractionSettings",
        player_interaction,
        mode,
    );

    let mut war_event =
        build_vrising_named_object(values, defaults_for_user_filter, VRISING_WAR_EVENT_SETTINGS);
    insert_vrising_time_window(
        &mut war_event,
        values,
        defaults_for_user_filter,
        "WeekdayTime",
        "war_event_weekday",
        mode,
    );
    insert_vrising_time_window(
        &mut war_event,
        values,
        defaults_for_user_filter,
        "WeekendTime",
        "war_event_weekend",
        mode,
    );
    for player_count in 1..=4 {
        insert_vrising_war_event_scaling(
            &mut war_event,
            values,
            defaults_for_user_filter,
            player_count,
            mode,
        );
    }
    merge_vrising_object_field(document, "WarEventGameSettings", war_event, mode);
}

pub(super) fn parse_vrising_advanced_game_settings(
    settings: &Map<String, Value>,
    schema_defaults: &Map<String, Value>,
) -> Map<String, Value> {
    let raw = vrising_setting_text(settings, schema_defaults, "server_game_settings_json")
        .unwrap_or_default();
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Map::new();
    }

    serde_json::from_str::<Value>(trimmed)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

pub(super) fn build_vrising_named_object(
    values: &Map<String, Value>,
    defaults_for_user_filter: Option<&Map<String, Value>>,
    entries: &[(&str, &str)],
) -> Map<String, Value> {
    let mut object = Map::new();
    for (settings_key, output_key) in entries {
        if let Some(value) =
            vrising_setting_value_for_merge(values, defaults_for_user_filter, settings_key)
        {
            object.insert(String::from(*output_key), value);
        }
    }
    object
}

pub(super) fn build_vrising_castle_heart_limits(
    values: &Map<String, Value>,
    defaults_for_user_filter: Option<&Map<String, Value>>,
    mode: VrisingGameSettingsMergeMode,
) -> Map<String, Value> {
    let mut heart_limits = Map::new();
    for (settings_key, level_key, output_key) in VRISING_CASTLE_HEART_LIMIT_SETTINGS {
        let Some(value) =
            vrising_setting_value_for_merge(values, defaults_for_user_filter, settings_key)
        else {
            continue;
        };
        let level = heart_limits
            .entry(String::from(*level_key))
            .or_insert_with(|| Value::Object(Map::new()));
        let Some(level) = level.as_object_mut() else {
            continue;
        };
        merge_vrising_value_field(level, output_key, value, mode);
    }
    heart_limits
}

pub(super) fn insert_vrising_time_window(
    parent: &mut Map<String, Value>,
    values: &Map<String, Value>,
    defaults_for_user_filter: Option<&Map<String, Value>>,
    output_key: &str,
    settings_prefix: &str,
    mode: VrisingGameSettingsMergeMode,
) {
    let entries = [
        (format!("{settings_prefix}_start_hour"), "StartHour"),
        (format!("{settings_prefix}_start_minute"), "StartMinute"),
        (format!("{settings_prefix}_end_hour"), "EndHour"),
        (format!("{settings_prefix}_end_minute"), "EndMinute"),
    ];
    let mut window = Map::new();

    for (settings_key, output_field) in entries {
        if let Some(value) =
            vrising_setting_value_for_merge(values, defaults_for_user_filter, &settings_key)
        {
            window.insert(String::from(output_field), value);
        }
    }

    if !window.is_empty() {
        merge_vrising_object_field(parent, output_key, window, mode);
    }
}

pub(super) fn insert_vrising_war_event_scaling(
    parent: &mut Map<String, Value>,
    values: &Map<String, Value>,
    defaults_for_user_filter: Option<&Map<String, Value>>,
    player_count: u8,
    mode: VrisingGameSettingsMergeMode,
) {
    let mut scaling = Map::new();
    let prefix = format!("war_event_scaling_players_{player_count}");
    for (settings_key, output_field) in [
        (format!("{prefix}_points_modifier"), "PointsModifier"),
        (format!("{prefix}_drop_modifier"), "DropModifier"),
    ] {
        if let Some(value) =
            vrising_setting_value_for_merge(values, defaults_for_user_filter, &settings_key)
        {
            scaling.insert(String::from(output_field), value);
        }
    }

    if !scaling.is_empty() {
        merge_vrising_object_field(
            parent,
            &format!("ScalingPlayers{player_count}"),
            scaling,
            mode,
        );
    }
}

pub(super) fn merge_vrising_object_field(
    parent: &mut Map<String, Value>,
    output_key: &str,
    generated: Map<String, Value>,
    mode: VrisingGameSettingsMergeMode,
) {
    if generated.is_empty() {
        return;
    }

    match parent.remove(output_key) {
        Some(Value::Object(mut existing)) => {
            for (key, value) in generated {
                merge_vrising_value(&mut existing, key, value, mode);
            }
            parent.insert(String::from(output_key), Value::Object(existing));
        }
        Some(existing) if matches!(mode, VrisingGameSettingsMergeMode::FillMissing) => {
            parent.insert(String::from(output_key), existing);
        }
        _ => {
            parent.insert(String::from(output_key), Value::Object(generated));
        }
    }
}

pub(super) fn merge_vrising_value_field(
    parent: &mut Map<String, Value>,
    key: &str,
    generated: Value,
    mode: VrisingGameSettingsMergeMode,
) {
    merge_vrising_value(parent, String::from(key), generated, mode);
}

pub(super) fn merge_vrising_value(
    parent: &mut Map<String, Value>,
    key: String,
    generated: Value,
    mode: VrisingGameSettingsMergeMode,
) {
    match (parent.remove(&key), generated) {
        (Some(Value::Object(mut existing)), Value::Object(generated_object)) => {
            for (nested_key, nested_value) in generated_object {
                merge_vrising_value(&mut existing, nested_key, nested_value, mode);
            }
            parent.insert(key, Value::Object(existing));
        }
        (Some(existing), _) if matches!(mode, VrisingGameSettingsMergeMode::FillMissing) => {
            parent.insert(key, existing);
        }
        (_, generated_value) => {
            parent.insert(key, generated_value);
        }
    }
}

pub(super) fn vrising_setting_value(values: &Map<String, Value>, key: &str) -> Option<Value> {
    values
        .get(key)
        .cloned()
        .filter(|value| !matches!(value, Value::Null))
}

pub(super) fn vrising_setting_value_for_merge(
    values: &Map<String, Value>,
    defaults_for_user_filter: Option<&Map<String, Value>>,
    key: &str,
) -> Option<Value> {
    let value = vrising_setting_value(values, key)?;
    if defaults_for_user_filter
        .and_then(|defaults| defaults.get(key))
        .is_some_and(|default| default == &value)
    {
        return None;
    }
    Some(value)
}

pub(super) fn vrising_setting_text(
    settings: &Map<String, Value>,
    schema_defaults: &Map<String, Value>,
    key: &str,
) -> Option<String> {
    settings
        .get(key)
        .or_else(|| schema_defaults.get(key))
        .cloned()
        .filter(|value| !matches!(value, Value::Null))
        .map(|value| stringify_template_value(&value))
}
