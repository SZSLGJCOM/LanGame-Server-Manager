use super::*;
use serde_json::json;

fn cluster_arguments(settings: &Value, saves: &Path, edition: &str) -> Vec<String> {
    let instance: InstanceDetails = serde_json::from_value(json!({
        "summary": { "id": "map-a", "name": "Map A", "module_id": edition,
            "status": "Stopped", "bind_ip": "127.0.0.1", "port_count": 0, "autostart": false },
        "config_file_path": "config/settings.json", "saves_path": "saves",
        "auto_backup_on_stop": false, "backup_retention_count": 0,
        "settings_json": settings.to_string(), "ports": [], "active_run": null
    }))
    .unwrap();
    let context = TemplateContext {
        instance: &instance,
        settings,
        install_root: saves,
        config_dir: saves,
        data_dir: saves,
        logs_dir: saves,
        saves_dir: saves,
    };
    let prefix = if edition == "arksurvivalascended" {
        "arksa"
    } else {
        "arkse"
    };
    [
        "cluster_dir_override_flag",
        "map_ini_flags",
        "official_launch_flags",
        "custom_launch_flags",
    ]
    .into_iter()
    .flat_map(|name| {
        expand_resolved_argument_segments(&format!("{{{{{prefix}.{name}}}}}"), &context)
    })
    .collect()
}

#[test]
fn ark_map_ini_paths_are_separate_arguments_and_preserve_spaces() {
    for edition in ["arksurvivalascended", "arksurvivalevolved"] {
        let directory = Path::new("D:/ARK Worlds/config/ark-maps/main");
        let settings = json!({"_managed_ark_ini_directory": directory.to_string_lossy()});
        let args = cluster_arguments(&settings, Path::new("D:/instance/saves"), edition);
        assert_eq!(
            args,
            [
                format!("-GameINI={}", directory.join("Game.ini").display()),
                format!(
                    "-GameUserSettingsINI={}",
                    directory.join("GameUserSettings.ini").display()
                ),
            ]
        );
        let executable = directory.join(if edition == "arksurvivalascended" {
            "ArkAscendedServer.exe"
        } else {
            "ShooterGameServer.exe"
        });
        let command = crate::build_spawn_command_line(&executable.to_string_lossy(), &args);
        assert!(command.contains(&format!(
            "-GameINI=\"{}\"",
            directory.join("Game.ini").display()
        )));
        assert!(command.contains(&format!(
            "-GameUserSettingsINI=\"{}\"",
            directory.join("GameUserSettings.ini").display()
        )));
        assert_eq!(command, build_command_line(&executable, &args));
        assert_eq!(
            quote_launch_argument(Path::new("OtherServer.exe"), &args[0]),
            quote_command_segment(&args[0])
        );
        assert!(cluster_arguments(&json!({}), directory, edition).is_empty());
    }
}

#[test]
fn ark_map_ini_overrides_cannot_be_redirected_by_shared_expert_flags() {
    for edition in ["arksurvivalascended", "arksurvivalevolved"] {
        for raw in [
            "-GameINI=D:/other.ini",
            "GAMEUSERSETTINGSINI=D:/other.ini",
            "-DefGameIni=other.ini",
            "-iniFile=other.ini",
            "-IniBootstrap=other.ini",
            "-ConfigSubDir=other",
            "-UserDir=D:/other",
        ] {
            let settings = json!({"ark_mod_map_overrides":{"main":{"mods":{"1254808":{"active":false}}}}, "custom_launch_flags":raw});
            let issues = collect_module_launch_setting_issues(edition, &settings, &[]);
            assert!(
                issues
                    .iter()
                    .any(|issue| issue.code == "managed_launch_option_conflict"),
                "{edition}: {raw}"
            );
        }
        let shared =
            json!({"ark_mod_map_overrides":{}, "custom_launch_flags":"-GameINI=D:/expert.ini"});
        assert!(collect_module_launch_setting_issues(edition, &shared, &[]).is_empty());
    }
}

#[test]
fn ark_current_mod_map_targets_reach_native_launch_plans() {
    let descriptors =
        app_modules::discover_modules(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules"))
            .unwrap();
    for edition in ["arksurvivalascended", "arksurvivalevolved"] {
        let descriptor = descriptors
            .iter()
            .find(|module| module.summary.id == edition)
            .unwrap();
        let module = ModuleDetails {
            summary: descriptor.summary.clone(),
            schema_json: descriptor.schema_json.clone(),
            default_ports: descriptor.default_ports.clone(),
            install: descriptor.install.clone(),
            process: descriptor.process.clone(),
            workshop: descriptor.workshop.clone(),
            mods: None,
            runtime: descriptor.runtime.clone(),
        };
        let active_key = if edition == "arksurvivalascended" {
            "mod_ids_csv"
        } else {
            "active_mod_ids"
        };
        let settings = json!({
            active_key: "1254808,939713",
            "map_name":"TheIsland", "max_players":8, "server_name":"Map Targets",
            "additional_maps":[
                {"id":"desert","name":"Desert","map_name":"ScorchedEarth_P","enabled":true},
                {"id":"center","name":"Center","map_name":"TheCenter","enabled":true}
            ],
            "ark_mod_map_overrides":{
                "main":{"mods":{"1254808":{"active":true}}},
                "map-desert":{"mods":{"1254808":{"active":true}}},
                "map-center":{"mods":{"1254808":{"active":false}}}
            }
        });
        let ports = app_core::ark_maps::requested_ports(
            edition,
            &settings,
            &module.default_ports,
            &module.default_ports,
            &[],
        )
        .unwrap();
        let instance: InstanceDetails = serde_json::from_value(json!({
            "summary":{"id":"mod-targets","name":"Map Targets","module_id":edition,
                "status":"Stopped","bind_ip":"0.0.0.0","port_count":ports.len(),"autostart":false},
            "config_file_path":"C:/fixture/map targets/config/instance.json",
            "saves_path":"C:/fixture/map targets/runtime/ShooterGame/Saved",
            "auto_backup_on_stop":false,"backup_retention_count":0,
            "settings_json":settings.to_string(), "ports":ports, "active_run":null
        }))
        .unwrap();
        for (key, expected) in [
            ("main", "1254808,939713"),
            ("map-desert", "1254808,939713"),
            ("map-center", "939713"),
        ] {
            let projected = app_core::ark_maps::project_process(&instance, Some(key)).unwrap();
            let effective: Value = serde_json::from_str(&projected.settings_json).unwrap();
            assert_eq!(effective[active_key], expected, "{edition}: {key}");
            let plan = build_launch_plan_with_override(
                &AppSettings::default(),
                &module,
                &projected,
                Some("C:/fixture/program"),
            )
            .unwrap();
            let path = Path::new("C:/fixture/map targets/config/ark-maps").join(key);
            for (option, filename) in [
                ("-GameINI=", "Game.ini"),
                ("-GameUserSettingsINI=", "GameUserSettings.ini"),
            ] {
                let actual = plan
                    .args
                    .iter()
                    .find_map(|arg| arg.strip_prefix(option))
                    .unwrap_or_else(|| panic!("missing {option}: {:?}", plan.args));
                assert_eq!(
                    Path::new(actual),
                    path.join(filename),
                    "{edition}: {key} must consume its own native file"
                );
            }
            if edition == "arksurvivalascended" {
                assert!(plan.args.contains(&format!("-mods={expected}")));
            }
            assert!(
                !plan
                    .validation_issues
                    .iter()
                    .any(|issue| issue.code == "ark_mod_map_overrides_invalid")
            );
        }
    }
}

#[test]
fn ark_map_ini_paths_always_quote_native_separator_characters() {
    for executable in ["ArkAscendedServer.exe", "ShooterGameServer.exe"] {
        for directory in [
            "D:/ARK,Worlds/config",
            "D:/ARK)Worlds/config",
            "D:/ARK/config",
        ] {
            for key in ["-GameINI", "-GameUserSettingsINI"] {
                let argument = format!("{key}={directory}/Game.ini");
                assert_eq!(
                    quote_launch_argument(Path::new(executable), &argument),
                    format!("{key}=\"{directory}/Game.ini\""),
                );
            }
        }
    }
}

#[test]
fn ark_cluster_shared_directory_is_identical_across_instance_save_roots() {
    let settings = json!({ "cluster_id": "group-a", "cluster_directory": "D:/Ark Shared/group-a" });
    for edition in ["arksurvivalascended", "arksurvivalevolved"] {
        for saves in ["D:/instances/map-a/saves", "D:/instances/map-b/saves"] {
            let args = cluster_arguments(&settings, Path::new(saves), edition);
            assert!(
                args.contains(&String::from("-ClusterDirOverride=D:/Ark Shared/group-a")),
                "{args:?}"
            );
            assert!(args.contains(&String::from("-clusterid=group-a")));
        }
    }
}

#[test]
fn ark_cluster_empty_directory_preserves_existing_local_path() {
    let saves = Path::new("D:/instances/map-a/saves");
    for edition in ["arksurvivalascended", "arksurvivalevolved"] {
        let args = cluster_arguments(&json!({ "cluster_id": "group-a" }), saves, edition);
        assert!(args.contains(&format!(
            "-ClusterDirOverride={}",
            saves.join("cluster").display()
        )));
    }
}

#[test]
fn ark_cluster_invalid_identity_and_directory_block_launch() {
    for edition in ["arksurvivalascended", "arksurvivalevolved"] {
        for settings in [
            json!({ "cluster_id": "group-a\n-NoBattlEye" }),
            json!({ "cluster_id": "group\u{0000}a" }),
            json!({ "cluster_id": "group-a", "cluster_directory": "relative/cluster" }),
            json!({ "cluster_id": "group-a", "cluster_directory": "D:/cluster\"bad" }),
            json!({ "cluster_directory": "D:/shared/cluster" }),
        ] {
            let issues = collect_module_launch_setting_issues(edition, &settings, &[]);
            assert!(
                issues.iter().any(|issue| issue.severity == "error"),
                "{edition}: {settings}"
            );
        }
    }
}

#[test]
fn ark_cluster_managed_and_raw_duplicates_are_rejected_without_removing_expert_flags() {
    for edition in ["arksurvivalascended", "arksurvivalevolved"] {
        for raw in ["-clusterid=other", "-ClusterDirOverride=D:/other"] {
            let settings = json!({ "cluster_id": "group-a", "custom_launch_flags": raw });
            assert!(!collect_module_launch_setting_issues(edition, &settings, &[]).is_empty());
        }
        let expert = json!({ "custom_launch_flags": "-clusterid=expert -ClusterDirOverride=\"D:/Ark Shared/expert\" -NoBattlEye" });
        assert!(collect_module_launch_setting_issues(edition, &expert, &[]).is_empty());
        let args = cluster_arguments(&expert, Path::new("D:/instance/saves"), edition);
        assert!(args.contains(&String::from("-clusterid=expert")));
        assert!(args.contains(&String::from("-ClusterDirOverride=D:/Ark Shared/expert")));
        for raw in [
            "-clusterid=one -clusterid=two",
            "-ClusterDirOverride",
            "-clusterid=one -ClusterDirOverride=../other",
        ] {
            assert!(
                !collect_module_launch_setting_issues(
                    edition,
                    &json!({ "custom_launch_flags": raw }),
                    &[]
                )
                .is_empty()
            );
        }
    }
}

#[test]
fn ark_map_clusters_reject_raw_overrides_of_per_map_native_inputs() {
    for edition in ["arksurvivalascended", "arksurvivalevolved"] {
        for enabled in [false, true] {
            for raw in [
                "-port=7777",
                "-QuErYpOrT 27015",
                "-RCONPort=27020",
                "?RCONEnabled=false",
                "?AltSaveDirectoryName=shared",
                "-AltSaveDirectoryName shared",
                "-ABSLOG=\"D:/Ark Shared/server.log\"",
                "?SessionName=\"Shared name\"",
                "-MULTIHOME 127.0.0.1",
                "QueryPort=27015",
                "ScorchedEarth_P?MaxPlayers=10?AltSaveDirectoryName=shared?Port=7777",
            ] {
                let settings = json!({
                    "cluster_id": "managed-group", "custom_launch_flags": raw,
                    "additional_maps": [{"id": "desert", "name": "Desert", "map_name": "ScorchedEarth_P", "enabled": enabled}]
                });
                let issues = collect_module_launch_setting_issues(edition, &settings, &[]);
                assert_eq!(issues.len(), 1, "{edition}: {raw}");
                assert_eq!(issues[0].severity, "error");
                assert_eq!(issues[0].code, "managed_launch_option_conflict");
                assert_eq!(issues[0].context["field"], "custom_launch_flags");
            }
        }
    }
}

#[test]
fn ark_cluster_flags_preserve_standalone_experts_and_unrelated_future_options() {
    for edition in ["arksurvivalascended", "arksurvivalevolved"] {
        for maps in [
            json!([]),
            json!([{"id": "desert", "name": "Desert", "map_name": "ScorchedEarth_P", "enabled": true}]),
        ] {
            let settings = json!({
                "cluster_id": "managed-group", "additional_maps": maps,
                "custom_launch_flags": "-NoBattlEye -FutureOption=\"literal ?Port=9000\" -QueryPortExtra=1234 -FutureSwitch"
            });
            assert!(collect_module_launch_setting_issues(edition, &settings, &[]).is_empty());
            let args = cluster_arguments(&settings, Path::new("D:/instance/saves"), edition);
            assert!(args.contains(&"-FutureOption=literal ?Port=9000".to_owned()));
            assert!(args.contains(&"-QueryPortExtra=1234".to_owned()));
            assert!(args.contains(&"-FutureSwitch".to_owned()));
        }
        for raw in [
            "-port=9000",
            "-ABSLOG=\"D:/Ark Shared/expert.log\"",
            "?AltSaveDirectoryName=expert",
        ] {
            let settings = json!({"additional_maps": [], "custom_launch_flags": raw});
            assert!(
                collect_module_launch_setting_issues(edition, &settings, &[]).is_empty(),
                "{edition}: {raw}"
            );
        }
    }
}

#[test]
fn ark_cluster_existing_file_is_rejected_and_normal_directory_is_preserved() {
    let root = crate::test_support::unique_test_root();
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("uploads-file");
    std::fs::write(&file, "preserve this file").unwrap();
    let error =
        app_core::ark_cluster::resolve_cluster_directory("group-a", &file.to_string_lossy(), &root)
            .unwrap_err();
    assert_eq!(error.field, "cluster_directory");
    assert!(error.message.contains("existing file"));
    let actual = app_core::ark_cluster::resolve_cluster_directory(
        " group-a ",
        &root.to_string_lossy(),
        &root,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        std::fs::canonicalize(actual).unwrap(),
        std::fs::canonicalize(&root).unwrap()
    );
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "preserve this file"
    );
    std::fs::remove_dir_all(root).unwrap();
}
