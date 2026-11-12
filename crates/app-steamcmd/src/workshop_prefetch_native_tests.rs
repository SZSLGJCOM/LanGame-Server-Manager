use super::*;
use crate::{apply_no_window, ps_literal, run_command_capture, run_powershell};

async fn workers(fixture: &Fixture) -> (PathBuf, PathBuf) {
    let first = fixture.0.join("worker-a");
    let second = fixture.0.join("worker-b");
    crate::prepare_configured_steamcmd_root(&first).unwrap();
    crate::prepare_configured_steamcmd_root(&second).unwrap();
    // Substitute the external Steam binary only. Real process ownership,
    // runscript construction, evidence parsing, leases and cache publication run.
    let native = r#"
using System;
using System.IO;
using System.Diagnostics;
using System.Threading;
public static class PrivateWorkshopFixture {
  public static int Main(string[] args) {
    if (Array.IndexOf(args, "--child") >= 0) {
      File.WriteAllText("child-pid", Process.GetCurrentProcess().Id.ToString());
      new ManualResetEvent(false).WaitOne();
      return 0;
    }
    Console.WriteLine("Steam Console Client (c) Valve Corporation - fixture");
    Console.WriteLine("Loading Steam API...OK");
    if (Array.IndexOf(args, "+quit") >= 0) return 0;
    int index = Array.IndexOf(args, "+runscript");
    if (index < 0) return 80;
    // This fixture targets .NET Framework, whose legacy FileIOPermission
    // rejects Rust's canonical extended path prefix. Steam uses native IO.
    string scriptPath = args[index + 1];
    if (scriptPath.StartsWith(@"\\?\UNC\", StringComparison.OrdinalIgnoreCase))
      scriptPath = @"\\" + scriptPath.Substring(8);
    else if (scriptPath.StartsWith(@"\\?\", StringComparison.Ordinal))
      scriptPath = scriptPath.Substring(4);
    string script = File.ReadAllText(scriptPath);
    File.WriteAllText("script-path", args[index + 1]);
    if (!script.Contains("login anonymous")) return 81;
    string id = null;
    foreach (string line in script.Split('\n')) {
      if (line.StartsWith("workshop_download_item 42 ")) id = line.Split(' ')[2];
    }
    if (id == null) return 82;
    // Both processes must have entered their native download phase before
    // either can finish. A serial implementation cannot pass this barrier.
    File.WriteAllText("entered", "ready");
    string peer = File.ReadAllText("peer");
    int polls = 0;
    while (!File.Exists(Path.Combine(peer, "entered"))) {
      if (++polls > 1000) return 83;
      Thread.Sleep(10);
    }
    string root = Path.Combine(Environment.CurrentDirectory, "steamapps", "workshop");
    if (Directory.Exists(root)) return 84; // abandoned cache must be cleared before launch
    string partial = Path.Combine(root, "downloads", id);
    Directory.CreateDirectory(partial);
    File.WriteAllText(Path.Combine(partial, "partial.bin"), "partial");
    if (File.Exists("fail")) {
      Console.WriteLine("ERROR! Download item " + id + " failed (FixtureFailure).");
      return 0;
    }
    if (File.Exists("stall")) {
      File.WriteAllText("parent-pid", Process.GetCurrentProcess().Id.ToString());
      var start = new ProcessStartInfo(Process.GetCurrentProcess().MainModule.FileName, "--child");
      start.UseShellExecute = false;
      start.CreateNoWindow = true;
      start.WorkingDirectory = Environment.CurrentDirectory;
      Process.Start(start);
      new ManualResetEvent(false).WaitOne();
    }
    // Deliberately use SteamCMD's fallback cache instead of force_install_dir.
    string content = Path.Combine(root, "content", "42", id);
    Directory.CreateDirectory(content);
    string value = File.Exists("payload-value") ? File.ReadAllText("payload-value") : "latest";
    File.WriteAllText(Path.Combine(content, "payload.bin"), value);
    File.WriteAllText(Path.Combine(root, "appworkshop_42.acf"),
      "\"AppWorkshop\" { \"appid\" \"42\" \"WorkshopItemsInstalled\" { \"" + id + "\" { \"manifest\" \"10\" \"size\" \"6\" } } }");
    Console.WriteLine("Success. Downloaded item " + id + " to \"" + content + "\" (6 bytes)");
    return 0;
  }
}
"#;
    let exe = first.join("steamcmd.exe");
    let output = run_powershell(&format!(
        "$ErrorActionPreference='Stop'; Add-Type -TypeDefinition @'\n{native}\n'@ -OutputAssembly {} -OutputType ConsoleApplication",
        ps_literal(&exe),
    ), Some(&first), deadline()).await.unwrap();
    assert!(
        output.status.success(),
        "{}",
        crate::output_excerpt(&output.stdout, &output.stderr)
    );
    fs::copy(&exe, second.join("steamcmd.exe")).unwrap();
    for (root, peer) in [(&first, &second), (&second, &first)] {
        fs::write(root.join("peer"), peer.to_str().unwrap()).unwrap();
        fs::create_dir(root.join("package")).unwrap();
        fs::write(root.join("package/keep-package"), b"package sentinel").unwrap();
        fs::create_dir(root.join("config")).unwrap();
        fs::write(root.join("config/keep-config"), b"config sentinel").unwrap();
        fs::write(root.join("package/steam_cmd_win64.manifest"), format!(
            "\"win64\" {{ \"version\" \"123\" \"steamcmd_win64\" {{ \"file\" \"steamcmd_win64.zip.hash\" \"size\" \"12\" \"sha2\" \"{}\" \"IsBootstrapperPackage\" \"1\" }} }}",
            "a".repeat(64),
        )).unwrap();
        let mut command = tokio::process::Command::new(root.join("steamcmd.exe"));
        apply_no_window(&mut command);
        command.arg("+quit").current_dir(root);
        let output = run_command_capture(command, deadline()).await.unwrap();
        assert!(output.status.success());
        assert!(
            String::from_utf8(output.stdout)
                .unwrap()
                .contains("Loading Steam API...OK")
        );
        let settings = AppSettings {
            steamcmd_root: root.to_string_lossy().into_owned(),
            ..fixture.settings()
        };
        crate::steamcmd_readiness::record_verified(&crate::managed_steamcmd_status(&settings))
            .unwrap();
    }
    (first, second)
}

fn deadline() -> InstallDeadline {
    InstallDeadline::new("private native fixture", Duration::from_secs(30))
}

async fn fetch(
    settings: &AppSettings,
    runtime: &Path,
    attempt: &Path,
    id: &str,
    cancellation: &InstallCancellation,
) -> Result<PreparedWorkshopDownload, SteamCmdError> {
    prefetch_workshop_items(
        settings,
        WorkshopPrefetchRequest {
            runtime_root: runtime,
            download_root: attempt,
            consumer_app_id: 42,
            ids: &[id.to_owned()],
        },
        crate::test_install_lock_context(),
        cancellation,
        |_| {},
    )
    .await
}

#[tokio::test]
async fn native_workers_overlap_and_ready_snapshot_survives_runtime_reuse_and_cancellation() {
    let fixture = Fixture::new();
    let (a, b) = workers(&fixture).await;
    // A previous crash's content and temporary downloads must not accumulate.
    write_cache(&a, 42, &[("579999", "1", b"abandoned")]);
    let settings = fixture.settings();
    let target = fixture.0.join("game");
    // Holding the actual module/instance resource tree must not block fetch.
    let game = crate::acquire_game_install_lifecycle(
        crate::test_install_lock_context(),
        "prefetch-native",
        std::slice::from_ref(&target),
    )
    .await
    .unwrap();
    let first_root = fixture.0.join("attempt-a");
    let second_root = fixture.0.join("attempt-b");
    let cancellation = InstallCancellation::new();
    let (first, second) = tokio::time::timeout(Duration::from_secs(20), async {
        tokio::join!(
            fetch(&settings, &a, &first_root, "571001", &cancellation),
            fetch(&settings, &b, &second_root, "571002", &cancellation),
        )
    })
    .await
    .expect("both native processes must overlap, without the held module lease");
    let first = Arc::new(first.unwrap());
    let second = second.unwrap();
    assert!(
        Path::new(&first.download_result().items[0].expected_path)
            .starts_with(canonical_resource_path(&first_root).unwrap())
    );
    assert!(
        Path::new(&second.download_result().items[0].expected_path)
            .starts_with(canonical_resource_path(&second_root).unwrap())
    );
    assert_eq!(payload(&first_root, 42, "571001"), b"latest");
    assert_worker_clean(&a);
    assert_worker_clean(&b);
    let script_a = fs::read_to_string(a.join("script-path")).unwrap();
    let script_b = fs::read_to_string(b.join("script-path")).unwrap();
    assert_ne!(script_a, script_b);
    assert!(!Path::new(&script_a).exists() && !Path::new(&script_b).exists());

    fs::write(a.join("payload-value"), "second").unwrap();
    let next_root = fixture.0.join("attempt-next");
    let next = tokio::time::timeout(
        Duration::from_secs(10),
        fetch(&settings, &a, &next_root, "571001", &cancellation),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(payload(&next_root, 42, "571001"), b"second");
    assert_worker_clean(&a);
    assert_eq!(
        payload(&first_root, 42, "571001"),
        b"latest",
        "ready ticket must not depend on a reused runtime cache"
    );
    let published = publish_prefetched_workshop_items(Arc::clone(&first), target.clone(), &game)
        .await
        .unwrap();
    assert_eq!(payload(&target, 42, "571001"), b"latest");
    assert!(Path::new(&published.items[0].expected_path).starts_with(&target));
    drop(next);

    fs::write(a.join("fail"), "").unwrap();
    for index in 0..2 {
        let failed_root = fixture.0.join(format!("attempt-failed-{index}"));
        let result = fetch(&settings, &a, &failed_root, "571004", &cancellation).await;
        let Err(error) = result else {
            panic!("native failure must remain a failure")
        };
        assert!(error.to_string().contains("FixtureFailure"));
        assert_worker_clean(&a);
    }
    fs::remove_file(a.join("fail")).unwrap();

    fs::write(a.join("stall"), "").unwrap();
    let cancelled = InstallCancellation::new();
    let stalled_root = fixture.0.join("attempt-cancelled");
    let cancel_after_child_starts = async {
        tokio::time::timeout(Duration::from_secs(10), async {
            while !a.join("child-pid").exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        cancelled.cancel();
    };
    let (result, ()) = tokio::join!(
        fetch(&settings, &a, &stalled_root, "571001", &cancelled),
        cancel_after_child_starts,
    );
    assert!(matches!(
        result,
        Err(SteamCmdError::InstallCancelled { .. })
    ));
    for name in ["parent-pid", "child-pid"] {
        let pid: u32 = fs::read_to_string(a.join(name)).unwrap().parse().unwrap();
        assert_process_exited(pid);
    }
    assert_worker_clean(&a);
    // Returning cancellation has released the runtime only after owned children exited.
    let lease = ResourceLocks::acquire(crate::test_install_lock_context(), &[], &[a], deadline())
        .await
        .unwrap();
    drop(lease);
    assert_eq!(payload(&first_root, 42, "571001"), b"latest");
}

fn assert_worker_clean(root: &Path) {
    assert!(
        !root.join("steamapps/workshop").exists(),
        "worker payload/staging cache must not accumulate"
    );
    assert_eq!(
        fs::read(root.join("package/keep-package")).unwrap(),
        b"package sentinel"
    );
    assert_eq!(
        fs::read(root.join("config/keep-config")).unwrap(),
        b"config sentinel"
    );
    assert!(root.join("package/steam_cmd_win64.manifest").exists());
    assert!(root.join("steamcmd.exe").exists());
}

fn assert_process_exited(pid: u32) {
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: these PIDs are written only by the owned fixture process tree.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(ERROR_INVALID_PARAMETER as i32)
            );
            return;
        }
        let mut exit = 0;
        let result = GetExitCodeProcess(handle, &mut exit);
        CloseHandle(handle);
        assert_ne!(result, 0);
        assert_ne!(
            exit, STILL_ACTIVE as u32,
            "owned process survived cancellation"
        );
    }
}
