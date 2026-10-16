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
        TrackedLog::new(RunKey {
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
    let registry = EnshroudedPlayerRegistry::default();
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
