use super::*;

async fn installed_fixture() -> Fixture {
    let fixture = Fixture::new().await;
    let root = fixture.install_root();
    fs::remove_file(root.join(RETAINED_INSTALL_DATA_MARKER)).unwrap();
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::write(root.join("bin/Server.exe"), b"old server payload").unwrap();
    fs::create_dir_all(root.join("mods/workshop-123")).unwrap();
    fs::write(
        root.join("mods/dedicated_server_mods_setup.lua"),
        b"operator mods",
    )
    .unwrap();
    fs::write(
        root.join("mods/workshop-123/modmain.lua"),
        b"operator workshop mod",
    )
    .unwrap();
    fixture
}

fn journal(fixture: &Fixture) -> PathBuf {
    preservation::journal_directory(&fixture.module.summary.id, &fixture.install_root()).unwrap()
}

fn assert_pending(fixture: &Fixture) {
    assert!(matches!(
        read_program_install_revision(
            Path::new(&fixture.settings.servers_root),
            &fixture.module.summary.id,
            &fixture.install_root(),
        ),
        Err(SteamCmdError::PackageRevisionPending { .. })
    ));
}

fn assert_mods(fixture: &Fixture) {
    assert_eq!(
        fs::read(
            fixture
                .install_root()
                .join("mods/dedicated_server_mods_setup.lua")
        )
        .unwrap(),
        b"operator mods"
    );
    assert_eq!(
        fs::read(fixture.install_root().join("mods/workshop-123/modmain.lua")).unwrap(),
        b"operator workshop mod"
    );
}

#[tokio::test]
async fn installed_steam_failed_update_restores_data_and_retry_preserves_later_edits() {
    let fixture = installed_fixture().await;
    let fail = Path::new(&fixture.settings.steamcmd_root).join("fail");
    fs::write(&fail, b"fail").unwrap();
    assert!(matches!(
        fixture.install(|_| {}).await,
        Err(SteamCmdError::SteamCmdCommandFailed { .. })
    ));
    fixture.assert_retained(false);
    assert_mods(&fixture);
    assert_pending(&fixture);
    let pending = journal(&fixture);
    assert!(pending.join("restored").is_file());
    assert_eq!(
        fs::read(pending.join("originals/ServerConfig/Server.cfg")).unwrap(),
        b"operator settings"
    );
    assert_eq!(
        fs::read(pending.join("changed/ServerConfig/Server.cfg")).unwrap(),
        b"depot defaults"
    );
    fs::write(
        fixture.install_root().join("ServerConfig/Server.cfg"),
        b"later operator edit",
    )
    .unwrap();
    fs::remove_file(fail).unwrap();
    fixture.install(|_| {}).await.unwrap();
    assert_eq!(
        fs::read(fixture.install_root().join("ServerConfig/Server.cfg")).unwrap(),
        b"later operator edit"
    );
    assert_mods(&fixture);
    assert!(!pending.exists());
    assert!(
        read_program_install_revision(
            Path::new(&fixture.settings.servers_root),
            &fixture.module.summary.id,
            &fixture.install_root()
        )
        .is_ok()
    );
}

#[tokio::test]
async fn installed_steam_cancel_restores_data_before_returning_and_keeps_recovery_copy() {
    let fixture = installed_fixture().await;
    let cancellation = InstallCancellation::new();
    let result = fixture
        .install_with_cancellation(&cancellation, |update| {
            if update.detail == "Verifying installed server files..." {
                cancellation.cancel();
            }
        })
        .await;
    assert!(matches!(
        result,
        Err(SteamCmdError::InstallCancelled { .. })
    ));
    fixture.assert_retained(false);
    assert_mods(&fixture);
    assert_pending(&fixture);
    assert!(journal(&fixture).join("restored").is_file());
}

#[cfg(windows)]
#[tokio::test]
async fn installed_steam_restore_failure_blocks_ready_and_next_retry_recovers_first() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = installed_fixture().await;
    let target = fixture.install_root().join("ServerConfig/Server.cfg");
    let mut lock = None;
    let mut ready = false;
    let result = fixture
        .install(|update| {
            if update.detail == "Verifying installed server files..." {
                lock = Some(
                    OpenOptions::new()
                        .read(true)
                        .share_mode(1)
                        .open(&target)
                        .unwrap(),
                );
            }
            ready |= update
                .install_progress
                .is_some_and(|progress| progress.phase == InstallPhase::Ready);
        })
        .await;
    assert!(matches!(
        result,
        Err(SteamCmdError::InstallRollbackFailed { .. })
    ));
    assert!(!ready);
    assert_pending(&fixture);
    assert_eq!(fs::read(&target).unwrap(), b"depot defaults");
    assert_eq!(
        fs::read(journal(&fixture).join("originals/ServerConfig/Server.cfg")).unwrap(),
        b"operator settings"
    );
    assert!(journal(&fixture).join("native-stopped").is_file());
    assert!(!journal(&fixture).join("restored").exists());
    drop(lock);
    fixture.install(|_| {}).await.unwrap();
    fixture.assert_retained(false);
    assert_mods(&fixture);
}

#[tokio::test]
async fn installed_steam_unknown_native_completion_does_not_recover_over_a_possible_writer() {
    let fixture = installed_fixture().await;
    let target = fixture.install_root();
    let guard = acquire_game_install_lifecycle(
        crate::test_install_lock_context(),
        &fixture.module.summary.id,
        std::slice::from_ref(&target),
    )
    .await
    .unwrap();
    let revision_key =
        package_revision::program_revision_key(&fixture.module.summary.id, &target).unwrap();
    begin_game_install_revision(Path::new(&fixture.settings.servers_root), &revision_key).unwrap();
    let data = preservation::PreservedSteamData::prepare(
        journal(&fixture),
        &target,
        &fixture.module.summary.id,
        &[target.join("ServerConfig")],
        &guard,
        test_deadline(),
    )
    .await
    .unwrap()
    .unwrap();
    data.native_started().unwrap();
    fs::write(
        target.join("ServerConfig/Server.cfg"),
        b"unconfirmed native output",
    )
    .unwrap();
    drop(data);
    drop(guard);
    let error = fixture.install(|_| {}).await.unwrap_err();
    assert!(error.to_string().contains("termination was not confirmed"));
    assert_eq!(
        fs::read(target.join("ServerConfig/Server.cfg")).unwrap(),
        b"unconfirmed native output"
    );
    assert_eq!(
        fs::read(journal(&fixture).join("originals/ServerConfig/Server.cfg")).unwrap(),
        b"operator settings"
    );
    assert!(
        !Path::new(&fixture.settings.steamcmd_root)
            .join("destination.txt")
            .exists()
    );
    assert_pending(&fixture);
}

#[tokio::test]
async fn installed_steam_rejects_linked_retained_files_before_native_writes() {
    let fixture = installed_fixture().await;
    fs::hard_link(
        fixture.install_root().join("ServerConfig/Server.cfg"),
        fixture.root.join("linked.cfg"),
    )
    .unwrap();
    assert!(fixture.install(|_| {}).await.is_err());
    fixture.assert_retained(false);
    assert_mods(&fixture);
    assert_eq!(
        fs::read(fixture.install_root().join("bin/Server.exe")).unwrap(),
        b"old server payload"
    );
    assert!(
        !Path::new(&fixture.settings.steamcmd_root)
            .join("destination.txt")
            .exists()
    );
}

#[tokio::test]
async fn installed_steam_retry_reprotects_data_recovered_after_selection() {
    let fixture = installed_fixture().await;
    let target = fixture.install_root();
    let guard = acquire_game_install_lifecycle(
        crate::test_install_lock_context(),
        &fixture.module.summary.id,
        std::slice::from_ref(&target),
    )
    .await
    .unwrap();
    let key = package_revision::program_revision_key(&fixture.module.summary.id, &target).unwrap();
    begin_game_install_revision(Path::new(&fixture.settings.servers_root), &key).unwrap();
    let data = preservation::PreservedSteamData::prepare(
        journal(&fixture),
        &target,
        &fixture.module.summary.id,
        &[target.join("ServerConfig")],
        &guard,
        test_deadline(),
    )
    .await
    .unwrap()
    .unwrap();
    data.native_started().unwrap();
    fs::remove_file(target.join("ServerConfig/Server.cfg")).unwrap();
    fs::remove_dir(target.join("ServerConfig")).unwrap();
    // A crash after confirmed native termination but before restoration leaves
    // no current file for the caller's existing-data selection to discover.
    fs::write(journal(&fixture).join("native-stopped"), b"failed\n").unwrap();
    drop(data);
    let cancellation = InstallCancellation::new();
    let result = install_or_update_module_at_with_login_and_callbacks(
        SteamModuleInstallRequest {
            settings: &fixture.settings,
            module: &fixture.module,
            install_root: &target,
            guard: &guard,
            validate: false,
            cancellation: &cancellation,
            account_name: None,
            retained_paths: &[],
        },
        ModuleInstallCallbacks {
            on_progress: |_| {},
            prepare_fresh_payload: |_| async { Ok(()) },
        },
    )
    .await
    .unwrap();
    assert_eq!(result.install_state, InstallState::Installed);
    fixture.assert_retained(false);
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "requires an explicitly owned writable directory on a second volume"]
async fn installed_steam_cross_volume_revision_storage_keeps_atomic_retention_local() {
    struct Secondary(PathBuf);
    impl Drop for Secondary {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    let base = PathBuf::from(
        std::env::var_os("LANGAME_TEST_SECOND_VOLUME_ROOT").expect("explicit second-volume root"),
    );
    assert!(base.is_absolute() && base.is_dir());
    let mut fixture = installed_fixture().await;
    assert_ne!(base.components().next(), fixture.root.components().next());
    let secondary = Secondary(base.join(fixture.root.file_name().unwrap()));
    fs::create_dir(&secondary.0).unwrap();
    fixture.settings.servers_root = secondary.0.join("instances").to_string_lossy().into_owned();
    let mut reported = None;
    fixture.install(|update| {
        if let Some(path) = update.detail.strip_prefix("Restored preserved operator data. Differing files written during the update were retained for review at ") {
            reported = Some(PathBuf::from(path.trim_end_matches('.')));
        }
    }).await.unwrap();
    fixture.assert_retained(false);
    assert_mods(&fixture);
    let history = reported.expect("differing files are disclosed in the ordinary progress channel");
    assert!(history.starts_with(fs::canonicalize(&fixture.settings.games_root).unwrap()));
    assert_eq!(
        fs::read(history.join("changed/ServerConfig/Server.cfg")).unwrap(),
        b"depot defaults"
    );
    assert!(!history.join("originals").exists());
    assert!(
        read_program_install_revision(
            Path::new(&fixture.settings.servers_root),
            &fixture.module.summary.id,
            &fixture.install_root()
        )
        .is_ok()
    );
}

#[tokio::test]
async fn installed_steam_recovery_then_retained_reinstall_preserves_both_data_and_publication() {
    let fixture = installed_fixture().await;
    let fail = Path::new(&fixture.settings.steamcmd_root).join("fail");
    fs::write(&fail, b"fail").unwrap();
    assert!(fixture.install(|_| {}).await.is_err());
    fixture.assert_retained(false);
    assert!(journal(&fixture).join("restored").is_file());
    fs::remove_file(fixture.install_root().join("bin/Server.exe")).unwrap();
    mark_retained_install_data(&fixture.install_root()).unwrap();
    fs::remove_file(fail).unwrap();
    fixture.install(|_| {}).await.unwrap();
    fixture.assert_retained(false);
    assert_mods(&fixture);
    assert!(!journal(&fixture).exists());
    assert_eq!(
        fs::read(fixture.install_root().join("bin/Server.exe")).unwrap(),
        b"server payload"
    );
}
