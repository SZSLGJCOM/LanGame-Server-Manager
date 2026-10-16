#![cfg(test)]
use super::*;

fn join(roster: &mut NameRoster, name: &str) {
    roster.apply(&format!(
        "[server] Player '{name}' logged in with Permissions:"
    ));
}

#[test]
fn native_join_and_remove_update_only_the_named_membership() {
    let mut roster = NameRoster::default();
    roster.apply("[server] Machine '1': Player '0(0)' logged in");
    join(&mut roster, "测试玩家");
    roster.apply("[server] Remove Entity for Player '测试玩家'");
    roster.apply("[server] Send Shutdown Message to Player '测试玩家'");
    assert_eq!(roster.names.get("测试玩家"), Some(&1));
    roster.apply("[server] Remove Player '测试玩家'");
    assert_eq!(roster.count(), 0);
    assert!(!roster.incomplete);
}

#[test]
fn unchanged_count_does_not_keep_a_departed_name() {
    let mut roster = NameRoster::default();
    join(&mut roster, "First");
    roster.apply("[server] Remove Player 'First'");
    join(&mut roster, "Second");
    assert_eq!(roster.count(), 1);
    assert!(!roster.names.contains_key("First"));
    assert_eq!(roster.names.get("Second"), Some(&1));
}

#[test]
fn same_names_are_a_multiset_not_account_identities() {
    let mut roster = NameRoster::default();
    join(&mut roster, "Same");
    join(&mut roster, "Same");
    assert_eq!(roster.names.get("Same"), Some(&2));
    roster.apply("[server] Remove Player 'Same'");
    assert_eq!(roster.names.get("Same"), Some(&1));
    assert!(!roster.incomplete);
}

#[test]
fn interleaved_machine_lines_do_not_assign_names_to_machine_ids() {
    let mut roster = NameRoster::default();
    roster.apply("[server] Machine '1': Player '0(0)' logged in");
    roster.apply("[server] Machine '2': Player '1(0)' logged in");
    join(&mut roster, "Second");
    join(&mut roster, "First");
    assert_eq!(roster.count(), 2);
    assert!(!roster.incomplete);
}

#[test]
fn unknown_departure_and_invalid_names_break_completeness() {
    for event in [
        "[server] Remove Player 'Unknown'",
        "[server] Player ' ' logged in with Permissions:",
        "[server] Player 'Broken�' logged in with Permissions:",
        "[server] Remove Player missing-quotes",
    ] {
        let mut roster = NameRoster::default();
        join(&mut roster, "Old");
        roster.apply(event);
        assert!(roster.incomplete, "{event}");
        assert!(roster.names.is_empty());
    }
}

#[test]
fn apostrophes_and_unicode_are_not_split_into_synthetic_players() {
    let mut roster = NameRoster::default();
    join(&mut roster, "玩家 O'Brien");
    assert_eq!(roster.names.get("玩家 O'Brien"), Some(&1));
    roster.apply("[server] Remove Player '玩家 O'Brien'");
    assert_eq!(roster.count(), 0);
}
