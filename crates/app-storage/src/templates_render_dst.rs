use super::templates_render_dst_inventory::{
    DST_SHARED_OVERRIDE_ENTRIES, dst_caves_inherited_override_entries, dst_shard_override_entries,
};
use super::*;
use crate::player_access_normalization::normalize_dst_klei_id;

#[derive(Clone, Copy)]
pub(super) enum DstWorldOverrideTarget {
    InlineTable,
    FillMissing,
}

pub(super) fn derive_dst_caves_shard_id(instance_id: &str) -> u32 {
    let mut hash = 2_166_136_261_u32;
    for byte in instance_id.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    (hash & 0x7fff_ffff).max(2)
}

pub(super) fn render_dst_cluster_intention_line(settings: &Map<String, Value>) -> String {
    let Some(value) = lookup_setting_text(settings, "cluster_intention") else {
        return String::new();
    };
    let normalized = value.trim();
    if normalized.is_empty() || normalized.eq_ignore_ascii_case("default") {
        return String::new();
    }
    format!("cluster_intention = {normalized}")
}

pub(super) fn render_dst_klei_user_id_lines(settings: &Map<String, Value>, key: &str) -> String {
    let mut seen = HashSet::new();
    parse_config_lines(settings, key)
        .into_iter()
        .filter_map(|line| normalize_dst_klei_id(&line))
        .filter(|line| seen.insert(line.to_ascii_lowercase()))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn render_dst_worldgenoverride(settings: &Map<String, Value>, shard: &str) -> String {
    if matches!(shard, "islands" | "volcano") {
        let key = format!("{shard}_worldgenoverride_lua");
        if let Some(raw) = lookup_setting_text(settings, &key)
            && !raw.trim().is_empty()
        {
            return ensure_normalized_trailing_newline(&raw);
        }
        let (preset, overrides) = if shard == "islands" {
            ("SURVIVAL_SHIPWRECKED_CLASSIC", "volcanoisland = \"none\",")
        } else {
            ("SURVIVAL_VOLCANO_CLASSIC", "")
        };
        return format!(
            "return {{\n  override_enabled = true,\n  settings_preset = \"{preset}\",\n  worldgen_preset = \"{preset}\",\n  overrides = {{ {overrides} }},\n}}\n"
        );
    }
    let raw_override = if shard == "master" {
        lookup_setting_text(settings, "master_worldgenoverride_lua")
            .map(|raw| (raw, DEFAULT_DST_MASTER_WORLDGENOVERRIDE_LUA))
    } else {
        lookup_setting_text(settings, "caves_worldgenoverride_lua")
            .map(|raw| (raw, DEFAULT_DST_CAVES_WORLDGENOVERRIDE_LUA))
    };

    if let Some((raw, default_raw)) = raw_override
        && !raw.trim().is_empty()
    {
        let normalized = ensure_normalized_trailing_newline(&raw);
        if normalized.trim() != default_raw.trim() {
            return normalized;
        }
    }

    let default_preset = if shard == "master" {
        "SURVIVAL_TOGETHER"
    } else {
        "DST_CAVE"
    };
    let (settings_preset_key, worldgen_preset_key, extra_key) = if shard == "master" {
        (
            "master_settings_preset",
            "master_worldgen_preset",
            "master_world_overrides_extra",
        )
    } else {
        (
            "caves_settings_preset",
            "caves_worldgen_preset",
            "caves_world_overrides_extra",
        )
    };
    let settings_preset = lookup_setting_text(settings, settings_preset_key)
        .unwrap_or_else(|| String::from(default_preset));
    let worldgen_preset = lookup_setting_text(settings, worldgen_preset_key)
        .unwrap_or_else(|| String::from(default_preset));
    // Instance settings include materialized schema defaults. Re-emitting those values
    // over a different preset would replace its rules with the base world's defaults.
    let inherit_preset_defaults =
        settings_preset != default_preset || worldgen_preset != default_preset;
    let extra = lookup_setting_text(settings, extra_key).filter(|value| !value.trim().is_empty());
    let override_target = if extra.is_some() {
        DstWorldOverrideTarget::FillMissing
    } else {
        DstWorldOverrideTarget::InlineTable
    };

    let mut lines = vec![
        String::from(if extra.is_some() {
            "local configuration = {"
        } else {
            "return {"
        }),
        String::from("  override_enabled = true,"),
        format!(
            "  settings_preset = \"{}\",\n  worldgen_preset = \"{}\",",
            escape_dst_lua_string(&settings_preset),
            escape_dst_lua_string(&worldgen_preset)
        ),
        String::from("  overrides = {"),
    ];
    if let Some(extra) = extra {
        // Fill missing keys after the explicit overrides. This avoids duplicate-key
        // constructor ordering and works even in a Lua sandbox without global functions.
        append_dst_worldgen_extra_lines(&mut lines, Some(extra));
        lines.push(String::from("  }"));
        lines.push(String::from("}"));
    }

    if shard == "master" {
        for &(setting_key, output_key, default_value) in DST_SHARED_OVERRIDE_ENTRIES {
            push_dst_worldgen_entry(
                &mut lines,
                settings,
                setting_key,
                output_key,
                default_value,
                inherit_preset_defaults,
                override_target,
            );
        }
    } else {
        let inherited = super::dst_world_settings::inherited_master_settings(settings);
        for &(setting_key, output_key, default_value) in dst_caves_inherited_override_entries() {
            if inherited.contains_key(setting_key) {
                push_dst_worldgen_entry(
                    &mut lines,
                    &inherited,
                    setting_key,
                    output_key,
                    default_value,
                    false,
                    override_target,
                );
            }
        }
    }

    for &(setting_key, output_key, default_value) in dst_shard_override_entries(shard) {
        push_dst_worldgen_entry(
            &mut lines,
            settings,
            setting_key,
            output_key,
            default_value,
            inherit_preset_defaults,
            override_target,
        );
    }

    match override_target {
        DstWorldOverrideTarget::InlineTable => {
            lines.push(String::from("  }"));
            lines.push(String::from("}"));
        }
        DstWorldOverrideTarget::FillMissing => lines.push(String::from("return configuration")),
    }
    format!("{}\n", lines.join("\n"))
}

pub(super) fn push_dst_worldgen_entry(
    lines: &mut Vec<String>,
    settings: &Map<String, Value>,
    setting_key: &str,
    output_key: &str,
    default_value: &str,
    inherit_preset_defaults: bool,
    target: DstWorldOverrideTarget,
) {
    let value = lookup_setting_text(settings, setting_key)
        .map(|raw| raw.trim().to_string())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| String::from(default_value));
    if inherit_preset_defaults && value == default_value {
        return;
    }
    let value = escape_dst_lua_string(&value);
    match target {
        DstWorldOverrideTarget::InlineTable =>
            lines.push(format!("    {output_key} = \"{value}\",")),
        DstWorldOverrideTarget::FillMissing => lines.push(format!(
            "if configuration.overrides.{output_key} == nil then configuration.overrides.{output_key} = \"{value}\" end"
        )),
    }
}

pub(super) fn append_dst_worldgen_extra_lines(lines: &mut Vec<String>, raw: Option<String>) {
    let Some(raw) = raw else {
        return;
    };

    for line in raw.replace("\r\n", "\n").replace('\r', "\n").lines() {
        if line.trim().is_empty() {
            continue;
        }
        if line.chars().next().is_some_and(char::is_whitespace) {
            lines.push(String::from(line));
        } else {
            lines.push(format!("    {line}"));
        }
    }
}

pub(super) fn render_dst_modoverrides(settings: &Map<String, Value>, shard: &str) -> String {
    let raw_key = format!("{shard}_modoverrides_lua");

    if let Some(raw) = lookup_setting_text(settings, &raw_key)
        && !raw.trim().is_empty()
    {
        let normalized = ensure_normalized_trailing_newline(&raw);
        if normalized.trim() != DEFAULT_DST_MODOVERRIDES_LUA.trim() {
            return normalized;
        }
    }

    let enabled_key = format!("{shard}_enabled_workshop_mod_ids");
    let config_key = format!("{shard}_mod_configuration_options");
    let enabled_ids = parse_workshop_id_list(settings, &enabled_key);
    if enabled_ids.is_empty() {
        return String::from(DEFAULT_DST_MODOVERRIDES_LUA);
    }

    let mut rendered = String::from("return {\n");
    for mod_id in enabled_ids {
        if let Some(options_block) =
            render_dst_mod_configuration_block(settings, &config_key, &mod_id)
        {
            rendered.push_str(&format!(
                "  [\"workshop-{mod_id}\"] = {{\n    enabled = true,\n{options_block}  }},\n"
            ));
        } else {
            rendered.push_str(&format!(
                "  [\"workshop-{mod_id}\"] = {{ enabled = true }},\n"
            ));
        }
    }
    rendered.push_str("}\n");
    rendered
}

pub(super) fn render_dst_mod_configuration_block(
    settings: &Map<String, Value>,
    config_key: &str,
    mod_id: &str,
) -> Option<String> {
    let options = lookup_dst_mod_configuration_object(settings, config_key, mod_id)?;
    let mut entries = options
        .iter()
        .filter_map(|(key, value)| {
            render_dst_mod_option_value(value).map(|rendered| (key.as_str(), rendered))
        })
        .collect::<Vec<_>>();

    if entries.is_empty() {
        return None;
    }

    entries.sort_by(|left, right| left.0.cmp(right.0));

    let mut rendered = String::from("    configuration_options = {\n");
    for (key, value) in entries {
        rendered.push_str(&format!(
            "      [\"{}\"] = {},\n",
            escape_dst_lua_string(key),
            value
        ));
    }
    rendered.push_str("    },\n");
    Some(rendered)
}

pub(super) fn lookup_dst_mod_configuration_object<'a>(
    settings: &'a Map<String, Value>,
    config_key: &str,
    mod_id: &str,
) -> Option<&'a Map<String, Value>> {
    let configurations = settings.get(config_key)?.as_object()?;
    configurations
        .get(mod_id)
        .or_else(|| configurations.get(&format!("workshop-{mod_id}")))
        .and_then(Value::as_object)
}

pub(super) fn render_dst_mod_option_value(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::Bool(boolean) => Some(boolean.to_string()),
        Value::Number(number) => Some(number.to_string()),
        Value::String(text) => Some(format!("\"{}\"", escape_dst_lua_string(text))),
        _ => None,
    }
}

pub(super) fn escape_dst_lua_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}
