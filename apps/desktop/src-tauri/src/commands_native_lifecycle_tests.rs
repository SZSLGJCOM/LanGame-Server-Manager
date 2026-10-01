use super::*;
use inventory::save_inventory;
use runtime::print_native_failure;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

#[path = "commands_native_archive.rs"]
mod archive;
#[path = "commands_native_astroneer_probe.rs"]
mod astroneer_console;
#[path = "commands_native_astroneer_readiness.rs"]
mod astroneer_readiness;
#[path = "commands_native_catalog.rs"]
mod catalog;
#[path = "commands_native_creation.rs"]
mod creation;
#[path = "commands_native_diagnostics.rs"]
mod diagnostics;
#[path = "commands_native_environment.rs"]
mod environment;
#[cfg(windows)]
#[path = "commands_existing_instance_acceptance.rs"]
mod existing_instances;
use crate::commands::tests::native_firewall as firewall;
#[path = "commands_native_gm_tools.rs"]
mod gm_tools;
#[path = "commands_native_inventory.rs"]
mod inventory;
#[path = "commands_native_maintenance.rs"]
mod maintenance;
#[path = "commands_native_package.rs"]
mod package;
#[path = "commands_native_player_counts.rs"]
mod player_counts;
#[path = "commands_native_readiness.rs"]
mod readiness;
#[path = "commands_native_runtime.rs"]
mod runtime;
#[path = "commands_native_seed_probe.rs"]
mod seed_probe;
#[path = "commands_native_squad_probe.rs"]
mod squad_probe;
#[path = "commands_native_theforest_probe.rs"]
mod theforest_probe;

struct NativeCleanup<'a> {
    state: &'a DesktopState,
    instance_id: String,
    cleanup_allowed: Arc<AtomicBool>,
}

impl Drop for NativeCleanup<'_> {
    fn drop(&mut self) {
        let Ok(mut supervisor) = self.state.runtime_supervisor.lock() else {
            self.cleanup_allowed.store(false, Ordering::SeqCst);
            return;
        };
        let stopped = supervisor
            .take_running_for_stop(&self.instance_id)
            .map(|mut running| match stop_managed_instance(&mut running) {
                Ok(processes) => {
                    for process in processes.iter().take(8) {
                        let code = process.exit_code.map(|value| format!("0x{:08X}", value as u32));
                        eprintln!(
                            "NATIVE_CLEANUP module={} phase=stopped process={} run_id={} exit_code={:?} exit_hex={} owned_tree=finished",
                            running.summary.module_id,
                            process.process_key,
                            process.run_id,
                            process.exit_code,
                            code.as_deref().unwrap_or("unavailable"),
                        );
                    }
                    true
                }
                Err(_) => false,
            });
        // A failed startup may have never published a supervisor entry. Its
        // absence alone cannot establish that every spawned child was stopped.
        if let Some(stopped) = stopped {
            self.cleanup_allowed.store(stopped, Ordering::SeqCst);
        }
        if !self.cleanup_allowed.load(Ordering::SeqCst) {
            eprintln!("NATIVE_LIFECYCLE cleanup=process_stop_failed");
        }
    }
}

#[test]
fn native_cleanup_does_not_assume_failed_start_was_stopped() {
    let state = DesktopState::default();
    let cleanup_allowed = Arc::new(AtomicBool::new(false));
    drop(NativeCleanup {
        state: &state,
        instance_id: "not-published-start".into(),
        cleanup_allowed: Arc::clone(&cleanup_allowed),
    });
    assert!(!cleanup_allowed.load(Ordering::SeqCst));
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "opt-in: selects one local package, copies into disposable storage, creates instances, verifies production start/stop/restart and retained data; official acquisition is a separate explicit mode"]
async fn native_package_lifecycle() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let descriptors = discover_modules(workspace_root().join("modules"))?;
    let cases = catalog::from_env(&descriptors)?;
    for (module_id, source) in cases {
        let started = Instant::now();
        println!("NATIVE_CATALOG module={module_id} phase=begin");
        Box::pin(run_native_package(module_id.clone(), source, false)).await?;
        println!(
            "NATIVE_CATALOG module={module_id} phase=passed elapsed_ms={}",
            started.elapsed().as_millis()
        );
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "opt-in: isolated local package, production start/readiness/stop; no existing instance or save is used"]
async fn native_start_stop() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let descriptors = discover_modules(workspace_root().join("modules"))?;
    for (module_id, source) in catalog::from_env(&descriptors)? {
        Box::pin(run_native_package(module_id, source, true)).await?;
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "opt-in: isolated local packages and frontend-exported GM commands; verifies production start, native effects, stop and owned-process cleanup"]
async fn native_game_tools() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    if std::env::var("LANGAME_NATIVE_GM_TOOLS").as_deref() != Ok("true") {
        return Err("native game tools require LANGAME_NATIVE_GM_TOOLS=true".into());
    }
    let descriptors = discover_modules(workspace_root().join("modules"))?;
    for (module_id, source) in catalog::from_env(&descriptors)? {
        let started = Instant::now();
        println!("NATIVE_GM_CATALOG module={module_id} phase=begin");
        Box::pin(run_native_package(module_id.clone(), source, true)).await?;
        println!(
            "NATIVE_GM_CATALOG module={module_id} phase=passed elapsed_ms={}",
            started.elapsed().as_millis()
        );
    }
    Ok(())
}

async fn run_native_package(
    module_id: String,
    source: PathBuf,
    cycle_only: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let observation_seconds = diagnostics::observation_seconds()?;
    let descriptors = discover_modules(workspace_root().join("modules"))?;
    let descriptor = find_descriptor(&descriptors, &module_id)?;
    let creation_mode = creation::CreationMode::from_env(descriptor)?;
    let maintenance_mode = maintenance::ProgramMaintenance::from_env(&creation_mode)?;
    if cycle_only
        && (creation_mode.acquisition != creation::Acquisition::LocalPackage
            || creation_mode.instances != creation::InstanceMode::Default)
    {
        return Err("native single-cycle tests require one isolated local package instance".into());
    }
    if creation_mode.acquisition == creation::Acquisition::Official
        && std::env::var_os("LANGAME_NATIVE_STEAMCMD_ROOT").is_some()
    {
        return Err("official native acquisition must use its disposable SteamCMD root; unset LANGAME_NATIVE_STEAMCMD_ROOT".into());
    }
    let mut smoke = readiness::SmokeManifest::load(descriptor)?;
    let elevated = WindowsPlatform::current_process_is_elevated()?;
    let elevated_fixture = firewall::elevated_fixture_allowed(
        &module_id,
        elevated,
        std::env::var("LANGAME_NATIVE_ALLOW_ELEVATED_SCUM")
            .ok()
            .as_deref(),
    )?;
    let offline_dst = module_id == "dontstarve"
        && std::env::var("LANGAME_NATIVE_DST_OFFLINE_LAN").as_deref() == Ok("true");
    for precondition in &smoke.preconditions {
        if offline_dst && precondition.kind == "secret" && precondition.id == "dst-cluster-token" {
            continue;
        }
        match precondition.kind.as_str() {
            "operator_acknowledgement" => {
                let key = precondition.environment.as_deref().ok_or("missing acknowledgement environment")?;
                if std::env::var(key).ok().as_deref() != precondition.expected.as_deref() {
                    return Err(format!("native prerequisite: {}", precondition.reason_code).into());
                }
                // This propagates the caller's explicit prior acknowledgement;
                // the runner never creates or changes the acknowledgement env.
                smoke.fixture.settings.insert(precondition.setting_key.clone().ok_or("missing acknowledged setting")?, json!(true));
            }
            "elevation" => {
                if !elevated {
                    return Err(format!("native prerequisite: {}", precondition.reason_code).into());
                }
            }
            "secret" => return Err("DST native lifecycle requires explicit offline LAN mode; no online credential is used".into()),
            _ => return Err("unsupported native prerequisite".into()),
        }
    }
    let source = source.canonicalize()?;
    let excluded = match descriptor.storage.saves_path_template.as_deref() {
        Some(template) if template.contains("paths.install_root") => {
            let data = app_storage::install_save_directory_prefix(template, &source)
                .ok_or("cannot safely separate existing world data from the native package")?;
            Some(data.strip_prefix(&source)?.to_path_buf())
        }
        _ => None,
    };
    let max_bytes = std::env::var("LANGAME_NATIVE_COPY_MAX_BYTES")
        .ok()
        .map(|value| value.parse::<u64>())
        .transpose()?
        .unwrap_or(64 * 1024 * 1024 * 1024);
    let copy_seconds = std::env::var("LANGAME_NATIVE_COPY_TIMEOUT_SECONDS")
        .ok()
        .map(|value| value.parse::<u64>())
        .transpose()?
        .unwrap_or(1200);
    if max_bytes == 0 || max_bytes > 256 * 1024 * 1024 * 1024 || !(1..=3600).contains(&copy_seconds)
    {
        return Err("native copy limits are invalid".into());
    }
    let copy_module = module_id.clone();
    let copy_started = Instant::now();
    let package = tokio::task::spawn_blocking(move || {
        package::copy_package(
            &source,
            &copy_module,
            excluded.as_deref(),
            max_bytes,
            Duration::from_secs(copy_seconds),
        )
    })
    .await??;
    println!(
        "NATIVE_LIFECYCLE module={module_id} phase=package_copy bytes={} elapsed_ms={} acquisition={} creation_scope={} source_provenance=unverified",
        package.copied_bytes,
        copy_started.elapsed().as_millis(),
        creation_mode.acquisition_label(),
        creation_mode.creation_scope(),
    );
    let env_guard = environment::NativeEnvironment::set(&package.root)?;
    save_app_settings(AppSettings {
        archives_root: String::new(),
        // The acquisition coordinator requires its clean library staging to
        // stay outside the instances root. Storage-only local-package probes
        // can use sibling leaves to leave more room for deep native DLL paths.
        servers_root: if creation_mode.acquisition == creation::Acquisition::Official {
            package.root.join("i")
        } else {
            package.root.clone()
        }
        .to_string_lossy()
        .into_owned(),
        games_root: package.root.to_string_lossy().into_owned(),
        modules_root: workspace_root()
            .join("modules")
            .to_string_lossy()
            .into_owned(),
        steamcmd_root: match std::env::var_os("LANGAME_NATIVE_STEAMCMD_ROOT") {
            Some(path) => {
                let path = PathBuf::from(path).canonicalize()?;
                if !path.is_dir() {
                    return Err("configured native tool root must be an existing directory".into());
                }
                path.to_string_lossy().into_owned()
            }
            None => package.root.join("steamcmd").to_string_lossy().into_owned(),
        },
    })?;
    let storage = bootstrap_storage()?;
    initialize_database(&storage.paths).await?;
    sync_modules(&storage.paths, &descriptors).await?;
    app_storage::sync_game_installs(
        &storage.paths,
        &[app_storage::GameInstallSyncRecord {
            module_id: module_id.clone(),
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
    let mut seed_probe = None;
    let first_instance = if creation_mode.instances != creation::InstanceMode::Default {
        let first = creation_mode
            .create(state.clone(), &storage, descriptor, &package, "peer")
            .await?;
        if creation_mode.instances == creation::InstanceMode::SecondPrivate {
            let probe = seed_probe::SeedProbe::prepare(&first, descriptor, &package.root).await?;
            // Keep the first program modified. The second creation must obtain
            // an official source automatically without changing this instance.
            seed_probe = Some(probe);
        }
        let config = fs::read(&first.config_file_path)?;
        let saves = save_inventory(Path::new(&first.saves_path))?;
        let programs = inventory::program_files(descriptor, &creation::program_root(&first)?)?;
        Some((first, config, saves, programs))
    } else {
        None
    };
    let initial = creation_mode
        .create(state.clone(), &storage, descriptor, &package, "n")
        .await?;
    let id = initial.summary.id.clone();
    let effective_install_root = creation::program_root(&initial)?;
    if let Some((first, ..)) = &first_instance {
        creation_mode.verify_pair(first, &initial)?;
    }
    if let Some(probe) = &seed_probe {
        probe.verify(&initial, descriptor).await?;
    }
    // Declared first so error unwinding stops the server before removing rules.
    let mut firewall_cleanup = None;
    let cleanup = NativeCleanup {
        state: &state,
        instance_id: id.clone(),
        cleanup_allowed: Arc::clone(&package.cleanup_allowed),
    };
    let mut settings: serde_json::Map<String, Value> =
        serde_json::from_str(&initial.settings_json)?;
    settings.extend(smoke.fixture.settings.clone());
    settings.extend(diagnostics::fixture_settings_override(
        std::env::var("LANGAME_NATIVE_SETTINGS_JSON")
            .ok()
            .as_deref(),
    )?);
    for name in ["public_server", "list_on_steam", "list_on_eos"] {
        if let Some(value) = settings.get_mut(name) {
            *value = if value.is_boolean() {
                json!(false)
            } else if value.is_number() {
                json!(0)
            } else {
                value.clone()
            };
        }
    }
    if module_id == "sevendaystodie" {
        settings.insert("visibility".into(), json!(0));
    }
    if module_id == "astroneer" {
        // Shipping supports ABSLOG; keep early-exit diagnostics inside this
        // disposable runtime even when the native console stays empty.
        let log = effective_install_root.join("Astro/Saved/Logs/Astro.log");
        settings.insert(
            "extra_launch_args".into(),
            json!(format!("-ABSLOG=\"{}\"", log.display())),
        );
        println!("NATIVE_DIAGNOSTIC module=astroneer file_logging=explicit_fixture_path");
    }
    if offline_dst {
        for (key, value) in [
            ("offline_cluster", json!(true)),
            ("lan_only_cluster", json!(true)),
            ("enable_caves", json!(true)),
            ("cluster_token", json!("")),
            ("disable_data_collection", json!(true)),
            ("master_world_size", json!("small")),
            ("caves_world_size", json!("small")),
        ] {
            settings.insert(key.into(), value);
        }
    }
    readiness::ark_maps::apply_fixture_maps(
        &module_id,
        &mut settings,
        std::env::var("LANGAME_NATIVE_ARK_MAPS").ok().as_deref(),
    )?;
    let details = app_storage::update_instance(
        &storage.paths,
        UpdateInstanceInput {
            id: id.clone(),
            bind_ip: "0.0.0.0".into(),
            auto_backup_on_stop: false,
            backup_retention_count: 2,
            settings_json: serde_json::to_string(&settings)?,
            ports: initial.ports,
        },
    )
    .await?;
    let saves_before_start = save_inventory(Path::new(&details.saves_path))?;
    if elevated_fixture {
        firewall_cleanup = Some(firewall::NativeFirewall::prepare(&details, &package.root)?);
    }
    let runtime = runtime::NativeRuntime {
        app: app.handle(),
        state: &state,
        storage: &storage,
        descriptor,
        smoke: &smoke,
        package: &package,
        settings: &settings,
        effective_install_root: &effective_install_root,
        offline_dst,
        elevated_fixture,
    };
    if cycle_only {
        // This selector has its own explicit scope. It neither invokes nor
        // weakens the separate archive/restore/uninstall lifecycle acceptance.
        Box::pin(runtime.run_cycle(&details, "initial", observation_seconds)).await?;
        drop(cleanup);
        if let Some(mut firewall) = firewall_cleanup {
            firewall.finish()?;
        }
        drop(app);
        drop(env_guard);
        let disposable_root = package.root.clone();
        drop(package);
        if disposable_root.exists() {
            return Err("native start/stop passed but disposable directory cleanup failed".into());
        }
        println!(
            "NATIVE_LIFECYCLE module={module_id} phase=complete scope=production_start_stop cleanup=passed gui=not_tested"
        );
        return Ok(());
    }
    // Reuse the acquired official source before startup or an explicit update
    // invalidates its original package inventory. Later phases still archive,
    // restore, launch and delete the actual maintained instance.
    Box::pin(archive::verify_retirement_with_library(&runtime, &details)).await?;
    // Keep independent lifecycle phases out of this long fixture's state. A
    // cycle includes production startup and observation futures; embedding two
    // cycles exhausted the default Windows test stack in the native run.
    let first_run = Box::pin(runtime.run_cycle(&details, "initial", observation_seconds)).await?;
    let stopped_config = fs::read(&details.config_file_path)?;
    let reloaded = read_instance_details_from_storage(state.clone(), id.clone()).await?;
    if reloaded.settings_json != details.settings_json
        || serde_json::to_value(&reloaded.ports)? != serde_json::to_value(&details.ports)?
        || reloaded.saves_path != details.saves_path
        || creation::program_root(&reloaded)? != effective_install_root
        || fs::read(&reloaded.config_file_path)? != stopped_config
    {
        return Err("native configuration or program root changed during stopped readback".into());
    }
    // run_cycle captures new log/status baselines on every invocation. The
    // first run's readiness output cannot satisfy the restarted run.
    let second_run = Box::pin(runtime.run_cycle(&reloaded, "restart", 0)).await?;
    if first_run == second_run {
        return Err("native restart reused its previous persisted run identity".into());
    }
    println!(
        "NATIVE_LIFECYCLE module={module_id} phase=restarted persisted_readback=verified fresh_run=verified"
    );
    if Box::pin(maintenance::verify_backup_restore(&runtime, &reloaded)).await? {
        let restored_run = Box::pin(runtime.run_cycle(&reloaded, "after_restore", 0)).await?;
        if restored_run == second_run {
            return Err("native restore restart reused its previous persisted run identity".into());
        }
    }
    if Box::pin(maintenance::verify_program_maintenance(
        &runtime,
        &reloaded,
        maintenance_mode,
    ))
    .await?
    {
        Box::pin(runtime.run_cycle(&reloaded, "after_program_maintenance", 0)).await?;
    }
    let saves = save_inventory(Path::new(&details.saves_path))?;
    // Some servers wait for a player before creating a world. Still exercise
    // their empty-data uninstall path, without claiming native save generation.
    let observation = if saves.is_empty() {
        "none"
    } else if saves == saves_before_start {
        "unchanged"
    } else {
        "generated"
    };
    println!("NATIVE_LIFECYCLE module={module_id} phase=stopped save_observation={observation}");
    let backup = if saves.is_empty() {
        None
    } else {
        let backup = crate::commands::create_instance_backup(state.clone(), id.clone()).await?;
        if backup.file_count == 0
            || save_inventory(&Path::new(&backup.backup_path).join("saves"))? != saves
        {
            return Err("native save backup does not contain the stopped save bytes".into());
        }
        Some(backup)
    };
    let stopped_config = fs::read(&details.config_file_path)?;
    let native_configs = descriptor
        .storage
        .retained_paths
        .iter()
        .map(|relative| {
            let path = effective_install_root.join(relative);
            save_inventory(&path).map(|inventory| (path, inventory))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let retained_root = creation::instance_root(&details)?.join("installation-retained");
    let retained_installation = save_inventory(&retained_root)?;
    let programs = inventory::program_files(descriptor, &effective_install_root)?;
    let library = app_storage::read_library_program_install(&storage.paths, &module_id)
        .await?
        .ok_or("native uninstall has no registered library")?;
    let shared = !crate::commands::commands_program_storage::shared_program_references(
        &storage.paths,
        &module_id,
        &library.install_root,
    )
    .await?
    .is_empty();
    let uninstall = uninstall_module_game(app.handle().clone(), module_id.clone()).await;
    if shared {
        let error = uninstall
            .err()
            .ok_or("library uninstall succeeded while a shared instance still references it")?;
        if !error.contains("仍有实例使用此服务器程序") {
            return Err(
                "library uninstall failed without reaching the shared-reference guard".into(),
            );
        }
    } else {
        uninstall?;
    }
    if inventory::program_files(descriptor, &effective_install_root)? != programs
        || save_inventory(&retained_root)? != retained_installation
    {
        return Err(
            "library uninstall changed an instance program or retained installation data".into(),
        );
    }
    if save_inventory(Path::new(&details.saves_path))? != saves {
        return Err("native uninstallation changed declared save data".into());
    }
    if fs::read(&details.config_file_path)? != stopped_config {
        return Err("native uninstallation changed instance configuration".into());
    }
    for (path, inventory) in &native_configs {
        if save_inventory(path)? != *inventory {
            return Err("native uninstallation changed retained native configuration".into());
        }
    }
    println!(
        "NATIVE_LIFECYCLE module={module_id} phase=library_uninstall outcome={} program_entries=preserved saves_preserved={} native_config_files_preserved={}",
        if shared {
            "shared_reference_rejected"
        } else {
            "library_only_instance_program_unchanged"
        },
        saves.len(),
        native_configs
            .iter()
            .map(|(_, files)| files.len())
            .sum::<usize>()
    );
    let deleted = archive_instance_record(app.handle().clone(), id.clone()).await?;
    let archive = PathBuf::from(
        deleted
            .archived_instance_root
            .as_ref()
            .ok_or("instance archiving did not create an archive")?,
    );
    archive::verify_archived_native_data(
        &storage.paths,
        &deleted.archive_id,
        Path::new(&details.saves_path),
        Path::new(&deleted.previous_instance_root),
        &archive,
        &saves,
        "save data",
    )
    .await?;
    for (path, inventory) in &native_configs {
        archive::verify_archived_native_data(
            &storage.paths,
            &deleted.archive_id,
            path,
            Path::new(&deleted.previous_instance_root),
            &archive,
            inventory,
            "retained native configuration",
        )
        .await?;
    }
    let backup_preserved = backup.as_ref().is_none_or(|backup| {
        archive
            .join("backups")
            .join(&backup.backup_id)
            .join("backup.json")
            .is_file()
    });
    if !backup_preserved {
        return Err("native instance archiving did not retain saves and backup".into());
    }
    if let Some(backup) = &backup
        && save_inventory(
            &archive
                .join("backups")
                .join(&backup.backup_id)
                .join("saves"),
        )? != saves
    {
        return Err("native instance archiving changed backed-up save bytes".into());
    }
    if fs::read(archive.join("config/instance.json"))? != stopped_config
        || save_inventory(&archive.join("installation-retained"))? != retained_installation
        || Path::new(&deleted.previous_instance_root).exists()
        || list_instances(&storage.paths)
            .await?
            .iter()
            .any(|instance| instance.id == id)
    {
        return Err("native instance archiving did not archive configuration and retained installation data or remove the active record".into());
    }
    println!("NATIVE_LIFECYCLE module={module_id} phase=archived archive=verified");
    let restored = Box::pin(archive::restore_and_verify(
        &runtime, &details, &archive, &saves, &programs,
    ))
    .await?;
    Box::pin(runtime.run_cycle(&restored, "after_archive_restore", 0)).await?;
    Box::pin(archive::delete_restored(&runtime, &restored)).await?;
    if let Some((first, config, saves, programs)) = first_instance {
        if let Some(probe) = &seed_probe {
            probe.verify_first_preserved()?;
        }
        if read_active_instance_run(&storage.paths, &first.summary.id)
            .await?
            .is_some()
            || fs::read(&first.config_file_path)? != config
            || save_inventory(Path::new(&first.saves_path))? != saves
            || inventory::program_files(descriptor, &creation::program_root(&first)?)? != programs
        {
            return Err(
                "native lifecycle changed its unstarted peer's program, configuration or saves"
                    .into(),
            );
        }
        Box::pin(archive::delete_restored(&runtime, &first)).await?;
        if !list_instances(&storage.paths).await?.is_empty() {
            return Err("native peer deletion did not release its instance association".into());
        }
        if let Some(probe) = &seed_probe {
            probe.verify_unknown_mod_preserved()?;
        }
        println!(
            "NATIVE_LIFECYCLE module={module_id} phase=first_instance_deleted library=preserved remaining_instances=0"
        );
    }
    drop(cleanup);
    if let Some(mut firewall) = firewall_cleanup {
        firewall.finish()?;
        println!("NATIVE_LIFECYCLE module={module_id} phase=firewall_cleaned exact_rules=verified");
    }
    drop(app);
    drop(env_guard);
    let disposable_root = package.root.clone();
    drop(package);
    if disposable_root.exists() {
        return Err("native lifecycle passed but disposable directory cleanup failed".into());
    }
    println!(
        "NATIVE_LIFECYCLE module={module_id} phase=complete cleanup=passed lifecycle_scope=production_start_stop_restart creation_scope={} acquisition={} gui=not_tested",
        creation_mode.creation_scope(),
        creation_mode.acquisition_label(),
    );
    Ok(())
}
