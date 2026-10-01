use super::*;

#[path = "steam_install_payload_callback_tests.rs"]
mod payload_callback_tests;

struct Fixture {
    root: PathBuf,
    settings: AppSettings,
    module: ModuleDetails,
}

impl Fixture {
    async fn new() -> Self {
        let root = crate::tests::unique_test_root();
        let settings = AppSettings {
            archives_root: String::new(),
            games_root: root.join("games").to_string_lossy().into_owned(),
            servers_root: root.join("instances").to_string_lossy().into_owned(),
            modules_root: root.join("modules").to_string_lossy().into_owned(),
            steamcmd_root: root.join("steamcmd").to_string_lossy().into_owned(),
        };
        let module = ModuleDetails {
            summary: app_core::ModuleSummary {
                id: "retained-steam-fixture".into(),
                name: "Retained Steam Fixture".into(),
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
                verification_path: Some("bin/Server.exe".into()),
                download_url_windows: None,
                download_integrity_windows: None,
                source: None,
                minecraft: None,
            }),
            process: Some(ProcessSpec {
                executable: "{{config.server_executable}}".into(),
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
        let fixture = Self {
            root,
            settings,
            module,
        };
        fs::create_dir_all(fixture.install_root().join("ServerConfig")).unwrap();
        fs::write(
            fixture.install_root().join("ServerConfig/Server.cfg"),
            b"operator settings",
        )
        .unwrap();
        fs::write(fixture.install_root().join("world.sav"), b"operator world").unwrap();
        mark_retained_install_data(&fixture.install_root()).unwrap();
        fs::create_dir_all(&fixture.settings.steamcmd_root).unwrap();
        // This owned executable implements only the runscript contract. It writes
        // real colliding depot defaults through the production process runner.
        let source = r#"
using System;
using System.IO;
public static class Installer {
  public static int Main(string[] args) {
    if (args.Length == 1 && args[0] == "+quit") {
      Console.WriteLine("Steam Console Client (c) Valve Corporation\nLoading Steam API...OK");
      return 0;
    }
    if (args.Length != 2 || args[0] != "+runscript") return 80;
    string root = null;
    foreach (string line in File.ReadAllLines(args[1])) {
      if (line.StartsWith("force_install_dir ")) root = line.Substring(18).Trim().Trim('"');
    }
    if (root == null) return 81;
    File.WriteAllText("destination.txt", root);
    Directory.CreateDirectory(Path.Combine(root, "ServerConfig"));
    File.WriteAllText(Path.Combine(root, "ServerConfig/Server.cfg"), "depot defaults");
    if (File.Exists("fail")) return 9;
    Directory.CreateDirectory(Path.Combine(root, "bin"));
    File.WriteAllText(Path.Combine(root, "bin/Server.exe"), "server payload");
    Console.WriteLine("Success! App '42' fully installed.");
    return 0;
  }
}
"#;
        let script = format!(
            "$ErrorActionPreference='Stop'; Add-Type -TypeDefinition @'\n{source}\n'@ -OutputAssembly {} -OutputType ConsoleApplication",
            ps_literal(&PathBuf::from(&fixture.settings.steamcmd_root).join("steamcmd.exe"))
        );
        let output = run_powershell(&script, Some(&fixture.root), test_deadline())
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            output_excerpt(&output.stdout, &output.stderr)
        );
        let status = steamcmd_status(&fixture.settings);
        assert_eq!(status.ownership, SteamCmdOwnership::External);
        ensure_steamcmd_installed(&fixture.settings)
            .await
            .expect("explicit fixture preparation");
        fixture
    }

    fn install_root(&self) -> PathBuf {
        PathBuf::from(&self.settings.games_root).join("game")
    }

    async fn install(
        &self,
        progress: impl FnMut(InstallProgressUpdate),
    ) -> Result<ModuleInstallResult, SteamCmdError> {
        install_or_update_module_with_progress(&self.settings, &self.module, false, progress).await
    }

    fn assert_retained(&self, uninstalled: bool) {
        assert_eq!(
            fs::read(self.install_root().join("ServerConfig/Server.cfg")).unwrap(),
            b"operator settings"
        );
        assert_eq!(
            fs::read(self.install_root().join("world.sav")).unwrap(),
            b"operator world"
        );
        assert_eq!(has_retained_install_data(&self.install_root()), uninstalled);
    }

    async fn assert_clean(&self) {
        let limit = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let entries: Vec<_> = fs::read_dir(&self.settings.games_root)
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();
            if entries == [std::ffi::OsString::from("game")] {
                return;
            }
            assert!(
                std::time::Instant::now() < limit,
                "transaction debris: {entries:?}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn test_deadline() -> InstallDeadline {
    InstallDeadline::new("retained Steam fixture", Duration::from_secs(30))
}

#[tokio::test]
async fn explicit_program_install_reuses_held_lock_and_isolates_target_and_revision() {
    let fixture = Fixture::new().await;
    let target = fixture.root.join("instances/first/runtime");
    let other = fixture.root.join("instances/second/runtime");
    let guard = acquire_game_install_lifecycle(
        &fixture.module.summary.id,
        &[target.clone(), other.clone()],
    )
    .await
    .unwrap();
    let cancellation = InstallCancellation::new();
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        install_or_update_module_at_with_progress_and_cancellation(
            &fixture.settings,
            &fixture.module,
            &target,
            &guard,
            true,
            &cancellation,
            |_| {},
        ),
    )
    .await
    .expect("held lifecycle lock must not be reacquired")
    .unwrap();
    assert_eq!(Path::new(&result.install_root), target);
    assert_eq!(result.install_state, InstallState::Installed);
    assert_eq!(
        fs::read(target.join("bin/Server.exe")).unwrap(),
        b"server payload"
    );
    fixture.assert_retained(true);
    assert!(!fixture.install_root().join("bin/Server.exe").exists());
    let revisions = Path::new(&fixture.settings.servers_root);
    assert_eq!(
        read_program_install_revision(revisions, &fixture.module.summary.id, &target).unwrap(),
        1
    );
    assert_eq!(
        read_program_install_revision(revisions, &fixture.module.summary.id, &other).unwrap(),
        0
    );
    assert_eq!(
        read_game_install_revision(revisions, &fixture.module.summary.id).unwrap(),
        0
    );
    cancellation.cancel();
    let cancelled = install_or_update_module_at_with_progress_and_cancellation(
        &fixture.settings,
        &fixture.module,
        &other,
        &guard,
        false,
        &cancellation,
        |_| {},
    )
    .await;
    assert!(matches!(
        cancelled,
        Err(SteamCmdError::InstallCancelled { .. })
    ));
    assert!(!other.exists());
    assert_eq!(
        read_program_install_revision(revisions, &fixture.module.summary.id, &other).unwrap(),
        0
    );
}

#[tokio::test]
async fn retained_steam_reinstall_preserves_config_overwritten_by_depot() {
    let fixture = Fixture::new().await;
    let result = fixture.install(|_| {}).await.unwrap();
    assert_eq!(result.install_state, InstallState::Installed);
    fixture.assert_retained(false);
    let destination =
        fs::read_to_string(PathBuf::from(&fixture.settings.steamcmd_root).join("destination.txt"))
            .unwrap();
    assert_ne!(Path::new(&destination), fixture.install_root());
    fixture.assert_clean().await;
}

#[tokio::test]
async fn retained_steam_failed_native_command_keeps_original_data_and_marker() {
    let fixture = Fixture::new().await;
    fs::write(
        PathBuf::from(&fixture.settings.steamcmd_root).join("fail"),
        b"fail",
    )
    .unwrap();
    assert!(matches!(
        fixture.install(|_| {}).await,
        Err(SteamCmdError::SteamCmdCommandFailed { .. })
    ));
    fixture.assert_retained(true);
    assert!(!fixture.install_root().join("bin/Server.exe").exists());
    fixture.assert_clean().await;
}

#[tokio::test]
async fn retained_steam_failed_probe_restores_original_data_and_marker() {
    let fixture = Fixture::new().await;
    let mut published_seen = false;
    let result = fixture
        .install(|update| {
            if update
                .install_progress
                .as_ref()
                .is_some_and(|progress| progress.phase == InstallPhase::Verifying)
                && !has_retained_install_data(&fixture.install_root())
            {
                published_seen = true;
                assert!(!has_retained_install_data(&fixture.install_root()));
                fs::remove_file(fixture.install_root().join("bin/Server.exe")).unwrap();
            }
        })
        .await;
    assert!(published_seen);
    assert!(matches!(
        result,
        Err(SteamCmdError::InstallationVerificationFailed { .. })
    ));
    fixture.assert_retained(true);
    fixture.assert_clean().await;
}

#[tokio::test]
async fn retained_steam_cancel_after_publication_restores_original_data_and_marker() {
    let fixture = Fixture::new().await;
    let cancellation = InstallCancellation::new();
    let mut published_seen = false;
    let result = install_or_update_module_with_progress_and_cancellation(
        &fixture.settings,
        &fixture.module,
        false,
        &cancellation,
        |update| {
            if update
                .install_progress
                .as_ref()
                .is_some_and(|progress| progress.phase == InstallPhase::Verifying)
                && !has_retained_install_data(&fixture.install_root())
            {
                published_seen = true;
                assert!(!has_retained_install_data(&fixture.install_root()));
                cancellation.cancel();
            }
        },
    )
    .await;
    assert!(published_seen);
    assert!(matches!(
        result,
        Err(SteamCmdError::InstallCancelled { .. })
    ));
    fixture.assert_retained(true);
    assert!(!fixture.install_root().join("bin/Server.exe").exists());
    fixture.assert_clean().await;
}

#[tokio::test]
async fn retained_steam_rejects_hardlinks_without_changing_external_data() {
    let fixture = Fixture::new().await;
    let outside = fixture.root.join("outside.cfg");
    fs::write(&outside, b"external configuration").unwrap();
    fs::hard_link(
        &outside,
        fixture.install_root().join("ServerConfig/linked.cfg"),
    )
    .unwrap();
    let result = fixture.install(|_| {}).await;
    assert!(matches!(
        result,
        Err(SteamCmdError::SteamCmdCommandFailed { .. })
    ));
    fixture.assert_retained(true);
    assert_eq!(fs::read(outside).unwrap(), b"external configuration");
    fixture.assert_clean().await;
}

#[tokio::test]
async fn retained_steam_rejects_junctions_without_changing_external_data() {
    let fixture = Fixture::new().await;
    let outside = fixture.root.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"external world").unwrap();
    let junction = fixture.install_root().join("linked");
    let script = format!(
        "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path {} -Target {} | Out-Null",
        ps_literal(&junction),
        ps_literal(&outside)
    );
    let output = run_powershell(&script, Some(&fixture.root), test_deadline())
        .await
        .unwrap();
    assert!(output.status.success());
    let result = fixture.install(|_| {}).await;
    assert!(matches!(
        result,
        Err(SteamCmdError::SteamCmdCommandFailed { .. })
    ));
    fixture.assert_retained(true);
    assert_eq!(
        fs::read(outside.join("sentinel")).unwrap(),
        b"external world"
    );
    fs::remove_dir(junction).unwrap();
    fixture.assert_clean().await;
}
