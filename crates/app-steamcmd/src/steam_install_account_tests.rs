use super::*;

async fn install_with_account(
    fixture: &Fixture,
    target: &Path,
    account_name: Option<&str>,
) -> (Result<ModuleInstallResult, SteamCmdError>, usize) {
    let guard = acquire_game_install_lifecycle(
        crate::test_install_lock_context(),
        &fixture.module.summary.id,
        &[target.to_owned()],
    )
    .await
    .unwrap();
    let cancellation = InstallCancellation::new();
    let mut callbacks = 0;
    let result = install_or_update_module_at_with_login_and_callbacks(
        SteamModuleInstallRequest {
            settings: &fixture.settings,
            module: &fixture.module,
            install_root: target,
            guard: &guard,
            validate: false,
            cancellation: &cancellation,
            account_name,
        },
        ModuleInstallCallbacks {
            on_progress: |_| {},
            prepare_fresh_payload: |_| {
                callbacks += 1;
                std::future::ready(Ok(()))
            },
        },
    )
    .await;
    (result, callbacks)
}

fn set_mode(fixture: &Fixture, mode: &str) {
    fs::write(
        Path::new(&fixture.settings.steamcmd_root).join("account-mode"),
        mode,
    )
    .unwrap();
}

fn logins(fixture: &Fixture) -> Vec<String> {
    fs::read_to_string(Path::new(&fixture.settings.steamcmd_root).join("login-commands"))
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[tokio::test]
async fn anonymous_server_permission_failure_remains_failure_with_exit_zero_and_cached_executable()
{
    for mode in ["denied", "failure"] {
        let fixture = Fixture::new().await;
        let target = fixture.root.join("program");
        fs::create_dir_all(target.join("bin")).unwrap();
        fs::write(target.join("bin/Server.exe"), b"old server").unwrap();
        set_mode(&fixture, mode);
        let (result, callbacks) = install_with_account(&fixture, &target, None).await;
        let error = result.unwrap_err();
        if mode == "denied" {
            assert!(matches!(
                error,
                SteamCmdError::SteamAppAccountRequired { app_id: 42, .. }
            ));
        } else {
            assert!(matches!(error, SteamCmdError::SteamCmdCommandFailed { .. }));
        }
        assert_eq!(callbacks, 0);
        assert_eq!(
            fs::read(target.join("bin/Server.exe")).unwrap(),
            b"old server"
        );
        assert_eq!(logins(&fixture), ["login anonymous"]);
        fixture.assert_retained(true);
    }
}

#[tokio::test]
async fn cached_server_account_login_publishes_only_after_verified_identity() {
    let fixture = Fixture::new().await;
    let target = fixture.install_root();
    set_mode(&fixture, "authenticated");
    let (result, callbacks) =
        install_with_account(&fixture, &target, Some("selected_account")).await;
    let result = result.unwrap();
    assert_eq!(result.install_state, InstallState::Installed);
    assert_eq!(callbacks, 1);
    assert_eq!(logins(&fixture), ["login selected_account"]);
    fixture.assert_retained(false);
    fixture.assert_clean().await;
}

#[tokio::test]
async fn wrong_server_account_never_publishes_or_retries_anonymous() {
    let fixture = Fixture::new().await;
    let target = fixture.install_root();
    set_mode(&fixture, "wrong-account");
    let (result, callbacks) =
        install_with_account(&fixture, &target, Some("selected_account")).await;
    assert!(matches!(
        result,
        Err(SteamCmdError::SteamAccountAuthenticationFailed { .. })
    ));
    assert_eq!(callbacks, 0);
    assert_eq!(logins(&fixture), ["login selected_account"]);
    assert!(!target.join("bin/Server.exe").exists());
    fixture.assert_retained(true);
    fixture.assert_clean().await;
}

#[tokio::test]
async fn invalid_server_account_is_rejected_before_native_or_filesystem_work() {
    let fixture = Fixture::new().await;
    let target = fixture.root.join("program");
    let (result, callbacks) = install_with_account(&fixture, &target, Some("user\nquit")).await;
    assert!(matches!(
        result,
        Err(SteamCmdError::InvalidSteamAccountName)
    ));
    assert_eq!(callbacks, 0);
    assert!(!target.exists());
    assert!(
        !Path::new(&fixture.settings.steamcmd_root)
            .join("login-commands")
            .exists()
    );
    fixture.assert_retained(true);
}
