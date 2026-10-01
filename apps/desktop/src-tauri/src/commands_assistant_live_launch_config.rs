use super::*;

pub(super) fn validate_new_launch_configuration(
    current: &InstanceDetails,
    original: &InstanceDetails,
    ready: bool,
) -> LiveResult {
    let values: Value = serde_json::from_str(&current.settings_json)?;
    let defaults: Value = serde_json::from_str(&original.settings_json)?;
    let allowed = [
        "cluster_name",
        "cluster_description",
        "offline_cluster",
        "lan_only_cluster",
        "disable_data_collection",
        "enable_caves",
        "master_world_size",
        "max_players",
        "pause_when_empty",
        "bind_ip",
    ];
    for (key, value) in values
        .as_object()
        .ok_or("launch settings must be an object")?
    {
        if !allowed.contains(&key.as_str()) && defaults.get(key) != Some(value) {
            return Err(
                format!("launch acceptance refused an out-of-scope setting change: {key}").into(),
            );
        }
    }
    if current.summary.id != original.summary.id
        || current.summary.module_id != "dontstarve"
        || (current.summary.bind_ip != original.summary.bind_ip
            && current.summary.bind_ip != "127.0.0.1")
        || values["bind_ip"].as_str() != Some(current.summary.bind_ip.as_str())
        || current.summary.autostart
        || current.config_file_path != original.config_file_path
        || current.saves_path != original.saves_path
    {
        return Err("launch configuration changed the isolated instance boundary".into());
    }
    if ready
        && (values["offline_cluster"] != true
            || values["lan_only_cluster"] != true
            || values["disable_data_collection"] != true
            || values["cluster_token"] != ""
            || values["enable_caves"] != false
            || values["master_world_size"] != "small"
            || values["max_players"] != 4
            || current.summary.bind_ip != "127.0.0.1"
            || values["bind_ip"] != "127.0.0.1"
            || values["cluster_name"] != "本地开服验收")
    {
        return Err(
            "launch configuration does not satisfy the requested isolated single-world settings"
                .into(),
        );
    }
    for key in [
        "shared_workshop_mod_ids",
        "shared_workshop_collection_ids",
        "master_enabled_workshop_mod_ids",
        "caves_enabled_workshop_mod_ids",
    ] {
        if values[key]
            .as_str()
            .is_some_and(|value| !value.trim().is_empty())
        {
            return Err("native launch refused a Workshop download".into());
        }
    }
    Ok(())
}
