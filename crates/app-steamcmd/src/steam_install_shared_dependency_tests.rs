use super::*;

async fn dependency_fixture(mode: &str) -> Fixture {
    let fixture = Fixture::new().await;
    let executable = PathBuf::from(&fixture.settings.steamcmd_root).join("steamcmd.exe");
    fs::remove_file(&executable).unwrap();
    fs::write(
        PathBuf::from(&fixture.settings.steamcmd_root).join("dependency-mode"),
        mode,
    )
    .unwrap();
    // This external boundary writes real ACFs and records every native script.
    // It never contacts Steam or runs software downloaded from the host.
    let source = r#"
using System;
using System.IO;
public static class SharedDependencyInstaller {
  public static int Main(string[] args) {
    if (args.Length == 1 && args[0] == "+quit") {
      Console.WriteLine("Steam Console Client (c) Valve Corporation\nLoading Steam API...OK");
      return 0;
    }
    if (args.Length != 2 || args[0] != "+runscript") return 80;
    string root = null, update = null, login = null;
    foreach (string line in File.ReadAllLines(args[1])) {
      if (line.StartsWith("force_install_dir ")) root = line.Substring(18).Trim().Trim('"');
      if (line.StartsWith("app_update ")) update = line.Trim();
      if (line.StartsWith("login ")) login = line.Trim();
    }
    if (root == null || update == null) return 81;
    int calls = File.Exists("dependency-calls") ? Int32.Parse(File.ReadAllText("dependency-calls")) : 0;
    calls++;
    File.WriteAllText("dependency-calls", calls.ToString());
    File.AppendAllText("dependency-commands", update + "\n");
    File.AppendAllText("dependency-logins", login + "\n");
    File.AppendAllText("dependency-destinations", root + "\n");
    if (calls > 1 && update != "app_update 42 validate") return 82;
    string mode = File.ReadAllText("dependency-mode").Trim();
    if (mode == "accountRepair" || mode == "wrongRepairAccount") {
      if (login != "login selected_account") return 83;
      string name = mode == "wrongRepairAccount" && calls > 1 ? "other_account" : "selected_account";
      Console.WriteLine("Logging in user '" + name + "' to Steam Public...OK");
      Console.WriteLine("Waiting for user info...OK");
    }
    Directory.CreateDirectory(Path.Combine(root, "bin"));
    Directory.CreateDirectory(Path.Combine(root, "ServerConfig"));
    Directory.CreateDirectory(Path.Combine(root, "steamapps"));
    File.WriteAllText(Path.Combine(root, "bin/Server.exe"), "server payload");
    File.WriteAllText(Path.Combine(root, "ServerConfig/Server.cfg"), "depot defaults");
    string shared = mode == "none" ? "" : "\"SharedDepots\" { \"1004\" \"1007\" \"1008\" \"1007\" }";
    string main = "\"AppState\" { \"appid\" \"42\" \"StateFlags\" \"4\" \"buildid\" \"17\" \"TargetBuildID\" \"17\" \"InstalledDepots\" { \"42\" { \"manifest\" \"9\" } } " + shared + " }";
    File.WriteAllText(Path.Combine(root, "steamapps/appmanifest_42.acf"), main);
    if (mode == "cmdFail" && calls > 1) {
      Console.WriteLine("ERROR! Shared dependency validation failed.");
      return 9;
    }
    if (mode == "cancelDuringRepair" && calls > 1) {
      Console.WriteLine("Waiting for client config...");
      Console.Out.Flush();
      new System.Threading.ManualResetEvent(false).WaitOne();
    }
    if (mode != "none" && (mode != "missing" || calls > 1)) {
      bool complete = mode == "complete" || (calls > 1 && mode != "remainsIncomplete");
      string flags = complete ? "4" : "2";
      string owner = "\"AppState\" { \"appid\" \"1007\" \"StateFlags\" \"" + flags + "\" \"buildid\" \"23\" \"InstalledDepots\" { \"1004\" { \"manifest\" \"10\" } \"1008\" { \"manifest\" \"11\" } } }";
      File.WriteAllText(Path.Combine(root, "steamapps/appmanifest_1007.acf"), owner);
    }
    Console.WriteLine("Success! App '42' fully installed.");
    return 0;
  }
}
"#;
    let script = format!(
        "$ErrorActionPreference='Stop'; Add-Type -TypeDefinition @'\n{source}\n'@ -OutputAssembly {} -OutputType ConsoleApplication",
        ps_literal(&executable)
    );
    let output = run_powershell(&script, Some(&fixture.root), test_deadline())
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        output_excerpt(&output.stdout, &output.stderr)
    );
    let mut preparation = std::collections::VecDeque::new();
    let result = crate::ensure_steamcmd_installed_with_progress(
        crate::test_install_lock_context(),
        &fixture.settings,
        |progress| {
            if preparation.len() == 32 {
                preparation.pop_front();
            }
            preparation.push_back(format!(
                "{:?}: {}\n{}",
                progress.phase, progress.detail, progress.output_excerpt
            ));
        },
    )
    .await;
    assert!(
        result.is_ok(),
        "Shared dependency fixture preparation failed: mode={mode}, executable={}, status={:?}, result={result:?}\n{}",
        executable.display(),
        steamcmd_status(&fixture.settings),
        preparation.into_iter().collect::<Vec<_>>().join("\n")
    );
    fixture
}

fn native_calls(fixture: &Fixture) -> usize {
    fs::read_to_string(PathBuf::from(&fixture.settings.steamcmd_root).join("dependency-calls"))
        .unwrap()
        .parse()
        .unwrap()
}

fn native_records(fixture: &Fixture, name: &str) -> Vec<String> {
    fs::read_to_string(PathBuf::from(&fixture.settings.steamcmd_root).join(name))
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[derive(Clone, Copy, Debug)]
enum CancellationPoint {
    None,
    BeforeRepair,
    DuringRepair,
}

async fn install_with_dependencies(
    fixture: &Fixture,
    validate: bool,
    cancellation_point: CancellationPoint,
) -> (Result<ModuleInstallResult, SteamCmdError>, usize, bool) {
    install_with_dependencies_and_account(fixture, validate, cancellation_point, None).await
}

async fn install_with_dependencies_and_account(
    fixture: &Fixture,
    validate: bool,
    cancellation_point: CancellationPoint,
    account_name: Option<&str>,
) -> (Result<ModuleInstallResult, SteamCmdError>, usize, bool) {
    let target = fixture.install_root();
    let guard = acquire_game_install_lifecycle(
        crate::test_install_lock_context(),
        &fixture.module.summary.id,
        std::slice::from_ref(&target),
    )
    .await
    .unwrap();
    let cancellation = InstallCancellation::new();
    let mut callbacks = 0;
    let mut ready = false;
    let result = install_or_update_module_at_with_login_and_callbacks(
        SteamModuleInstallRequest {
            settings: &fixture.settings,
            module: &fixture.module,
            install_root: &target,
            guard: &guard,
            validate,
            cancellation: &cancellation,
            account_name,
        },
        ModuleInstallCallbacks {
            on_progress: |update: InstallProgressUpdate| {
                ready |= update
                    .install_progress
                    .is_some_and(|progress| progress.phase == InstallPhase::Ready);
                match cancellation_point {
                    CancellationPoint::BeforeRepair
                        if update
                            .detail
                            .starts_with("Completing Steam shared dependencies") =>
                    {
                        cancellation.cancel();
                    }
                    CancellationPoint::DuringRepair
                        if update.detail.contains("Waiting for client config") =>
                    {
                        cancellation.cancel();
                    }
                    _ => {}
                }
            },
            prepare_fresh_payload: |stage: PathBuf| {
                callbacks += 1;
                assert!(!stage.join("world.sav").exists());
                let app = crate::steam_depot::manifest::parse_acf(
                    &fs::read(stage.join("steamapps/appmanifest_42.acf")).unwrap(),
                    42,
                )
                .unwrap();
                if app.contains_key("shareddepots") {
                    crate::steam_depot::manifest::parse_acf(
                        &fs::read(stage.join("steamapps/appmanifest_1007.acf")).unwrap(),
                        1007,
                    )
                    .expect("shared dependency must be complete before baseline callback");
                }
                std::future::ready(Ok(()))
            },
        },
    )
    .await;
    (result, callbacks, ready)
}

#[tokio::test]
async fn shared_dependency_repair_reuses_and_reverifies_the_selected_server_account() {
    for mode in ["accountRepair", "wrongRepairAccount"] {
        let fixture = dependency_fixture(mode).await;
        let (result, callbacks, ready) = install_with_dependencies_and_account(
            &fixture,
            false,
            CancellationPoint::None,
            Some("selected_account"),
        )
        .await;
        assert_eq!(native_calls(&fixture), 2);
        assert_eq!(
            native_records(&fixture, "dependency-logins"),
            ["login selected_account", "login selected_account"]
        );
        if mode == "accountRepair" {
            result.unwrap();
            assert_eq!(callbacks, 1);
            assert!(ready);
            fixture.assert_retained(false);
        } else {
            assert!(matches!(
                result,
                Err(SteamCmdError::SteamAccountAuthenticationFailed { .. })
            ));
            assert_eq!(callbacks, 0);
            assert!(!ready);
            fixture.assert_retained(true);
        }
        fixture.assert_clean().await;
    }
}

#[tokio::test]
async fn complete_or_absent_shared_dependencies_need_only_the_original_native_command() {
    for mode in ["complete", "none"] {
        let fixture = dependency_fixture(mode).await;
        let (result, callbacks, ready) =
            install_with_dependencies(&fixture, false, CancellationPoint::None).await;
        result.unwrap();
        assert_eq!(native_calls(&fixture), 1, "{mode}");
        assert_eq!(callbacks, 1);
        assert!(ready);
        fixture.assert_retained(false);
        fixture.assert_clean().await;
    }
}

#[tokio::test]
async fn incomplete_or_missing_shared_owner_is_validated_once_before_publication() {
    for (mode, initial_validate) in [
        ("incomplete", false),
        ("missing", false),
        ("incomplete", true),
    ] {
        let fixture = dependency_fixture(mode).await;
        let (result, callbacks, ready) =
            install_with_dependencies(&fixture, initial_validate, CancellationPoint::None).await;
        result.unwrap();
        assert_eq!(
            native_calls(&fixture),
            2,
            "{mode}, validate={initial_validate}"
        );
        let commands = native_records(&fixture, "dependency-commands");
        assert_eq!(
            commands,
            [
                if initial_validate {
                    "app_update 42 validate"
                } else {
                    "app_update 42"
                },
                "app_update 42 validate",
            ]
        );
        let roots = native_records(&fixture, "dependency-destinations");
        assert_eq!(roots.len(), 2);
        assert_eq!(
            roots[0], roots[1],
            "dependency repair must use the same staged root"
        );
        assert_ne!(PathBuf::from(&roots[0]), fixture.install_root());
        assert_eq!(callbacks, 1);
        assert!(ready);
        fixture.assert_retained(false);
        fixture.assert_clean().await;
    }
}

#[tokio::test]
async fn unsuccessful_shared_dependency_repair_cannot_publish_or_report_ready() {
    for mode in ["remainsIncomplete", "cmdFail"] {
        let fixture = dependency_fixture(mode).await;
        let (result, callbacks, ready) =
            install_with_dependencies(&fixture, false, CancellationPoint::None).await;
        assert!(result.is_err(), "{mode}");
        assert_eq!(native_calls(&fixture), 2, "repair must be bounded: {mode}");
        assert_eq!(callbacks, 0);
        assert!(!ready);
        fixture.assert_retained(true);
        assert!(!fixture.install_root().join("bin/Server.exe").exists());
        fixture.assert_clean().await;
    }
}

#[tokio::test]
async fn cancellation_before_or_during_shared_dependency_repair_preserves_retained_data() {
    for (mode, cancellation_point, calls) in [
        ("incomplete", CancellationPoint::BeforeRepair, 1),
        ("cancelDuringRepair", CancellationPoint::DuringRepair, 2),
    ] {
        let fixture = dependency_fixture(mode).await;
        let (result, callbacks, ready) =
            install_with_dependencies(&fixture, false, cancellation_point).await;
        assert!(matches!(
            result,
            Err(SteamCmdError::InstallCancelled { .. })
        ));
        assert_eq!(native_calls(&fixture), calls, "{cancellation_point:?}");
        assert_eq!(callbacks, 0);
        assert!(!ready);
        fixture.assert_retained(true);
        assert!(!fixture.install_root().join("bin/Server.exe").exists());
        fixture.assert_clean().await;
    }
}
