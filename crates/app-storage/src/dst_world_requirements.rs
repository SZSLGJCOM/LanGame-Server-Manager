use super::*;

/// Canonical guided values can contain schema fallbacks for opaque Lua. Those
/// fallbacks cannot prove a requested world setting was configured. This check
/// only establishes static configuration evidence; it never executes Lua or
/// claims the running world applied an option.
pub fn dst_world_setting_evidence_known(
    settings: &Value,
    schema_json: &str,
    key: &str,
) -> Result<bool, StorageError> {
    let Some(shard) = ["master", "caves"].into_iter().find(|shard| {
        entries(shard).any(|(entry, _, _)| *entry == key)
            || key == format!("{shard}_settings_preset")
            || key == format!("{shard}_worldgen_preset")
    }) else {
        return Ok(true);
    };
    let schema: Value = serde_json::from_str(schema_json)?;
    let Some(settings) = settings.as_object() else {
        return Ok(false);
    };
    if let Some(raw) = custom_raw(&schema, settings, shard) {
        return Ok(
            lua::parse(raw, false).is_some_and(|table| raw_is_editable(&schema, shard, &table))
        );
    }
    let extra = text(settings, &format!("{shard}_world_overrides_extra"));
    Ok(extra.trim().is_empty()
        || lua::parse(extra, true)
            .is_some_and(|table| overrides_are_editable(&schema, shard, &table)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SCHEMA: &str = include_str!("../../../modules/dontstarve/schema.json");

    #[test]
    fn default_template_does_not_hide_explicit_guided_world_settings() {
        let schema: Value = serde_json::from_str(SCHEMA).unwrap();
        let settings = json!({
            "master_world_size":"small",
            "master_worldgenoverride_lua": schema["properties"]["master_worldgenoverride_lua"]["default"]
        });
        assert!(dst_world_setting_evidence_known(&settings, SCHEMA, "master_world_size").unwrap());
    }

    #[test]
    fn opaque_world_lua_does_not_turn_a_projected_default_into_evidence() {
        for raw in [
            "return make_world()",
            "return {override_enabled=true, overrides={world_size=choose_size()}}",
            "return {override_enabled=false, overrides={world_size='small'}}",
        ] {
            let settings = json!({"master_world_size":"default","master_worldgenoverride_lua":raw});
            assert!(
                !dst_world_setting_evidence_known(&settings, SCHEMA, "master_world_size").unwrap()
            );
            assert!(dst_world_setting_evidence_known(&settings, SCHEMA, "cluster_name").unwrap());
        }
    }

    #[test]
    fn static_world_overrides_remain_observable_without_executing_them() {
        let mut settings = json!({"master_worldgenoverride_lua":"return {override_enabled=true, overrides={world_size='small'}}"});
        assert!(dst_world_setting_evidence_known(&settings, SCHEMA, "master_world_size").unwrap());
        settings["master_worldgenoverride_lua"] = json!("");
        settings["master_world_overrides_extra"] = json!("world_size = choose_size(),");
        assert!(!dst_world_setting_evidence_known(&settings, SCHEMA, "master_world_size").unwrap());
        settings["master_world_overrides_extra"] = json!("world_size = 'small',");
        assert!(dst_world_setting_evidence_known(&settings, SCHEMA, "master_world_size").unwrap());
    }
}
