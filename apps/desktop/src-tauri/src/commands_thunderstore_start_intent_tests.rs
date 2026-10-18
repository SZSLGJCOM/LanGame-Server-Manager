use super::*;

fn package(intent: Option<ThunderstoreInstallIntent>, version: &str) -> InstalledPackage {
    InstalledPackage {
        thunderstore_intent: intent,
        target_name: "thunderstore-Author-Plugin".into(),
        files: BTreeMap::new(),
        routed_files: BTreeMap::new(),
        version: Some(version.into()),
        dependencies: Some(Vec::new()),
        enabled: true,
    }
}

#[test]
fn explicit_ui_policy_separates_download_version_from_permanent_pin() {
    let references = vec!["https://thunderstore.io/package/Author/Plugin/1.0.0/".into()];
    let key = "thunderstore-Author-Plugin";
    assert_eq!(
        root_thunderstore_intents_with_policy(&references, Some("follow_latest")).unwrap()[key],
        ThunderstoreInstallIntent::Root { version: None }
    );
    for policy in [None, Some("pinned")] {
        assert_eq!(
            root_thunderstore_intents_with_policy(&references, policy).unwrap()[key],
            ThunderstoreInstallIntent::Root {
                version: Some("1.0.0".into())
            }
        );
    }
    let unversioned = vec!["https://thunderstore.io/package/Author/Plugin/".into()];
    assert!(root_thunderstore_intents_with_policy(&unversioned, Some("pinned")).is_err());
    assert!(root_thunderstore_intents_with_policy(&references, Some("not_a_policy")).is_err());
}

#[test]
fn dependencies_do_not_demote_roots_unpin_them_or_migrate_unknown_intent() {
    let pinned = package(
        Some(ThunderstoreInstallIntent::Root {
            version: Some("1.0.0".into()),
        }),
        "1.0.0",
    );
    assert_eq!(
        retained_intent(
            Some(&pinned),
            Some(&ThunderstoreInstallIntent::Dependency),
            "1.0.0"
        )
        .unwrap(),
        pinned.thunderstore_intent
    );
    assert!(
        retained_intent(
            Some(&pinned),
            Some(&ThunderstoreInstallIntent::Dependency),
            "2.0.0"
        )
        .is_err()
    );
    let unknown = package(None, "1.0.0");
    assert_eq!(
        retained_intent(
            Some(&unknown),
            Some(&ThunderstoreInstallIntent::Dependency),
            "1.0.0"
        )
        .unwrap(),
        None
    );
    assert!(
        retained_intent(
            Some(&unknown),
            Some(&ThunderstoreInstallIntent::Dependency),
            "2.0.0"
        )
        .is_err()
    );
    assert_eq!(
        retained_intent(None, Some(&ThunderstoreInstallIntent::Dependency), "1.0.0").unwrap(),
        Some(ThunderstoreInstallIntent::Dependency)
    );
    assert_eq!(
        retained_intent(
            Some(&pinned),
            Some(&ThunderstoreInstallIntent::Root { version: None }),
            "2.0.0"
        )
        .unwrap(),
        Some(ThunderstoreInstallIntent::Root { version: None })
    );
}

#[test]
fn malformed_cross_provider_or_conflicting_pins_are_rejected() {
    let pinned = package(
        Some(ThunderstoreInstallIntent::Root {
            version: Some("1.0.0".into()),
        }),
        "2.0.0",
    );
    assert!(validate_thunderstore_intent("thunderstore-Author-Plugin", &pinned).is_err());
    assert!(validate_thunderstore_intent("modrinth-Author", &pinned).is_err());
    assert!(
        root_thunderstore_intents(&[
            "https://thunderstore.io/package/Author/Plugin/1.0.0/".into(),
            "https://thunderstore.io/package/Author/Plugin/2.0.0/".into(),
        ])
        .is_err()
    );
    for key in [
        "thunderstore-Author-Plugin-version",
        "thunderstore-../Plugin",
        "thunderstore-Author-Plugin?token=x",
    ] {
        assert!(thunderstore_project(key).is_err());
    }
}
