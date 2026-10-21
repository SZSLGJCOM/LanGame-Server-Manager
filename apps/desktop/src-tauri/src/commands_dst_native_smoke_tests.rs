use super::*;
use crate::commands::commands_dst_world_state::{DstWorldState, preview_dontstarve_world_start};
use app_core::StartedProcess;

#[path = "smoke_resource_copy_manifest.rs"]
mod smoke_resource_copy_manifest;
use smoke_resource_copy_manifest::{CopyProfile, SmokeResourceCopyManifest};

#[path = "smoke_resource_copy_preflight.rs"]
mod smoke_resource_copy_preflight;

#[path = "commands_dst_native_failure_tests.rs"]
mod failure_checks;

#[path = "commands_dst_native_island_tests.rs"]
mod island_checks;

#[path = "commands_dst_native_features.rs"]
mod feature_checks;
#[path = "commands_dst_native_map_fixtures_tests.rs"]
mod map_fixtures;
#[path = "commands_dst_native_map_save_tests.rs"]
mod map_save_checks;
#[path = "commands_dst_native_mod_input_tests.rs"]
mod mod_input;
#[path = "commands_dst_native_player_tests.rs"]
mod player_checks;

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
    // Large Mod layouts may not fit the small vanilla smoke world. Keep all
    // native required-prefab checks enabled while varying the real world size.
    let initial_size =
        std::env::var("LANGAME_DST_SMOKE_WORLD_SIZE").unwrap_or_else(|_| "small".into());
    if !matches!(initial_size.as_str(), "small" | "default" | "medium") {
        return Err("Native smoke world size must be small, default or medium".into());
    }
    let custom_json = std::env::var_os("LANGAME_DST_SMOKE_CUSTOM_MODS_JSON")
        .map(|raw| {
            raw.into_string()
                .map_err(|_| "Custom Mod input must be UTF-8")
        })
        .transpose()?;
    let mod_input = mod_input::resolve(&scenario, custom_json.as_deref())?;
    let fixture_json = std::env::var_os("LANGAME_DST_SMOKE_MAP_FIXTURES_JSON")
        .map(|raw| {
            raw.into_string()
                .map_err(|_| "Map fixture input must be UTF-8")
        })
        .transpose()?;
    let fixture_input = map_fixtures::parse(&scenario, fixture_json.as_deref())?;
    let mod_ids = mod_input.mod_ids.clone();
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
    let mut resource_manifest = SmokeResourceCopyManifest::begin(&run_root)?;
    let copy_preflight = resource_manifest
        .as_ref()
        .map(SmokeResourceCopyManifest::copy_preflight);
    if resource_manifest.is_none() {
        println!("NATIVE_SMOKE_RESOURCES_UNMANAGED=resource-policy-not-configured");
    }
    println!(
        "NATIVE_SMOKE_ROOT={}; SCENARIO={scenario}",
        run_root.display()
    );
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let mut settings = dontstarve_command_smoke_settings(&run_root);
    settings.servers_root = run_root.join("i").to_string_lossy().into_owned();
    if let Some(root) = std::env::var_os("LANGAME_DST_SMOKE_STEAMCMD_ROOT") {
        // Reuse the owned official depot cache through normal creation's
        // inventory verification; do not manufacture a clean-package marker.
        settings.steamcmd_root = PathBuf::from(root).to_string_lossy().into_owned();
    }
    let install_root = std::env::var_os("LANGAME_DST_SMOKE_PACKAGE")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(&settings.games_root).join("dontstarve"));
    let version = fs::read_to_string(install_root.join("version.txt"))?;
    let executable = install_root.join("bin64/dontstarve_dedicated_server_nullrenderer_x64.exe");
    assert!(executable.is_file() && install_root.join("data/databundles/scripts.zip").is_file());
    let descriptor = app_modules::discover_modules(&settings.modules_root)?
        .into_iter()
        .find(|item| item.summary.id == "dontstarve")
        .ok_or("Missing DST module")?;
    let inspected = app_steamcmd::probe_module_install_state_with_override(
        &settings,
        "dontstarve",
        descriptor.summary.steam_app_id,
        descriptor.install.as_ref(),
        descriptor.process.as_ref(),
        Some(&install_root.to_string_lossy()),
    );
    assert_eq!(inspected.install_state, InstallState::Installed);
    // Steam's build ID is distinct from DST's game compatibility version.
    let build_id = inspected
        .current_version
        .ok_or("Missing official Steam build ID")?;
    println!("NATIVE_PACKAGE_BUILD={build_id}");
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
    if !mod_ids.is_empty() {
        app_steamcmd::ensure_steamcmd_installed_with_progress(
            &state.install_locks,
            &storage.settings,
            |progress| println!("NATIVE_STEAMCMD {:?}: {}", progress.phase, progress.detail),
        )
        .await?;
    }
    // The caller supplies a locally verified official package. Register that
    // inspection in this isolated DB; the pinned policy below preserves the
    // inspected build so a connected native client uses the same game version.
    app_storage::sync_game_installs(
        &storage.paths,
        &[app_storage::GameInstallSyncRecord {
            module_id: "dontstarve".into(),
            install_root: install_root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some(build_id),
            mark_verified: true,
        }],
    )
    .await?;
    if let Some(preflight) = copy_preflight.as_ref() {
        preflight.require_capacity(&[install_root.clone()])?;
    }
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
    if let Some(manifest) = resource_manifest.as_mut() {
        let runtime = PathBuf::from(
            crate::commands::commands_runtime_lifecycle::private_runtime_install_root(&initial)?,
        );
        manifest.record_copy(CopyProfile::ServerAssets, &install_root, &runtime)?;
    }
    let mut values: serde_json::Map<String, Value> = serde_json::from_str(&initial.settings_json)?;
    for (key, value) in [
        ("offline_cluster", json!(true)),
        ("lan_only_cluster", json!(true)),
        ("enable_caves", json!(true)),
        ("disable_data_collection", json!(true)),
        ("pause_when_empty", json!(false)),
        ("program_update", json!({"policy": "pinned"})),
        ("cluster_token", json!("")),
        ("master_world_size", json!(initial_size)),
        ("caves_world_size", json!(initial_size)),
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
    if let Ok(options) = std::env::var("LANGAME_DST_SMOKE_MOD_CONFIGURATION_OPTIONS") {
        if options.len() > 64 * 1024 {
            return Err("Native smoke Mod configuration exceeds 64 KiB".into());
        }
        let options: serde_json::Map<String, Value> = serde_json::from_str(&options)?;
        for shard in ["master", "caves"] {
            values.insert(
                format!("{shard}_mod_configuration_options"),
                Value::Object(options.clone()),
            );
        }
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
        let installed = if let Some(source) = std::env::var_os("LANGAME_DST_SMOKE_WORKSHOP_CACHE") {
            let instance_root = Path::new(&initial.config_file_path)
                .parent()
                .and_then(Path::parent)
                .ok_or("Missing disposable instance directory")?;
            let source = PathBuf::from(source);
            // Seed the normal SteamCMD cache as well as shard UGC. Startup still
            // performs native Workshop validation and records its own receipt;
            // the official manifest lets it reuse the already downloaded bytes.
            let runtime = PathBuf::from(
                crate::commands::commands_runtime_lifecycle::private_runtime_install_root(
                    &initial,
                )?,
            );
            let runtime_cache = runtime.join("steamapps/workshop");
            let workshop_sources = mod_ids
                .iter()
                .map(|id| source.join("content").join("322330").join(id))
                .collect::<Vec<_>>();
            if let Some(manifest) = resource_manifest.as_mut() {
                manifest.record_workshop_items(&source, &runtime_cache, &mod_ids)?;
            }
            if let Some(preflight) = copy_preflight.as_ref() {
                preflight.require_capacity(&workshop_sources)?;
            }
            let mut count =
                app_steamcmd::deploy_dst_workshop_cache(&source, &runtime_cache, &mod_ids)?
                    .file_count;
            for shard in app_core::dst_shards::DST_SHARDS {
                let shard_cache = instance_root.join("data/ugc").join(shard.directory);
                if let Some(manifest) = resource_manifest.as_mut() {
                    manifest.record_workshop_items(&source, &shard_cache, &mod_ids)?;
                }
                if let Some(preflight) = copy_preflight.as_ref() {
                    preflight.require_capacity(&workshop_sources)?;
                }
                count += app_steamcmd::deploy_dst_workshop_cache(&source, &shard_cache, &mod_ids)?
                    .file_count;
            }
            json!({"preparedCache":source,"deployedFiles":count,"ids":mod_ids})
        } else {
            let result = command_result(
                download_steam_workshop_items(
                    app.handle().clone(),
                    id.clone(),
                    mod_ids.clone(),
                    Some(true),
                    Some("en".into()),
                )
                .await,
            )?;
            assert_eq!(result.items.len(), mod_ids.len());
            assert!(result.items.iter().all(|item| item.expected_path_exists));
            serde_json::to_value(result)?
        };
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
        // A valid Mod may intentionally expose no configurable options.
        // Preserve failures and warning states; only an empty `no_options`
        // result is equivalent to a fully loaded configuration here.
        assert!(specs.iter().all(|spec| {
            spec.status == "loaded" || (spec.status == "no_options" && spec.options.is_empty())
        }));
        if four_shards {
            island_mod_options = island_checks::configure_mod_options(&mut values, &specs);
        }
    }
    let mut details = command_result(
        update_instance_record_if_current(
            state.clone(),
            UpdateInstanceInput {
                id: id.clone(),
                bind_ip: std::env::var("LANGAME_DST_SMOKE_BIND_IP")
                    .unwrap_or_else(|_| "127.0.0.1".into()),
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
    prepare_firewall_evidence(&details, &run_root).await?;
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
    let mut fixture_evidence = Vec::new();
    for generation in 0..2 {
        let expected_size = if generation == 0 {
            initial_size.as_str()
        } else {
            "huge"
        };
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
                    fingerprint.split_whitespace().nth(2) == Some(initial_size.as_str()),
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
        if generation == 0 {
            if let Some(input) = &fixture_input {
                for process in &started.processes {
                    fixture_evidence.push(
                        map_fixtures::arrange(
                            &state,
                            &id,
                            process,
                            started.run_id,
                            &run_root,
                            input,
                        )
                        .await?,
                    );
                }
            }
        }
        map_save_checks::verify(
            state.clone(),
            &storage,
            &id,
            &started,
            &run_root,
            generation,
        )
        .await?;
        if generation == 0 {
            map_save_checks::verify_connected_player(state.clone(), &id, &started, &run_root)
                .await?;
            map_save_checks::verify_pre_stop_if_all_shards(
                state.clone(),
                &storage,
                &id,
                &started,
                &run_root,
            )
            .await?;
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
    let mut smoke_evidence = json!({
        "version": version.trim(), "instance_id": id, "cluster_config": details.config_file_path,
        "offline": true, "scenario": scenario, "cycles": evidence,
        "mod_configuration_options": values.get("master_mod_configuration_options"),
    });
    if let Some(custom) = mod_input.custom_evidence {
        smoke_evidence["custom_mod_input"] = serde_json::to_value(custom)?;
    }
    if let Some(input) = fixture_input {
        smoke_evidence["map_fixture_input"] = serde_json::to_value(input)?;
        smoke_evidence["map_fixture_evidence"] = serde_json::to_value(fixture_evidence)?;
    }
    fs::write(
        run_root.join("native-smoke.json"),
        serde_json::to_vec_pretty(&smoke_evidence)?,
    )?;
    if let Some(manifest) = resource_manifest.as_mut() {
        manifest.complete()?;
    }
    println!(
        "NATIVE_SMOKE_COMPLETE={}",
        run_root.join("native-smoke.json").display()
    );
    Ok(())
}

// libtest cannot handle the desktop executable's UAC helper entry point. An
// opt-in external controller may prepare these exact rules; normal startup
// still independently validates them. Neither Cargo nor the game is elevated.
async fn prepare_firewall_evidence(
    instance: &InstanceDetails,
    root: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let Ok(seconds) = std::env::var("LANGAME_DST_SMOKE_FIREWALL_WAIT_SECONDS") else {
        return Ok(());
    };
    let seconds = seconds.parse::<u64>()?;
    if !(30..=900).contains(&seconds) {
        return Err("Firewall preparation timeout must be 30..900 seconds".into());
    }
    let rules = app_platform_win::build_instance_firewall_rule_specs(
        &instance.summary.id,
        &instance.summary.name,
        &instance.ports,
        Some(&instance.summary.bind_ip),
    );
    fs::write(
        root.join("native-firewall-request.json"),
        serde_json::to_vec(&json!({"instanceId": instance.summary.id,
            "processId": std::process::id(), "rules": rules}))?,
    )?;
    println!("NATIVE_FIREWALL_WAIT root={}", root.display());
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let receipt = root.join("native-firewall-ready.json");
    while !receipt.is_file() {
        if Instant::now() >= deadline {
            return Err("External firewall preparation did not complete before deadline".into());
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let receipt: Value = serde_json::from_str(&fs::read_to_string(receipt)?)?;
    if receipt["state"] != "ready" {
        return Err(format!("External firewall preparation failed: {}", receipt["state"]).into());
    }
    Ok(())
}
