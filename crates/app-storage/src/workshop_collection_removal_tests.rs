use super::*;
use serde_json::json;

fn settings() -> Value {
    json!({"operator_setting": "keep", "steam_workshop_collections": [
        {"id": "111111", "title": "Selected", "member_ids": ["222222", "333333", "555555"]},
        {"id": "444444", "title": "Other", "member_ids": ["333333"]}
    ]})
}

#[test]
fn requested_subset_preserves_shared_and_unselected_members() {
    let original = settings();
    let mut replacement = original.clone();
    replacement[FIELD].as_array_mut().unwrap().remove(0);
    assert_eq!(
        validate_removal(&original, &replacement, "111111", &["222222".into()], false).unwrap(),
        vec!["222222"]
    );
    for members in [
        vec!["333333".into()],
        vec!["666666".into()],
        vec!["222222".into(), "222222".into()],
        vec![],
        vec!["../222222".into()],
    ] {
        assert!(validate_removal(&original, &replacement, "111111", &members, false).is_err());
    }
}

#[test]
fn cannot_change_other_settings_records_or_remove_with_unknown_overlap() {
    let original = settings();
    let mut replacement = original.clone();
    replacement[FIELD].as_array_mut().unwrap().remove(0);
    replacement["operator_setting"] = json!("changed");
    assert!(
        validate_removal(&original, &replacement, "111111", &["222222".into()], false).is_err()
    );
    let mut original = original;
    original[FIELD][1]["member_ids"] = json!([]);
    replacement = original.clone();
    replacement[FIELD].as_array_mut().unwrap().remove(0);
    assert!(
        validate_removal(&original, &replacement, "111111", &["222222".into()], false).is_err()
    );
}

#[test]
fn explicit_member_removal_retains_every_snapshot_and_accepts_shared_members() {
    let original = settings();
    for id in ["222222", "333333"] {
        assert_eq!(
            validate_removal(&original, &original, "111111", &[id.into()], true).unwrap(),
            vec![id]
        );
    }
    for members in [
        vec![],
        vec!["666666".into()],
        vec!["222222".into(), "555555".into()],
    ] {
        assert!(validate_removal(&original, &original, "111111", &members, true).is_err());
    }
    let mut changed = original.clone();
    changed[FIELD].as_array_mut().unwrap().remove(0);
    assert!(validate_removal(&original, &changed, "111111", &["222222".into()], true).is_err());
    changed = original.clone();
    changed["operator_setting"] = json!("changed");
    assert!(validate_removal(&original, &changed, "111111", &["222222".into()], true).is_err());
    let mut unknown = original;
    unknown[FIELD][1]["member_ids"] = json!([]);
    assert!(validate_removal(&unknown, &unknown, "111111", &["222222".into()], true).is_ok());
}
