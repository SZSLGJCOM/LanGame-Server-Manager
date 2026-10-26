use super::*;
use crate::*;
use std::path::Path;

struct Fixture {
    root: PathBuf,
    settings: AppSettings,
}

impl Fixture {
    async fn new() -> Self {
        let root = crate::tests::unique_test_root();
        let settings = AppSettings {
            steamcmd_root: root.join("steamcmd").to_string_lossy().into_owned(),
            ..AppSettings::default()
        };
        let native = Path::new(&settings.steamcmd_root);
        prepare_configured_steamcmd_root(native).unwrap();
        let source = r#"
using System;
using System.IO;
using System.Net;
using System.Diagnostics;
public static class AccountBridgeFixture {
  private static void Runtime() {
    Console.WriteLine("Steam Console Client (c) Valve Corporation - fixture");
    Console.WriteLine("Loading Steam API...OK");
  }
  public static int Main(string[] args) {
    if (args.Length == 1 && args[0] == "+quit") { Runtime(); return 0; }
    int script = Array.IndexOf(args, "+runscript");
    string phase = script < 0 ? "interactive" : "cache";
    if (Array.IndexOf(args, "+login") >= 0 || Array.IndexOf(args, "selected_account") >= 0
      || Array.IndexOf(args, "7Q9XW") >= 0 || Array.IndexOf(args, "MAIL7") >= 0
      || Array.IndexOf(args, "fixture-password") >= 0
      || (script < 0 && Array.IndexOf(args, "+quit") >= 0)) return 80;
    if (script >= 0) {
      string contents = File.ReadAllText(args[script + 1]);
      if (contents.Contains("7Q9XW") || contents.Contains("MAIL7") || contents.Contains("fixture-password")) return 85;
    }
    int update = Array.IndexOf(args, "-overridepackageurl");
    string endpoint = update >= 0 && update + 1 < args.Length ? args[update + 1] : "plain";
    if (endpoint != "plain") {
      if (update != 0 || !endpoint.StartsWith("http://127.0.0.1:")) return 81;
      HttpWebRequest request = (HttpWebRequest)WebRequest.Create(endpoint + "/steam_cmd_win64");
      request.Proxy = null; request.Timeout = 5000;
      using (WebResponse response = request.GetResponse())
      using (StreamReader reader = new StreamReader(response.GetResponseStream())) {
        if (!reader.ReadToEnd().Contains("987654321")) return 82;
      }
    }
    File.AppendAllText("bridge-calls", phase + " " + endpoint + "\n");
    string mode = File.Exists("mode") ? File.ReadAllText("mode").Trim() : "success";
    if (mode == phase + "-failure") return 9;
    if (phase == "interactive") {
      int attempt = File.Exists("attempts") ? File.ReadAllLines("attempts").Length + 1 : 1;
      if (attempt > 2) return 89;
      if (File.Exists("native-pid")) {
        try { using (Process old = Process.GetProcessById(Int32.Parse(File.ReadAllText("native-pid")))) {
          if (!old.HasExited) return 88;
        }} catch (ArgumentException) {}
      }
      File.WriteAllText("native-pid", Process.GetCurrentProcess().Id.ToString());
      File.AppendAllText("attempts", "native\n");
      Runtime(); Console.Write("Steam>"); Console.Out.Flush();
      string expected = attempt == 1 ? "login selected_account" : "login selected_account \"\" 7Q9XW";
      if (Console.ReadLine() != expected) return 83;
      if (mode == "progress") {
        while (!File.Exists("connection-progress-seen")) System.Threading.Thread.Sleep(10);
      }
      if (mode == "return-prompt") {
        Console.WriteLine("Login Failure: Invalid Password"); Console.Write("Steam>"); Console.Out.Flush();
        new System.Threading.ManualResetEvent(false).WaitOne();
      }
      if (mode != "cached-success" && !(mode == "mobile-cached" && attempt == 2)) {
        if (mode == "cached-invalid-password") {
          Console.WriteLine("Logging in using cached credentials.");
          Console.WriteLine("Warning: invalid cached credentials.");
        }
        Console.Write("Password: "); Console.Out.Flush();
        string password = mode == "retry-password-mobile" && attempt == 2 ? "fixture-server-password" : "fixture-password";
        if (Console.ReadLine() != password) return 86;
        File.AppendAllText("password-reads", "native\n");
        if (attempt == 1 && mode.StartsWith("retry-password")) {
          Console.WriteLine("Login Failure: Invalid Password");
          Console.Write("Password: "); Console.Out.Flush();
          if (Console.ReadLine() != "fixture-server-password") return 87;
          File.AppendAllText("password-reads", "retry\n");
        }
      }
      if ((mode.StartsWith("mobile") || mode == "retry-password-mobile") && (attempt == 1 || mode == "mobile-again")) {
        Console.WriteLine("Please confirm the login in the Steam Mobile app on your phone."); Console.Out.Flush();
        if (mode == "mobile-approved") {
          while (!File.Exists("mobile-approved")) System.Threading.Thread.Sleep(10);
        } else new System.Threading.ManualResetEvent(false).WaitOne();
      }
      if (mode == "native-email" || mode == "native-token" || mode == "mobile-retry-code" && attempt == 2) {
        if (mode == "native-email") {
          Console.WriteLine("Please check your email for the message from Steam, and enter the Steam Guard");
          Console.WriteLine("code from that message.");
          Console.Write("Steam Guard code: ");
        } else Console.Write("Two-factor code: ");
        Console.Out.Flush();
        if (Console.ReadLine() != (mode == "native-email" ? "MAIL7" : "7Q9XW")) return 90;
      }
    }
    Runtime();
    Console.WriteLine("Logging in user 'selected_account' to Steam Public...OK");
    if (mode == "progress" && phase == "interactive") {
      Console.WriteLine("Waiting for client config...OK"); Console.Out.Flush();
      while (!File.Exists("auth-progress-seen")) System.Threading.Thread.Sleep(10);
    }
    Console.WriteLine("Waiting for user info...OK");
    if (phase == "interactive") {
      Console.Write("Steam>"); Console.Out.Flush();
      if (Console.ReadLine() != "quit") return 84;
    }
    return 0;
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
        fs::create_dir(native.join("package")).unwrap();
        fs::write(native.join("package/steam_cmd_win64.manifest"), format!(
            "\"win64\" {{ \"version\" \"987654321\" \"steamcmd_win64\" {{ \"file\" \"steamcmd_win64.zip.hash\" \"size\" \"12\" \"sha2\" \"{}\" \"IsBootstrapperPackage\" \"1\" }} }}\r\n", "a".repeat(64),
        )).unwrap();
        let guard = acquire_steamcmd_operation(
            test_install_lock_context(),
            &settings,
            InstallDeadline::new("SteamCMD test lifecycle", GAME_INSTALL_LIFECYCLE_TIMEOUT),
        )
        .await
        .unwrap();
        let mut command = Command::new(&executable);
        command.arg("+quit").current_dir(native);
        let verified = run_command_capture(command, deadline()).await.unwrap();
        let mut evidence = crate::steamcmd_runtime_evidence::RuntimeEvidence::default();
        evidence.push(0, &verified.stdout);
        evidence.push(1, &verified.stderr);
        assert!(verified.status.success() && evidence.ready());
        crate::steamcmd_readiness::record_verified(&managed_steamcmd_status(&settings)).unwrap();
        drop(guard);
        Self { root, settings }
    }

    fn mode(&self, mode: &str) {
        fs::write(Path::new(&self.settings.steamcmd_root).join("mode"), mode).unwrap();
    }

    fn calls(&self) -> Vec<(String, String)> {
        fs::read_to_string(Path::new(&self.settings.steamcmd_root).join("bridge-calls"))
            .unwrap()
            .lines()
            .map(|row| {
                let (phase, source) = row.split_once(' ').unwrap();
                (phase.to_owned(), source.to_owned())
            })
            .collect()
    }

    async fn authenticate(
        &self,
        cancellation: &InstallCancellation,
    ) -> Result<SteamWorkshopAccountAuthenticationResult, SteamCmdError> {
        let (sender, mut input) = mpsc::channel(2);
        let native = Path::new(&self.settings.steamcmd_root);
        let mode = fs::read_to_string(native.join("mode")).unwrap_or_default();
        let mut phases = Vec::new();
        let mut password_challenges = 0;
        let mut guard_challenges = 0;
        let mut first_guard_revision = None;
        let result = authenticate_workshop_account_with_progress(
            test_install_lock_context(),
            &self.settings,
            "selected_account",
            cancellation,
            &mut input,
            Duration::from_secs(20),
            |progress| {
                let phase = progress.phase;
                phases.push(phase);
                assert_eq!(
                    progress.challenge_revision.is_some(),
                    matches!(
                        phase,
                        SteamWorkshopAccountPhase::Password
                            | SteamWorkshopAccountPhase::GuardCode
                            | SteamWorkshopAccountPhase::EmailCode
                            | SteamWorkshopAccountPhase::SteamGuard
                    )
                );
                let value = match phase {
                    SteamWorkshopAccountPhase::GuardCode => {
                        guard_challenges += 1;
                        if mode == "mobile-cancel" {
                            cancellation.cancel();
                            return;
                        }
                        if mode == "mobile-approved" {
                            fs::write(native.join("mobile-approved"), []).unwrap();
                            return;
                        }
                        if guard_challenges == 1 {
                            first_guard_revision = progress.challenge_revision;
                        }
                        if mode == "mobile-retry-code" && guard_challenges == 2 {
                            assert!(
                                progress.challenge_revision.unwrap()
                                    > first_guard_revision.unwrap()
                            );
                            sender
                                .try_send(SteamWorkshopAccountInput {
                                    challenge_revision: first_guard_revision,
                                    value: "BAD77".into(),
                                })
                                .unwrap();
                        }
                        Some("7Q9XW")
                    }
                    SteamWorkshopAccountPhase::Password => {
                        password_challenges += 1;
                        if mode == "cancel" {
                            cancellation.cancel();
                            return;
                        }
                        assert!(
                            password_challenges == 1
                                || mode.starts_with("retry-password") && password_challenges == 2,
                            "a replacement native attempt must reuse the latest password only once"
                        );
                        Some(if password_challenges == 1 {
                            "fixture-password"
                        } else {
                            "fixture-server-password"
                        })
                    }
                    SteamWorkshopAccountPhase::EmailCode => Some("MAIL7"),
                    SteamWorkshopAccountPhase::SteamGuard => Some("7Q9XW"),
                    _ => None,
                };
                if let Some(value) = value {
                    sender
                        .try_send(SteamWorkshopAccountInput {
                            challenge_revision: progress.challenge_revision,
                            value: value.into(),
                        })
                        .unwrap();
                }
                if mode == "progress" && phase == SteamWorkshopAccountPhase::Connecting {
                    fs::write(native.join("connection-progress-seen"), []).unwrap();
                }
                if mode == "progress" && phase == SteamWorkshopAccountPhase::Authenticating {
                    fs::write(native.join("auth-progress-seen"), []).unwrap();
                }
            },
        )
        .await;
        assert!(!phases.contains(&SteamWorkshopAccountPhase::MobileConfirmation));
        if let Some(code) = phases
            .iter()
            .position(|phase| *phase == SteamWorkshopAccountPhase::GuardCode)
        {
            let password = phases
                .iter()
                .position(|phase| *phase == SteamWorkshopAccountPhase::Password)
                .unwrap();
            assert!(password < code);
        }
        if result.is_ok() {
            assert_eq!(
                password_challenges,
                if mode == "cached-success" {
                    0
                } else if mode.starts_with("retry-password") {
                    2
                } else {
                    1
                }
            );
            assert!(phases.contains(&SteamWorkshopAccountPhase::PreparingSteamcmd));
            assert!(phases.contains(&SteamWorkshopAccountPhase::VerifyingCache));
            if !mode.starts_with("mobile")
                && mode != "native-token"
                && mode != "retry-password-mobile"
            {
                assert_eq!(
                    guard_challenges, 0,
                    "native direct/email login must not ask for a speculative TOTP"
                );
            }
        }
        if mode == "native-email" {
            assert!(phases.contains(&SteamWorkshopAccountPhase::EmailCode));
        }
        if mode == "progress" {
            assert!(phases.contains(&SteamWorkshopAccountPhase::Connecting));
            assert!(phases.contains(&SteamWorkshopAccountPhase::Authenticating));
        }
        result
    }

    async fn assert_closed(&self, endpoint: &str) {
        let guard = acquire_steamcmd_operation(
            test_install_lock_context(),
            &self.settings,
            InstallDeadline::new("SteamCMD test lifecycle", GAME_INSTALL_LIFECYCLE_TIMEOUT),
        )
        .await
        .unwrap();
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(1))
            .build()
            .unwrap();
        assert!(
            client
                .get(format!("{endpoint}/steam_cmd_win64"))
                .send()
                .await
                .is_err()
        );
        drop(guard);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn deadline() -> InstallDeadline {
    InstallDeadline::new(
        "native authentication update bridge fixture",
        Duration::from_secs(30),
    )
}

#[tokio::test]
async fn managed_interactive_and_cached_authentication_share_one_verified_manifest_bridge() {
    let fixture = Fixture::new().await;
    let result = fixture
        .authenticate(&InstallCancellation::new())
        .await
        .unwrap();
    assert_eq!(result.account_name, "selected_account");
    let calls = fixture.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, "interactive");
    assert_eq!(calls[1].0, "cache");
    assert_eq!(calls[0].1, calls[1].1);
    assert!(calls[0].1.starts_with("http://127.0.0.1:"));
    fixture.assert_closed(&calls[0].1).await;
}

#[tokio::test]
async fn native_password_is_requested_only_when_needed_and_retained_for_a_code_restart() {
    for mode in [
        "cached-success",
        "cached-invalid-password",
        "retry-password",
        "progress",
    ] {
        let fixture = Fixture::new().await;
        fixture.mode(mode);
        fixture
            .authenticate(&InstallCancellation::new())
            .await
            .unwrap();
        let reads =
            fs::read_to_string(Path::new(&fixture.settings.steamcmd_root).join("password-reads"))
                .unwrap_or_default();
        assert_eq!(
            reads,
            match mode {
                "cached-success" => "",
                "retry-password" => "native\nretry\n",
                _ => "native\n",
            }
        );
        let calls = fixture.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].1, calls[1].1);
        fixture.assert_closed(&calls[0].1).await;
    }
}

#[tokio::test]
async fn authentication_failures_close_the_manifest_bridge_and_release_the_runtime_lease() {
    for mode in ["interactive-failure", "cache-failure"] {
        let fixture = Fixture::new().await;
        fixture.mode(mode);
        assert!(matches!(
            fixture.authenticate(&InstallCancellation::new()).await,
            Err(SteamCmdError::SteamAccountAuthenticationFailed { .. })
        ));
        let calls = fixture.calls();
        assert_eq!(
            calls.len(),
            if mode == "interactive-failure" { 1 } else { 2 }
        );
        if calls.len() == 2 {
            assert_eq!(calls[0].1, calls[1].1);
        }
        fixture.assert_closed(&calls[0].1).await;
    }
}

#[tokio::test]
async fn cancelling_an_interactive_challenge_closes_the_bridge_before_lock_reuse() {
    for mode in ["cancel", "mobile-cancel"] {
        let fixture = Fixture::new().await;
        fixture.mode(mode);
        assert!(matches!(
            fixture.authenticate(&InstallCancellation::new()).await,
            Err(SteamCmdError::InstallCancelled { .. })
        ));
        let calls = fixture.calls();
        assert_eq!(calls.len(), 1);
        fixture.assert_closed(&calls[0].1).await;
    }
}

#[tokio::test]
async fn explicitly_verified_external_authentication_keeps_native_bootstrap_arguments() {
    let fixture = Fixture::new().await;
    fs::remove_file(
        Path::new(&fixture.settings.steamcmd_root).join(".langame-steamcmd-owner.json"),
    )
    .unwrap();
    ensure_steamcmd_installed(test_install_lock_context(), &fixture.settings)
        .await
        .unwrap();
    assert_eq!(
        steamcmd_status(&fixture.settings).ownership,
        SteamCmdOwnership::External
    );
    fixture
        .authenticate(&InstallCancellation::new())
        .await
        .unwrap();
    assert_eq!(
        fixture.calls(),
        [
            ("interactive".into(), "plain".into()),
            ("cache".into(), "plain".into())
        ]
    );
}

#[tokio::test]
async fn native_authentication_selects_email_or_token_without_an_extra_verification_choice() {
    for mode in [
        "native-email",
        "native-token",
        "mobile",
        "mobile-cached",
        "mobile-approved",
        "mobile-retry-code",
        "retry-password-mobile",
    ] {
        let fixture = Fixture::new().await;
        fixture.mode(mode);
        fixture
            .authenticate(&InstallCancellation::new())
            .await
            .unwrap();
        let calls = fixture.calls();
        let restarted = mode.starts_with("mobile") && mode != "mobile-approved"
            || mode == "retry-password-mobile";
        assert_eq!(calls.len(), if restarted { 3 } else { 2 });
        assert!(calls.iter().all(|call| call.1 == calls[0].1));
        assert_eq!(
            fs::read_to_string(Path::new(&fixture.settings.steamcmd_root).join("attempts"))
                .unwrap()
                .lines()
                .count(),
            if restarted { 2 } else { 1 }
        );
        if mode == "retry-password-mobile" {
            assert_eq!(
                fs::read_to_string(
                    Path::new(&fixture.settings.steamcmd_root).join("password-reads")
                )
                .unwrap(),
                "native\nretry\nnative\n"
            );
        }
        fixture.assert_closed(&calls[0].1).await;
    }
}

#[tokio::test]
async fn code_phone_fallback_or_failed_login_prompt_is_a_safe_terminal_failure() {
    for mode in ["mobile-again", "return-prompt"] {
        let fixture = Fixture::new().await;
        fixture.mode(mode);
        let error = fixture
            .authenticate(&InstallCancellation::new())
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            SteamCmdError::SteamAccountAuthenticationFailed { .. }
        ));
        assert!(!error.to_string().contains("7Q9XW"));
        let calls = fixture.calls();
        assert_eq!(calls.len(), if mode == "mobile-again" { 2 } else { 1 });
        fixture.assert_closed(&calls[0].1).await;
    }
}
