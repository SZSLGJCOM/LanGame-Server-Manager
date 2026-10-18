use super::*;

#[test]
fn java_version_probe_handles_supported_output_and_rejects_noise() {
    assert_eq!(
        parse_major("openjdk version \"21.0.7\" 2025-04-15\nOpenJDK Runtime Environment"),
        Some(21)
    );
    assert_eq!(parse_major("java version \"1.8.0_442\""), Some(8));
    assert_eq!(
        parse_major("Could not initialize the Java virtual machine."),
        None
    );
    assert_eq!(parse_major("java version \"broken\""), None);
}

#[test]
fn installed_game_metadata_selects_java_without_selecting_a_new_game_release() {
    let root = crate::tests::unique_test_root();
    fs::create_dir_all(root.join(".langame")).unwrap();
    let metadata = root.join(".langame/minecraft-server.json");
    fs::write(
        &metadata,
        r#"{"version_id":"retained-game-version","required_java_major":17}"#,
    )
    .unwrap();
    assert_eq!(required_major(&root).unwrap(), 17);
    for invalid in [
        r#"{"required_java_major":0}"#,
        r#"{"required_java_major":-1}"#,
        r#"{"required_java_major":"21"}"#,
        "{}",
    ] {
        fs::write(&metadata, invalid).unwrap();
        assert!(required_major(&root).is_err());
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn java_archive_requires_one_runtime_root() {
    let root = crate::tests::unique_test_root();
    for name in ["first", "second"] {
        fs::create_dir_all(root.join(name).join("bin")).unwrap();
        fs::write(root.join(name).join("bin/java.exe"), b"fixture").unwrap();
        if name == "first" {
            assert!(extracted_java_root(&root).is_ok());
        }
    }
    assert!(extracted_java_root(&root).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn bundled_release_metadata_allows_only_known_windows_x64_java_major() {
    assert_eq!(
        parse_bundled_major("JAVA_VERSION=\"17.0.12\"\nOS_ARCH=\"amd64\"\nOS_NAME=\"Windows\"\n")
            .unwrap(),
        17
    );
    assert_eq!(
        parse_bundled_major("JAVA_VERSION=\"1.8.0_442\"\nOS_ARCH=\"x86_64\"\n").unwrap(),
        8
    );
    for invalid in [
        "JAVA_VERSION=\"17.0.12\"\nOS_ARCH=\"aarch64\"",
        "JAVA_VERSION=\"17.0.12\"\nOS_ARCH=\"amd64\"\nOS_NAME=\"Linux\"",
        "JAVA_VERSION=\"17.0.12\"\nJAVA_VERSION=\"21.0.4\"\nOS_ARCH=\"amd64\"",
        "JAVA_VERSION=\"25-ea\"\nOS_ARCH=\"amd64\"",
        "OS_ARCH=\"amd64\"",
    ] {
        assert!(parse_bundled_major(invalid).is_err(), "{invalid}");
    }
}

fn source_failure(network: bool, detail: &str) -> SteamCmdError {
    SteamCmdError::HttpDownload {
        url: "https://example.invalid/runtime.zip".into(),
        detail: detail.into(),
        permits_source_fallback: true,
        source_unavailable: network,
        permits_ip_family_retry: false,
    }
}

#[tokio::test]
async fn java_reuses_verified_runtime_after_artifact_network_failure_only() {
    let candidate = PathBuf::from("cached-java");
    let mut messages = Vec::new();
    let mut probes = 0;
    let reused = reuse_checked_runtime(
        source_failure(
            true,
            "artifact endpoint unavailable after metadata succeeded",
        ),
        vec![candidate.clone()],
        21,
        InstallDeadline::new("offline fixture", Duration::from_secs(5)),
        &mut |update| messages.push(update.detail),
        |path, _| {
            probes += 1;
            assert_eq!(path, candidate);
            async { Ok(true) }
        },
    )
    .await
    .unwrap();
    assert_eq!(reused.executable, Some(candidate.join("bin/java.exe")));
    assert_eq!(probes, 1);
    assert!(
        messages
            .iter()
            .any(|message| message.contains("reusing the verified installed runtime"))
    );

    for detail in [
        "SHA-256 checksum mismatch",
        "download size mismatch",
        "disk write failed",
        "invalid metadata",
    ] {
        messages.clear();
        let result = reuse_checked_runtime(
            source_failure(false, detail),
            vec![candidate.clone()],
            21,
            InstallDeadline::new("closed failure fixture", Duration::from_secs(5)),
            &mut |update| messages.push(update.detail),
            |_, _| {
                probes += 1;
                async { Ok(true) }
            },
        )
        .await;
        assert!(matches!(
            result,
            Err(SteamCmdError::HttpDownload {
                source_unavailable: false,
                ..
            })
        ));
        assert!(messages.is_empty());
        assert_eq!(
            probes, 1,
            "closed failures must not invoke the runtime probe"
        );
    }
}

#[tokio::test]
async fn java_offline_reuse_rejects_unhealthy_runtime_and_cancellation() {
    let mut messages = Vec::new();
    let result = reuse_checked_runtime(
        SteamCmdError::HttpDownloadInterrupted {
            detail: "artifact body interrupted".into(),
        },
        vec![PathBuf::from("damaged-java")],
        21,
        InstallDeadline::new("unhealthy fixture", Duration::from_secs(5)),
        &mut |update| messages.push(update.detail),
        |_, _| async { Ok(false) },
    )
    .await;
    assert!(matches!(
        result,
        Err(SteamCmdError::HttpDownloadInterrupted { .. })
    ));
    assert!(messages.is_empty());

    let cancellation = InstallCancellation::new();
    let during_probe = cancellation.clone();
    let result = cancellation
        .scope(reuse_checked_runtime(
            source_failure(true, "network unavailable"),
            vec![PathBuf::from("healthy-java")],
            21,
            InstallDeadline::new("cancel fixture", Duration::from_secs(5)),
            &mut |update| messages.push(update.detail),
            move |_, _| {
                during_probe.cancel();
                async { Ok(true) }
            },
        ))
        .await;
    assert!(matches!(
        result,
        Err(SteamCmdError::InstallCancelled { .. })
    ));
    assert!(messages.is_empty());

    let cancellation = InstallCancellation::new();
    let during_progress = cancellation.clone();
    let result = cancellation
        .scope(reuse_checked_runtime(
            source_failure(true, "network unavailable"),
            vec![PathBuf::from("healthy-java")],
            21,
            InstallDeadline::new("progress cancel fixture", Duration::from_secs(5)),
            &mut |update| {
                messages.push(update.detail);
                during_progress.cancel();
            },
            |_, _| async { Ok(true) },
        ))
        .await;
    assert!(matches!(
        result,
        Err(SteamCmdError::InstallCancelled { .. })
    ));
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("reusing the verified installed runtime"));
    messages.clear();

    let mut expired_probes = 0;
    let result = reuse_checked_runtime(
        source_failure(true, "network unavailable"),
        vec![PathBuf::from("healthy-java")],
        21,
        InstallDeadline::new("expired fixture", Duration::ZERO),
        &mut |update| messages.push(update.detail),
        |_, _| {
            expired_probes += 1;
            async { Ok(true) }
        },
    )
    .await;
    assert!(matches!(
        result,
        Err(SteamCmdError::OperationTimedOut { .. })
    ));
    assert!(messages.is_empty());
    assert_eq!(expired_probes, 0);
}
