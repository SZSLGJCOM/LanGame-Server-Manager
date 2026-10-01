use super::*;

const NONCE: &str = "1234567890abcdef1234567890abcdef";
const EMPTY: &str = include_str!("../../test-data/live-players/returntomoria/empty.txt");
const NORMAL: &str = include_str!("../../test-data/live-players/returntomoria/normal.txt");

#[test]
fn accepts_correlated_native_empty_response() {
    let result = parse("moria", NONCE, 100, EMPTY).unwrap();
    assert_eq!(result.public_snapshot.current_players, Some(0));
    assert_eq!(result.public_snapshot.max_players, Some(8));
    assert!(result.public_snapshot.complete);
    assert!(result.public_snapshot.entries.is_empty());
}

#[test]
fn preserves_native_labels_and_duplicate_names_without_account_or_action_bindings() {
    let result = parse("moria", NONCE, 100, NORMAL).unwrap();
    let rows = &result.public_snapshot.entries;
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].display_name, "Dwarf (opaque-one)");
    assert_eq!(rows[1].display_name, "Dwarf (opaque-two)");
    assert_eq!(rows[2].display_name, "矮人 (矿工) (unverified-token)");
    assert!(
        rows.iter()
            .all(|row| row.identifiers.is_empty() && row.available_action_ids.is_empty())
    );
    assert!(result.private_action_bindings.is_empty());
    assert_ne!(rows[0].player_key, rows[1].player_key);
}

#[test]
fn rejects_stale_missing_and_interleaved_frames() {
    for invalid in [
        EMPTY.replace(NONCE, "ffffffffffffffffffffffffffffffff"),
        EMPTY.replace("> players\n", ""),
        EMPTY.replace("Players: 0/8", "Players: 1/8"),
        EMPTY.replace("Players: 0/8", "Players: 0/16"),
        EMPTY.trim_end().to_owned(),
        EMPTY.replace("Players: 0/8", "Players: 0/8\n> status"),
        format!("{EMPTY}{EMPTY}"),
        EMPTY.replace("Players: 0/8", "Players: 00/8"),
    ] {
        assert!(parse("moria", NONCE, 100, &invalid).is_err(), "{invalid}");
    }
}

#[test]
fn refuses_partially_connected_and_unescaped_line_breaks() {
    for invalid in [
        NORMAL.replace("Dwarf (opaque-one)", "Not all players are fully connected"),
        NORMAL.replace("Dwarf (opaque-two)", "Dwarf\n * Injected (opaque-two)"),
        NORMAL.replace("Dwarf (opaque-one)", "Dwarf\t(opaque-one)"),
        NORMAL.replace("Dwarf (opaque-one)", "Dwarf ()"),
    ] {
        assert!(parse("moria", NONCE, 100, &invalid).is_err(), "{invalid}");
    }
}
