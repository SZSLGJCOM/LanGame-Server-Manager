#![cfg(test)]

use super::*;
use crate::commands::commands_astroneer_saves::read_astroneer_save_catalog;
use crate::commands::commands_instance_network::read_instance_connection_info_from_storage;
use crate::commands::commands_player_access::{
    PlayerAccessLiveStatus, apply_instance_player_access_mutation,
};
use crate::commands::tests::{ProgramDataEnvGuard, prepare_fake_registered_program, temp_test_dir};

const CHILD: &str = "commands::commands_assistant_ops::storage_context_tests::assistant_storage_context_process_fixture";
const ROOT_VARIABLE: &str = "LANGAME_ASSISTANT_STORAGE_CONTEXT_TEST_ROOT";

#[tokio::test]
async fn assistant_and_player_operations_keep_their_storage_after_environment_changes() {
    let root = temp_test_dir("assistant-context");
    // Environment changes belong to one child fixture, never the shared harness.
    let stdout_path = root.join("stdout.log");
    let stderr_path = root.join("stderr.log");
    let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            CHILD,
            "--ignored",
            "--test-threads=1",
            "--nocapture",
        ])
        .env(ROOT_VARIABLE, &root)
        .stdout(fs::File::create(&stdout_path).unwrap())
        .stderr(fs::File::create(&stderr_path).unwrap())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let completed = tokio::time::timeout(Duration::from_secs(60), child.wait()).await;
    let success = matches!(&completed, Ok(Ok(status)) if status.success());
    if !success && child.try_wait().unwrap().is_none() {
        child.kill().await.unwrap();
        child.wait().await.unwrap();
    }
    let stdout = fs::read_to_string(&stdout_path).unwrap();
    let stderr = fs::read_to_string(&stderr_path).unwrap();
    assert!(
        success && stdout.contains("test result: ok. 1 passed;"),
        "assistant context fixture failed ({completed:?}); retained {}\n{stdout}\n{stderr}",
        root.display(),
    );
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "child entry: changes only its own environment and disposable storage"]
async fn assistant_storage_context_process_fixture() -> Result<(), Box<dyn std::error::Error>> {
    let started = std::time::Instant::now();
    let phase = |name: &str| {
        eprintln!(
            "assistant context: {name} ({} ms)",
            started.elapsed().as_millis()
        );
    };
    phase("initialize owned storage");
    let root = PathBuf::from(std::env::var_os(ROOT_VARIABLE).ok_or("missing child fixture root")?);
    let _owned_environment = ProgramDataEnvGuard::set(&root.join("owned/programdata"));
    let storage = bootstrap_storage()?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    phase("initialize owned database");
    ensure_storage_ready(app.state::<DesktopState>()).await?;
    phase("create Minecraft instance");
    let minecraft = create_context_instance(&app, &storage, "minecraft").await?;
    phase("create Astroneer instance");
    let astroneer = create_context_instance(&app, &storage, "astroneer").await?;
    let native_save =
        Path::new(&astroneer.saves_path).join("Owned World$2026.10.09-12.00.00.savegame");
    fs::create_dir_all(native_save.parent().unwrap())?;
    fs::write(&native_save, b"original context save")?;

    phase("switch ambient storage");
    let _ambient_environment = ProgramDataEnvGuard::set(&root.join("ambient/programdata"));
    let ambient = bootstrap_storage()?;
    let ambient_settings = fs::read(&ambient.paths.settings_path)?;
    assert_ne!(storage.paths.database_path, ambient.paths.database_path);
    let provider = AssistantProviderSettings {
        provider: "openai-compatible".into(),
        model: "fixture".into(),
        base_url: "mock://langame-assistant-smoke".into(),
        api_key: "synthetic-test-only".into(),
    };
    phase("assistant provider request");
    let reply = assistant_run(
        app.state::<DesktopState>(),
        AssistantRunInput {
            settings: provider.clone(),
            prompt_label: "Storage context fixture".into(),
            prompt: "Explain the supplied server status.".into(),
            context: "The server is stopped.".into(),
        },
    )
    .await?;
    assert!(!reply.content.is_empty());

    // Only the provider is synthetic. Preview, confirmation, optimistic
    // preconditions, configuration rendering and persisted readback are real.
    phase("assistant preview");
    let preview = assistant_preview_operation_inner(
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: AssistantTaskRequest {
                goal: AssistantTaskGoal::ApplyChange,
                preserve_existing_mods: true,
            },
            settings: provider.clone(),
            prompt: "Set max_players to 24 on the selected Minecraft server.\nmock-action:customizeConfig\nmock-settings-patch:{\"max_players\":24}".into(),
            context: None,
            selected_instance_id: Some(minecraft.summary.id.clone()),
            selected_module_id: Some("minecraft".into()),
        },
    )
    .await?;
    assert!(preview.requires_confirmation);
    assert_eq!(
        read_instance_details(&storage.paths, &minecraft.summary.id)
            .await?
            .settings_json,
        minecraft.settings_json,
        "a preview must not write settings"
    );
    let confirmation = AssistantConfirmOperationInput {
        continue_task: false,
        conversation_id: preview.conversation_id,
        settings: provider.clone(),
        confirmation_token: preview
            .confirmation_token
            .ok_or("missing confirmation token")?,
        plan_summary: preview.plan_summary.ok_or("missing plan summary")?,
    };
    phase("assistant confirmation");
    let output =
        assistant_confirm_operation_inner(app.state::<DesktopState>(), confirmation.clone())
            .await?;
    assert!(output.handled);
    assert_eq!(output.applied_settings_keys, ["max_players"]);
    assert!(
        assistant_confirm_operation_inner(app.state::<DesktopState>(), confirmation)
            .await
            .is_err(),
        "confirmation tokens remain single-use"
    );
    let saved = read_instance_details(&storage.paths, &minecraft.summary.id).await?;
    let settings: Value = serde_json::from_str(&saved.settings_json)?;
    assert_eq!(settings["max_players"], 24);

    phase("player access mutation");
    let roster_entry = "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11,Alex";
    let mutation = apply_instance_player_access_mutation(
        app.state::<DesktopState>(),
        app_storage::ApplyInstancePlayerAccessMutationInput {
            instance_id: minecraft.summary.id.clone(),
            field_key: "whitelist_entries".into(),
            operation: app_storage::PlayerAccessMutationOperation::Add,
            value: json!(roster_entry),
            expected_value: None,
        },
    )
    .await?;
    assert!(matches!(
        mutation.live_status,
        PlayerAccessLiveStatus::NotRunning
    ));
    let saved = read_instance_details(&storage.paths, &minecraft.summary.id).await?;
    let settings: Value = serde_json::from_str(&saved.settings_json)?;
    assert_eq!(settings["whitelist_entries"], roster_entry);
    assert_eq!(settings["max_players"], 24);
    let roster: Value = serde_json::from_slice(&fs::read(
        Path::new(&saved.config_file_path)
            .parent()
            .ok_or("missing configuration directory")?
            .join("whitelist.json"),
    )?)?;
    assert_eq!(roster[0]["uuid"], "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11");
    assert_eq!(roster[0]["name"], "Alex");

    phase("stopped server manual action");
    let denied =
        crate::commands::commands_manual_player_actions::execute_instance_manual_player_action(
            app.state::<DesktopState>(),
            app_core::ExecuteInstanceManualPlayerActionInput {
                instance_id: minecraft.summary.id.clone(),
                action_id: "whitelist_add".into(),
                target: "Alex".into(),
                role: None,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(denied, "The server is not running.");

    phase("read connection and save catalogs");
    let connections = read_instance_connection_info_from_storage(
        app.state::<DesktopState>(),
        vec![minecraft.summary.id.clone()],
    )
    .await?;
    assert_eq!(connections.len(), 1);
    assert_eq!(connections[0].instance_id, minecraft.summary.id);
    assert_eq!(connections[0].ports.len(), minecraft.ports.len());
    let catalog =
        read_astroneer_save_catalog(app.state::<DesktopState>(), astroneer.summary.id.clone())
            .await?;
    assert_eq!(catalog.entries.len(), 1);
    assert_eq!(catalog.entries[0].descriptive_name, "Owned World");
    assert_eq!(fs::read(&native_save)?, b"original context save");

    phase("verify retained storage and logs");
    let log = fs::read_to_string(storage.paths.app_log_path())?;
    assert!(log.contains("assistant.run.success"));
    assert!(log.contains("instance.player_access.request"));
    assert!(log.contains("instance.live_players.manual_action"));
    assert_eq!(fs::read(&ambient.paths.settings_path)?, ambient_settings);
    assert!(!ambient.paths.database_path.exists());
    assert!(!ambient.paths.app_log_path().exists());
    phase("fixture assertions complete");
    Ok(())
}

async fn create_context_instance(
    app: &tauri::App<tauri::test::MockRuntime>,
    storage: &StorageBootstrap,
    module_id: &str,
) -> Result<InstanceDetails, Box<dyn std::error::Error>> {
    let descriptors = discover_modules(&storage.paths.modules_root)?;
    let descriptor = find_descriptor(&descriptors, module_id)?;
    sync_modules(&storage.paths, std::slice::from_ref(descriptor)).await?;
    prepare_fake_registered_program(&storage.paths, descriptor).await?;
    let instance = crate::commands::tests::creation_lifecycle_tests::CREATION_INSTALL_FORBIDDEN
        .scope(
            true,
            create_instance_record_inner(
                app.state::<DesktopState>(),
                CreateInstanceInput {
                    name: format!("Owned {module_id}"),
                    module_id: module_id.into(),
                },
            ),
        )
        .await?;
    Ok(read_instance_details(&storage.paths, &instance.summary.id).await?)
}
