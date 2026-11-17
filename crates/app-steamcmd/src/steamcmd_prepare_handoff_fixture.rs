use super::*;

pub(super) struct HandoffFixture {
    root: PathBuf,
}

impl HandoffFixture {
    pub(super) async fn new() -> Self {
        let root = crate::tests::unique_test_root();
        std::fs::create_dir_all(&root).unwrap();
        // Compile outside the behavior deadline. These tests exercise native
        // handoff and cleanup, not two nested PowerShell interpreter startups.
        let source = r#"
using System;
using System.Diagnostics;
using System.IO;
using System.Threading;
public static class PreparationHandoffFixture {
  private static void Runtime() {
    Console.WriteLine("Steam Console Client (c) Valve Corporation - version fixture");
    Console.WriteLine("Loading Steam API...OK");
  }
  private static void Updated() {
    Console.WriteLine("Update complete, launching...");
    Console.Out.Flush();
  }
  public static int Main(string[] args) {
    switch (args[0]) {
      case "update-ready":
        Updated(); Runtime(); return 0;
      case "failure":
        Console.Error.WriteLine("fresh runtime failed validation"); return 17;
      case "update-stall":
        Updated(); new ManualResetEvent(false).WaitOne(); return 0;
      case "child":
        Console.WriteLine("restarted child"); Console.Out.Flush();
        File.WriteAllText("child-pid.pending", Process.GetCurrentProcess().Id.ToString());
        File.Move("child-pid.pending", "child-pid");
        new ManualResetEvent(false).WaitOne(); return 0;
      case "parent":
        var start = new ProcessStartInfo(Process.GetCurrentProcess().MainModule.FileName, "child");
        start.UseShellExecute = false; start.CreateNoWindow = true;
        using (var child = Process.Start(start)) {
          while (!File.Exists("child-pid")) {
            if (child.HasExited) return 18;
            Thread.Sleep(10);
          }
          Updated(); return 0;
        }
      case "verify-cleanup":
        int oldPid = Int32.Parse(File.ReadAllText("child-pid"));
        try {
          using (var old = Process.GetProcessById(oldPid)) {
            if (!old.HasExited) return 19;
          }
        } catch (ArgumentException) { }
        Runtime(); return 0;
      default: return 80;
    }
  }
}
"#;
        let fixture = Self { root };
        let compiled = crate::run_powershell(
            &format!(
                "$ErrorActionPreference='Stop'; Add-Type -TypeDefinition @'\n{source}\n'@ -OutputAssembly {} -OutputType ConsoleApplication",
                ps_literal(&fixture.root.join("handoff.exe")),
            ),
            Some(&fixture.root),
            InstallDeadline::new("handoff fixture compilation", Duration::from_secs(30)),
        )
        .await
        .unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            crate::output_excerpt(&compiled.stdout, &compiled.stderr)
        );
        fixture
    }

    pub(super) fn command(&self, mode: &str) -> Command {
        let mut command = Command::new(self.root.join("handoff.exe"));
        apply_no_window(&mut command);
        command.arg(mode).current_dir(&self.root);
        command
    }
}

impl Drop for HandoffFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
