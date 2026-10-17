#![cfg(test)]
use super::*;
use app_core::{NativeBanEntry, RuntimePlayerIdentityKind};

#[test]
fn saved_ban_authorization_requires_one_exact_current_account() {
    let mut roster = NativeBanRoster {
        instance_id: "fixture".into(),
        module_id: "palworld".into(),
        observed_at_unix_ms: 1,
        entries: vec![NativeBanEntry {
            account_id: "steam_76561198000000001".into(),
            display_name: Some("Same name".into()),
            identity_kind: RuntimePlayerIdentityKind::PalworldUserId,
        }],
    };
    assert!(authorize_account(&roster, "steam_76561198000000001").is_ok());
    for target in ["76561198000000001", "Same name", "steam_76561198000000002"] {
        assert!(authorize_account(&roster, target).is_err());
    }
    roster.entries.push(roster.entries[0].clone());
    assert!(authorize_account(&roster, "steam_76561198000000001").is_err());
    roster.entries = vec![NativeBanEntry {
        account_id: "00000000000000000000000000000001".into(),
        display_name: None,
        identity_kind: RuntimePlayerIdentityKind::PalworldPlayerUid,
    }];
    assert!(authorize_account(&roster, "00000000000000000000000000000001").is_err());
}

#[test]
fn asa_native_unban_refuses_a_known_display_name_fallback_collision() {
    let mut roster = NativeBanRoster {
        instance_id: "fixture".into(),
        module_id: "arksurvivalascended".into(),
        observed_at_unix_ms: 1,
        entries: vec![
            NativeBanEntry {
                account_id: "10000000000000001".into(),
                display_name: Some("Target".into()),
                identity_kind: RuntimePlayerIdentityKind::SteamId,
            },
            NativeBanEntry {
                account_id: "10000000000000002".into(),
                display_name: Some("10000000000000001".into()),
                identity_kind: RuntimePlayerIdentityKind::SteamId,
            },
        ],
    };
    assert!(
        authorize_account(&roster, "10000000000000001")
            .unwrap_err()
            .contains("ambiguous")
    );
    roster.module_id = "arksurvivalevolved".into();
    assert!(authorize_account(&roster, "10000000000000001").is_ok());
}

#[test]
fn native_ban_commands_are_registered_with_authenticated_runtime_ownership() {
    use crate::command_registry::{Completion, Execution, LanAccess};
    for (name, completion) in [
        ("read_instance_native_bans", Completion::Bounded),
        ("unban_instance_native_ban", Completion::Owned),
    ] {
        let command = crate::command_registry::find(name).unwrap();
        assert_eq!(command.execution, Execution::Runtime);
        assert_eq!(command.lan, LanAccess::Allowed);
        assert_eq!(command.completion, completion);
    }
    assert_eq!(native_unban_action("rimworld").unwrap(), "unban_user");
    assert!(native_unban_action("projectzomboid").is_err());
}

#[tokio::test]
async fn stopped_squad_roster_reads_actual_runtime_and_removes_persisted_bans()
-> Result<(), Box<dyn std::error::Error>> {
    let root = crate::commands::tests::temp_test_dir("native-ban-command");
    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    let _fixture = Fixture(root.clone());
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let storage = app_storage::bootstrap_storage_with_paths(app_storage::StoragePaths {
        app_data_root: root.join("app-data"),
        settings_path: root.join("app-data/settings.json"),
        database_path: root.join("app-data/db/lgs.db"),
        logs_root: root.join("app-data/logs"),
        modules_root: workspace.join("modules"),
        migrations_root: workspace.join("migrations"),
        steamcmd_root: root.join("steamcmd"),
        games_root: root.join("games"),
        instances_root: root.join("instances"),
        archives_root: root.join("instances/.trash"),
    })?;
    app_storage::save_app_settings_with_paths(storage.settings.clone(), &storage.paths)?;
    initialize_database(&storage.paths).await?;
    let descriptor = discover_modules(&storage.paths.modules_root)?
        .into_iter()
        .find(|module| module.summary.id == "squad")
        .ok_or("Missing Squad module")?;
    sync_modules(&storage.paths, std::slice::from_ref(&descriptor)).await?;
    crate::commands::tests::prepare_fake_registered_program(&storage.paths, &descriptor).await?;
    let created = app_storage::create_instance(
        &storage.paths,
        &descriptor,
        CreateInstanceInput {
            name: "Native ban fixture".into(),
            module_id: "squad".into(),
        },
    )
    .await?;
    let details = read_instance_details(&storage.paths, &created.summary.id).await?;
    let runtime_root = app_storage::resolve_instance_runtime_root(
        super::super::commands_program_storage::instance_root(&details)?,
    )?;
    let path = runtime_root.join("SquadGame/ServerConfig/Bans.cfg");
    fs::create_dir_all(path.parent().unwrap())?;
    let original = b"// keep\r\n76561198000000001:0 // Fixture\r\n";
    fs::write(&path, original)?;
    let config_before = fs::read(&details.config_file_path)?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let roster =
        read_instance_native_bans(app.state::<DesktopState>(), details.summary.id.clone()).await?;
    assert_eq!(roster.entries.len(), 1);
    assert_eq!(roster.entries[0].account_id, "76561198000000001");
    let result = unban_instance_native_ban(
        app.handle().clone(),
        app.state::<DesktopState>(),
        details.summary.id.clone(),
        roster.entries[0].account_id.clone(),
    )
    .await?;
    assert_eq!(result.status, RuntimeLivePlayerActionStatus::Sent);
    assert!(result.summary.contains("next starts"), "{}", result.summary);
    assert_eq!(fs::read(&path)?, b"// keep\r\n");
    let refreshed =
        read_instance_native_bans(app.state::<DesktopState>(), details.summary.id.clone()).await?;
    assert!(refreshed.entries.is_empty());
    let error = unban_instance_native_ban(
        app.handle().clone(),
        app.state::<DesktopState>(),
        details.summary.id,
        roster.entries[0].account_id.clone(),
    )
    .await
    .unwrap_err();
    assert!(error.contains("absent"), "{error}");
    let backups = fs::read_dir(
        root.join("instances")
            .join(&created.summary.id)
            .join("config/native-ban-recovery"),
    )?
    .map(|entry| entry.unwrap().path())
    .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    assert_eq!(fs::read(&backups[0])?, original);
    assert_eq!(fs::read(&details.config_file_path)?, config_before);
    Ok(())
}

#[tokio::test]
async fn squad_saved_unban_requires_restart_and_preserves_unrelated_records() {
    let root = crate::commands::tests::temp_test_dir("squad-ban-reload");
    let path = root.join("SquadGame/ServerConfig/Bans.cfg");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original = b"76561198000000001:0 // fixture\r\n76561198000000002:0 // keep\r\n";
    fs::write(&path, original).unwrap();
    let state = DesktopState::default();
    let operation = state
        .begin_storage_context_operation("Squad reload fixture")
        .unwrap();
    let result = unban_squad(
        &operation,
        root.clone(),
        root.clone(),
        "76561198000000001".into(),
        true,
    )
    .await
    .unwrap();
    assert!(result.contains("Restart the server"), "{result}");
    assert!(result.contains("has not been unbanned"), "{result}");
    assert_eq!(fs::read(&path).unwrap(), b"76561198000000002:0 // keep\r\n");
    let backups = fs::read_dir(root.join("config/native-ban-recovery"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    assert_eq!(fs::read(&backups[0]).unwrap(), original);
    let descriptor =
        discover_modules(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../modules"))
            .unwrap()
            .into_iter()
            .find(|module| module.summary.id == "squad")
            .unwrap();
    assert!(
        !descriptor
            .runtime
            .player_actions
            .iter()
            .any(|action| action.id == "reload_server_config")
    );
    assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
#[ignore = "Explicit disposable native Squad fixture; requires LANGAME_SQUAD_UNBAN_FIXTURE_ROOT."]
async fn squad_native_virtual_fixture_file_edit_acceptance() {
    let root = PathBuf::from(
        std::env::var_os("LANGAME_SQUAD_UNBAN_FIXTURE_ROOT")
            .expect("Provide the disposable native Squad probe root"),
    );
    assert!(root.is_absolute());
    let receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("copy.json")).unwrap()).unwrap();
    assert_eq!(receipt["module"], "squad");
    assert_eq!(receipt["source_saved_copied"], false);
    let runtime = root.join("runtime");
    let path = runtime.join("SquadGame/ServerConfig/Bans.cfg");
    // Do not edit an arbitrary native list even if the environment is wrong.
    let original = b"// LGSM disposable native ban probe\r\n10000000000000001:0 // target\r\n10000000000000002:0 // keep\r\n";
    assert_eq!(fs::read(&path).unwrap(), original);
    assert!(!root.join("config/native-ban-recovery").exists());
    let state = DesktopState::default();
    let operation = state
        .begin_storage_context_operation("Squad virtual native acceptance")
        .unwrap();
    let result = unban_squad(
        &operation,
        runtime.clone(),
        root.clone(),
        "10000000000000001".into(),
        true,
    )
    .await
    .unwrap();
    assert!(result.contains("Restart the server"));
    assert_eq!(
        fs::read(&path).unwrap(),
        b"// LGSM disposable native ban probe\r\n10000000000000002:0 // keep\r\n"
    );
    let roster =
        app_storage::read_native_ban_roster(&runtime, &root, "squad", "native-probe").unwrap();
    assert_eq!(roster.entries.len(), 1);
    assert_eq!(roster.entries[0].account_id, "10000000000000002");
    let copies = fs::read_dir(root.join("config/native-ban-recovery"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(copies.len(), 1);
    assert_eq!(fs::read(&copies[0]).unwrap(), original);
    println!(
        "native_squad_virtual_fixture exact_remove=passed unrelated_preserved=passed recovery_copy=passed restart_required=true"
    );
}
