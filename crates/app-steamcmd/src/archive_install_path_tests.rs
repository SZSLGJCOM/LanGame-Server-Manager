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
