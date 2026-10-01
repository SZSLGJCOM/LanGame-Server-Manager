use super::*;
use app_core::ModulePlayerListSource;

fn roster() -> RuntimeLivePlayerSnapshot {
    RuntimeLivePlayerSnapshot {
        snapshot_id: String::from("count-snapshot"),
        instance_id: String::from("instance"),
        status: RuntimeLivePlayerStatus::Ready,
        source: Some(ModulePlayerListSource::RuntimeAction),
        observed_at_unix_ms: Some(10_000),
        expires_at_unix_ms: Some(40_000),
        complete: true,
        truncated: false,
        stale: false,
        current_players: Some(3),
        max_players: Some(80),
        entries: Vec::new(),
        issue: None,
    }
}

#[test]
fn player_list_count_preserves_reported_counts_and_genuine_empty_rosters() {
    for players in [0, 3, 80] {
        let mut roster = roster();
        roster.current_players = Some(players);
        let count = project_player_list_count(&roster);
        // Counts are reported by the complete protocol; never derived from rows.
        assert_eq!(count.current_players, Some(players));
        assert_eq!(count.max_players, Some(80));
        assert_eq!(count.query.status, "ready");
    }
}

#[test]
fn player_list_count_never_reports_retained_rows_from_failed_or_pending_refreshes() {
    for status in [
        RuntimeLivePlayerStatus::Failed,
        RuntimeLivePlayerStatus::Misconfigured,
        RuntimeLivePlayerStatus::Stopped,
        RuntimeLivePlayerStatus::Unsupported,
        RuntimeLivePlayerStatus::Refreshing,
    ] {
        let mut roster = roster();
        roster.status = status;
        let count = project_player_list_count(&roster);
        assert_eq!(count.current_players, None, "{status:?}");
        assert_eq!(count.max_players, None);
        assert_ne!(count.query.status, "ready");
    }
}

#[test]
fn player_list_count_requires_a_fresh_complete_untruncated_reported_count() {
    for variant in 0..4 {
        let mut roster = roster();
        match variant {
            0 => roster.stale = true,
            1 => roster.complete = false,
            2 => roster.truncated = true,
            _ => roster.current_players = None,
        }
        let count = project_player_list_count(&roster);
        assert_eq!(count.current_players, None);
        assert_eq!(count.query.status, "unavailable");
    }
}
