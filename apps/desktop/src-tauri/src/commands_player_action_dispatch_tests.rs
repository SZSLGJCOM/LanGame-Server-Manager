use std::collections::HashMap;

use crate::live_players::cache::{
    CachedLivePlayerSnapshot, LivePlayerCacheKey, LivePlayerRegistry,
};
use app_core::{
    ModulePlayerListSource, RuntimeLivePlayerEntry, RuntimeLivePlayerSnapshot,
    RuntimeLivePlayerStatus,
};
use app_modules::discover_modules;

use super::super::commands_assistant_ops::runtime_action_rejection_audit;
use super::super::commands_runtime_actions::resolve_declared_runtime_action;

#[tokio::test(flavor = "current_thread")]
async fn player_action_revokes_snapshot_authorization_before_dispatch_is_polled() {
    let instance_id = "authorization-before-dispatch";
    let snapshot_id = "snapshot-a";
    let player_key = "player-a";
    let action_id = "kick";
    let canonical_target = "canonical-player-a";
    let registry = LivePlayerRegistry::default();
    let key = LivePlayerCacheKey {
        instance_id: instance_id.to_owned(),
        run_id: String::from("run-a"),
        security_contract_fingerprint: String::from("contract-a"),
        refresh_interval_ms: 30_000,
    };
    let collected_at = registry.now_unix_ms();
    registry.store_success(
        &key,
        CachedLivePlayerSnapshot {
            public_snapshot: RuntimeLivePlayerSnapshot {
                snapshot_id: snapshot_id.to_owned(),
                instance_id: instance_id.to_owned(),
                status: RuntimeLivePlayerStatus::Ready,
                source: Some(ModulePlayerListSource::StructuredLog),
                observed_at_unix_ms: Some(collected_at),
                expires_at_unix_ms: None,
                complete: true,
                truncated: false,
                stale: false,
                current_players: Some(1),
                max_players: Some(8),
                entries: vec![RuntimeLivePlayerEntry {
                    player_key: player_key.to_owned(),
                    display_name: String::from("Player A"),
                    identifiers: Vec::new(),
                    available_action_ids: vec![action_id.to_owned()],
                    ping_ms: None,
                    session_started_at_unix_ms: None,
                    role: None,
                    attributes: Vec::new(),
                }],
                issue: None,
            },
            private_action_bindings: HashMap::from([(
                (player_key.to_owned(), action_id.to_owned()),
                canonical_target.to_owned(),
            )]),
            collected_at,
        },
    );
    assert_eq!(
        registry.resolve_action_binding(&key, snapshot_id, player_key, action_id),
        Some(canonical_target.to_owned()),
    );

    let authorization_seen_by_dispatch =
        super::super::commands_live_players::dispatch_after_live_player_authorization_revocation(
            &registry,
            instance_id,
            async { registry.resolve_action_binding(&key, snapshot_id, player_key, action_id) },
        )
        .await;

    assert_eq!(authorization_seen_by_dispatch, None);
}

#[test]
fn redacted_declared_action_rejection_audit_omits_all_source_details() {
    let modules_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../modules");
    let descriptors = discover_modules(&modules_root).expect("discover repository modules");
    let descriptor = descriptors
        .iter()
        .find(|descriptor| descriptor.summary.id == "necesse")
        .expect("Necesse descriptor");
    let action = descriptor
        .runtime
        .player_actions
        .iter()
        .find(|action| action.id == "set_permission")
        .expect("declared Necesse permission action");
    let role_error = resolve_declared_runtime_action(
        descriptor,
        "set_permission",
        Some("TARGET_SECRET"),
        Some("MY_SECRET"),
        None,
        true,
    )
    .expect_err("an undeclared Necesse role must be rejected");
    let source_error = format!(
        "{role_error}; role=MY_SECRET; target=TARGET_SECRET; endpoint=ENDPOINT_SECRET; os=OS_SECRET; service=SERVICE_SECRET"
    );

    let audit =
        runtime_action_rejection_audit(true, "instance-a", "necesse", Some(action), &source_error);
    let emitted = format!(
        "{}\n{}\n{}",
        audit.message,
        serde_json::to_string(&audit.payload).expect("serialize audit payload"),
        audit.public_error,
    );

    assert_eq!(audit.payload["error_kind"], "semantic_validation_rejected");
    assert_eq!(audit.payload["module_id"], "necesse");
    assert_eq!(audit.payload["runtime_action_id"], "set_permission");
    for secret in [
        "MY_SECRET",
        "TARGET_SECRET",
        "ENDPOINT_SECRET",
        "OS_SECRET",
        "SERVICE_SECRET",
    ] {
        assert!(
            !emitted.contains(secret),
            "redacted runtime-action rejection exposed {secret}"
        );
    }

    let undeclared_error = resolve_declared_runtime_action(
        descriptor,
        "ACTION_ID_SECRET",
        Some("TARGET_SECRET"),
        Some("MY_SECRET"),
        None,
        true,
    )
    .expect_err("an undeclared action id must be rejected");
    let undeclared_audit =
        runtime_action_rejection_audit(true, "instance-a", "necesse", None, &undeclared_error);
    let undeclared_emitted = format!(
        "{}\n{}\n{}",
        undeclared_audit.message,
        serde_json::to_string(&undeclared_audit.payload).expect("serialize undeclared audit"),
        undeclared_audit.public_error,
    );
    assert_eq!(
        undeclared_audit.payload["runtime_action_id"],
        serde_json::Value::Null
    );
    assert!(!undeclared_emitted.contains("ACTION_ID_SECRET"));
}
