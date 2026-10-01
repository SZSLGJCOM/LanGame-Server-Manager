use super::*;

struct Fixture {
    root: PathBuf,
    settings: AppSettings,
    module: ModuleDetails,
}

impl Fixture {
    fn new() -> Self {
        let root = crate::tests::unique_test_root();
        let settings = AppSettings {
            steamcmd_root: root.join("steamcmd").to_string_lossy().into_owned(),
            games_root: root.join("games").to_string_lossy().into_owned(),
            servers_root: root.join("servers").to_string_lossy().into_owned(),
            ..AppSettings::default()
        };
        let module = ModuleDetails {
            summary: app_core::ModuleSummary {
                id: "dependency-fixture".into(),
                name: "Dependency Fixture".into(),
                version: "1".into(),
                description: None,
                steam_app_id: Some(42),
                install_state: InstallState::NotInstalled,
                instance_program_count: 0,
                archived_program_count: 0,
                supported_platforms: vec!["windows".into()],
            },
            schema_json: None,
            default_ports: vec![],
            install: Some(InstallSpec {
                shared_game_dir: "game".into(),
                verification_path: Some("Server.exe".into()),
                download_url_windows: None,
                download_integrity_windows: None,
                source: None,
                minecraft: None,
            }),
            process: Some(ProcessSpec {
                executable: "Server.exe".into(),
                args_template: vec![],
                environment_template: Default::default(),
                working_directory_template: None,
                window_policy: app_core::ProcessWindowPolicy::Background,
                host_surface: app_core::ProcessHostSurface::ManagedTerminal,
                host_notes: None,
            }),
            workshop: None,
            mods: None,
            runtime: app_core::ModuleRuntimeSpec::default(),
        };
        Self {
            root,
            settings,
            module,
        }
    }

    async fn reject_install(&self, validate: bool) {
        let cancellation = InstallCancellation::new();
        let mut prepared_dependency = false;
        let result = install_or_update_module_with_progress_and_cancellation(
            &self.settings,
            &self.module,
            validate,
            &cancellation,
            |update| {
                // A regression must fail without downloading software or running
                // a dependency from the host: stop any implicit preparation.
                if update.detail.starts_with("SteamCMD:") {
                    prepared_dependency = true;
                    cancellation.cancel();
                }
            },
        )
        .await;
        assert!(
            !prepared_dependency,
            "server operations must not prepare SteamCMD"
        );
        assert!(
            matches!(result, Err(SteamCmdError::SteamCmdNotReady { .. })),
            "an unready dependency must reject the operation: {result:?}"
        );
        assert!(
            !Path::new(&self.settings.servers_root).exists(),
            "dependency rejection must not mutate revisions"
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn steamcmd_dependency_uninstall_blocks_fresh_install_without_recreating_either_root() {
    let fixture = Fixture::new();
    let root = Path::new(&fixture.settings.steamcmd_root);
    prepare_configured_steamcmd_root(root).unwrap();
    fs::write(root.join("steamcmd.exe"), b"owned fixture runtime").unwrap();
    crate::steamcmd_readiness::record_verified(&managed_steamcmd_status(&fixture.settings))
        .unwrap();
    assert!(managed_steamcmd_status(&fixture.settings).ready);
    remove_managed_steamcmd(&fixture.settings).await.unwrap();
    assert!(!root.exists());

    fixture.reject_install(false).await;

    assert!(!root.exists(), "server install must not reinstall SteamCMD");
    assert!(
        !Path::new(&fixture.settings.games_root).exists(),
        "dependency rejection must not create the game directory"
    );
}

#[tokio::test]
async fn steamcmd_dependency_unverified_executable_blocks_update_and_validation() {
    let fixture = Fixture::new();
    let root = Path::new(&fixture.settings.steamcmd_root);
    fs::create_dir_all(root).unwrap();
    fs::write(root.join("steamcmd.exe"), b"unverified fixture runtime").unwrap();
    let game_root = Path::new(&fixture.settings.games_root).join("game");
    fs::create_dir_all(&game_root).unwrap();
    fs::write(game_root.join("Server.exe"), b"existing server").unwrap();
    assert!(steamcmd_status(&fixture.settings).executable_exists);
    assert!(!steamcmd_status(&fixture.settings).ready);

    fixture.reject_install(false).await;
    fixture.reject_install(true).await;

    assert_eq!(
        fs::read(game_root.join("Server.exe")).unwrap(),
        b"existing server"
    );
    assert_eq!(
        fs::read(root.join("steamcmd.exe")).unwrap(),
        b"unverified fixture runtime"
    );
}
