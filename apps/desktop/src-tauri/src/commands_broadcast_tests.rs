use super::*;

#[test]
fn bundled_dst_broadcast_quotes_lua_data_and_requires_master() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../modules");
    let modules = discover_modules(&root).unwrap();
    let descriptor = find_descriptor(&modules, "dontstarve").unwrap();
    let action = find_broadcast_action(descriptor).unwrap();
    assert_eq!(action.transport, "stdin");
    assert_eq!(action.process_key.as_deref(), Some("master"));
    assert_eq!(action.target_encoding.as_deref(), Some("quoted_string"));
    let message = validate_instance_broadcast_message(r#"集合到 "东门" \ 营地"#).unwrap();
    assert_eq!(
        render_broadcast_command(action, &message).unwrap(),
        r#"c_announce("集合到 \"东门\" \\ 营地")"#
    );
    for unsafe_message in [
        "hello\nworld",
        "hello\0world",
        "x; c_shutdown()",
        "{{target}}",
    ] {
        assert!(validate_instance_broadcast_message(unsafe_message).is_err());
    }
}

#[test]
fn assistant_broadcast_message_distinguishes_pending_from_transport_confirmation() {
    let mut output = SendInstanceBroadcastOutput {
        event: serde_json::from_value(json!({
            "event_id": "broadcast-fixture", "instance_id": "fixture", "module_id": "dontstarve",
            "source": "manual", "message": "hello", "status": "pending", "created_at_unix_ms": 0,
        }))
        .unwrap(),
        action_id: "broadcast".into(),
        transport: "stdin".into(),
        command_preview: "c_announce(\"hello\")".into(),
    };
    let pending = broadcast_dispatch_message(&output);
    assert!(pending.contains("confirmation is pending"));
    assert!(!pending.contains("Broadcast sent"));
    output.event.status = "sent".into();
    let confirmed = broadcast_dispatch_message(&output);
    assert!(confirmed.contains("Broadcast sent"));
    assert!(confirmed.contains("Game delivery is not independently confirmed"));
}
