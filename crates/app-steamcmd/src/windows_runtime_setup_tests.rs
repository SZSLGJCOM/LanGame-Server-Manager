use super::*;
use app_runtime::{WindowsRuntimeArchitecture as Architecture, WindowsRuntimeFamily as Family};
use std::future::Future;

#[cfg(windows)]
#[path = "windows_runtime_msi_tests.rs"]
mod msi;

#[cfg(windows)]
#[path = "windows_runtime_sandbox_tests.rs"]
mod sandbox;

#[cfg(windows)]
#[path = "windows_runtime_signature_tests.rs"]
mod signature;

fn requirement(
    family: Family,
    architecture: Architecture,
    directory: &str,
    dll: &str,
) -> WindowsRuntimeRequirement {
    WindowsRuntimeRequirement {
        family,
        architecture,
        imported_dlls: vec![dll.into()],
        search_directories: vec![PathBuf::from(directory)],
    }
}

#[test]
fn xna_is_only_added_for_vanilla_terraria() {
    assert!(requires_xna("terraria", false));
    assert!(!requires_xna("terraria", true));
    assert!(!requires_xna("valheim", false));
    assert!(planned_packages(Vec::new(), false).is_empty());
}

#[test]
fn package_planning_preserves_each_importing_directory() {
    let first = requirement(
        Family::VisualCpp2015,
        Architecture::X64,
        "C:/servers/first",
        "vcruntime140.dll",
    );
    let second = requirement(
        Family::VisualCpp2015,
        Architecture::X64,
        "C:/servers/second",
        "msvcp140.dll",
    );
    let plans = planned_packages(vec![first.clone(), second.clone()], false);
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].requirements, vec![first.clone(), second.clone()]);
    let script = process::probe_script(Some(&first));
    assert!(script.contains("'vcruntime140.dll'"));
    assert!(!script.contains("'msvcp140.dll'"));
    assert!(!script.contains("C:/servers/second"));
    assert!(script.contains("LoadLibraryEx($candidate,[IntPtr]::Zero,0x800)"));
    assert!(script.contains("Assert-MicrosoftSignature $candidate"));
}

#[test]
fn directx_is_installed_once_but_keeps_both_architecture_probes() {
    let plans = planned_packages(
        vec![
            requirement(
                Family::DirectXJune2010,
                Architecture::X86,
                "C:/servers/x86",
                "xaudio2_7.dll",
            ),
            requirement(
                Family::DirectXJune2010,
                Architecture::X64,
                "C:/servers/x64",
                "d3dx9_43.dll",
            ),
        ],
        false,
    );
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].package.id, "directx-june-2010");
    assert_eq!(plans[0].requirements.len(), 2);
    assert_ne!(
        plans[0].requirements[0].architecture,
        plans[0].requirements[1].architecture
    );
}

#[test]
fn newer_visual_cpp_packages_do_not_downgrade_current_installations() {
    assert!(version_is_newer("14.44.35211.0", Some("v14.40.33816.00")).unwrap());
    assert!(!version_is_newer("14.44.35211", Some("v14.44.35211.00")).unwrap());
    assert!(!version_is_newer("14.44.35211.0", Some("14.50.0.0")).unwrap());
    assert!(version_is_newer("14.44.35211.0", None).unwrap());
    assert!(version_is_newer("untrusted version", None).is_err());
}

#[test]
fn only_success_is_ready_and_restart_codes_remain_blocking() {
    use process::{InstallerExit, classify_installer_exit};
    assert_eq!(classify_installer_exit(0), InstallerExit::Success);
    assert_eq!(classify_installer_exit(3010), InstallerExit::RebootRequired);
    assert_eq!(classify_installer_exit(1641), InstallerExit::RebootRequired);
    assert_eq!(classify_installer_exit(1618), InstallerExit::Busy);
    assert_eq!(classify_installer_exit(1603), InstallerExit::Failed);
    assert_eq!(classify_installer_exit(1638), InstallerExit::Failed);
}

#[test]
fn installers_use_only_the_catalog_with_signature_checks_and_atomic_waiting() {
    let root = Path::new("C:/trusted tools/runtimes/windows-packages");
    for package in [
        catalog::XNA,
        catalog::package(Family::VisualCpp2010, Architecture::X86),
        catalog::package(Family::VisualCpp2015, Architecture::X64),
        catalog::package(Family::DirectXJune2010, Architecture::X64),
    ] {
        let script = process::installer_script(package, &package_path(root, package), root);
        assert!(script.contains("Global\\LanGame.WindowsRuntimePrerequisites"));
        assert!(script.contains("Assert-MicrosoftSignature"));
        assert!(
            script
                .find("$packageLease=[LanGameRuntimeFileLease]::Acquire(")
                .unwrap()
                < script.find("    Assert-MicrosoftSignature ").unwrap()
        );
        assert!(
            script
                .contains("File.Open(lease.path, FileMode.Open, FileAccess.Read, FileShare.Read)")
        );
        assert!(script.contains("$packageLease.Validate()"));
        assert!(script.contains("-Verb RunAs -PassThru"));
        assert!(script.contains("$installer.WaitForExit()"));
        assert!(script.contains("if ($null -ne $installer -and !$installer.HasExited)"));
        assert!(!script.contains("installscript"));
        assert!(!script.contains("Stop-Process"));
        if package.id == "vc-2010-x86" {
            assert!(script.contains("$arguments='/q /norestart'"));
        }
        if package.id == catalog::XNA.id {
            assert!(script.contains(catalog::XNA.sha256.unwrap()));
        }
        if package.kind == catalog::InstallerKind::DirectX {
            assert!(script.contains("Assert-MicrosoftSignature $file"));
            assert!(script.contains("$fullStage.StartsWith($allowedRoot"));
        }
    }
}

#[test]
fn xna_health_requires_each_independent_framework_assembly_without_game_creation() {
    let script = process::probe_script(None);
    for assembly in [
        "'Microsoft.Xna.Framework'",
        "'Microsoft.Xna.Framework.Game'",
        "'Microsoft.Xna.Framework.Graphics'",
        "'Microsoft.Xna.Framework.Xact'",
    ] {
        assert!(script.contains(assembly));
    }
    assert!(script.contains("[Reflection.Assembly]::Load("));
    assert!(!script.contains("Activator"));
    assert!(!script.contains("New-Object Microsoft.Xna"));
    assert!(!script.contains(".Run("));
}

struct RuntimeFixture(PathBuf);

impl RuntimeFixture {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "lgsm-runtime-boundary-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}

impl Drop for RuntimeFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn failed_atomic_state_publication_preserves_the_target_and_leaves_no_partial_file() {
    let fixture = RuntimeFixture::new();
    fs::create_dir(fixture.0.join(INSTALLING_MARKER)).unwrap();
    assert!(
        process::publish_install_state(&fixture.0, INSTALLING_MARKER, "12345", "test-runtime")
            .is_err()
    );
    let names = fs::read_dir(&fixture.0)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(names, vec![std::ffi::OsString::from(INSTALLING_MARKER)]);
    assert!(fixture.0.join(INSTALLING_MARKER).is_dir());
    process::publish_install_state(&fixture.0, REBOOT_MARKER, "12345", "test-runtime").unwrap();
    let saved: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.0.join(REBOOT_MARKER)).unwrap()).unwrap();
    assert_eq!(saved["boot_identity"], "12345");
}

#[cfg(windows)]
fn capture_fixture_script(script: &str) -> std::process::Output {
    let executable = app_runtime::windows_system_directory()
        .unwrap()
        .join("WindowsPowerShell/v1.0/powershell.exe");
    app_runtime::capture_windows_utility(
        &executable,
        &["-NoProfile", "-NonInteractive", "-Command", script],
        Duration::from_secs(30),
    )
    .unwrap()
}

#[cfg(windows)]
#[test]
fn runtime_source_lease_blocks_writes_replacement_and_parent_rename_until_released() {
    let fixture = RuntimeFixture::new();
    let parent = fixture.0.join("package");
    fs::create_dir(&parent).unwrap();
    let source = parent.join("source.msi");
    fs::write(&source, b"locked source fixture; never executed").unwrap();
    let script = format!(
        r#"$ErrorActionPreference='Stop'
{native}
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class LanGameDirectoryWriteFixture {{
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern SafeFileHandle CreateFile(string path, uint access, uint share, IntPtr security, uint disposition, uint flags, IntPtr template);
}}
'@
$lease=[LanGameRuntimeFileLease]::Acquire({source})
try {{
    $lease.Validate()
    $blocked=0
    try {{ $writer=[IO.File]::Open({source},[IO.FileMode]::Open,[IO.FileAccess]::Write,[IO.FileShare]::ReadWrite); $writer.Dispose() }} catch [IO.IOException] {{ $blocked++ }}
    try {{ [IO.File]::Delete({source}) }} catch [IO.IOException] {{ $blocked++ }}
    try {{ [IO.Directory]::Move({parent},{moved}) }} catch [IO.IOException] {{ $blocked++ }}
    if ($blocked -ne 3) {{ throw 'Source write/delete/parent rename was not fully blocked.' }}
    $directoryWriter=[LanGameDirectoryWriteFixture]::CreateFile({parent},0x102,7,[IntPtr]::Zero,3,0x02200000,[IntPtr]::Zero)
    try {{ if (!$directoryWriter.IsInvalid) {{ throw 'A directory data/attribute writer bypassed the source lease.' }} }} finally {{ $directoryWriter.Dispose() }}
    [IO.File]::WriteAllText((Join-Path {parent} 'unrelated.txt'),'independent child writes remain available')
}} finally {{ $lease.Dispose() }}
$directoryWriter=[LanGameDirectoryWriteFixture]::CreateFile({parent},0x102,7,[IntPtr]::Zero,3,0x02200000,[IntPtr]::Zero)
try {{ if ($directoryWriter.IsInvalid) {{ throw 'Directory writer remained blocked after source lease disposal.' }} }} finally {{ $directoryWriter.Dispose() }}
[IO.File]::WriteAllText({source},'released')
[IO.Directory]::Move({parent},{moved})
"#,
        native = process::FILE_LEASE_FUNCTION,
        source = ps_literal(&source),
        parent = ps_literal(&parent),
        moved = ps_literal(&fixture.0.join("moved"))
    );
    let output = capture_fixture_script(&script);
    assert!(
        output.status.success(),
        "Source lease boundary failed: {}",
        output_excerpt(&output.stdout, &output.stderr)
    );
    assert_eq!(
        fs::read(fixture.0.join("moved/source.msi")).unwrap(),
        b"released"
    );
}

#[cfg(windows)]
#[test]
fn local_crt_dependencies_load_after_full_verification_and_remain_loaded_until_the_end() {
    let fixture = RuntimeFixture::new();
    fs::write(
        fixture.0.join("msvcp140.dll"),
        b"fake DLL bytes; never executed",
    )
    .unwrap();
    fs::write(
        fixture.0.join("vcruntime140.dll"),
        b"fake DLL bytes; never executed",
    )
    .unwrap();
    let request = WindowsRuntimeRequirement {
        family: Family::VisualCpp2015,
        architecture: Architecture::X64,
        imported_dlls: vec!["msvcp140.dll".into(), "vcruntime140.dll".into()],
        search_directories: vec![fixture.0.clone()],
    };
    // Only the external loader and signature boundary are fake. Production's
    // planner, file/parent locks, ordering, progress bound and cleanup run.
    let fake_loader = r#"
Add-Type -TypeDefinition @'
using System;
using System.IO;
public static class LanGameRuntimeLoader {
    public static bool AllTrusted, BaseLoaded;
    public static int Loads, Frees;
    public static IntPtr LoadLibraryEx(string path, IntPtr ignored, uint flags) {
        if (!AllTrusted) throw new Exception("A DLL was loaded before all signatures were checked.");
        if (flags != 0x800) throw new Exception("Unsafe DLL search flags.");
        Loads++;
        if (Path.GetFileName(path) == "vcruntime140.dll") { BaseLoaded=true; return new IntPtr(1); }
        return BaseLoaded ? new IntPtr(2) : IntPtr.Zero;
    }
    public static bool FreeLibrary(IntPtr handle) { if (handle.ToInt64() == 1) BaseLoaded=false; Frees++; return true; }
}
'@
"#;
    for trusted in [true, false] {
        let signature = if trusted {
            r#"
$script:SignatureChecks=0
function Assert-MicrosoftSignature([string]$Path) {
    $script:SignatureChecks++
    if ($script:SignatureChecks -eq 2) { [LanGameRuntimeLoader]::AllTrusted=$true }
}
"#
        } else {
            "function Assert-MicrosoftSignature([string]$Path) { throw 'Untrusted runtime fixture' }\n"
        };
        let mut script = process::probe_script(Some(&request))
            .replacen(process::SIGNATURE_FUNCTION, signature, 1)
            .replacen(process::NATIVE_LOADER_FUNCTION, fake_loader, 1);
        script.push_str(
            "\nif ([LanGameRuntimeLoader]::BaseLoaded) { throw 'Leaked base runtime handle' }\n",
        );
        script.push_str(if trusted { "if ([LanGameRuntimeLoader]::Frees -ne 2) { throw 'Runtime handles were not retained/freed correctly' }\n" }
            else { "if ([LanGameRuntimeLoader]::Loads -ne 0) { throw 'An untrusted runtime was loaded' }\n" });
        let output = capture_fixture_script(&script);
        assert!(
            output.status.success(),
            "Local CRT fixture failed: {}",
            output_excerpt(&output.stdout, &output.stderr)
        );
        let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["ready"], trusted);
    }
}

#[tokio::test]
async fn cancellation_waits_for_atomic_worker_completion() {
    let cancellation = InstallCancellation::new();
    let (release, released) = tokio::sync::oneshot::channel();
    let worker = tokio::spawn(async move {
        released.await.unwrap();
        0_u32
    });
    let deadline = InstallDeadline::new("test atomic runtime install", Duration::from_secs(5));
    cancellation.cancel();
    let mut report = |_| {};
    let waiting = cancellation.scope(await_atomic_worker(worker, deadline, &mut report));
    tokio::pin!(waiting);
    // Poll the owner while cancellation is already latched. It must still be
    // pending until the fake installer returns an actual outcome.
    assert!(
        std::future::poll_fn(|context| std::task::Poll::Ready(
            waiting.as_mut().poll(context).is_pending()
        ))
        .await
    );
    release.send(()).unwrap();
    assert_eq!(waiting.await.unwrap(), 0);
}

#[tokio::test]
async fn dropping_an_async_waiter_does_not_release_the_installers_lease() {
    let gate = std::sync::Arc::new(tokio::sync::Mutex::new(()));
    let guard = gate.clone().lock_owned().await;
    let (release, released) = tokio::sync::oneshot::channel();
    let (completed, completion) = tokio::sync::oneshot::channel();
    let worker = tokio::spawn(async move {
        released.await.unwrap();
        drop(guard);
        completed.send(()).unwrap();
    });
    let deadline = InstallDeadline::new("test detached runtime installer", Duration::from_secs(5));
    let waiter =
        tokio::spawn(async move { await_atomic_worker(worker, deadline, &mut |_| {}).await });
    waiter.abort();
    let _ = waiter.await;
    assert!(gate.try_lock().is_err());
    release.send(()).unwrap();
    completion.await.unwrap();
    assert!(gate.try_lock().is_ok());
}

#[cfg(windows)]
#[test]
fn generated_powershell_is_syntax_valid_without_executing_installers() {
    let root = Path::new("C:/trusted tools/O'Brien/windows-packages");
    let mut scripts = vec![
        process::probe_script(None),
        process::probe_script(Some(&requirement(
            Family::VisualCpp2015,
            Architecture::X64,
            "C:/server/O'Brien",
            "vcruntime140.dll",
        ))),
    ];
    for package in [
        catalog::XNA,
        catalog::package(Family::VisualCpp2010, Architecture::X86),
        catalog::package(Family::VisualCpp2015, Architecture::X64),
        catalog::package(Family::DirectXJune2010, Architecture::X64),
    ] {
        scripts.push(process::installer_script(
            package,
            &package_path(root, package),
            root,
        ));
    }
    let executable = app_runtime::windows_system_directory()
        .unwrap()
        .join("WindowsPowerShell/v1.0/powershell.exe");
    for script in scripts {
        let parser = format!(
            "$tokens=$null; $parseErrors=$null; $null=[Management.Automation.Language.Parser]::ParseInput('{}',[ref]$tokens,[ref]$parseErrors); if ($parseErrors.Count -gt 0) {{ $parseErrors|ForEach-Object {{ Write-Output $_.Message }}; exit 1 }}",
            script.replace('\'', "''")
        );
        let output = app_runtime::capture_windows_utility(
            &executable,
            &["-NoProfile", "-NonInteractive", "-Command", &parser],
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(
            output.status.success(),
            "PowerShell parse failed: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}

#[cfg(windows)]
#[tokio::test]
async fn xna_framework_health_probe_runs_without_installing_or_downloading() {
    // The result is an environment health snapshot, not a requirement that CI
    // machines install XNA. Only the trusted Framework loader runs here.
    let snapshot = process::probe(
        None,
        InstallDeadline::new("readonly XNA health probe", Duration::from_secs(30)),
    )
    .await
    .expect("Windows Framework assembly probe must return a health result");
    assert!(snapshot.version.is_none());
    eprintln!(
        "Readonly x86 .NET Framework XNA 4 assembly load: ready={}",
        snapshot.ready
    );
}
