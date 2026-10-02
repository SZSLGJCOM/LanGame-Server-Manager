use super::commands_assistant_ops::*;
use super::commands_broadcast::*;
use super::commands_runtime_lifecycle::*;
use super::commands_runtime_observability::*;
use super::commands_stdin_dispatch::*;
use super::commands_storage::*;
use super::*;
use app_core::AppSettings;
use std::env;
use std::ffi::OsString;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::Manager;

#[path = "commands_ark_tools_tests.rs"]
mod ark_tools_tests;

#[cfg(windows)]
#[path = "commands_native_firewall.rs"]
mod native_firewall;

#[path = "commands_assistant_bind_ip_tests.rs"]
mod assistant_bind_ip_tests;
#[path = "commands_assistant_clarification_workflow_tests.rs"]
mod assistant_clarification_workflow_tests;
#[path = "commands_assistant_conversation_tests.rs"]
mod assistant_conversation_tests;
#[path = "commands_assistant_intent_workflow_tests.rs"]
mod assistant_intent_workflow_tests;
#[path = "commands_assistant_launch_workflow_tests.rs"]
mod assistant_launch_workflow_tests;
#[path = "commands_assistant_requirements_workflow_tests.rs"]
mod assistant_requirements_workflow_tests;
#[path = "commands_assistant_start_ports_tests.rs"]
mod assistant_start_ports_tests;
#[path = "commands_assistant_task_workflow_tests.rs"]
mod assistant_task_workflow_tests;
#[path = "commands_assistant_tool_fixtures.rs"]
mod assistant_tool_fixtures;
#[path = "commands_assistant_workflow_tests.rs"]
mod assistant_workflow_tests;

#[path = "commands_assistant_live_acceptance_tests.rs"]
mod assistant_live_acceptance_tests;

#[path = "commands_assistant_mod_evidence_tests.rs"]
mod assistant_mod_evidence_tests;

#[path = "commands_assistant_runtime_dispatch_tests.rs"]
mod assistant_runtime_dispatch_tests;

#[path = "command_smoke_paths.rs"]
mod real_smoke_support;
#[path = "commands_workshop_collection_removal_tests.rs"]
mod workshop_collection_removal_tests;

static COMMAND_SMOKE_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
static TEMP_TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn temp_test_dir(label: &str) -> PathBuf {
    const MAX_ATTEMPTS: usize = 256;

    let short_label = label
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(12)
        .collect::<String>();
    for _ in 0..MAX_ATTEMPTS {
        let sequence = TEMP_TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!(
            "lg-test-{short_label}-{:x}-{sequence:x}",
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return path,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!(
                "failed to create test directory {}: {error}",
                path.display()
            ),
        }
    }
    panic!("failed to allocate a unique test directory after {MAX_ATTEMPTS} attempts")
}

pub(super) fn command_smoke_lock() -> &'static tokio::sync::Mutex<()> {
    COMMAND_SMOKE_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

#[tokio::test]
async fn app_exit_joins_the_registered_lan_directory_thread() {
    let state = DesktopState::default();
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let thread_cancel = Arc::clone(&cancel);
    let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let thread_finished = Arc::clone(&finished);
    let directory_thread = std::thread::spawn(move || {
        while !thread_cancel.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(1));
        }
        thread_finished.store(true, Ordering::SeqCst);
    });

    state
        .register_lan_directory_worker(crate::state::LanDirectoryWorker::new(
            cancel,
            directory_thread,
        ))
        .expect("register LanGame LAN directory worker");
    join_lan_directory_for_app_exit(&state)
        .await
        .expect("join LanGame LAN directory thread");

    assert!(finished.load(Ordering::SeqCst));
    assert!(
        state
            .take_lan_directory_worker()
            .expect("read LanGame LAN directory worker slot")
            .is_none()
    );
}

#[tokio::test]
async fn app_exit_joins_the_registered_lan_host_thread() {
    let state = DesktopState::default();
    let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let thread_finished = Arc::clone(&finished);
    let host_thread = std::thread::spawn(move || {
        thread_finished.store(true, Ordering::SeqCst);
    });

    state
        .register_lan_host_thread(host_thread)
        .expect("register LAN host thread");
    join_lan_host_for_app_exit(&state)
        .await
        .expect("join LAN host thread");

    assert!(finished.load(Ordering::SeqCst));
    assert!(
        state
            .take_lan_host_thread()
            .expect("read LAN host thread slot")
            .is_none()
    );
}

fn stored_openai_compatible_ai_settings()
-> Result<AssistantProviderSettings, Box<dyn std::error::Error>> {
    let settings = AssistantProviderSettings {
        provider: String::from("openai-compatible"),
        model: String::from("Gemini-3.5-Flash"),
        base_url: String::from("https://api.poe.com/v1"),
        api_key: String::new(),
    };
    let secret_status = read_secret_status(&AssistantSecretDescriptor {
        provider: settings.provider.clone(),
        base_url: settings.base_url.clone(),
    })?;
    if !secret_status.stored {
        return Err(std::io::Error::other(
            "stored OpenAI-compatible AI key was not found in the system keyring",
        )
        .into());
    }
    Ok(settings)
}

fn stored_openai_compatible_ai_mock_settings() -> AssistantProviderSettings {
    AssistantProviderSettings {
        provider: String::from("openai-compatible"),
        model: String::from("Gemini-3.5-Flash"),
        base_url: String::from("mock://langame-assistant-smoke"),
        api_key: String::from("mock-openai-key"),
    }
}

struct AssistantOperationProbeRequest<'a> {
    prompt: &'a str,
    context: &'a str,
    config_documents: &'a [AssistantOperationConfigDocument],
}

async fn assistant_operation_plan_probe(
    ai_settings: &AssistantProviderSettings,
    module_summaries: &[ModuleSummary],
    instance_summaries: &[InstanceSummary],
    selected_instance: Option<&InstanceDetails>,
    selected_module: &ModuleDetails,
    request: AssistantOperationProbeRequest<'_>,
) -> Result<AssistantOperationPlan, Box<dyn std::error::Error>> {
    let input = AssistantExecuteOperationInput {
        task: Default::default(),
        settings: ai_settings.clone(),
        prompt: String::from(request.prompt),
        context: Some(String::from(request.context)),
        selected_instance_id: selected_instance.map(|instance| instance.summary.id.clone()),
        selected_module_id: Some(selected_module.summary.id.clone()),
    };
    let planner_prompt = build_assistant_operation_planner_prompt(
        &input,
        instance_summaries,
        module_summaries,
        selected_instance,
        Some(selected_module),
        request.config_documents,
        None,
    )?;
    let output = run_assistant_with_system_prompt(
        &AssistantRunInput {
            settings: ai_settings.clone(),
            prompt_label: String::from("AI Module Planner Probe"),
            prompt: planner_prompt,
            context: String::new(),
        },
        ASSISTANT_OPERATION_SYSTEM_PROMPT,
    )
    .await
    .map_err(std::io::Error::other)?;
    let plan =
        parse_assistant_operation_plan_response(&output.content).map_err(std::io::Error::other)?;
    Ok(plan)
}

fn assistant_smoke_gm_command_for_module(module_id: &str) -> Option<&'static str> {
    match module_id {
        "arksurvivalascended" => Some("DestroyWildDinos"),
        "minecraft" => Some("list"),
        "projectzomboid" => Some("players"),
        "vrising" => Some("ListUsers"),
        "dontstarve" => Some(
            "for _,v in ipairs(AllPlayers) do for i=1,20 do v.components.inventory:GiveItem(SpawnPrefab(\"log\")) end end",
        ),
        "terraria" => Some("playing"),
        "palworld" => Some("ShowPlayers"),
        "sevendaystodie" => Some("listplayerids"),
        "rust" => Some("status"),
        _ => None,
    }
}

fn isolated_smoke_app_settings(run_root: &Path) -> Result<AppSettings, Box<dyn std::error::Error>> {
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
    Ok(settings)
}

#[tokio::test(flavor = "current_thread")]
async fn command_fixture_storage_stays_isolated_and_initializes_desktop_state()
-> Result<(), Box<dyn std::error::Error>> {
    let _lock = command_smoke_lock().lock().await;
    let root = temp_test_dir("state-storage");
    let environment = ProgramDataEnvGuard::set(&root.join("programdata"));
    let storage = bootstrap_storage()?;
    for path in [
        &storage.paths.app_data_root,
        &storage.paths.database_path,
        &storage.paths.logs_root,
        &storage.paths.games_root,
        &storage.paths.instances_root,
        &storage.paths.archives_root,
        &storage.paths.steamcmd_root,
    ] {
        assert!(
            path.starts_with(&root),
            "fixture path escaped: {}",
            path.display()
        );
    }
    assert!(
        !storage
            .paths
            .app_data_root
            .join("storage-location.json")
            .exists()
    );
    assert!(
        !storage
            .paths
            .app_data_root
            .join("storage-location.lock")
            .exists()
    );
    let saved = fs::read(&storage.paths.settings_path)?;
    let nested = ProgramDataEnvGuard::set(&root.join("programdata"));
    assert_eq!(fs::read(&storage.paths.settings_path)?, saved);
    drop(nested);

    let empty = DesktopState::default();
    assert!(!storage_context_snapshot_is_current(
        &empty,
        &storage.settings
    )?);
    let state = DesktopState::from_storage(&storage);
    ensure_storage_context_snapshot_current(&state, &storage, "fixture initialization")?;
    let mut stale = storage.clone();
    stale.settings.servers_root = root
        .join("different-instances")
        .to_string_lossy()
        .into_owned();
    assert!(
        ensure_storage_context_snapshot_current(&state, &stale, "fixture stale snapshot").is_err()
    );
    drop(environment);
    fs::remove_dir_all(root)?;
    Ok(())
}

pub(super) async fn assistant_assessment_fixture() -> (
    tokio::sync::MutexGuard<'static, ()>,
    ProgramDataEnvGuard,
    tauri::App<tauri::test::MockRuntime>,
    StorageBootstrap,
) {
    let lock = command_smoke_lock().lock().await;
    let root = temp_test_dir("assessment");
    let environment = ProgramDataEnvGuard::set(&root.join("programdata"));
    isolated_smoke_app_settings(&root).expect("isolated assessment settings");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(
            &bootstrap_storage().expect("bootstrap isolated fixture storage"),
        ))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("assessment mock app");
    // The app initializes only its isolated settings paths. Assessment receives
    // a separate nonexistent store so unexpected database access is observable.
    let root = root.join("uninitialized-assessment");
    let paths = app_storage::StoragePaths {
        app_data_root: root.clone(),
        settings_path: root.join("settings.json"),
        database_path: root.join("db/lgs.db"),
        logs_root: root.join("logs"),
        modules_root: root.join("modules"),
        migrations_root: root.join("migrations"),
        steamcmd_root: root.join("steamcmd"),
        games_root: root.join("games"),
        instances_root: root.join("instances"),
        archives_root: root.join("instances").join(".trash"),
    };
    let storage = StorageBootstrap {
        settings: paths.settings(),
        storage_status: paths.probe_status(),
        paths,
    };
    (lock, environment, app, storage)
}

/// Called only after a test has built its synthetic official package. Never
/// infer trust from an arbitrary existing directory in a creation helper.
pub(in crate::commands) fn record_fake_program_baseline(
    settings: &AppSettings,
    module_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let descriptors = discover_modules(Path::new(&settings.modules_root))?;
    let descriptor = find_descriptor(&descriptors, module_id)?;
    let install = descriptor
        .install
        .as_ref()
        .ok_or("fixture install contract is missing")?;
    let root = Path::new(&settings.games_root).join(&install.shared_game_dir);
    app_storage::record_library_program_baseline(&root, descriptor, true, None)?;
    Ok(())
}

/// Builds an inert package in a new fixture directory before registering it.
pub(in crate::commands) async fn prepare_fake_registered_program(
    paths: &app_storage::StoragePaths,
    descriptor: &ModuleDescriptor,
) -> Result<(), Box<dyn std::error::Error>> {
    let install = descriptor
        .install
        .as_ref()
        .ok_or("fixture install contract is missing")?;
    let root = paths.games_root.join(&install.shared_game_dir);
    fs::create_dir_all(&paths.games_root)?;
    fs::create_dir(&root)?;
    let executable = descriptor
        .process
        .as_ref()
        .map(|process| process.executable.as_str())
        .filter(|path| !path.contains("{{"));
    for relative in install
        .verification_path
        .as_deref()
        .into_iter()
        .chain(executable)
    {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().ok_or("fixture program parent is missing")?)?;
        fs::write(path, b"synthetic program fixture, never executed")?;
    }
    record_fake_program_baseline(&paths.settings(), &descriptor.summary.id)?;
    sync_game_installs(
        paths,
        &[GameInstallSyncRecord {
            module_id: descriptor.summary.id.clone(),
            install_root: root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: None,
            mark_verified: true,
        }],
    )
    .await?;
    Ok(())
}

fn prepare_fake_minecraft_install(
    settings: &AppSettings,
) -> Result<(), Box<dyn std::error::Error>> {
    let minecraft_install_root = PathBuf::from(&settings.games_root).join("minecraft");
    fs::create_dir_all(&minecraft_install_root)?;
    fs::write(
        minecraft_install_root.join("server.jar"),
        "fake minecraft server jar",
    )?;
    record_fake_program_baseline(settings, "minecraft")
}

fn prepare_fake_dontstarve_install(
    settings: &AppSettings,
) -> Result<(), Box<dyn std::error::Error>> {
    let dontstarve_bin = PathBuf::from(&settings.games_root)
        .join("dontstarve")
        .join("bin64");
    fs::create_dir_all(&dontstarve_bin)?;
    fs::write(
        dontstarve_bin.join("dontstarve_dedicated_server_nullrenderer_x64.exe"),
        "fake Don't Starve Together executable",
    )?;
    record_fake_program_baseline(settings, "dontstarve")
}

fn prepare_fake_ark_survival_ascended_install(
    settings: &AppSettings,
) -> Result<(), Box<dyn std::error::Error>> {
    let asa_install_root = PathBuf::from(&settings.games_root)
        .join("arksurvivalascended")
        .join("ShooterGame")
        .join("Binaries")
        .join("Win64");
    fs::create_dir_all(&asa_install_root)?;
    fs::write(
        asa_install_root.join("ArkAscendedServer.exe"),
        "fake ARK Survival Ascended executable",
    )?;
    record_fake_program_baseline(settings, "arksurvivalascended")
}

fn prepare_fake_palworld_install(settings: &AppSettings) -> Result<(), Box<dyn std::error::Error>> {
    let palworld_bin = PathBuf::from(&settings.games_root)
        .join("palworld")
        .join("Pal")
        .join("Binaries")
        .join("Win64");
    fs::create_dir_all(&palworld_bin)?;
    fs::write(
        palworld_bin.join("PalServer-Win64-Shipping-Cmd.exe"),
        "fake palworld server executable",
    )?;
    record_fake_program_baseline(settings, "palworld")
}

fn prepare_fake_sevendaystodie_install(
    settings: &AppSettings,
) -> Result<(), Box<dyn std::error::Error>> {
    let sevendaystodie_root = PathBuf::from(&settings.games_root).join("sevendaystodie");
    fs::create_dir_all(&sevendaystodie_root)?;
    fs::write(
        sevendaystodie_root.join("7DaysToDieServer.exe"),
        "fake 7 Days to Die server executable",
    )?;
    record_fake_program_baseline(settings, "sevendaystodie")
}

fn prepare_fake_rust_install(settings: &AppSettings) -> Result<(), Box<dyn std::error::Error>> {
    let rust_root = PathBuf::from(&settings.games_root).join("rust");
    fs::create_dir_all(&rust_root)?;
    fs::write(
        rust_root.join("RustDedicated.exe"),
        "fake Rust dedicated server executable",
    )?;
    record_fake_program_baseline(settings, "rust")
}

fn prepare_fake_vrising_install(settings: &AppSettings) -> Result<(), Box<dyn std::error::Error>> {
    let vrising_root = PathBuf::from(&settings.games_root).join("vrising");
    fs::create_dir_all(&vrising_root)?;
    fs::write(
        vrising_root.join("VRisingServer.exe"),
        "fake V Rising dedicated server executable",
    )?;
    record_fake_program_baseline(settings, "vrising")
}

fn prepare_fake_terraria_install(settings: &AppSettings) -> Result<(), Box<dyn std::error::Error>> {
    let descriptors = discover_modules(Path::new(&settings.modules_root))?;
    let descriptor = find_descriptor(&descriptors, "terraria")?;
    let install = descriptor
        .install
        .as_ref()
        .ok_or("Terraria fixture install contract is missing")?;
    let executable = Path::new(&settings.games_root)
        .join(&install.shared_game_dir)
        .join(
            install
                .verification_path
                .as_deref()
                .ok_or("Terraria fixture verification path is missing")?,
        );
    fs::create_dir_all(
        executable
            .parent()
            .ok_or("Terraria fixture program parent is missing")?,
    )?;
    fs::write(executable, "fake Terraria dedicated server executable")?;
    record_fake_program_baseline(settings, "terraria")
}

fn prepare_fake_project_zomboid_install(
    settings: &AppSettings,
) -> Result<(), Box<dyn std::error::Error>> {
    let project_zomboid_java = PathBuf::from(&settings.games_root)
        .join("projectzomboid")
        .join("jre64")
        .join("bin");
    fs::create_dir_all(&project_zomboid_java)?;
    fs::write(
        project_zomboid_java.join("java.exe"),
        "fake project zomboid java runtime",
    )?;
    record_fake_program_baseline(settings, "projectzomboid")
}

fn prepare_fake_enshrouded_install(
    settings: &AppSettings,
) -> Result<(), Box<dyn std::error::Error>> {
    let enshrouded_root = PathBuf::from(&settings.games_root).join("enshrouded");
    fs::create_dir_all(&enshrouded_root)?;
    let executable = enshrouded_root.join("enshrouded_server.exe");
    let system_root = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let source = [
        system_root.join(r"System32\WindowsPowerShell\v1.0\powershell.exe"),
        system_root.join(r"System32\cmd.exe"),
        PathBuf::from(r"C:\Windows\System32\cmd.exe"),
    ]
    .into_iter()
    .find(|candidate| candidate.exists())
    .ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "No host executable found for fake Enshrouded server",
        )
    })?;
    fs::copy(&source, executable)?;
    record_fake_program_baseline(settings, "enshrouded")
}

async fn create_fake_module_instance(
    state: tauri::State<'_, DesktopState>,
    module_id: &str,
    name: &str,
) -> Result<InstanceProvisioning, Box<dyn std::error::Error>> {
    let mut provisioning = command_result(
        create_instance_record_inner(
            state,
            CreateInstanceInput {
                name: name.to_string(),
                module_id: module_id.to_string(),
            },
        )
        .await,
    )?;
    let storage = bootstrap_storage()?;
    let details = read_instance_details(&storage.paths, &provisioning.summary.id).await?;
    let updated = update_instance(
        &storage.paths,
        UpdateInstanceInput {
            id: details.summary.id,
            bind_ip: String::from("127.0.0.1"),
            auto_backup_on_stop: details.auto_backup_on_stop,
            backup_retention_count: details.backup_retention_count,
            settings_json: details.settings_json,
            ports: details.ports,
        },
    )
    .await?;
    provisioning.summary = updated.summary;
    Ok(provisioning)
}

async fn create_fake_minecraft_instance(
    state: tauri::State<'_, DesktopState>,
    name: &str,
) -> Result<InstanceProvisioning, Box<dyn std::error::Error>> {
    create_fake_module_instance(state, "minecraft", name).await
}

async fn register_smoke_instance_running(
    state: tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    details: &InstanceDetails,
    run_root: &Path,
    label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let log_path = run_root.join("logs").join(format!(
        "{}-{}.log",
        details.summary.id,
        normalize_assistant_match_text(label)
    ));
    fs::create_dir_all(log_path.parent().unwrap_or(run_root))?;
    fs::write(&log_path, "smoke runtime log\n")?;
    let log_path_string = log_path.to_string_lossy().into_owned();
    let session_id = format!("{}-{label}", details.summary.id);
    let process_identity = inspect_process_identity(std::process::id())?
        .ok_or("test process identity is unavailable")?;
    let process_state = mark_instance_process_started_with_identity(
        &storage.paths,
        &StartedInstanceProcess {
            instance_id: &details.summary.id,
            session_id: Some(&session_id),
            process_key: "main",
            display_name: "Server",
            pid: std::process::id(),
            log_path: &log_path_string,
            is_primary: true,
        },
        None,
    )
    .await?;

    let refreshed_details = read_instance_details(&storage.paths, &details.summary.id).await?;
    let mut runtime = state.runtime_supervisor.lock().unwrap();
    runtime.insert_running(
        refreshed_details.summary,
        Some(session_id),
        vec![ManagedProcess {
            run_id: process_state.run_id,
            process_key: String::from("main"),
            display_name: String::from("Server"),
            pid: std::process::id(),
            process_identity: process_identity.clone(),
            root_process_identity: process_identity,
            log_path: log_path_string,
            is_primary: true,
            uses_script_entrypoint: false,
            performance_policy: RuntimePerformancePolicy::default(),
            last_performance_refresh: None,
            last_performance_target_count: None,
            last_performance_application: None,
            child: None,
            hidden_desktop: None,
        }],
    );
    Ok(())
}

pub(super) async fn register_smoke_stdin_capture_process(
    state: tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    details: &InstanceDetails,
    run_root: &Path,
    process_key: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let process_root = run_root.join("stdin-capture");
    fs::create_dir_all(&process_root)?;
    let script_path = process_root.join(format!("{process_key}.ps1"));
    let capture_path = process_root.join(format!("{process_key}-command.txt"));
    let pending_capture_path = process_root.join(format!("{process_key}-command.pending"));
    let escaped_capture_path = capture_path.to_string_lossy().replace('\'', "''");
    let escaped_pending_path = pending_capture_path.to_string_lossy().replace('\'', "''");
    fs::write(
        &script_path,
        format!(
            "$line = [Console]::In.ReadLine()\nSet-Content -LiteralPath '{escaped_pending_path}' -Value $line -ErrorAction Stop\n[System.IO.File]::Move('{escaped_pending_path}', '{escaped_capture_path}')\nStart-Sleep -Seconds 20\n"
        ),
    )?;

    let powershell = PathBuf::from("C:/Windows/System32/WindowsPowerShell/v1.0/powershell.exe");
    let log_path = process_root.join(format!("{process_key}.log"));
    let launch_plan = LaunchPlan {
        environment: Default::default(),
        instance_id: details.summary.id.clone(),
        instance_name: details.summary.name.clone(),
        module_id: details.summary.module_id.clone(),
        install_root: process_root.to_string_lossy().into_owned(),
        install_state: InstallState::Installed,
        uses_private_runtime: false,
        working_directory: process_root.to_string_lossy().into_owned(),
        executable_path: powershell.to_string_lossy().into_owned(),
        executable_exists: powershell.exists(),
        ready_to_launch: true,
        validation_issues: Vec::new(),
        args: vec![
            String::from("-NoProfile"),
            String::from("-ExecutionPolicy"),
            String::from("Bypass"),
            String::from("-File"),
            script_path.to_string_lossy().into_owned(),
        ],
        command_line: String::from("powershell stdin capture"),
        window_policy: ProcessWindowPolicy::Background,
        uses_script_entrypoint: false,
        requires_admin: false,
        host_surface: app_core::ProcessHostSurface::ManagedTerminal,
        host_notes: None,
        performance_policy: RuntimePerformancePolicy::default(),
        performance_preview: app_core::RuntimePerformancePolicyPreview::default(),
    };
    let mut spawned = spawn_launch_plan(&launch_plan, &log_path)?;
    let session_id = format!("{}-{process_key}-stdin-capture", details.summary.id);
    let process_state = mark_instance_process_started_with_identity(
        &storage.paths,
        &StartedInstanceProcess {
            instance_id: &details.summary.id,
            session_id: Some(&session_id),
            process_key,
            display_name: "Master shard",
            pid: spawned.pid,
            log_path: &spawned.log_path,
            is_primary: true,
        },
        None,
    )
    .await?;

    let refreshed_details = read_instance_details(&storage.paths, &details.summary.id).await?;
    let mut runtime = state.runtime_supervisor.lock().unwrap();
    runtime.insert_running(
        refreshed_details.summary,
        Some(session_id),
        vec![ManagedProcess {
            run_id: process_state.run_id,
            process_key: process_key.to_string(),
            display_name: String::from("Master shard"),
            pid: spawned.pid,
            process_identity: spawned.process_identity.clone(),
            root_process_identity: spawned.root_process_identity.clone(),
            log_path: spawned.log_path,
            is_primary: true,
            uses_script_entrypoint: spawned.uses_script_entrypoint,
            performance_policy: RuntimePerformancePolicy::default(),
            last_performance_refresh: None,
            last_performance_target_count: None,
            last_performance_application: None,
            child: spawned.child.take(),
            hidden_desktop: spawned.hidden_desktop.take(),
        }],
    );

    Ok(capture_path)
}
#[path = "commands_access_smoke_tests.rs"]
mod access_smoke_tests;
#[path = "commands_ai_smoke_tests.rs"]
mod ai_smoke_tests;
#[path = "commands_assistant_operation_tests.rs"]
mod assistant_operation_tests;
#[path = "commands_assistant_plan_tests.rs"]
mod assistant_plan_tests;
#[path = "commands_assistant_prompt_tests.rs"]
mod assistant_prompt_tests;
#[path = "commands_creation_lifecycle_tests.rs"]
pub(super) mod creation_lifecycle_tests;
#[path = "commands_dst_native_smoke_tests.rs"]
mod dst_native_smoke_tests;
#[path = "commands_instance_deletion_tests.rs"]
mod instance_deletion_tests;
#[path = "commands_manual_runtime_tests.rs"]
mod manual_runtime_tests;
#[path = "commands_mod_reference_tests.rs"]
mod mod_reference_tests;
#[path = "commands_player_action_dispatch_tests.rs"]
mod player_action_dispatch_tests;
#[path = "commands_romestead_runtime_tests.rs"]
mod romestead_runtime_tests;
#[path = "commands_stdin_dispatch_tests.rs"]
mod stdin_dispatch_tests;
#[path = "commands_transport_tests.rs"]
mod transport_tests;

use assistant_prompt_tests::*;
pub(in crate::commands) use manual_runtime_tests::ProgramDataEnvGuard;
use manual_runtime_tests::*;
use transport_tests::*;

#[path = "commands_assistant_backup_workflow_tests.rs"]
mod assistant_backup_workflow_tests;
#[path = "commands_assistant_dst_workflow_tests.rs"]
mod assistant_dst_workflow_tests;
#[path = "commands_assistant_file_patch_tests.rs"]
mod assistant_file_patch_tests;
#[path = "commands_assistant_generic_evaluation_tests.rs"]
mod assistant_generic_evaluation_tests;
#[path = "commands_assistant_lifecycle_workflow_tests.rs"]
#[cfg(windows)]
pub(in crate::commands) mod assistant_lifecycle_workflow_tests;
#[path = "commands_assistant_repair_integration_tests.rs"]
mod assistant_repair_integration_tests;
#[path = "commands_instance_reconciliation_tests.rs"]
mod instance_reconciliation_tests;
#[path = "commands_instance_retirement_tests.rs"]
mod instance_retirement_tests;
pub(super) use instance_retirement_tests::retirement_fixture;
#[path = "commands_module_details_tests.rs"]
mod module_details_tests;
#[path = "commands_program_storage_tests.rs"]
mod program_storage_tests;
