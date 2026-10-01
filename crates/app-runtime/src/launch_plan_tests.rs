use super::*;
use crate::performance_policy::{
    balanced_affinity_group_count, balanced_affinity_mask_for_group, host_reserve_affinity_mask,
};
use crate::test_support::unique_test_root;
use app_core::{
    InstallSpec, InstanceDetails, InstanceStatus, InstanceSummary, ModuleDetails, ModuleSummary,
    PortBinding, ProcessSpec,
};

#[test]
fn runtime_performance_policy_uses_module_defaults_and_instance_overrides() {
    let module_policy = RuntimePerformancePolicy {
        priority_class: RuntimePriorityClass::AboveNormal,
        cpu_affinity_mask: Some(0b11),
        apply_to_child_processes: true,
        startup_stagger_ms: 1500,
        child_process_stagger_ms: 500,
        resource_limits: Default::default(),
    };
    let settings = serde_json::json!({
        "runtime_performance": {
            "priority_class": "high",
            "cpu_affinity_mask": "0xF0",
            "apply_to_child_processes": false,
            "startup_stagger_ms": 2500,
            "child_process_stagger_ms": 750
        }
    });

    let policy = resolve_runtime_performance_policy(&module_policy, &settings);

    assert_eq!(policy.priority_class, RuntimePriorityClass::High);
    assert_eq!(policy.cpu_affinity_mask, Some(0xF0));
    assert!(!policy.apply_to_child_processes);
    assert_eq!(policy.startup_stagger_ms, 2500);
    assert_eq!(policy.child_process_stagger_ms, 750);
}

#[test]
fn runtime_performance_policy_keeps_module_values_without_instance_overrides() {
    let module_policy = RuntimePerformancePolicy {
        priority_class: RuntimePriorityClass::BelowNormal,
        cpu_affinity_mask: Some(0b1010),
        apply_to_child_processes: false,
        startup_stagger_ms: 3000,
        child_process_stagger_ms: 900,
        resource_limits: Default::default(),
    };

    let policy = resolve_runtime_performance_policy(&module_policy, &Value::Null);

    assert_eq!(policy.priority_class, RuntimePriorityClass::BelowNormal);
    assert_eq!(policy.cpu_affinity_mask, Some(0b1010));
    assert!(!policy.apply_to_child_processes);
    assert_eq!(policy.startup_stagger_ms, 3000);
    assert_eq!(policy.child_process_stagger_ms, 900);
}

#[test]
fn runtime_performance_policy_resolves_cpu_affinity_presets() {
    assert_eq!(half_affinity_mask(8, false), Some(0x0F));
    assert_eq!(half_affinity_mask(8, true), Some(0xF0));
    assert_eq!(host_reserve_affinity_mask(4), Some(0x0E));
    assert_eq!(balanced_affinity_group_count(4), 2);
    assert_eq!(balanced_affinity_group_count(8), 4);
    assert_eq!(balanced_affinity_group_count(16), 4);
    assert_eq!(balanced_affinity_mask_for_group(8, 0, 4), Some(0x03));
    assert_eq!(balanced_affinity_mask_for_group(8, 1, 4), Some(0x0C));
    assert_eq!(balanced_affinity_mask_for_group(8, 2, 4), Some(0x30));
    assert_eq!(balanced_affinity_mask_for_group(8, 3, 4), Some(0xC0));
    assert_eq!(balanced_affinity_mask_for_group(10, 0, 4), Some(0x007));
    assert_eq!(balanced_affinity_mask_for_group(10, 1, 4), Some(0x038));
    assert_eq!(balanced_affinity_mask_for_group(10, 2, 4), Some(0x0C0));
    assert_eq!(balanced_affinity_mask_for_group(10, 3, 4), Some(0x300));

    let module_policy = RuntimePerformancePolicy::default();
    let settings = serde_json::json!({
        "runtime_performance": {
            "cpu_affinity_preset": "first_half"
        }
    });
    let policy =
        resolve_runtime_performance_policy_for_instance(&module_policy, &settings, "demo-1");

    if available_logical_cpu_count() > 2 {
        assert_eq!(
            policy.cpu_affinity_mask,
            half_affinity_mask(available_logical_cpu_count().clamp(1, 64), false)
        );
    } else {
        assert_eq!(policy.cpu_affinity_mask, None);
    }
}

#[test]
fn runtime_performance_policy_balances_multi_instance_affinity_by_instance_id() {
    let module_policy = RuntimePerformancePolicy::default();
    let settings = serde_json::json!({
        "runtime_performance": {
            "cpu_affinity_preset": "auto_balance"
        }
    });

    let first =
        resolve_runtime_performance_policy_for_instance(&module_policy, &settings, "server-a");
    let second =
        resolve_runtime_performance_policy_for_instance(&module_policy, &settings, "server-b");

    if available_logical_cpu_count() > 2 {
        assert!(first.cpu_affinity_mask.is_some());
        assert!(second.cpu_affinity_mask.is_some());
        assert_eq!(
            first.cpu_affinity_mask,
            balanced_instance_affinity_mask(available_logical_cpu_count().clamp(1, 64), "server-a")
        );
    } else {
        assert_eq!(first.cpu_affinity_mask, None);
        assert_eq!(second.cpu_affinity_mask, None);
    }
}

#[test]
fn runtime_performance_policy_prefers_explicit_cpu_affinity_mask_over_preset() {
    let module_policy = RuntimePerformancePolicy::default();
    let settings = serde_json::json!({
        "runtime_performance": {
            "cpu_affinity_mask": "0xAA",
            "cpu_affinity_preset": "first_half"
        }
    });

    let policy =
        resolve_runtime_performance_policy_for_instance(&module_policy, &settings, "demo-1");

    assert_eq!(policy.cpu_affinity_mask, Some(0xAA));
}

#[test]
fn runtime_performance_policy_all_preset_clears_module_affinity() {
    let module_policy = RuntimePerformancePolicy {
        cpu_affinity_mask: Some(0xAA),
        ..RuntimePerformancePolicy::default()
    };
    let settings = serde_json::json!({
        "runtime_performance": {
            "cpu_affinity_preset": "all_cpus"
        }
    });

    let resolution =
        resolve_runtime_performance_resolution_for_instance(&module_policy, &settings, "demo-1");

    assert_eq!(resolution.policy.cpu_affinity_mask, None);
    assert_eq!(resolution.preview.cpu_affinity_source, "instance_preset");
    assert_eq!(
        resolution.preview.cpu_affinity_preset,
        Some(String::from("all_cpus"))
    );
}

#[test]
fn ark_ascended_mod_ids_normalize_multiline_and_url_ids() {
    let raw = "929420\n929420, 940123; cf-950456\nhttps://www.curseforge.com/ark-survival-ascended/projects/970111\nhttps://www.curseforge.com/ark-survival-ascended/mods/example?projectId=960789\nnot-a-mod";

    assert_eq!(
        parse_ark_ascended_mod_ids(raw),
        ["929420", "940123", "950456", "970111", "960789"]
            .map(String::from)
            .to_vec()
    );

    assert!(
        parse_ark_ascended_mod_ids(
            "https://www.curseforge.com/ark-survival-ascended/mods/slug-without-project-id\nnot-a-mod"
        )
        .is_empty()
    );
}

#[test]
fn ark_ascended_passive_mod_ids_are_distinct_from_active_mods() {
    let settings = serde_json::json!({
        "mod_ids_csv": "929420,940123",
        "passive_mod_ids_csv": "950456\nhttps://www.curseforge.com/ark-survival-ascended/projects/970111"
    });

    assert_eq!(
        parse_ark_ascended_mod_ids(settings["mod_ids_csv"].as_str().unwrap()),
        ["929420", "940123"].map(String::from).to_vec()
    );
    assert_eq!(
        parse_ark_ascended_mod_ids(settings["passive_mod_ids_csv"].as_str().unwrap()),
        ["950456", "970111"].map(String::from).to_vec()
    );
}

#[test]
fn soulmask_workshop_mods_arg_renders_comma_separated_workshop_ids() {
    let settings = serde_json::json!({
        "mod_workshop_ids": "3324057706\nhttps://steamcommunity.com/sharedfiles/filedetails/?id=3330908154;3324057706\nnot-a-workshop-id"
    });

    assert_eq!(
        render_soulmask_workshop_mods_arg(&settings),
        r#"-mod="3324057706,3330908154""#
    );

    let empty_settings = serde_json::json!({
        "mod_workshop_ids": "not-a-workshop-id"
    });
    assert_eq!(render_soulmask_workshop_mods_arg(&empty_settings), "");
}

#[test]
fn rust_launch_flags_expand_with_explicit_security_opt_out() {
    let temp_root = unique_test_root();
    let games_root = temp_root.join("games");
    let install_root = games_root.join("rust");
    let executable_path = install_root.join("RustDedicated.exe");
    std::fs::create_dir_all(executable_path.parent().expect("install root"))
        .expect("create install root");
    std::fs::write(&executable_path, []).expect("write exe placeholder");

    let instance_root = temp_root.join("instances").join("rust-main");
    let config_path = instance_root.join("config").join("instance.json");
    let saves_path = install_root.join("server").join("rust-main");
    std::fs::create_dir_all(config_path.parent().expect("config parent")).expect("create config");
    std::fs::create_dir_all(&saves_path).expect("create saves");

    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: temp_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: temp_root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: temp_root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let module = ModuleDetails {
        summary: ModuleSummary {
            id: String::from("rust"),
            name: String::from("Rust Dedicated Server"),
            version: String::from("0.1.0"),
            description: None,
            steam_app_id: Some(258550),
            install_state: app_core::InstallState::NotInstalled,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
        schema_json: None,
        default_ports: vec![
            PortBinding {
                name: String::from("game"),
                protocol: String::from("udp"),
                port: 28015,
            },
            PortBinding {
                name: String::from("rcon"),
                protocol: String::from("tcp"),
                port: 28016,
            },
            PortBinding {
                name: String::from("query"),
                protocol: String::from("udp"),
                port: 28017,
            },
        ],
        install: Some(InstallSpec {
            shared_game_dir: String::from("rust"),
            download_url_windows: None,
            download_integrity_windows: None,
            source: None,
            verification_path: None,
            minecraft: None,
        }),
        process: Some(ProcessSpec {
            environment_template: Default::default(),
            executable: String::from("RustDedicated.exe"),
            args_template: vec![
                String::from("-batchmode"),
                String::from("{{rust.world_configfile_args}}"),
                String::from("{{rust.insecure_flag}}"),
                String::from("{{rust.custom_launch_flags}}"),
                String::from("+server.identity"),
                String::from("{{instance.id}}"),
            ],
            working_directory_template: None,
            window_policy: ProcessWindowPolicy::Background,
            host_surface: ProcessHostSurface::ManagedTerminal,
            host_notes: None,
        }),
        workshop: None,
        mods: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
    };
    let instance = InstanceDetails {
        summary: InstanceSummary {
            id: String::from("rust-main"),
            name: String::from("Rust Main"),
            module_id: String::from("rust"),
            status: InstanceStatus::Stopped,
            bind_ip: String::from("0.0.0.0"),
            port_count: 3,
            autostart: false,
            active_process_count: 0,
        },
        config_file_path: config_path.to_string_lossy().into_owned(),
        saves_path: saves_path.to_string_lossy().into_owned(),
        settings_json: String::from(
            r#"{"world_config_json":"{\"MainRoads\":false}","custom_launch_flags":"+server.tickrate 30 -load \"oxide profile\""}"#,
        ),
        ports: vec![
            PortBinding {
                name: String::from("game"),
                protocol: String::from("udp"),
                port: 28015,
            },
            PortBinding {
                name: String::from("rcon"),
                protocol: String::from("tcp"),
                port: 28016,
            },
            PortBinding {
                name: String::from("query"),
                protocol: String::from("udp"),
                port: 28017,
            },
        ],
        auto_backup_on_stop: false,
        backup_retention_count: 1,
        backup_uses_declared_saves_path: true,
        active_run: None,
    };

    let launch_plan =
        build_launch_plan(&settings, &module, &instance).expect("build rust launch plan");
    assert_eq!(
        launch_plan.args,
        vec![
            String::from("-batchmode"),
            String::from("+world.configfile"),
            String::from("world-config.json"),
            String::from("+server.tickrate"),
            String::from("30"),
            String::from("-load"),
            String::from("oxide profile"),
            String::from("+server.identity"),
            String::from("rust-main"),
        ]
    );
    for (value, expected_count) in [
        (serde_json::json!(true), 0),
        (serde_json::json!(false), 1),
        (serde_json::Value::Null, 0),
        (serde_json::json!("false"), 0),
    ] {
        let mut explicit_instance = instance.clone();
        let mut values: serde_json::Value =
            serde_json::from_str(&instance.settings_json).expect("settings");
        values["secure"] = value;
        explicit_instance.settings_json = values.to_string();
        let plan = build_launch_plan(&settings, &module, &explicit_instance)
            .expect("build launch plan with explicit security setting");
        assert_eq!(
            plan.args.iter().filter(|arg| *arg == "-insecure").count(),
            expected_count
        );
        assert!(!plan.args.iter().any(|arg| arg.contains("server.secure")));
    }
}

#[test]
fn build_launch_plan_resolves_paths_and_settings_tokens() {
    let temp_root = unique_test_root();
    let games_root = temp_root.join("games");
    let install_root = games_root.join("dontstarve");
    let executable_path = install_root.join("bin").join("server.exe");
    std::fs::create_dir_all(executable_path.parent().expect("bin parent")).expect("create bin");
    std::fs::write(&executable_path, []).expect("write exe placeholder");

    let instance_root = temp_root.join("instances").join("dst-main");
    let config_path = instance_root.join("config").join("instance.json");
    let saves_path = instance_root.join("config").join("savegame");
    std::fs::create_dir_all(config_path.parent().expect("config parent")).expect("create config");
    std::fs::create_dir_all(&saves_path).expect("create saves");

    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: temp_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: temp_root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: temp_root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let module = ModuleDetails {
        summary: ModuleSummary {
            id: String::from("dontstarve"),
            name: String::from("Dont Starve Together"),
            version: String::from("0.1.0"),
            description: None,
            steam_app_id: Some(343050),
            install_state: app_core::InstallState::NotInstalled,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
        schema_json: None,
        default_ports: vec![PortBinding {
            name: String::from("master"),
            protocol: String::from("udp"),
            port: 10999,
        }],
        install: Some(InstallSpec {
            shared_game_dir: String::from("dontstarve"),
            download_url_windows: None,
            download_integrity_windows: None,
            source: None,
            verification_path: None,
            minecraft: None,
        }),
        process: Some(ProcessSpec {
            environment_template: Default::default(),
            executable: String::from("bin/server.exe"),
            args_template: vec![
                String::from("-cluster"),
                String::from("{{settings.cluster_name}}"),
                String::from("-bind"),
                String::from("{{instance.bind_ip}}"),
                String::from("-config"),
                String::from("{{paths.config_dir}}"),
                String::from("-savedir"),
                String::from("{{paths.saves_dir}}"),
                String::from("-port"),
                String::from("{{ports.master.port}}"),
            ],
            working_directory_template: None,
            window_policy: ProcessWindowPolicy::Background,
            host_surface: ProcessHostSurface::ManagedTerminal,
            host_notes: None,
        }),
        workshop: None,
        mods: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
    };
    let instance = InstanceDetails {
        summary: InstanceSummary {
            id: String::from("dst-main"),
            name: String::from("DST Main"),
            module_id: String::from("dontstarve"),
            status: InstanceStatus::Stopped,
            bind_ip: String::from("0.0.0.0"),
            port_count: 1,
            autostart: false,
            active_process_count: 0,
        },
        config_file_path: config_path.to_string_lossy().into_owned(),
        saves_path: saves_path.to_string_lossy().into_owned(),
        backup_uses_declared_saves_path: true,
        auto_backup_on_stop: false,
        backup_retention_count: 10,
        settings_json: String::from(
            r#"{
  "cluster_name": "DST Main",
  "bind_ip": "0.0.0.0"
}"#,
        ),
        ports: vec![PortBinding {
            name: String::from("master"),
            protocol: String::from("udp"),
            port: 10999,
        }],
        active_run: None,
    };

    let launch_plan = build_launch_plan(&settings, &module, &instance).expect("build plan");

    assert_eq!(launch_plan.install_root, install_root.to_string_lossy());
    assert!(
        launch_plan.executable_path.ends_with("bin/server.exe")
            || launch_plan.executable_path.ends_with("bin\\server.exe")
    );
    assert!(launch_plan.executable_exists);
    assert_eq!(
        launch_plan.args,
        vec![
            String::from("-cluster"),
            String::from("DST Main"),
            String::from("-bind"),
            String::from("0.0.0.0"),
            String::from("-config"),
            instance_root.join("config").to_string_lossy().into_owned(),
            String::from("-savedir"),
            saves_path.to_string_lossy().into_owned(),
            String::from("-port"),
            String::from("10999"),
        ]
    );

    let _ = std::fs::remove_dir_all(temp_root);
}

#[test]
fn build_launch_plan_keeps_explicit_java_runtime_path() {
    let temp_root = unique_test_root();
    let install_root = temp_root.join("games").join("java-server");
    let runtime_root = install_root.join("other-runtime").join("bin");
    let config_dir = temp_root
        .join("instances")
        .join("java-server-1")
        .join("config");
    std::fs::create_dir_all(&runtime_root).expect("create other runtime");
    std::fs::create_dir_all(&config_dir).expect("create instance config");
    for executable in ["java", "java.exe"] {
        std::fs::write(runtime_root.join(executable), []).expect("write other Java executable");
    }
    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: temp_root.join("instances").to_string_lossy().into_owned(),
        games_root: temp_root.join("games").to_string_lossy().into_owned(),
        modules_root: temp_root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: temp_root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let mut module = ModuleDetails {
        summary: ModuleSummary {
            id: String::from("java-server"),
            name: String::from("Java Server"),
            version: String::from("0.1.0"),
            description: None,
            steam_app_id: None,
            install_state: app_core::InstallState::NotInstalled,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
        schema_json: None,
        default_ports: vec![],
        install: Some(InstallSpec {
            shared_game_dir: String::from("java-server"),
            download_url_windows: None,
            download_integrity_windows: None,
            source: None,
            verification_path: None,
            minecraft: None,
        }),
        process: Some(ProcessSpec {
            environment_template: Default::default(),
            executable: String::new(),
            args_template: vec![],
            working_directory_template: Some(String::from("{{paths.install_root}}")),
            window_policy: ProcessWindowPolicy::Background,
            host_surface: ProcessHostSurface::ManagedTerminal,
            host_notes: None,
        }),
        workshop: None,
        mods: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
    };
    let instance = InstanceDetails {
        summary: InstanceSummary {
            id: String::from("java-server-1"),
            name: String::from("Java Server"),
            module_id: String::from("java-server"),
            status: InstanceStatus::Stopped,
            bind_ip: String::from("0.0.0.0"),
            port_count: 0,
            autostart: false,
            active_process_count: 0,
        },
        config_file_path: config_dir
            .join("instance.json")
            .to_string_lossy()
            .into_owned(),
        saves_path: temp_root.join("saves").to_string_lossy().into_owned(),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: false,
        backup_retention_count: 10,
        settings_json: String::from("{}"),
        ports: vec![],
        active_run: None,
    };
    let expected_java = install_root.join("jre64").join("bin").join("java.exe");

    for executable in ["jre64/bin/java.exe", r"jre64\bin\java.exe"] {
        module.process.as_mut().expect("process").executable = String::from(executable);
        let plan =
            build_launch_plan(&settings, &module, &instance).expect("build missing Java plan");
        assert_eq!(PathBuf::from(&plan.executable_path), expected_java);
        assert!(!plan.executable_exists);
        assert!(!plan.ready_to_launch);
        assert!(plan.validation_issues.iter().any(|issue| {
            issue.code == "launch_executable_missing" && issue.severity == "error"
        }));
    }

    std::fs::create_dir_all(expected_java.parent().expect("bundled Java parent"))
        .expect("create bundled runtime");
    std::fs::write(&expected_java, []).expect("write bundled Java executable");
    let plan = build_launch_plan(&settings, &module, &instance).expect("build restored Java plan");
    assert_eq!(PathBuf::from(&plan.executable_path), expected_java);
    assert!(plan.executable_exists);
    assert!(plan.ready_to_launch);
    std::fs::remove_file(&expected_java).expect("remove bundled Java executable");

    for executable in ["java", "java.exe"] {
        module.process.as_mut().expect("process").executable = String::from(executable);
        let plan = build_launch_plan(&settings, &module, &instance).expect("build bare Java plan");
        assert_eq!(
            PathBuf::from(&plan.executable_path),
            runtime_root.join(executable)
        );
        assert!(plan.executable_exists);
        assert!(plan.ready_to_launch);
    }

    std::fs::remove_dir_all(&temp_root).expect("remove Java launch test root");
}

#[test]
fn build_launch_plan_falls_back_to_matching_executable_name() {
    let temp_root = unique_test_root();
    let games_root = temp_root.join("games");
    let install_root = games_root.join("demo");
    let executable_path = install_root
        .join("Binaries")
        .join("Win64")
        .join("server.exe");
    std::fs::create_dir_all(executable_path.parent().expect("exe parent"))
        .expect("create exe parent");
    std::fs::write(&executable_path, []).expect("write exe placeholder");

    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: temp_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: temp_root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: temp_root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let module = ModuleDetails {
        summary: ModuleSummary {
            id: String::from("demo"),
            name: String::from("Demo"),
            version: String::from("0.1.0"),
            description: None,
            steam_app_id: Some(1),
            install_state: app_core::InstallState::NotInstalled,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
        schema_json: None,
        default_ports: vec![],
        install: Some(InstallSpec {
            shared_game_dir: String::from("demo"),
            download_url_windows: None,
            download_integrity_windows: None,
            source: None,
            verification_path: None,
            minecraft: None,
        }),
        process: Some(ProcessSpec {
            environment_template: Default::default(),
            executable: String::from("server.exe"),
            args_template: vec![],
            working_directory_template: None,
            window_policy: ProcessWindowPolicy::Background,
            host_surface: ProcessHostSurface::ManagedTerminal,
            host_notes: None,
        }),
        workshop: None,
        mods: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
    };
    let instance = InstanceDetails {
        summary: InstanceSummary {
            id: String::from("demo-1"),
            name: String::from("Demo"),
            module_id: String::from("demo"),
            status: InstanceStatus::Stopped,
            bind_ip: String::from("0.0.0.0"),
            port_count: 0,
            autostart: false,
            active_process_count: 0,
        },
        config_file_path: String::from("D:/LanGame/instances/demo-1/config/instance.json"),
        saves_path: String::from("D:/LanGame/instances/saves"),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: false,
        backup_retention_count: 10,
        settings_json: String::from("{}"),
        ports: vec![],
        active_run: None,
    };

    let launch_plan = build_launch_plan(&settings, &module, &instance).expect("build plan");
    assert!(launch_plan.executable_exists);
    assert!(
        launch_plan
            .executable_path
            .ends_with("Binaries\\Win64\\server.exe")
            || launch_plan
                .executable_path
                .ends_with("Binaries/Win64/server.exe")
    );

    let _ = std::fs::remove_dir_all(temp_root);
}

#[test]
fn build_launch_plan_ignores_steamapps_downloading_executable_fallback() {
    let temp_root = unique_test_root();
    let games_root = temp_root.join("games");
    let install_root = games_root.join("corekeeper");
    let downloading_executable = install_root
        .join("steamapps")
        .join("downloading")
        .join("1963720")
        .join("CoreKeeperServer.exe");
    std::fs::create_dir_all(downloading_executable.parent().expect("downloading parent"))
        .expect("create downloading parent");
    std::fs::write(&downloading_executable, []).expect("write downloading executable");

    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: temp_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: temp_root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: temp_root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let module = ModuleDetails {
        summary: ModuleSummary {
            id: String::from("corekeeper"),
            name: String::from("Core Keeper"),
            version: String::from("0.1.0"),
            description: None,
            steam_app_id: Some(1_963_720),
            install_state: app_core::InstallState::NotInstalled,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
        schema_json: None,
        default_ports: vec![],
        install: Some(InstallSpec {
            shared_game_dir: String::from("corekeeper"),
            download_url_windows: None,
            download_integrity_windows: None,
            source: None,
            verification_path: None,
            minecraft: None,
        }),
        process: Some(ProcessSpec {
            environment_template: Default::default(),
            executable: String::from("CoreKeeperServer.exe"),
            args_template: vec![],
            working_directory_template: None,
            window_policy: ProcessWindowPolicy::Background,
            host_surface: ProcessHostSurface::ManagedTerminal,
            host_notes: None,
        }),
        workshop: None,
        mods: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
    };
    let instance = InstanceDetails {
        summary: InstanceSummary {
            id: String::from("corekeeper-1"),
            name: String::from("Core Keeper"),
            module_id: String::from("corekeeper"),
            status: InstanceStatus::Stopped,
            bind_ip: String::from("0.0.0.0"),
            port_count: 0,
            autostart: false,
            active_process_count: 0,
        },
        config_file_path: temp_root
            .join("instances")
            .join("corekeeper-1")
            .join("config")
            .join("instance.json")
            .to_string_lossy()
            .into_owned(),
        saves_path: temp_root
            .join("instances")
            .join("corekeeper-1")
            .join("saves")
            .to_string_lossy()
            .into_owned(),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: false,
        backup_retention_count: 10,
        settings_json: String::from("{}"),
        ports: vec![],
        active_run: None,
    };

    let launch_plan = build_launch_plan(&settings, &module, &instance).expect("build plan");

    assert!(!launch_plan.executable_exists);
    assert_eq!(
        PathBuf::from(&launch_plan.executable_path),
        install_root.join("CoreKeeperServer.exe")
    );

    let _ = std::fs::remove_dir_all(temp_root);
}

#[test]
fn corekeeper_direct_platform_token_uses_official_platform_names() {
    let temp_root = unique_test_root();
    let instance = InstanceDetails {
        summary: InstanceSummary {
            id: String::from("corekeeper-1"),
            name: String::from("Core Keeper"),
            module_id: String::from("corekeeper"),
            status: InstanceStatus::Stopped,
            bind_ip: String::from("0.0.0.0"),
            port_count: 0,
            autostart: false,
            active_process_count: 0,
        },
        config_file_path: temp_root
            .join("instances")
            .join("corekeeper-1")
            .join("config")
            .join("instance.json")
            .to_string_lossy()
            .into_owned(),
        saves_path: temp_root
            .join("instances")
            .join("corekeeper-1")
            .join("saves")
            .to_string_lossy()
            .into_owned(),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: false,
        backup_retention_count: 10,
        settings_json: String::from("{}"),
        ports: vec![],
        active_run: None,
    };
    let install_root = temp_root.join("games").join("corekeeper");
    let config_dir = temp_root
        .join("instances")
        .join("corekeeper-1")
        .join("config");
    let data_dir = temp_root
        .join("instances")
        .join("corekeeper-1")
        .join("data");
    let logs_dir = temp_root
        .join("instances")
        .join("corekeeper-1")
        .join("logs");
    let saves_dir = temp_root
        .join("instances")
        .join("corekeeper-1")
        .join("saves");

    let epic_settings = serde_json::json!({
        "direct_connection_enabled": true,
        "allowed_platform_code": 2
    });
    let epic_context = TemplateContext {
        instance: &instance,
        settings: &epic_settings,
        install_root: &install_root,
        config_dir: &config_dir,
        data_dir: &data_dir,
        logs_dir: &logs_dir,
        saves_dir: &saves_dir,
    };

    assert_eq!(
        lookup_corekeeper_launch_token(&epic_context, "direct_allowed_platform_flag").as_deref(),
        Some("-allowonlyplatform")
    );
    assert_eq!(
        lookup_corekeeper_launch_token(&epic_context, "direct_allowed_platform_value").as_deref(),
        Some("Epic")
    );

    let all_platform_settings = serde_json::json!({
        "direct_connection_enabled": true,
        "allowed_platform_code": 0
    });
    let all_platform_context = TemplateContext {
        instance: &instance,
        settings: &all_platform_settings,
        install_root: &install_root,
        config_dir: &config_dir,
        data_dir: &data_dir,
        logs_dir: &logs_dir,
        saves_dir: &saves_dir,
    };

    assert_eq!(
        lookup_corekeeper_launch_token(&all_platform_context, "direct_allowed_platform_flag")
            .as_deref(),
        Some("")
    );
    assert_eq!(
        lookup_corekeeper_launch_token(&all_platform_context, "direct_allowed_platform_value")
            .as_deref(),
        Some("")
    );

    let _ = std::fs::remove_dir_all(temp_root);
}

#[test]
fn corekeeper_direct_tokens_default_to_enabled_when_missing_in_settings() {
    let temp_root = unique_test_root();
    let install_root = temp_root.join("games").join("corekeeper");
    let config_dir = temp_root
        .join("instances")
        .join("corekeeper-1")
        .join("config");
    let data_dir = temp_root
        .join("instances")
        .join("corekeeper-1")
        .join("data");
    let logs_dir = temp_root
        .join("instances")
        .join("corekeeper-1")
        .join("logs");
    let saves_dir = temp_root
        .join("instances")
        .join("corekeeper-1")
        .join("saves");
    let instance = InstanceDetails {
        summary: InstanceSummary {
            id: String::from("corekeeper-1"),
            name: String::from("Core Keeper"),
            module_id: String::from("corekeeper"),
            status: InstanceStatus::Stopped,
            bind_ip: String::from("0.0.0.0"),
            port_count: 0,
            autostart: false,
            active_process_count: 0,
        },
        config_file_path: temp_root
            .join("instances")
            .join("corekeeper-1")
            .join("config")
            .join("instance.json")
            .to_string_lossy()
            .into_owned(),
        saves_path: saves_dir.to_string_lossy().into_owned(),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: false,
        backup_retention_count: 10,
        settings_json: String::from("{}"),
        ports: vec![],
        active_run: None,
    };
    let settings = serde_json::json!({});
    let context = TemplateContext {
        instance: &instance,
        settings: &settings,
        install_root: &install_root,
        config_dir: &config_dir,
        data_dir: &data_dir,
        logs_dir: &logs_dir,
        saves_dir: &saves_dir,
    };

    assert_eq!(
        lookup_corekeeper_launch_token(&context, "direct_ip_flag").as_deref(),
        Some("-ip")
    );
    assert_eq!(
        lookup_corekeeper_launch_token(&context, "direct_port_flag").as_deref(),
        Some("-port")
    );
    assert_eq!(
        lookup_corekeeper_launch_token(&context, "direct_password_flag").as_deref(),
        Some("-password")
    );
    assert_eq!(
        lookup_corekeeper_launch_token(&context, "direct_allowed_platform_flag").as_deref(),
        Some("")
    );
    assert_eq!(
        lookup_corekeeper_launch_token(&context, "direct_allowed_platform_value").as_deref(),
        Some("")
    );

    let _ = std::fs::remove_dir_all(temp_root);
}

#[test]
fn abioticfactor_multihome_uses_only_instance_bind_and_keeps_use_local_ips_independent() {
    let temp_root = unique_test_root();
    let install_root = temp_root.join("games").join("abioticfactor");
    let config_dir = temp_root
        .join("instances")
        .join("abioticfactor-1")
        .join("config");
    let data_dir = temp_root
        .join("instances")
        .join("abioticfactor-1")
        .join("data");
    let logs_dir = temp_root
        .join("instances")
        .join("abioticfactor-1")
        .join("logs");
    let saves_dir = temp_root
        .join("instances")
        .join("abioticfactor-1")
        .join("saves");
    let mut instance = InstanceDetails {
        summary: InstanceSummary {
            id: String::from("abioticfactor-1"),
            name: String::from("Abiotic Factor"),
            module_id: String::from("abioticfactor"),
            status: InstanceStatus::Stopped,
            bind_ip: String::from("192.168.31.42"),
            port_count: 0,
            autostart: false,
            active_process_count: 0,
        },
        config_file_path: config_dir
            .join("instance.json")
            .to_string_lossy()
            .into_owned(),
        saves_path: saves_dir.to_string_lossy().into_owned(),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: false,
        backup_retention_count: 10,
        settings_json: String::from("{}"),
        ports: vec![],
        active_run: None,
    };
    let settings = serde_json::json!({
        "multihome_address": "10.66.0.99",
        "use_local_ips": true
    });

    {
        let context = TemplateContext {
            instance: &instance,
            settings: &settings,
            install_root: &install_root,
            config_dir: &config_dir,
            data_dir: &data_dir,
            logs_dir: &logs_dir,
            saves_dir: &saves_dir,
        };
        let multihome = lookup_abioticfactor_launch_token(&context, "multihome_flag")
            .expect("known Abiotic Factor token");
        assert_eq!(multihome, "-MultiHome=192.168.31.42");
        assert_eq!(multihome.matches("-MultiHome=").count(), 1);
        assert_eq!(
            lookup_abioticfactor_launch_token(&context, "use_local_ips_flag").as_deref(),
            Some("-UseLocalIPs")
        );
    }

    instance.summary.bind_ip = String::from("0.0.0.0");
    let wildcard_context = TemplateContext {
        instance: &instance,
        settings: &settings,
        install_root: &install_root,
        config_dir: &config_dir,
        data_dir: &data_dir,
        logs_dir: &logs_dir,
        saves_dir: &saves_dir,
    };
    assert_eq!(
        lookup_abioticfactor_launch_token(&wildcard_context, "multihome_flag").as_deref(),
        Some("")
    );
    assert_eq!(
        lookup_abioticfactor_launch_token(&wildcard_context, "use_local_ips_flag").as_deref(),
        Some("-UseLocalIPs")
    );

    let _ = std::fs::remove_dir_all(temp_root);
}

#[test]
fn build_launch_plan_prefers_platform_specific_executable_directory() {
    let temp_root = unique_test_root();
    let games_root = temp_root.join("games");
    let install_root = games_root.join("terraria");
    let linux_executable = install_root
        .join("1458")
        .join("Linux")
        .join("TerrariaServer.exe");
    let windows_executable = install_root
        .join("1458")
        .join("Windows")
        .join("TerrariaServer.exe");
    std::fs::create_dir_all(linux_executable.parent().expect("linux parent"))
        .expect("create linux parent");
    std::fs::create_dir_all(windows_executable.parent().expect("windows parent"))
        .expect("create windows parent");
    std::fs::write(&linux_executable, []).expect("write linux placeholder");
    std::fs::write(&windows_executable, []).expect("write windows placeholder");

    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: temp_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: temp_root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: temp_root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let module = ModuleDetails {
        summary: ModuleSummary {
            id: String::from("terraria"),
            name: String::from("Terraria"),
            version: String::from("0.1.0"),
            description: None,
            steam_app_id: Some(105600),
            install_state: app_core::InstallState::NotInstalled,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
        schema_json: None,
        default_ports: vec![],
        install: Some(InstallSpec {
            shared_game_dir: String::from("terraria"),
            download_url_windows: None,
            download_integrity_windows: None,
            source: None,
            verification_path: None,
            minecraft: None,
        }),
        process: Some(ProcessSpec {
            environment_template: Default::default(),
            executable: String::from("TerrariaServer.exe"),
            args_template: vec![
                String::from("-config"),
                String::from("{{paths.config_dir}}/serverconfig.txt"),
                String::from("-ip"),
                String::from("{{instance.bind_ip}}"),
            ],
            working_directory_template: None,
            window_policy: ProcessWindowPolicy::Background,
            host_surface: ProcessHostSurface::ManagedTerminal,
            host_notes: None,
        }),
        workshop: None,
        mods: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
    };
    let instance = InstanceDetails {
        summary: InstanceSummary {
            id: String::from("terraria-1"),
            name: String::from("Terraria Smoke"),
            module_id: String::from("terraria"),
            status: InstanceStatus::Stopped,
            bind_ip: String::from("0.0.0.0"),
            port_count: 0,
            autostart: false,
            active_process_count: 0,
        },
        config_file_path: temp_root
            .join("instances")
            .join("terraria-1")
            .join("config")
            .join("instance.json")
            .to_string_lossy()
            .into_owned(),
        saves_path: String::from("D:/LanGame/instances/saves"),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: false,
        backup_retention_count: 10,
        settings_json: String::from("{}"),
        ports: vec![],
        active_run: None,
    };

    let launch_plan = build_launch_plan(&settings, &module, &instance).expect("build plan");
    assert!(launch_plan.executable_exists);

    #[cfg(windows)]
    {
        let expected_suffix = if xna_framework_is_installed() {
            r"\Windows\TerrariaServer.exe"
        } else {
            r"\Linux\TerrariaServer.exe"
        };
        assert!(
            launch_plan.executable_path.ends_with(expected_suffix)
                || launch_plan
                    .executable_path
                    .ends_with(&expected_suffix.replace('\\', "/")),
            "launch plan chose the wrong executable: {}",
            launch_plan.executable_path
        );
    }

    #[cfg(not(windows))]
    assert!(
        launch_plan
            .executable_path
            .ends_with(r"\Linux\TerrariaServer.exe")
            || launch_plan
                .executable_path
                .ends_with("/Linux/TerrariaServer.exe"),
        "launch plan chose the wrong executable: {}",
        launch_plan.executable_path
    );

    assert_eq!(launch_plan.args[0], "-config");
    assert_eq!(
        PathBuf::from(&launch_plan.args[1]),
        temp_root
            .join("instances")
            .join("terraria-1")
            .join("config")
            .join("serverconfig.txt")
    );
    assert_eq!(launch_plan.args[2], "-ip");
    assert_eq!(launch_plan.args[3], "0.0.0.0");
    assert_eq!(launch_plan.window_policy, ProcessWindowPolicy::Background);
    assert!(!launch_plan.uses_script_entrypoint);
    let _ = std::fs::remove_dir_all(temp_root);
}

#[test]
fn terraria_tmodloader_runtime_uses_instance_scoped_entrypoint_and_save_directory() {
    let temp_root = unique_test_root();
    let games_root = temp_root.join("games");
    let install_root = games_root.join("terraria");
    let config_path = temp_root
        .join("instances")
        .join("terraria-tmod")
        .join("config")
        .join("instance.json");
    let instance_root = config_path
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let runtime_dir = instance_root.join("tmodloader").join("runtime");
    let runtime_entrypoint = runtime_dir.join("start-tModLoaderServer.bat");

    std::fs::create_dir_all(&runtime_dir).expect("create tmodloader runtime dir");
    std::fs::create_dir_all(config_path.parent().expect("config parent"))
        .expect("create config parent");
    std::fs::create_dir_all(&install_root).expect("create vanilla install root");
    std::fs::write(&runtime_entrypoint, "@echo off").expect("write tmodloader entrypoint");

    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: temp_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: temp_root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: temp_root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let module = ModuleDetails {
        summary: ModuleSummary {
            id: String::from("terraria"),
            name: String::from("Terraria"),
            version: String::from("0.1.0"),
            description: None,
            steam_app_id: Some(105600),
            install_state: app_core::InstallState::NotInstalled,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
        schema_json: None,
        default_ports: vec![],
        install: Some(InstallSpec {
            shared_game_dir: String::from("terraria"),
            download_url_windows: None,
            download_integrity_windows: None,
            source: None,
            verification_path: None,
            minecraft: None,
        }),
        process: Some(ProcessSpec {
            environment_template: Default::default(),
            executable: String::from("{{terraria.server_executable}}"),
            args_template: vec![String::from("{{terraria.launch_args}}")],
            working_directory_template: Some(String::from("{{terraria.working_directory}}")),
            window_policy: ProcessWindowPolicy::Background,
            host_surface: ProcessHostSurface::ManagedTerminal,
            host_notes: None,
        }),
        workshop: None,
        mods: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
    };
    let instance = InstanceDetails {
        summary: InstanceSummary {
            id: String::from("terraria-tmod"),
            name: String::from("Terraria tModLoader"),
            module_id: String::from("terraria"),
            status: InstanceStatus::Stopped,
            bind_ip: String::from("0.0.0.0"),
            port_count: 0,
            autostart: false,
            active_process_count: 0,
        },
        config_file_path: config_path.to_string_lossy().into_owned(),
        saves_path: instance_root.join("saves").to_string_lossy().into_owned(),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: false,
        backup_retention_count: 10,
        settings_json: String::from(r#"{"server_runtime":"tmodloader"}"#),
        ports: vec![],
        active_run: None,
    };

    let launch_plan = build_launch_plan(&settings, &module, &instance).expect("build plan");

    assert_eq!(
        PathBuf::from(&launch_plan.executable_path),
        runtime_entrypoint
    );
    assert!(launch_plan.executable_exists);
    assert_eq!(PathBuf::from(&launch_plan.working_directory), runtime_dir);
    assert_eq!(launch_plan.args[0], "-nosteam");
    assert_eq!(launch_plan.args[1], "-config");
    assert_eq!(
        PathBuf::from(&launch_plan.args[2]),
        instance_root.join("config").join("serverconfig.txt")
    );
    assert_eq!(launch_plan.args[3], "-tmlsavedirectory");
    assert_eq!(
        PathBuf::from(&launch_plan.args[4]),
        instance_root.join("tmodloader")
    );
    assert_eq!(launch_plan.args[5], "-ip");
    assert_eq!(launch_plan.args[6], "0.0.0.0");
    assert!(launch_plan.uses_script_entrypoint);

    let _ = std::fs::remove_dir_all(temp_root);
}

#[test]
fn build_launch_plan_resolves_working_directory_template() {
    let temp_root = unique_test_root();
    let games_root = temp_root.join("games");
    let install_root = games_root.join("enshrouded");
    let executable_path = install_root.join("enshrouded_server.exe");
    let config_path = temp_root
        .join("instances")
        .join("enshrouded-1")
        .join("config")
        .join("instance.json");
    std::fs::create_dir_all(executable_path.parent().expect("exe parent"))
        .expect("create exe parent");
    std::fs::create_dir_all(config_path.parent().expect("config parent"))
        .expect("create config parent");
    std::fs::write(&executable_path, []).expect("write exe placeholder");

    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: temp_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: temp_root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: temp_root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let module = ModuleDetails {
        summary: ModuleSummary {
            id: String::from("enshrouded"),
            name: String::from("Enshrouded"),
            version: String::from("0.1.0"),
            description: None,
            steam_app_id: Some(2278520),
            install_state: app_core::InstallState::NotInstalled,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
        schema_json: None,
        default_ports: vec![],
        install: Some(InstallSpec {
            shared_game_dir: String::from("enshrouded"),
            download_url_windows: None,
            download_integrity_windows: None,
            source: None,
            verification_path: None,
            minecraft: None,
        }),
        process: Some(ProcessSpec {
            environment_template: Default::default(),
            executable: String::from("enshrouded_server.exe"),
            args_template: vec![],
            working_directory_template: Some(String::from("{{paths.config_dir}}")),
            window_policy: ProcessWindowPolicy::Background,
            host_surface: ProcessHostSurface::ManagedTerminal,
            host_notes: None,
        }),
        workshop: None,
        mods: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
    };
    let instance = InstanceDetails {
        summary: InstanceSummary {
            id: String::from("enshrouded-1"),
            name: String::from("Enshrouded Alpha"),
            module_id: String::from("enshrouded"),
            status: InstanceStatus::Stopped,
            bind_ip: String::from("0.0.0.0"),
            port_count: 0,
            autostart: false,
            active_process_count: 0,
        },
        config_file_path: config_path.to_string_lossy().into_owned(),
        saves_path: String::from("D:/LanGame/instances/saves"),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: false,
        backup_retention_count: 10,
        settings_json: String::from("{}"),
        ports: vec![],
        active_run: None,
    };

    let launch_plan = build_launch_plan(&settings, &module, &instance).expect("build plan");
    assert_eq!(
        launch_plan.working_directory,
        config_path
            .parent()
            .expect("config dir")
            .to_string_lossy()
            .into_owned()
    );

    let _ = std::fs::remove_dir_all(temp_root);
}

#[test]
fn build_launch_plan_filters_empty_palworld_args_and_launches_direct_binary() {
    let temp_root = unique_test_root();
    let games_root = temp_root.join("games");
    let install_root = games_root.join("palworld");
    let executable_path = install_root
        .join("Pal")
        .join("Binaries")
        .join("Win64")
        .join("PalServer-Win64-Shipping-Cmd.exe");
    let config_path = temp_root
        .join("instances")
        .join("palworld-1")
        .join("config")
        .join("instance.json");
    std::fs::create_dir_all(executable_path.parent().expect("exe parent"))
        .expect("create exe parent");
    std::fs::create_dir_all(config_path.parent().expect("config parent"))
        .expect("create config parent");
    std::fs::write(&executable_path, []).expect("write exe placeholder");

    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: temp_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: temp_root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: temp_root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let module = ModuleDetails {
        summary: ModuleSummary {
            id: String::from("palworld"),
            name: String::from("Palworld"),
            version: String::from("0.1.0"),
            description: None,
            steam_app_id: Some(2394010),
            install_state: app_core::InstallState::NotInstalled,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
        schema_json: None,
        default_ports: vec![],
        install: Some(InstallSpec {
            shared_game_dir: String::from("palworld"),
            download_url_windows: None,
            download_integrity_windows: None,
            source: None,
            verification_path: None,
            minecraft: None,
        }),
        process: Some(ProcessSpec {
            environment_template: Default::default(),
            executable: String::from("Pal/Binaries/Win64/PalServer-Win64-Shipping-Cmd.exe"),
            args_template: vec![
                String::from("-port={{ports.game.port}}"),
                String::from("-players={{settings.max_players}}"),
                String::from("-logformat={{settings.log_format}}"),
                String::from("{{palworld.public_lobby_flag}}"),
                String::from("{{palworld.public_ip_flag}}"),
                String::from("{{palworld.public_port_flag}}"),
                String::from("{{palworld.use_perf_threads_flag}}"),
                String::from("{{palworld.no_async_loading_thread_flag}}"),
                String::from("{{palworld.use_multithread_for_ds_flag}}"),
                String::from("{{palworld.worker_thread_count_flag}}"),
                String::from("{{palworld.gamedata_api_flag}}"),
            ],
            working_directory_template: Some(String::from(
                "{{paths.install_root}}/Pal/Binaries/Win64",
            )),
            window_policy: ProcessWindowPolicy::Background,
            host_surface: ProcessHostSurface::ManagedTerminal,
            host_notes: None,
        }),
        workshop: None,
        mods: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
    };
    let instance = InstanceDetails {
        summary: InstanceSummary {
            id: String::from("palworld-1"),
            name: String::from("Palworld Smoke"),
            module_id: String::from("palworld"),
            status: InstanceStatus::Stopped,
            bind_ip: String::from("0.0.0.0"),
            port_count: 0,
            autostart: false,
            active_process_count: 0,
        },
        config_file_path: config_path.to_string_lossy().into_owned(),
        saves_path: String::from("D:/LanGame/instances/saves"),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: false,
        backup_retention_count: 10,
        settings_json: String::from(
            r#"{"max_players":32,"log_format":"text","community_server":false,"public_ip":"","public_port":8211,"launch_perf_threads":true,"launch_worker_threads_enabled":false,"worker_thread_count":8}"#,
        ),
        ports: vec![
            PortBinding {
                name: String::from("game"),
                protocol: String::from("udp"),
                port: 8211,
            },
            PortBinding {
                name: String::from("rcon"),
                protocol: String::from("tcp"),
                port: 25575,
            },
        ],
        active_run: None,
    };

    let launch_plan = build_launch_plan(&settings, &module, &instance).expect("build plan");
    assert_eq!(PathBuf::from(&launch_plan.executable_path), executable_path);
    assert_eq!(
        PathBuf::from(&launch_plan.working_directory),
        install_root.join("Pal").join("Binaries").join("Win64")
    );
    assert_eq!(
        launch_plan.args,
        vec![
            String::from("-port=8211"),
            String::from("-players=32"),
            String::from("-logformat=text"),
            String::from("-useperfthreads"),
            String::from("-NoAsyncLoadingThread"),
            String::from("-UseMultithreadForDS"),
        ]
    );
    assert!(!launch_plan.uses_script_entrypoint);

    let mut api_instance = instance.clone();
    let mut api_settings: serde_json::Value =
        serde_json::from_str(&api_instance.settings_json).expect("parse Palworld settings");
    api_settings["gamedata_api_enabled"] = serde_json::json!(true);
    api_instance.settings_json = api_settings.to_string();
    let api_plan = build_launch_plan(&settings, &module, &api_instance).expect("build API plan");
    assert_eq!(
        api_plan
            .args
            .iter()
            .filter(|argument| *argument == "-enable-gamedata-api")
            .count(),
        1
    );

    let _ = std::fs::remove_dir_all(temp_root);
}

#[test]
fn palworld_worker_count_requires_multithread_flags() {
    let settings = serde_json::json!({
        "launch_perf_threads": false,
        "launch_worker_threads_enabled": true,
        "worker_thread_count": 12
    });

    assert_eq!(
        crate::launch_templates::render_palworld_worker_thread_flag(&settings),
        ""
    );

    let settings = serde_json::json!({
        "launch_perf_threads": true,
        "launch_worker_threads_enabled": true,
        "worker_thread_count": 12
    });
    assert_eq!(
        crate::launch_templates::render_palworld_worker_thread_flag(&settings),
        "-NumberOfWorkerThreadsServer=12"
    );
}

#[test]
fn build_launch_plan_resolves_valheim_crossplay_flag() {
    let temp_root = unique_test_root();
    let games_root = temp_root.join("games");
    let install_root = games_root.join("valheim");
    let executable_path = install_root.join("valheim_server.exe");
    let config_path = temp_root
        .join("instances")
        .join("valheim-1")
        .join("config")
        .join("instance.json");
    std::fs::create_dir_all(executable_path.parent().expect("exe parent"))
        .expect("create exe parent");
    std::fs::create_dir_all(config_path.parent().expect("config parent"))
        .expect("create config parent");
    std::fs::write(&executable_path, []).expect("write exe placeholder");

    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: temp_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: temp_root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: temp_root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let module = ModuleDetails {
        summary: ModuleSummary {
            id: String::from("valheim"),
            name: String::from("Valheim"),
            version: String::from("0.1.0"),
            description: None,
            steam_app_id: Some(896660),
            install_state: app_core::InstallState::NotInstalled,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
        schema_json: None,
        default_ports: vec![],
        install: Some(InstallSpec {
            shared_game_dir: String::from("valheim"),
            download_url_windows: None,
            download_integrity_windows: None,
            source: None,
            verification_path: None,
            minecraft: None,
        }),
        process: Some(ProcessSpec {
            environment_template: Default::default(),
            executable: String::from("valheim_server.exe"),
            args_template: vec![
                String::from("-nographics"),
                String::from("-batchmode"),
                String::from("-name"),
                String::from("{{settings.server_name}}"),
                String::from("-port"),
                String::from("{{ports.game.port}}"),
                String::from("-world"),
                String::from("{{settings.world_name}}"),
                String::from("-password"),
                String::from("{{settings.server_password}}"),
                String::from("-savedir"),
                String::from("{{paths.saves_dir}}"),
                String::from("-public"),
                String::from("{{settings.public_server}}"),
                String::from("{{valheim.crossplay_flag}}"),
            ],
            working_directory_template: Some(String::from("{{paths.install_root}}")),
            window_policy: ProcessWindowPolicy::Background,
            host_surface: ProcessHostSurface::ManagedTerminal,
            host_notes: None,
        }),
        workshop: None,
        mods: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
    };
    let instance = InstanceDetails {
        summary: InstanceSummary {
            id: String::from("valheim-1"),
            name: String::from("Valheim Ridge"),
            module_id: String::from("valheim"),
            status: InstanceStatus::Stopped,
            bind_ip: String::from("0.0.0.0"),
            port_count: 0,
            autostart: false,
            active_process_count: 0,
        },
        config_file_path: config_path.to_string_lossy().into_owned(),
        saves_path: temp_root
            .join("instances")
            .join("valheim-1")
            .join("saves")
            .to_string_lossy()
            .into_owned(),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: false,
        backup_retention_count: 10,
        settings_json: String::from(
            r#"{"server_name":"Valheim Ridge","world_name":"Valheim Ridge","server_password":"change-me","public_server":1,"crossplay_enabled":true}"#,
        ),
        ports: vec![PortBinding {
            name: String::from("game"),
            protocol: String::from("udp"),
            port: 2456,
        }],
        active_run: None,
    };

    let launch_plan = build_launch_plan(&settings, &module, &instance).expect("build plan");
    assert_eq!(PathBuf::from(&launch_plan.executable_path), executable_path);
    assert_eq!(PathBuf::from(&launch_plan.working_directory), install_root);
    assert_eq!(
        launch_plan.args,
        vec![
            String::from("-nographics"),
            String::from("-batchmode"),
            String::from("-name"),
            String::from("Valheim Ridge"),
            String::from("-port"),
            String::from("2456"),
            String::from("-world"),
            String::from("Valheim Ridge"),
            String::from("-password"),
            String::from("change-me"),
            String::from("-savedir"),
            temp_root
                .join("instances")
                .join("valheim-1")
                .join("saves")
                .to_string_lossy()
                .into_owned(),
            String::from("-public"),
            String::from("1"),
            String::from("-crossplay"),
        ]
    );

    let _ = std::fs::remove_dir_all(temp_root);
}

#[test]
fn build_launch_plan_expands_valheim_world_tuning_tokens() {
    let temp_root = unique_test_root();
    let games_root = temp_root.join("games");
    let install_root = games_root.join("valheim");
    let executable_path = install_root.join("valheim_server.exe");
    let config_path = temp_root
        .join("instances")
        .join("valheim-tuning")
        .join("config")
        .join("instance.json");
    std::fs::create_dir_all(executable_path.parent().expect("exe parent"))
        .expect("create exe parent");
    std::fs::create_dir_all(config_path.parent().expect("config parent"))
        .expect("create config parent");
    std::fs::write(&executable_path, []).expect("write exe placeholder");

    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: temp_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: temp_root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: temp_root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let module = ModuleDetails {
        summary: ModuleSummary {
            id: String::from("valheim"),
            name: String::from("Valheim"),
            version: String::from("0.1.0"),
            description: None,
            steam_app_id: Some(896660),
            install_state: app_core::InstallState::NotInstalled,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
        schema_json: None,
        default_ports: vec![],
        install: Some(InstallSpec {
            shared_game_dir: String::from("valheim"),
            download_url_windows: None,
            download_integrity_windows: None,
            source: None,
            verification_path: None,
            minecraft: None,
        }),
        process: Some(ProcessSpec {
            environment_template: Default::default(),
            executable: String::from("valheim_server.exe"),
            args_template: vec![
                String::from("-name"),
                String::from("{{settings.server_name}}"),
                String::from("{{valheim.world_preset_args}}"),
                String::from("{{valheim.world_modifier_args}}"),
                String::from("{{valheim.world_setkey_args}}"),
                String::from("{{valheim.crossplay_flag}}"),
                String::from("{{valheim.custom_launch_flags}}"),
            ],
            working_directory_template: Some(String::from("{{paths.install_root}}")),
            window_policy: ProcessWindowPolicy::Background,
            host_surface: ProcessHostSurface::ManagedTerminal,
            host_notes: None,
        }),
        workshop: None,
        mods: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
    };
    let instance = InstanceDetails {
        summary: InstanceSummary {
            id: String::from("valheim-tuning"),
            name: String::from("Valheim Tuning"),
            module_id: String::from("valheim"),
            status: InstanceStatus::Stopped,
            bind_ip: String::from("0.0.0.0"),
            port_count: 0,
            autostart: false,
            active_process_count: 0,
        },
        config_file_path: config_path.to_string_lossy().into_owned(),
        saves_path: temp_root
            .join("instances")
            .join("valheim-tuning")
            .join("saves")
            .to_string_lossy()
            .into_owned(),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: false,
        backup_retention_count: 10,
        settings_json: String::from(
            r#"{"server_name":"Valheim Tuning","world_preset":"normal","world_modifiers":"combat hard\ndeathpenalty hard","world_set_keys":"playerevents","crossplay_enabled":false,"custom_launch_flags":"-publictest -logFile \"D:\\Logs\\valheim.log\""}"#,
        ),
        ports: vec![],
        active_run: None,
    };

    let launch_plan = build_launch_plan(&settings, &module, &instance).expect("build plan");
    assert_eq!(
        launch_plan.args,
        vec![
            String::from("-name"),
            String::from("Valheim Tuning"),
            String::from("-preset"),
            String::from("normal"),
            String::from("-modifier"),
            String::from("combat"),
            String::from("hard"),
            String::from("-modifier"),
            String::from("deathpenalty"),
            String::from("hard"),
            String::from("-setkey"),
            String::from("playerevents"),
            String::from("-publictest"),
            String::from("-logFile"),
            String::from("D:\\Logs\\valheim.log"),
        ]
    );

    let _ = std::fs::remove_dir_all(temp_root);
}

#[test]
fn build_launch_plan_expands_unturned_advanced_launch_tokens() {
    let temp_root = unique_test_root();
    let games_root = temp_root.join("games");
    let install_root = games_root.join("unturned");
    let executable_path = install_root.join("Unturned.exe");
    let config_path = temp_root
        .join("instances")
        .join("unturned-1")
        .join("config")
        .join("instance.json");
    std::fs::create_dir_all(executable_path.parent().expect("exe parent"))
        .expect("create exe parent");
    std::fs::create_dir_all(config_path.parent().expect("config parent"))
        .expect("create config parent");
    std::fs::write(&executable_path, []).expect("write exe placeholder");

    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: temp_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: temp_root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: temp_root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let module = ModuleDetails {
        summary: ModuleSummary {
            id: String::from("unturned"),
            name: String::from("Unturned"),
            version: String::from("0.1.0"),
            description: None,
            steam_app_id: Some(1110390),
            install_state: app_core::InstallState::NotInstalled,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
        schema_json: None,
        default_ports: vec![],
        install: Some(InstallSpec {
            shared_game_dir: String::from("unturned"),
            download_url_windows: None,
            download_integrity_windows: None,
            source: None,
            verification_path: None,
            minecraft: None,
        }),
        process: Some(ProcessSpec {
            environment_template: Default::default(),
            executable: String::from("Unturned.exe"),
            args_template: vec![
                String::from("{{unturned.server_launch_mode}}"),
                String::from("{{unturned.no_level_config_overrides_flag}}"),
                String::from("{{unturned.log_gameplay_config_flag}}"),
                String::from("{{unturned.gameplay_config_no_generated_comments_flag}}"),
                String::from("{{unturned.gameplay_config_no_empty_values_flag}}"),
                String::from("{{unturned.custom_launch_flags}}"),
            ],
            working_directory_template: Some(String::from("{{paths.install_root}}")),
            window_policy: ProcessWindowPolicy::Background,
            host_surface: ProcessHostSurface::ManagedTerminal,
            host_notes: None,
        }),
        workshop: None,
        mods: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
    };
    let instance = InstanceDetails {
        summary: InstanceSummary {
            id: String::from("unturned-1"),
            name: String::from("Unturned Ops"),
            module_id: String::from("unturned"),
            status: InstanceStatus::Stopped,
            bind_ip: String::from("0.0.0.0"),
            port_count: 0,
            autostart: false,
            active_process_count: 0,
        },
        config_file_path: config_path.to_string_lossy().into_owned(),
        saves_path: temp_root
            .join("instances")
            .join("unturned-1")
            .join("saves")
            .to_string_lossy()
            .into_owned(),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: false,
        backup_retention_count: 10,
        settings_json: String::from(
            r#"{"internet_server":false,"no_level_config_overrides":true,"log_gameplay_config":true,"gameplay_config_no_generated_comments":true,"gameplay_config_no_empty_values":true,"custom_launch_flags":"-NetTransport=SteamNetworkingSockets -SomeFlag=1"}"#,
        ),
        ports: vec![],
        active_run: None,
    };

    let launch_plan = build_launch_plan(&settings, &module, &instance).expect("build plan");
    assert_eq!(
        launch_plan.args,
        vec![
            String::from("+LanServer/unturned-1"),
            String::from("-NoLevelConfigOverrides"),
            String::from("-LogGameplayConfig"),
            String::from("-GameplayConfigNoGeneratedComments"),
            String::from("-GameplayConfigNoEmptyValues"),
            String::from("-NetTransport=SteamNetworkingSockets"),
            String::from("-SomeFlag=1"),
        ]
    );

    let _ = std::fs::remove_dir_all(temp_root);
}

#[test]
fn build_launch_plan_expands_satisfactory_custom_launch_flags() {
    let temp_root = unique_test_root();
    let games_root = temp_root.join("games");
    let install_root = games_root.join("satisfactory");
    let executable_path = install_root.join("FactoryServer.exe");
    let config_path = temp_root
        .join("instances")
        .join("satisfactory-1")
        .join("config")
        .join("instance.json");
    std::fs::create_dir_all(executable_path.parent().expect("exe parent"))
        .expect("create exe parent");
    std::fs::create_dir_all(config_path.parent().expect("config parent"))
        .expect("create config parent");
    std::fs::write(&executable_path, []).expect("write exe placeholder");

    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: temp_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: temp_root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: temp_root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let mut module = ModuleDetails {
        summary: ModuleSummary {
            id: String::from("satisfactory"),
            name: String::from("Satisfactory"),
            version: String::from("0.1.0"),
            description: None,
            steam_app_id: Some(1690800),
            install_state: app_core::InstallState::NotInstalled,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
        schema_json: None,
        default_ports: vec![],
        install: Some(InstallSpec {
            shared_game_dir: String::from("satisfactory"),
            download_url_windows: None,
            download_integrity_windows: None,
            source: None,
            verification_path: None,
            minecraft: None,
        }),
        process: Some(ProcessSpec {
            environment_template: BTreeMap::from([(
                "USERPROFILE".into(),
                "{{paths.data_dir}}/profile".into(),
            )]),
            executable: String::from("FactoryServer.exe"),
            args_template: vec![
                String::from("-Port={{ports.game.port}}"),
                String::from("{{satisfactory.insecure_local_api_flag}}"),
                String::from("{{satisfactory.custom_launch_flags}}"),
            ],
            working_directory_template: Some(String::from("{{paths.install_root}}")),
            window_policy: ProcessWindowPolicy::Background,
            host_surface: ProcessHostSurface::ManagedTerminal,
            host_notes: None,
        }),
        workshop: None,
        mods: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
    };
    let instance = InstanceDetails {
        summary: InstanceSummary {
            id: String::from("satisfactory-1"),
            name: String::from("Satisfactory Ops"),
            module_id: String::from("satisfactory"),
            status: InstanceStatus::Stopped,
            bind_ip: String::from("0.0.0.0"),
            port_count: 1,
            autostart: false,
            active_process_count: 0,
        },
        config_file_path: config_path.to_string_lossy().into_owned(),
        saves_path: temp_root
            .join("instances")
            .join("satisfactory-1")
            .join("saves")
            .to_string_lossy()
            .into_owned(),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: false,
        backup_retention_count: 10,
        settings_json: String::from(
            r#"{"allow_insecure_local_api":true,"custom_launch_flags":"-NoCrashDialog -NoVerifyGC"}"#,
        ),
        ports: vec![PortBinding {
            name: String::from("game"),
            protocol: String::from("udp"),
            port: 7777,
        }],
        active_run: None,
    };

    let launch_plan = build_launch_plan(&settings, &module, &instance).expect("build plan");
    assert_eq!(
        PathBuf::from(&launch_plan.environment["USERPROFILE"]),
        temp_root.join("instances/satisfactory-1/data/profile")
    );
    assert_eq!(
        launch_plan.args,
        vec![
            String::from("-Port=7777"),
            String::from(
                "-ini:Engine:[SystemSettings]:FG.DedicatedServer.AllowInsecureLocalAccess=1"
            ),
            String::from("-NoCrashDialog"),
            String::from("-NoVerifyGC"),
        ]
    );

    module
        .process
        .as_mut()
        .expect("process")
        .environment_template
        .insert("USERPROFILE".into(), "{{paths.unknown}}/profile".into());
    let invalid =
        build_launch_plan(&settings, &module, &instance).expect("invalid environment plan");
    assert!(!invalid.ready_to_launch);
    assert!(
        invalid
            .validation_issues
            .iter()
            .any(|issue| issue.code == "launch_environment_invalid")
    );

    let _ = std::fs::remove_dir_all(temp_root);
}

#[test]
fn raw_launch_arguments_cannot_override_satisfactory_managed_options() {
    let issues = collect_module_launch_setting_issues(
        "satisfactory",
        &serde_json::json!({
            "custom_launch_flags": "-NoCrashDialog -PORT=9999 -DisableSeasonalEvents"
        }),
        &[],
    );
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].code, "managed_launch_option_conflict");
    assert_eq!(issues[0].severity, "error");
    assert_eq!(issues[0].context["field"], "custom_launch_flags");

    assert!(
        collect_module_launch_setting_issues(
            "satisfactory",
            &serde_json::json!({ "custom_launch_flags": "-NoCrashDialog -NoVerifyGC" }),
            &[],
        )
        .is_empty()
    );
}

#[test]
fn rust_raw_launch_arguments_cannot_override_managed_runtime_authority() {
    for raw in [
        "+server.ip 127.0.0.1",
        "+server.identity another-instance",
        "+SERVER.PORT=29015",
        "+server.queryport 29017",
        "+rcon.port 29016",
        "+rcon.password exposed",
        "+world.configfile other.json",
        "-LOGFILE=other.log",
    ] {
        let issues = collect_module_launch_setting_issues(
            "rust",
            &serde_json::json!({ "custom_launch_flags": raw }),
            &[],
        );
        assert_eq!(issues.len(), 1, "{raw}");
        assert_eq!(issues[0].code, "managed_launch_option_conflict");
    }

    assert!(
        collect_module_launch_setting_issues(
            "rust",
            &serde_json::json!({ "custom_launch_flags": "+server.tickrate 30" }),
            &[],
        )
        .is_empty()
    );
}

#[test]
fn theforest_raw_launch_arguments_cannot_override_native_config_or_managed_paths() {
    for raw in [
        "-serverplayers 2",
        "-ALLOWCHEATS",
        "-configfilepath C:/other/server.cfg",
        "-servergameport=9999",
        "-nographics",
    ] {
        let issues = collect_module_launch_setting_issues(
            "theforest",
            &serde_json::json!({ "extra_launch_args": raw }),
            &[],
        );
        assert_eq!(issues.len(), 1, "{raw}");
        assert_eq!(issues[0].code, "managed_launch_option_conflict");
    }

    assert!(
        collect_module_launch_setting_issues(
            "theforest",
            &serde_json::json!({ "extra_launch_args": "-log -silent-crashes" }),
            &[],
        )
        .is_empty()
    );
}

#[test]
fn build_launch_plan_blocks_unresolved_arguments_for_an_installed_server() {
    let root = unique_test_root();
    let install_root = root.join("games/demo");
    let config_dir = root.join("instances/demo-1/config");
    std::fs::create_dir_all(&install_root).expect("create install directory");
    std::fs::create_dir_all(&config_dir).expect("create instance configuration directory");
    std::fs::write(install_root.join("server.exe"), []).expect("write executable fixture");
    let settings = AppSettings {
        games_root: root.join("games").to_string_lossy().into_owned(),
        ..AppSettings::default()
    };
    let module = ModuleDetails {
        summary: ModuleSummary {
            id: String::from("demo"),
            name: String::from("Demo"),
            version: String::from("0.1.0"),
            description: None,
            steam_app_id: None,
            install_state: app_core::InstallState::Installed,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
        schema_json: None,
        default_ports: vec![],
        install: Some(InstallSpec {
            shared_game_dir: String::from("demo"),
            download_url_windows: None,
            download_integrity_windows: None,
            source: None,
            verification_path: None,
            minecraft: None,
        }),
        process: Some(ProcessSpec {
            environment_template: Default::default(),
            executable: String::from("server.exe"),
            args_template: vec![String::from("{{settings.missing}}")],
            working_directory_template: None,
            window_policy: ProcessWindowPolicy::Background,
            host_surface: ProcessHostSurface::ManagedTerminal,
            host_notes: None,
        }),
        workshop: None,
        mods: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
    };
    let instance = InstanceDetails {
        summary: InstanceSummary {
            id: String::from("demo-1"),
            name: String::from("Demo"),
            module_id: String::from("demo"),
            status: InstanceStatus::Stopped,
            bind_ip: String::from("0.0.0.0"),
            port_count: 0,
            autostart: false,
            active_process_count: 0,
        },
        config_file_path: config_dir
            .join("instance.json")
            .to_string_lossy()
            .into_owned(),
        saves_path: String::from("D:/LanGame/instances/saves"),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: false,
        backup_retention_count: 10,
        settings_json: String::from("{}"),
        ports: vec![],
        active_run: None,
    };

    let launch_plan = build_launch_plan(&settings, &module, &instance).expect("build plan");
    assert_eq!(launch_plan.args, vec![String::from("{{settings.missing}}")]);
    assert!(launch_plan.executable_exists);
    assert!(!launch_plan.ready_to_launch);
    assert_eq!(launch_plan.validation_issues.len(), 1);
    assert!(
        launch_plan
            .validation_issues
            .iter()
            .any(|issue| { issue.code == "unresolved_launch_args" && issue.severity == "error" })
    );
    std::fs::remove_dir_all(root).expect("remove launch validation fixture");
}

#[test]
fn build_launch_plan_resolves_caves_alias_port() {
    let temp_root = unique_test_root();
    let games_root = temp_root.join("games");
    let install_root = games_root.join("dontstarve");
    let executable_path = install_root.join("bin").join("server.exe");
    std::fs::create_dir_all(executable_path.parent().expect("bin parent")).expect("create bin");
    std::fs::write(&executable_path, []).expect("write exe placeholder");

    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: temp_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: temp_root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: temp_root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let module = ModuleDetails {
        summary: ModuleSummary {
            id: String::from("dontstarve"),
            name: String::from("Dont Starve Together"),
            version: String::from("0.1.0"),
            description: None,
            steam_app_id: Some(343050),
            install_state: app_core::InstallState::NotInstalled,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
        schema_json: None,
        default_ports: vec![],
        install: Some(InstallSpec {
            shared_game_dir: String::from("dontstarve"),
            download_url_windows: None,
            download_integrity_windows: None,
            source: None,
            verification_path: None,
            minecraft: None,
        }),
        process: Some(ProcessSpec {
            environment_template: Default::default(),
            executable: String::from("bin/server.exe"),
            args_template: vec![String::from("-port"), String::from("{{ports.caves.port}}")],
            working_directory_template: None,
            window_policy: ProcessWindowPolicy::Background,
            host_surface: ProcessHostSurface::ManagedTerminal,
            host_notes: None,
        }),
        workshop: None,
        mods: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
    };
    let instance = InstanceDetails {
        summary: InstanceSummary {
            id: String::from("dst-caves"),
            name: String::from("DST Caves"),
            module_id: String::from("dontstarve"),
            status: InstanceStatus::Stopped,
            bind_ip: String::from("0.0.0.0"),
            port_count: 1,
            autostart: false,
            active_process_count: 0,
        },
        config_file_path: String::from("D:/LanGame/instances/dst-caves/config/instance.json"),
        saves_path: String::from("D:/LanGame/instances/saves"),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: false,
        backup_retention_count: 10,
        settings_json: String::from("{}"),
        ports: vec![PortBinding {
            name: String::from("backup"),
            protocol: String::from("udp"),
            port: 11000,
        }],
        active_run: None,
    };

    let launch_plan = build_launch_plan(&settings, &module, &instance).expect("build plan");
    assert_eq!(
        launch_plan.args,
        vec![String::from("-port"), String::from("11000")]
    );

    let _ = std::fs::remove_dir_all(temp_root);
}

#[test]
fn dontstarve_launch_options_expand_to_independent_arguments() {
    let settings = serde_json::json!({
        "disable_data_collection": true,
        "backup_log_count": 12,
        "backup_log_period": 3600,
        "friends_only": true,
        "allow_ioopenwrite_sandbox_escape": true
    });

    assert_eq!(
        render_dontstarve_launch_args(&settings),
        vec![
            "-disabledatacollection",
            "-backup_log_count",
            "12",
            "-backup_log_period",
            "3600",
            "-fo",
            "-allow_ioopenwrite_sandbox_escape",
        ]
    );

    let instance = InstanceDetails {
        summary: InstanceSummary {
            id: String::from("dst-launch-args"),
            name: String::from("DST launch args"),
            module_id: String::from("dontstarve"),
            status: InstanceStatus::Stopped,
            bind_ip: String::from("0.0.0.0"),
            port_count: 0,
            autostart: false,
            active_process_count: 0,
        },
        config_file_path: String::from("D:/LanGame/instances/dst-launch-args/config/instance.json"),
        saves_path: String::from("D:/LanGame/instances/dst-launch-args/config/clusters/main"),
        backup_uses_declared_saves_path: true,
        auto_backup_on_stop: false,
        backup_retention_count: 10,
        settings_json: settings.to_string(),
        ports: vec![],
        active_run: None,
    };
    let context = TemplateContext {
        instance: &instance,
        settings: &settings,
        install_root: Path::new("D:/LanGame/server-files/dontstarve"),
        config_dir: Path::new("D:/LanGame/instances/dst-launch-args/config"),
        data_dir: Path::new("D:/LanGame/instances/dst-launch-args/data"),
        logs_dir: Path::new("D:/LanGame/instances/dst-launch-args/logs"),
        saves_dir: Path::new("D:/LanGame/instances/dst-launch-args/config/clusters/main"),
    };
    assert_eq!(
        expand_resolved_argument_segments("{{dontstarve.launch_args}}", &context),
        vec![
            "-disabledatacollection",
            "-backup_log_count",
            "12",
            "-backup_log_period",
            "3600",
            "-fo",
            "-allow_ioopenwrite_sandbox_escape",
        ],
        "the derived launch token must expand to independent argv entries"
    );

    assert_eq!(
        render_dontstarve_launch_args(&serde_json::json!({})),
        vec!["-backup_log_count", "100", "-backup_log_period", "86400"]
    );
    assert_eq!(
        render_dontstarve_launch_args(&serde_json::json!({
            "backup_log_count": 0,
            "backup_log_period": 1
        })),
        vec!["-backup_log_count", "0", "-backup_log_period", "1"]
    );
}

#[test]
fn dontstarve_prelaunch_contract_rejects_online_without_token_and_invalid_lan_ports() {
    let ports = vec![
        PortBinding {
            name: String::from("master"),
            protocol: String::from("udp"),
            port: 11_019,
        },
        PortBinding {
            name: String::from("caves"),
            protocol: String::from("udp"),
            port: 11_020,
        },
    ];
    let online = serde_json::json!({
        "cluster_token": "  ",
        "offline_cluster": false,
        "lan_only_cluster": false,
        "enable_caves": true
    });
    let online_issues = collect_dontstarve_launch_setting_issues(&online, &ports);
    assert!(
        online_issues
            .iter()
            .any(|issue| issue.code == "dst_cluster_token_missing")
    );
    assert!(
        !online_issues
            .iter()
            .any(|issue| issue.code == "dst_lan_port_out_of_range")
    );

    let lan = serde_json::json!({
        "cluster_token": "token",
        "offline_cluster": false,
        "lan_only_cluster": true,
        "enable_caves": true
    });
    let lan_issues = collect_dontstarve_launch_setting_issues(&lan, &ports);
    assert_eq!(
        lan_issues
            .iter()
            .filter(|issue| issue.code == "dst_lan_port_out_of_range")
            .count(),
        2
    );

    let master_only_lan = serde_json::json!({
        "cluster_token": "token",
        "offline_cluster": true,
        "lan_only_cluster": false,
        "enable_caves": false
    });
    let master_only_issues = collect_dontstarve_launch_setting_issues(&master_only_lan, &ports);
    assert_eq!(
        master_only_issues
            .iter()
            .filter(|issue| issue.code == "dst_lan_port_out_of_range")
            .count(),
        1,
        "disabled Caves must not block Master-only startup"
    );
}

#[test]
fn dontstarve_prelaunch_contract_rejects_data_collection_opt_out_while_online() {
    let issues = collect_dontstarve_launch_setting_issues(
        &serde_json::json!({
            "cluster_token": "token",
            "offline_cluster": false,
            "disable_data_collection": true
        }),
        &[],
    );

    assert!(
        issues
            .iter()
            .any(|issue| issue.code == "dst_data_collection_requires_offline")
    );
}

#[test]
fn dontstarve_startup_port_remap_stays_inside_the_lan_discovery_range() {
    let occupied = UdpSocket::bind((Ipv4Addr::LOCALHOST, 11_018))
        .expect("reserve the last DST LAN discovery port");
    let remapped = remap_taken_port_bindings_for_module(
        "dontstarve",
        "127.0.0.1",
        &[PortBinding {
            name: String::from("master"),
            protocol: String::from("udp"),
            port: 11_018,
        }],
        &[],
    )
    .expect("DST startup remap should search the whole bounded range")
    .expect("occupied upper bound should remap to the free lower bound");
    assert_eq!(remapped[0].port, 10_998);
    drop(occupied);

    let occupied_public = UdpSocket::bind((Ipv4Addr::LOCALHOST, 12_000))
        .expect("reserve a public/direct-connect port");
    let remapped = remap_taken_port_bindings_for_module(
        "dontstarve",
        "127.0.0.1",
        &[PortBinding {
            name: String::from("master"),
            protocol: String::from("udp"),
            port: 12_000,
        }],
        &[],
    )
    .expect("public/direct-connect ports can remap outside the LAN list range")
    .expect("occupied public port should be remapped");
    assert_eq!(remapped[0].port, 12_001);
    drop(occupied_public);
}

#[test]
fn build_launch_plan_supports_necesse_direct_java_entrypoint() {
    let temp_root = unique_test_root();
    let games_root = temp_root.join("games");
    let install_root = games_root.join("necesse");
    let java_path = install_root.join("jre").join("bin").join("java.exe");
    let server_jar_path = install_root.join("Server.jar");
    let config_dir = temp_root.join("instances").join("necesse-1").join("config");
    std::fs::create_dir_all(java_path.parent().expect("java parent")).expect("create java parent");
    std::fs::create_dir_all(&config_dir).expect("create config dir");
    std::fs::write(&java_path, "placeholder").expect("write java placeholder");
    std::fs::write(&server_jar_path, "placeholder").expect("write server jar placeholder");

    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: temp_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: temp_root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: temp_root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let module = ModuleDetails {
        summary: ModuleSummary {
            id: String::from("necesse"),
            name: String::from("Necesse"),
            version: String::from("0.1.0"),
            description: None,
            steam_app_id: Some(1169040),
            install_state: app_core::InstallState::NotInstalled,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
        schema_json: None,
        default_ports: vec![],
        install: Some(InstallSpec {
            shared_game_dir: String::from("necesse"),
            download_url_windows: None,
            download_integrity_windows: None,
            source: None,
            verification_path: None,
            minecraft: None,
        }),
        process: Some(ProcessSpec {
            environment_template: Default::default(),
            executable: String::from("jre/bin/java.exe"),
            args_template: vec![
                String::from("-jar"),
                String::from("Server.jar"),
                String::from("-nogui"),
                String::from("{{necesse.owner_args}}"),
                String::from("-logs"),
                String::from("..\\logs"),
                String::from("-datadir"),
                String::from("{{paths.data_dir}}"),
            ],
            working_directory_template: Some(String::from("{{paths.install_root}}")),
            window_policy: ProcessWindowPolicy::Background,
            host_surface: ProcessHostSurface::ManagedTerminal,
            host_notes: None,
        }),
        workshop: None,
        mods: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
    };
    let mut instance = InstanceDetails {
        summary: InstanceSummary {
            id: String::from("necesse-1"),
            name: String::from("Necesse Smoke"),
            module_id: String::from("necesse"),
            status: InstanceStatus::Stopped,
            bind_ip: String::from("0.0.0.0"),
            port_count: 0,
            autostart: false,
            active_process_count: 0,
        },
        config_file_path: temp_root
            .join("instances")
            .join("necesse-1")
            .join("config")
            .join("instance.json")
            .to_string_lossy()
            .into_owned(),
        saves_path: String::from("D:/LanGame/instances/saves"),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: false,
        backup_retention_count: 10,
        settings_json: String::from(r#"{ "owner_name": "HostPlayer" }"#),
        ports: vec![],
        active_run: None,
    };

    let launch_plan = build_launch_plan(&settings, &module, &instance).expect("build plan");
    assert!(launch_plan.executable_exists);
    assert!(launch_plan.ready_to_launch);
    assert!(launch_plan.validation_issues.is_empty());
    assert_eq!(PathBuf::from(&launch_plan.executable_path), java_path);
    assert_eq!(
        launch_plan.args,
        vec![
            String::from("-jar"),
            String::from("Server.jar"),
            String::from("-nogui"),
            String::from("-owner"),
            String::from("HostPlayer"),
            String::from("-logs"),
            String::from("..\\logs"),
            String::from("-datadir"),
            temp_root
                .join("instances")
                .join("necesse-1")
                .join("data")
                .to_string_lossy()
                .into_owned(),
        ]
    );
    assert_eq!(PathBuf::from(&launch_plan.working_directory), install_root);
    assert_eq!(launch_plan.window_policy, ProcessWindowPolicy::Background);
    assert!(!launch_plan.uses_script_entrypoint);

    instance.settings_json = String::from(r#"{ "owner_name": "" }"#);
    let launch_plan_without_owner =
        build_launch_plan(&settings, &module, &instance).expect("build plan without owner");
    assert!(
        !launch_plan_without_owner
            .args
            .iter()
            .any(|arg| arg == "-owner")
    );

    let _ = std::fs::remove_dir_all(temp_root);
}

#[test]
fn build_launch_plan_reports_missing_executable() {
    let temp_root = unique_test_root();
    let games_root = temp_root.join("games");
    let config_dir = temp_root
        .join("instances")
        .join("palworld-missing-executable")
        .join("config");
    std::fs::create_dir_all(&config_dir).expect("create config dir");

    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: temp_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: temp_root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: temp_root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let module = ModuleDetails {
        summary: ModuleSummary {
            id: String::from("palworld"),
            name: String::from("Palworld"),
            version: String::from("0.1.0"),
            description: None,
            steam_app_id: Some(2394010),
            install_state: app_core::InstallState::NotInstalled,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
        schema_json: None,
        default_ports: vec![],
        install: Some(InstallSpec {
            shared_game_dir: String::from("palworld"),
            download_url_windows: None,
            download_integrity_windows: None,
            source: None,
            verification_path: None,
            minecraft: None,
        }),
        process: Some(ProcessSpec {
            environment_template: Default::default(),
            executable: String::from("Pal/Binaries/Win64/PalServer-Win64-Shipping-Cmd.exe"),
            args_template: vec![],
            working_directory_template: None,
            window_policy: ProcessWindowPolicy::Background,
            host_surface: ProcessHostSurface::ManagedTerminal,
            host_notes: None,
        }),
        workshop: None,
        mods: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
    };
    let instance = InstanceDetails {
        summary: InstanceSummary {
            id: String::from("palworld-missing-executable"),
            name: String::from("Palworld Missing Executable"),
            module_id: String::from("palworld"),
            status: InstanceStatus::Stopped,
            bind_ip: String::from("0.0.0.0"),
            port_count: 0,
            autostart: false,
            active_process_count: 0,
        },
        config_file_path: config_dir
            .join("instance.json")
            .to_string_lossy()
            .into_owned(),
        saves_path: String::from("D:/LanGame/instances/saves"),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: false,
        backup_retention_count: 10,
        settings_json: String::from("{}"),
        ports: vec![],
        active_run: None,
    };

    let launch_plan = build_launch_plan(&settings, &module, &instance).expect("build plan");
    assert!(!launch_plan.executable_exists);
    assert!(!launch_plan.ready_to_launch);
    assert!(launch_plan.validation_issues.iter().any(|issue| {
        issue.code == "launch_executable_missing"
            && issue
                .message
                .contains("Install or repair the game files first")
            && issue
                .path
                .as_deref()
                .unwrap_or_default()
                .contains("PalServer-Win64-Shipping-Cmd.exe")
    }));

    let _ = std::fs::remove_dir_all(temp_root);
}

#[test]
fn build_launch_plan_reports_unavailable_udp_port() {
    let occupied_socket = UdpSocket::bind("127.0.0.1:0").expect("bind occupied udp socket");
    let occupied_port = occupied_socket
        .local_addr()
        .expect("occupied udp addr")
        .port();

    let temp_root = unique_test_root();
    let games_root = temp_root.join("games");
    let install_root = games_root.join("enshrouded");
    let executable_path = install_root.join("enshrouded_server.exe");
    let config_dir = temp_root
        .join("instances")
        .join("enshrouded-occupied-port")
        .join("config");
    std::fs::create_dir_all(executable_path.parent().expect("exe parent"))
        .expect("create exe parent");
    std::fs::create_dir_all(&config_dir).expect("create config dir");
    std::fs::write(&executable_path, []).expect("write exe placeholder");

    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: temp_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: temp_root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: temp_root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let module = ModuleDetails {
        summary: ModuleSummary {
            id: String::from("enshrouded"),
            name: String::from("Enshrouded"),
            version: String::from("0.1.0"),
            description: None,
            steam_app_id: Some(2278520),
            install_state: app_core::InstallState::Installed,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
        schema_json: None,
        default_ports: vec![],
        install: Some(InstallSpec {
            shared_game_dir: String::from("enshrouded"),
            download_url_windows: None,
            download_integrity_windows: None,
            source: None,
            verification_path: None,
            minecraft: None,
        }),
        process: Some(ProcessSpec {
            environment_template: Default::default(),
            executable: String::from("enshrouded_server.exe"),
            args_template: vec![],
            working_directory_template: None,
            window_policy: ProcessWindowPolicy::Background,
            host_surface: ProcessHostSurface::ManagedTerminal,
            host_notes: None,
        }),
        workshop: None,
        mods: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
    };
    let instance = InstanceDetails {
        summary: InstanceSummary {
            id: String::from("enshrouded-occupied-port"),
            name: String::from("Enshrouded Occupied Port"),
            module_id: String::from("enshrouded"),
            status: app_core::InstanceStatus::Stopped,
            bind_ip: String::from("127.0.0.1"),
            port_count: 1,
            autostart: false,
            active_process_count: 0,
        },
        config_file_path: config_dir
            .join("instance.json")
            .to_string_lossy()
            .into_owned(),
        saves_path: temp_root
            .join("instances")
            .join("enshrouded-occupied-port")
            .join("savegame")
            .to_string_lossy()
            .into_owned(),
        backup_uses_declared_saves_path: true,
        auto_backup_on_stop: false,
        backup_retention_count: 10,
        settings_json: String::from("{}"),
        ports: vec![PortBinding {
            name: String::from("query"),
            protocol: String::from("udp"),
            port: occupied_port,
        }],
        active_run: None,
    };

    let launch_plan = build_launch_plan(&settings, &module, &instance).expect("build plan");
    assert!(!launch_plan.ready_to_launch);
    assert!(launch_plan.validation_issues.iter().any(|issue| {
        issue.code == "port_binding_unavailable"
            && issue.message.contains("query")
            && issue.message.contains(&occupied_port.to_string())
            && issue.context["port_name"] == "query"
            && issue.context["protocol"] == "UDP"
            && issue.context["address"].ends_with(&format!(":{occupied_port}"))
    }));

    drop(occupied_socket);
    let _ = std::fs::remove_dir_all(temp_root);
}
