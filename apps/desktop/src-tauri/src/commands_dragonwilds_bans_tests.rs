use super::*;
use std::sync::Arc;
use tauri::Manager;

struct Fixture(PathBuf);

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

async fn fixture() -> Result<(Fixture, StorageBootstrap), Box<dyn std::error::Error>> {
    let root = crate::commands::tests::temp_test_dir("dragon-bans");
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
    Ok((Fixture(root), storage))
}

async fn create(
    storage: &StorageBootstrap,
    module_id: &str,
) -> Result<InstanceDetails, Box<dyn std::error::Error>> {
    let descriptor = discover_modules(&storage.paths.modules_root)?
        .into_iter()
        .find(|descriptor| descriptor.summary.id == module_id)
        .ok_or("fixture module is missing")?;
    sync_modules(&storage.paths, std::slice::from_ref(&descriptor)).await?;
    crate::commands::tests::prepare_fake_registered_program(&storage.paths, &descriptor).await?;
    let created = app_storage::create_instance(
        &storage.paths,
        &descriptor,
        CreateInstanceInput {
            name: "Saved ban roster fixture".into(),
            module_id: module_id.into(),
        },
    )
    .await?;
    Ok(read_instance_details(&storage.paths, &created.summary.id).await?)
}

#[tokio::test]
async fn dragonwilds_bans_command_reads_stopped_instance_without_changing_saved_state()
-> Result<(), Box<dyn std::error::Error>> {
    let (_fixture, storage) = fixture().await?;
    let instance = create(&storage, "runescapedragonwilds").await?;
    assert!(instance.active_run.is_none());
    let root = app_storage::resolve_instance_runtime_root(
        super::super::commands_program_storage::instance_root(&instance)?,
    )?;
    let ini = root.join("RSDragonwilds/Saved/Config/WindowsServer/DedicatedServer.ini");
    fs::create_dir_all(ini.parent().unwrap())?;
    let account = "00000000000000000000000000000001";
    let saved = format!(
        "[/Script/Dominion.DedicatedServerSettings]\r\nKnownPlayerList=(UserId={account},UserName=Offline fixture,Privileges=(PrivilegeMask=14),bIsBanned=True)\r\n"
    );
    fs::write(&ini, &saved)?;
    let config_before = fs::read(&instance.config_file_path)?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let roster =
        read_instance_dragonwilds_bans(app.state::<DesktopState>(), instance.summary.id.clone())
            .await?;
    assert_eq!(roster.instance_id, instance.summary.id);
    assert_eq!(roster.entries.len(), 1);
    assert_eq!(roster.entries[0].account_id, account);
    assert_eq!(fs::read(&ini)?, saved.as_bytes());
    assert_eq!(fs::read(&instance.config_file_path)?, config_before);
    assert!(
        read_instance_details(&storage.paths, &instance.summary.id)
            .await?
            .active_run
            .is_none()
    );
    Ok(())
}

#[tokio::test]
async fn dragonwilds_bans_command_rejects_other_modules_before_reading_runtime_files()
-> Result<(), Box<dyn std::error::Error>> {
    let (_fixture, storage) = fixture().await?;
    let instance = create(&storage, "minecraft").await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let error =
        read_instance_dragonwilds_bans(app.state::<DesktopState>(), instance.summary.id.clone())
            .await
            .unwrap_err();
    assert!(error.contains("requires a Dragonwilds instance"), "{error}");
    Ok(())
}

#[tokio::test]
async fn dragonwilds_bans_cancelled_waiter_retains_instance_and_storage_permits() {
    let state = Arc::new(DesktopState::default());
    let operation = state
        .begin_storage_context_operation("ban roster fixture")
        .unwrap();
    let permit = state.acquire_instance_mutation("fixture").await;
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::sync_channel(1);
    let outer = tokio::spawn(async move {
        spawn_roster_read(&operation, permit, move || {
            started.send(()).unwrap();
            wait.recv().unwrap();
        })
        .await
        .unwrap();
    });
    ready.await.unwrap();
    outer.abort();
    assert!(outer.await.unwrap_err().is_cancelled());
    assert!(
        state
            .try_acquire_instance_mutation("fixture")
            .await
            .is_none()
    );
    assert!(state.begin_storage_context_transition().is_err());
    release.send(()).unwrap();
    let permit = state.acquire_instance_mutation("fixture").await;
    drop(permit);
    // The worker drops its instance permit before its storage lease. Yield until
    // that same completed worker releases both, without relying on a sleep.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if state.begin_storage_context_transition().is_ok() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[test]
fn dragonwilds_bans_command_has_authenticated_bounded_runtime_registration() {
    use crate::command_registry::{Completion, Execution, LanAccess};
    let registration = crate::command_registry::find("read_instance_dragonwilds_bans").unwrap();
    assert_eq!(registration.execution, Execution::Runtime);
    assert_eq!(registration.lan, LanAccess::Allowed);
    assert_eq!(registration.completion, Completion::Bounded);
}
