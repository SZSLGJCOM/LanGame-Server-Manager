use super::*;

struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "langame-thunderstore-start-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(root.join("CoreKeeperServer_Data/StreamingAssets/Mods")).unwrap();
        fs::write(root.join("CoreKeeperServer.exe"), b"MZnative-fixture").unwrap();
        Self { root }
    }
    fn target(&self) -> ResolvedManualModTarget {
        ResolvedManualModTarget {
            instance_id: "fixture".into(),
            module_id: "corekeeper".into(),
            source_label: "Thunderstore".into(),
            target_label: "native Mods".into(),
            target_path: self.root.join("CoreKeeperServer_Data/StreamingAssets/Mods"),
            accepts: vec!["folder".into()],
            id_strategy: None,
        }
    }
    fn source(
        &self,
        project: &str,
        version: &str,
        intent: Option<ThunderstoreInstallIntent>,
        dependencies: &[&str],
    ) -> DownloadedManualModSource {
        let path = self
            .root
            .join(format!("download-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir(&path).unwrap();
        fs::write(path.join("Plugin.dll"), format!("MZ{version}")).unwrap();
        fs::write(
            path.join("ModManifest.json"),
            format!(r#"{{"name":"{project}","version":"{version}"}}"#),
        )
        .unwrap();
        fs::create_dir(path.join("Config")).unwrap();
        fs::write(path.join("Config/user.ini"), b"default").unwrap();
        DownloadedManualModSource {
            path,
            label: project.into(),
            identity: OnlineModIdentity {
                provider: "thunderstore".into(),
                project: project.into(),
                historical_names: vec![project.into()],
                version: version.into(),
                dependencies: dependencies.iter().map(|value| (*value).into()).collect(),
                thunderstore_intent: intent,
            },
        }
    }
    fn install(&self, sources: &[DownloadedManualModSource]) {
        crate::commands::commands_mod_staging::stage_thunderstore_online_mod_sources(
            self.target(),
            sources,
            None,
        )
        .unwrap();
    }
    fn plugin(&self) -> PathBuf {
        self.target().target_path.join("thunderstore-Author-Plugin")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn complete_payload_unknown_intent_is_not_guessed_and_disabled_roots_are_not_requested() {
    let fixture = Fixture::new();
    fixture.install(&[fixture.source("Author-Plugin", "1.0.0", None, &[])]);
    let proof = inspect_thunderstore_startup(&fixture.target()).unwrap();
    assert!(proof.references.is_empty());
    assert_eq!(proof.source_required, ["Author-Plugin"]);
    proof.verify_unchanged(&fixture.target()).unwrap();
    let path = fixture.target().target_path.join(ONLINE_MOD_MANIFEST);
    let mut manifest = read_manifest(&path).unwrap();
    let package = manifest
        .packages
        .get_mut("thunderstore-Author-Plugin")
        .unwrap();
    package.version = None;
    package.dependencies = None;
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert_eq!(
        inspect_thunderstore_startup(&fixture.target())
            .unwrap()
            .source_required,
        ["Author-Plugin"]
    );
    manifest
        .packages
        .get_mut("thunderstore-Author-Plugin")
        .unwrap()
        .enabled = false;
    fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(
        inspect_thunderstore_startup(&fixture.target())
            .unwrap()
            .packages
            .is_empty()
    );
}

#[test]
fn complete_cache_requires_actual_sha_and_identity_metadata_not_directory_or_size() {
    for relative in ["Plugin.dll", "ModManifest.json"] {
        let fixture = Fixture::new();
        fixture.install(&[fixture.source(
            "Author-Plugin",
            "1.0.0",
            Some(ThunderstoreInstallIntent::Root { version: None }),
            &[],
        )]);
        let path = fixture.plugin().join(relative);
        let before = fs::read(&path).unwrap();
        fs::write(&path, vec![b'X'; before.len()]).unwrap();
        assert!(inspect_thunderstore_startup(&fixture.target()).is_err());
        assert_eq!(fs::read(&path).unwrap(), vec![b'X'; before.len()]);
        fs::remove_file(&path).unwrap();
        assert!(inspect_thunderstore_startup(&fixture.target()).is_err());
    }
}

#[test]
fn roots_keep_pins_dependencies_stay_dependencies_and_missing_dependency_blocks() {
    let fixture = Fixture::new();
    fixture.install(&[
        fixture.source(
            "Author-Plugin",
            "1.0.0",
            Some(ThunderstoreInstallIntent::Root {
                version: Some("1.0.0".into()),
            }),
            &["Author-Library-2.0.0"],
        ),
        fixture.source(
            "Author-Library",
            "2.0.0",
            Some(ThunderstoreInstallIntent::Dependency),
            &[],
        ),
    ]);
    let proof = inspect_thunderstore_startup(&fixture.target()).unwrap();
    assert_eq!(
        proof.references,
        ["https://thunderstore.io/package/Author/Plugin/1.0.0/"]
    );
    assert!(proof.source_required.is_empty());
    assert_eq!(proof.packages.len(), 2);
    fs::remove_file(
        fixture
            .target()
            .target_path
            .join("thunderstore-Author-Library/Plugin.dll"),
    )
    .unwrap();
    assert!(inspect_thunderstore_startup(&fixture.target()).is_err());
}

#[test]
fn orphan_dependency_needs_source_confirmation_and_new_unowned_payload_blocks() {
    let fixture = Fixture::new();
    fixture.install(&[fixture.source(
        "Author-Plugin",
        "1.0.0",
        Some(ThunderstoreInstallIntent::Dependency),
        &[],
    )]);
    assert_eq!(
        inspect_thunderstore_startup(&fixture.target())
            .unwrap()
            .source_required,
        ["Author-Plugin"]
    );
    fs::write(fixture.plugin().join("unknown.pak"), b"unowned").unwrap();
    assert!(inspect_thunderstore_startup(&fixture.target()).is_err());
    assert_eq!(
        fs::read(fixture.plugin().join("unknown.pak")).unwrap(),
        b"unowned"
    );
}

#[test]
fn startup_package_transaction_preserves_user_config_and_publishes_root_intent_together() {
    let fixture = Fixture::new();
    fixture.install(&[fixture.source(
        "Author-Plugin",
        "1.0.0",
        Some(ThunderstoreInstallIntent::Root { version: None }),
        &[],
    )]);
    fs::write(fixture.plugin().join("Config/user.ini"), b"operator").unwrap();
    fs::write(fixture.plugin().join("user-note.txt"), b"note").unwrap();
    let proof = inspect_thunderstore_startup(&fixture.target()).unwrap();
    let next = fixture.source(
        "Author-Plugin",
        "2.0.0",
        Some(ThunderstoreInstallIntent::Root { version: None }),
        &[],
    );
    crate::commands::commands_mod_staging::stage_thunderstore_online_mod_sources_for_startup(
        fixture.target(),
        &[next],
        &proof,
        &app_steamcmd::InstallCancellation::new(),
    )
    .unwrap();
    let after = inspect_thunderstore_startup(&fixture.target()).unwrap();
    assert_eq!(after.packages[0].version, "2.0.0");
    let identity: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.plugin().join("ModManifest.json")).unwrap())
            .unwrap();
    assert_eq!(
        identity["version"], "2.0.0",
        "native identity metadata must be replaced with the verified new payload"
    );
    assert_eq!(
        after.references,
        ["https://thunderstore.io/package/Author/Plugin/"]
    );
    assert_eq!(
        fs::read(fixture.plugin().join("Config/user.ini")).unwrap(),
        b"operator"
    );
    assert_eq!(
        fs::read(fixture.plugin().join("user-note.txt")).unwrap(),
        b"note"
    );
}

#[test]
fn startup_preserves_same_path_nested_operator_metadata_without_relaxing_root_identity() {
    let fixture = Fixture::new();
    let old = fixture.source(
        "Author-Plugin",
        "1.0.0",
        Some(ThunderstoreInstallIntent::Root { version: None }),
        &[],
    );
    fs::write(old.path.join("Config/Info.json"), b"vendor default").unwrap();
    fixture.install(&[old]);
    fs::write(
        fixture.plugin().join("Config/Info.json"),
        b"operator JSON bytes",
    )
    .unwrap();
    let proof = inspect_thunderstore_startup(&fixture.target()).unwrap();
    let next = fixture.source(
        "Author-Plugin",
        "2.0.0",
        Some(ThunderstoreInstallIntent::Root { version: None }),
        &[],
    );
    fs::write(
        next.path.join("Config/Info.json"),
        b"different new vendor default",
    )
    .unwrap();
    crate::commands::commands_mod_staging::stage_thunderstore_online_mod_sources_for_startup(
        fixture.target(),
        &[next],
        &proof,
        &app_steamcmd::InstallCancellation::new(),
    )
    .unwrap();
    assert_eq!(
        fs::read(fixture.plugin().join("Config/Info.json")).unwrap(),
        b"operator JSON bytes"
    );
    assert_eq!(
        inspect_thunderstore_startup(&fixture.target())
            .unwrap()
            .packages[0]
            .version,
        "2.0.0"
    );
}

#[test]
fn copy_window_operator_change_rejects_publication_and_never_authorizes_cached_start() {
    let fixture = Fixture::new();
    fixture.install(&[fixture.source(
        "Author-Plugin",
        "1.0.0",
        Some(ThunderstoreInstallIntent::Root { version: None }),
        &[],
    )]);
    let proof = inspect_thunderstore_startup(&fixture.target()).unwrap();
    let next = fixture.source(
        "Author-Plugin",
        "2.0.0",
        Some(ThunderstoreInstallIntent::Root { version: None }),
        &[],
    );
    let entered = std::cell::Cell::new(false);
    let before = || {
        entered.set(true);
        // Deterministic injection at the actual checked-staging boundary,
        // after copying sources and immediately before commit.
        fs::write(
            fixture.plugin().join("Config/late.ini"),
            b"late operator data",
        )
        .unwrap();
        proof.verify_unchanged(&fixture.target())
    };
    assert!(
        stage_online_mod_sources_checked(fixture.target(), &[next], Some(&before), None).is_err()
    );
    assert!(
        entered.get(),
        "startup adapter rejected the package before the copy-window injection"
    );
    assert_eq!(
        fs::read(fixture.plugin().join("Config/late.ini")).unwrap(),
        b"late operator data"
    );
    assert_eq!(
        fs::read(fixture.plugin().join("Plugin.dll")).unwrap(),
        b"MZ1.0.0"
    );
    assert!(proof.verify_unchanged(&fixture.target()).is_err());
}

#[test]
fn cancelling_before_publication_keeps_payload_and_ownership_bytes() {
    let fixture = Fixture::new();
    fixture.install(&[fixture.source(
        "Author-Plugin",
        "1.0.0",
        Some(ThunderstoreInstallIntent::Root { version: None }),
        &[],
    )]);
    let target = fixture.target();
    let before = fingerprint_tree(&target.target_path).unwrap();
    let proof = inspect_thunderstore_startup(&target).unwrap();
    let cancellation = app_steamcmd::InstallCancellation::new();
    cancellation.cancel();
    let next = fixture.source(
        "Author-Plugin",
        "2.0.0",
        Some(ThunderstoreInstallIntent::Root { version: None }),
        &[],
    );
    assert!(
        crate::commands::commands_mod_staging::stage_thunderstore_online_mod_sources_for_startup(
            target.clone(),
            &[next],
            &proof,
            &cancellation
        )
        .is_err()
    );
    assert_eq!(fingerprint_tree(&target.target_path).unwrap(), before);
}

#[test]
fn conflicting_dependency_pin_rolls_back_entire_record_and_payload_batch() {
    let fixture = Fixture::new();
    fixture.install(&[fixture.source(
        "Author-Plugin",
        "1.0.0",
        Some(ThunderstoreInstallIntent::Root {
            version: Some("1.0.0".into()),
        }),
        &[],
    )]);
    let before = fingerprint_tree(&fixture.target().target_path).unwrap();
    let incoming = fixture.source(
        "Author-Plugin",
        "2.0.0",
        Some(ThunderstoreInstallIntent::Dependency),
        &[],
    );
    assert!(stage_online_mod_sources(fixture.target(), &[incoming]).is_err());
    assert_eq!(
        fingerprint_tree(&fixture.target().target_path).unwrap(),
        before
    );
}

#[test]
fn post_rename_operator_change_retains_new_target_and_original_recovery_tree() {
    let fixture = Fixture::new();
    fixture.install(&[fixture.source(
        "Author-Plugin",
        "1.0.0",
        Some(ThunderstoreInstallIntent::Root { version: None }),
        &[],
    )]);
    let target = fixture.target();
    let proof = inspect_thunderstore_startup(&target).unwrap();
    let next = fixture.source(
        "Author-Plugin",
        "2.0.0",
        Some(ThunderstoreInstallIntent::Root { version: None }),
        &[],
    );
    let before = || proof.verify_unchanged(&target);
    let recovery = std::cell::RefCell::new(None);
    let entered = std::cell::Cell::new(false);
    let after = |backup: &Path, roots: &HashSet<PathBuf>| {
        entered.set(true);
        let original = backup.join("thunderstore-Author-Plugin/Config/user.ini");
        fs::write(&original, b"racing operator data").unwrap();
        recovery.replace(Some(original));
        proof.verify_backup(backup, &target.target_path, roots)
    };
    assert!(
        stage_online_mod_sources_checked(target.clone(), &[next], Some(&before), Some(&after))
            .is_err()
    );
    assert!(
        entered.get(),
        "startup adapter did not reach the moved-original guard"
    );
    assert_eq!(
        fs::read(recovery.borrow().as_ref().unwrap()).unwrap(),
        b"racing operator data"
    );
    assert_eq!(
        fs::read(fixture.plugin().join("Plugin.dll")).unwrap(),
        b"MZ2.0.0"
    );
    assert!(proof.verify_unchanged(&target).is_err());
}

#[test]
fn missing_original_ownership_record_prevents_cleanup_and_cached_start_authorization() {
    let fixture = Fixture::new();
    fixture.install(&[fixture.source(
        "Author-Plugin",
        "1.0.0",
        Some(ThunderstoreInstallIntent::Root { version: None }),
        &[],
    )]);
    let target = fixture.target();
    let proof = inspect_thunderstore_startup(&target).unwrap();
    let next = fixture.source(
        "Author-Plugin",
        "2.0.0",
        Some(ThunderstoreInstallIntent::Root { version: None }),
        &[],
    );
    let before = || proof.verify_unchanged(&target);
    let recovery = std::cell::RefCell::new(None);
    let entered = std::cell::Cell::new(false);
    let after = |backup: &Path, roots: &HashSet<PathBuf>| {
        entered.set(true);
        fs::remove_file(backup.join(ONLINE_MOD_MANIFEST)).unwrap();
        recovery.replace(Some(backup.to_path_buf()));
        proof.verify_backup(backup, &target.target_path, roots)
    };
    assert!(
        stage_online_mod_sources_checked(target.clone(), &[next], Some(&before), Some(&after))
            .is_err()
    );
    assert!(
        entered.get(),
        "startup adapter did not reach the moved-original guard"
    );
    assert_eq!(
        fs::read(
            recovery
                .borrow()
                .as_ref()
                .unwrap()
                .join("thunderstore-Author-Plugin/Plugin.dll")
        )
        .unwrap(),
        b"MZ1.0.0"
    );
    assert_eq!(
        fs::read(fixture.plugin().join("Plugin.dll")).unwrap(),
        b"MZ2.0.0"
    );
    assert!(proof.verify_unchanged(&target).is_err());
}
