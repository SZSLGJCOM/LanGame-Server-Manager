#![cfg(windows)]

use super::*;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::os::windows::fs::MetadataExt;

const MODULE: &str = "runescapedragonwilds";
const MARKER: &str = ".lgsm-dragonwilds-compatibility-fixture";
const BUILDING: &str = "Difficulty.Progression.BuildingMaterialCostScale";
type ProbeResult<T> = Result<T, Box<dyn std::error::Error>>;

fn plain_existing(path: &Path) -> ProbeResult<PathBuf> {
    if !path.is_absolute()
        || path.components().any(|part| {
            matches!(
                part,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        return Err("compatibility inputs must be absolute normalized paths".into());
    }
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor)?;
        if metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0 {
            return Err("compatibility inputs cannot cross links or reparse points".into());
        }
    }
    Ok(path.canonicalize()?)
}

fn bounded_bytes(path: &Path, maximum: u64) -> ProbeResult<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(maximum + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum {
        return Err("compatibility input exceeds its read bound".into());
    }
    Ok(bytes)
}

// Keep actual bytes for the stopped-save and backup comparisons. The existing
// inventory checks links, entries and depth before this small fixture is read.
fn save_bytes(root: &Path) -> ProbeResult<BTreeMap<PathBuf, Vec<u8>>> {
    let inventory = save_inventory(root)?;
    if inventory.len() > 256
        || inventory.values().map(|(bytes, _)| bytes).sum::<u64>() > 256 * 1024 * 1024
    {
        return Err("compatibility saves exceed the 256-entry / 256 MiB fixture bound".into());
    }
    inventory
        .into_iter()
        .map(|(relative, (length, _))| {
            let bytes = bounded_bytes(&root.join(&relative), length)?;
            if bytes.len() as u64 != length {
                return Err("compatibility save changed while reading".into());
            }
            Ok((relative, bytes))
        })
        .collect()
}

fn native_log(path: &Path) -> ProbeResult<String> {
    match fs::metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error.into()),
        Ok(_) => Ok(String::from_utf8(bounded_bytes(path, 8 * 1024 * 1024)?)?),
    }
}

async fn wait_log(
    state: &DesktopState,
    id: &str,
    path: &Path,
    timeout: Duration,
    matches: impl Fn(&str) -> bool,
) -> ProbeResult<String> {
    let deadline = Instant::now() + timeout;
    loop {
        let log = native_log(path)?;
        if matches(&log) {
            return Ok(log);
        }
        if state
            .runtime_supervisor
            .lock()
            .map_err(|_| "runtime supervisor lock poisoned")?
            .instance_process_tree_is_running(id)?
            != Some(true)
        {
            return Err("owned compatibility server exited before its native evidence".into());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "native compatibility evidence timed out; inspect {}",
                path.display()
            )
            .into());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "explicit marked isolated package/save inputs; production start/stop, completed autosave backup and restart; no downloads or real installation changes"]
async fn isolated_dragonwilds_production_stop_backs_up_completed_autosave_and_restarts()
-> ProbeResult<()> {
    let _guard = command_smoke_lock().lock().await;
    let isolated = plain_existing(&PathBuf::from(
        std::env::var_os("LGSM_DRAGONWILDS_ISOLATED_ROOT")
            .ok_or("supply the marked task-owned input root")?,
    ))?;
    let marker = plain_existing(&isolated.join(MARKER))?;
    let source = plain_existing(&PathBuf::from(
        std::env::var_os("LGSM_DRAGONWILDS_SOURCE_PACKAGE")
            .ok_or("supply the isolated clean local package")?,
    ))?;
    let golden_path = plain_existing(&PathBuf::from(
        std::env::var_os("LGSM_DRAGONWILDS_SAVE_FIXTURE")
            .ok_or("supply an immutable isolated Custom world copy with Building=1.5")?,
    ))?;
    let repository = workspace_root().canonicalize()?;
    if !marker.is_file()
        || !isolated.is_dir()
        || isolated.starts_with(&repository)
        || repository.starts_with(&isolated)
        || !source.is_dir()
        || !source.starts_with(&isolated)
        || !golden_path.is_file()
        || !golden_path.starts_with(&isolated)
        || source.join("langame_player_query").try_exists()?
    {
        return Err(
            "compatibility inputs must belong to one marked external root; package IPC is excluded"
                .into(),
        );
    }
    let golden = bounded_bytes(&golden_path, 128 * 1024 * 1024)?;
    let expected_world = std::env::var("LGSM_DRAGONWILDS_EXPECTED_WORLD_NAME")?;
    if expected_world.is_empty() || expected_world.chars().any(char::is_control) {
        return Err("supply the existing world's name".into());
    }
    let descriptors = discover_modules(workspace_root().join("modules"))?;
    let descriptor = find_descriptor(&descriptors, MODULE)?;
    assert!(
        descriptor
            .process
            .as_ref()
            .unwrap()
            .args_template
            .iter()
            .any(|arg| arg.eq_ignore_ascii_case("-NewConsole"))
    );
    let shutdown = descriptor.runtime.shutdown.as_ref().unwrap();
    assert_eq!(shutdown.commands.len(), 1);
    assert_eq!(shutdown.commands[0].transport, "unreal_console");
    assert_eq!(shutdown.commands[0].command, "quit");
    assert!(shutdown.commands[0].fallback_transport.is_none());

    // Source Saved is never copied. A clean package with no IPC is required;
    // only the explicitly supplied immutable save enters the created instance.
    let source_copy = source.clone();
    let package = tokio::task::spawn_blocking(move || {
        package::copy_package(
            &source_copy,
            MODULE,
            Some(Path::new("RSDragonwilds/Saved")),
            16 * 1024 * 1024 * 1024,
            Duration::from_secs(600),
        )
    })
    .await??;
    assert!(!package.install_root.join("RSDragonwilds/Saved").exists());
    assert!(!package.install_root.join("langame_player_query").exists());
    fs::write(
        package.root.join(MARKER),
        b"owned disposable production compatibility fixture\n",
    )?;
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
    let initial = creation::CreationMode {
        acquisition: creation::Acquisition::LocalPackage,
        instances: creation::InstanceMode::Default,
    }
    .create(
        state.clone(),
        &storage,
        descriptor,
        &package,
        "dragon-compat",
    )
    .await?;
    let id = initial.summary.id.clone();
    let cleanup = NativeCleanup {
        state: &state,
        instance_ids: vec![id.clone()],
        never_started_instance_ids: Default::default(),
        cleanup_allowed: Arc::clone(&package.cleanup_allowed),
    };
    let saves = PathBuf::from(&initial.saves_path);
    assert!(
        saves
            .canonicalize()?
            .starts_with(package.root.canonicalize()?)
    );
    assert!(
        save_bytes(&saves)?.is_empty(),
        "new instance must not inherit source saves"
    );
    let imported = saves.join("ImportedWorld.sav");
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&imported)?
        .write_all(&golden)?;
    let mut settings: serde_json::Map<String, Value> =
        serde_json::from_str(&initial.settings_json)?;
    settings.insert("owner_id".into(), json!(""));
    settings.insert("server_name".into(), json!("LGSM isolated"));
    settings.insert("default_world_name".into(), json!(expected_world));
    settings.insert(
        "admin_password".into(),
        json!(uuid::Uuid::new_v4().simple().to_string()),
    );
    settings.insert(
        "world_password".into(),
        json!(uuid::Uuid::new_v4().simple().to_string()),
    );
    // Pinned is the production policy: no fake SteamCMD readiness or downloads.
    settings.insert("program_update".into(), json!({"policy": "pinned"}));
    let mut details = initial;
    let mut previous_run = None;
    for pass in 0..2 {
        let log_path = package
            .root
            .join(format!("dragon-compatibility-{pass}.log"));
        settings.insert(
            "extra_launch_args".into(),
            json!(format!(
                "-MULTIHOME=127.0.0.1 -unattended -NoSplash -abslog=\"{}\"",
                log_path.display()
            )),
        );
        app_storage::update_instance(
            &storage.paths,
            UpdateInstanceInput {
                id: id.clone(),
                // This module only declares the default managed bind. The
                // explicit fixture-only MULTIHOME argument limits native UDP.
                bind_ip: "0.0.0.0".into(),
                auto_backup_on_stop: true,
                backup_retention_count: 4,
                settings_json: serde_json::to_string(&settings)?,
                ports: details.ports,
            },
        )
        .await?;
        let selected = app_storage::read_dragonwilds_world_settings(&storage.paths, &id).await?;
        assert_eq!(
            selected.world_name.as_deref(),
            Some(expected_world.as_str())
        );
        assert_eq!(selected.values.get(BUILDING), Some(&1.5));
        let before = save_bytes(&saves)?;
        let backups_before = app_storage::list_instance_backups(&storage.paths, &id).await?;
        package.cleanup_allowed.store(false, Ordering::SeqCst);
        let started = Box::pin(start_instance_process_after_reconcile(
            None,
            &state,
            &storage,
            id.clone(),
            "manual",
            None,
        ))
        .await?;
        assert!(
            previous_run != Some(started.run_id),
            "restart must publish a new run"
        );
        previous_run = Some(started.run_id);
        let load_marker = format!("Loading configured world [{expected_world}] from save slot [");
        let ready_log = wait_log(&state, &id, &log_path, Duration::from_secs(180), |log| {
            log.contains(&load_marker) && log.contains("Load: Success")
                && log.contains("Applied custom setting: Difficulty.Progression.BuildingMaterialCostScale : 1.50")
                && log.contains("LogNetSessionSettings: Setting [\"ReadyToJoin\"] written with key[x0] value[1]")
        }).await?;
        let completed = if pass == 0 {
            assert!(
                ready_log.contains("dom.StateSaveFrequencyMins:5"),
                "the 360-second fixture budget requires the actual five-minute native configuration"
            );
            wait_log(&state, &id, &log_path, Duration::from_secs(360), |log| {
                log.strip_prefix(&ready_log).is_some_and(|fresh| {
                    fresh.lines().any(|line| {
                        line.contains("Save completed SUCCESSFULLY (slot:")
                            || (line.contains("Save to slot ") && line.ends_with(": Success"))
                    })
                })
            })
            .await?;
            let completed = save_bytes(&saves)?;
            assert_ne!(
                completed, before,
                "fresh native autosave must change the stored world bytes"
            );
            completed
        } else {
            save_bytes(&saves)?
        };
        let before_stop = native_log(&log_path)?;
        let stopped = Box::pin(stop_instance_process(state.clone(), id.clone())).await?;
        assert_eq!(stopped.run_id, started.run_id);
        assert_eq!(stopped.exit_code, Some(0));
        assert!(!stopped.processes.is_empty());
        assert!(
            stopped
                .processes
                .iter()
                .all(|process| process.exit_code == Some(0))
        );
        let stopped_log = native_log(&log_path)?;
        let shutdown_log = stopped_log
            .strip_prefix(&before_stop)
            .ok_or("native log was replaced during stop")?;
        assert!(
            shutdown_log.contains("PreExit Game.")
                && shutdown_log
                    .contains("Draining save requests before world teardown (world teardown)")
                && shutdown_log.contains("LogExit: Exiting."),
            "quit must drain the native exit path"
        );
        // quit is not a save request. Preserve the last completed autosave.
        assert_eq!(save_bytes(&saves)?, completed);
        let backups = app_storage::list_instance_backups(&storage.paths, &id).await?;
        let fresh: Vec<_> = backups
            .iter()
            .filter(|backup| {
                !backups_before
                    .iter()
                    .any(|old| old.backup_id == backup.backup_id)
            })
            .collect();
        assert_eq!(
            fresh.len(),
            1,
            "production stop must create its automatic backup"
        );
        assert_eq!(fresh[0].backup_kind, app_core::InstanceBackupKind::AutoStop);
        assert_eq!(
            save_bytes(&Path::new(&fresh[0].backup_path).join("saves"))?,
            completed
        );
        details = read_instance_details(&storage.paths, &id).await?;
        assert!(matches!(details.summary.status, InstanceStatus::Stopped));
        assert_eq!(details.summary.active_process_count, 0);
        assert!(details.active_run.is_none());
        assert!(!state.runtime_supervisor.lock().unwrap().is_tracked(&id));
        assert!(state.pending_runtime_start_instance_ids()?.is_empty());
        assert_eq!(bounded_bytes(&golden_path, 128 * 1024 * 1024)?, golden);
        package.cleanup_allowed.store(true, Ordering::SeqCst);
        println!(
            "DRAGONWILDS_COMPATIBILITY pass={pass} production_start_stop=passed exit=0 native_pre_exit=true auto_backup=exact_bytes restart_same_world=true graceful_stop_requests_save=false"
        );
    }
    drop(cleanup);
    drop(app);
    drop(env_guard);
    let disposable = package.root.clone();
    drop(package);
    assert!(
        !disposable.exists(),
        "owned compatibility fixture cleanup must complete"
    );
    Ok(())
}
