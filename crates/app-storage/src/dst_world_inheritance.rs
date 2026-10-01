use super::super::templates_render_dst_inventory::dst_caves_inherited_override_entries;
use super::*;

// Read literal overrides through the same bounded parser used by guided editing.
// Arbitrary scripts and mod preset defaults remain owned by the operator.
pub(in super::super) fn inherited_master_settings(settings: &Settings) -> Settings {
    let mut inherited = Settings::new();
    let raw = text(settings, "master_worldgenoverride_lua");
    let normalize = |value: &str| {
        value
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .trim()
            .to_string()
    };
    let is_custom_raw = !raw.trim().is_empty()
        && normalize(raw) != normalize(super::super::DEFAULT_DST_MASTER_WORLDGENOVERRIDE_LUA);
    let parsed;
    let (master_preset, overrides) = if is_custom_raw {
        let Some(table) = lua::parse(raw, false) else {
            return inherited;
        };
        if table.get("override_enabled").and_then(lua::Node::scalar) != Some(&Value::Bool(true)) {
            return inherited;
        }
        parsed = table;
        let master_preset = preset(&parsed, "settings_preset")
            .and_then(Value::as_str)
            .unwrap_or("SURVIVAL_TOGETHER");
        (
            master_preset,
            parsed.get("overrides").and_then(lua::Node::table),
        )
    } else {
        let extra = text(settings, "master_world_overrides_extra");
        let Some(table) = lua::parse(extra, true) else {
            return inherited;
        };
        parsed = table;
        let master_preset = settings
            .get("master_settings_preset")
            .and_then(Value::as_str)
            .unwrap_or("SURVIVAL_TOGETHER");
        (master_preset, Some(&parsed))
    };
    let known_preset = matches!(
        master_preset,
        "SURVIVAL_TOGETHER" | "RELAXED" | "ENDLESS" | "WILDERNESS" | "LIGHTS_OUT"
    );
    for &(setting, native, default) in dst_caves_inherited_override_entries() {
        let explicit = overrides
            .and_then(|table| table.get(native))
            .and_then(lua::Node::scalar)
            .and_then(Value::as_str);
        let guided = (!is_custom_raw)
            .then(|| settings.get(setting).and_then(Value::as_str))
            .flatten()
            .map(str::trim)
            .filter(|value| !value.is_empty() && *value != default);
        let value = explicit.or(guided).or_else(|| {
            known_preset.then(|| master_preset_default(master_preset, native).unwrap_or(default))
        });
        if let Some(value) = value {
            inherited.insert(setting.to_string(), Value::String(value.to_string()));
        }
    }
    inherited
}

// First-party map/levels/forest.lua settings presets. Generation presets cannot
// change these world settings. Keep unknown mod presets free of guessed defaults.
fn master_preset_default(preset: &str, native: &str) -> Option<&'static str> {
    match (preset, native) {
        ("RELAXED", "ghostsanitydrain" | "healthpenalty" | "resettime") => Some("none"),
        ("RELAXED", "portalresurection" | "lessdamagetaken") => Some("always"),
        ("RELAXED", "temperaturedamage" | "hunger" | "darkness") => Some("nonlethal"),
        ("RELAXED", "shadowcreatures" | "brightmarecreatures") => Some("rare"),
        ("ENDLESS", "portalresurection" | "basicresource_regrowth") => Some("always"),
        ("ENDLESS", "resettime" | "ghostsanitydrain") => Some("none"),
        ("WILDERNESS", "spawnmode") => Some("scatter"),
        ("WILDERNESS", "basicresource_regrowth") => Some("always"),
        ("WILDERNESS", "ghostenabled" | "ghostsanitydrain" | "resettime") => Some("none"),
        ("LIGHTS_OUT", "day") => Some("onlynight"),
        _ => None,
    }
}
