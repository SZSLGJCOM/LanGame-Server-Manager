use super::*;

async fn install_with_callback<C, Fut>(
    fixture: &Fixture,
    url: &str,
    mut callback: C,
    cancellation: &InstallCancellation,
) -> Result<ModuleInstallResult, SteamCmdError>
where
    C: FnMut(PathBuf) -> Fut,
    Fut: std::future::Future<Output = Result<(), SteamCmdError>>,
{
    let root = fixture.install_root();
    let mut module = fixture.module.clone();
    module.install.as_mut().unwrap().download_url_windows = Some(url.to_owned());
    let guard =
        acquire_game_install_lifecycle(&module.summary.id, std::slice::from_ref(&root)).await?;
    let last_phase = std::sync::Mutex::new(None);
    install_or_update_module_at_with_callbacks(
        &fixture.settings,
        &module,
        &root,
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
async fn fresh_payload_callback_records_archive_inventory_before_retained_data_is_merged() {
    let fixture = Fixture::new();
    let mut server = PackageServer::new(fixture.archive(None).await).await;
    let mut calls = 0;
    install_with_callback(
        &fixture,
        &server.url,
        |stage| {
            calls += 1;
            assert_ne!(stage, fixture.install_root());
            assert_eq!(
                fs::read(stage.join("Assets/ServerConfig.json")).unwrap(),
                b"package defaults"
            );
            assert!(!stage.join("Assets/Worlds/world.sav").exists());
            assert!(!stage.join(".langame-clean-package.json").exists());
            for name in [
                ".langame-initial-package.json",
                ".langame-clean-package.json",
            ] {
                fs::write(stage.join(name), b"fresh inventory").unwrap();
            }
            std::future::ready(Ok(()))
        },
        &InstallCancellation::new(),
    )
    .await
    .unwrap();
    server.finish().await;
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
    fixture.assert_transaction_clean().await;
}

#[tokio::test]
async fn fresh_payload_callback_failure_or_cancellation_keeps_archive_original_unpublished() {
    for cancel in [false, true] {
        let fixture = Fixture::new();
        let mut server = PackageServer::new(fixture.archive(None).await).await;
        let cancellation = InstallCancellation::new();
        let mut calls = 0;
        let result = install_with_callback(
            &fixture,
            &server.url,
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
        server.finish().await;
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
        fixture.assert_transaction_clean().await;
    }
}

#[tokio::test]
async fn fresh_payload_callback_rejects_archive_supplied_inventory_before_observation() {
    let fixture = Fixture::new();
    let mut server =
        PackageServer::new(fixture.archive(Some(".langame-clean-package.json")).await).await;
    let mut called = false;
    let result = install_with_callback(
        &fixture,
        &server.url,
        |_| {
            called = true;
            std::future::ready(Ok(()))
        },
        &InstallCancellation::new(),
    )
    .await;
    server.finish().await;
    assert!(!called);
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("manager-owned package inventory")
    );
    fixture.assert_retained(true);
    fixture.assert_transaction_clean().await;
}

#[tokio::test]
async fn fresh_payload_callback_preserves_conflicting_retained_inventory_files_and_directories() {
    for directory in [false, true] {
        let fixture = Fixture::new();
        let original = fixture.install_root().join(".langame-clean-package.json");
        let data = if directory {
            fs::create_dir(&original).unwrap();
            original.join("operator.txt")
        } else {
            original
        };
        fs::write(&data, b"preserve unknown operator data").unwrap();
        let mut server = PackageServer::new(fixture.archive(None).await).await;
        let result = install_with_callback(
            &fixture,
            &server.url,
            |stage| {
                fs::write(
                    stage.join(".langame-clean-package.json"),
                    b"fresh inventory",
                )
                .unwrap();
                std::future::ready(Ok(()))
            },
            &InstallCancellation::new(),
        )
        .await;
        server.finish().await;
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("conflicts with fresh manager-owned package inventory")
        );
        assert_eq!(fs::read(data).unwrap(), b"preserve unknown operator data");
        fixture.assert_retained(true);
        assert!(!fixture.install_root().join("bin/Server.exe").exists());
        fixture.assert_transaction_clean().await;
    }
}
