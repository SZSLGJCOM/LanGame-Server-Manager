use super::*;

fn temporary_root() -> PathBuf {
    std::env::temp_dir().join(format!(
        "langame-update-cache-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn package(bytes: &[u8]) -> UpdatePackage {
    UpdatePackage {
        name: "steamcmd_fixture".into(),
        file: "steamcmd_fixture.zip.hash".into(),
        size: bytes.len() as u64,
        sha256: Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        bootstrapper: true,
        archive_file: "steamcmd_fixture.zip.hash".into(),
        archive_size: bytes.len() as u64,
        archive_sha256: Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
    }
}

#[tokio::test]
async fn cache_reuse_requires_size_and_digest_and_rejects_bad_cache_types() {
    let root = temporary_root();
    std::fs::create_dir(&root).unwrap();
    let item = package(b"verified");
    let path = root.join(&item.file);
    let deadline = InstallDeadline::new("cache test", std::time::Duration::from_secs(10));
    assert!(!cache_matches(&path, &item, deadline).await.unwrap());
    std::fs::write(&path, b"verified").unwrap();
    let manifest = UpdateManifest {
        version: "1".into(),
        packages: vec![item.clone()],
    };
    assert!(
        missing_packages(&root, &manifest, Some(&item), deadline)
            .await
            .unwrap()
            .is_empty()
    );
    std::fs::write(&path, b"tampered").unwrap();
    assert!(!cache_matches(&path, &item, deadline).await.unwrap());
    assert_eq!(
        missing_packages(&root, &manifest, Some(&item), deadline)
            .await
            .unwrap()
            .len(),
        1
    );
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(cache_matches(&path, &item, deadline).await.is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn cancelled_cache_inspection_preserves_previous_package() {
    let root = temporary_root();
    std::fs::create_dir(&root).unwrap();
    let item = package(b"verified");
    let path = root.join(&item.file);
    std::fs::write(&path, b"verified").unwrap();
    let cancellation = crate::InstallCancellation::new();
    cancellation.cancel();
    let result = cancellation
        .scope(cache_matches(
            &path,
            &item,
            InstallDeadline::new("cancelled cache", std::time::Duration::from_secs(10)),
        ))
        .await;
    assert!(matches!(
        result,
        Err(SteamCmdError::InstallCancelled { .. })
    ));
    assert_eq!(std::fs::read(&path).unwrap(), b"verified");
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn only_modern_windows_bootstrapper_skips_seeding() {
    let root = temporary_root();
    std::fs::create_dir(&root).unwrap();
    let executable = root.join("steamcmd.exe");
    let deadline = InstallDeadline::new("bootstrap check", std::time::Duration::from_secs(10));
    assert!(
        !supports_update_override(&executable, deadline)
            .await
            .unwrap()
    );
    let mut bytes = vec![0_u8; 256];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[60..64].copy_from_slice(&64_u32.to_le_bytes());
    bytes[64..70].copy_from_slice(b"PE\0\0\x64\x86");
    std::fs::write(&executable, &bytes).unwrap();
    assert!(
        !supports_update_override(&executable, deadline)
            .await
            .unwrap()
    );
    bytes[128..147].copy_from_slice(b"-overridepackageurl");
    std::fs::write(&executable, &bytes).unwrap();
    assert!(
        supports_update_override(&executable, deadline)
            .await
            .unwrap()
    );
    let cancelled = crate::InstallCancellation::new();
    cancelled.cancel();
    assert!(
        cancelled
            .scope(supports_update_override(&executable, deadline))
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(&executable).unwrap(), bytes);
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn bootstrap_extraction_handles_long_paths_and_preserves_existing_executable_on_bad_zip() {
    use std::os::windows::ffi::OsStrExt;

    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    let fixture = Fixture(temporary_root());
    std::fs::create_dir(&fixture.0).unwrap();
    let deadline = InstallDeadline::new("bootstrap long paths", std::time::Duration::from_secs(30));
    // Build the ZIP in memory so fixture creation has no filesystem path limit.
    let output = run_powershell(
        r#"$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.IO.Compression
$memory = [IO.MemoryStream]::new()
try {
  $zip = [IO.Compression.ZipArchive]::new($memory, [IO.Compression.ZipArchiveMode]::Create, $true)
  try {
    foreach ($name in @('steamcmd.exe', '../outside.txt')) {
      $stream = $zip.CreateEntry($name).Open()
      try {
        $bytes = [Text.Encoding]::UTF8.GetBytes('MZ verified bootstrap')
        $stream.Write($bytes, 0, $bytes.Length)
      } finally { $stream.Dispose() }
    }
  } finally { $zip.Dispose() }
  [BitConverter]::ToString($memory.ToArray()).Replace('-', '')
} finally { $memory.Dispose() }
"#,
        Some(&fixture.0),
        deadline,
    )
    .await
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        crate::output_excerpt(&output.stdout, &output.stderr)
    );
    let hex = String::from_utf8(output.stdout).unwrap();
    let hex = hex.trim();
    assert_eq!(hex.len() % 2, 0);
    let archive_bytes = (0..hex.len())
        .step_by(2)
        .map(|offset| u8::from_str_radix(&hex[offset..offset + 2], 16).unwrap())
        .collect::<Vec<_>>();

    let mut root = fixture.0.join("worker");
    while root.as_os_str().encode_wide().count() <= 280 {
        root.push("long-bootstrap-installation-directory");
    }
    std::fs::create_dir_all(root.join("package")).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let archive = root.join("package/steamcmd_win64.zip.verified");
    let executable = root.join("steamcmd.exe");
    assert!(crate::command_path_string(&archive).encode_utf16().count() > 260);
    assert!(
        crate::command_path_string(&executable)
            .encode_utf16()
            .count()
            > 260
    );
    std::fs::write(&archive, &archive_bytes).unwrap();
    std::fs::write(&executable, b"previous bootstrap").unwrap();
    install_bootstrapper(&root, &archive, deadline)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(&executable).unwrap(),
        b"MZ verified bootstrap"
    );
    assert!(!root.parent().unwrap().join("outside.txt").exists());
    assert_eq!(std::fs::read(&archive).unwrap(), archive_bytes);

    std::fs::write(&archive, b"invalid ZIP").unwrap();
    assert!(
        install_bootstrapper(&root, &archive, deadline)
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read(&executable).unwrap(),
        b"MZ verified bootstrap"
    );
    assert_eq!(std::fs::read(&archive).unwrap(), b"invalid ZIP");
    assert!(std::fs::read_dir(&root).unwrap().all(|entry| {
        let name = entry.unwrap().file_name();
        name == "package" || name == "steamcmd.exe"
    }));
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "downloads and executes official SteamCMD in an explicitly selected new diagnostic directory"]
async fn live_direct_install_cache_reuse_and_cancel() {
    let root = PathBuf::from(
        std::env::var("LANGAME_STEAMCMD_LIVE_ROOT")
            .expect("set an unused absolute diagnostic directory"),
    );
    assert!(
        root.is_absolute() && !root.exists(),
        "live probe must own a new directory"
    );
    assert_eq!(
        std::env::var("NO_PROXY").unwrap_or_default(),
        "*",
        "live measurement requires direct HTTP"
    );
    crate::prepare_configured_steamcmd_root(&root).unwrap();
    let settings = app_core::AppSettings {
        steamcmd_root: root.to_string_lossy().into_owned(),
        ..Default::default()
    };
    for run in 1..=2 {
        let started = std::time::Instant::now();
        let previous_exe = std::fs::metadata(root.join("steamcmd.exe"))
            .ok()
            .and_then(|value| value.modified().ok());
        let mut bytes = 0;
        let mut last_phase = None;
        let mut last_print = started;
        let status = crate::ensure_steamcmd_installed_with_progress(
            crate::test_install_lock_context(),
            &settings,
            |progress| {
                bytes = bytes.max(progress.downloaded_bytes.unwrap_or(0));
                if last_phase != Some(progress.phase) || last_print.elapsed().as_secs() >= 1 {
                    println!(
                        "LIVE_PROGRESS {}",
                        serde_json::to_string(&progress).unwrap()
                    );
                    last_phase = Some(progress.phase);
                    last_print = std::time::Instant::now();
                }
            },
        )
        .await
        .expect("native API verification must succeed");
        assert!(status.ready && crate::managed_steamcmd_status(&settings).ready);
        if run == 1 {
            assert!(bytes > 0);
        } else {
            assert_eq!(bytes, 0, "validated cache must avoid package downloads");
            assert_eq!(
                previous_exe,
                Some(
                    std::fs::metadata(root.join("steamcmd.exe"))
                        .unwrap()
                        .modified()
                        .unwrap()
                ),
                "unchanged bootstrapper must not be replaced"
            );
        }
        println!(
            "LIVE_RESULT run={run} elapsed_ms={} downloaded_bytes={bytes} ready={}",
            started.elapsed().as_millis(),
            status.ready
        );
    }
    let script = root.join("probe-quit.txt");
    std::fs::write(&script, b"quit\n").unwrap();
    let started = std::time::Instant::now();
    let result = crate::steamcmd_stream::run_steamcmd_script_with_progress(
        &root.join("steamcmd.exe").to_string_lossy(),
        &script,
        &root.to_string_lossy(),
        InstallDeadline::new("live native script", std::time::Duration::from_secs(60)),
        |_| {},
    )
    .await
    .unwrap();
    assert!(
        result.success && result.excerpt.contains("Loading Steam API...OK"),
        "{result:?}"
    );
    println!(
        "LIVE_SCRIPT elapsed_ms={} success=true",
        started.elapsed().as_millis()
    );
    let mut damaged = vec![0_u8; 256];
    damaged[..2].copy_from_slice(b"MZ");
    damaged[60..64].copy_from_slice(&64_u32.to_le_bytes());
    damaged[64..70].copy_from_slice(b"PE\0\0\x64\x86");
    damaged[128..147].copy_from_slice(b"-overridepackageurl");
    std::fs::write(root.join("steamcmd.exe"), damaged).unwrap();
    let mut repaired = false;
    let status = crate::ensure_steamcmd_installed_with_progress(
        crate::test_install_lock_context(),
        &settings,
        |progress| {
            repaired |= progress
                .detail
                .contains("Repairing the SteamCMD bootstrapper");
        },
    )
    .await
    .unwrap();
    assert!(repaired && status.ready);
    println!("LIVE_REPAIR ready=true");
    let cancel_root = root.join("cancel-probe");
    crate::prepare_configured_steamcmd_root(&cancel_root).unwrap();
    let settings = app_core::AppSettings {
        steamcmd_root: cancel_root.to_string_lossy().into_owned(),
        ..Default::default()
    };
    let token = crate::InstallCancellation::new();
    let mut cancelled_at = None;
    let result = crate::ensure_steamcmd_installed_with_progress_and_cancellation(
        crate::test_install_lock_context(),
        &settings,
        &token,
        |progress| {
            if cancelled_at.is_none() && progress.downloaded_bytes.unwrap_or(0) >= 256 * 1024 {
                cancelled_at = Some(std::time::Instant::now());
                token.cancel();
            }
        },
    )
    .await;
    assert!(
        matches!(result, Err(SteamCmdError::InstallCancelled { .. })),
        "{result:?}"
    );
    assert!(!crate::managed_steamcmd_status(&settings).ready);
    assert!(
        std::fs::read_dir(cancel_root.join("package"))
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".download-"))
    );
    println!(
        "LIVE_CANCEL elapsed_ms={} ready=false staging_files=0",
        cancelled_at.unwrap().elapsed().as_millis()
    );
}
