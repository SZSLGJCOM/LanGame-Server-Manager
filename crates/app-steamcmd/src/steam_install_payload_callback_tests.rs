use super::*;

async fn install_with_callback<C, Fut>(
    fixture: &Fixture,
    target: &Path,
    mut callback: C,
    cancellation: &InstallCancellation,
) -> Result<ModuleInstallResult, SteamCmdError>
where
    C: FnMut(PathBuf) -> Fut,
    Fut: std::future::Future<Output = Result<(), SteamCmdError>>,
{
    let guard =
        acquire_game_install_lifecycle(&fixture.module.summary.id, &[target.to_owned()]).await?;
    let last_phase = std::sync::Mutex::new(None);
    install_or_update_module_at_with_callbacks(
        &fixture.settings,
        &fixture.module,
        target,
        &guard,
        false,
        cancellation,
        ModuleInstallCallbacks {
            on_progress: |update: InstallProgressUpdate| {
                *last_phase.lock().unwrap() =
                    update.install_progress.map(|progress| progress.phase);
            },
            prepare_fresh_payload: |stage| {
                assert_eq!(*last_phase.lock().unwrap(), Some(InstallPhase::Verifying));
                callback(stage)
            },
        },
    )
    .await
}

#[tokio::test]
async fn fresh_payload_callback_records_steam_inventory_before_retained_data_is_merged() {
    let fixture = Fixture::new().await;
    let mut calls = 0;
    install_with_callback(
        &fixture,
        &fixture.install_root(),
        |stage| {
            calls += 1;
            assert_ne!(stage, fixture.install_root());
            assert_eq!(
                fs::read(stage.join("ServerConfig/Server.cfg")).unwrap(),
                b"depot defaults"
            );
            assert_eq!(
                fs::read(stage.join("bin/Server.exe")).unwrap(),
                b"server payload"
            );
            assert!(!stage.join("world.sav").exists());
            for name in [
                ".langame-initial-package.json",
                ".langame-clean-package.json",
            ] {
                assert!(!stage.join(name).exists());
                fs::write(stage.join(name), b"fresh inventory").unwrap();
            }
            std::future::ready(Ok(()))
        },
        &InstallCancellation::new(),
    )
    .await
    .unwrap();
    assert_eq!(calls, 1);
    fixture.assert_retained(false);
    for name in [
        ".langame-initial-package.json",
        ".langame-clean-package.json",
    ] {
        assert_eq!(
            fs::read(fixture.install_root().join(name)).unwrap(),
            b"fresh inventory"
        );
    }
    fixture.assert_clean().await;
}

#[tokio::test]
async fn fresh_payload_callback_failure_or_cancellation_keeps_steam_original_unpublished() {
    for cancel in [false, true] {
        let fixture = Fixture::new().await;
        let cancellation = InstallCancellation::new();
        let mut calls = 0;
        let result = install_with_callback(
            &fixture,
            &fixture.install_root(),
            |stage| {
                calls += 1;
                fs::write(
                    stage.join(".langame-clean-package.json"),
                    b"partial inventory",
                )
                .unwrap();
                if cancel {
                    cancellation.cancel();
                    std::future::ready(Ok(()))
                } else {
                    std::future::ready(Err(SteamCmdError::DirectDownloadFailed {
                        output_excerpt: "fixture callback rejected".into(),
                    }))
                }
            },
            &cancellation,
        )
        .await;
        assert_eq!(calls, 1);
        if cancel {
            assert!(matches!(
                result,
                Err(SteamCmdError::InstallCancelled { .. })
            ));
        } else {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("fixture callback rejected")
            );
        }
        fixture.assert_retained(true);
        assert!(!fixture.install_root().join("bin/Server.exe").exists());
        assert!(
            !fixture
                .install_root()
                .join(".langame-clean-package.json")
                .exists()
        );
        fixture.assert_clean().await;
    }
}

#[tokio::test]
async fn fresh_payload_callback_is_not_called_for_regular_steam_installation() {
    let fixture = Fixture::new().await;
    let target = fixture.root.join("separate-install");
    let mut called = false;
    install_with_callback(
        &fixture,
        &target,
        |_| {
            called = true;
            std::future::ready(Ok(()))
        },
        &InstallCancellation::new(),
    )
    .await
    .unwrap();
    assert!(!called);
    assert_eq!(
        fs::read(target.join("bin/Server.exe")).unwrap(),
        b"server payload"
    );
    fixture.assert_retained(true);
}
