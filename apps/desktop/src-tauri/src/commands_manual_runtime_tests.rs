use super::*;
use crate::commands::commands_mod_staging::{
    stage_downloaded_workshop_items_into_manual_target, stage_manual_mod_sources,
    validate_manual_mod_archive_metrics,
};
use crate::commands::commands_mods::{
    infer_manual_mod_inventory_id, read_manual_mod_inventory_items,
    resolve_direct_manual_mod_reference, should_stage_downloaded_workshop_items,
};

#[path = "commands_mod_deployment_lock_tests.rs"]
mod mod_deployment_lock_tests;

#[path = "commands_app_exit_sessions_tests.rs"]
mod app_exit_sessions_tests;
#[path = "commands_creation_catalog_tests.rs"]
mod creation_catalog_tests;
#[path = "commands_install_launch_matrix_tests.rs"]
mod install_launch_matrix_tests;
#[path = "commands_native_lifecycle_tests.rs"]
mod native_lifecycle_tests;
#[path = "commands_uninstall_catalog_tests.rs"]
mod uninstall_catalog_tests;
#[path = "commands_uninstall_tests.rs"]
mod uninstall_tests;

#[test]
pub(super) fn manual_mod_reference_candidates_keep_urls_and_ids() {
    assert_eq!(
            normalize_manual_mod_references(vec![String::from(
                "https://www.curseforge.com/ark-survival-ascended/mods/devkitlivemodtesting\n1346144\nmodrinth:fabric-api"
            )])
            .unwrap(),
            vec![
                String::from("https://www.curseforge.com/ark-survival-ascended/mods/devkitlivemodtesting"),
                String::from("1346144"),
                String::from("modrinth:fabric-api")
            ]
        );
}

#[test]
pub(super) fn manual_mod_reference_candidates_unwrap_common_redirect_links() {
    let references = normalize_manual_mod_references(vec![String::from(
            r#"
            <a href="https://steamcommunity.com/linkfilter/?u=https%3A%2F%2Fwww.curseforge.com%2Fark-survival-ascended%2Fmods%2Fawesome-spyglass">CurseForge</a>
            https://www.google.com/url?q=https%3A%2F%2Fmodrinth.com%2Fmod%2Fsodium&amp;sa=D
            https://example.com/redirect?target=https%3A%2F%2Fwww.nexusmods.com%2F7daystodie%2Fmods%2F1234
            "#,
        )])
        .expect("manual mod references");
    assert_eq!(
        references,
        vec![
            String::from("https://www.curseforge.com/ark-survival-ascended/mods/awesome-spyglass"),
            String::from("https://modrinth.com/mod/sodium"),
            String::from("https://www.nexusmods.com/7daystodie/mods/1234"),
        ]
    );
}

#[test]
pub(super) fn manual_mod_reference_candidates_deduplicates_case_insensitively() {
    let references = normalize_manual_mod_references(vec![
        String::from("  https://www.curseforge.com/ark-survival-ascended/mods/ExampleMod "),
        String::from("https://www.curseforge.com/ark-survival-ascended/mods/examplemod"),
        String::from("modrinth:FABRIC-API"),
        String::from("modrinth:Fabric-API"),
    ])
    .expect("normalize duplicate mod references");

    assert_eq!(
        references,
        vec![
            String::from("https://www.curseforge.com/ark-survival-ascended/mods/ExampleMod"),
            String::from("modrinth:FABRIC-API"),
        ]
    );
}

#[test]
pub(super) fn folder_name_mod_reference_strategy_accepts_resource_names() {
    let enablement = app_core::ModuleModEnablementSpec {
        setting_key: String::from("ensured_resources"),
        setting_label: String::from("Ensured Resources"),
        id_strategy: Some(String::from("folder_name")),
        reference_strategy: Some(String::from("plain_id")),
        reference_game_id: None,
    };

    assert_eq!(
        infer_manual_mod_inventory_id(enablement.id_strategy.as_deref(), "qb-core")
            .expect("folder resource id"),
        String::from("qb-core")
    );
    assert_eq!(
        resolve_direct_manual_mod_reference(&enablement, "  qb-core  ").expect("resource name"),
        String::from("qb-core")
    );
    assert!(infer_manual_mod_inventory_id(enablement.id_strategy.as_deref(), "..").is_none());
    assert!(resolve_direct_manual_mod_reference(&enablement, "bad/name").is_none());
}

#[test]
pub(super) fn steam_workshop_reference_strategy_extracts_item_ids() {
    let enablement = app_core::ModuleModEnablementSpec {
        setting_key: String::from("mod_workshop_ids"),
        setting_label: String::from("Workshop mods"),
        id_strategy: Some(String::from("folder_name")),
        reference_strategy: Some(String::from("steam_workshop_id")),
        reference_game_id: Some(1371580),
    };

    assert_eq!(
        resolve_direct_manual_mod_reference(
            &enablement,
            "https://steamcommunity.com/sharedfiles/filedetails/?id=3181632026"
        )
        .expect("workshop url"),
        String::from("3181632026")
    );
    assert_eq!(
        resolve_direct_manual_mod_reference(&enablement, "workshop-3202759474")
            .expect("workshop token"),
        String::from("3202759474")
    );
    assert!(resolve_direct_manual_mod_reference(&enablement, "not-a-workshop-id").is_none());
}

#[test]
pub(super) fn workshop_download_auto_stage_allows_enablement_backed_folder_mods() {
    let manifest = r#"
        [mods.manual_staging]
        target_template = "{{paths.install_root}}/MOE/Mods"
        target_label = "MOE/Mods"
        accepts = ["folder"]

        [mods.enablement]
        setting_key = "mod_workshop_ids"
        setting_label = "Steam Workshop Mods"
        id_strategy = "folder_name"
        reference_strategy = "steam_workshop_id"
    "#;

    assert!(should_stage_downloaded_workshop_items(manifest));

    let missing_staging = r#"
        [mods.enablement]
        setting_key = "mod_workshop_ids"
        setting_label = "Steam Workshop Mods"
        id_strategy = "folder_name"
        reference_strategy = "steam_workshop_id"
    "#;

    assert!(!should_stage_downloaded_workshop_items(missing_staging));
}

#[test]
pub(super) fn palworld_package_name_inventory_strategy_reads_info_json() {
    let root = real_smoke_support::allocate_smoke_run_root("palworld-mod-inventory")
        .expect("palworld inventory root");
    let workshop_item = root.join("3202759474");
    fs::create_dir_all(&workshop_item).unwrap();
    fs::write(
        workshop_item.join("Info.json"),
        r#"{"PackageName":"GamingCattiva","Version":"1.0.0"}"#,
    )
    .unwrap();
    fs::write(root.join("loose.pak"), []).unwrap();

    let items =
        read_manual_mod_inventory_items(Some("palworld_package_name"), &root).expect("inventory");
    let ids = items
        .iter()
        .map(|item| (item.name.as_str(), item.inferred_id.as_deref()))
        .collect::<Vec<_>>();

    assert!(ids.contains(&("3202759474", Some("GamingCattiva"))));
    assert!(ids.contains(&("loose.pak", None)));
}

#[test]
pub(super) fn manual_mod_source_path_inputs_route_site_links_to_references() {
    let split = split_manual_mod_source_path_inputs(vec![
        String::from("D:/Downloads/NexusSmokeMod"),
        String::from("https://www.nexusmods.com/7daystodie/mods/123"),
        String::from("modrinth:fabric-api"),
    ]);

    assert_eq!(
        split.source_paths,
        vec![String::from("D:/Downloads/NexusSmokeMod")]
    );
    assert_eq!(
        split.reference_inputs,
        vec![
            String::from("https://www.nexusmods.com/7daystodie/mods/123"),
            String::from("modrinth:fabric-api")
        ]
    );
}

#[test]
pub(super) fn runtime_restart_policy_defaults_to_guarded_off() {
    let policy = runtime_restart_policy_from_settings("{}");

    assert!(!policy.enabled);
    assert_eq!(policy.max_restarts, 3);
    assert_eq!(policy.backoff_ms, 5_000);
    assert!(policy.only_nonzero_exit);
}

#[test]
pub(super) fn runtime_restart_policy_ignores_obsolete_json_keys_and_invalid_scalar_types() {
    for settings_json in [
        r#"{
            "restart_policy": {"enabled":true,"max_restarts":9,"backoff_ms":20000,"only_nonzero_exit":false},
            "auto_restart_enabled":true,"crash_restart_limit":8,
            "auto_restart_backoff_ms":25000,"auto_restart_only_nonzero_exit":false
        }"#,
        r#"{"runtime_restart":{"enabled":"yes","max_restarts":"8","backoff_ms":"20000","only_nonzero_exit":"off"}}"#,
    ] {
        let policy = runtime_restart_policy_from_settings(settings_json);
        assert!(!policy.enabled);
        assert_eq!(policy.max_restarts, 3);
        assert_eq!(policy.backoff_ms, 5_000);
        assert!(policy.only_nonzero_exit);
    }
    let policy = runtime_restart_policy_from_settings(
        r#"{"runtime_restart":{"enabled":false,"backoff_ms":0},
            "restart_policy":{"enabled":true,"max_restarts":9,"only_nonzero_exit":false},
            "auto_restart_enabled":true,"auto_restart_backoff_ms":20000}"#,
    );
    assert!(!policy.enabled);
    assert_eq!(policy.max_restarts, 3);
    assert_eq!(policy.backoff_ms, 0);
    assert!(policy.only_nonzero_exit);
}

#[test]
pub(super) fn runtime_restart_policy_reads_nested_settings_with_bounds() {
    let policy = runtime_restart_policy_from_settings(
        r#"{
                "runtime_restart": {
                    "enabled": true,
                    "max_restarts": 25,
                    "backoff_ms": 900000,
                    "only_nonzero_exit": false
                }
            }"#,
    );

    assert!(policy.enabled);
    assert_eq!(policy.max_restarts, 10);
    assert_eq!(policy.backoff_ms, 300_000);
    assert!(!policy.only_nonzero_exit);
}

#[test]
pub(super) fn runtime_performance_state_diagnostic_flags_untracked_runs() {
    let diagnostic = runtime_performance_state_diagnostic_signal(&RuntimePerformanceSnapshot {
        applied_resource_limits: None,
        status: String::from("untracked"),
        summary: String::from("untracked"),
        policy: RuntimePerformancePolicy::default(),
        preview: app_core::RuntimePerformancePolicyPreview::default(),
        process_count: 1,
        processes: Vec::new(),
    })
    .expect("untracked diagnostic");

    assert_eq!(diagnostic.code, "runtime_performance_untracked");
    assert_eq!(diagnostic.severity, "warning");
    assert!(diagnostic.actionable);
}

#[test]
pub(super) fn active_runtime_summary_counts_instances_and_process_rows_separately() {
    let entries = vec![
        ActiveInstanceRunEntry {
            instance_id: String::from("dst-main"),
            run_id: 1,
            session_id: Some(String::from("dst-session")),
            process_key: String::from("master"),
            display_name: String::from("Master shard"),
            pid: Some(1001),
            process_identity: None,
            log_path: None,
            is_primary: true,
        },
        ActiveInstanceRunEntry {
            instance_id: String::from("dst-main"),
            run_id: 2,
            session_id: Some(String::from("dst-session")),
            process_key: String::from("caves"),
            display_name: String::from("Caves shard"),
            pid: Some(1002),
            process_identity: None,
            log_path: None,
            is_primary: false,
        },
        ActiveInstanceRunEntry {
            instance_id: String::from("valheim-main"),
            run_id: 3,
            session_id: Some(String::from("valheim-session")),
            process_key: String::from("server"),
            display_name: String::from("Valheim server"),
            pid: Some(2001),
            process_identity: None,
            log_path: None,
            is_primary: true,
        },
    ];

    assert_eq!(summarize_active_runtime_entries(&entries), (2, 3));
}

pub(in crate::commands) struct ProgramDataEnvGuard {
    previous_program_data: Option<OsString>,
    previous_local_app_data: Option<OsString>,
}

impl ProgramDataEnvGuard {
    pub(in crate::commands) fn set(path: &Path) -> Self {
        let previous_program_data = env::var_os("PROGRAMDATA");
        let previous_local_app_data = env::var_os("LOCALAPPDATA");
        let local_app_data = path.parent().unwrap_or(path).join("localappdata");
        unsafe {
            env::set_var("PROGRAMDATA", path);
            env::set_var("LOCALAPPDATA", local_app_data);
        }
        Self {
            previous_program_data,
            previous_local_app_data,
        }
    }
}

impl Drop for ProgramDataEnvGuard {
    fn drop(&mut self) {
        match self.previous_program_data.take() {
            Some(value) => unsafe {
                env::set_var("PROGRAMDATA", value);
            },
            None => unsafe {
                env::remove_var("PROGRAMDATA");
            },
        }
        match self.previous_local_app_data.take() {
            Some(value) => unsafe {
                env::set_var("LOCALAPPDATA", value);
            },
            None => unsafe {
                env::remove_var("LOCALAPPDATA");
            },
        }
    }
}

pub(super) fn workspace_root() -> PathBuf {
    real_smoke_support::smoke_workspace_root()
}

pub(super) fn command_smoke_games_root(env_name: &str) -> PathBuf {
    real_smoke_support::resolve_smoke_games_root(
        Some(env_name),
        &real_smoke_support::smoke_games_root(),
    )
}

pub(super) fn project_zomboid_command_smoke_run_root() -> PathBuf {
    real_smoke_support::allocate_smoke_run_root("projectzomboid-command-smoke-runs")
        .expect("command smoke run root")
}

pub(super) fn project_zomboid_command_smoke_settings(run_root: &Path) -> AppSettings {
    let workspace_root = workspace_root();
    let games_root = command_smoke_games_root("LANGAME_PZ_SMOKE_GAMES_ROOT");

    AppSettings {
        archives_root: String::new(),
        servers_root: run_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: workspace_root
            .join("modules")
            .to_string_lossy()
            .into_owned(),
        steamcmd_root: run_root.join("steamcmd").to_string_lossy().into_owned(),
    }
}

pub(super) fn dontstarve_command_smoke_run_root() -> PathBuf {
    real_smoke_support::allocate_smoke_run_root("dontstarve-command-smoke-runs")
        .expect("command smoke run root")
}

pub(super) fn palworld_command_smoke_run_root() -> PathBuf {
    real_smoke_support::allocate_smoke_run_root("palworld-command-smoke-runs")
        .expect("command smoke run root")
}

pub(super) fn dontstarve_command_smoke_settings(run_root: &Path) -> AppSettings {
    let workspace_root = workspace_root();
    let games_root = command_smoke_games_root("LANGAME_DST_SMOKE_GAMES_ROOT");

    AppSettings {
        archives_root: String::new(),
        servers_root: run_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: workspace_root
            .join("modules")
            .to_string_lossy()
            .into_owned(),
        steamcmd_root: run_root.join("steamcmd").to_string_lossy().into_owned(),
    }
}

pub(super) fn palworld_command_smoke_settings(run_root: &Path) -> AppSettings {
    let workspace_root = workspace_root();
    let games_root = command_smoke_games_root("LANGAME_PALWORLD_SMOKE_GAMES_ROOT");

    AppSettings {
        archives_root: String::new(),
        servers_root: run_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: workspace_root
            .join("modules")
            .to_string_lossy()
            .into_owned(),
        steamcmd_root: run_root.join("steamcmd").to_string_lossy().into_owned(),
    }
}

pub(super) fn corekeeper_command_smoke_run_root() -> PathBuf {
    real_smoke_support::allocate_smoke_run_root("corekeeper-command-smoke-runs")
        .expect("command smoke run root")
}

pub(super) fn corekeeper_command_smoke_settings(run_root: &Path) -> AppSettings {
    let workspace_root = workspace_root();
    let games_root = command_smoke_games_root("LANGAME_COREKEEPER_SMOKE_GAMES_ROOT");

    AppSettings {
        archives_root: String::new(),
        servers_root: run_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: workspace_root
            .join("modules")
            .to_string_lossy()
            .into_owned(),
        steamcmd_root: run_root.join("steamcmd").to_string_lossy().into_owned(),
    }
}

pub(super) fn sevendaystodie_command_smoke_run_root() -> PathBuf {
    real_smoke_support::allocate_smoke_run_root("sevendaystodie-command-smoke-runs")
        .expect("command smoke run root")
}

pub(super) fn sevendaystodie_command_smoke_settings(run_root: &Path) -> AppSettings {
    let workspace_root = workspace_root();
    let games_root = command_smoke_games_root("LANGAME_7DTD_SMOKE_GAMES_ROOT");

    AppSettings {
        archives_root: String::new(),
        servers_root: run_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: workspace_root
            .join("modules")
            .to_string_lossy()
            .into_owned(),
        steamcmd_root: run_root.join("steamcmd").to_string_lossy().into_owned(),
    }
}

pub(super) fn valheim_command_smoke_run_root() -> PathBuf {
    real_smoke_support::allocate_smoke_run_root("valheim-command-smoke-runs")
        .expect("command smoke run root")
}

pub(super) fn vrising_command_smoke_run_root() -> PathBuf {
    real_smoke_support::allocate_smoke_run_root("vrising-command-smoke-runs")
        .expect("command smoke run root")
}

pub(super) fn valheim_command_smoke_settings(run_root: &Path) -> AppSettings {
    let workspace_root = workspace_root();
    let games_root = command_smoke_games_root("LANGAME_VALHEIM_SMOKE_GAMES_ROOT");

    AppSettings {
        archives_root: String::new(),
        servers_root: run_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: workspace_root
            .join("modules")
            .to_string_lossy()
            .into_owned(),
        steamcmd_root: run_root.join("steamcmd").to_string_lossy().into_owned(),
    }
}

pub(super) fn vrising_command_smoke_settings(run_root: &Path) -> AppSettings {
    let workspace_root = workspace_root();
    let games_root = command_smoke_games_root("LANGAME_VRISING_SMOKE_GAMES_ROOT");

    AppSettings {
        archives_root: String::new(),
        servers_root: run_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: workspace_root
            .join("modules")
            .to_string_lossy()
            .into_owned(),
        steamcmd_root: run_root.join("steamcmd").to_string_lossy().into_owned(),
    }
}

pub(super) fn command_result<T>(
    result: Result<T, String>,
) -> Result<T, Box<dyn std::error::Error>> {
    result.map_err(|message| std::io::Error::other(message).into())
}

pub(super) fn read_desktop_log_actions(
    path: &Path,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    Ok(fs::read_to_string(path)?
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|entry| {
            entry
                .get("action")
                .and_then(Value::as_str)
                .map(String::from)
        })
        .collect())
}

#[tokio::test(flavor = "current_thread")]
pub(super) async fn uninstall_module_game_removes_managed_server_files_only()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = real_smoke_support::allocate_smoke_run_root("module-uninstall-command-tests")
        .expect("uninstall command run root");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let workspace_root = workspace_root();
    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: run_root.join("instances").to_string_lossy().into_owned(),
        games_root: run_root.join("games").to_string_lossy().into_owned(),
        modules_root: workspace_root
            .join("modules")
            .to_string_lossy()
            .into_owned(),
        steamcmd_root: run_root.join("steamcmd").to_string_lossy().into_owned(),
    };

    save_app_settings(settings.clone())?;
    let games_root = PathBuf::from(&settings.games_root);
    let install_root = games_root.join("sevendaystodie");
    fs::create_dir_all(&install_root)?;
    fs::write(
        install_root.join("7DaysToDieServer.exe"),
        "fake 7DTD executable",
    )?;
    fs::write(games_root.join("keep.txt"), "keep")?;

    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");

    let result = command_result(
        uninstall_module_game(app.handle().clone(), String::from("sevendaystodie")).await,
    )?;

    assert_eq!(result.module_id, "sevendaystodie");
    assert!(!result.executable_exists);
    assert!(matches!(result.install_state, InstallState::NotInstalled));
    assert!(
        !install_root.exists(),
        "managed 7DTD install root should be removed"
    );
    assert!(
        games_root.join("keep.txt").exists(),
        "uninstall must not delete unrelated game-root files"
    );
    assert_eq!(result.cleanup.removed_install_roots.len(), 1);
    assert_eq!(
        Path::new(&result.cleanup.removed_install_roots[0]),
        install_root
    );
    assert!(result.cleanup.retained_installs.is_empty());
    assert!(result.cleanup.preserved_data_paths.is_empty());

    let tracked_module = app
        .state::<DesktopState>()
        .app_state
        .read()
        .unwrap()
        .modules
        .iter()
        .find(|module| module.id == "sevendaystodie")
        .cloned()
        .expect("uninstall should refresh module state");
    assert!(matches!(
        tracked_module.install_state,
        InstallState::NotInstalled
    ));

    Ok(())
}

pub(super) fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

pub(super) fn join_relative(root: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .fold(root.to_path_buf(), |path, segment| path.join(segment))
}

pub(super) fn write_text_fixture_files(
    root: &Path,
    files: &[(&str, &str)],
) -> Result<(usize, u64), Box<dyn std::error::Error>> {
    let mut file_count = 0usize;
    let mut total_bytes = 0u64;

    for (relative, contents) in files {
        let path = join_relative(root, relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, contents)?;
        file_count += 1;
        total_bytes += contents.len() as u64;
    }

    Ok((file_count, total_bytes))
}

pub(super) fn read_text_fixture_file(
    root: &Path,
    relative: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    Ok(fs::read_to_string(join_relative(root, relative))?)
}

pub(super) fn snapshot_text_fixture_files(
    root: &Path,
    files: &[&str],
) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    files
        .iter()
        .map(|relative| {
            Ok((
                String::from(*relative),
                read_text_fixture_file(root, relative)?,
            ))
        })
        .collect()
}

pub(super) fn dontstarve_enable_caves_update(
    details: &InstanceDetails,
) -> Result<UpdateInstanceInput, Box<dyn std::error::Error>> {
    let mut settings =
        serde_json::from_str::<serde_json::Map<String, Value>>(&details.settings_json)?;
    settings.insert(String::from("enable_caves"), Value::Bool(true));
    settings.insert(
        String::from("cluster_name"),
        Value::String(details.summary.name.clone()),
    );

    Ok(UpdateInstanceInput {
        id: details.summary.id.clone(),
        bind_ip: details.summary.bind_ip.clone(),
        auto_backup_on_stop: details.auto_backup_on_stop,
        backup_retention_count: details.backup_retention_count,
        settings_json: serde_json::to_string_pretty(&Value::Object(settings))?,
        ports: details.ports.clone(),
    })
}

pub(super) fn dontstarve_access_update(
    details: &InstanceDetails,
) -> Result<UpdateInstanceInput, Box<dyn std::error::Error>> {
    let mut settings =
        serde_json::from_str::<serde_json::Map<String, Value>>(&details.settings_json)?;
    settings.insert(
        String::from("cluster_name"),
        Value::String(String::from("Command Access DST")),
    );
    settings.insert(
        String::from("cluster_description"),
        Value::String(String::from("Command-lane managed DST cluster")),
    );
    settings.insert(String::from("max_players"), Value::from(8));
    settings.insert(
        String::from("game_mode"),
        Value::String(String::from("endless")),
    );
    settings.insert(
        String::from("cluster_intention"),
        Value::String(String::from("cooperative")),
    );
    settings.insert(
        String::from("bind_ip"),
        Value::String(String::from("127.0.0.1")),
    );
    settings.insert(
        String::from("cluster_password"),
        Value::String(String::from("dst-safe-pass")),
    );
    settings.insert(String::from("pause_when_empty"), Value::Bool(true));
    settings.insert(String::from("pvp"), Value::Bool(false));
    settings.insert(String::from("vote_enabled"), Value::Bool(false));
    settings.insert(String::from("offline_cluster"), Value::Bool(false));
    settings.insert(String::from("lan_only_cluster"), Value::Bool(false));
    settings.insert(String::from("tick_rate"), Value::from(20));
    settings.insert(String::from("autosaver_enabled"), Value::Bool(true));
    settings.insert(String::from("enable_caves"), Value::Bool(true));
    settings.insert(
        String::from("cluster_token"),
        Value::String(String::from("dst-command-token")),
    );
    settings.insert(
        String::from("admin_list"),
        Value::String(String::from("KU_admin_one\nKU_admin_two")),
    );
    settings.insert(
        String::from("whitelist"),
        Value::String(String::from("KU_white_one\nKU_white_two")),
    );
    settings.insert(
        String::from("blocklist"),
        Value::String(String::from("KU_blocked_one")),
    );
    settings.insert(String::from("whitelist_slots"), Value::from(2));
    settings.insert(String::from("steam_group_only"), Value::Bool(true));
    settings.insert(
        String::from("steam_group_id"),
        Value::from(103582791400000000_i64),
    );
    settings.insert(String::from("steam_group_admins"), Value::Bool(true));
    settings.insert(
            String::from("shared_workshop_mod_ids"),
            Value::String(String::from(
                "workshop-1234567890\nhttps://steamcommunity.com/sharedfiles/filedetails/?id=9876543210\n1234567890\nbad",
            )),
        );
    settings.insert(
        String::from("shared_workshop_collection_ids"),
        Value::String(String::from("2345678901")),
    );
    settings.insert(
        String::from("master_enabled_workshop_mod_ids"),
        Value::String(String::from("1234567890\n9876543210")),
    );
    settings.insert(
        String::from("caves_enabled_workshop_mod_ids"),
        Value::String(String::from("9876543210")),
    );
    settings.insert(
        String::from("master_mod_configuration_options"),
        json!({
            "1234567890": {
                "difficulty": "hard",
                "enabled": true,
                "spawn_rate": 2
            }
        }),
    );
    settings.insert(
        String::from("caves_mod_configuration_options"),
        json!({
            "9876543210": {
                "cave_setting": "safe",
                "enabled": true
            }
        }),
    );

    let mut ports = details.ports.clone();
    for port in &mut ports {
        match port.name.as_str() {
            "master" => port.port = 11999,
            "caves" => port.port = 12000,
            "shard_master" => port.port = 11888,
            "steam_query" => port.port = 28016,
            "steam_auth" => port.port = 18766,
            "caves_steam_query" => port.port = 28017,
            "caves_steam_auth" => port.port = 18767,
            _ => {}
        }
    }

    Ok(UpdateInstanceInput {
        id: details.summary.id.clone(),
        bind_ip: String::from("127.0.0.1"),
        auto_backup_on_stop: details.auto_backup_on_stop,
        backup_retention_count: details.backup_retention_count,
        settings_json: serde_json::to_string_pretty(&Value::Object(settings))?,
        ports,
    })
}

pub(super) fn palworld_access_update(
    details: &InstanceDetails,
) -> Result<UpdateInstanceInput, Box<dyn std::error::Error>> {
    let mut settings =
        serde_json::from_str::<serde_json::Map<String, Value>>(&details.settings_json)?;
    settings.insert(
        String::from("server_name"),
        Value::String(String::from("Command Access Palworld")),
    );
    settings.insert(
        String::from("server_description"),
        Value::String(String::from("Command-lane managed Palworld server")),
    );
    settings.insert(String::from("max_players"), Value::from(24));
    settings.insert(
        String::from("server_password"),
        Value::String(String::from("pal-safe-pass")),
    );
    settings.insert(
        String::from("admin_password"),
        Value::String(String::from("pal-admin-safe")),
    );
    settings.insert(String::from("community_server"), Value::Bool(true));
    settings.insert(
        String::from("public_ip"),
        Value::String(String::from("203.0.113.24")),
    );
    settings.insert(String::from("public_port"), Value::from(18211));
    settings.insert(String::from("region"), Value::String(String::from("Asia")));
    settings.insert(
        String::from("crossplay_platforms"),
        Value::String(String::from("Steam\nXbox\nPS5")),
    );
    settings.insert(String::from("use_auth"), Value::Bool(true));
    settings.insert(String::from("show_player_list"), Value::Bool(true));
    settings.insert(String::from("join_left_message"), Value::Bool(false));
    settings.insert(String::from("allow_client_mod"), Value::Bool(false));
    settings.insert(String::from("use_backup_save_data"), Value::Bool(true));
    settings.insert(
        String::from("log_format"),
        Value::String(String::from("json")),
    );
    settings.insert(String::from("rcon_enabled"), Value::Bool(true));
    settings.insert(String::from("rest_api_enabled"), Value::Bool(true));
    settings.insert(
        String::from("ban_list_url"),
        Value::String(String::from("https://ops.example.com/palworld/banlist.txt")),
    );
    settings.insert(String::from("enable_fast_travel"), Value::Bool(false));
    settings.insert(String::from("is_pvp"), Value::Bool(true));
    settings.insert(
        String::from("enable_player_to_player_damage"),
        Value::Bool(true),
    );
    settings.insert(String::from("enable_friendly_fire"), Value::Bool(true));
    settings.insert(
        String::from("death_penalty"),
        Value::String(String::from("ItemAndEquipment")),
    );
    settings.insert(
        String::from("deny_technology_list"),
        Value::String(String::from("TechnologyA\nTechnologyB")),
    );
    settings.insert(String::from("chat_post_limit_per_minute"), Value::from(12));
    settings.insert(String::from("launch_perf_threads"), Value::Bool(true));
    settings.insert(
        String::from("launch_worker_threads_enabled"),
        Value::Bool(true),
    );
    settings.insert(String::from("worker_thread_count"), Value::from(12));

    let mut ports = details.ports.clone();
    for port in &mut ports {
        match port.name.as_str() {
            "game" => port.port = 18211,
            "rcon" => port.port = 28575,
            "rest_api" => port.port = 18212,
            _ => {}
        }
    }

    Ok(UpdateInstanceInput {
        id: details.summary.id.clone(),
        bind_ip: String::from("127.0.0.1"),
        auto_backup_on_stop: details.auto_backup_on_stop,
        backup_retention_count: details.backup_retention_count,
        settings_json: serde_json::to_string_pretty(&Value::Object(settings))?,
        ports,
    })
}

pub(super) fn project_zomboid_access_update(
    details: &InstanceDetails,
) -> Result<UpdateInstanceInput, Box<dyn std::error::Error>> {
    let mut settings =
        serde_json::from_str::<serde_json::Map<String, Value>>(&details.settings_json)?;
    settings.insert(
        String::from("server_name"),
        Value::String(String::from("Command Access Project Zomboid")),
    );
    settings.insert(
        String::from("server_description"),
        Value::String(String::from("Command-lane managed Project Zomboid room")),
    );
    settings.insert(
        String::from("welcome_message"),
        Value::String(String::from("Welcome survivor\nKeep the safehouse locked")),
    );
    settings.insert(String::from("max_players"), Value::from(18));
    settings.insert(String::from("public_server"), Value::Bool(true));
    settings.insert(String::from("pause_empty"), Value::Bool(false));
    settings.insert(String::from("global_chat"), Value::Bool(false));
    settings.insert(String::from("open_server"), Value::Bool(false));
    settings.insert(
        String::from("server_password"),
        Value::String(String::from("pz-safe-pass")),
    );
    settings.insert(
        String::from("auto_create_user_in_whitelist"),
        Value::Bool(true),
    );
    settings.insert(String::from("display_user_name"), Value::Bool(false));
    settings.insert(String::from("show_first_and_last_name"), Value::Bool(true));
    settings.insert(String::from("do_lua_checksum"), Value::Bool(false));
    settings.insert(
        String::from("deny_login_on_overloaded_server"),
        Value::Bool(false),
    );
    settings.insert(String::from("steam_vac"), Value::Bool(false));
    settings.insert(
        String::from("spawn_point"),
        Value::String(String::from("10635,9342,0")),
    );
    settings.insert(String::from("pvp"), Value::Bool(false));
    settings.insert(String::from("safety_system"), Value::Bool(false));
    settings.insert(String::from("show_safety"), Value::Bool(false));
    settings.insert(String::from("sleep_allowed"), Value::Bool(true));
    settings.insert(String::from("sleep_needed"), Value::Bool(true));
    settings.insert(String::from("player_respawn_with_self"), Value::Bool(true));
    settings.insert(String::from("player_respawn_with_other"), Value::Bool(true));
    settings.insert(
        String::from("admin_username"),
        Value::String(String::from("opsadmin")),
    );
    settings.insert(
        String::from("admin_password"),
        Value::String(String::from("ops-admin-safe")),
    );
    settings.insert(
        String::from("rcon_password"),
        Value::String(String::from("pz-rcon-safe")),
    );
    settings.insert(String::from("memory_gb"), Value::from(6));
    settings.insert(String::from("ping_limit"), Value::from(250));
    settings.insert(String::from("voice_enable"), Value::Bool(false));
    settings.insert(String::from("voice_min_distance"), Value::from(8));
    settings.insert(String::from("voice_max_distance"), Value::from(120));
    settings.insert(String::from("use_tcp_for_map_downloads"), Value::Bool(true));
    settings.insert(
        String::from("map_name"),
        Value::String(String::from(
            "Muldraugh, KY\nRavenCreek\nravencreek\nWest Point, KY",
        )),
    );
    settings.insert(
            String::from("workshop_items"),
            Value::String(String::from(
                "workshop-1234567890\nhttps://steamcommunity.com/sharedfiles/filedetails/?id=9876543210\n1234567890\nbad",
            )),
        );
    settings.insert(
        String::from("mods"),
        Value::String(String::from("BaseMod\nSafehouseTools;baseMod\nMapHelper")),
    );

    let mut ports = details.ports.clone();
    for port in &mut ports {
        match port.name.as_str() {
            "game" => port.port = 17261,
            "direct" => port.port = 17262,
            "rcon" => port.port = 28015,
            _ => {}
        }
    }

    Ok(UpdateInstanceInput {
        id: details.summary.id.clone(),
        bind_ip: String::from("127.0.0.1"),
        auto_backup_on_stop: details.auto_backup_on_stop,
        backup_retention_count: details.backup_retention_count,
        settings_json: serde_json::to_string_pretty(&Value::Object(settings))?,
        ports,
    })
}

pub(super) fn corekeeper_access_update(
    details: &InstanceDetails,
) -> Result<UpdateInstanceInput, Box<dyn std::error::Error>> {
    let mut settings =
        serde_json::from_str::<serde_json::Map<String, Value>>(&details.settings_json)?;
    settings.insert(
        String::from("server_name"),
        Value::String(String::from("Command Access Core Keeper")),
    );
    settings.insert(
        String::from("game_id"),
        Value::String(String::from("CoreKeeperRelay12345")),
    );
    settings.insert(String::from("max_players"), Value::from(16));
    settings.insert(String::from("world_index"), Value::from(3));
    settings.insert(String::from("season_override"), Value::from(6));
    settings.insert(String::from("direct_connection_enabled"), Value::Bool(true));
    settings.insert(
        String::from("join_password"),
        Value::String(String::from("corekeeper-safe-pass")),
    );
    settings.insert(String::from("allowed_platform_code"), Value::from(4));
    settings.insert(
        String::from("admin_list"),
        Value::String(String::from(
            "76561198077777777\n76561198000000000\n76561198077777777\ninvalid\n#comment",
        )),
    );
    settings.insert(
        String::from("ban_list"),
        Value::String(String::from(
            "76561198011111111\n76561198011111111\nnot-a-steam-id",
        )),
    );

    Ok(UpdateInstanceInput {
        id: details.summary.id.clone(),
        bind_ip: String::from("127.0.0.1"),
        auto_backup_on_stop: details.auto_backup_on_stop,
        backup_retention_count: details.backup_retention_count,
        settings_json: serde_json::to_string_pretty(&Value::Object(settings))?,
        ports: details.ports.clone(),
    })
}

pub(super) fn valheim_access_update(
    details: &InstanceDetails,
) -> Result<UpdateInstanceInput, Box<dyn std::error::Error>> {
    let mut settings =
        serde_json::from_str::<serde_json::Map<String, Value>>(&details.settings_json)?;
    settings.insert(
        String::from("server_name"),
        Value::String(String::from("Command Access Valheim")),
    );
    settings.insert(
        String::from("world_name"),
        Value::String(String::from("OpsAccessWorld")),
    );
    settings.insert(
        String::from("server_password"),
        Value::String(String::from("raid-safe-pass")),
    );
    settings.insert(String::from("public_server"), Value::from(0));
    settings.insert(String::from("crossplay_enabled"), Value::Bool(true));
    settings.insert(
        String::from("admin_list"),
        Value::String(String::from("76561198000000001\n76561198000000002")),
    );
    settings.insert(
        String::from("banned_list"),
        Value::String(String::from("76561198000000003")),
    );
    settings.insert(
        String::from("permitted_list"),
        Value::String(String::from("76561198000000001\n76561198000000004")),
    );

    Ok(UpdateInstanceInput {
        id: details.summary.id.clone(),
        bind_ip: String::from("127.0.0.1"),
        auto_backup_on_stop: details.auto_backup_on_stop,
        backup_retention_count: details.backup_retention_count,
        settings_json: serde_json::to_string_pretty(&Value::Object(settings))?,
        ports: details.ports.clone(),
    })
}

pub(super) fn sevendaystodie_access_update(
    details: &InstanceDetails,
) -> Result<UpdateInstanceInput, Box<dyn std::error::Error>> {
    let mut settings =
        serde_json::from_str::<serde_json::Map<String, Value>>(&details.settings_json)?;
    settings.insert(
        String::from("server_name"),
        Value::String(String::from("Command Access 7DTD")),
    );
    settings.insert(
        String::from("server_description"),
        Value::String(String::from("Command-lane managed 7 Days to Die server")),
    );
    settings.insert(
        String::from("server_password"),
        Value::String(String::from("7dtd-safe-pass")),
    );
    settings.insert(
        String::from("world_name"),
        Value::String(String::from("OpsAccess7DTD")),
    );
    settings.insert(String::from("visibility"), Value::from(2));
    settings.insert(String::from("max_players"), Value::from(10));
    settings.insert(String::from("server_allow_crossplay"), Value::Bool(false));
    settings.insert(String::from("ignore_eos_sanctions"), Value::Bool(true));
    settings.insert(String::from("telnet_failed_login_limit"), Value::from(3));
    settings.insert(
        String::from("telnet_failed_logins_blocktime"),
        Value::from(30),
    );
    settings.insert(String::from("max_chunk_age"), Value::from(14));
    settings.insert(String::from("save_data_limit"), Value::from(2048));
    settings.insert(
        String::from("sandbox_code"),
        Value::String(String::from("AAAJABJACJADJARFBNC")),
    );
    settings.insert(String::from("allow_spawn_near_friend"), Value::from(1));
    settings.insert(String::from("camera_restriction_mode"), Value::from(1));
    settings.insert(String::from("max_queued_mesh_layers"), Value::from(750));
    settings.insert(String::from("web_dashboard_enabled"), Value::Bool(true));
    settings.insert(
        String::from("web_dashboard_url"),
        Value::String(String::from("https://ops.example.com/7dtd")),
    );
    settings.insert(String::from("enable_map_rendering"), Value::Bool(true));
    settings.insert(String::from("telnet_enabled"), Value::Bool(true));
    settings.insert(
        String::from("telnet_password"),
        Value::String(String::from("telnet-safe-pass")),
    );
    settings.insert(
        String::from("admin_users"),
        json!([
            {
                "steam_id": "76561198077777777",
                "name": "Host Lead",
                "permission_level": 0
            },
            {
                "steam_id": "76561198000000000",
                "name": "Ops \"Two\"",
                "permission_level": 5
            }
        ]),
    );
    settings.insert(
        String::from("admin_groups"),
        json!([
            {
                "steam_id": "103582791434672565",
                "name": "Steam Universe",
                "permission_level_default": 1000,
                "permission_level_mod": 0
            }
        ]),
    );
    settings.insert(
        String::from("whitelist_users"),
        json!([
            {
                "steam_id": "76561198011111111",
                "name": "Trusted Friend"
            }
        ]),
    );
    settings.insert(
        String::from("whitelist_groups"),
        json!([
            {
                "steam_id": "103582791434672566",
                "name": "Weekend Survivors"
            }
        ]),
    );
    settings.insert(
        String::from("blacklist_entries"),
        json!([
            {
                "steam_id": "76561198033333333",
                "name": "Raider",
                "unbandate": "2025-01-01",
                "reason": "Griefing & spam"
            }
        ]),
    );
    settings.insert(
        String::from("command_permissions"),
        json!([
            {
                "cmd": "help",
                "permission_level": 500
            },
            {
                "cmd": "listplayerids",
                "permission_level": 1000
            },
            {
                "cmd": "say",
                "permission_level": 0
            }
        ]),
    );

    let mut ports = details.ports.clone();
    for port in &mut ports {
        match port.name.as_str() {
            "game_udp" | "game_tcp" => port.port = 27900,
            "web_dashboard" => port.port = 18080,
            "telnet" => port.port = 18081,
            _ => {}
        }
    }

    Ok(UpdateInstanceInput {
        id: details.summary.id.clone(),
        bind_ip: String::from("127.0.0.1"),
        auto_backup_on_stop: details.auto_backup_on_stop,
        backup_retention_count: details.backup_retention_count,
        settings_json: serde_json::to_string_pretty(&Value::Object(settings))?,
        ports,
    })
}

pub(super) fn vrising_access_update(
    details: &InstanceDetails,
) -> Result<UpdateInstanceInput, Box<dyn std::error::Error>> {
    let mut settings =
        serde_json::from_str::<serde_json::Map<String, Value>>(&details.settings_json)?;
    settings.insert(
        String::from("server_name"),
        Value::String(String::from("Command Access V Rising")),
    );
    settings.insert(
        String::from("server_description"),
        Value::String(String::from("Command-lane managed V Rising room.")),
    );
    settings.insert(String::from("max_players"), Value::from(20));
    settings.insert(String::from("max_admins"), Value::from(6));
    settings.insert(
        String::from("server_password"),
        Value::String(String::from("vrising-safe-pass")),
    );
    settings.insert(
        String::from("save_name"),
        Value::String(String::from("OpsRealm")),
    );
    settings.insert(String::from("hide_ip_address"), Value::Bool(true));
    settings.insert(String::from("list_on_steam"), Value::Bool(true));
    settings.insert(String::from("list_on_eos"), Value::Bool(true));
    settings.insert(String::from("server_fps"), Value::from(45));
    settings.insert(String::from("admin_only_debug_events"), Value::Bool(false));
    settings.insert(String::from("api_enabled"), Value::Bool(true));
    settings.insert(String::from("rcon_enabled"), Value::Bool(true));
    settings.insert(
        String::from("rcon_password"),
        Value::String(String::from("vrising-rcon-safe")),
    );
    settings.insert(
        String::from("admin_list"),
        Value::String(String::from("76561198000000001\n76561198000000002")),
    );
    settings.insert(
        String::from("ban_list"),
        Value::String(String::from("76561198000000003")),
    );

    Ok(UpdateInstanceInput {
        id: details.summary.id.clone(),
        bind_ip: String::from("127.0.0.1"),
        auto_backup_on_stop: details.auto_backup_on_stop,
        backup_retention_count: details.backup_retention_count,
        settings_json: serde_json::to_string_pretty(&Value::Object(settings))?,
        ports: details.ports.clone(),
    })
}

pub(super) fn player_snapshot_fixture_instance(
    module_id: &str,
    status: InstanceStatus,
) -> InstanceDetails {
    InstanceDetails {
        summary: InstanceSummary {
            id: String::from("fixture-instance"),
            name: String::from("Fixture Instance"),
            module_id: String::from(module_id),
            active_process_count: usize::from(matches!(&status, InstanceStatus::Running)),
            status,
            bind_ip: String::from("0.0.0.0"),
            port_count: 1,
            autostart: false,
        },
        config_file_path: String::from("D:/fixture/config/instance.json"),
        saves_path: String::from("D:/fixture/config/clusters/main"),
        backup_uses_declared_saves_path: true,
        auto_backup_on_stop: false,
        backup_retention_count: 3,
        settings_json: String::from("{}"),
        ports: Vec::new(),
        active_run: None,
    }
}

pub(super) fn a2s_player_query_spec(port_names: &[&str]) -> ModulePlayerQuerySpec {
    ModulePlayerQuerySpec {
        protocol: String::from("a2s_info"),
        port_names: port_names
            .iter()
            .map(|port_name| String::from(*port_name))
            .collect(),
    }
}

pub(super) fn minecraft_player_query_spec(port_names: &[&str]) -> ModulePlayerQuerySpec {
    ModulePlayerQuerySpec {
        protocol: String::from("minecraft_query"),
        port_names: port_names
            .iter()
            .map(|port_name| String::from(*port_name))
            .collect(),
    }
}

#[test]
pub(super) fn read_latest_runtime_window_suppression_attempt_from_log_prefers_latest_matching_instance()
 {
    let root = temp_test_dir("runtime-window-log");
    let log_path = root.join("desktop-app.log");
    fs::write(
        &log_path,
        [
            json!({
                "ts_unix_ms": 101_u128,
                "level": "info",
                "action": RUNTIME_WINDOW_MANUAL_SUPPRESSION_ACTION,
                "message": "older result",
                "context": {
                    "instance_id": "fixture-instance",
                    "visible_window_count_before": 2,
                    "suppressed_window_count": 1,
                    "remaining_visible_window_count": 1,
                    "inspected_process_count": 2
                }
            })
            .to_string(),
            json!({
                "ts_unix_ms": 202_u128,
                "level": "info",
                "action": RUNTIME_WINDOW_AUTO_SUPPRESSION_ACTION,
                "message": "different instance",
                "context": {
                    "instance_id": "other-instance",
                    "visible_window_count_before": 3,
                    "suppressed_window_count": 3,
                    "remaining_visible_window_count": 0,
                    "inspected_process_count": 2
                }
            })
            .to_string(),
            json!({
                "ts_unix_ms": 303_u128,
                "level": "info",
                "action": RUNTIME_WINDOW_AUTO_SUPPRESSION_ACTION,
                "message": "latest result",
                "context": {
                    "instance_id": "fixture-instance",
                    "visible_window_count_before": 1,
                    "suppressed_window_count": 1,
                    "remaining_visible_window_count": 0,
                    "inspected_process_count": 2
                }
            })
            .to_string(),
        ]
        .join("\n"),
    )
    .expect("write desktop app log");

    let result =
        read_latest_runtime_window_suppression_attempt_from_log(&log_path, "fixture-instance")
            .expect("latest suppression result");

    assert_eq!(result.instance_id, "fixture-instance");
    assert_eq!(result.source, "automatic");
    assert_eq!(result.status, "clear");
    assert_eq!(result.attempted_at_unix_ms, 303);
    assert_eq!(result.visible_window_count_before, 1);
    assert_eq!(result.suppressed_window_count, 1);
    assert_eq!(result.remaining_visible_window_count, 0);
    assert_eq!(result.inspected_process_count, 2);
    assert_eq!(result.summary, "latest result");

    let _ = fs::remove_dir_all(root);
}

#[test]
pub(super) fn read_latest_runtime_window_suppression_attempt_from_log_returns_none_without_match() {
    let root = temp_test_dir("runtime-window-log-none");
    let log_path = root.join("desktop-app.log");
    fs::write(
        &log_path,
        json!({
            "ts_unix_ms": 101_u128,
            "level": "info",
            "action": "instance.runtime_command.sent",
            "message": "not a suppression event",
            "context": {
                "instance_id": "fixture-instance"
            }
        })
        .to_string(),
    )
    .expect("write desktop app log");

    assert!(
        read_latest_runtime_window_suppression_attempt_from_log(&log_path, "fixture-instance")
            .is_none()
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
pub(super) fn read_latest_runtime_window_suppression_attempt_from_log_reads_failure_attempts() {
    let root = temp_test_dir("runtime-window-log-failure");
    let log_path = root.join("desktop-app.log");
    fs::write(
        &log_path,
        [
            json!({
                "ts_unix_ms": 101_u128,
                "level": "info",
                "action": RUNTIME_WINDOW_MANUAL_SUPPRESSION_ACTION,
                "message": "older result",
                "context": {
                    "instance_id": "fixture-instance",
                    "visible_window_count_before": 1,
                    "suppressed_window_count": 1,
                    "remaining_visible_window_count": 0,
                    "inspected_process_count": 1
                }
            })
            .to_string(),
            json!({
                "ts_unix_ms": 202_u128,
                "level": "error",
                "action": RUNTIME_WINDOW_AUTO_SUPPRESSION_FAILED_ACTION,
                "message": "Win32 inspection failed",
                "context": {
                    "instance_id": "fixture-instance",
                    "process_count": 3
                }
            })
            .to_string(),
        ]
        .join("\n"),
    )
    .expect("write desktop app log");

    let result =
        read_latest_runtime_window_suppression_attempt_from_log(&log_path, "fixture-instance")
            .expect("latest suppression attempt");

    assert_eq!(result.instance_id, "fixture-instance");
    assert_eq!(result.source, "automatic");
    assert_eq!(result.status, "failed");
    assert_eq!(result.attempted_at_unix_ms, 202);
    assert_eq!(result.inspected_process_count, 3);
    assert_eq!(result.visible_window_count_before, 0);
    assert_eq!(result.suppressed_window_count, 0);
    assert_eq!(result.remaining_visible_window_count, 0);
    assert_eq!(result.summary, "Win32 inspection failed");

    let _ = fs::remove_dir_all(root);
}

#[test]
pub(super) fn player_capacity_supports_abiotic_factor_setting_key() {
    let capacity = extract_instance_player_capacity(
        r#"{
                "server_name": "Abiotic Runtime",
                "max_server_players": 6
            }"#,
    );

    assert_eq!(capacity, Some(6));
}

#[test]
pub(super) fn module_runtime_capability_loader_reports_invalid_descriptor() {
    let root = temp_test_dir("invalid-runtime-capability-module");
    let module_root = root.join("broken");
    fs::create_dir_all(&module_root).unwrap();
    fs::write(module_root.join("module.toml"), "[module\n").unwrap();

    let error = load_module_runtime_capability_map(&root).unwrap_err();

    assert!(error.contains("Failed to load module runtime capabilities"));
    let _ = fs::remove_dir_all(root);
}

#[test]
pub(super) fn resolve_player_query_target_prefers_abiotic_query_udp_port() {
    let mut instance = player_snapshot_fixture_instance("abioticfactor", InstanceStatus::Running);
    instance.summary.bind_ip = String::from("0.0.0.0");
    instance.ports = vec![
        app_core::PortBinding {
            name: String::from("game"),
            protocol: String::from("udp"),
            port: 7777,
        },
        app_core::PortBinding {
            name: String::from("query"),
            protocol: String::from("udp"),
            port: 27015,
        },
    ];

    assert_eq!(
        resolve_player_query_target(&instance, Some(&a2s_player_query_spec(&["query"]))),
        Some((String::from("127.0.0.1"), 27015))
    );
}

#[test]
pub(super) fn resolve_player_query_target_accepts_minecraft_query_udp_port() {
    let mut instance = player_snapshot_fixture_instance("minecraft", InstanceStatus::Running);
    instance.summary.bind_ip = String::from("0.0.0.0");
    instance.ports = vec![
        app_core::PortBinding {
            name: String::from("game"),
            protocol: String::from("tcp"),
            port: 25565,
        },
        app_core::PortBinding {
            name: String::from("query"),
            protocol: String::from("udp"),
            port: 25565,
        },
    ];

    assert_eq!(
        resolve_player_query_target(&instance, Some(&minecraft_player_query_spec(&["query"]))),
        Some((String::from("127.0.0.1"), 25565))
    );
}

#[test]
pub(super) fn parse_a2s_info_payload_reads_player_counts() {
    let payload = [
        0x11, b'A', b'b', b'i', b'o', 0x00, b'L', b'a', b'b', 0x00, b'F', b'a', b'c', 0x00, b'S',
        b't', b'e', b'a', b'm', 0x00, 0xde, 0xad, 4, 6,
    ];

    let queried = parse_a2s_info_payload(&payload).expect("payload should parse");
    assert_eq!(queried.current_players, 4);
    assert_eq!(queried.max_players, 6);
}

#[test]
pub(super) fn parse_minecraft_query_challenge_response_reads_token() {
    let response = [0x09, 1, 2, 3, 4, b'-', b'1', b'2', b'3', 0];

    let challenge = parse_minecraft_query_challenge_response(&response, [1, 2, 3, 4])
        .expect("challenge response should parse");

    assert_eq!(challenge, -123);
}

#[test]
pub(super) fn parse_minecraft_query_basic_response_reads_player_counts() {
    let response = [
        b"\x00\x01\x02\x03\x04Lan world\0SMP\0world\x007\x0032\0".as_slice(),
        &[0xdd, 0x63],
        b"127.0.0.1\0".as_slice(),
    ]
    .concat();

    let queried = parse_minecraft_query_basic_response(&response, [1, 2, 3, 4])
        .expect("basic query response should parse");

    assert_eq!(queried.current_players, 7);
    assert_eq!(queried.max_players, 32);
}

#[test]
pub(super) fn resolve_dontstarve_cluster_root_accepts_cluster_and_config_paths() {
    let root = temp_test_dir("resolve");
    let cluster_root = root.join("config").join("clusters").join("main");
    let save_root = cluster_root.join("Master").join("save");
    fs::create_dir_all(save_root.join("session").join("TEST-SESSION")).unwrap();
    fs::write(
        save_root.join("shardindex"),
        "return { session_id = 'TEST-SESSION' }",
    )
    .unwrap();
    fs::write(
        save_root
            .join("session")
            .join("TEST-SESSION")
            .join("0000000001"),
        b"\x00synthetic DST world snapshot",
    )
    .unwrap();
    fs::write(
        save_root
            .join("session")
            .join("TEST-SESSION")
            .join("0000000001.meta"),
        b"\x00synthetic DST snapshot metadata",
    )
    .unwrap();

    assert_eq!(
        resolve_dontstarve_cluster_source_root(&cluster_root),
        Some(cluster_root.clone())
    );
    assert_eq!(
        resolve_dontstarve_cluster_source_root(&root.join("config")),
        Some(cluster_root.clone())
    );
    assert_eq!(
        resolve_dontstarve_cluster_source_root(&cluster_root.join("Master")),
        Some(cluster_root.clone())
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
pub(super) fn clear_directory_contents_except_preserves_managed_shard_files() {
    let root = temp_test_dir("clear");
    let shard_root = root.join("Master");
    fs::create_dir_all(shard_root.join("save")).unwrap();
    fs::write(shard_root.join("server.ini"), "managed").unwrap();
    fs::write(shard_root.join("worldgenoverride.lua"), "managed").unwrap();
    fs::write(shard_root.join("modoverrides.lua"), "managed").unwrap();
    fs::write(shard_root.join("save").join("index"), "world").unwrap();
    fs::write(shard_root.join("session.json"), "world").unwrap();

    clear_directory_contents_except(
        &shard_root,
        &["server.ini", "worldgenoverride.lua", "modoverrides.lua"],
    )
    .unwrap();

    assert!(shard_root.join("server.ini").exists());
    assert!(shard_root.join("worldgenoverride.lua").exists());
    assert!(shard_root.join("modoverrides.lua").exists());
    assert!(!shard_root.join("save").exists());
    assert!(!shard_root.join("session.json").exists());

    let _ = fs::remove_dir_all(&root);
}

#[test]
pub(super) fn copy_directory_contents_skipping_root_files_keeps_nested_world_data() {
    let root = temp_test_dir("copy");
    let source = root.join("source");
    let target = root.join("target");
    fs::create_dir_all(source.join("save")).unwrap();
    fs::create_dir_all(source.join("session")).unwrap();
    fs::create_dir_all(&target).unwrap();
    fs::write(source.join("server.ini"), "managed").unwrap();
    fs::write(source.join("worldgenoverride.lua"), "managed").unwrap();
    fs::write(source.join("save").join("index"), "world").unwrap();
    fs::write(source.join("session").join("abc"), "world").unwrap();
    fs::write(source.join("cluster-meta.json"), "world").unwrap();

    let stats = copy_directory_contents_skipping_root_files(
        &source,
        &target,
        &["server.ini", "worldgenoverride.lua", "modoverrides.lua"],
    )
    .unwrap();

    assert!(!target.join("server.ini").exists());
    assert!(!target.join("worldgenoverride.lua").exists());
    assert!(target.join("save").join("index").exists());
    assert!(target.join("session").join("abc").exists());
    assert!(target.join("cluster-meta.json").exists());
    assert_eq!(stats.file_count, 3);

    let _ = fs::remove_dir_all(&root);
}

fn manual_mod_test_target(target_path: PathBuf, accepts: &[&str]) -> ResolvedManualModTarget {
    ResolvedManualModTarget {
        instance_id: String::from("manual-mod-test"),
        module_id: String::from("manual-mod-test"),
        source_label: String::from("Manual Mod Test"),
        target_label: String::from("Mods"),
        target_path,
        accepts: accepts.iter().map(|value| String::from(*value)).collect(),
        id_strategy: None,
    }
}

fn write_manual_mod_test_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let archive_file = fs::File::create(path).unwrap();
    let mut archive = zip::ZipWriter::new(archive_file);
    for (entry_path, contents) in entries {
        archive
            .start_file(
                *entry_path,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
        archive.write_all(contents).unwrap();
    }
    archive.finish().unwrap();
}

fn manual_mod_transaction_directories(root: &Path) -> Vec<PathBuf> {
    fs::read_dir(root)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(".langame-mod-transaction-"))
        })
        .collect()
}

fn create_manual_mod_test_file_symlink(source: &Path, link: &Path) -> bool {
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(source, link).is_ok()
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, link).is_ok()
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = (source, link);
        false
    }
}

#[test]
pub(super) fn manual_mod_source_batch_copies_folder_under_target() {
    let root = temp_test_dir("manual-mod-stage");
    let source = root.join("DownloadedMod");
    let target = root.join("Mods");
    fs::create_dir_all(source.join("Config")).unwrap();
    fs::create_dir_all(&target).unwrap();
    fs::write(source.join("ModInfo.xml"), "<xml/>").unwrap();
    fs::write(source.join("Config").join("items.xml"), "<items/>").unwrap();

    let staged = stage_manual_mod_sources(
        manual_mod_test_target(target.clone(), &["folder"]),
        vec![source],
    )
    .unwrap();

    assert_eq!(staged.copied_file_count, 2);
    assert!(PathBuf::from(&staged.items[0].target_path).ends_with("DownloadedMod"));
    assert!(target.join("DownloadedMod").join("ModInfo.xml").exists());
    assert!(
        target
            .join("DownloadedMod")
            .join("Config")
            .join("items.xml")
            .exists()
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
pub(super) fn manual_mod_sources_reject_unsupported_type_before_writing() {
    let root = temp_test_dir("manual-mod-accepts");
    let supported = root.join("server.jar");
    let unsupported = root.join("notes.txt");
    let target = root.join("Mods");
    fs::write(&supported, "jar").unwrap();
    fs::write(&unsupported, "text").unwrap();
    let manual_target = manual_mod_test_target(target.clone(), &["jar"]);

    let error = stage_manual_mod_sources(manual_target, vec![supported, unsupported])
        .expect_err("unsupported file type should reject the batch");

    assert!(error.contains("unsupported type `txt`"));
    assert!(!target.exists());
    let _ = fs::remove_dir_all(&root);
}

#[test]
pub(super) fn manual_mod_archive_limits_reject_unsafe_metrics() {
    let file_count_error = validate_manual_mod_archive_metrics(usize::MAX, 1, 1)
        .expect_err("file count should be bounded");
    let size_error = validate_manual_mod_archive_metrics(1, u64::MAX, u64::MAX)
        .expect_err("uncompressed bytes should be bounded");
    let ratio_error = validate_manual_mod_archive_metrics(1, 1, 201)
        .expect_err("compression ratio should be bounded");

    assert!(file_count_error.contains("file count"));
    assert!(size_error.contains("uncompressed size"));
    assert!(ratio_error.contains("compression ratio"));
}

#[test]
pub(super) fn manual_mod_archive_rejects_parent_traversal_before_writing() {
    let root = temp_test_dir("manual-mod-zip-traversal");
    let archive_path = root.join("unsafe.zip");
    let target = root.join("Mods");
    write_manual_mod_test_zip(&archive_path, &[("../escaped.dll", b"unsafe")]);

    let error = stage_manual_mod_sources(
        manual_mod_test_target(target.clone(), &["zip"]),
        vec![archive_path],
    )
    .expect_err("parent traversal should be rejected");

    assert!(error.contains("unsafe archive entry"));
    assert!(!target.exists());
    assert!(!root.join("escaped.dll").exists());
    let _ = fs::remove_dir_all(&root);
}

#[test]
pub(super) fn manual_mod_archive_extracts_after_safety_preflight() {
    let root = temp_test_dir("manual-mod-zip-safe");
    let archive_path = root.join("safe.zip");
    let target = root.join("Mods");
    write_manual_mod_test_zip(
        &archive_path,
        &[("Plugin/config.json", br#"{"enabled":true}"#)],
    );

    let staged = stage_manual_mod_sources(
        manual_mod_test_target(target.clone(), &["zip"]),
        vec![archive_path],
    )
    .unwrap();

    assert_eq!(staged.copied_file_count, 1);
    assert_eq!(staged.copied_total_bytes, 16);
    assert!(target.join("Plugin").join("config.json").exists());
    let _ = fs::remove_dir_all(&root);
}

#[test]
pub(super) fn manual_mod_batch_failure_leaves_existing_target_unchanged() {
    let root = temp_test_dir("manual-mod-batch-rollback");
    let target = root.join("Mods");
    let valid_source = root.join("valid.jar");
    let invalid_archive = root.join("invalid.zip");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("existing.jar"), "existing").unwrap();
    fs::write(&valid_source, "valid").unwrap();
    fs::write(&invalid_archive, "not a zip archive").unwrap();

    let error = stage_manual_mod_sources(
        manual_mod_test_target(target.clone(), &["jar", "zip"]),
        vec![valid_source, invalid_archive],
    )
    .expect_err("the whole source batch should fail");

    assert!(error.contains("mod archive"));
    assert_eq!(
        fs::read_to_string(target.join("existing.jar")).unwrap(),
        "existing"
    );
    assert!(!target.join("valid.jar").exists());
    assert!(manual_mod_transaction_directories(&root).is_empty());
    let _ = fs::remove_dir_all(&root);
}

#[test]
pub(super) fn manual_mod_archive_bounds_actual_output_when_size_is_forged() {
    let root = temp_test_dir("manual-mod-zip-forged-size");
    let archive_path = root.join("forged.zip");
    let target = root.join("Mods");
    let archive_file = fs::File::create(&archive_path).unwrap();
    let mut archive = zip::ZipWriter::new(archive_file);
    archive
        .start_file(
            "payload.dll",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
    archive.write_all(&vec![b'A'; 16 * 1024]).unwrap();
    archive.finish().unwrap();

    let mut archive_bytes = fs::read(&archive_path).unwrap();
    let local_header = archive_bytes
        .windows(4)
        .position(|window| window == b"PK\x03\x04")
        .unwrap();
    let central_header = archive_bytes
        .windows(4)
        .rposition(|window| window == b"PK\x01\x02")
        .unwrap();
    archive_bytes[local_header + 22..local_header + 26].copy_from_slice(&1u32.to_le_bytes());
    archive_bytes[central_header + 24..central_header + 28].copy_from_slice(&1u32.to_le_bytes());
    fs::write(&archive_path, archive_bytes).unwrap();

    let error = stage_manual_mod_sources(
        manual_mod_test_target(target.clone(), &["zip"]),
        vec![archive_path],
    )
    .expect_err("actual output must not exceed the declared entry size");

    assert!(error.contains("exceeded its declared or cumulative extraction limit"));
    assert!(!target.exists());
    assert!(manual_mod_transaction_directories(&root).is_empty());
    let _ = fs::remove_dir_all(&root);
}

#[test]
pub(super) fn manual_mod_archive_rejects_case_aliased_duplicate_paths() {
    let root = temp_test_dir("manual-mod-zip-duplicate");
    let archive_path = root.join("duplicate.zip");
    let target = root.join("Mods");
    write_manual_mod_test_zip(
        &archive_path,
        &[("Plugin.dll", b"first"), ("plugin.dll", b"second")],
    );

    let error = stage_manual_mod_sources(
        manual_mod_test_target(target.clone(), &["zip"]),
        vec![archive_path],
    )
    .expect_err("Windows path aliases must be rejected");

    assert!(error.contains("duplicate or Windows-aliased path"));
    assert!(!target.exists());
    let _ = fs::remove_dir_all(&root);
}

#[test]
pub(super) fn manual_mod_archive_rejects_windows_special_paths() {
    let root = temp_test_dir("manual-mod-zip-windows-paths");
    for (index, entry_path) in ["Plugin.dll:payload", "CON.txt", "Folder./file.dll"]
        .into_iter()
        .enumerate()
    {
        let archive_path = root.join(format!("unsafe-{index}.zip"));
        let target = root.join(format!("Mods-{index}"));
        write_manual_mod_test_zip(&archive_path, &[(entry_path, b"unsafe")]);

        let error = stage_manual_mod_sources(
            manual_mod_test_target(target.clone(), &["zip"]),
            vec![archive_path],
        )
        .expect_err("Windows-special paths must be rejected");

        assert!(error.contains("Windows"));
        assert!(!target.exists());
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
pub(super) fn manual_mod_folder_source_rejects_symbolic_links() {
    let root = temp_test_dir("manual-mod-folder-link");
    let source = root.join("DownloadedMod");
    let target = root.join("Mods");
    let external_file = root.join("external.dll");
    fs::create_dir_all(&source).unwrap();
    fs::write(&external_file, "external").unwrap();
    if !create_manual_mod_test_file_symlink(&external_file, &source.join("linked.dll")) {
        let _ = fs::remove_dir_all(&root);
        return;
    }

    let error = stage_manual_mod_sources(
        manual_mod_test_target(target.clone(), &["folder"]),
        vec![source],
    )
    .expect_err("source links must not be followed");

    assert!(error.contains("symbolic link or reparse point"));
    assert!(!target.exists());
    let _ = fs::remove_dir_all(&root);
}

#[test]
pub(super) fn manual_mod_target_tree_rejects_symbolic_links() {
    let root = temp_test_dir("manual-mod-target-link");
    let target = root.join("Mods");
    let source = root.join("source.jar");
    let external_file = root.join("external.jar");
    fs::create_dir_all(&target).unwrap();
    fs::write(&source, "source").unwrap();
    fs::write(&external_file, "external").unwrap();
    if !create_manual_mod_test_file_symlink(&external_file, &target.join("linked.jar")) {
        let _ = fs::remove_dir_all(&root);
        return;
    }

    let error = stage_manual_mod_sources(
        manual_mod_test_target(target.clone(), &["jar"]),
        vec![source],
    )
    .expect_err("target links must be rejected");

    assert!(error.contains("symbolic link or reparse point"));
    assert_eq!(fs::read_to_string(&external_file).unwrap(), "external");
    let _ = fs::remove_dir_all(&root);
}

#[test]
pub(super) fn downloaded_workshop_items_stage_into_manual_mod_target() {
    let root = temp_test_dir("workshop-manual-stage");
    let workshop_item = root
        .join("steamapps")
        .join("workshop")
        .join("content")
        .join("393380")
        .join("1205163003");
    let target = root.join("SquadGame").join("Plugins").join("Mods");
    fs::create_dir_all(workshop_item.join("Content")).unwrap();
    fs::create_dir_all(&target).unwrap();
    fs::write(workshop_item.join("Content").join("mod.pak"), "pak").unwrap();

    let result = app_steamcmd::SteamWorkshopDownloadResult {
        consumer_app_id: 393380,
        install_root: root.to_string_lossy().into_owned(),
        workshop_root: root
            .join("steamapps")
            .join("workshop")
            .join("content")
            .join("393380")
            .to_string_lossy()
            .into_owned(),
        items: vec![app_steamcmd::SteamWorkshopDownloadItemResult {
            item_id: String::from("1205163003"),
            expected_path: workshop_item.to_string_lossy().into_owned(),
            expected_path_exists: true,
        }],
        output_excerpt: String::from("downloaded"),
    };
    let manual_target = ResolvedManualModTarget {
        instance_id: String::from("squad-1"),
        module_id: String::from("squad"),
        source_label: String::from("Steam Workshop"),
        target_label: String::from("SquadGame/Plugins/Mods"),
        target_path: target.clone(),
        accepts: vec![String::from("folder"), String::from("zip")],
        id_strategy: None,
    };

    let staged =
        stage_downloaded_workshop_items_into_manual_target(&result, &manual_target).unwrap();

    assert_eq!(staged.copied_file_count, 1);
    assert!(PathBuf::from(&staged.items[0].target_path).ends_with(Path::new("1205163003")));
    assert!(
        target
            .join("1205163003")
            .join("Content")
            .join("mod.pak")
            .exists()
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
pub(super) fn ensure_launch_plans_ready_blocks_preflight_errors() {
    let mut launch_plans = vec![ProcessLaunchPlan {
        process_key: String::from("main"),
        display_name: String::from("Server"),
        log_path: String::new(),
        launch_plan: LaunchPlan {
            environment: Default::default(),
            instance_id: String::from("palworld-1"),
            instance_name: String::from("Palworld Smoke"),
            module_id: String::from("palworld"),
            install_root: String::from("D:/LanGame/server-files/palworld"),
            working_directory: String::from("D:/LanGame/server-files/palworld/Pal/Binaries/Win64"),
            executable_path: String::from(
                "D:/LanGame/server-files/palworld/Pal/Binaries/Win64/PalServer-Win64-Shipping-Cmd.exe",
            ),
            install_state: InstallState::NotInstalled,
            uses_private_runtime: false,
            executable_exists: false,
            ready_to_launch: false,
            validation_issues: vec![app_core::LaunchValidationIssue {
                code: String::from("launch_executable_missing"),
                severity: String::from("error"),
                message: String::from(
                    "Launch executable is missing: D:/LanGame/server-files/palworld/Pal/Binaries/Win64/PalServer-Win64-Shipping-Cmd.exe. Install or repair the game files first.",
                ),
                path: Some(String::from(
                    "D:/LanGame/server-files/palworld/Pal/Binaries/Win64/PalServer-Win64-Shipping-Cmd.exe",
                )),
                context: Default::default(),
            }],
            args: Vec::new(),
            command_line: String::from(
                "\"D:/LanGame/server-files/palworld/Pal/Binaries/Win64/PalServer-Win64-Shipping-Cmd.exe\"",
            ),
            window_policy: app_core::ProcessWindowPolicy::Background,
            uses_script_entrypoint: false,
            requires_admin: false,
            host_surface: app_core::ProcessHostSurface::ManagedTerminal,
            host_notes: None,
            performance_policy: RuntimePerformancePolicy::default(),
            performance_preview: app_core::RuntimePerformancePolicyPreview::default(),
        },
    }];

    launch_plans[0].display_name = String::from("Server \"主\"\\log\nname");
    let mut warning = launch_plans[0].launch_plan.validation_issues[0].clone();
    warning.code = String::from("unresolved_launch_args");
    warning.severity = String::from("warning");
    launch_plans[0].launch_plan.validation_issues.push(warning);
    let error = ensure_launch_plans_ready(&launch_plans).expect_err("should block");
    let payload: serde_json::Value = serde_json::from_str(&error).expect("JSON error contract");
    assert_eq!(payload["code"], "launch_preflight_failed");
    assert_eq!(payload["issues"].as_array().unwrap().len(), 1);
    let issue = &payload["issues"][0];
    assert_eq!(issue["process_key"], "main");
    assert_eq!(issue["display_name"], launch_plans[0].display_name);
    assert_eq!(issue["code"], "launch_executable_missing");
    assert_eq!(issue["severity"], "error");
    assert_eq!(issue["path"], launch_plans[0].launch_plan.executable_path);
    assert_eq!(
        issue["message"],
        launch_plans[0].launch_plan.validation_issues[0].message
    );
    assert!(
        payload["message"]
            .as_str()
            .unwrap()
            .contains("Launch preflight failed")
    );
    assert!(
        payload["message"]
            .as_str()
            .unwrap()
            .contains("Install or repair the game files first")
    );

    let port_issue = &mut launch_plans[0].launch_plan.validation_issues[0];
    port_issue.code = String::from("port_binding_unavailable");
    port_issue.path = None;
    port_issue
        .context
        .insert(String::from("address"), String::from("127.0.0.1:8211"));
    let error = ensure_launch_plans_ready(&launch_plans).expect_err("port issue should block");
    let payload: serde_json::Value = serde_json::from_str(&error).expect("JSON error contract");
    assert_eq!(payload["issues"][0]["context"]["address"], "127.0.0.1:8211");
    assert!(payload["issues"][0]["path"].is_null());

    launch_plans[0].launch_plan.validation_issues.remove(0);
    ensure_launch_plans_ready(&launch_plans).expect("warnings alone must not block launch");
    ensure_launch_plans_ready(&[]).expect("empty plans have no blocking errors");
}

#[test]
fn runtime_lifecycle_conflicts_keep_codes_and_original_diagnostics() {
    let instance_id = "server-\"双引号\"\\instance\nline";
    for (conflict, code, reason, diagnostic) in [
        (
            InstanceRunConflict::ActiveRunRecord,
            "instance_already_running",
            Some("active_run_record"),
            "already has an active run record",
        ),
        (
            InstanceRunConflict::TrackedRunning,
            "instance_already_running",
            Some("tracked_running"),
            "is already tracked as running",
        ),
        (
            InstanceRunConflict::NotRunning,
            "instance_not_running",
            None,
            "is not marked as running",
        ),
    ] {
        let error = conflict.into_error(instance_id);
        let payload: serde_json::Value = serde_json::from_str(&error).expect("JSON error contract");
        assert_eq!(payload["code"], code);
        assert_eq!(payload["instance_id"], instance_id);
        assert_eq!(
            payload.get("reason").and_then(|value| value.as_str()),
            reason
        );
        assert_eq!(
            payload["message"],
            format!("instance `{instance_id}` {diagnostic}")
        );
    }
}

fn module_for_start_install_state(install_state: InstallState) -> ModuleDetails {
    ModuleDetails {
        summary: ModuleSummary {
            id: String::from("corekeeper"),
            name: String::from("Core Keeper"),
            version: String::from("0.1.0"),
            description: None,
            steam_app_id: Some(1_963_720),
            install_state,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
        schema_json: None,
        default_ports: Vec::new(),
        install: None,
        process: None,
        workshop: None,
        mods: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
    }
}

#[test]
pub(super) fn ensure_module_installed_for_start_allows_installed_module() {
    let module = module_for_start_install_state(InstallState::Installed);
    assert_eq!(ensure_module_installed_for_start(&module), Ok(()));
}

#[test]
pub(super) fn ensure_module_installed_for_start_reports_each_nonready_state() {
    for (install_state, expected_state) in [
        (InstallState::NotInstalled, "NotInstalled"),
        (InstallState::Installing, "Installing"),
        (InstallState::Incomplete, "Incomplete"),
        (InstallState::Updating, "Updating"),
        (InstallState::Uninstalling, "Uninstalling"),
        (InstallState::Corrupted, "Corrupted"),
    ] {
        let module = module_for_start_install_state(install_state);
        let error = ensure_module_installed_for_start(&module).expect_err("should block");
        let payload: Value = serde_json::from_str(&error).expect("business error JSON");

        assert_eq!(payload["code"], "module_not_ready");
        assert_eq!(payload["module_id"], "corekeeper");
        assert_eq!(payload["module_name"], "Core Keeper");
        assert_eq!(payload["install_state"], expected_state);
        assert_eq!(
            payload["message"],
            format!(
                "module `corekeeper` is not ready to start; current install state is {expected_state}. Finish, repair, or rescan the game install before launching an instance."
            )
        );
    }
}

#[test]
pub(super) fn ensure_module_installed_for_start_uses_effective_preview_state() {
    use crate::commands::commands_runtime_lifecycle::build_instance_launch_preview;

    let root = temp_test_dir("launch-preview-install-state");
    let config_dir = root.join("instances").join("server").join("config");
    let private_root = root.join("instances").join("server").join("runtime");
    let shared_root = root.join("games").join("sevendaystodie");
    let private_executable = private_root.join("7DaysToDieServer.exe");
    let private_marker = private_root.join(".langame-private-runtime");
    fs::create_dir_all(&config_dir).expect("create config directory");
    fs::create_dir_all(&private_root).expect("create private runtime directory");
    fs::write(&private_executable, []).expect("write private executable");
    let settings = app_core::AppSettings {
        archives_root: String::new(),
        servers_root: root.join("instances").to_string_lossy().into_owned(),
        games_root: root.join("games").to_string_lossy().into_owned(),
        modules_root: workspace_root()
            .join("modules")
            .to_string_lossy()
            .into_owned(),
        steamcmd_root: root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let descriptors = discover_modules(Path::new(&settings.modules_root)).expect("module catalog");
    let descriptor = descriptors
        .iter()
        .find(|descriptor| descriptor.summary.id == "sevendaystodie")
        .expect("7 Days to Die module");
    let instance = InstanceDetails {
        summary: InstanceSummary {
            id: String::from("server"),
            name: String::from("Server"),
            module_id: descriptor.summary.id.clone(),
            status: InstanceStatus::Stopped,
            active_process_count: 0,
            bind_ip: String::from("0.0.0.0"),
            port_count: 0,
            autostart: false,
        },
        config_file_path: config_dir
            .join("instance.json")
            .to_string_lossy()
            .into_owned(),
        saves_path: root.join("saves").to_string_lossy().into_owned(),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: false,
        backup_retention_count: 10,
        settings_json: String::from("{}"),
        ports: vec![],
        active_run: None,
    };

    assert!(
        build_instance_launch_preview(&settings, descriptor, &instance).is_err(),
        "a present unmarked runtime must not fall back to the shared package"
    );

    fs::write(&private_marker, b"managed\n").expect("mark private runtime");
    let plan =
        build_instance_launch_preview(&settings, descriptor, &instance).expect("private preview");
    assert_eq!(plan.install_state, InstallState::Installed);
    assert!(plan.uses_private_runtime);
    assert!(plan.ready_to_launch);
    assert_eq!(PathBuf::from(&plan.install_root), private_root);

    fs::remove_file(&private_executable).expect("remove private executable");
    fs::create_dir_all(&shared_root).expect("create shared runtime");
    fs::write(shared_root.join("7DaysToDieServer.exe"), []).expect("write shared executable");
    let plan = build_instance_launch_preview(&settings, descriptor, &instance)
        .expect("damaged private preview");
    assert_eq!(plan.install_state, InstallState::Corrupted);
    assert!(plan.uses_private_runtime);
    assert!(!plan.ready_to_launch);
    assert_eq!(PathBuf::from(&plan.install_root), private_root);

    fs::remove_file(&private_marker).expect("remove private marker");
    assert!(build_instance_launch_preview(&settings, descriptor, &instance).is_err());
    fs::remove_dir_all(&private_root).expect("remove owned fixture runtime");
    assert!(
        build_instance_launch_preview(&settings, descriptor, &instance).is_err(),
        "a missing private runtime must not launch the installed shared executable"
    );

    fs::remove_dir_all(&root).expect("remove launch preview test root");
}

#[test]
pub(super) fn ensure_module_installed_for_start_preserves_escaped_context() {
    let mut module = module_for_start_install_state(InstallState::Corrupted);
    module.summary.id = String::from("server\\runtime\"\n");
    module.summary.name = String::from("Server \"七日杀\"\n第二行");

    let error = ensure_module_installed_for_start(&module).expect_err("should block");
    let payload: Value = serde_json::from_str(&error).expect("business error JSON");

    assert_eq!(payload["code"], "module_not_ready");
    assert_eq!(payload["module_id"], module.summary.id);
    assert_eq!(payload["module_name"], module.summary.name);
    assert_eq!(payload["install_state"], "Corrupted");
    assert_eq!(
        payload["message"],
        format!(
            "module `{}` is not ready to start; current install state is Corrupted. Finish, repair, or rescan the game install before launching an instance.",
            module.summary.id
        )
    );
}

#[tokio::test]
pub(super) async fn start_without_port_remap_rejects_missing_start_required_setting()
-> Result<(), Box<dyn std::error::Error>> {
    let root = temp_test_dir("start-required-setting");
    let workspace = workspace_root();
    let paths = app_storage::StoragePaths {
        app_data_root: root.join("appdata"),
        settings_path: root.join("appdata").join("settings.json"),
        database_path: root.join("appdata").join("db").join("lgs.db"),
        logs_root: root.join("appdata").join("logs"),
        modules_root: root.join("modules"),
        migrations_root: workspace.join("migrations"),
        steamcmd_root: root.join("steamcmd"),
        games_root: root.join("games"),
        instances_root: root.join("instances"),
        archives_root: root.join("instances").join(".trash"),
    };
    fs::create_dir_all(
        paths
            .database_path
            .parent()
            .expect("temporary database parent"),
    )?;
    initialize_database(&paths).await?;

    let module_root = paths.modules_root.join("start-required-fixture");
    fs::create_dir_all(&module_root)?;
    fs::write(
        module_root.join("module.toml"),
        concat!(
            "id = \"start-required-fixture\"\n",
            "name = \"Start-required fixture\"\n",
            "version = \"1.0.0\"\n",
            "[install]\n",
            "shared_game_dir = \"start-required-fixture\"\n",
            "verification_path = \"server.exe\"\n",
            "[process]\n",
            "executable = \"server.exe\"\n",
        ),
    )?;
    fs::write(
        module_root.join("schema.json"),
        json!({
            "type": "object",
            "properties": {
                "owner_id": {
                    "type": "string",
                    "default": "",
                    "x-lsgm-required-before-start": true
                }
            }
        })
        .to_string(),
    )?;
    let install_root = paths.games_root.join("start-required-fixture");
    fs::create_dir_all(&install_root)?;
    fs::write(install_root.join("server.exe"), "fixture executable")?;
    let descriptors = discover_modules(&paths.modules_root)?;
    let descriptor = descriptors
        .first()
        .expect("start-required module descriptor");
    assert_eq!(
        map_module_details_with_install_state(&paths.settings(), descriptor, None)
            .summary
            .install_state,
        InstallState::Installed,
        "fixture must reach startup configuration validation"
    );
    sync_modules(&paths, std::slice::from_ref(descriptor)).await?;
    let created = create_instance(
        &paths,
        descriptor,
        CreateInstanceInput {
            name: String::from("Required-setting startup"),
            module_id: descriptor.summary.id.clone(),
        },
    )
    .await?;
    app_storage::update_instance_ports(&paths, &created.summary.id, &[]).await?;

    let details = read_instance_details(&paths, &created.summary.id).await?;
    let settings = serde_json::from_str::<Value>(&details.settings_json)?;
    assert_eq!(settings["owner_id"], "");
    assert!(details.ports.is_empty());
    assert!(
        app_runtime::remap_taken_port_bindings(&details.summary.bind_ip, &details.ports)?.is_none(),
        "fixture must cover the no-remap startup path"
    );

    let storage = StorageBootstrap {
        settings: paths.settings(),
        storage_status: paths.probe_status(),
        paths,
    };
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    {
        let mut app_state = state.app_state.write().expect("desktop state lock");
        app_state.settings = storage.settings.clone();
        app_state.storage = storage.storage_status.clone();
    }
    let reservation = match state
        .try_reserve_runtime_start(&created.summary.id, "test")
        .expect("runtime start reservation")
    {
        RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
        attempt => panic!("unexpected runtime start reservation: {attempt:?}"),
    };
    let error = start_instance_process_after_reconcile_reserved(
        None,
        &state,
        &storage,
        created.summary.id.clone(),
        "test",
        &reservation,
        Default::default(),
    )
    .await
    .expect_err("missing required owner_id must block startup");

    assert!(
        error.contains("failed to materialize startup configuration"),
        "unexpected startup error: {error}"
    );
    assert!(
        error.contains("owner_id"),
        "unexpected startup error: {error}"
    );
    assert!(
        read_active_instance_run(&storage.paths, &created.summary.id)
            .await?
            .is_none()
    );
    assert!(
        !state
            .runtime_supervisor
            .lock()
            .expect("runtime supervisor lock")
            .is_tracked(&created.summary.id)
    );

    drop(app);
    let _ = fs::remove_dir_all(&root);
    Ok(())
}

pub(super) fn test_module_details(
    install_state: InstallState,
    steam_app_id: Option<u32>,
    install: Option<app_core::InstallSpec>,
) -> ModuleDetails {
    ModuleDetails {
        summary: ModuleSummary {
            id: String::from("test-module"),
            name: String::from("Test Module"),
            version: String::from("0.1.0"),
            description: None,
            steam_app_id,
            install_state,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
        schema_json: None,
        default_ports: Vec::new(),
        install,
        process: None,
        workshop: None,
        mods: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
    }
}

pub(super) fn test_install_spec() -> app_core::InstallSpec {
    app_core::InstallSpec {
        shared_game_dir: String::from("test-module"),
        download_url_windows: None,
        download_integrity_windows: None,
        source: None,
        verification_path: None,
        minecraft: None,
    }
}

#[test]
pub(super) fn prestart_update_runs_for_installed_steam_modules() {
    let module = test_module_details(
        InstallState::Installed,
        Some(1_623_730),
        Some(test_install_spec()),
    );

    assert!(should_run_prestart_update(&module));
}

#[test]
pub(super) fn prestart_update_runs_for_installed_minecraft_java_modules() {
    let mut install = test_install_spec();
    install.source = Some(InstallSource::MinecraftJava);
    let module = test_module_details(InstallState::Installed, None, Some(install));

    assert!(should_run_prestart_update(&module));
}

#[test]
pub(super) fn prestart_update_skips_manual_modules_without_managed_source() {
    let module_without_install = test_module_details(InstallState::Installed, None, None);
    let module_without_update_source =
        test_module_details(InstallState::Installed, None, Some(test_install_spec()));

    assert!(!should_run_prestart_update(&module_without_install));
    assert!(!should_run_prestart_update(&module_without_update_source));
}

#[test]
pub(super) fn prestart_update_skips_direct_download_modules() {
    let mut install = test_install_spec();
    install.download_url_windows = Some(String::from("https://example.test/server.zip"));
    let module = test_module_details(InstallState::Installed, None, Some(install));

    assert!(!should_run_prestart_update(&module));
}

#[test]
pub(super) fn prestart_update_does_not_auto_install_missing_modules() {
    let module = test_module_details(
        InstallState::NotInstalled,
        Some(1_623_730),
        Some(test_install_spec()),
    );

    assert!(!should_run_prestart_update(&module));
}

#[test]
pub(super) fn prestart_update_skips_whole_package_replacement_even_with_a_steam_id() {
    let mut install = test_install_spec();
    install.download_url_windows = Some("https://example.test/server.zip".into());
    let module = test_module_details(InstallState::Installed, Some(1234), Some(install));
    assert!(!should_run_prestart_update(&module));
}

#[test]
pub(super) fn prestart_update_console_lines_append_to_instance_log() {
    let root = temp_test_dir("prestart-update-console-log");
    let log_path = root.join("instance").join("logs").join("run-123-main.log");

    append_prestart_update_console_line(
        &log_path,
        "Checking SteamCMD runtime...",
        "Progress: 42.00 (123 / 456)",
    )
    .expect("append prestart update line");

    let text = fs::read_to_string(&log_path).expect("read prestart update log");
    assert!(text.contains("[LanGame startup update] Checking SteamCMD runtime..."));
    assert!(text.contains("[LanGame startup update] Progress: 42.00 (123 / 456)"));

    let _ = fs::remove_dir_all(root);
}

#[test]
pub(super) fn pending_start_console_snapshot_reads_reserved_start_log() {
    let root = temp_test_dir("pending-start-console-log");
    let log_path = root
        .join("instance")
        .join("logs")
        .join("managed-console")
        .join("run-456-main.log");
    append_startup_console_line(&log_path, "Preparing Test Server startup...", "")
        .expect("append startup console line");

    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    let state = app.state::<DesktopState>();
    {
        let mut reservations = state.runtime_start_reservations.lock().unwrap();
        reservations
            .reserve("test-server", "manual")
            .expect("reserve start");
        reservations.set_console_log_path("test-server", &log_path.to_string_lossy());
    }

    let snapshot = pending_start_console_log_snapshot(&state, "test-server", 20)
        .expect("pending start snapshot");
    let expected_path = log_path.to_string_lossy().into_owned();

    assert_eq!(
        snapshot.source_path.as_deref(),
        Some(expected_path.as_str())
    );
    assert_eq!(snapshot.read_error, None);
    assert!(
        snapshot
            .lines
            .iter()
            .any(|line| line.contains("[LanGame startup] Preparing Test Server startup..."))
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn manager_console_messages_prefix_every_physical_line() {
    let root = temp_test_dir("manager-console-line-origin");
    let path = root.join("managed-console").join("run-1-main.log");
    append_startup_console_line(
        &path,
        "Preparing Name\nServer ready\rListening on port 7777\r\nstartup...",
        "Loading\rReadyToJoin value[1]\n",
    )
    .unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(
        text.lines().collect::<Vec<_>>(),
        [
            "[LanGame startup] Preparing Name",
            "[LanGame startup] Server ready",
            "[LanGame startup] Listening on port 7777",
            "[LanGame startup] startup...",
            "[LanGame startup] Loading",
            "[LanGame startup] ReadyToJoin value[1]",
        ]
    );
    assert!(!text.contains('\r'));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn run_log_paths_keep_elevated_redirection_outside_managed_retention() {
    let instance = player_snapshot_fixture_instance("scum", InstanceStatus::Stopped);
    let mut plans = vec![ProcessLaunchPlan {
        process_key: String::from("main"),
        display_name: String::from("Server"),
        log_path: String::new(),
        launch_plan: LaunchPlan {
            instance_id: instance.summary.id.clone(),
            instance_name: instance.summary.name.clone(),
            module_id: instance.summary.module_id.clone(),
            install_root: String::new(),
            install_state: InstallState::NotInstalled,
            uses_private_runtime: false,
            working_directory: String::new(),
            executable_path: String::new(),
            executable_exists: false,
            ready_to_launch: false,
            validation_issues: Vec::new(),
            args: Vec::new(),
            environment: Default::default(),
            command_line: String::new(),
            window_policy: app_core::ProcessWindowPolicy::Background,
            uses_script_entrypoint: false,
            requires_admin: true,
            host_surface: app_core::ProcessHostSurface::ManagedTerminal,
            host_notes: None,
            performance_policy: RuntimePerformancePolicy::default(),
            performance_preview: app_core::RuntimePerformancePolicyPreview::default(),
        },
    }];
    assign_process_log_paths_with_stamp(&instance, &mut plans, 456);
    assert_eq!(
        Path::new(&plans[0].log_path),
        Path::new("D:/fixture/logs/run-456-main.log")
    );
    plans[0].launch_plan.requires_admin = false;
    assign_process_log_paths_with_stamp(&instance, &mut plans, 456);
    let expected = if cfg!(windows) {
        "D:/fixture/logs/managed-console/run-456-main.log"
    } else {
        "D:/fixture/logs/run-456-main.log"
    };
    assert_eq!(Path::new(&plans[0].log_path), Path::new(expected));
}

#[test]
pub(super) fn firewall_rule_failures_do_not_block_instance_start() {
    let decision = firewall_rule_failure_start_decision(
        "Abiotic Factor Dedicated Serv124124",
        "New-NetFirewallRule failed",
        None,
    );

    assert!(decision.continue_start);
    assert_eq!(decision.log_level, "warn");
    assert!(decision.message.contains("continuing startup"));
}

#[test]
pub(super) fn strict_bind_firewall_rule_failures_block_instance_start() {
    let decision = firewall_rule_failure_start_decision(
        "Necesse",
        "New-NetFirewallRule failed",
        Some("192.168.31.150"),
    );

    assert!(!decision.continue_start);
    assert_eq!(decision.log_level, "error");
    assert!(decision.message.contains("startup was blocked"));
}

#[tokio::test(flavor = "current_thread")]
pub(super) async fn firewall_rule_application_runs_on_blocking_thread() {
    let async_thread = std::thread::current().id();
    let observed_thread = std::sync::Arc::new(std::sync::Mutex::new(None));
    let observed_thread_for_apply = std::sync::Arc::clone(&observed_thread);

    let result = run_firewall_rule_apply_on_blocking_thread(
        String::from("srv-test"),
        String::from("Test Server"),
        vec![app_core::PortBinding {
            name: String::from("game"),
            protocol: String::from("udp"),
            port: 27015,
        }],
        move |instance_id, instance_name, ports| {
            *observed_thread_for_apply.lock().unwrap() = Some(std::thread::current().id());
            assert_eq!(instance_id, "srv-test");
            assert_eq!(instance_name, "Test Server");
            assert_eq!(ports.len(), 1);
            Ok(vec![app_platform_win::WindowsFirewallRuleApplyResult {
                rule_name: String::from("LanGame Test"),
                protocol: String::from("udp"),
                local_port: 27015,
                local_address: String::from("Any"),
                status: String::from("created"),
                message: None,
            }])
        },
    )
    .await
    .expect("blocking firewall apply should succeed");

    assert_eq!(result[0].status, "created");
    assert_ne!(observed_thread.lock().unwrap().unwrap(), async_thread);
}
