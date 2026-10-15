//! Invalidate standard input only inside a disposable test worker.
use super::*;
use std::ffi::c_void;

const WORKER: &str = "steamcmd_stream::spawn_tests::invalid_stdin_worker";
const WORKER_ROOT: &str = "LGSM_STEAMCMD_INVALID_STDIN_TEST_ROOT";
const STD_INPUT_HANDLE: u32 = -10_i32 as u32;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetStdHandle(kind: u32) -> *mut c_void;
    fn SetStdHandle(kind: u32, handle: *mut c_void) -> i32;
}

struct StandardInputRestore(*mut c_void);

impl Drop for StandardInputRestore {
    fn drop(&mut self) {
        // SAFETY: this worker owns its process standard-handle table. The
        // original handle stays open while the invalid value is installed.
        unsafe { SetStdHandle(STD_INPUT_HANDLE, self.0) };
    }
}

struct Fixture(PathBuf);

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn steamcmd_script_spawns_with_invalid_parent_stdin() {
    let fixture = Fixture(crate::tests::unique_test_root());
    fs::create_dir_all(&fixture.0).unwrap();
    fs::write(
        fixture.0.join("install.txt"),
        "app_update 42 validate\nquit\n",
    )
    .unwrap();
    let executable = fixture.0.join("steamcmd.exe");
    // Only the native SteamCMD boundary is substituted. Requiring EOF also
    // verifies that the replacement standard input is valid and noninteractive.
    let source = r#"
using System;
using System.IO;
public static class SteamCmdInputProbe {
  public static int Main(string[] args) {
    if (args.Length != 2 || args[0] != "+runscript" || !File.Exists(args[1])) return 81;
    using (Stream input = Console.OpenStandardInput()) {
      if (input.ReadByte() != -1) return 82;
    }
    File.WriteAllText(Path.Combine(Path.GetDirectoryName(args[1]), "spawned"), "stdin EOF");
    Console.WriteLine("Success! App '42' fully installed.");
    Console.Error.WriteLine("fixture stderr captured");
    return 0;
  }
}
"#;
    let script = format!(
        "$ErrorActionPreference='Stop'; Add-Type -TypeDefinition @'\n{source}\n'@ -OutputAssembly {} -OutputType ConsoleApplication",
        ps_literal(&executable)
    );
    let compiled = run_powershell(
        &script,
        Some(&fixture.0),
        InstallDeadline::new("invalid stdin fixture", Duration::from_secs(15)),
    )
    .await
    .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        output_excerpt(&compiled.stdout, &compiled.stderr)
    );

    let output = tokio::time::timeout(
        Duration::from_secs(30),
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", WORKER, "--nocapture", "--test-threads=1"])
            .env(WORKER_ROOT, &fixture.0)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .expect("isolated standard-input worker must finish within its deadline")
    .unwrap();
    assert!(
        output.status.success(),
        "isolated standard-input worker failed: {}",
        output_excerpt(&output.stdout, &output.stderr)
    );
    assert_eq!(
        fs::read_to_string(fixture.0.join("spawned")).unwrap(),
        "stdin EOF"
    );
}

#[tokio::test]
async fn invalid_stdin_worker() {
    let Some(root) = std::env::var_os(WORKER_ROOT) else {
        return;
    };
    let root = PathBuf::from(root);
    let executable = root.join("steamcmd.exe");
    let invalid = windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    // SAFETY: the parent launches only this test in a separate process. Do not
    // close the inherited handle; restore it before reporting test assertions.
    let original = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    assert!(!original.is_null() && original != invalid);
    let restore = StandardInputRestore(original);
    // SAFETY: changing this disposable worker's standard input cannot affect
    // the test runner or any sibling tests in its process.
    assert_ne!(unsafe { SetStdHandle(STD_INPUT_HANDLE, invalid) }, 0);
    // SAFETY: reading the same process-owned standard-handle slot has no side effects.
    assert_eq!(unsafe { GetStdHandle(STD_INPUT_HANDLE) }, invalid);
    let streaming = run_steamcmd_script_with_source(
        executable.to_str().unwrap(),
        &root.join("install.txt"),
        root.to_str().unwrap(),
        InstallDeadline::new("invalid standard input", Duration::from_secs(10)),
        |_| {},
        None,
    )
    .await;
    // Collect both outcomes before asserting: an inherited-input spawn error
    // in streaming must not hide the independent capture-path regression.
    let captured = run_powershell(
        "$text = [Console]::In.ReadToEnd(); if ($text.Length -ne 0) { throw 'stdin was not empty' }; [Console]::Out.WriteLine('powershell stdin EOF'); [Console]::Error.WriteLine('powershell stderr captured')",
        Some(&root),
        InstallDeadline::new("invalid standard input capture", Duration::from_secs(10)),
    )
    .await;
    drop(restore);
    assert!(
        streaming.is_ok() && captured.is_ok(),
        "invalid stdin spawn results: streaming={:?}; powershell_capture={:?}",
        streaming.as_ref().map(|_| ()),
        captured.as_ref().map(|_| ())
    );
    let output = streaming.unwrap();
    assert!(output.success, "{}", output.excerpt);
    assert_eq!(output.exit_code, Some(0));
    assert!(
        output
            .excerpt
            .contains("Success! App '42' fully installed.")
    );
    assert!(output.excerpt.contains("fixture stderr captured"));
    let output = captured.unwrap();
    assert!(
        output.status.success(),
        "{}",
        output_excerpt(&output.stdout, &output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("powershell stdin EOF"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("powershell stderr captured"));
}
