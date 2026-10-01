use super::*;

const NORMAL: &[u8] = include_bytes!("../../test-data/live-players/satisfactory/normal.json");
const EMPTY: &[u8] = include_bytes!("../../test-data/live-players/satisfactory/empty.json");

#[test]
fn frm_filters_offline_characters_and_never_advertises_actor_ids_as_accounts() {
    let result = parse_players("instance", "request", 42, NORMAL).expect("upstream fixture");
    assert_eq!(result.public_snapshot.current_players, Some(2));
    assert_eq!(result.public_snapshot.entries[1].display_name, "探索者");
    assert!(result.public_snapshot.complete);
    assert!(result.private_action_bindings.is_empty());
    assert!(
        result
            .public_snapshot
            .entries
            .iter()
            .all(|row| row.identifiers.is_empty() && row.available_action_ids.is_empty())
    );
}

#[test]
fn frm_accepts_only_explicit_complete_empty_results() {
    for body in [EMPTY, b"[]"] {
        let result = parse_players("instance", "request", 42, body).expect("empty fixture");
        assert_eq!(result.public_snapshot.current_players, Some(0));
        assert!(result.public_snapshot.complete);
    }
    for body in [
        "",
        "{}",
        "[",
        r#"[{"ID":"a","Name":"Builder"}]"#,
        r#"[{"ID":"a","Name":"Builder","Online":null}]"#,
        r#"{"errorCode":"unknown_function"}"#,
        r#"[{"ID":"a","Name":"","Online":true}]"#,
        r#"[{"ID":"a","Name":"A","Online":true},{"ID":"a","Name":"B","Online":true}]"#,
        r#"[{"ID":"a","Name":"A\u0000B","Online":true}]"#,
        r#"[{"ID":"a","Name":"A","Online":"true"}]"#,
    ] {
        let failure = parse_players("instance", "request", 42, body.as_bytes())
            .expect_err("invalid response");
        assert!(!failure.complete);
        assert_eq!(failure.current_players, None);
        assert!(failure.entries.is_empty());
    }
}

#[test]
fn frm_keeps_same_named_players_as_separate_read_only_rows() {
    let body = br#"[{"ID":"a","Name":"A","Online":true},{"ID":"b","Name":"A","Online":true}]"#;
    let result = parse_players("instance", "request", 42, body).expect("different actors");
    let rows = result.public_snapshot.entries;
    assert_eq!(rows.len(), 2);
    assert_ne!(rows[0].player_key, rows[1].player_key);
}

#[test]
fn frm_rejects_oversized_responses_without_returning_partial_names() {
    let response = vec![b' '; MAX_HTTP_RESPONSE_BYTES + 1];
    let failure =
        parse_players("instance", "request", 42, &response).expect_err("bounded response");
    assert_eq!(
        failure.issue.as_ref().map(|issue| issue.code),
        Some(RuntimeLivePlayerIssueCode::CaptureLimit)
    );
    assert!(failure.truncated);
    assert!(failure.entries.is_empty());
}
