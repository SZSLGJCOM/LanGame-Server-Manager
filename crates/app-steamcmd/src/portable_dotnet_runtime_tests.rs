use super::*;

fn config(version: &str, policy: Option<&str>, patches: Option<bool>) -> serde_json::Value {
    let mut options =
        serde_json::json!({"framework": {"name":"Microsoft.NETCore.App", "version":version}});
    if let Some(policy) = policy {
        options["rollForward"] = policy.into();
    }
    if let Some(patches) = patches {
        options["applyPatches"] = patches.into();
    }
    serde_json::json!({"runtimeOptions":options})
}

fn release(version: &str) -> serde_json::Value {
    serde_json::json!({"runtime": {"version":version,"files":[{
        "name":"dotnet-runtime-win-x64.zip", "rid":"win-x64",
        "url":format!("https://builds.dotnet.microsoft.com/dotnet/Runtime/{version}/dotnet-runtime-{version}-win-x64.zip"),
        "hash":"a".repeat(128)
    }]}})
}

#[test]
fn runtime_config_preserves_exact_patch_and_supported_roll_forward() {
    let rolling = requirement(&config("8.0.2", None, None)).unwrap();
    assert!(rolling.accepts([8, 0, 28]));
    assert!(!rolling.accepts([8, 0, 1]));
    assert!(!rolling.accepts([9, 0, 2]));
    let exact = requirement(&config("8.0.2", Some("dIsAbLe"), None)).unwrap();
    assert!(exact.accepts([8, 0, 2]));
    assert!(!exact.accepts([8, 0, 28]));
    let lowest = requirement(&config("8.0.2", None, Some(false))).unwrap();
    assert!(lowest.accepts([8, 0, 28]));
    assert!(lowest.prefer_lowest);
    assert!(
        !requirement(&config("8.0.2", Some("minor"), None))
            .unwrap()
            .exact
    );
    assert!(requirement(&config("8.0.2", Some("Minor"), Some(false))).is_err());
    assert_eq!(
        requirement(&config("9.0.0", None, None)).unwrap().channel(),
        "9.0"
    );
    for value in [
        config("2.0.0", None, None),
        config("8.0.2-preview", None, None),
        config("8.0.2", Some("unknown"), None),
    ] {
        assert!(requirement(&value).is_err());
    }
}

#[test]
fn multiple_process_requirements_merge_without_cross_channel_or_exact_patch_drift() {
    let rolling = Requirement {
        version: [8, 0, 2],
        exact: false,
        prefer_lowest: false,
    };
    assert_eq!(
        rolling
            .merge(Requirement {
                version: [8, 0, 28],
                exact: false,
                prefer_lowest: false
            })
            .unwrap()
            .version,
        [8, 0, 28]
    );
    let exact = Requirement {
        version: [8, 0, 28],
        exact: true,
        prefer_lowest: false,
    };
    assert_eq!(rolling.merge(exact).unwrap(), exact);
    assert!(
        exact
            .merge(Requirement {
                version: [8, 0, 29],
                exact: false,
                prefer_lowest: false
            })
            .is_err()
    );
    assert!(
        exact
            .merge(Requirement {
                version: [8, 0, 2],
                exact: true,
                prefer_lowest: false
            })
            .is_err()
    );
    assert!(
        rolling
            .merge(Requirement {
                version: [9, 0, 0],
                exact: false,
                prefer_lowest: false
            })
            .is_err()
    );
}

#[test]
fn framework_discovery_uses_actual_executable_and_verifies_self_contained_packages() {
    let root = crate::tests::unique_test_root();
    fs::create_dir_all(&root).unwrap();
    let executable = root.join("CustomGameHost.exe");
    let runtimeconfig = executable.with_extension("runtimeconfig.json");
    fs::write(&runtimeconfig, config("10.0.1", None, None).to_string()).unwrap();
    assert_eq!(
        discover(std::slice::from_ref(&executable))
            .unwrap()
            .unwrap()
            .channel(),
        "10.0"
    );
    fs::write(&runtimeconfig, serde_json::json!({"runtimeOptions":{"includedFrameworks":[{"name":"Microsoft.NETCore.App","version":"10.0.1"}]}}).to_string()).unwrap();
    assert!(discover(std::slice::from_ref(&executable)).is_err());
    for file in [
        "coreclr.dll",
        "hostpolicy.dll",
        "hostfxr.dll",
        "System.Private.CoreLib.dll",
    ] {
        fs::write(root.join(file), b"self-contained fixture").unwrap();
    }
    assert!(
        discover(std::slice::from_ref(&executable))
            .unwrap()
            .is_none()
    );
    fs::write(&runtimeconfig, serde_json::json!({"runtimeOptions":{"framework":{"name":"Microsoft.AspNetCore.App","version":"10.0.1"}}}).to_string()).unwrap();
    assert!(discover(std::slice::from_ref(&executable)).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn additional_self_contained_frameworks_preserve_the_game_package_entrypoint() {
    let root = crate::tests::unique_test_root();
    fs::create_dir_all(&root).unwrap();
    let executable = root.join("GameHost.exe");
    for file in [
        "coreclr.dll",
        "hostpolicy.dll",
        "hostfxr.dll",
        "System.Private.CoreLib.dll",
    ] {
        fs::write(root.join(file), b"package fixture").unwrap();
    }
    fs::write(
        executable.with_extension("runtimeconfig.json"),
        serde_json::json!({
            "runtimeOptions": {"includedFrameworks": [
                {"name": "Microsoft.NETCore.App", "version": "8.0.1"},
                {"name": "Microsoft.AspNetCore.App", "version": "8.0.1"}
            ]}
        })
        .to_string(),
    )
    .unwrap();
    assert!(discover(&[executable]).unwrap().is_none());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn metadata_channel_follows_installed_application_requirement() {
    let required = Requirement {
        version: [10, 0, 1],
        exact: false,
        prefer_lowest: false,
    };
    let metadata = serde_json::json!({"channel-version":"10.0", "releases":[release("10.0.1"),release("10.0.2"),release("11.0.0")]});
    assert_eq!(
        select_package(&metadata.to_string(), required)
            .unwrap()
            .version,
        [10, 0, 2]
    );
    assert!(
        select_package(
            &metadata.to_string(),
            Requirement {
                version: [8, 0, 0],
                exact: false,
                prefer_lowest: false
            }
        )
        .is_err()
    );
}

#[test]
fn apphost_architecture_rejects_x86_and_accepts_x64_before_selecting_runtime() {
    let root = crate::tests::unique_test_root();
    fs::create_dir_all(&root).unwrap();
    let executable = root.join("host.exe");
    let mut bytes = vec![0u8; 70];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[60..64].copy_from_slice(&64u32.to_le_bytes());
    bytes[64..68].copy_from_slice(b"PE\0\0");
    bytes[68..70].copy_from_slice(&0x014cu16.to_le_bytes());
    fs::write(&executable, &bytes).unwrap();
    assert!(validate_apphost_architecture(&executable).is_err());
    bytes[68..70].copy_from_slice(&0x8664u16.to_le_bytes());
    fs::write(&executable, &bytes).unwrap();
    assert!(validate_apphost_architecture(&executable).is_ok());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn official_metadata_selects_latest_compatible_patch_and_rejects_tampering() {
    let required = Requirement {
        version: [8, 0, 2],
        exact: false,
        prefer_lowest: false,
    };
    let mut metadata = serde_json::json!({"channel-version":"8.0", "releases":[release("8.0.2"),release("8.0.28"),release("9.0.1")]});
    assert_eq!(
        select_package(&metadata.to_string(), required)
            .unwrap()
            .version,
        [8, 0, 28]
    );
    let exact = Requirement {
        exact: true,
        ..required
    };
    assert_eq!(
        select_package(&metadata.to_string(), exact)
            .unwrap()
            .version,
        [8, 0, 2]
    );
    metadata["releases"][1]["runtime"]["files"][0]["url"] =
        "https://example.com/dotnet-runtime-8.0.28-win-x64.zip".into();
    assert!(select_package(&metadata.to_string(), required).is_err());
    metadata["releases"][1] = release("8.0.28");
    metadata["releases"][1]["runtime"]["files"][0]["hash"] = "1234".into();
    assert!(select_package(&metadata.to_string(), required).is_err());
}

#[test]
fn dotnet_package_rejects_credentials_queries_wrong_architecture_and_non_tls() {
    for url in [
        "http://builds.dotnet.microsoft.com/dotnet/Runtime/8.0.28/dotnet-runtime-8.0.28-win-x64.zip",
        "https://builds.dotnet.microsoft.com/dotnet-runtime-8.0.28-win-x64.zip?source=elsewhere",
        "https://user@builds.dotnet.microsoft.com/dotnet-runtime-8.0.28-win-x64.zip",
        "https://builds.dotnet.microsoft.com/dotnet-runtime-8.0.28-win-arm64.zip",
    ] {
        assert!(!official_package_url(url, [8, 0, 28]), "{url}");
    }
}

fn source_failure(network: bool, detail: &str) -> SteamCmdError {
    SteamCmdError::HttpDownload {
        url: "https://example.invalid/dotnet.zip".into(),
        detail: detail.into(),
        permits_source_fallback: true,
        source_unavailable: network,
        permits_ip_family_retry: false,
    }
}

#[tokio::test]
async fn dotnet_reuses_verified_runtime_after_artifact_network_failure_only() {
    let candidate = PathBuf::from("cached-dotnet");
    let mut messages = Vec::new();
    let mut probes = 0;
    let reused = reuse_checked_runtime(
        source_failure(
            true,
            "artifact endpoint unavailable after metadata succeeded",
        ),
        vec![candidate.clone()],
        "8.0",
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
    assert_eq!(
        reused.environment.get("DOTNET_ROOT_X64"),
        Some(&candidate.to_string_lossy().into_owned())
    );
    assert_eq!(probes, 1);
    assert!(
        messages
            .iter()
            .any(|message| message.contains("reusing the verified installed runtime"))
    );

    for detail in [
        "SHA-512 checksum mismatch",
        "download size mismatch",
        "disk write failed",
        "invalid metadata",
    ] {
        messages.clear();
        let result = reuse_checked_runtime(
            source_failure(false, detail),
            vec![candidate.clone()],
            "8.0",
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
async fn dotnet_offline_reuse_rejects_unhealthy_runtime_and_cancellation() {
    let mut messages = Vec::new();
    let result = reuse_checked_runtime(
        SteamCmdError::HttpDownloadInterrupted {
            detail: "artifact body interrupted".into(),
        },
        vec![PathBuf::from("damaged-dotnet")],
        "8.0",
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
            vec![PathBuf::from("healthy-dotnet")],
            "8.0",
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
            vec![PathBuf::from("healthy-dotnet")],
            "8.0",
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
        vec![PathBuf::from("healthy-dotnet")],
        "8.0",
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
