use super::*;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

use windows_sys::Win32::Foundation::{ERROR_INVALID_PARAMETER, STILL_ACTIVE};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};

const APP_ID: u32 = 42;
const ITEM_ID: &str = "123456";
const READY_RECORD: &str = ".langame-steamcmd-ready.json";

struct Fixture {
    root: PathBuf,
    settings: AppSettings,
    target: PathBuf,
}

impl Fixture {
    async fn new() -> Self {
        let root = crate::tests::unique_test_root();
        let native = root.join("steamcmd");
        let target = root.join("game");
        let settings = AppSettings {
            steamcmd_root: native.to_string_lossy().into_owned(),
            games_root: root.join("games").to_string_lossy().into_owned(),
            servers_root: root.join("servers").to_string_lossy().into_owned(),
            ..AppSettings::default()
        };
        prepare_configured_steamcmd_root(&native).unwrap();
        // Replace only the external native executable. Real scripts, cache
        // inspection, lifecycle leases, streaming and process ownership run.
        let source = r#"
using System;
using System.Diagnostics;
using System.IO;
using System.Threading;
public static class WorkshopCancellationFixture {
  private static void Runtime() {
    Console.WriteLine("Steam Console Client (c) Valve Corporation - fixture");
    Console.WriteLine("Loading Steam API...OK");
  }
  private static void Stall() {
    File.WriteAllText("parent-pid", Process.GetCurrentProcess().Id.ToString());
    var child = new ProcessStartInfo(Process.GetCurrentProcess().MainModule.FileName, "--child");
    child.UseShellExecute = false;
    child.CreateNoWindow = true;
    Process.Start(child);
    while (!File.Exists("child-pid")) Thread.Sleep(10);
    Console.WriteLine("Waiting for client config... fixture running");
    Console.Out.Flush();
    new ManualResetEvent(false).WaitOne();
  }
  public static int Main(string[] args) {
    if (args.Length == 1 && args[0] == "--child") {
      File.WriteAllText("child-pid", Process.GetCurrentProcess().Id.ToString());
      new ManualResetEvent(false).WaitOne();
      return 0;
    }
    Runtime();
    if (args.Length == 1 && args[0] == "+quit") {
      File.AppendAllText("native-calls", "verify\n");
      if (File.Exists("verify-stall")) Stall();
      return 0;
    }
    int scriptIndex = Array.IndexOf(args, "+runscript");
    if (scriptIndex < 0 || scriptIndex + 1 >= args.Length) return 80;
    string script = File.ReadAllText(args[scriptIndex + 1]);
    File.WriteAllText("captured-script", script);
    File.WriteAllText("captured-script-path", args[scriptIndex + 1]);
    File.AppendAllText("native-calls", "workshop\n");
    if (!script.Contains("workshop_download_item 42 123456 validate")) return 81;
    if (File.Exists("download-stall")) Stall();
    if (File.Exists("download-fail")) {
      Console.WriteLine("ERROR! Download item 123456 failed (Failure).");
      return 0;
    }
    if (File.Exists("omit-item-success")) return 0;
    string root = File.ReadAllText("install-root");
    if (File.Exists("download-alternate")) root = Environment.CurrentDirectory;
    string workshop = Path.Combine(root, "steamapps", "workshop");
    string payload = Path.Combine(workshop, "content", "42", "123456", "payload.bin");
    Directory.CreateDirectory(Path.GetDirectoryName(payload));
    File.WriteAllText(payload, File.Exists("incomplete-download") ? "l" : "latest");
    File.WriteAllText(Path.Combine(workshop, "appworkshop_42.acf"),
      "\"AppWorkshop\" { \"appid\" \"42\" \"WorkshopItemsInstalled\" { \"123456\" { \"manifest\" \"2\" \"size\" \"6\" } } }");
    if (File.Exists("omit-download-path")) Console.WriteLine("Success. Downloaded item 123456 to cache");
    else Console.WriteLine("Success. Downloaded item 123456 to \"" + Path.GetDirectoryName(payload) + "\" (6 bytes)");
    for (int index = 0; index < 100; index++) Console.WriteLine("later unrelated diagnostic " + index);
    return 0;
  }
}
"#;
        let executable = native.join("steamcmd.exe");
        let compiled = run_powershell(
            &format!(
                "$ErrorActionPreference='Stop'; Add-Type -TypeDefinition @'\n{source}\n'@ -OutputAssembly {} -OutputType ConsoleApplication",
                ps_literal(&executable),
            ),
            Some(&native),
            deadline(),
        )
        .await
        .unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            output_excerpt(&compiled.stdout, &compiled.stderr)
        );
        fs::create_dir(native.join("package")).unwrap();
        fs::write(
            native.join("package/steam_cmd_win64.manifest"),
            format!(
                "\"win64\" {{ \"version\" \"123\" \"steamcmd_win64\" {{ \"file\" \"steamcmd_win64.zip.hash\" \"size\" \"12\" \"sha2\" \"{}\" \"IsBootstrapperPackage\" \"1\" }} }}",
                "a".repeat(64),
            ),
        )
        .unwrap();
        // Establish the fixture's advisory runtime record only after its native
        // +quit entry has exited with both console and Steam API markers.
        let mut command = Command::new(&executable);
        apply_no_window(&mut command);
        command.arg("+quit").current_dir(&native);
        let verified = run_command_capture(command, deadline()).await.unwrap();
        assert!(verified.status.success());
        let output = String::from_utf8(verified.stdout).unwrap();
        assert!(output.contains("Steam Console Client"));
        assert!(output.contains("Loading Steam API...OK"));
        steamcmd_readiness::record_verified(&managed_steamcmd_status(&settings)).unwrap();
        fs::write(native.join("install-root"), target.to_str().unwrap()).unwrap();
        let fixture = Self {
            root,
            settings,
            target,
        };
        fs::create_dir_all(fixture.payload().parent().unwrap()).unwrap();
        fs::write(fixture.payload(), "cached").unwrap();
        fs::write(
            fixture.target.join("steamapps/workshop/appworkshop_42.acf"),
            "\"AppWorkshop\" { \"appid\" \"42\" \"WorkshopItemsInstalled\" { \"123456\" { \"manifest\" \"1\" \"size\" \"6\" } } }",
        )
        .unwrap();
        fixture
    }

    fn native(&self) -> &Path {
        Path::new(&self.settings.steamcmd_root)
    }

    fn payload(&self) -> PathBuf {
        self.target
            .join("steamapps/workshop/content/42/123456/payload.bin")
    }

    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.native().join("native-calls"))
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    async fn download<F: FnMut(InstallProgressUpdate)>(
        &self,
        cancellation: &InstallCancellation,
        on_progress: F,
    ) -> Result<SteamWorkshopDownloadResult, SteamCmdError> {
        let game = acquire_game_install_lifecycle(
            test_install_lock_context(),
            "workshop-cancellation-fixture",
            std::slice::from_ref(&self.target),
        )
        .await?;
        let steamcmd = game.acquire_steamcmd(&self.settings).await?;
        download_workshop_items_with_login_and_cancellation_and_progress(
            &self.settings,
            SteamWorkshopDownloadRequest {
                consumer_app_id: APP_ID,
                install_root: &self.target,
                ids: &[ITEM_ID.into()],
                account_name: None,
            },
            &game,
            &steamcmd,
            cancellation,
            on_progress,
        )
        .await
    }

    fn assert_owned_processes_exited(&self) {
        for name in ["parent-pid", "child-pid"] {
            let pid: u32 = fs::read_to_string(self.native().join(name))
                .unwrap()
                .parse()
                .unwrap();
            let raw = unsafe {
                // SAFETY: the PID was recorded by this fixture's owned process.
                OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid)
            };
            if raw.is_null() {
                assert_eq!(
                    std::io::Error::last_os_error().raw_os_error(),
                    Some(ERROR_INVALID_PARAMETER as i32)
                );
                continue;
            }
            let process = unsafe {
                // SAFETY: OpenProcess returned an owned process handle.
                OwnedHandle::from_raw_handle(raw)
            };
            let mut exit_code = 0;
            let queried = unsafe {
                // SAFETY: both handle and exit-code buffer remain valid.
                GetExitCodeProcess(process.as_raw_handle().cast(), &mut exit_code)
            };
            assert_ne!(queried, 0);
            assert_ne!(
                exit_code, STILL_ACTIVE as u32,
                "{name} survived cancellation"
            );
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn deadline() -> InstallDeadline {
    InstallDeadline::new("Workshop cancellation fixture", Duration::from_secs(30))
}

#[tokio::test]
async fn complete_cached_item_is_still_validated_by_native_steamcmd() {
    let fixture = Fixture::new().await;
    let cached = inspect_workshop_items(
        &fixture.settings,
        APP_ID,
        &fixture.target,
        &[ITEM_ID.into()],
    )
    .unwrap();
    assert!(cached.items[0].installed);
    let result = fixture
        .download(&InstallCancellation::new(), |_| {})
        .await
        .unwrap();
    assert!(result.items[0].expected_path_exists);
    assert_eq!(fixture.calls(), ["verify", "workshop"]);
    assert_eq!(fs::read_to_string(fixture.payload()).unwrap(), "latest");
    let script = fs::read_to_string(fixture.native().join("captured-script")).unwrap();
    assert_eq!(
        script
            .lines()
            .filter(|line| line.starts_with("workshop_download_item"))
            .collect::<Vec<_>>(),
        ["workshop_download_item 42 123456 validate"]
    );
    let path = fs::read_to_string(fixture.native().join("captured-script-path")).unwrap();
    assert!(!Path::new(&path).exists(), "operation script is cleaned up");
}

#[tokio::test]
async fn cached_item_does_not_hide_native_update_failure_even_with_zero_exit() {
    let fixture = Fixture::new().await;
    fs::write(fixture.native().join("download-fail"), "").unwrap();
    let result = fixture.download(&InstallCancellation::new(), |_| {}).await;
    assert!(matches!(
        result,
        Err(SteamCmdError::SteamCmdCommandFailed { .. })
    ));
    assert_eq!(fixture.calls(), ["verify", "workshop"]);
    assert_eq!(fs::read_to_string(fixture.payload()).unwrap(), "cached");
}

#[tokio::test]
async fn zero_exit_with_only_runtime_markers_does_not_confirm_cached_item_is_current() {
    let fixture = Fixture::new().await;
    fs::write(fixture.native().join("omit-item-success"), "").unwrap();
    let result = fixture.download(&InstallCancellation::new(), |_| {}).await;
    let Err(SteamCmdError::SteamCmdCommandFailed { output_excerpt }) = result else {
        panic!("a successful process exit cannot establish item freshness");
    };
    assert!(output_excerpt.contains("did not confirm Workshop item 123456"));
    assert_eq!(fixture.calls(), ["verify", "workshop"]);
    assert_eq!(fs::read_to_string(fixture.payload()).unwrap(), "cached");
}

#[tokio::test]
async fn native_success_path_selects_fresh_alternate_over_complete_old_primary() {
    let fixture = Fixture::new().await;
    fs::write(fixture.native().join("download-alternate"), "").unwrap();
    let result = fixture
        .download(&InstallCancellation::new(), |_| {})
        .await
        .unwrap();
    let latest_root = fixture.native().join("steamapps/workshop/content/42");
    assert_eq!(Path::new(&result.workshop_root), latest_root);
    assert_eq!(
        Path::new(&result.items[0].expected_path),
        latest_root.join(ITEM_ID)
    );
    let prior_selection = inspect_workshop_item_paths(
        APP_ID,
        &fixture.target.join("steamapps/workshop/content/42"),
        &latest_root,
        vec![ITEM_ID.into()],
    )
    .unwrap();
    assert!(prior_selection[0].expected_path_exists);
    assert_eq!(
        Path::new(&prior_selection[0].expected_path),
        fixture.payload().parent().unwrap(),
        "the former directory-order selection reproduces the stale-cache bug"
    );
    assert_eq!(fs::read_to_string(fixture.payload()).unwrap(), "cached");
    assert_eq!(
        fs::read_to_string(latest_root.join(ITEM_ID).join("payload.bin")).unwrap(),
        "latest"
    );
    assert!(
        !result.output_excerpt.contains("Success. Downloaded item"),
        "download source evidence survives diagnostic tail eviction"
    );
}

#[tokio::test]
async fn conflicting_complete_caches_without_native_location_are_rejected() {
    let fixture = Fixture::new().await;
    fs::write(fixture.native().join("download-alternate"), "").unwrap();
    fs::write(fixture.native().join("omit-download-path"), "").unwrap();
    let result = fixture.download(&InstallCancellation::new(), |_| {}).await;
    let Err(SteamCmdError::SteamCmdCommandFailed { output_excerpt }) = result else {
        panic!("conflicting cached versions must not guess the validated source");
    };
    assert!(output_excerpt.contains("conflicting complete cached manifests"));
    assert_eq!(fs::read_to_string(fixture.payload()).unwrap(), "cached");
}

#[tokio::test]
async fn incomplete_native_download_does_not_fallback_to_complete_old_primary() {
    let fixture = Fixture::new().await;
    fs::write(fixture.native().join("download-alternate"), "").unwrap();
    fs::write(fixture.native().join("incomplete-download"), "").unwrap();
    let result = fixture.download(&InstallCancellation::new(), |_| {}).await;
    let Err(SteamCmdError::SteamCmdCommandFailed { output_excerpt }) = result else {
        panic!("an incomplete current source must not be replaced by old cache");
    };
    assert!(output_excerpt.contains("did not materialize Workshop items: 123456"));
    assert_eq!(fs::read_to_string(fixture.payload()).unwrap(), "cached");
}

#[tokio::test]
async fn workshop_download_cancellation_reaps_parent_and_descendant_before_return() {
    let fixture = Fixture::new().await;
    fs::write(fixture.native().join("download-stall"), "").unwrap();
    let cancellation = InstallCancellation::new();
    let token = cancellation.clone();
    let result = tokio::time::timeout(
        Duration::from_secs(8),
        fixture.download(&cancellation, move |progress| {
            if progress.output_excerpt.contains("fixture running") {
                token.cancel();
            }
        }),
    )
    .await
    .expect("download cancellation finishes owned-process cleanup");
    assert!(matches!(
        result,
        Err(SteamCmdError::InstallCancelled { .. })
    ));
    fixture.assert_owned_processes_exited();
    assert_eq!(fs::read_to_string(fixture.payload()).unwrap(), "cached");
    let path = fs::read_to_string(fixture.native().join("captured-script-path")).unwrap();
    assert!(
        !Path::new(&path).exists(),
        "cancelled operation script is cleaned up"
    );
}

#[tokio::test]
async fn workshop_readiness_cancellation_reaps_tree_without_recording_ready_or_downloading() {
    let fixture = Fixture::new().await;
    let record = fs::read(fixture.native().join(READY_RECORD)).unwrap();
    fs::write(
        fixture.native().join("steamclient64.dll"),
        "changed runtime",
    )
    .unwrap();
    fs::write(fixture.native().join("verify-stall"), "").unwrap();
    let cancellation = InstallCancellation::new();
    let token = cancellation.clone();
    let result = tokio::time::timeout(
        Duration::from_secs(8),
        fixture.download(&cancellation, move |progress| {
            if progress.output_excerpt.contains("fixture running") {
                token.cancel();
            }
        }),
    )
    .await
    .expect("readiness cancellation finishes owned-process cleanup");
    assert!(matches!(
        result,
        Err(SteamCmdError::InstallCancelled { .. })
    ));
    fixture.assert_owned_processes_exited();
    assert_eq!(fixture.calls(), ["verify", "verify"]);
    assert_eq!(
        fs::read(fixture.native().join(READY_RECORD)).unwrap(),
        record
    );
    assert!(!steamcmd_status(&fixture.settings).ready);
    assert_eq!(fs::read_to_string(fixture.payload()).unwrap(), "cached");
}
