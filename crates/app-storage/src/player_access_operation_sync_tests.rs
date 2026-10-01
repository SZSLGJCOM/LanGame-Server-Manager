use super::*;
use serde_json::json;

fn blacklist_schema() -> Map<String, Value> {
    let schema: Value =
        serde_json::from_str(include_str!("../../../modules/sevendaystodie/schema.json"))
            .expect("7DTD schema");
    schema["properties"]["blacklist_entries"]
        .as_object()
        .expect("blacklist field")
        .clone()
}

#[test]
fn seven_days_blacklist_syncs_only_removal_and_consumes_its_runtime_action() {
    let sync = parse_sync_metadata("sevendaystodie", "blacklist_entries", &blacklist_schema())
        .expect("operation-specific sync");
    assert_eq!(sync.mode, PlayerAccessSyncMode::Direct);
    assert_eq!(
        sync.mutation_action_id(PlayerAccessMutationOperation::Add),
        None
    );
    assert_eq!(
        sync.mutation_action_id(PlayerAccessMutationOperation::Remove),
        Some("unban_player")
    );
    assert_eq!(sync.consume_action_ids, ["unban_player"]);
}

#[test]
fn partial_direct_sync_rejects_missing_empty_and_wrong_typed_action_ids() {
    for raw in [
        json!({"mode":"direct"}),
        json!({"mode":"direct","remove_action_id":""}),
        json!({"mode":"direct","remove_action_id":17}),
        json!({"mode":"direct","remove_action_id":"unban","action_id":"reload"}),
        json!({"mode":"direct","remove_action_id":"unban","add_action_id":null}),
    ] {
        let schema = json!({"x-lsgm-player-access-sync":raw});
        assert!(parse_sync_metadata("fixture", "bans", schema.as_object().unwrap()).is_err());
    }
    let schema = json!({"x-lsgm-player-access-sync":{"mode":"direct","add_action_id":"ban"}});
    let sync =
        parse_sync_metadata("fixture", "bans", schema.as_object().unwrap()).expect("add only");
    assert_eq!(
        sync.mutation_action_id(PlayerAccessMutationOperation::Add),
        Some("ban")
    );
    assert_eq!(
        sync.mutation_action_id(PlayerAccessMutationOperation::Remove),
        None
    );
}

#[test]
fn seven_days_live_target_retains_platform_and_removes_only_the_matching_account() {
    let schema = blacklist_schema();
    let steam = json!({"platform":"Steam","userid":"76561198000000001","name":"Offline",
        "unbandate":"2030-01-01","reason":"Stored reason"});
    let eos = json!({"platform":"EOS","userid":"76561198000000001","name":"Other account",
        "unbandate":"2031-01-01","reason":"Other reason"});
    for (entry, expected) in [
        (&steam, "Steam_76561198000000001"),
        (&eos, "EOS_76561198000000001"),
    ] {
        let normalized = normalize_entry(
            "sevendaystodie",
            "blacklist_entries",
            PlayerAccessCodec::ObjectIdentity,
            &schema,
            PlayerAccessMutationOperation::Remove,
            entry,
        )
        .expect("platform account");
        assert_eq!(normalized.live_target, expected);
        assert_eq!(&normalized.stored, entry);
        let patched = patch_field_value(
            "sevendaystodie",
            "blacklist_entries",
            PlayerAccessCodec::ObjectIdentity,
            &schema,
            PlayerAccessMutationOperation::Remove,
            &json!([steam, eos]),
            &normalized,
        )
        .expect("remove account");
        assert!(patched.changed);
        assert_eq!(patched.matched_live_target.as_deref(), Some(expected));
        assert_eq!(patched.value.as_array().unwrap().len(), 1);
        assert_ne!(patched.value[0]["platform"], entry["platform"]);
    }
}

#[test]
fn seven_days_platform_target_never_invents_identity_or_bypasses_metadata_validation() {
    let schema = blacklist_schema();
    for entry in [
        json!("76561198000000001"),
        json!({"userid":"76561198000000001"}),
        json!({"platform":"Steam"}),
        json!({"platform":"Unknown","userid":"76561198000000001"}),
        json!({"platform":"Steam","userid":"76561198000000001;quit"}),
        json!({"platform":"Steam","userid":"76561198000000001","reason":"unsafe\nvalue"}),
        json!({"platform":"Steam","userid":"76561198000000001","unbandate":"tomorrow"}),
        json!({"platform":"Steam","userid":"76561198000000001","name":17}),
    ] {
        assert!(
            normalize_entry(
                "sevendaystodie",
                "blacklist_entries",
                PlayerAccessCodec::ObjectIdentity,
                &schema,
                PlayerAccessMutationOperation::Remove,
                &entry
            )
            .is_err()
        );
    }
    let mut unsupported = schema.clone();
    unsupported.insert("x-lsgm-player-access-live-target".into(), json!("unknown"));
    assert!(
        normalize_entry(
            "sevendaystodie",
            "blacklist_entries",
            PlayerAccessCodec::ObjectIdentity,
            &unsupported,
            PlayerAccessMutationOperation::Remove,
            &json!({"platform":"Steam","userid":"76561198000000001"})
        )
        .is_err()
    );
}
