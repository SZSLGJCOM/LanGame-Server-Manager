use super::*;

#[test]
fn cache_and_unknown_source_warnings_are_distinct_and_never_echo_provider_details() {
    let root = std::env::temp_dir().join(format!(
        "langame-ts-warning-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let target_path = root.join("CoreKeeperServer_Data/StreamingAssets/Mods");
    let source = root.join("download");
    fs::create_dir_all(&target_path).unwrap();
    fs::create_dir(&source).unwrap();
    fs::write(root.join("CoreKeeperServer.exe"), b"MZfixture").unwrap();
    fs::write(source.join("ModManifest.json"), br#"{"name":"fixture"}"#).unwrap();
    fs::write(source.join("Plugin.dll"), b"MZfixture").unwrap();
    let target = ResolvedManualModTarget {
        instance_id: "fixture".into(),
        module_id: "corekeeper".into(),
        source_label: "Thunderstore".into(),
        target_label: "Mods".into(),
        target_path,
        accepts: vec!["folder".into()],
        id_strategy: None,
    };
    let source = DownloadedManualModSource {
        path: source,
        label: "Author-Plugin".into(),
        identity: OnlineModIdentity {
            provider: "thunderstore".into(),
            project: "Author-Plugin".into(),
            version: "1.0.0".into(),
            dependencies: Vec::new(),
            historical_names: Vec::new(),
            thunderstore_intent: None,
        },
    };
    super::super::super::commands_mod_staging::stage_thunderstore_online_mod_sources(
        target.clone(),
        &[source],
        None,
    )
    .unwrap();
    let proof = inspect_thunderstore_startup(&target).unwrap();
    let unknown = warning(&proof, None, None);
    assert_eq!(
        unknown.code,
        app_core::RuntimeStartWarningCode::ModUpdateSourceRequired
    );
    assert_eq!(unknown.item_ids, ["Author-Plugin"]);
    let cached = warning(
        &proof,
        Some(app_core::RuntimeModUpdateReason::Network),
        None,
    );
    assert_eq!(
        cached.code,
        app_core::RuntimeStartWarningCode::ModUpdateUsingCache
    );
    assert_eq!(cached.provider, app_core::RuntimeModProvider::Thunderstore);
    assert_eq!(cached.item_ids, ["Author-Plugin"]);
    assert!(!cached.message.contains("http"));
    assert!(!cached.message.contains("token"));
    let cancellation = app_steamcmd::InstallCancellation::new();
    cancellation.cancel();
    assert!(cancelled(&cancellation).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[path = "commands_thunderstore_start_workflow_tests.rs"]
mod workflow;
