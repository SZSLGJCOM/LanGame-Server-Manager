#![cfg(test)]
use super::super::cache::CachedLivePlayerSnapshot;
use super::*;
use app_core::{ModulePlayerListSource, RuntimeLivePlayerSnapshot, RuntimeLivePlayerStatus};

struct Fixture {
    root: PathBuf,
    log: app_storage::managed_console_log::ManagedConsoleLog,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("lgsm-enshrouded-log-{}", uuid::Uuid::new_v4()));
        let log = app_storage::managed_console_log::ManagedConsoleLog::open(
            root.join("managed-console/run-1-main.log"),
        )
        .expect("owned managed log");
        Self { root, log }
    }
    fn tracker(&self) -> TrackedLog {
        self.tracker_for(Game::Enshrouded)
    }
    fn tracker_for(&self, game: Game) -> TrackedLog {
        TrackedLog::new(RunKey {
            game,
            run_id: 1,
            pid: 123,
            identity: Some((456, "enshrouded_server.exe".into())),
            path: self.root.join("managed-console/run-1-main.log"),
        })
    }
    fn write(&self, text: &str) {
        self.log
            .write_all(text.as_bytes())
            .expect("append native log");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).expect("remove owned log fixture");
    }
}

fn evidence(count: usize, known: &[&str]) -> A2sPlayerEvidence {
    A2sPlayerEvidence {
        returned_players: count,
        info: Some(app_storage::QueriedPlayerCount {
            current_players: count,
            max_players: 16,
        }),
        snapshot: CachedLivePlayerSnapshot {
            public_snapshot: RuntimeLivePlayerSnapshot {
                snapshot_id: "request".into(),
                instance_id: "instance".into(),
                status: RuntimeLivePlayerStatus::Ready,
                source: Some(ModulePlayerListSource::ServerQuery),
                observed_at_unix_ms: Some(1_000),
                expires_at_unix_ms: None,
                complete: known.len() == count,
                truncated: false,
                stale: false,
                current_players: Some(count),
                max_players: Some(16),
                entries: known
                    .iter()
                    .enumerate()
                    .map(|(index, name)| RuntimeLivePlayerEntry {
                        player_key: index.to_string(),
                        display_name: (*name).into(),
                        identifiers: Vec::new(),
                        available_action_ids: Vec::new(),
                        ping_ms: None,
                        session_started_at_unix_ms: Some(999),
                        role: None,
                        attributes: Vec::new(),
                    })
                    .collect(),
                issue: None,
            },
            private_action_bindings: HashMap::new(),
            collected_at: 1_000,
        },
    }
}

fn project(log: &mut TrackedLog, count: usize, known: &[&str]) -> A2sPlayerEvidence {
    let mut query = evidence(count, known);
    let revision = log.roster.revision;
    let caught_up = log.caught_up;
    supplement(&mut query, log, revision, caught_up);
    query
}

#[test]
fn continuous_current_run_names_supplement_anonymous_players_without_ids_or_durations() {
    let fixture = Fixture::new();
    fixture.write("[server] Player '同名' logged in with Permissions:\n[server] Player '同名' logged in with Permissions:\n");
    let mut log = fixture.tracker();
    log.advance();
    let result = project(&mut log, 2, &[]).snapshot.public_snapshot;
    assert!(result.complete);
    assert_eq!(result.entries.len(), 2);
    assert_ne!(result.entries[0].player_key, result.entries[1].player_key);
    assert!(
        result
            .entries
            .iter()
            .all(|entry| entry.identifiers.is_empty()
                && entry.available_action_ids.is_empty()
                && entry.session_started_at_unix_ms.is_none())
    );
}

#[test]
fn zero_count_cannot_consume_a_join_written_during_the_query() {
    let fixture = Fixture::new();
    let mut log = fixture.tracker();
    log.advance();
    let revision = log.roster.revision;
    let caught_up = log.caught_up;
    let mut empty_query = evidence(0, &[]);
    fixture.write("[server] Player 'New' logged in with Permissions:\n");
    log.advance();
    supplement(&mut empty_query, &mut log, revision, caught_up);
    assert!(!empty_query.snapshot.public_snapshot.complete);
    assert_eq!(log.roster.names.get("New"), Some(&1));
    let next = project(&mut log, 1, &[]).snapshot.public_snapshot;
    assert!(next.complete);
    assert_eq!(next.entries[0].display_name, "New");
}

#[test]
fn same_count_replacement_during_query_is_incomplete_then_recovers() {
    let fixture = Fixture::new();
    fixture.write("[server] Player 'Old' logged in with Permissions:\n");
    let mut log = fixture.tracker();
    log.advance();
    let revision = log.roster.revision;
    fixture
        .write("[server] Remove Player 'Old'\n[server] Player 'New' logged in with Permissions:\n");
    log.advance();
    let mut query = evidence(1, &[]);
    supplement(&mut query, &mut log, revision, true);
    assert!(!query.snapshot.public_snapshot.complete);
    assert!(query.snapshot.public_snapshot.entries.is_empty());
    let next = project(&mut log, 1, &[]).snapshot.public_snapshot;
    assert!(next.complete);
    assert_eq!(next.entries[0].display_name, "New");
}

#[test]
fn named_a2s_rows_must_be_a_submultiset_before_log_supplementation() {
    let fixture = Fixture::new();
    fixture.write("[server] Player 'A' logged in with Permissions:\n[server] Player 'B' logged in with Permissions:\n");
    let mut log = fixture.tracker();
    log.advance();
    assert!(
        project(&mut log, 2, &["B"])
            .snapshot
            .public_snapshot
            .complete
    );
    let conflict = project(&mut log, 2, &["C"]).snapshot.public_snapshot;
    assert!(!conflict.complete);
    assert_eq!(conflict.entries[0].display_name, "C");
    let complete_a2s = project(&mut log, 2, &["C", "D"]).snapshot.public_snapshot;
    assert!(complete_a2s.complete);
    assert_eq!(complete_a2s.entries[0].display_name, "C");
}

#[test]
fn count_disagreement_or_missing_info_does_not_publish_log_names() {
    let fixture = Fixture::new();
    fixture.write("[server] Player 'A' logged in with Permissions:\n");
    let mut log = fixture.tracker();
    log.advance();
    assert!(!project(&mut log, 2, &[]).snapshot.public_snapshot.complete);
    let mut query = evidence(1, &[]);
    query.info = None;
    let revision = log.roster.revision;
    supplement(&mut query, &mut log, revision, true);
    assert!(!query.snapshot.public_snapshot.complete);
}

#[test]
fn unfinished_utf8_log_line_is_not_an_event_until_newline_arrives() {
    let fixture = Fixture::new();
    let line = "[server] Player '测试' logged in with Permissions:\n".as_bytes();
    let split = "[server] Player '".len() + 1;
    fixture.log.write_all(&line[..split]).unwrap();
    let mut log = fixture.tracker();
    log.advance();
    assert!(!log.caught_up);
    assert_eq!(log.roster.count(), 0);
    fixture.log.write_all(&line[split..]).unwrap();
    log.advance();
    assert!(project(&mut log, 1, &[]).snapshot.public_snapshot.complete);
}

#[test]
fn run_or_process_generation_changes_discard_previous_names() {
    let fixture = Fixture::new();
    let registry = NativePlayerLogRegistry::default();
    for variant in 0..4 {
        let mut old = fixture.tracker();
        old.roster
            .apply("[server] Player 'Old' logged in with Permissions:");
        let mut key = old.key.clone();
        match variant {
            0 => key.run_id += 1,
            1 => key.pid += 1,
            2 => key.identity.as_mut().unwrap().0 += 1,
            _ => key.path = fixture.root.join("managed-console/run-2-main.log"),
        }
        registry.remember("instance".into(), old).unwrap();
        let next = registry.take("instance", key).unwrap();
        assert_eq!(next.roster.count(), 0);
        assert_eq!(next.tail.byte_offset, 0);
    }
}

#[test]
fn missing_log_or_lost_history_never_reuses_old_names() {
    let fixture = Fixture::new();
    let mut log = fixture.tracker();
    log.roster
        .apply("[server] Player 'Old' logged in with Permissions:");
    log.key.path = fixture.root.join("managed-console/run-missing-main.log");
    log.advance();
    let missing = project(&mut log, 1, &[]).snapshot.public_snapshot;
    assert!(!missing.complete);
    assert!(missing.entries.is_empty());
    log.roster.invalidate();
    log.caught_up = true;
    log.needs_replay = false;
    let empty = project(&mut log, 0, &[]).snapshot.public_snapshot;
    assert!(empty.complete);
    assert!(!log.roster.incomplete);
}

#[test]
fn an_overlong_cross_chunk_line_cannot_leave_an_old_name_complete() {
    let fixture = Fixture::new();
    fixture.write("[server] Player 'Old' logged in with Permissions:\n");
    let mut log = fixture.tracker();
    log.advance();
    fixture.write(&format!(
        "{}[server] Remove Player 'Old'\n",
        "x".repeat(READ_CHUNK + MAX_LINE)
    ));
    log.advance();
    assert!(log.caught_up);
    assert!(log.roster.incomplete);
    assert!(log.roster.names.is_empty());
    assert!(!project(&mut log, 1, &[]).snapshot.public_snapshot.complete);
    log.advance();
    assert!(!project(&mut log, 1, &[]).snapshot.public_snapshot.complete);
}

#[test]
fn managed_truncation_or_longer_replacement_cannot_reuse_previous_names() {
    for replace in [false, true] {
        let fixture = Fixture::new();
        fixture.write("[server] Player 'Old' logged in with Permissions:\n");
        let mut log = fixture.tracker();
        log.advance();
        if replace {
            std::fs::rename(&log.key.path, fixture.root.join("previous.log")).unwrap();
            std::fs::write(&log.key.path, "replacement noise\n".repeat(100)).unwrap();
        } else {
            std::fs::write(&log.key.path, "short\n").unwrap();
        }
        log.advance();
        assert!(log.roster.names.is_empty());
        assert!(!project(&mut log, 1, &[]).snapshot.public_snapshot.complete);
        log.advance();
        assert!(!project(&mut log, 1, &[]).snapshot.public_snapshot.complete);
    }
}

#[test]
fn managed_rotation_preserves_names_but_retention_gaps_require_a_new_empty_baseline() {
    let fixture = Fixture::new();
    fixture.write("[server] Player 'Old' logged in with Permissions:\n");
    let mut log = fixture.tracker();
    log.advance();
    // Use actual production-sized segments and the managed writer's rotation.
    let padding = format!("{}\n", "x".repeat(1023)).repeat(8 * 1024);
    fixture.write(&padding);
    for _ in 0..4 {
        log.advance();
        if log.caught_up {
            break;
        }
    }
    assert!(log.caught_up);
    assert!(project(&mut log, 1, &[]).snapshot.public_snapshot.complete);
    for _ in 0..5 {
        fixture.write(&padding);
    }
    for _ in 0..10 {
        log.advance();
        if log.caught_up {
            break;
        }
    }
    assert!(log.caught_up);
    assert!(log.roster.incomplete);
    assert!(!project(&mut log, 1, &[]).snapshot.public_snapshot.complete);
    assert!(project(&mut log, 0, &[]).snapshot.public_snapshot.complete);
    fixture.write("[server] Player 'New' logged in with Permissions:\n");
    log.advance();
    let recovered = project(&mut log, 1, &[]).snapshot.public_snapshot;
    assert!(recovered.complete);
    assert_eq!(recovered.entries[0].display_name, "New");
}

#[test]
fn valheim_log_uses_only_active_session_names_and_removes_departures() {
    let fixture = Fixture::new();
    fixture.write("10/16/2026 20:55:39: Player history entry with index 0: Historical (Steam_123, abc)\n10/16/2026 20:55:39: Server: New peer connected,sending global keys\n10/16/2026 20:55:39: Got character ZDOID from Current : 42:1\n");
    let mut log = fixture.tracker_for(Game::Valheim);
    log.advance();
    let first = project(&mut log, 1, &[]).snapshot.public_snapshot;
    assert!(first.complete);
    assert_eq!(first.entries[0].display_name, "Current");
    assert_eq!(first.entries[0].identifiers.len(), 1);
    assert_eq!(
        first.entries[0].identifiers[0].kind,
        RuntimePlayerIdentityKind::PlayerName
    );
    assert_eq!(first.entries[0].identifiers[0].value, "Current");
    assert!(!first.entries[0].identifiers[0].stable);
    assert!(first.entries[0].available_action_ids.is_empty());
    fixture.write("10/16/2026 20:56:46: RPC_Disconnect\n10/16/2026 20:56:46: Destroying abandoned non persistent zdo 42:1 owner 42\n10/16/2026 20:56:46: Closing socket 76561198000000001\n");
    log.advance();
    let departed = project(&mut log, 0, &[]).snapshot.public_snapshot;
    assert!(departed.complete);
    assert!(departed.entries.is_empty());
}

#[test]
fn valheim_pending_join_or_unattributed_disconnect_prevents_complete_names() {
    let fixture = Fixture::new();
    fixture.write("10/16/2026 20:55:39: Server: New peer connected,sending global keys\n10/16/2026 20:55:39: Got character ZDOID from Old : 42:1\n10/16/2026 20:56:00: Server: New peer connected,sending global keys\n");
    let mut log = fixture.tracker_for(Game::Valheim);
    log.advance();
    assert!(!project(&mut log, 1, &[]).snapshot.public_snapshot.complete);
    fixture.write("10/16/2026 20:56:46: RPC_Disconnect\n10/16/2026 20:56:46: Closing socket 76561198000000001\n");
    log.advance();
    assert!(!project(&mut log, 1, &[]).snapshot.public_snapshot.complete);
    assert!(log.roster.names.is_empty());
}

fn valheim_snapshot(log: &mut TrackedLog, request: &str, count: usize) -> CachedLivePlayerSnapshot {
    let mut query = evidence(count, &[]);
    query.returned_players = 0;
    query.snapshot.public_snapshot.snapshot_id = request.to_owned();
    let revision = log.roster.revision;
    let caught_up = log.caught_up;
    supplement(&mut query, log, revision, caught_up);
    query.snapshot
}

#[test]
fn valheim_opaque_selection_key_survives_refresh_duplicate_events_and_respawn() {
    let fixture = Fixture::new();
    fixture.write("10/16/2026 20:55:39: Server: New peer connected,sending global keys\n10/16/2026 20:55:39: Got character ZDOID from Same : 42:1\n");
    let mut log = fixture.tracker_for(Game::Valheim);
    log.advance();
    let first = valheim_snapshot(&mut log, "first-request", 1);
    let key = first.public_snapshot.entries[0].player_key.clone();
    let opaque = key
        .strip_prefix("valheim-log:")
        .expect("Valheim opaque row key");
    assert!(uuid::Uuid::parse_str(opaque).is_ok());
    let refreshed = valheim_snapshot(&mut log, "second-request", 1);
    assert_ne!(
        first.public_snapshot.snapshot_id,
        refreshed.public_snapshot.snapshot_id
    );
    assert_eq!(refreshed.public_snapshot.entries[0].player_key, key);
    fixture.write("10/16/2026 20:55:40: Got character ZDOID from Same : 42:1\n10/16/2026 20:55:41: Got character ZDOID from Same : 0:0\n10/16/2026 20:55:42: Got character ZDOID from Renamed : 42:2\n");
    log.advance();
    let respawned = valheim_snapshot(&mut log, "third-request", 1);
    assert!(respawned.public_snapshot.complete);
    let entry = &respawned.public_snapshot.entries[0];
    assert_eq!(entry.player_key, key);
    assert_eq!(entry.display_name, "Renamed");
    assert_eq!(entry.identifiers.len(), 1);
    assert_eq!(
        entry.identifiers[0].kind,
        RuntimePlayerIdentityKind::PlayerName
    );
    assert_eq!(entry.identifiers[0].value, "Renamed");
    assert!(!entry.identifiers[0].stable);
    assert!(entry.available_action_ids.is_empty());
    assert!(respawned.private_action_bindings.is_empty());
}

#[test]
fn valheim_same_name_departure_and_owner_reuse_do_not_inherit_selection_keys() {
    let fixture = Fixture::new();
    fixture.write("10/16/2026 20:55:39: Server: New peer connected,sending global keys\n10/16/2026 20:55:39: Got character ZDOID from Same : 42:1\n10/16/2026 20:55:40: Server: New peer connected,sending global keys\n10/16/2026 20:55:40: Got character ZDOID from Same : 43:1\n");
    let mut log = fixture.tracker_for(Game::Valheim);
    log.advance();
    let first = valheim_snapshot(&mut log, "first-request", 2).public_snapshot;
    assert!(first.complete);
    assert_ne!(first.entries[0].player_key, first.entries[1].player_key);
    fixture.write("10/16/2026 20:56:46: RPC_Disconnect\n10/16/2026 20:56:46: Destroying abandoned non persistent zdo 42:1 owner 42\n10/16/2026 20:56:46: Closing socket 76561198000000001\n");
    log.advance();
    let remaining = valheim_snapshot(&mut log, "second-request", 1).public_snapshot;
    assert!(remaining.complete);
    let remaining_key = remaining.entries[0].player_key.clone();
    assert!(
        first
            .entries
            .iter()
            .any(|entry| entry.player_key == remaining_key)
    );
    let departed_key = first
        .entries
        .iter()
        .find(|entry| entry.player_key != remaining_key)
        .expect("distinct departed peer")
        .player_key
        .clone();
    fixture.write("10/16/2026 20:56:50: Server: New peer connected,sending global keys\n10/16/2026 20:56:50: Got character ZDOID from Same : 42:2\n");
    log.advance();
    let rejoined = valheim_snapshot(&mut log, "third-request", 2).public_snapshot;
    assert!(rejoined.complete);
    assert!(
        rejoined
            .entries
            .iter()
            .any(|entry| entry.player_key == remaining_key)
    );
    assert!(
        rejoined
            .entries
            .iter()
            .all(|entry| entry.player_key != departed_key)
    );
}

#[test]
fn valheim_process_run_generations_never_reuse_opaque_selection_keys() {
    let fixture = Fixture::new();
    fixture.write("10/16/2026 20:55:39: Got character ZDOID from Same : 42:1\n");
    let registry = NativePlayerLogRegistry::default();
    for variant in 0..3 {
        let mut old = fixture.tracker_for(Game::Valheim);
        old.advance();
        let previous_key = valheim_snapshot(&mut old, "old", 1).public_snapshot.entries[0]
            .player_key
            .clone();
        let mut next_key = old.key.clone();
        match variant {
            0 => next_key.run_id += 1,
            1 => next_key.pid += 1,
            _ => next_key.identity.as_mut().unwrap().0 += 1,
        }
        registry.remember("instance".into(), old).unwrap();
        let mut next = registry.take("instance", next_key).unwrap();
        next.advance();
        let current = valheim_snapshot(&mut next, "new", 1).public_snapshot;
        assert!(current.complete);
        assert_ne!(current.entries[0].player_key, previous_key);
    }
}

#[test]
fn valheim_log_gap_and_unknown_disconnect_invalidate_opaque_selection_keys() {
    let fixture = Fixture::new();
    fixture.write("10/16/2026 20:55:39: Got character ZDOID from Same : 42:1\n");
    let mut log = fixture.tracker_for(Game::Valheim);
    log.advance();
    let previous_key = valheim_snapshot(&mut log, "old", 1).public_snapshot.entries[0]
        .player_key
        .clone();
    log.read_failed();
    log.advance();
    let gap = valheim_snapshot(&mut log, "gap", 1).public_snapshot;
    assert!(!gap.complete);
    assert!(gap.entries.is_empty());
    // An actual empty baseline may recover the damaged stream, without restoring old tokens.
    assert!(
        valheim_snapshot(&mut log, "empty", 0)
            .public_snapshot
            .complete
    );
    fixture.write("10/16/2026 20:56:50: Server: New peer connected,sending global keys\n10/16/2026 20:56:50: Got character ZDOID from Same : 42:2\n");
    log.advance();
    let recovered = valheim_snapshot(&mut log, "recovered", 1).public_snapshot;
    assert!(recovered.complete);
    assert_ne!(recovered.entries[0].player_key, previous_key);
    fixture.write("10/16/2026 20:56:51: Closing socket 76561198000000001\n");
    log.advance();
    let unknown_departure = valheim_snapshot(&mut log, "unknown-departure", 1).public_snapshot;
    assert!(!unknown_departure.complete);
    assert!(unknown_departure.entries.is_empty());
    assert!(log.valheim_peers.entries().next().is_none());
}

#[test]
fn valheim_complete_a2s_names_use_proven_native_peer_keys_and_character_identifiers() {
    let fixture = Fixture::new();
    fixture.write("10/16/2026 20:55:39: Got character ZDOID from Same : 42:1\n10/16/2026 20:55:39: Got character ZDOID from Same : 43:1\n");
    let mut log = fixture.tracker_for(Game::Valheim);
    log.advance();
    let native = valheim_snapshot(&mut log, "native-first", 2).public_snapshot;
    let from_a2s = project(&mut log, 2, &["Same", "Same"]).snapshot;
    assert!(from_a2s.public_snapshot.complete);
    assert_eq!(from_a2s.public_snapshot.entries.len(), 2);
    for (expected, entry) in native.entries.iter().zip(&from_a2s.public_snapshot.entries) {
        assert_eq!(entry.player_key, expected.player_key);
        assert_eq!(entry.display_name, "Same");
        assert_eq!(entry.identifiers.len(), 1);
        assert_eq!(
            entry.identifiers[0].kind,
            RuntimePlayerIdentityKind::PlayerName
        );
        assert_eq!(entry.identifiers[0].value, "Same");
        assert!(!entry.identifiers[0].stable);
        assert!(entry.available_action_ids.is_empty());
        // Query durations cannot be attributed to same-named peers by row order.
        assert_eq!(entry.session_started_at_unix_ms, None);
    }
    assert_ne!(
        from_a2s.public_snapshot.entries[0].player_key,
        from_a2s.public_snapshot.entries[1].player_key
    );
    assert!(from_a2s.private_action_bindings.is_empty());
}

#[test]
fn valheim_complete_a2s_names_remain_read_only_and_complete_when_log_has_a_gap() {
    let fixture = Fixture::new();
    fixture.write("10/16/2026 20:55:39: Got character ZDOID from Character : 42:1\n");
    let mut log = fixture.tracker_for(Game::Valheim);
    log.advance();
    log.read_failed();
    log.advance();
    assert!(log.roster.incomplete);
    let query = project(&mut log, 1, &["Query display name"]).snapshot;
    let snapshot = query.public_snapshot;
    assert!(snapshot.complete);
    assert_eq!(snapshot.current_players, Some(1));
    assert_eq!(snapshot.entries.len(), 1);
    let entry = &snapshot.entries[0];
    assert_eq!(entry.player_key, "0");
    assert_eq!(entry.display_name, "Query display name");
    assert_eq!(entry.session_started_at_unix_ms, Some(999));
    assert!(entry.identifiers.is_empty());
    assert!(entry.available_action_ids.is_empty());
    assert!(snapshot.issue.is_none());
    assert!(query.private_action_bindings.is_empty());
}
