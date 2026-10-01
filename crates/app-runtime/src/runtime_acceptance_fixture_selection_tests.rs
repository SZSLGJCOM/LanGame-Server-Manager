use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::config_acceptance_test_support::{
    discover_fixture_paths, repository_root, resolve_fixture_selection,
};

#[test]
fn fixture_selection_defaults_to_all_and_rejects_explicit_empty_values() {
    let known = BTreeSet::from([String::from("minecraft"), String::from("rust")]);
    let fixtures = BTreeMap::from([
        (
            String::from("minecraft"),
            vec![PathBuf::from("minecraft.json")],
        ),
        (String::from("rust"), vec![PathBuf::from("rust.json")]),
    ]);

    assert_eq!(
        resolve_fixture_selection(&known, &fixtures, None).unwrap(),
        vec![String::from("minecraft"), String::from("rust")]
    );
    for requested in ["", "   ", ", ,"] {
        assert!(
            resolve_fixture_selection(&known, &fixtures, Some(requested))
                .unwrap_err()
                .contains("must not be empty")
        );
    }
}

#[test]
fn fixture_selection_without_target_rejects_fixtureless_modules() {
    let known = BTreeSet::from([String::from("minecraft"), String::from("rust")]);
    let fixtures = BTreeMap::from([(
        String::from("minecraft"),
        vec![PathBuf::from("minecraft.json")],
    )]);

    let error = resolve_fixture_selection(&known, &fixtures, None).unwrap_err();

    assert!(error.contains("rust"));
    assert!(error.contains("no fixtures"));
}

#[test]
fn fixture_selection_without_target_rejects_unknown_fixture_modules() {
    let known = BTreeSet::from([String::from("minecraft")]);
    let fixtures = BTreeMap::from([
        (
            String::from("minecraft"),
            vec![PathBuf::from("minecraft.json")],
        ),
        (String::from("ghost"), vec![PathBuf::from("ghost.json")]),
    ]);

    let error = resolve_fixture_selection(&known, &fixtures, None).unwrap_err();

    assert!(error.contains("ghost"));
    assert!(error.contains("unknown modules"));
}

#[test]
fn fixture_selection_rejects_duplicate_unknown_and_fixtureless_ids() {
    let known = BTreeSet::from([String::from("minecraft"), String::from("rust")]);
    let fixtures = BTreeMap::from([(
        String::from("minecraft"),
        vec![PathBuf::from("minecraft.json")],
    )]);

    assert_eq!(
        resolve_fixture_selection(&known, &fixtures, Some("minecraft")).unwrap(),
        vec![String::from("minecraft")]
    );
    assert!(
        resolve_fixture_selection(&known, &fixtures, Some("minecraft,minecraft"))
            .unwrap_err()
            .contains("duplicate")
    );
    assert!(
        resolve_fixture_selection(&known, &fixtures, Some("unknown"))
            .unwrap_err()
            .contains("unknown")
    );
    assert!(
        resolve_fixture_selection(&known, &fixtures, Some("rust"))
            .unwrap_err()
            .contains("no fixtures")
    );
}

#[test]
fn repository_discovery_requires_runtime_fixture_for_every_module() {
    let modules = app_modules::discover_modules(repository_root().join("modules"))
        .expect("discover repository modules");
    let known = modules
        .iter()
        .map(|module| module.summary.id.clone())
        .collect::<BTreeSet<_>>();
    let fixtures = discover_fixture_paths(&modules).expect("discover repository fixtures");

    assert_eq!(known.len(), 32);
    assert_eq!(
        fixtures.keys().cloned().collect::<BTreeSet<_>>(),
        known,
        "every discovered module must have runtime acceptance fixtures"
    );
    assert_eq!(
        resolve_fixture_selection(&known, &fixtures, None).unwrap(),
        fixtures.keys().cloned().collect::<Vec<_>>()
    );
}
