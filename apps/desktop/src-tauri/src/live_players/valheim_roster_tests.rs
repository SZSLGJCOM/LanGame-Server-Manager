#![cfg(test)]
use super::*;

fn event(peers: &mut ValheimPeers, roster: &mut NameRoster, text: &str) {
    peers.apply(roster, &format!("10/16/2026 20:55:39: {text}"));
}
fn join(peers: &mut ValheimPeers, roster: &mut NameRoster, name: &str, owner: i64) {
    event(
        peers,
        roster,
        "Server: New peer connected,sending global keys",
    );
    event(
        peers,
        roster,
        &format!("Got character ZDOID from {name} : {owner}:1"),
    );
}

#[test]
fn current_session_character_names_ignore_save_history_and_unrelated_messages() {
    let (mut peers, mut roster) = (ValheimPeers::default(), NameRoster::default());
    event(
        &mut peers,
        &mut roster,
        "Player history entry with index 0: Historical (Steam_123, abc)",
    );
    event(
        &mut peers,
        &mut roster,
        "Chat: Got character ZDOID from Forged : 99:1",
    );
    join(&mut peers, &mut roster, "玩家 : O'Brien", 42);
    assert_eq!(roster.names.get("玩家 : O'Brien"), Some(&1));
    assert_eq!(roster.count(), 1);
    assert!(peers.is_stable());
    assert!(!roster.incomplete);
}

#[test]
fn duplicate_character_events_and_respawns_do_not_duplicate_online_peers() {
    let (mut peers, mut roster) = (ValheimPeers::default(), NameRoster::default());
    join(&mut peers, &mut roster, "Same", 42);
    event(
        &mut peers,
        &mut roster,
        "Got character ZDOID from Same : 42:1",
    );
    event(
        &mut peers,
        &mut roster,
        "Got character ZDOID from Same : 0:0",
    );
    event(
        &mut peers,
        &mut roster,
        "Got character ZDOID from Same : 42:2",
    );
    assert_eq!(roster.count(), 1);
    assert!(peers.is_stable());
    join(&mut peers, &mut roster, "Same", 43);
    assert_eq!(roster.names.get("Same"), Some(&2));
}

#[test]
fn departure_uses_actual_disconnected_owner_not_zdo_creator_or_duplicate_names() {
    let (mut peers, mut roster) = (ValheimPeers::default(), NameRoster::default());
    join(&mut peers, &mut roster, "Same", 42);
    join(&mut peers, &mut roster, "Same", 43);
    event(&mut peers, &mut roster, "RPC_Disconnect");
    assert!(!peers.is_stable());
    event(
        &mut peers,
        &mut roster,
        "Destroying abandoned non persistent zdo 43:1 owner 42",
    );
    event(
        &mut peers,
        &mut roster,
        "Destroying abandoned non persistent zdo 42:2 owner 42",
    );
    event(&mut peers, &mut roster, "Closing socket 76561198000000001");
    assert!(peers.is_stable());
    assert!(!roster.incomplete);
    assert_eq!(roster.names.get("Same"), Some(&1));
    assert!(peers.names_by_owner.contains_key(&43));
}

#[test]
fn unknown_departures_never_remove_an_arbitrary_named_player() {
    let (mut peers, mut roster) = (ValheimPeers::default(), NameRoster::default());
    join(&mut peers, &mut roster, "Known", 42);
    event(&mut peers, &mut roster, "RPC_Disconnect");
    event(
        &mut peers,
        &mut roster,
        "Destroying abandoned non persistent zdo 99:1 owner 99",
    );
    assert!(!peers.is_stable());
    assert_eq!(roster.names.get("Known"), Some(&1));
    event(&mut peers, &mut roster, "Closing socket 76561198000000001");
    assert!(roster.incomplete);
    assert!(roster.names.is_empty());
}

#[test]
fn malformed_or_invalid_native_names_invalidate_membership() {
    for text in [
        "Got character ZDOID from Broken : nope:1",
        "Got character ZDOID from  : 43:1",
        "Got character ZDOID from Broken� : 43:1",
        "Destroying abandoned non persistent zdo 42:1 owner nope",
    ] {
        let (mut peers, mut roster) = (ValheimPeers::default(), NameRoster::default());
        join(&mut peers, &mut roster, "Old", 42);
        event(&mut peers, &mut roster, text);
        assert!(roster.incomplete, "{text}");
        assert!(roster.names.is_empty());
    }
}
