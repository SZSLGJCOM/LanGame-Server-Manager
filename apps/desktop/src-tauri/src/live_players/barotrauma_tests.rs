use super::*;

#[test]
fn stale_response_nonce_cannot_satisfy_current_request() {
    let lines = [
        "clientlist LGM_PLAYER_QUERY_previous",
        DELIMITER,
        "- 1: Old, endpoint, account, ping 1 ms",
        DELIMITER,
        "clientlist LGM_PLAYER_QUERY_request",
        DELIMITER,
        DELIMITER,
    ];
    assert!(parse(&lines[..4], "request").is_err());
    let parsed = parse(&lines, "request").unwrap();
    assert_eq!(parsed.current_players, Some(0));
    assert!(parsed.entries.is_empty());
}

#[test]
fn empty_response_requires_command_echo_and_both_delimiters() {
    // Shape verified with an isolated Barotrauma 1.13.4.0 server using ConPTY.
    let parsed = parse(
        &["clientlist LGM_PLAYER_QUERY_request", DELIMITER, DELIMITER],
        "request",
    )
    .unwrap();
    assert!(parsed.complete);
    assert_eq!(parsed.current_players, Some(0));
    for lines in [
        vec![],
        vec![DELIMITER, DELIMITER],
        vec!["clientlist LGM_PLAYER_QUERY_request", DELIMITER],
        vec!["clientlist LGM_PLAYER_QUERY_request", DELIMITER, "waiting"],
    ] {
        assert!(parse(&lines, "request").is_err());
    }
}

#[test]
fn source_derived_player_rows_preserve_labels_without_action_targets() {
    // Synthetic rows from official DebugConsole source, not a real-client capture.
    let parsed = parse(
        &[
            "clientlist LGM_PLAYER_QUERY_request",
            DELIMITER,
            "- 1: 玩家, One playing Engineer, 127.0.0.1:30000, account-one, ping 12 ms",
            "- 2: Name playing Literal, 127.0.0.1:30001, account-two, ping -1 ms",
            DELIMITER,
        ],
        "request",
    )
    .unwrap();
    assert!(parsed.complete);
    assert_eq!(parsed.current_players, Some(2));
    assert_eq!(parsed.entries[0].display_name, "玩家, One playing Engineer");
    assert_eq!(parsed.entries[1].display_name, "Name playing Literal");
    assert_eq!(parsed.entries[0].identifiers[0].value, "1");
    assert_eq!(parsed.entries[0].ping_ms, Some(12));
    assert_eq!(parsed.entries[1].ping_ms, None);
    assert!(
        parsed
            .entries
            .iter()
            .all(|entry| entry.available_action_ids.is_empty())
    );
    assert!(parsed.bindings.is_empty());
}

#[test]
fn partial_malformed_duplicate_or_overlapping_frames_are_not_lists() {
    let row = "- 1: Name, endpoint, account, ping 12 ms";
    for lines in [
        vec!["clientlist LGM_PLAYER_QUERY_request", DELIMITER, row],
        vec![
            "clientlist LGM_PLAYER_QUERY_request",
            DELIMITER,
            row,
            row,
            DELIMITER,
        ],
        vec![
            "clientlist LGM_PLAYER_QUERY_request",
            DELIMITER,
            "unrelated log line",
            DELIMITER,
        ],
        vec![
            "clientlist LGM_PLAYER_QUERY_request",
            DELIMITER,
            "- 1: Name, endpoint, ping 12 ms",
            DELIMITER,
        ],
        vec![
            "clientlist LGM_PLAYER_QUERY_request",
            DELIMITER,
            "- 1: Name, endpoint, account, ping x ms",
            DELIMITER,
        ],
        vec![
            "clientlist LGM_PLAYER_QUERY_request",
            DELIMITER,
            DELIMITER,
            "clientlist LGM_PLAYER_QUERY_request",
            DELIMITER,
            DELIMITER,
        ],
    ] {
        assert!(parse(&lines, "request").is_err(), "accepted {lines:?}");
    }
}
