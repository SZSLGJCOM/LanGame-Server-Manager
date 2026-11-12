#![cfg(windows)]

use super::*;
use std::collections::BTreeSet;
use std::io::Read;
use std::os::windows::fs::MetadataExt;

const MODULE: &str = "runescapedragonwilds";
const MARKER: &str = ".lgsm-dragonwilds-bootstrap-fixture";
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
        return Err("bootstrap fixture inputs must be absolute normalized paths".into());
    }
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor)?;
        if metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0 {
            return Err("bootstrap fixture inputs cannot cross links or reparse points".into());
        }
    }
    Ok(path.canonicalize()?)
}

fn text_file(path: &Path) -> ProbeResult<String> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("bootstrap fixture text exceeds 8 MiB".into());
    }
    Ok(String::from_utf8(bytes)?)
}

fn managed_log(path: &Path) -> ProbeResult<String> {
    let mut text = String::new();
    if let Some(segments) = app_storage::managed_console_log::open_log_segments(path)? {
        for segment in segments {
            segment
                .file
                .take(8 * 1024 * 1024)
                .read_to_string(&mut text)?;
        }
    } else {
        text = text_file(path)?;
    }
    Ok(text)
}

async fn observe_start(
    state: &DesktopState,
    storage: &StorageBootstrap,
    id: &str,
    session_path: &Path,
    finished: &std::sync::atomic::AtomicBool,
) -> ProbeResult<(BTreeSet<String>, bool)> {
    let mut sessions = BTreeSet::new();
    let mut pending_without_run = false;
    while !finished.load(Ordering::SeqCst) {
        match fs::read_to_string(session_path) {
            Ok(session) => {
                sessions.insert(session);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        if sessions.len() == 1 {
            let details = read_instance_details(&storage.paths, id).await?;
            pending_without_run |= state
                .pending_runtime_start_instance_ids()?
                .iter()
                .any(|value| value == id)
                && details.active_run.is_none()
                && details.summary.active_process_count == 0
                && !state
                    .runtime_supervisor
                    .lock()
                    .map_err(|_| "runtime supervisor lock poisoned")?
                    .is_tracked(id);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    if session_path.try_exists()? {
        sessions.insert(text_file(session_path)?);
    }
    Ok((sessions, pending_without_run))
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "marked isolated clean package; one production start initializes a new world and applies pending rules before formal readiness, then verifies restart; no old saves"]
async fn isolated_dragonwilds_first_start_applies_pending_world_settings() -> ProbeResult<()> {
    let _guard = command_smoke_lock().lock().await;
    let isolated = plain_existing(&PathBuf::from(
        std::env::var_os("LGSM_DRAGONWILDS_ISOLATED_ROOT")
            .ok_or("supply the marked task-owned input root")?,
    ))?;
    let marker = plain_existing(&isolated.join(MARKER))?;
    let source = plain_existing(&PathBuf::from(
        std::env::var_os("LGSM_DRAGONWILDS_SOURCE_PACKAGE")
            .ok_or("supply the clean local package")?,
    ))?;
    let repository = workspace_root().canonicalize()?;
    if !marker.is_file()
        || !isolated.is_dir()
        || isolated.starts_with(&repository)
        || repository.starts_with(&isolated)
        || !source.is_dir()
        || !source.starts_with(&isolated)
        || source.join("langame_player_query").try_exists()?
        || source.join("langame_player_control").try_exists()?
    {
        return Err("bootstrap inputs must belong to one marked external root; clean package IPC is excluded".into());
    }
    let descriptors = discover_modules(workspace_root().join("modules"))?;
    let descriptor = find_descriptor(&descriptors, MODULE)?;
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
    fs::write(
        package.root.join(MARKER),
        b"owned disposable first-world fixture\n",
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
        "dragon-bootstrap",
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
    assert!(save_inventory(&saves)?.is_empty());
    let world_name = format!("Boot{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let world_password = uuid::Uuid::new_v4().simple().to_string();
    let mut settings: serde_json::Map<String, Value> =
        serde_json::from_str(&initial.settings_json)?;
    settings.insert("owner_id".into(), json!(""));
    settings.insert("server_name".into(), json!("LGSM bootstrap"));
    settings.insert("default_world_name".into(), json!(world_name));
    settings.insert(
        "admin_password".into(),
        json!(uuid::Uuid::new_v4().simple().to_string()),
    );
    settings.insert("world_password".into(), json!(world_password));
    settings.insert("program_update".into(), json!({"policy":"pinned"}));
    settings.insert(
        "extra_launch_args".into(),
        json!("-MULTIHOME=127.0.0.1 -unattended -NoSplash"),
    );
    let details = app_storage::update_instance(
        &storage.paths,
        UpdateInstanceInput {
            id: id.clone(),
            bind_ip: "0.0.0.0".into(),
            auto_backup_on_stop: true,
            backup_retention_count: 4,
            settings_json: serde_json::to_string(&settings)?,
            ports: initial.ports,
        },
    )
    .await?;
    let fresh = app_storage::read_dragonwilds_world_settings(&storage.paths, &id).await?;
    assert_eq!(fresh.status, "new");
    assert!(fresh.writable && fresh.world_file.is_none());
    let pending = crate::commands::commands_dragonwilds_world::write_dragonwilds_world_settings(
        state.clone(),
        app_core::dragonwilds_world::WriteDragonwildsWorldSettingsInput {
            instance_id: id.clone(),
            world_file: None,
            expected_revision: fresh.revision.unwrap(),
            world_mode: app_core::dragonwilds_world::DragonwildsWorldMode::Custom,
            values: [(BUILDING.into(), 1.5)].into(),
        },
    )
    .await?;
    assert_eq!(pending.status, "new");
    assert_eq!(pending.values.get(BUILDING), Some(&1.5));
    assert!(
        save_inventory(&saves)?.is_empty(),
        "saving a draft must not fabricate native world bytes"
    );
    let root = creation::program_root(&details)?.canonicalize()?;
    assert!(root.starts_with(package.root.canonicalize()?));
    let native_log = root.join("RSDragonwilds/Saved/Logs/RSDragonwilds.log");
    let session_path = root.join("langame_player_control/save-session.txt");
    package.cleanup_allowed.store(false, Ordering::SeqCst);
    let finished = std::sync::atomic::AtomicBool::new(false);
    let start = async {
        // This is the complete production startup transaction. The isolated
        // Tauri MockRuntime has no Wry AppHandle, so it omits only UI events.
        let result = Box::pin(start_instance_process_after_reconcile(
            None,
            &state,
            &storage,
            id.clone(),
            "manual",
            None,
        ))
        .await;
        finished.store(true, Ordering::SeqCst);
        result
    };
    let (started, observed) = tokio::join!(
        start,
        observe_start(&state, &storage, &id, &session_path, &finished)
    );
    let started = started?;
    let (sessions, pending_without_run) = observed?;
    assert_eq!(
        sessions.len(),
        2,
        "initialization and formal startup require distinct save identities"
    );
    assert!(
        pending_without_run,
        "bootstrap must stay Starting without a normal service run"
    );
    let bootstrap_log_path =
        Path::new(&started.log_path).with_extension("dragonwilds-bootstrap.log");
    let bootstrap_log = managed_log(&bootstrap_log_path)?;
    let bootstrap_pid = bootstrap_log
        .lines()
        .find_map(|line| {
            line.split_once("Dragonwilds initialization owns native process ")
                .and_then(|(_, tail)| tail.trim().strip_suffix('.'))
                .and_then(|pid| pid.parse::<u32>().ok())
        })
        .ok_or("bootstrap must retain its exact native process receipt")?;
    assert_ne!(bootstrap_pid, started.pid);
    assert!(bootstrap_log.contains(&format!(
        "native process {bootstrap_pid} exited with code 0; its complete owned tree stopped"
    )));
    assert!(!app_runtime::process_is_running(bootstrap_pid)?);
    let loaded = text_file(&native_log)?;
    assert!(loaded.contains(&format!(
        "Loading configured world [{world_name}] from save slot ["
    )));
    assert!(loaded.contains("Load: Success"));
    assert!(loaded.contains(
        "Applied custom setting: Difficulty.Progression.BuildingMaterialCostScale : 1.50"
    ));
    assert!(loaded.contains(
        "LogNetSessionSettings: Setting [\"ReadyToJoin\"] written with key[x0] value[1]"
    ));
    let selected = app_storage::read_dragonwilds_world_settings(&storage.paths, &id).await?;
    assert_eq!(selected.world_name.as_deref(), Some(world_name.as_str()));
    assert_eq!(
        selected.world_mode,
        Some(app_core::dragonwilds_world::DragonwildsWorldMode::Custom)
    );
    assert_eq!(selected.values.get(BUILDING), Some(&1.5));
    assert_eq!(
        selected.values, pending.values,
        "every generated world rule must match its saved preconfiguration"
    );
    let expected_file = selected.world_file.clone();
    let ini =
        text_file(&root.join("RSDragonwilds/Saved/Config/WindowsServer/DedicatedServer.ini"))?;
    assert!(ini.lines().any(|line| line.trim().strip_prefix("WorldPassword=") == Some(world_password.as_str())), "formal start must restore the configured password");
    let backups = app_storage::list_instance_backups(&storage.paths, &id).await?;
    assert_eq!(
        backups.len(),
        1,
        "rule application must back up the generated native world"
    );
    let completed = save_inventory(&saves)?;
    assert!(!completed.is_empty());
    let stopped = Box::pin(stop_instance_process(state.clone(), id.clone(), None)).await?;
    assert_eq!(stopped.exit_code, Some(0));
    assert!(
        stopped
            .processes
            .iter()
            .all(|process| process.exit_code == Some(0))
    );
    assert_eq!(save_inventory(&saves)?, completed);
    let backups_after = app_storage::list_instance_backups(&storage.paths, &id).await?;
    assert_eq!(backups_after.len(), backups.len() + 1);
    let auto = backups_after
        .iter()
        .find(|backup| backup.backup_kind == app_core::InstanceBackupKind::AutoStop)
        .ok_or("normal stop must make its automatic backup")?;
    assert_eq!(
        save_inventory(&Path::new(&auto.backup_path).join("saves"))?,
        completed
    );
    assert!(
        app_storage::prepare_dragonwilds_bootstrap(&storage.paths, &id)
            .await?
            .is_none(),
        "consumed pending settings cannot initialize twice"
    );
    let restarted = Box::pin(start_instance_process_after_reconcile(
        None,
        &state,
        &storage,
        id.clone(),
        "manual",
        None,
    ))
    .await?;
    assert_ne!(started.run_id, restarted.run_id);
    assert!(
        !Path::new(&restarted.log_path)
            .with_extension("dragonwilds-bootstrap.log")
            .exists()
    );
    let selected = app_storage::read_dragonwilds_world_settings(&storage.paths, &id).await?;
    assert_eq!(selected.world_file, expected_file);
    assert_eq!(selected.values.get(BUILDING), Some(&1.5));
    assert_eq!(
        save_inventory(&saves)?,
        completed,
        "same-version restart must not regenerate or rewrite the existing world"
    );
    let stopped = Box::pin(stop_instance_process(state.clone(), id.clone(), None)).await?;
    assert_eq!(stopped.exit_code, Some(0));
    assert!(!state.runtime_supervisor.lock().unwrap().is_tracked(&id));
    assert!(state.pending_runtime_start_instance_ids()?.is_empty());
    package.cleanup_allowed.store(true, Ordering::SeqCst);
    println!(
        "DRAGONWILDS_BOOTSTRAP first_start=passed bootstrap_pid={bootstrap_pid} formal_pid={} bootstrap_native_exit=0 temporary_password_restored=true independent_save_sessions=2 custom_building=1.5 pre_change_backup=present auto_stop_backup=matched restart_existing_world=unchanged",
        started.pid
    );
    drop(cleanup);
    drop(app);
    drop(env_guard);
    let disposable = package.root.clone();
    drop(package);
    assert!(
        !disposable.exists(),
        "owned bootstrap fixture cleanup must complete"
    );
    Ok(())
}
