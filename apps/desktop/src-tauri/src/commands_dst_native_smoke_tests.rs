use super::*;
use crate::commands::commands_dst_world_state::{DstWorldState, preview_dontstarve_world_start};
use app_core::StartedProcess;

#[path = "commands_dst_native_failure_tests.rs"]
mod failure_checks;

#[path = "commands_dst_native_island_tests.rs"]
mod island_checks;

#[path = "commands_dst_native_features.rs"]
mod feature_checks;

// Only this disposable instance may be force-cleaned if a smoke assertion fails.
struct NativeCleanup<'a> {
    state: &'a DesktopState,
    instance_id: String,
}

impl Drop for NativeCleanup<'_> {
    fn drop(&mut self) {
        if let Some(mut runtime) = self
            .state
            .runtime_supervisor
            .lock()
            .unwrap()
            .take_running_for_stop(&self.instance_id)
        {
            let _ = stop_managed_instance(&mut runtime);
        }
    }
}

async fn world_fingerprint(
    state: &DesktopState,
    instance_id: &str,
    process: &StartedProcess,
    run_id: i64,
) -> Result<String, Box<dyn std::error::Error>> {
    native_probe(
        state,
        instance_id,
        process,
        run_id,
        "local w,h=TheWorld.Map:GetSize();",
        "tostring(TheNet:GetSessionIdentifier())..' '..tostring(TheWorld.meta.seed)..' '..tostring(TheWorld.topology.overrides.world_size)..' '..tostring(w)..' '..tostring(h)",
    )
    .await
}

async fn native_probe(
    state: &DesktopState,
    instance_id: &str,
    process: &StartedProcess,
    run_id: i64,
    prelude: &str,
    expression: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let marker = format!("[LGSM-NATIVE-{}]", uuid::Uuid::new_v4().simple());
    let command = format!("{prelude} print('{marker} '..({expression}))");
    command_result(
        dispatch_managed_stdin_command(
            state,
            instance_id,
            Some(&process.process_key),
            &command,
            Some(run_id),
        )
        .await,
    )?;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let log = fs::read_to_string(&process.log_path)?;
        if let Some(line) = log
            .lines()
            .find(|line| line.contains(&marker) && !line.contains("print("))
        {
            let value = line.split_once(&marker).unwrap().1.trim().to_string();
            println!("NATIVE_WORLD {}: {value}", process.process_key);
            return Ok(value);
        }
        if Instant::now() >= deadline {
            return Err(format!("No native world fingerprint in {}", process.log_path).into());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "starts installed official DST enabled shards in an isolated offline instance; set LANGAME_DST_SMOKE_GAMES_ROOT to an owned copy of the official package and LANGAME_SMOKE_RUNTIME_ROOT to disposable evidence storage"]
async fn smoke_dst_native_world_generation_save_and_reload()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    // Run each opt-in scenario in a separate invocation so native files and
    // process evidence remain isolated. Never use a user's installed package.
    let scenario = std::env::var("LANGAME_DST_SMOKE_SCENARIO").unwrap_or_else(|_| "vanilla".into());
    let mod_ids: Vec<String> = match scenario.as_str() {
        "vanilla"
        | "events"
        | "invalid_mod_lua"
        | "invalid_caves_mod_lua"
        | "invalid_world_lua"
        | "invalid_world_preset" => Vec::new(),
        "uncompromising" => vec!["2039181790".into()],
        "legion" => vec!["1392778117".into()],
        "combined" | "combined_recovery" | "configured_mods" => {
            vec!["2039181790".into(), "1392778117".into()]
        }
        "tropical" => vec!["1505270912".into()],
        "island_adventures" => vec!["3435352667".into(), "1467214795".into()],
        _ => return Err(format!("Unknown native smoke scenario: {scenario}").into()),
    };
    let events = [
        "crow_carnival",
        "hallowed_nights",
        "winters_feast",
        "year_of_the_beefalo",
        "year_of_the_bunnyman",
        "year_of_the_carrat",
        "year_of_the_catcoon",
        "year_of_the_dragonfly",
        "year_of_the_gobbler",
        "year_of_the_knight",
        "year_of_the_pig",
        "year_of_the_snake",
        "year_of_the_varg",
    ];
    // Keep native asset paths below the Windows engine's path limit even when
    // the caller's managed artifact root contains a full task identifier.
    let run_root = real_smoke_support::allocate_smoke_run_root("dst")?;
    println!(
        "NATIVE_SMOKE_ROOT={}; SCENARIO={scenario}",
        run_root.display()
    );
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let mut settings = dontstarve_command_smoke_settings(&run_root);
    settings.servers_root = run_root.join("i").to_string_lossy().into_owned();
    let install_root = Path::new(&settings.games_root).join("dontstarve");
    let version = fs::read_to_string(install_root.join("version.txt"))?;
    let executable = install_root.join("bin64/dontstarve_dedicated_server_nullrenderer_x64.exe");
    assert!(executable.is_file() && install_root.join("data/databundles/scripts.zip").is_file());
    save_app_settings(settings)?;
    let storage = bootstrap_storage()?;
    let desktop = DesktopState::default();
    desktop.app_state.write().unwrap().settings = storage.settings.clone();
    let app = tauri::test::mock_builder()
        .manage(desktop)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("Tauri command host");
    let state = app.state::<DesktopState>();
    command_result(sync_modules_to_storage(state.clone()).await)?;
    // The caller supplies a locally verified official package. Register that
    // inspection in this isolated DB so the normal start does not update it.
    app_storage::sync_game_installs(
        &storage.paths,
        &[app_storage::GameInstallSyncRecord {
            module_id: "dontstarve".into(),
            install_root: install_root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some(version.trim().into()),
            mark_verified: true,
        }],
    )
    .await?;
    let created = command_result(
        create_instance_record_inner(
            state.clone(),
            CreateInstanceInput {
                name: "dst".into(),
                module_id: "dontstarve".into(),
            },
        )
        .await,
    )?;
    let id = created.summary.id;
    println!("NATIVE_INSTANCE={id}; VERSION={}", version.trim());
    let _cleanup = NativeCleanup {
        state: &state,
        instance_id: id.clone(),
    };
    let initial =
        command_result(read_instance_details_from_storage(state.clone(), id.clone()).await)?;
    let mut values: serde_json::Map<String, Value> = serde_json::from_str(&initial.settings_json)?;
    for (key, value) in [
        ("offline_cluster", json!(true)),
        ("lan_only_cluster", json!(true)),
        ("enable_caves", json!(true)),
        ("disable_data_collection", json!(true)),
        ("pause_when_empty", json!(false)),
        ("cluster_token", json!("")),
        ("master_world_size", json!("small")),
        ("caves_world_size", json!("small")),
        ("shared_workshop_mod_ids", json!("")),
        ("shared_workshop_collection_ids", json!("")),
        ("master_enabled_workshop_mod_ids", json!("")),
        ("caves_enabled_workshop_mod_ids", json!("")),
    ] {
        values.insert(key.into(), value);
    }
    for key in [
        "shared_workshop_mod_ids",
        "master_enabled_workshop_mod_ids",
        "caves_enabled_workshop_mod_ids",
    ] {
        values.insert(key.into(), json!(mod_ids.join(",")));
    }
    if scenario == "events" {
        values.insert("world_specialevent".into(), json!("none"));
        for event in events {
            values.insert(format!("world_{event}"), json!("enabled"));
        }
    }
    failure_checks::configure_scenario(&scenario, &mut values);
    let four_shards = scenario == "island_adventures";
    if four_shards {
        island_checks::configure(&mut values, &mod_ids);
    }
    let process_count = if four_shards { 4 } else { 2 };
    let mut ports = initial.ports.clone();
    for port in &mut ports {
        port.port = match port.name.as_str() {
            "master" => {
                if four_shards {
                    11015
                } else {
                    11017
                }
            }
            "caves" => {
                if four_shards {
                    11016
                } else {
                    11018
                }
            }
            "islands" => {
                if four_shards {
                    11017
                } else {
                    11015
                }
            }
            "volcano" => {
                if four_shards {
                    11018
                } else {
                    11016
                }
            }
            "shard_master" => 18888,
            "steam_query" => 28996,
            "steam_auth" => 18766,
            "caves_steam_query" => 28997,
            "caves_steam_auth" => 18767,
            "islands_steam_query" => 28998,
            "islands_steam_auth" => 18768,
            "volcano_steam_query" => 28999,
            "volcano_steam_auth" => 18769,
            _ => port.port,
        };
    }
    let mut island_mod_options = Vec::new();
    if !mod_ids.is_empty() {
        println!("NATIVE_INSTALL_START={}", mod_ids.join(","));
        let installed = command_result(
            download_steam_workshop_items(
                app.handle().clone(),
                id.clone(),
                mod_ids.clone(),
                Some(true),
                Some("en".into()),
            )
            .await,
        )?;
        assert_eq!(installed.items.len(), mod_ids.len());
        assert!(installed.items.iter().all(|item| item.expected_path_exists));
        let specs = command_result(
            read_dontstarve_mod_configuration_specs(
                state.clone(),
                id.clone(),
                mod_ids.clone(),
                Some("zh-CN".into()),
            )
            .await,
        )?;
        fs::write(
            run_root.join("native-install.json"),
            serde_json::to_vec_pretty(&json!({"download": installed, "configuration": specs}))?,
        )?;
        println!(
            "NATIVE_INSTALL_COMPLETE={}; CONFIG={}",
            mod_ids.join(","),
            serde_json::to_string(
                &specs
                    .iter()
                    .map(|spec| (&spec.mod_id, &spec.status, spec.options.len()))
                    .collect::<Vec<_>>()
            )?
        );
        assert!(specs.iter().all(|spec| spec.status == "loaded"));
        if four_shards {
            island_mod_options = island_checks::configure_mod_options(&mut values, &specs);
        }
    }
    let mut details = command_result(
        update_instance_record_if_current(
            state.clone(),
            UpdateInstanceInput {
                id: id.clone(),
                bind_ip: "0.0.0.0".into(),
                auto_backup_on_stop: false,
                backup_retention_count: 2,
                settings_json: serde_json::to_string(&values)?,
                ports,
            },
            initial.settings_json,
        )
        .await,
    )?;
    if scenario.starts_with("invalid_") {
        return failure_checks::assert_rejected_start(&state, &storage, &id, &scenario, &run_root)
            .await;
    }
    let fresh = command_result(preview_dontstarve_world_start(state.clone(), id.clone()).await)?;
    assert!(
        fresh
            .shards
            .iter()
            .filter(|shard| shard.enabled)
            .all(|shard| shard.state == DstWorldState::New)
    );
    let mut first_fingerprints = Vec::new();
    let mut evidence = Vec::new();
    for generation in 0..2 {
        let expected_size = if generation == 0 { "small" } else { "huge" };
        let persisted =
            command_result(read_instance_details_from_storage(state.clone(), id.clone()).await)?;
        let persisted_values: Value = serde_json::from_str(&persisted.settings_json)?;
        for key in ["master_world_size", "caves_world_size"] {
            assert_eq!(persisted_values[key], json!(expected_size), "{key}");
        }
        let expected =
            command_result(preview_dontstarve_world_start(state.clone(), id.clone()).await)?;
        let started_at = Instant::now();
        let started = command_result(
            start_instance_process_after_reconcile(
                None,
                &state,
                &storage,
                id.clone(),
                "manual",
                Some(expected),
            )
            .await,
        )?;
        println!(
            "NATIVE_READY cycle={generation}; processes={}; seconds={:.1}",
            started.process_count,
            started_at.elapsed().as_secs_f64()
        );
        assert_eq!(started.process_count, process_count);
        let cluster_root = Path::new(&details.config_file_path)
            .parent()
            .expect("instance config directory")
            .join("clusters/main");
        if four_shards {
            island_checks::verify_rendered_configuration(
                &cluster_root,
                &mod_ids,
                &island_mod_options,
            )?;
        }
        for shard in ["Master", "Caves"] {
            let config = fs::read_to_string(cluster_root.join(shard).join("worldgenoverride.lua"))?;
            assert!(
                config
                    .lines()
                    .any(|line| line.trim() == format!("world_size = \"{expected_size}\",")),
                "{shard} startup configuration must contain {expected_size}"
            );
        }
        let mut fingerprints = Vec::new();
        let mut native_features = Vec::new();
        for process in &started.processes {
            if scenario == "configured_mods" {
                failure_checks::verify_mod_options(&state, &id, process, started.run_id).await?;
            }
            if four_shards {
                native_features.push(
                    island_checks::verify_world(
                        &state,
                        &id,
                        process,
                        started.run_id,
                        &island_mod_options,
                    )
                    .await?,
                );
            }
            let fingerprint = world_fingerprint(&state, &id, process, started.run_id).await?;
            if generation == 0 && matches!(process.process_key.as_str(), "master" | "caves") {
                assert!(
                    fingerprint.split_whitespace().nth(2) == Some("small"),
                    "{fingerprint}"
                );
            }
            // Klei may refresh the generation-options metadata on reload. The
            // session, original seed and actual map dimensions identify the map.
            fingerprints.push(
                fingerprint
                    .split_whitespace()
                    .enumerate()
                    .filter(|(index, _)| *index != 2)
                    .map(|(_, value)| value.to_string())
                    .collect::<Vec<_>>(),
            );
            native_features.extend(
                feature_checks::verify(
                    &state,
                    &id,
                    process,
                    started.run_id,
                    &scenario,
                    &mod_ids,
                    &events,
                )
                .await?,
            );
        }
        let stopped = command_result(stop_instance_process(state.clone(), id.clone()).await)?;
        assert_eq!(stopped.process_count, process_count);
        assert!(
            stopped
                .processes
                .iter()
                .all(|process| process.exit_code == Some(0))
        );
        let saved =
            command_result(preview_dontstarve_world_start(state.clone(), id.clone()).await)?;
        assert!(
            saved
                .shards
                .iter()
                .filter(|shard| shard.enabled)
                .all(|shard| shard.state == DstWorldState::Existing)
        );
        println!(
            "NATIVE_SAVED cycle={generation}; all_{process_count}_exit_zero=true; all_native_snapshots=true"
        );
        evidence.push(json!({ "cycle": generation, "verified_world_size": expected_size, "fingerprints": fingerprints, "features": native_features, "start": started, "stop": stopped }));
        if generation == 0 {
            first_fingerprints = fingerprints;
            values.insert("master_world_size".into(), json!("huge"));
            values.insert("caves_world_size".into(), json!("huge"));
            details = command_result(
                update_instance_record_if_current(
                    state.clone(),
                    UpdateInstanceInput {
                        id: id.clone(),
                        bind_ip: details.summary.bind_ip.clone(),
                        auto_backup_on_stop: false,
                        backup_retention_count: 2,
                        settings_json: serde_json::to_string(&values)?,
                        ports: details.ports.clone(),
                    },
                    command_result(
                        read_instance_details_from_storage(state.clone(), id.clone()).await,
                    )?
                    .settings_json,
                )
                .await,
            )?;
        } else {
            assert_eq!(
                fingerprints, first_fingerprints,
                "restarting after generation settings change must load the same map/session"
            );
        }
    }
    if scenario == "combined_recovery" {
        evidence.push(failure_checks::verify_shard_recovery(&state, &storage, &id).await?);
    }
    fs::write(
        run_root.join("native-smoke.json"),
        serde_json::to_vec_pretty(&json!({
            "version": version.trim(), "instance_id": id, "cluster_config": details.config_file_path,
            "offline": true, "scenario": scenario, "cycles": evidence,
        }))?,
    )?;
    println!(
        "NATIVE_SMOKE_COMPLETE={}",
        run_root.join("native-smoke.json").display()
    );
    Ok(())
}
