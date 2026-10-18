use super::*;
use serde_json::json;

#[test]
fn asa_native_query_argument_uses_each_maps_registered_nondefault_port_once() {
    let modules = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules");
    let descriptor = app_modules::discover_modules(modules)
        .unwrap()
        .into_iter()
        .find(|module| module.summary.id == "arksurvivalascended")
        .unwrap();
    let module = ModuleDetails {
        summary: descriptor.summary,
        schema_json: descriptor.schema_json,
        default_ports: descriptor.default_ports,
        install: descriptor.install,
        process: descriptor.process,
        workshop: descriptor.workshop,
        mods: None,
        runtime: descriptor.runtime,
    };
    let instance: InstanceDetails = serde_json::from_value(json!({
        "summary":{"id":"query-ports","name":"Query Ports","module_id":"arksurvivalascended",
            "status":"Stopped","bind_ip":"0.0.0.0","port_count":8,"autostart":false},
        "config_file_path":"C:/fixture/query-ports/config/instance.json",
        "saves_path":"C:/fixture/query-ports/runtime/ShooterGame/Saved",
        "auto_backup_on_stop":false,"backup_retention_count":0,
        "settings_json":json!({"map_name":"TheIsland_WP","max_players":8,"server_name":"Query Ports",
            "additional_maps":[{"id":"desert","name":"Desert Query Ports","map_name":"ScorchedEarth_WP","enabled":true}]}).to_string(),
        "ports":[
            {"name":"game","protocol":"udp","port":27777},
            {"name":"peer","protocol":"udp","port":27778},
            {"name":"query","protocol":"udp","port":47015},
            {"name":"rcon","protocol":"tcp","port":47020},
            {"name":"map-desert-game","protocol":"udp","port":27787},
            {"name":"map-desert-peer","protocol":"udp","port":27788},
            {"name":"map-desert-query","protocol":"udp","port":47025},
            {"name":"map-desert-rcon","protocol":"tcp","port":47030}
        ],"active_run":null
    })).unwrap();
    for (key, query_port) in [("main", 47015), ("map-desert", 47025)] {
        let projected = app_core::ark_maps::project_process(&instance, Some(key)).unwrap();
        let plan = build_launch_plan_with_override(
            &AppSettings::default(),
            &module,
            &projected,
            Some("C:/fixture/program"),
        )
        .unwrap();
        let query = plan
            .args
            .iter()
            .filter(|argument| {
                argument
                    .split_once('=')
                    .is_some_and(|(key, _)| key.eq_ignore_ascii_case("-QueryPort"))
            })
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            query,
            [format!("-QueryPort={query_port}")],
            "{key}: each native Steam subsystem needs its own query port"
        );
        assert!(
            plan.args[0].contains(&format!("?QueryPort={query_port}")),
            "preserve the native URL configuration"
        );
        #[cfg(windows)]
        {
            let command = crate::build_spawn_command_line(&plan.executable_path, &plan.args);
            assert!(command.contains(&format!("\"{}\"", plan.args[0])));
            assert!(
                command.contains(&format!(" -QueryPort={query_port} ")),
                "the keyword must remain outside the quoted URL when the server name has spaces"
            );
        }
    }
}

#[test]
fn asa_query_port_is_managed_for_single_enabled_and_paused_map_configurations() {
    for maps in [
        json!([]),
        json!([{"id":"desert","name":"Desert","map_name":"ScorchedEarth_WP","enabled":true}]),
        json!([{"id":"desert","name":"Desert","map_name":"ScorchedEarth_WP","enabled":false}]),
    ] {
        for raw in [
            "-QueryPort=47015",
            "-qUeRyPoRt 47015",
            "?QueryPort=47015",
            "QueryPort=47015",
            "TheIsland_WP?QueryPort=47015",
            "-QueryPort=",
            "-QueryPort",
        ] {
            let settings = json!({"additional_maps":maps,"custom_launch_flags":raw});
            let issues =
                collect_module_launch_setting_issues("arksurvivalascended", &settings, &[]);
            assert_eq!(issues.len(), 1, "{raw}: {maps}");
            assert_eq!(issues[0].severity, "error");
            assert_eq!(issues[0].code, "managed_launch_option_conflict");
            assert_eq!(issues[0].context["field"], "custom_launch_flags");
        }
    }
}

#[test]
fn asa_query_port_protection_preserves_unrelated_expert_flags_and_ase_single_map_behavior() {
    for raw in [
        "-NoBattlEye -QueryPortExtra=47015",
        "-FutureOption=\"literal ?QueryPort=47015\"",
        "-port=27777",
        "-ABSLOG=\"D:/Ark Shared/expert.log\"",
        "?AltSaveDirectoryName=expert",
        "-httpproxy=127.0.0.1:27897",
    ] {
        let settings = json!({"additional_maps":[],"custom_launch_flags":raw});
        assert!(
            collect_module_launch_setting_issues("arksurvivalascended", &settings, &[]).is_empty(),
            "{raw}"
        );
    }
    let ase = json!({"additional_maps":[],"custom_launch_flags":"-QueryPort=47015"});
    assert!(collect_module_launch_setting_issues("arksurvivalevolved", &ase, &[]).is_empty());
}
