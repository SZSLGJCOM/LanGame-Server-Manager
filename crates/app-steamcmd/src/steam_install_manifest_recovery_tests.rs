use super::*;

const INSTALLED: &str = r#""AppState" { "appid" "42" "StateFlags" "4" "buildid" "17" "InstalledDepots" { "43" { "manifest" "1" } } }"#;
const DENIAL: &str = "[2026-10-15 18:42:29] CDepotDownloadMgr::BYldRequestDepotManifest(App: 42, Depot: 43, Manifest: 1, branch: ): Failed to get manifest request code, 'Access Denied'\n";

async fn recovery_fixture(mode: &str) -> Fixture {
    recovery_fixture_for(mode, "retained-steam-fixture", 42, INSTALLED, DENIAL).await
}

async fn recovery_fixture_for(
    mode: &str,
    module_id: &str,
    app_id: u32,
    installed: &str,
    denials: &str,
) -> Fixture {
    let mut fixture = Fixture::new().await;
    fixture.module.summary.id = module_id.into();
    fixture.module.summary.steam_app_id = Some(app_id);
    let root = fixture.install_root();
    fs::remove_file(root.join(RETAINED_INSTALL_DATA_MARKER)).unwrap();
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::create_dir_all(root.join("steamapps")).unwrap();
    fs::write(root.join("bin/Server.exe"), b"existing server payload").unwrap();
    fs::write(
        root.join(format!("steamapps/appmanifest_{app_id}.acf")),
        installed,
    )
    .unwrap();
    let steamcmd = PathBuf::from(&fixture.settings.steamcmd_root);
    fs::create_dir_all(steamcmd.join("logs")).unwrap();
    fs::write(
        steamcmd.join("logs/content_log.txt"),
        if mode == "history" { denials } else { "" },
    )
    .unwrap();
    fs::write(steamcmd.join("recovery-mode"), mode).unwrap();
    fs::write(steamcmd.join("recovery-app"), app_id.to_string()).unwrap();
    fs::write(steamcmd.join("recovery-denials"), denials).unwrap();
    fs::write(
        steamcmd.join("recovery-failed-acf"),
        installed.replace("\"StateFlags\" \"4\"", "\"StateFlags\" \"6\""),
    )
    .unwrap();
    fs::write(
        steamcmd.join("recovery-complete-acf"),
        installed
            .replace("\"buildid\" \"17\"", "\"buildid\" \"18\"")
            .replace("\"manifest\" \"1\"", "\"manifest\" \"9\""),
    )
    .unwrap();
    let executable = steamcmd.join("steamcmd.exe");
    fs::remove_file(&executable).unwrap();
    // Only the external native installer is simulated. The production command,
    // log evidence, ACF reader, backup, retry, revision and verification all run.
    let source = r#"
using System;
using System.IO;
public static class ManifestRecoveryInstaller {
  public static int Main(string[] args) {
    if (args.Length == 1 && args[0] == "+quit") {
      Console.WriteLine("Steam Console Client (c) Valve Corporation\nLoading Steam API...OK");
      return 0;
    }
    if (args.Length != 2 || args[0] != "+runscript") return 80;
    string root = null, update = null;
    foreach (string line in File.ReadAllLines(args[1])) {
      if (line.StartsWith("force_install_dir ")) root = line.Substring(18).Trim().Trim('"');
      if (line.StartsWith("app_update ")) update = line.Trim();
    }
    if (root == null || update == null) return 81;
    int calls = File.Exists("recovery-calls") ? Int32.Parse(File.ReadAllText("recovery-calls")) : 0;
    calls++;
    File.WriteAllText("recovery-calls", calls.ToString());
    File.AppendAllText("recovery-commands", update + "\n");
    File.AppendAllText("recovery-destinations", root + "\n");
    string mode = File.ReadAllText("recovery-mode").Trim();
    string app = File.ReadAllText("recovery-app").Trim();
    string manifest = Path.Combine(root, "steamapps/appmanifest_" + app + ".acf");
    string deny = File.ReadAllText("recovery-denials");
    if (calls == 1) {
      if (mode == "wrongApp") deny = deny.Replace("App: " + app, "App: " + (UInt32.Parse(app) - 1));
      if (mode == "newManifest") deny = deny.Replace("Manifest: 1", "Manifest: 9");
      File.AppendAllText("logs/content_log.txt", mode == "history" ? "new unrelated connection failure\n" : deny);
      File.WriteAllText(manifest, File.ReadAllText("recovery-failed-acf"));
      Console.WriteLine("ERROR! App '" + app + "' state is 0x6 after update job. No connection.");
      return 8;
    }
    if (calls != 2 || update != "app_update " + app + " validate" || File.Exists(manifest)) return 82;
    if (mode == "cancel") {
      Console.WriteLine("Waiting for client config...");
      Console.Out.Flush();
      new System.Threading.ManualResetEvent(false).WaitOne();
    }
    if (mode == "fail") {
      File.AppendAllText("logs/content_log.txt", deny);
      Console.WriteLine("ERROR! App '" + app + "' state is 0x6 after update job. No connection.");
      return 8;
    }
    File.WriteAllText(manifest, File.ReadAllText("recovery-complete-acf"));
    Console.WriteLine("Success! App '" + app + "' fully installed.");
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
    ensure_steamcmd_installed(crate::test_install_lock_context(), &fixture.settings)
        .await
        .unwrap();
    fixture
}

fn calls(fixture: &Fixture) -> usize {
    fs::read_to_string(PathBuf::from(&fixture.settings.steamcmd_root).join("recovery-calls"))
        .unwrap()
        .parse()
        .unwrap()
}

fn backups(fixture: &Fixture) -> Vec<PathBuf> {
    fs::read_dir(fixture.install_root().join("steamapps"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect()
}

#[tokio::test]
async fn native_manifest_recovery_validates_once_and_preserves_data_and_revision() {
    let fixture = recovery_fixture("recover").await;
    let mut progress = Vec::new();
    let result = fixture
        .install(|update| progress.push(update.detail))
        .await
        .unwrap();
    assert_eq!(result.install_state, InstallState::Installed);
    assert_eq!(calls(&fixture), 2);
    let steamcmd = PathBuf::from(&fixture.settings.steamcmd_root);
    assert_eq!(
        fs::read_to_string(steamcmd.join("recovery-commands")).unwrap(),
        "app_update 42\napp_update 42 validate\n"
    );
    let destinations = fs::read_to_string(steamcmd.join("recovery-destinations")).unwrap();
    assert!(
        destinations
            .lines()
            .all(|path| Path::new(path) == fixture.install_root())
    );
    let backups = backups(&fixture);
    assert_eq!(backups.len(), 1);
    assert_eq!(
        fs::read_to_string(backups[0].join("before-attempt.acf")).unwrap(),
        INSTALLED
    );
    assert!(backups[0].join("failed-attempt.acf").is_file());
    fixture.assert_retained(false);
    assert!(
        progress
            .iter()
            .any(|detail| detail.contains("Preserved Steam manifests at"))
    );
    assert!(
        progress
            .iter()
            .any(|detail| detail.contains("recovery validation completed"))
    );
    assert_eq!(
        read_game_install_revision(
            Path::new(&fixture.settings.servers_root),
            &fixture.module.summary.id
        )
        .unwrap(),
        1
    );
}

#[tokio::test]
async fn catalog_apps_require_a_prior_acf_and_recover_a_nonfirst_depot_once() {
    // Use the real catalog identities with synthetic depots and payloads. Only
    // SteamCMD is replaced; production matching, backup, retry and verification run.
    let cases: [(&str, u32); 8] = [
        ("valheim", 896660),
        ("runescapedragonwilds", 4019830),
        ("arksurvivalascended", 2430930),
        ("corekeeper", 1963720),
        ("dontstarve", 343050),
        ("necesse", 1169370),
        ("palworld", 2394010),
        ("astroneer", 728470),
    ];
    for (module_id, app_id) in cases {
        let first_depot = u64::from(app_id) * 10 + 1;
        let denied_depot = first_depot + 1;
        let installed = format!(
            r#""AppState" {{ "appid" "{app_id}" "StateFlags" "4" "buildid" "17" "InstalledDepots" {{ "{first_depot}" {{ "manifest" "2" }} "{denied_depot}" {{ "manifest" "1" }} }} }}"#
        );
        let denial = |app, depot, manifest| {
            format!(
                "[2026-10-15 18:42:29] CDepotDownloadMgr::BYldRequestDepotManifest(App: {app}, Depot: {depot}, Manifest: {manifest}, branch: ): Failed to get manifest request code, 'Access Denied'\n"
            )
        };
        let denials = [
            denial(app_id + 1, denied_depot, 1),
            denial(app_id, denied_depot + 1, 1),
            denial(app_id, first_depot, 9),
            denial(app_id, denied_depot, 9),
            denial(app_id, denied_depot, 1),
        ]
        .concat();
        let fixture =
            recovery_fixture_for("recover", module_id, app_id, &installed, &denials).await;
        let root = fixture.install_root();
        let manifest = root.join(format!("steamapps/appmanifest_{app_id}.acf"));
        let steamcmd = PathBuf::from(&fixture.settings.steamcmd_root);

        // A failed first acquisition may create an ACF, but it is not evidence
        // of an installed old manifest captured before that invocation.
        fs::remove_file(&manifest).unwrap();
        let mut ready = false;
        let result = fixture
            .install(|update| {
                ready |= update
                    .install_progress
                    .is_some_and(|progress| progress.phase == InstallPhase::Ready);
            })
            .await;
        assert!(
            matches!(result, Err(SteamCmdError::SteamCmdCommandFailed { .. })),
            "{module_id}: missing prior ACF must not recover"
        );
        assert!(!ready, "{module_id}");
        assert_eq!(calls(&fixture), 1, "{module_id}");
        assert!(backups(&fixture).is_empty(), "{module_id}");
        assert_eq!(
            fs::read(&manifest).unwrap(),
            fs::read(steamcmd.join("recovery-failed-acf")).unwrap(),
            "{module_id}: the failed attempt's ACF must remain untouched"
        );
        fixture.assert_retained(false);

        // Start a separate invocation with a real pre-existing two-depot ACF.
        fs::write(&manifest, &installed).unwrap();
        fs::write(steamcmd.join("recovery-calls"), "0").unwrap();
        for name in [
            "recovery-commands",
            "recovery-destinations",
            "logs/content_log.txt",
        ] {
            fs::write(steamcmd.join(name), "").unwrap();
        }
        let mut progress = Vec::new();
        let result = fixture
            .install(|update| progress.push(update.detail))
            .await
            .unwrap_or_else(|error| panic!("{module_id}: {error}"));
        assert_eq!(result.module_id, module_id);
        assert_eq!(result.steam_app_id, app_id);
        assert_eq!(result.install_state, InstallState::Installed, "{module_id}");
        assert_eq!(calls(&fixture), 2, "{module_id}");
        assert_eq!(
            fs::read_to_string(steamcmd.join("recovery-commands")).unwrap(),
            format!("app_update {app_id}\napp_update {app_id} validate\n"),
            "{module_id}"
        );
        let backups = backups(&fixture);
        assert_eq!(backups.len(), 1, "{module_id}");
        assert_eq!(
            fs::read_to_string(backups[0].join("before-attempt.acf")).unwrap(),
            installed,
            "{module_id}"
        );
        assert_eq!(
            fs::read(backups[0].join("failed-attempt.acf")).unwrap(),
            fs::read(steamcmd.join("recovery-failed-acf")).unwrap(),
            "{module_id}"
        );
        assert_eq!(
            fs::read(&manifest).unwrap(),
            fs::read(steamcmd.join("recovery-complete-acf")).unwrap(),
            "{module_id}"
        );
        let recovery =
            format!("Steam denied installed manifest 1 for AppID {app_id}, depot {denied_depot}.");
        for detail in [
            recovery.as_str(),
            "Preserved Steam manifests at",
            "recovery validation completed",
        ] {
            assert_eq!(
                progress.iter().filter(|line| line.contains(detail)).count(),
                1,
                "{module_id}: {detail}"
            );
        }
        let destinations = fs::read_to_string(steamcmd.join("recovery-destinations")).unwrap();
        assert!(destinations.lines().all(|path| Path::new(path) == root));
        assert!(
            read_game_install_revision(Path::new(&fixture.settings.servers_root), module_id)
                .is_ok(),
            "{module_id}: successful recovery must finish the pending revision"
        );
        fixture.assert_retained(false);
    }
}

#[tokio::test]
async fn historical_or_unrelated_native_denials_never_reset_the_manifest() {
    for mode in ["history", "wrongApp", "newManifest"] {
        let fixture = recovery_fixture(mode).await;
        assert!(
            matches!(
                fixture.install(|_| {}).await,
                Err(SteamCmdError::SteamCmdCommandFailed { .. })
            ),
            "{mode}"
        );
        assert_eq!(calls(&fixture), 1, "{mode}");
        assert!(backups(&fixture).is_empty(), "{mode}");
        fixture.assert_retained(false);
    }
}

#[tokio::test]
async fn failed_recovery_preserves_backups_and_pending_revision_without_third_attempt() {
    let fixture = recovery_fixture("fail").await;
    let mut ready = false;
    let error = fixture
        .install(|update| {
            ready |= update
                .install_progress
                .is_some_and(|progress| progress.phase == InstallPhase::Ready);
        })
        .await
        .unwrap_err();
    assert!(
        matches!(&error, SteamCmdError::SteamCmdCommandFailed { output_excerpt } if output_excerpt.contains("preserved manifests:"))
    );
    assert!(!ready);
    assert_eq!(calls(&fixture), 2);
    assert_eq!(backups(&fixture).len(), 1);
    assert!(
        read_game_install_revision(
            Path::new(&fixture.settings.servers_root),
            &fixture.module.summary.id
        )
        .is_err()
    );
    fixture.assert_retained(false);
}

#[tokio::test]
async fn cancelling_recovery_reaps_native_process_and_retains_original_manifest() {
    let fixture = recovery_fixture("cancel").await;
    let cancellation = InstallCancellation::new();
    let result = install_or_update_module_with_progress_and_cancellation(
        crate::test_install_lock_context(),
        &fixture.settings,
        &fixture.module,
        false,
        &cancellation,
        |update| {
            if update.detail.contains("Waiting for client config") {
                cancellation.cancel();
            }
        },
    )
    .await;
    assert!(matches!(
        result,
        Err(SteamCmdError::InstallCancelled { .. })
    ));
    assert_eq!(calls(&fixture), 2);
    let backups = backups(&fixture);
    assert_eq!(backups.len(), 1);
    assert_eq!(
        fs::read_to_string(backups[0].join("before-attempt.acf")).unwrap(),
        INSTALLED
    );
    fixture.assert_retained(false);
}
