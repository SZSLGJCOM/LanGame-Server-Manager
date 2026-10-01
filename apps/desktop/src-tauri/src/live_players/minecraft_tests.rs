use super::*;

const NORMAL: &str = include_str!("../../test-data/live-players/minecraft/normal.txt");
const EMPTY: &str = include_str!("../../test-data/live-players/minecraft/empty.txt");

#[test]
fn minecraft_players_preserve_server_names_uuids_and_counts_without_authorizing_name_actions() {
    let result =
        parse_minecraft("instance", NORMAL, "request", 42).expect("valid package contract");
    assert_eq!(result.public_snapshot.current_players, Some(2));
    assert_eq!(result.public_snapshot.max_players, Some(20));
    assert!(result.public_snapshot.complete);
    assert_eq!(result.public_snapshot.entries[0].display_name, "Builder");
    assert_eq!(
        result.public_snapshot.entries[0].identifiers[0].value,
        "00000000-0000-4000-8000-000000000001"
    );
    assert_eq!(
        result.public_snapshot.entries[0].identifiers[0].kind,
        RuntimePlayerIdentityKind::MinecraftUuid
    );
    assert!(result.private_action_bindings.is_empty());
    assert!(
        result
            .public_snapshot
            .entries
            .iter()
            .all(|entry| entry.available_action_ids.is_empty())
    );
}

#[test]
fn minecraft_empty_list_is_a_success_but_absent_or_incomplete_response_is_not() {
    let result = parse_minecraft("instance", EMPTY, "request", 42).expect("empty contract");
    assert_eq!(result.public_snapshot.current_players, Some(0));
    assert!(result.public_snapshot.entries.is_empty());
    assert!(result.public_snapshot.complete);
    for response in [
        "",
        "Unknown command",
        "There are 2 of a max of 20 players online: Builder, Explorer",
        "There are 1 of a max of 20 players online:",
        "There are 0 of a max of 20 players online: Builder (00000000-0000-4000-8000-000000000001)",
        "There are 1 of a max of 20 players online: Builder (not-a-uuid)",
        "There are 1 of a max of 20 players online: Builder (00000000-0000-0000-0000-000000000000)",
        "There are 1 of a max of 20 players online: Builder (00000000-0000-4000-8000-000000000001)
another console response",
    ] {
        let error = parse_minecraft("instance", response, "request", 42).expect_err(response);
        assert!(!error.complete);
        assert!(error.entries.is_empty());
    }
}

#[test]
fn minecraft_rejects_duplicate_identities_and_response_overflow() {
    let duplicate = NORMAL.replace("000000000002", "000000000001");
    assert!(parse_minecraft("instance", &duplicate, "request", 42).is_err());
    for response in [
        "x".repeat(MAX_RESPONSE_BYTES + 1),
        format!(
            "There are {} of a max of 2048 players online:",
            MAX_PLAYERS + 1
        ),
    ] {
        let error = parse_minecraft("instance", &response, "request", 42).expect_err("bounded");
        assert!(error.truncated);
        assert_eq!(
            error.issue.expect("limit issue").code,
            RuntimeLivePlayerIssueCode::CaptureLimit
        );
    }
}

#[test]
fn minecraft_does_not_assume_online_count_is_below_configured_capacity() {
    // An operator can lower capacity while existing players remain connected.
    let result = parse_minecraft(
        "instance",
        &NORMAL.replace("max of 20", "max of 1"),
        "request",
        42,
    )
    .expect("observed players remain authoritative");
    assert_eq!(result.public_snapshot.current_players, Some(2));
    assert_eq!(result.public_snapshot.max_players, Some(1));
}
