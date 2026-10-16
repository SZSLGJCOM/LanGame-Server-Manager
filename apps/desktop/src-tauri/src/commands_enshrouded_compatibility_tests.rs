#![cfg(windows)]

use super::*;
use support::*;

#[path = "commands_enshrouded_bans_acceptance.rs"]
mod bans;
#[path = "commands_enshrouded_contract_tests.rs"]
mod contract;
#[path = "commands_enshrouded_mods_acceptance.rs"]
mod mods_acceptance;
#[path = "commands_enshrouded_settings_acceptance.rs"]
mod settings_acceptance;
#[path = "commands_enshrouded_compatibility_support.rs"]
mod support;

const MODULE: &str = "enshrouded";
const MARKER: &str = ".lgsm-enshrouded-compatibility-fixture";

#[tokio::test(flavor = "current_thread")]
#[ignore = "explicit marked 1.0 package and immutable old-world copy; low-level native loopback probe plus production storage backup/restore; not full Tauri lifecycle"]
async fn isolated_enshrouded_old_world_upgrade_restart_and_backup_restore() -> ProbeResult<()> {
    let _guard = command_smoke_lock().lock().await;
    let isolated = input("LGSM_ENSHROUDED_ISOLATED_ROOT")?;
    let source = input("LGSM_ENSHROUDED_SOURCE_PACKAGE")?;
    let golden_root = input("LGSM_ENSHROUDED_SAVE_FIXTURE")?;
    let marker = plain_existing(&isolated.join(MARKER))?;
    let evidence_arg = PathBuf::from(
        std::env::var_os("LGSM_ENSHROUDED_EVIDENCE_ROOT")
            .ok_or("supply a new evidence directory")?,
    );
    let evidence_parent = plain_existing(evidence_arg.parent().ok_or("evidence has no parent")?)?;
    let evidence = evidence_parent.join(evidence_arg.file_name().ok_or("evidence has no name")?);
    let repository = workspace_root().canonicalize()?;
    if !evidence_arg.is_absolute()
        || evidence_arg.components().any(|part| {
            matches!(
                part,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
        || !isolated.is_dir()
        || !marker.is_file()
        || !source.is_dir()
        || !golden_root.is_dir()
        || isolated.starts_with(&repository)
        || repository.starts_with(&isolated)
        || !source.starts_with(&isolated)
        || !golden_root.starts_with(&isolated)
        || !evidence_parent.starts_with(&isolated)
        || source.starts_with(&golden_root)
        || golden_root.starts_with(&source)
        || evidence.starts_with(&source)
        || source.starts_with(&evidence)
        || evidence.starts_with(&golden_root)
        || golden_root.starts_with(&evidence)
    {
        return Err(
            "compatibility inputs must be separate children of one marked external root".into(),
        );
    }
    let package_identity = hotfix_package_identity(&source)?;
    // Exclusive creation refuses every existing evidence directory, including links.
    fs::create_dir(&evidence)?;
    let golden = read_files(&golden_root)?;
    if golden.is_empty() || golden.values().any(Vec::is_empty) {
        return Err("supply a nonempty existing world, never a freshly generated fixture".into());
    }
    write_files(&evidence.join("immutable-before"), &golden)?;
    let observation_seconds = std::env::var("LGSM_ENSHROUDED_OBSERVATION_SECONDS")
        .unwrap_or_else(|_| "330".into())
        .parse::<u64>()?;
    if observation_seconds > 600 {
        return Err("native observation is bounded to 600 seconds".into());
    }
    let allow_native_udp = match std::env::var("LGSM_ENSHROUDED_ALLOW_NATIVE_UDP")
        .ok()
        .as_deref()
    {
        None | Some("false") => false,
        Some("true") => true,
        Some(_) => return Err("native UDP acknowledgement must be true or false".into()),
    };
    let contract_only =
        std::env::var("LGSM_ENSHROUDED_CONTRACT_ONLY").is_ok_and(|value| value == "true");
    let descriptors = discover_modules(workspace_root().join("modules"))?;
    let descriptor = find_descriptor(&descriptors, MODULE)?;
    let source_copy = source.clone();
    let package = tokio::task::spawn_blocking(move || {
        package::copy_package(
            &source_copy,
            MODULE,
            Some(Path::new("savegame")),
            16 * 1024 * 1024 * 1024,
            Duration::from_secs(600),
        )
    })
    .await??;
    assert!(!package.install_root.join("savegame").exists());
    let env_guard = environment::NativeEnvironment::set(&package.root)?;
    save_app_settings(AppSettings {
        archives_root: String::new(),
        servers_root: package.root.join("i").to_string_lossy().into_owned(),
        games_root: package.root.to_string_lossy().into_owned(),
        modules_root: workspace_root()
            .join("modules")
            .to_string_lossy()
            .into_owned(),
        steamcmd_root: package.root.join("steamcmd").to_string_lossy().into_owned(),
    })?;
    let storage = bootstrap_storage()?;
    initialize_database(&storage.paths).await?;
    sync_modules(&storage.paths, &descriptors).await?;
    app_storage::sync_game_installs(
        &storage.paths,
        &[app_storage::GameInstallSyncRecord {
            module_id: MODULE.into(),
            install_root: package.install_root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: None,
            mark_verified: true,
        }],
    )
    .await?;
    let desktop = DesktopState::default();
    desktop.app_state.write().unwrap().settings = storage.settings.clone();
    let app = tauri::test::mock_builder()
        .manage(desktop)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    // Local-package creation does not certify official acquisition or invoke repair.
    let initial = creation::CreationMode {
        acquisition: creation::Acquisition::LocalPackage,
        instances: creation::InstanceMode::Default,
    }
    .create(state.clone(), &storage, descriptor, &package, "ensh-compat")
    .await?;
    let id = initial.summary.id.clone();
    let runtime_root =
        seed_probe::checked_path(&package.root, &creation::program_root(&initial)?, true)?;
    let mut owned = OwnedNative {
        process: None,
        cleanup_allowed: Arc::clone(&package.cleanup_allowed),
    };
    let saves = seed_probe::checked_path(&package.root, Path::new(&initial.saves_path), true)?;
    let mut secrets = Vec::new();
    let mut details = initial;
    let mut cycles = Vec::new();
    let verification: ProbeResult<()> = async {
        assert!(read_files(&saves)?.is_empty(), "new instance must not inherit source saves");
        for (relative, bytes) in &golden {
            write_new(&saves.join(relative), bytes)?;
        }
        assert_eq!(read_files(&saves)?, golden, "every imported byte must match the immutable input");
        let mut settings: serde_json::Map<String, Value> = serde_json::from_str(&details.settings_json)?;
        settings.insert("server_name".into(), json!("LGSM isolated 1.0 migration"));
        settings.insert("program_update".into(), json!({"policy": "pinned"}));
        settings.insert("game_settings_preset".into(), json!("Custom"));
        settings.insert("random_spawner_amount".into(), json!("Automatic"));
        settings.insert("flame_level_fog_removal".into(), json!(1));
        settings.insert("player_health_factor".into(), json!(1.5));
        settings.insert("enemy_health_factor".into(), json!(2.5));
        settings.insert("hunger_to_starving_minutes".into(), json!(13));
        settings.insert("day_time_minutes".into(), json!(42));
        settings.insert("night_time_minutes".into(), json!(17));
        settings.insert("admin_reserved_slots".into(), json!(1));
        settings.insert("friend_can_edit_base".into(), json!(false));
        for (key, value) in &mut settings {
            if key.contains("password") {
                let secret = uuid::Uuid::new_v4().simple().to_string();
                *value = json!(secret);
                secrets.push(secret);
            }
        }
        // This explicitly bounded native probe never invokes production firewall
        // admission. It exercises the real module launch plan only on loopback.
        let reserved = std::net::UdpSocket::bind("127.0.0.1:0")?;
        let port = reserved.local_addr()?.port();
        let mut ports = details.ports.clone();
        assert_eq!(ports.len(), 1, "review port isolation if the native contract changes");
        assert_eq!(ports[0].name, "query");
        assert_eq!(ports[0].protocol, "udp");
        ports[0].port = port;
        details = app_storage::update_instance(&storage.paths, UpdateInstanceInput {
            id: id.clone(), bind_ip: "127.0.0.1".into(), auto_backup_on_stop: false,
            backup_retention_count: 12, settings_json: serde_json::to_string(&settings)?, ports,
        }).await?;
        let native: Value = serde_json::from_slice(&fs::read(runtime_root.join("enshrouded_server.json"))?)?;
        assert_eq!(native["ip"], "127.0.0.1");
        assert_eq!(native["queryPort"], port);
        assert_eq!(plain_existing(Path::new(native["saveDirectory"].as_str().ok_or("missing native saveDirectory")?))?, saves);
        let native_logs = plain_existing(Path::new(native["logDirectory"].as_str().ok_or("missing native logDirectory")?))?;
        assert!(native_logs.starts_with(package.root.canonicalize()?));
        let before_backup = create_instance_backup(state.clone(), id.clone()).await?;
        assert_eq!(read_files(&Path::new(&before_backup.backup_path).join("saves"))?, golden);
        write_files(&evidence.join("migration-before-backup"), &read_files(Path::new(&before_backup.backup_path))?)?;
        capture(&evidence, "imported", &details, &runtime_root, &package.root, &secrets)?;
        drop(reserved);
        if contract_only {
            let install_text = runtime_root.to_string_lossy();
            let module = map_module_details_with_install_state(&storage.settings, descriptor, Some(&install_text));
            let plan = app_runtime::build_launch_plan_with_override(&storage.settings, &module, &details, Some(&install_text))?;
            cycles = contract::run(&plan, &details, &mut owned, contract::ContractProbeContext {
                runtime: &runtime_root, fixture: &package.root, evidence: &evidence,
                query_port: port, allow_native_udp, secrets: &secrets,
            }).await?;
            return Ok(());
        }
        mods_acceptance::run(state.clone(), &details, &runtime_root, &package.install_root, &package.root, &evidence).await?;
        details = bans::seed_and_exercise(&storage.paths, &details, &runtime_root, &evidence).await?;
        let mut migrated = None;
        let mut native_recovery_source: Option<Vec<u8>> = None;
        for (index, phase) in ["upgrade", "restart", "restored-restart", "premigration-restored-upgrade", "native-backup-restored-restart"].into_iter().enumerate() {
            if index == 2 {
                details = bans::remove_added(&storage.paths, &details, &runtime_root, &evidence).await?;
                let expected = migrated.as_ref().ok_or("missing stopped migrated snapshot")?;
                let backup = create_instance_backup(state.clone(), id.clone()).await?;
                assert_eq!(read_files(&Path::new(&backup.backup_path).join("saves"))?, *expected);
                write_files(&evidence.join("restore-source-backup"), &read_files(Path::new(&backup.backup_path))?)?;
                let marker = saves.join(format!("lgsm-restore-{}.fixture", uuid::Uuid::new_v4().simple()));
                write_new(&marker, b"owned post-backup restore probe")?;
                let changed = read_files(&saves)?;
                let restored = restore_instance_backup(state.clone(), id.clone(), backup.backup_id.clone(), None).await?;
                assert_eq!(restored.restored_file_count, expected.len());
                assert_eq!(read_files(&saves)?, *expected);
                assert!(!marker.exists());
                assert_eq!(read_files(&Path::new(&restored.safeguard_backup_path).join("saves"))?, changed);
                write_files(&evidence.join("restore-safeguard"), &read_files(Path::new(&restored.safeguard_backup_path))?)?;
                capture(&evidence, "restored-before-restart", &details, &runtime_root, &package.root, &secrets)?;
            }
            if index == 3 {
                let changed = read_files(&saves)?;
                let restored = restore_instance_backup(state.clone(), id.clone(), before_backup.backup_id.clone(), None).await?;
                assert_eq!(restored.restored_file_count, golden.len());
                assert_eq!(read_files(&saves)?, golden, "restoring the original backup must remove every migrated file");
                assert!(!saves.join("world0.ksd").exists());
                assert_eq!(read_files(&Path::new(&restored.safeguard_backup_path).join("saves"))?, changed);
                write_files(&evidence.join("premigration-restore-safeguard"), &read_files(Path::new(&restored.safeguard_backup_path))?)?;
                capture(&evidence, "premigration-restored", &details, &runtime_root, &package.root, &secrets)?;
            }
            if index == 4 {
                // Restore a real native rotating backup, with an exact production
                // backup of the current migrated world retained as a safeguard.
                let changed = read_files(&saves)?;
                let safeguard = create_instance_backup(state.clone(), id.clone()).await?;
                assert_eq!(read_files(&Path::new(&safeguard.backup_path).join("saves"))?, changed);
                write_files(&evidence.join("native-restore-safeguard"), &read_files(Path::new(&safeguard.backup_path))?)?;
                let native_bytes = native_recovery_source.as_ref().ok_or("missing actual native backup")?;
                fs::write(saves.join("world0.ksd"), native_bytes)?;
                assert_eq!(&fs::read(saves.join("world0.ksd"))?, native_bytes);
                write_new(&evidence.join("native-restore-source.ksd"), native_bytes)?;
                capture(&evidence, "native-backup-restored", &details, &runtime_root, &package.root, &secrets)?;
            }
            if index > 0 {
                settings.insert("flame_level_fog_removal".into(), json!(11));
                details = app_storage::update_instance(&storage.paths, UpdateInstanceInput {
                    id: id.clone(), bind_ip: "127.0.0.1".into(), auto_backup_on_stop: false,
                    backup_retention_count: 12, settings_json: serde_json::to_string(&settings)?, ports: details.ports.clone(),
                }).await?;
            }
            bans::assert_roster(&runtime_root, index < 2, index == 0)?;
            let native: Value = serde_json::from_slice(&fs::read(runtime_root.join("enshrouded_server.json"))?)?;
            assert_eq!(native["ip"], "127.0.0.1");
            assert_eq!(native["queryPort"], port);
            assert_eq!(plain_existing(Path::new(native["saveDirectory"].as_str().ok_or("missing native saveDirectory")?))?, saves);
            assert_eq!(plain_existing(Path::new(native["logDirectory"].as_str().ok_or("missing native logDirectory")?))?, native_logs);
            assert_eq!(native["gameSettingsPreset"], "Custom");
            assert_eq!(native["gameSettings"]["randomSpawnerAmount"], "Automatic");
            assert_eq!(native["gameSettings"]["flameLevelFogRemoval"], if index == 0 { 1 } else { 11 });
            let before = read_files(&saves)?;
            let install_text = runtime_root.to_string_lossy();
            let module = map_module_details_with_install_state(&storage.settings, descriptor, Some(&install_text));
            let plan = app_runtime::build_launch_plan_with_override(&storage.settings, &module, &details, Some(&install_text))?;
            if !plan.ready_to_launch || plan.requires_admin {
                return Err("native probe launch plan is not ready or unexpectedly requires elevation".into());
            }
            assert_eq!(plain_existing(Path::new(&plan.working_directory))?, runtime_root);
            assert!(plain_existing(Path::new(&plan.executable_path))?.starts_with(&runtime_root));
            let log_path = creation::instance_root(&details)?.join("logs").join(format!("native-{phase}.log"));
            assert!(!log_path.exists(), "world acceptance must inspect a fresh per-launch log");
            package.cleanup_allowed.store(false, Ordering::SeqCst);
            println!("ENSHROUDED_COMPATIBILITY phase={phase} action=start fixture={} query_port={port} imported_files={} scope=native_loopback_probe", package.root.display(), golden.len());
            owned.process = Some(app_runtime::spawn_launch_plan(&plan, &log_path)?);
            let process = owned.process.as_mut().ok_or("native spawn ownership missing")?;
            if app_runtime::stabilize_spawned_process(&plan.executable_path, process, Duration::from_millis(300))?.is_some() {
                return Err("native server exited during launch stabilization".into());
            }
            let pid = process.pid;
            let identity = process.process_identity.clone();
            let ready_world = observe_native(process, NativeObservation {
                query_port: port, evidence: &evidence, phase,
                observation_seconds: if index == 0 { observation_seconds } else { 0 },
                allow_native_udp, log_path: &log_path,
                expected_flame_level: if index == 0 { 1 } else { 11 },
                world_path: &saves.join("world0.ksd"),
            }).await?;
            let live_world = fs::read(saves.join("world0.ksd"))?;
            assert!(!live_world.is_empty());
            write_new(&evidence.join(format!("{phase}-running-world.ksd")), &live_world)?;
            if index == 0 {
                let live_backups = read_files(&saves.join("backup"))?;
                let distinct = live_backups.values().collect::<std::collections::BTreeSet<_>>();
                assert!(distinct.len() >= 2, "native backups must include multiple saved revisions while the process is running");
                assert_ne!(live_world, ready_world, "a later world revision must be written while the owned process is still running");
                native_recovery_source = Some(live_backups.get(Path::new("world0.ksd.backup-0")).ok_or("missing actual native rotating backup")?.clone());
                write_files(&evidence.join("upgrade-running-native-backups"), &live_backups)?;
            }
            let exit_code = app_runtime::stop_spawned_process_gracefully(process, Duration::from_secs(20))?;
            if process.child.is_some() || app_runtime::process_matches_identity(pid, &identity)? {
                return Err("graceful native stop did not release its owned complete process tree".into());
            }
            owned.process.take();
            package.cleanup_allowed.store(true, Ordering::SeqCst);
            let stopped_details = read_instance_details(&storage.paths, &id).await?;
            assert!(stopped_details.active_run.is_none());
            assert_eq!(stopped_details.summary.active_process_count, 0);
            assert!(matches!(stopped_details.summary.status, InstanceStatus::Stopped));
            assert!(!state.runtime_supervisor.lock().unwrap().is_tracked(&id));
            assert!(state.pending_runtime_start_instance_ids()?.is_empty());
            let after = read_files(&saves)?;
            assert!(!after.is_empty(), "native save inventory must remain nonempty");
            let native_text = fs::read_to_string(&log_path)?;
            let gameplay_audit = settings_acceptance::audit(&native_text, &native["gameSettings"])?;
            write_new(&evidence.join(format!("{phase}-38-gameplay-settings.json")), &serde_json::to_vec_pretty(&gameplay_audit)?)?;
            check_native_world(&native_text, if index == 0 { 1 } else { 11 })?
                .then_some(()).ok_or("fresh native log never completed world loading")?;
            if index == 0 || index == 3 {
                assert!(native_text.contains("[savedata2-migration] Add index file"));
                assert!(native_text.contains("Read result:ok"));
                assert_eq!(read_files(&saves.join("migrated"))?, golden);
                let initial_native = read_files(&saves.join("backup"))?;
                assert!(initial_native.keys().any(|path| path.to_string_lossy().starts_with("world0.ksd.backup-")), "migration must retain a native backup");
            } else {
                assert!(!native_text.contains("[savedata2-migration] Add index file"), "restarts must read the migrated world, not repeat old-format conversion");
            }
            assert!(after.contains_key(Path::new("world0.ksd")));
            // The low-level probe does not publish a Tauri run or AutoStop.
            // Request the actual production backup only after complete exit.
            let backup = create_instance_backup(state.clone(), id.clone()).await?;
            assert_eq!(backup.backup_kind, app_core::InstanceBackupKind::Manual);
            assert_eq!(read_files(&Path::new(&backup.backup_path).join("saves"))?, after);
            write_files(&evidence.join(format!("{phase}-manual-backup")), &read_files(Path::new(&backup.backup_path))?)?;
            capture(&evidence, phase, &stopped_details, &runtime_root, &package.root, &secrets)?;
            let written: Value = serde_json::from_slice(&fs::read(runtime_root.join("enshrouded_server.json"))?)?;
            if written["userGroups"] != native["userGroups"] {
                return Err("native role permissions or generated private credentials changed during typed rewrite".into());
            }
            let native_roster = bans::assert_roster(&runtime_root, index < 2, false)?;
            let managed_roster = bans::assert_roster(Path::new(&stopped_details.config_file_path).parent().ok_or("missing managed config directory")?, index < 2, true)?;
            cycles.push(json!({"phase": phase, "pid": pid, "process_identity": identity, "exit_code": exit_code,
                "native_ban_roster": native_roster, "managed_complete_ban_roster": managed_roster, "known_native_ban_metadata_preserved": true, "managed_unknown_metadata_preserved": true, "all_38_gameplay_effective_values_matched": true, "all_4_role_groups_retained": true,
                "save_changed_from_before_cycle": after != before, "save_changed_from_old_input": after != golden,
                "save_files": after.len(), "save_bytes": after.values().map(Vec::len).sum::<usize>(), "manual_backup_exact": true, "native_world_deserialized_and_running": true,
                "effective_native_flame_level": if index == 0 { 1 } else { 11 }, "running_world_bytes": live_world.len(),
                "native_random_spawner_amount": written["gameSettings"]["randomSpawnerAmount"], "native_flame_level_fog_removal": written["gameSettings"]["flameLevelFogRemoval"]}));
            assert_eq!(read_files(&golden_root)?, golden, "immutable input must never change");
            if exit_code != Some(0) {
                return Err("native graceful stop returned a nonzero exit; inspect preserved evidence".into());
            }
            assert_eq!(written["gameSettingsPreset"], "Custom");
            assert_eq!(written["gameSettings"]["randomSpawnerAmount"], "Automatic");
            assert_eq!(written["gameSettings"]["flameLevelFogRemoval"], if index == 0 { 1 } else { 11 });
            migrated = Some(after);
            details = stopped_details;
            println!("ENSHROUDED_COMPATIBILITY phase={phase} action=stopped pid={pid} exit=0 manual_backup=exact scope=native_loopback_probe");
        }
        Ok(())
    }.await;
    // On a failed start/readiness, stop the owned tree before reading mutable
    // files. Unconfirmed cleanup retains the complete fixture for inspection.
    drop(owned);
    let immutable_check = read_files(&golden_root).and_then(|current| {
        if current == golden {
            Ok(())
        } else {
            Err("immutable old-world input changed".into())
        }
    });
    let captured = capture(
        &evidence,
        "final",
        &details,
        &runtime_root,
        &package.root,
        &secrets,
    );
    let report = json!({"module_id": MODULE, "package": package_identity, "scope": if contract_only { "native_nonempty_ban_contract_exploration" } else { "native_loopback_query_with_acknowledged_udp_and_production_storage_backup_restore" }, "tauri_start_stop_and_auto_backup_tested": false,
        "two_native_udp_transport_ports_acknowledged": allow_native_udp,
        "verification_ok": verification.is_ok(), "error": verification.as_ref().err().map(ToString::to_string),
        "immutable_input_unchanged": immutable_check.is_ok(), "evidence_capture_ok": captured.is_ok(),
        "owned_process_cleanup_confirmed": package.cleanup_allowed.load(Ordering::SeqCst),
        "native_migration_proof": if contract_only { "This is a nonempty configuration-contract probe, not the five-cycle migration acceptance." } else { "Each fresh per-launch log must report successful world deserialization and LoadEcsScene to Run, with effective Custom settings. Old payloads are retained exactly under migrated/. Running world snapshots, manual backups and both pre/post migration restore safeguards are preserved. This tiny sample does not establish complex-world or client gameplay compatibility." },
        "cycles": cycles});
    let recorded = write_new(
        &evidence.join("report.json"),
        &serde_json::to_vec_pretty(&report)?,
    );
    if captured.is_err() || recorded.is_err() {
        package.cleanup_allowed.store(false, Ordering::SeqCst);
    }
    verification?;
    immutable_check?;
    captured?;
    recorded?;
    drop(app);
    drop(env_guard);
    let disposable = package.root.clone();
    drop(package);
    assert!(
        !disposable.exists(),
        "owned compatibility fixture cleanup must complete"
    );
    println!(
        "ENSHROUDED_COMPATIBILITY outcome=native_scoped_probe_passed tauri_start_stop=not_tested evidence={}",
        evidence.display()
    );
    Ok(())
}
