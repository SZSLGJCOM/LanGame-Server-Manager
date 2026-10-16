use super::*;
use serde_json::json;

fn settings(ids: &str) -> Map<String, Value> {
    json!({"banned_player_ids": ids})
        .as_object()
        .unwrap()
        .clone()
}

#[test]
fn ordinary_saves_union_native_and_managed_ids_without_inferring_removal() {
    let persisted = settings("1\n18446744073709551615");
    let mut incoming = settings("3");
    reconcile_ids(
        "fixture",
        [2].into_iter(),
        &persisted,
        &mut incoming,
        None,
        None,
    )
    .unwrap();
    assert_eq!(incoming[SETTINGS_KEY], "1\n2\n3\n18446744073709551615");
}

#[test]
fn an_explicit_remove_affects_only_its_target() {
    let persisted = settings("1\n3");
    let mut incoming = settings("3");
    reconcile_ids(
        "fixture",
        [1, 2].into_iter(),
        &persisted,
        &mut incoming,
        None,
        Some(1),
    )
    .unwrap();
    assert_eq!(incoming[SETTINGS_KEY], "2\n3");
}

#[test]
fn stale_expected_roster_fails_without_changing_the_draft() {
    let persisted = settings("1");
    let mut incoming = settings("3");
    let original = incoming.clone();
    let error = reconcile_ids(
        "fixture",
        [2].into_iter(),
        &persisted,
        &mut incoming,
        Some(r#"{"banned_player_ids":"1"}"#),
        None,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        StorageError::InstanceSettingsPreconditionFailed { .. }
    ));
    assert_eq!(incoming, original);
}

#[test]
fn invalid_ids_fail_without_changing_the_draft() {
    let mut incoming = settings("18446744073709551616");
    let original = incoming.clone();
    assert!(
        reconcile_ids(
            "fixture",
            [].into_iter(),
            &settings("1"),
            &mut incoming,
            None,
            None
        )
        .is_err()
    );
    assert_eq!(incoming, original);
}

#[test]
fn records_retain_unknown_nested_metadata_and_reject_conflicting_values() {
    let mut current = json!({"future": {"native": true}, "identity": 1});
    merge_record(
        &mut current,
        json!({"future": {"managed": true}, "identity": 1}),
    )
    .unwrap();
    assert_eq!(
        current,
        json!({"future": {"native": true,"managed": true}, "identity": 1})
    );
    let original = current.clone();
    assert!(merge_record(&mut current, json!({"future": {"native": false}})).is_err());
    assert_eq!(current, original);
}

#[test]
fn managed_name_and_date_placeholders_cannot_erase_native_metadata() {
    let native = json!({"accountId": 1,"displayName":"native name","characterName":"character","banDate":{"value":123,"future":true}});
    let placeholder =
        json!({"accountId":1,"displayName":"","characterName":"","banDate":{"value":0}});
    let mut current = native.clone();
    merge_record(&mut current, placeholder.clone()).unwrap();
    assert_eq!(current, native);
    let mut current = placeholder;
    merge_record(&mut current, native.clone()).unwrap();
    assert_eq!(current, native);
}
