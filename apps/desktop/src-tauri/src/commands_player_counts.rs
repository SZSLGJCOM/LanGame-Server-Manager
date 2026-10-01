use app_core::{
    RuntimeLivePlayerSnapshot, RuntimeLivePlayerStatus, RuntimePlayerQueryState,
    RuntimePlayerSnapshot,
};

use super::DesktopState;
use super::commands_live_players::read_or_refresh_live_players_for_count;

pub(super) async fn collect_player_list_count(
    state: &tauri::State<'_, DesktopState>,
    instance_id: &str,
) -> Result<RuntimePlayerSnapshot, String> {
    let roster = read_or_refresh_live_players_for_count(state, instance_id).await?;
    Ok(project_player_list_count(&roster))
}

fn project_player_list_count(roster: &RuntimeLivePlayerSnapshot) -> RuntimePlayerSnapshot {
    let authoritative = roster.status == RuntimeLivePlayerStatus::Ready
        && roster.complete
        && !roster.truncated
        && !roster.stale;
    let current_players = authoritative.then_some(roster.current_players).flatten();
    let (status, summary) = if current_players.is_some() {
        (
            "ready",
            "Player count reported by the current server's complete online-player list.",
        )
    } else {
        match roster.status {
            RuntimeLivePlayerStatus::Stopped => ("stopped", "The server is not running."),
            RuntimeLivePlayerStatus::Unsupported => (
                "unsupported",
                "This module has no connected online-player source.",
            ),
            RuntimeLivePlayerStatus::Misconfigured => (
                "misconfigured",
                "Configure the server's remote command interface to read its player count.",
            ),
            RuntimeLivePlayerStatus::Refreshing => (
                "refreshing",
                "The server's online-player list is being refreshed.",
            ),
            RuntimeLivePlayerStatus::Failed => (
                "failed",
                "The server's online-player list could not be collected.",
            ),
            RuntimeLivePlayerStatus::Ready => (
                "unavailable",
                "A fresh, complete online-player list is required to report the player count.",
            ),
        }
    };
    RuntimePlayerSnapshot {
        current_players,
        max_players: authoritative.then_some(roster.max_players).flatten(),
        query: RuntimePlayerQueryState {
            status: status.to_owned(),
            summary: roster
                .issue
                .as_ref()
                .map_or_else(|| summary.to_owned(), |issue| issue.summary.clone()),
        },
    }
}

#[cfg(test)]
#[path = "commands_player_counts_tests.rs"]
mod tests;
