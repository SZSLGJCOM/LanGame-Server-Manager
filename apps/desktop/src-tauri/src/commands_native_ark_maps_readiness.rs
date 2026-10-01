use super::*;
use app_platform_win::ProcessNetworkEndpoint;
use serde_json::{Map, Value, json};

pub fn apply_fixture_maps(
    module_id: &str,
    settings: &mut Map<String, Value>,
    requested: Option<&str>,
) -> Result<(), String> {
    let Some(requested) = requested else {
        return Ok(());
    };
    if !app_core::ark_maps::is_ark(module_id) {
        return Err("LANGAME_NATIVE_ARK_MAPS requires ASE or ASA".into());
    }
    let names = requested.split(',').map(str::trim).collect::<Vec<_>>();
    if names.is_empty() || names.len() > 2 || names.iter().any(|name| name.is_empty()) {
        return Err(
            "Native ARK map acceptance requires one or two additional package names".into(),
        );
    }
    let mut projected = settings.clone();
    projected.insert(
        "additional_maps".into(),
        Value::Array(
            names
                .iter()
                .enumerate()
                .map(|(index, name)| {
                    json!({
                        "id":format!("native-{}", index + 1), "map_name":name,
                        "name":format!("Native map {}", index + 1), "enabled":true
                    })
                })
                .collect(),
        ),
    );
    app_core::ark_maps::parse_additional_maps(&Value::Object(projected.clone()))?;
    // A generated disposable credential establishes each native RCON listener
    // without borrowing a password or configuration from the source package.
    projected.insert("rcon_enabled".into(), json!(true));
    projected.insert(
        "admin_password".into(),
        json!(uuid::Uuid::new_v4().simple().to_string()),
    );
    *settings = projected;
    Ok(())
}

fn enabled(instance: &InstanceDetails) -> Result<bool, String> {
    if !app_core::ark_maps::is_ark(&instance.summary.module_id) {
        return Ok(false);
    }
    let settings: Value = serde_json::from_str(&instance.settings_json)
        .map_err(|_| "invalid ARK map fixture settings")?;
    Ok(!app_core::ark_maps::parse_additional_maps(&settings)?.is_empty())
}

pub(super) fn capture_baselines(
    instance: &InstanceDetails,
    baselines: &mut ProbeBaselines,
) -> Result<(), String> {
    if !enabled(instance)? {
        return Ok(());
    }
    for map in app_core::ark_maps::processes(instance)? {
        let path = PathBuf::from(map.native_log_path);
        if let std::collections::hash_map::Entry::Vacant(entry) = baselines.logs.entry(path) {
            let baseline = LogBaseline::capture(entry.key())?;
            entry.insert(baseline);
        }
    }
    Ok(())
}

pub(super) async fn pending(
    descriptor: &ModuleDescriptor,
    instance: &InstanceDetails,
    endpoints: &[ProcessNetworkEndpoint],
    baselines: &mut ProbeBaselines,
) -> Result<Vec<String>, String> {
    if !enabled(instance)? {
        return Ok(Vec::new());
    }
    let mut pending = Vec::new();
    for map in app_core::ark_maps::processes(instance)? {
        let projected = app_core::ark_maps::project_process(instance, Some(&map.process_key))?;
        let active = instance.active_run.as_ref().and_then(|run| {
            run.processes
                .iter()
                .find(|process| process.process_key == map.process_key)
        });
        if active.is_none_or(|process| {
            process.status != "running"
                || process.pid.is_none()
                || process.process_identity.is_none()
        }) {
            return Err(format!(
                "ARK map {} has no stable running process",
                map.process_key
            ));
        }
        let pid = active
            .and_then(|process| process.pid)
            .ok_or("missing ARK map PID")?;
        for (name, protocol) in [("game", "udp"), ("rcon", "tcp")] {
            let port = port_number(&projected, name)?;
            if !owned(endpoints, &map.process_key, pid, port, protocol) {
                pending.push(format!("{}:{name}/{protocol}", map.process_key));
            }
        }
        let path = PathBuf::from(&map.native_log_path);
        let baseline = baselines
            .logs
            .get_mut(&path)
            .ok_or("missing per-map native log baseline")?;
        if !new_log_contains(
            &path,
            baseline,
            &[
                "has successfully started".into(),
                "advertising for join".into(),
            ],
        )? {
            pending.push(format!("{}:fresh-native-log", map.process_key));
        }
        if descriptor.summary.id == "arksurvivalevolved" {
            let port = port_number(&projected, "query")?;
            let query_ready = if owned(endpoints, &map.process_key, pid, port, "udp") {
                tokio::task::spawn_blocking(move || {
                    super::super::diagnostics::query_player_count("a2s_info", port)
                })
                .await
                .map_err(|_| "ARK map player-query worker failed")?
                .is_ok()
            } else {
                false
            };
            if !query_ready {
                pending.push(format!("{}:a2s-query", map.process_key));
            }
        }
    }
    Ok(pending)
}

fn owned(
    endpoints: &[ProcessNetworkEndpoint],
    key: &str,
    pid: u32,
    port: u16,
    protocol: &str,
) -> bool {
    endpoints.iter().any(|endpoint| {
        endpoint.process_key == key
            && endpoint.owning_pid == pid
            && endpoint.local_port == port
            && endpoint.protocol.eq_ignore_ascii_case(protocol)
    })
}

pub(super) fn verify_layout(instance: &InstanceDetails, install_root: &Path) -> Result<(), String> {
    if !enabled(instance)? {
        return Ok(());
    }
    let maps = app_core::ark_maps::processes(instance)?;
    let active = instance
        .active_run
        .as_ref()
        .ok_or("missing active ARK map session")?;
    if active.process_count != maps.len() || active.processes.len() != maps.len() {
        return Err("native ARK map process count does not match the enabled map list".into());
    }
    let saved = install_root.join("ShooterGame/Saved");
    let mut observed = std::collections::HashSet::new();
    for map in &maps {
        let directory = saved.join(&map.save_directory);
        if !directory.is_dir() {
            return Err(format!(
                "native ARK map {} did not create its expected save directory: {}",
                map.process_key,
                directory.display()
            ));
        }
        if !observed.insert(directory) {
            return Err("native ARK map save directories are not independent".into());
        }
    }
    println!(
        "NATIVE_ARK_MAPS module={} phase=all_maps_ready processes={} distinct_saves={} expected_saves={} scope=isolated_single_install gui=not_tested client_transfer=not_tested",
        instance.summary.module_id,
        maps.len(),
        observed.len(),
        maps.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn native_map_fixture_validates_before_replacing_existing_settings() {
        for edition in ["arksurvivalevolved", "arksurvivalascended"] {
            let mut settings = Map::from_iter([("server_name".into(), json!("Retained fixture"))]);
            let original = settings.clone();
            for requested in ["", "A,B,C", "TheIsland?ServerPassword=wrong", "../other"] {
                assert!(apply_fixture_maps(edition, &mut settings, Some(requested)).is_err());
                assert_eq!(settings, original);
            }
            apply_fixture_maps(edition, &mut settings, Some("TheIsland,ScorchedEarth_P")).unwrap();
            assert_eq!(settings["server_name"], original["server_name"]);
            assert_eq!(settings["additional_maps"].as_array().unwrap().len(), 2);
            assert_eq!(settings["rcon_enabled"], true);
        }
    }

    #[test]
    fn another_map_cannot_satisfy_a_native_endpoint_probe() {
        let endpoint = ProcessNetworkEndpoint {
            protocol: "udp".into(),
            local_address: "127.0.0.1".into(),
            local_port: 7777,
            owning_pid: 10,
            process_key: "main".into(),
            relation: "root".into(),
        };
        assert!(owned(
            std::slice::from_ref(&endpoint),
            "main",
            10,
            7777,
            "udp"
        ));
        assert!(!owned(
            std::slice::from_ref(&endpoint),
            "map-extra",
            10,
            7777,
            "udp"
        ));
        assert!(!owned(
            std::slice::from_ref(&endpoint),
            "main",
            11,
            7777,
            "udp"
        ));
    }

    #[test]
    fn native_map_acceptance_requires_each_maps_real_save_directory() {
        let root =
            std::env::temp_dir().join(format!("lg-native-map-layout-{}", uuid::Uuid::new_v4()));
        for edition in ["arksurvivalevolved", "arksurvivalascended"] {
            let mut processes = Vec::new();
            for (index, key) in ["main", "map-extra"].into_iter().enumerate() {
                processes.push(json!({"run_id":index + 1,"process_key":key,"display_name":key,
                    "pid":100 + index,"status":"running","crash_flag":false,"is_primary":index == 0}));
            }
            let instance: InstanceDetails = serde_json::from_value(json!({
                "summary":{"id":"owned","name":"ARK map fixture","module_id":edition,
                    "status":"Running","active_process_count":2,"bind_ip":"127.0.0.1","port_count":0,"autostart":false},
                "config_file_path":root.join("config/instance.json"),"saves_path":root.join("ShooterGame/Saved"),
                "auto_backup_on_stop":false,"backup_retention_count":2,"ports":[],
                "settings_json":json!({"additional_maps":[{"id":"extra","map_name":"ScorchedEarth_P","name":"Extra","enabled":true}]}).to_string(),
                "active_run":{"run_id":1,"pid":100,"process_count":2,"processes":processes}
            })).unwrap();
            let saved = root.join("ShooterGame/Saved");
            fs::create_dir_all(saved.join("owned")).unwrap();
            assert!(
                verify_layout(&instance, &root).is_err(),
                "{edition}: a ready main cannot replace a missing secondary save"
            );
            fs::create_dir_all(saved.join("owned-map-extra")).unwrap();
            verify_layout(&instance, &root).unwrap();
            fs::remove_dir_all(saved.join("owned-map-extra")).unwrap();
        }
        fs::remove_dir_all(root).unwrap();
    }
}
