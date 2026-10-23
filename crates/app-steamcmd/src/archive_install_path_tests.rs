use super::*;

struct Fixture {
    root: PathBuf,
    parent: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("az-{:x}-{stamp:x}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let root_length = root.as_os_str().encode_wide().count();
        assert!(
            root_length < 169,
            "ZIP test needs room for a 170-character parent path"
        );
        let parent = root.join("p".repeat(170 - root_length - 1));
        fs::create_dir(&parent).unwrap();
        Self { root, parent }
    }

    async fn zip(&self, escape: bool) -> Vec<u8> {
        let path = self
            .root
            .join(if escape { "escape.zip" } else { "fixture.zip" });
        let mut entries = vec!["bin/Server.exe", "assets/nested/package.txt"];
        if escape {
            entries.push("../outside.txt");
        }
        let mut script = format!(
            "$ErrorActionPreference='Stop'; Add-Type -AssemblyName System.IO.Compression,System.IO.Compression.FileSystem; \
             $zip=[IO.Compression.ZipFile]::Open({}, [IO.Compression.ZipArchiveMode]::Create); try {{\n",
            ps_literal(&path),
        );
        for entry in entries {
            script.push_str(&format!(
                "$entry=$zip.CreateEntry('{entry}'); $stream=$entry.Open(); try {{ \
                 $bytes=[Text.Encoding]::UTF8.GetBytes('fixture payload'); $stream.Write($bytes,0,$bytes.Length) \
                 }} finally {{ $stream.Dispose() }}\n"
            ));
        }
        script.push_str("} finally { $zip.Dispose() }");
        let output = run_powershell(&script, Some(&self.root), deadline())
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            output_excerpt(&output.stdout, &output.stderr)
        );
        fs::read(path).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // The exclusively created fixture is a direct child of the managed
        // test TEMP directory. Never clean a computed installation path.
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn deadline() -> InstallDeadline {
    InstallDeadline::new("ZIP long path fixture", Duration::from_secs(30))
}

#[tokio::test]
async fn archive_download_staging_opens_zip_without_repeating_long_destination_name() {
    let fixture = Fixture::new();
    let zip = fixture.zip(false).await;
    let destination = fixture
        .parent
        .join(".rimworld-00000000-0000-0000-0000-000000000000.download-1700000000000000000.zip");
    // The former algorithm repeated this already-qualified target plus another
    // PID, timestamp, sequence and extension. Rust can write it; Windows
    // PowerShell's legacy ZipFile.OpenRead fails at >= 260 UTF-16 units.
    let old_path = fixture.parent.join(format!(
        ".{}.download-12345-1700000000000000000-0.zip",
        destination.file_name().unwrap().to_string_lossy(),
    ));
    fs::write(&old_path, &zip).unwrap();
    assert!(old_path.as_os_str().encode_wide().count() >= 260);
    let legacy_probe = format!(
        "$ErrorActionPreference='Stop'; Add-Type -AssemblyName System.IO.Compression,System.IO.Compression.FileSystem; \
         $archive=[IO.Compression.ZipFile]::OpenRead({}); $archive.Dispose()",
        ps_literal(&old_path),
    );
    let old_result = run_powershell(&legacy_probe, Some(&fixture.root), deadline())
        .await
        .unwrap();
    eprintln!(
        "ARCHIVE_PATH_COUNTEREXAMPLE utf16_units={} legacy_open_succeeded={}",
        old_path.as_os_str().encode_wide().count(),
        old_result.status.success()
    );
    // Do not require newer Windows/.NET environments to retain the legacy
    // limitation. The production-generated path must work in either case.
    let mut paths = Vec::new();
    for _ in 0..2 {
        let (path, mut file) = create_minecraft_staging_file(&destination).unwrap();
        file.write_all(&zip).unwrap();
        file.sync_all().unwrap();
        drop(file);
        assert_eq!(path.parent(), destination.parent());
        assert_eq!(path.extension().unwrap(), "zip");
        assert!(path.as_os_str().encode_wide().count() < 260);
        let script = format!(
            "$ErrorActionPreference='Stop'; Add-Type -AssemblyName System.IO.Compression,System.IO.Compression.FileSystem; \
             $archive=[IO.Compression.ZipFile]::OpenRead({}); try {{ if ($archive.Entries.Count -ne 2) {{ throw 'Unexpected ZIP entries' }} }} finally {{ $archive.Dispose() }}",
            ps_literal(&path),
        );
        let result = run_powershell(&script, Some(&fixture.root), deadline())
            .await
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            output_excerpt(&result.stdout, &result.stderr)
        );
        paths.push(path);
    }
    assert_ne!(paths[0], paths[1]);
    assert_eq!(fs::read(&paths[0]).unwrap(), zip);
    assert_eq!(fs::read(&paths[1]).unwrap(), zip);
}

#[tokio::test]
async fn archive_short_transaction_paths_publish_and_preserve_rollback_and_zip_slip_guards() {
    let fixture = Fixture::new();
    let root = fixture
        .parent
        .join("rimworld-00000000-0000-0000-0000-000000000000");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("previous.txt"), b"original program").unwrap();
    for escape in [false, true] {
        let paths = ArchiveScratchPaths::new(&fixture.parent);
        let (archive_path, mut file) = create_minecraft_staging_file(&paths.archive_path).unwrap();
        file.write_all(&fixture.zip(escape).await).unwrap();
        drop(file);
        let script = direct_download_publish_script(
            &root,
            &paths.staging_root,
            &paths.rollback_root,
            &paths.publish_phase_path,
            &paths.staging_root.join("bin/Server.exe"),
            &archive_path,
            &Default::default(),
        );
        let output = run_powershell(&script, Some(&fixture.parent), deadline())
            .await
            .unwrap();
        if escape {
            assert!(!output.status.success(), "ZIP traversal must not publish");
            assert!(!paths.rollback_root.exists());
            assert!(!fixture.parent.join("outside.txt").exists());
        } else {
            assert!(
                output.status.success(),
                "{}",
                output_excerpt(&output.stdout, &output.stderr)
            );
            assert_eq!(
                fs::read(paths.rollback_root.join("previous.txt")).unwrap(),
                b"original program"
            );
            assert_eq!(
                fs::read_to_string(&paths.publish_phase_path).unwrap(),
                "rollback_pending"
            );
        }
        assert_eq!(
            fs::read(root.join("bin/Server.exe")).unwrap(),
            b"fixture payload"
        );
        assert_eq!(
            fs::read(root.join("assets/nested/package.txt")).unwrap(),
            b"fixture payload"
        );
        assert!(!archive_path.exists());
        assert!(!paths.staging_root.exists());
    }
}

fn locked_publication_script(
    root: &Path,
    paths: &ArchiveScratchPaths,
    locked_file: &Path,
    on_wait: &str,
) -> String {
    let publish = crate::install_transaction::prepared_install_publish_script(
        root,
        &paths.staging_root,
        &paths.rollback_root,
        &paths.publish_phase_path,
        &paths.staging_root.join("Server.exe"),
        &Default::default(),
    );
    format!(
        r#"$script:held = [IO.File]::Open({locked_file}, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
$script:waits = 0
function Start-Sleep([int]$Milliseconds) {{
  $script:waits++
  {on_wait}
  Microsoft.PowerShell.Utility\Start-Sleep -Milliseconds $Milliseconds
}}
try {{
{publish}
}} finally {{
  if ($null -ne $script:held) {{ $script:held.Dispose() }}
  Write-Output "Contention waits: $script:waits"
}}
"#,
        locked_file = ps_literal(locked_file),
    )
}

#[tokio::test]
async fn archive_publication_retries_released_locks_and_restores_after_persistent_locks() {
    for release in [true, false] {
        let fixture = Fixture::new();
        let root = fixture.parent.join("server");
        let paths = ArchiveScratchPaths::new(&fixture.parent);
        fs::create_dir(&root).unwrap();
        fs::write(root.join("previous.txt"), b"original program").unwrap();
        fs::create_dir(&paths.staging_root).unwrap();
        fs::write(paths.staging_root.join("Server.exe"), b"new program").unwrap();
        // Release only after a real rename failed. Scheduling speed cannot make
        // the transient-lock case pass without exercising the retry boundary.
        let on_wait = if release {
            "if ($null -ne $script:held) { $script:held.Dispose(); $script:held = $null }"
        } else {
            ""
        };
        let script = locked_publication_script(
            &root,
            &paths,
            &paths.staging_root.join("Server.exe"),
            on_wait,
        );
        let output = run_powershell(&script, Some(&fixture.parent), deadline())
            .await
            .unwrap();
        let details = output_excerpt(&output.stdout, &output.stderr);
        assert!(
            details.contains("Waiting for directory publication contention"),
            "{details}"
        );
        if release {
            assert!(output.status.success(), "{details}");
            assert_eq!(fs::read(root.join("Server.exe")).unwrap(), b"new program");
            assert_eq!(
                fs::read(paths.rollback_root.join("previous.txt")).unwrap(),
                b"original program"
            );
            assert_eq!(
                fs::read_to_string(&paths.publish_phase_path).unwrap(),
                "rollback_pending"
            );
        } else {
            assert!(
                !output.status.success(),
                "persistent lock must prevent publication"
            );
            assert!(
                details.contains("Directory publication failed"),
                "{details}"
            );
            assert!(
                details.contains("attempts") && details.contains("elapsed"),
                "{details}"
            );
            assert_eq!(
                fs::read(root.join("previous.txt")).unwrap(),
                b"original program"
            );
            assert!(!root.join("Server.exe").exists());
            assert!(!paths.rollback_root.exists());
            assert!(!paths.publish_phase_path.exists());
        }
    }
}

#[tokio::test]
async fn archive_publication_does_not_retry_replaced_sources_or_new_destinations() {
    for replace_source in [true, false] {
        let fixture = Fixture::new();
        let root = fixture.parent.join("server");
        let displaced = fixture.parent.join("original");
        let paths = ArchiveScratchPaths::new(&fixture.parent);
        fs::create_dir(&root).unwrap();
        fs::write(root.join("previous.txt"), b"original program").unwrap();
        fs::create_dir(&paths.staging_root).unwrap();
        fs::write(paths.staging_root.join("Server.exe"), b"new program").unwrap();
        let on_wait = if replace_source {
            format!(
                "$script:held.Dispose(); $script:held = $null; \
                 [IO.Directory]::Move($root, {}); [IO.Directory]::CreateDirectory($root) | Out-Null; \
                 [IO.File]::WriteAllText((Join-Path $root 'operator.txt'), 'new directory owner')",
                ps_literal(&displaced),
            )
        } else {
            String::from(
                "[IO.Directory]::CreateDirectory($rollback) | Out-Null; \
                 [IO.File]::WriteAllText((Join-Path $rollback 'operator.txt'), 'new directory owner')",
            )
        };
        let script = locked_publication_script(&root, &paths, &root.join("previous.txt"), &on_wait);
        let output = run_powershell(&script, Some(&fixture.parent), deadline())
            .await
            .unwrap();
        let details = output_excerpt(&output.stdout, &output.stderr);
        assert!(
            !output.status.success(),
            "changed paths must prevent publication"
        );
        assert!(details.contains("Contention waits: 1"), "{details}");
        let (message, old_root, new_root) = if replace_source {
            ("Directory publication source changed", &displaced, &root)
        } else {
            (
                "Directory publication destination already exists",
                &root,
                &paths.rollback_root,
            )
        };
        assert!(details.contains(message), "{details}");
        assert!(!paths.publish_phase_path.exists());
        let recovery = crate::install_transaction::recover_install_publication(
            &root,
            &paths.staging_root,
            &paths.rollback_root,
            &paths.rejected_root,
            &paths.publish_phase_path,
            false,
        )
        .await;
        if replace_source {
            recovery.unwrap();
        } else {
            assert!(matches!(
                recovery,
                Err(SteamCmdError::InstallRollbackFailed { .. })
            ));
        }
        assert_eq!(
            fs::read(old_root.join("previous.txt")).unwrap(),
            b"original program"
        );
        assert_eq!(
            fs::read(new_root.join("operator.txt")).unwrap(),
            b"new directory owner"
        );
        assert!(!root.join("Server.exe").exists());
        assert!(!paths.rejected_root.exists());
    }
}
