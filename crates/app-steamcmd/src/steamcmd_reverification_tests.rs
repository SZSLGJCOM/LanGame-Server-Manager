use super::*;
use crate::*;
use std::path::Path;

const READY_RECORD: &str = ".langame-steamcmd-ready.json";

struct Fixture {
    root: PathBuf,
    settings: AppSettings,
}

impl Fixture {
    async fn new() -> Self {
        let root = crate::tests::unique_test_root();
        let settings = AppSettings {
            steamcmd_root: root.join("steamcmd").to_string_lossy().into_owned(),
            games_root: root.join("games").to_string_lossy().into_owned(),
            servers_root: root.join("servers").to_string_lossy().into_owned(),
            ..AppSettings::default()
        };
        let native = Path::new(&settings.steamcmd_root);
        prepare_configured_steamcmd_root(native).unwrap();
        // An owned executable is the external process boundary. It implements
        // real +quit, runscript and interactive launch contracts without Steam
        // connections, actual credentials or installed game data.
        let source = r#"
using System;
using System.IO;
public static class ReverificationFixture {
  private static void Runtime() {
    Console.WriteLine("Steam Console Client (c) Valve Corporation - fixture");
    Console.WriteLine("Loading Steam API...OK");
  }
  private static void Account() {
    Console.WriteLine("Logging in user 'selected_account' to Steam Public...OK");
    Console.WriteLine("Waiting for user info...OK");
  }
  public static int Main(string[] args) {
    if (args.Length == 1 && args[0] == "+quit") {
      File.AppendAllText("native-calls", "verify\n");
      string mode = File.Exists("verify-mode") ? File.ReadAllText("verify-mode").Trim() : "valid";
      if (mode == "missing-api") {
        Console.WriteLine("Steam Console Client (c) Valve Corporation - fixture");
        return 0;
      }
      Runtime();
      if (mode == "exit-failure") return 9;
      if (mode == "stall") { Console.Out.Flush(); new System.Threading.ManualResetEvent(false).WaitOne(); }
      return 0;
    }
    if (Array.IndexOf(args, "+runscript") < 0) {
      if (Array.IndexOf(args, "+login") >= 0 || Array.IndexOf(args, "+quit") >= 0) return 83;
      File.AppendAllText("native-calls", "interactive\n");
      Runtime(); Console.Write("Steam>"); Console.Out.Flush();
      if (Console.ReadLine() != "login selected_account") return 84;
      Account(); Console.Write("Steam>"); Console.Out.Flush();
      if (Console.ReadLine() != "quit") return 85;
      return 0;
    }
    int scriptIndex = Array.IndexOf(args, "+runscript");
    bool plainScript = args.Length == 2 && scriptIndex == 0;
    bool managedScript = args.Length == 4 && scriptIndex == 2
      && args[0] == "-overridepackageurl" && args[1].StartsWith("http://127.0.0.1:");
    if (!plainScript && !managedScript) return 80;
    string script = File.ReadAllText(args[scriptIndex + 1]);
    Runtime();
    if (script.Contains("workshop_download_item 1623730 3816899969")) {
      File.AppendAllText("native-calls", "workshop\n");
      Console.WriteLine("ERROR! Download item 3816899969 failed (No subscription).");
      return 0;
    }
    if (script.Contains("app_update 42")) {
      File.AppendAllText("native-calls", "app-update\n");
      Console.WriteLine("ERROR! Failed to install app '42' (No subscription)");
      return 0;
    }
    if (script.Contains("login selected_account")) {
      File.AppendAllText("native-calls", "cache\n");
      Account(); return 0;
    }
    return 81;
  }
}
"#;
        let executable = native.join("steamcmd.exe");
        let script = format!(
            "$ErrorActionPreference='Stop'; Add-Type -TypeDefinition @'\n{source}\n'@ -OutputAssembly {} -OutputType ConsoleApplication",
            ps_literal(&executable)
        );
        let compiled = run_powershell(&script, Some(native), deadline())
            .await
            .unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            output_excerpt(&compiled.stdout, &compiled.stderr)
        );
        // Managed runscript launches retain their real cached-manifest bridge.
        // The fake native boundary consumes no packages or remote endpoints.
        fs::create_dir(native.join("package")).unwrap();
        fs::write(
            native.join("package/steam_cmd_win64.manifest"),
            format!(
                "\"win64\" {{ \"version\" \"123\" \"steamcmd_win64\" {{ \"file\" \"steamcmd_win64.zip.hash\" \"size\" \"12\" \"sha2\" \"{}\" \"IsBootstrapperPackage\" \"1\" }} }}\r\n",
                "a".repeat(64),
            ),
        ).unwrap();
        // Seed the prior record through the real +quit verifier, never by
        // assuming that a leftover executable is a verified installation.
        let guard = acquire_steamcmd_lifecycle(test_install_lock_context(), &settings)
            .await
            .unwrap();
        let status = managed_steamcmd_status(&settings);
        let mut reporter = PrepareReporter::new(|_| {});
        warm_up_steamcmd(&status, deadline(), &mut reporter)
            .await
            .unwrap();
        reporter.ready(status).unwrap();
        drop(guard);
        Self { root, settings }
    }

    fn native(&self) -> &Path {
        Path::new(&self.settings.steamcmd_root)
    }

    fn stale(&self, bytes: &[u8]) {
        fs::write(self.native().join("steamclient64.dll"), bytes).unwrap();
        assert!(!steamcmd_status(&self.settings).ready);
    }

    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.native().join("native-calls"))
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn record(&self) -> Vec<u8> {
        fs::read(self.native().join(READY_RECORD)).unwrap()
    }

    async fn workshop(&self) -> Result<SteamWorkshopDownloadResult, SteamCmdError> {
        let target = Path::new(&self.settings.games_root).join("game");
        let game = acquire_game_install_lifecycle(
            test_install_lock_context(),
            "reverification-fixture",
            std::slice::from_ref(&target),
        )
        .await?;
        let steamcmd = game.acquire_steamcmd(&self.settings).await?;
        download_workshop_items_with_progress(
            &self.settings,
            1_623_730,
            &target,
            &["3816899969".into()],
            &game,
            &steamcmd,
            |_| {},
        )
        .await
    }

    async fn login(&self) -> Result<SteamWorkshopAccountAuthenticationResult, SteamCmdError> {
        let (sender, mut input) = tokio::sync::mpsc::channel(1);
        authenticate_workshop_account_with_progress(
            test_install_lock_context(),
            &self.settings,
            "selected_account",
            &InstallCancellation::new(),
            &mut input,
            Duration::from_secs(20),
            |progress| {
                if matches!(
                    progress.phase,
                    SteamWorkshopAccountPhase::Password | SteamWorkshopAccountPhase::GuardCode
                ) {
                    sender
                        .try_send(SteamWorkshopAccountInput {
                            challenge_revision: progress.challenge_revision,
                            value: if progress.phase == SteamWorkshopAccountPhase::Password {
                                "synthetic-password"
                            } else {
                                "7Q9XW"
                            }
                            .into(),
                        })
                        .unwrap();
                }
            },
        )
        .await
    }

    async fn install(&self) -> Result<ModuleInstallResult, SteamCmdError> {
        let module = self.module();
        let target = Path::new(&self.settings.games_root).join("game");
        let guard = acquire_game_install_lifecycle(
            test_install_lock_context(),
            &module.summary.id,
            std::slice::from_ref(&target),
        )
        .await?;
        install_or_update_module_at_with_progress_and_cancellation(
            &self.settings,
            &module,
            &target,
            &guard,
            false,
            &InstallCancellation::new(),
            |_| {},
        )
        .await
    }

    fn module(&self) -> ModuleDetails {
        ModuleDetails {
            summary: app_core::ModuleSummary {
                id: "reverification-fixture".into(),
                name: "Reverification fixture".into(),
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
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn deadline() -> InstallDeadline {
    InstallDeadline::new("SteamCMD reverification fixture", Duration::from_secs(30))
}

#[tokio::test]
async fn stale_managed_runtime_is_reverified_before_workshop_access_and_subsequent_login() {
    let fixture = Fixture::new().await;
    let old_record = fixture.record();
    fixture.stale(b"first native self-update");
    let result = fixture.workshop().await;
    assert!(
        matches!(
            &result,
            Err(SteamCmdError::WorkshopAccountRequired {
                consumer_app_id: 1_623_730,
                ..
            })
        ),
        "{result:?}"
    );
    assert!(steamcmd_status(&fixture.settings).ready);
    assert_ne!(fixture.record(), old_record);
    assert_eq!(fixture.calls(), ["verify", "verify", "workshop"]);

    // The login entry also owns revalidation when another native update occurs.
    fixture.stale(b"a later native runtime self-update");
    let account = fixture.login().await.unwrap();
    assert_eq!(account.account_name, "selected_account");
    assert!(steamcmd_status(&fixture.settings).ready);
    assert_eq!(
        fixture.calls(),
        [
            "verify",
            "verify",
            "workshop",
            "verify",
            "interactive",
            "cache"
        ]
    );
}

#[tokio::test]
async fn stale_managed_runtime_is_reverified_before_server_installation_preflight() {
    let fixture = Fixture::new().await;
    fixture.stale(b"native runtime self-update");
    let result = fixture.install().await;
    assert!(
        matches!(
            &result,
            Err(SteamCmdError::SteamAppAccountRequired { app_id: 42, .. })
        ),
        "{result:?}"
    );
    assert!(steamcmd_status(&fixture.settings).ready);
    assert_eq!(fixture.calls(), ["verify", "verify", "app-update"]);
}

#[tokio::test]
async fn failed_or_incomplete_reverification_never_refreshes_a_stale_record() {
    let fixture = Fixture::new().await;
    fixture.stale(b"native runtime self-update");
    let old_record = fixture.record();
    let guard = acquire_steamcmd_lifecycle(test_install_lock_context(), &fixture.settings)
        .await
        .unwrap();
    for mode in ["missing-api", "exit-failure"] {
        fs::write(fixture.native().join("verify-mode"), mode).unwrap();
        assert!(matches!(
            guard
                .require_ready_with_progress(&fixture.settings, deadline(), |_| {})
                .await,
            Err(SteamCmdError::PrepareSteamCmd { .. })
        ));
        assert_eq!(fixture.record(), old_record);
        assert!(!steamcmd_status(&fixture.settings).ready);
    }
    assert_eq!(fixture.calls(), ["verify", "verify", "verify"]);
    assert!(!Path::new(&fixture.settings.games_root).exists());
    assert!(!Path::new(&fixture.settings.servers_root).exists());
}

#[tokio::test]
async fn cancelled_or_timed_out_reverification_keeps_the_old_record_unready() {
    let fixture = Fixture::new().await;
    fixture.stale(b"native runtime self-update");
    let old_record = fixture.record();
    let guard = acquire_steamcmd_lifecycle(test_install_lock_context(), &fixture.settings)
        .await
        .unwrap();
    let cancellation = InstallCancellation::new();
    cancellation.cancel();
    assert!(matches!(
        cancellation
            .scope(guard.require_ready_with_progress(&fixture.settings, deadline(), |_| {}))
            .await,
        Err(SteamCmdError::InstallCancelled { .. })
    ));
    assert_eq!(fixture.calls(), ["verify"]);
    fs::write(fixture.native().join("verify-mode"), "stall").unwrap();
    assert!(matches!(
        guard
            .require_ready_with_progress(
                &fixture.settings,
                deadline().limited_to(Duration::from_secs(2)),
                |_| {}
            )
            .await,
        Err(SteamCmdError::SteamCmdPreparationTimedOut { .. })
    ));
    assert_eq!(fixture.record(), old_record);
    assert!(!steamcmd_status(&fixture.settings).ready);
    assert_eq!(fixture.calls(), ["verify", "verify"]);
}

#[tokio::test]
async fn invalid_or_foreign_records_do_not_start_implicit_dependency_preparation() {
    let fixture = Fixture::new().await;
    fixture.stale(b"native runtime self-update");
    let saved: serde_json::Value = serde_json::from_slice(&fixture.record()).unwrap();
    let mut wrong_root = saved.clone();
    wrong_root["root"] = "another canonical runtime".into();
    let mut missing_exe = saved.clone();
    missing_exe["files"] = serde_json::json!([]);
    let mut duplicate = saved.clone();
    duplicate["files"] = serde_json::json!([saved["files"][0], saved["files"][0]]);
    let mut unknown = saved.clone();
    unknown["unexpected"] = true.into();
    for invalid in [wrong_root, missing_exe, duplicate, unknown] {
        fs::write(
            fixture.native().join(READY_RECORD),
            serde_json::to_vec(&invalid).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            fixture.workshop().await,
            Err(SteamCmdError::SteamCmdNotReady { .. })
        ));
        assert!(matches!(
            fixture.login().await,
            Err(SteamCmdError::SteamCmdNotReady { .. })
        ));
    }
    fs::write(fixture.native().join(READY_RECORD), b"truncated record {").unwrap();
    assert!(matches!(
        fixture.install().await,
        Err(SteamCmdError::SteamCmdNotReady { .. })
    ));
    fs::remove_file(fixture.native().join(READY_RECORD)).unwrap();
    assert!(matches!(
        fixture.workshop().await,
        Err(SteamCmdError::SteamCmdNotReady { .. })
    ));
    fs::write(
        fixture.native().join(READY_RECORD),
        serde_json::to_vec(&saved).unwrap(),
    )
    .unwrap();
    fs::remove_file(fixture.native().join(".langame-steamcmd-owner.json")).unwrap();
    assert_eq!(
        steamcmd_status(&fixture.settings).ownership,
        SteamCmdOwnership::External
    );
    assert!(matches!(
        fixture.workshop().await,
        Err(SteamCmdError::SteamCmdNotReady { .. })
    ));
    assert_eq!(fixture.calls(), ["verify"]);
    assert!(!Path::new(&fixture.settings.games_root).exists());
    assert!(!Path::new(&fixture.settings.servers_root).exists());
}
