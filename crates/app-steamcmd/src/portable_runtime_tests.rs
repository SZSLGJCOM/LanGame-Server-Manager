use super::*;

#[tokio::test]
async fn unrelated_modules_do_not_touch_runtime_roots_or_require_install_metadata() {
    let root = crate::tests::unique_test_root();
    let settings = AppSettings {
        steamcmd_root: root.join("steamcmd").to_string_lossy().into_owned(),
        ..Default::default()
    };
    let result = prepare_portable_runtime(
        crate::test_install_lock_context(),
        &settings,
        "valheim",
        &root.join("missing"),
        &[],
        InstallCancellation::new(),
        |_| panic!("unrelated module must not prepare runtimes"),
    )
    .await
    .unwrap();
    assert!(result.is_none());
    assert!(!root.exists());
}

#[tokio::test]
async fn cancellation_stops_before_reading_game_metadata_or_creating_tools() {
    let root = crate::tests::unique_test_root();
    let cancellation = InstallCancellation::new();
    cancellation.cancel();
    let settings = AppSettings {
        steamcmd_root: root.join("steamcmd").to_string_lossy().into_owned(),
        ..Default::default()
    };
    assert!(matches!(
        prepare_portable_runtime(
            crate::test_install_lock_context(),
            &settings,
            "minecraft",
            &root.join("missing"),
            &[],
            cancellation,
            |_| {}
        )
        .await,
        Err(SteamCmdError::InstallCancelled { .. })
    ));
    assert!(!root.exists());
}

#[tokio::test]
async fn runtime_root_does_not_claim_existing_operator_files() {
    let root = crate::tests::unique_test_root();
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("operator-data"), b"preserve").unwrap();
    let result = acquire_runtime_root(
        crate::test_install_lock_context(),
        &root,
        "runtime-root-fixture",
        InstallDeadline::new("fixture", Duration::from_secs(5)),
    )
    .await;
    assert!(result.is_err());
    assert_eq!(fs::read(root.join("operator-data")).unwrap(), b"preserve");
    assert!(!root.join(".langame-portable-runtime-root").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn published_runtime_generations_preserve_existing_files() {
    let root = crate::tests::unique_test_root();
    fs::create_dir_all(root.join("same-package")).unwrap();
    fs::write(root.join("same-package/native.dll"), b"in-use").unwrap();
    let stage = root.join("stage");
    fs::create_dir_all(&stage).unwrap();
    fs::write(stage.join("native.dll"), b"verified-repair").unwrap();
    let result = publish_runtime(&stage, &root, "same-package", "java", "same-package").unwrap();
    assert_ne!(result, root.join("same-package"));
    assert_eq!(
        fs::read(root.join("same-package/native.dll")).unwrap(),
        b"in-use"
    );
    assert_eq!(
        fs::read(result.join("native.dll")).unwrap(),
        b"verified-repair"
    );
    assert_eq!(cache_roots(&root, "java"), vec![result]);
    fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn bounded_zip_extraction_accepts_normal_files_and_rejects_parent_traversal() {
    let root = crate::tests::unique_test_root();
    fs::create_dir_all(&root).unwrap();
    for (name, entry, success) in [
        ("valid", "bin/runtime.dll", true),
        ("escape", "../escaped.dll", false),
    ] {
        let archive = root.join(format!("{name}.zip"));
        let script = format!(
            r#"$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
Add-Type -AssemblyName System.IO.Compression
$zip=[IO.Compression.ZipFile]::Open({archive},[IO.Compression.ZipArchiveMode]::Create)
try {{
  $entry=$zip.CreateEntry('{entry}')
  $writer=New-Object IO.StreamWriter($entry.Open())
  try {{ $writer.Write('fixture bytes') }} finally {{ $writer.Dispose() }}
}} finally {{ $zip.Dispose() }}"#,
            archive = ps_literal(&archive)
        );
        let deadline = InstallDeadline::new("local zip fixture", Duration::from_secs(30));
        let created = run_powershell(&script, Some(&root), deadline)
            .await
            .unwrap();
        assert!(
            created.status.success(),
            "{}",
            output_excerpt(&created.stdout, &created.stderr)
        );
        let stage = root.join(name);
        assert_eq!(
            extract_archive(&archive, &stage, deadline).await.is_ok(),
            success
        );
        assert!(!root.join("escaped.dll").exists());
        if success {
            assert_eq!(
                fs::read(stage.join("bin/runtime.dll")).unwrap(),
                b"fixture bytes"
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}
