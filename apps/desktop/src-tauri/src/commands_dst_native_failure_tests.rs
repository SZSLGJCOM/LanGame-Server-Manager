use super::*;

pub(super) fn configure_scenario(scenario: &str, values: &mut serde_json::Map<String, Value>) {
    let invalid = match scenario {
        "invalid_mod_lua" => Some(("master_modoverrides_lua", "return { broken = }")),
        "invalid_caves_mod_lua" => Some((
            "caves_modoverrides_lua",
            "error('LGSM isolated invalid config')",
        )),
        "invalid_world_lua" => Some(("master_worldgenoverride_lua", "return { broken = }")),
        "invalid_world_preset" => Some(("master_worldgen_preset", "LGSM_NONEXISTENT_PRESET")),
        _ => None,
    };
    if let Some((key, value)) = invalid {
        values.insert(key.into(), json!(value));
    }
    if scenario == "combined_recovery" {
        values.insert(
            "runtime_restart".into(),
            json!({
                "enabled": true, "max_restarts": 3, "backoff_ms": 0, "only_nonzero_exit": true,
            }),
        );
    }
    if scenario == "configured_mods" {
        for (shard, music, language, interval) in [
            ("master", false, "english", 0.5),
            ("caves", true, "chinese", 1.2),
        ] {
            values.insert(
                format!("{shard}_mod_configuration_options"),
                json!({
                    "2039181790": { "um_music": music },
                    "1392778117": { "Language": language, "MouseInfo": interval },
                }),
            );
        }
    }
}

pub(super) async fn verify_mod_options(
    state: &DesktopState,
    id: &str,
    process: &StartedProcess,
    run_id: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    let actual = native_probe(state, id, process, run_id, "",
        "tostring(GetModConfigData('um_music','workshop-2039181790'))..' '..tostring(GetModConfigData('Language','workshop-1392778117'))..' '..tostring(GetModConfigData('MouseInfo','workshop-1392778117'))").await?;
    let expected = if process.process_key == "master" {
        "false english 0.5"
    } else {
        "true chinese 1.2"
    };
    assert_eq!(
        actual, expected,
        "native Mod boolean/string/number options must match the selected shard"
    );
    println!("NATIVE_MOD_OPTIONS {}: {actual}", process.process_key);
    Ok(())
}

pub(super) async fn assert_rejected_start(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    id: &str,
    scenario: &str,
    run_root: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let started_at = Instant::now();
    let error =
        start_instance_process_after_reconcile(None, state, storage, id.into(), "manual", None)
            .await
            .expect_err("invalid native configuration must never become Ready");
    let expected = match scenario {
        "invalid_mod_lua" => "Failed to load modoverrides.lua",
        "invalid_caves_mod_lua" => "Failed to run code from modoverrides.lua",
        "invalid_world_lua" => "Failed to load ../worldgenoverride.lua",
        "invalid_world_preset" => "nonexistent worldgen preset",
        _ => unreachable!("validated smoke scenario"),
    };
    assert!(error.contains(expected), "expected {expected}: {error}");
    assert!(
        read_active_instance_run(&storage.paths, id)
            .await?
            .is_none()
    );
    assert!(
        !state
            .runtime_supervisor
            .lock()
            .unwrap()
            .tracked_instances()
            .iter()
            .any(|run| run.summary.id == id)
    );
    fs::write(
        run_root.join("native-rejected.json"),
        serde_json::to_vec_pretty(&json!({
            "scenario": scenario, "error": error, "seconds": started_at.elapsed().as_secs_f64(),
            "active_run": false, "tracked_processes": false,
        }))?,
    )?;
    println!("NATIVE_REJECTED {scenario}: {error}; all_owned_processes_cleaned=true");
    Ok(())
}

pub(super) async fn verify_shard_recovery(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    id: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    let started = command_result(
        start_instance_process_after_reconcile(None, state, storage, id.into(), "manual", None)
            .await,
    )?;
    let mut before = Vec::new();
    for process in &started.processes {
        before.push(world_fingerprint(state, id, process, started.run_id).await?);
    }
    let active = read_active_instance_run(&storage.paths, id)
        .await?
        .expect("running disposable instance");
    let caves = active
        .processes
        .iter()
        .find(|process| process.process_key == "caves")
        .unwrap();
    app_runtime::kill_process_by_pid(caves.pid.unwrap(), caves.process_identity.as_ref().unwrap())?;
    println!(
        "NATIVE_CRASH_INJECTED caves_run={}; caves_pid={}",
        caves.run_id,
        caves.pid.unwrap()
    );
    let deadline = Instant::now() + Duration::from_secs(180);
    let recovered = loop {
        command_result(reconcile_runtime_state(state).await)?;
        if let Some(run) = read_active_instance_run(&storage.paths, id).await?
            && run.session_id != active.session_id
            && run.process_count == 2
        {
            break run;
        }
        if Instant::now() >= deadline {
            return Err("Native shard auto restart did not finish".into());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    let history = read_instance_runtime_overview(&storage.paths, id).await?;
    let crashed = history
        .recent_runs
        .iter()
        .find(|run| run.session_id == active.session_id)
        .unwrap();
    let old_master = crashed
        .processes
        .iter()
        .find(|process| process.process_key == "master")
        .unwrap();
    let old_caves = crashed
        .processes
        .iter()
        .find(|process| process.process_key == "caves")
        .unwrap();
    assert_eq!(
        old_master.exit_code,
        Some(0),
        "surviving Master must save and exit normally"
    );
    assert!(!old_master.crash_flag);
    assert!(old_caves.crash_flag && old_caves.exit_code != Some(0));
    let mut after = Vec::new();
    for template in &started.processes {
        let record = recovered
            .processes
            .iter()
            .find(|process| process.process_key == template.process_key)
            .unwrap();
        let mut process = template.clone();
        process.run_id = record.run_id;
        process.pid = record.pid.unwrap();
        process.log_path = record.log_path.clone().unwrap();
        assert_ne!(process.run_id, template.run_id);
        after.push(world_fingerprint(state, id, &process, recovered.run_id).await?);
        let mods = native_probe(state, id, &process, recovered.run_id, "",
            "tostring(ModManager:GetMod('workshop-2039181790')~=nil)..' '..tostring(ModManager:GetMod('workshop-1392778117')~=nil)..' '..tostring(#ModManager.failedmods)").await?;
        assert_eq!(mods, "true true 0");
    }
    assert_eq!(
        before, after,
        "both recovered shards must load their original world sessions and maps"
    );
    let stopped = command_result(stop_instance_process(state.clone(), id.into()).await)?;
    assert_eq!(stopped.process_count, 2);
    assert!(
        stopped
            .processes
            .iter()
            .all(|process| process.exit_code == Some(0))
    );
    command_result(reconcile_runtime_state(state).await)?;
    assert!(
        read_active_instance_run(&storage.paths, id)
            .await?
            .is_none(),
        "manual stop must not restart"
    );
    println!(
        "NATIVE_RECOVERY_COMPLETE both_maps_preserved=true; survivor_exit_zero=true; both_mods_loaded=true; manual_stop_stays_stopped=true"
    );
    Ok(
        json!({"recovery": true, "before": before, "after": after, "crashed": crashed,
        "recovered": recovered, "stop": stopped}),
    )
}
