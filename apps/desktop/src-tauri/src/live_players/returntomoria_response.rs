use std::collections::HashMap;

use app_core::{
    ModulePlayerListSource, RuntimeLivePlayerEntry, RuntimeLivePlayerIssueCode,
    RuntimeLivePlayerSnapshot, RuntimeLivePlayerStatus,
};

use super::failure;
use crate::live_players::cache::{CachedLivePlayerSnapshot, LivePlayerCollectionResult};

pub(super) fn parse(
    instance_id: &str,
    nonce: &str,
    observed_at: u64,
    text: &str,
) -> LivePlayerCollectionResult {
    let invalid = || {
        failure(
            instance_id,
            nonce,
            RuntimeLivePlayerIssueCode::ProtocolIncomplete,
            "Return to Moria did not provide a complete, unambiguous online-player response.",
        )
    };
    if text.len() > 64 * 1024
        || nonce.len() != 32
        || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
        || !text.ends_with('\n')
    {
        return Err(invalid());
    }
    let lines: Vec<_> = text.lines().filter(|line| !line.is_empty()).collect();
    let begin = format!("Unknown command \"LGM_PLAYER_QUERY_BEGIN_{nonce}\"!");
    let end_echo = format!("> LGM_PLAYER_QUERY_END_{nonce}");
    let end = format!("Unknown command \"LGM_PLAYER_QUERY_END_{nonce}\"!");
    if lines.len() < 5
        || lines.first() != Some(&begin.as_str())
        || lines.get(1) != Some(&"> players")
        || lines.get(lines.len() - 2) != Some(&end_echo.as_str())
        || lines.last() != Some(&end.as_str())
    {
        return Err(invalid());
    }
    let (current, maximum) = lines[2]
        .strip_prefix("Players: ")
        .and_then(|counts| counts.split_once('/'))
        .ok_or_else(invalid)?;
    let parse_count = |value: &str| {
        value
            .parse::<usize>()
            .ok()
            .filter(|count| count.to_string() == value)
    };
    let current = parse_count(current).ok_or_else(invalid)?;
    let maximum = parse_count(maximum).ok_or_else(invalid)?;
    if maximum != 8 || current > maximum || lines.len() - 5 != current {
        return Err(invalid());
    }
    let mut entries = Vec::with_capacity(current);
    for (index, row) in lines[3..lines.len() - 2].iter().enumerate() {
        let label = row.strip_prefix(" * ").ok_or_else(invalid)?;
        // These are first-party connected-player labels. The native unescaped
        // parentheses have no verified account contract; preserve the whole
        // label and expose neither an account identifier nor moderation target.
        if label == "Not all players are fully connected"
            || label.len() > 2048
            || label.trim().is_empty()
            || label.chars().any(char::is_control)
            || !label.ends_with(')')
            || !label
                .rsplit_once(" (")
                .is_some_and(|(name, tail)| !name.trim().is_empty() && tail.len() > 1)
        {
            return Err(invalid());
        }
        entries.push(RuntimeLivePlayerEntry {
            player_key: format!("returntomoria:{nonce}:{index}"),
            display_name: label.to_owned(),
            identifiers: Vec::new(),
            available_action_ids: Vec::new(),
            ping_ms: None,
            session_started_at_unix_ms: None,
            role: None,
            attributes: Vec::new(),
        });
    }
    Ok(CachedLivePlayerSnapshot {
        public_snapshot: RuntimeLivePlayerSnapshot {
            snapshot_id: nonce.to_owned(),
            instance_id: instance_id.to_owned(),
            status: RuntimeLivePlayerStatus::Ready,
            source: Some(ModulePlayerListSource::NativeConsole),
            observed_at_unix_ms: Some(observed_at),
            expires_at_unix_ms: None,
            complete: true,
            truncated: false,
            stale: false,
            current_players: Some(current),
            max_players: Some(maximum),
            entries,
            issue: None,
        },
        private_action_bindings: HashMap::new(),
        collected_at: observed_at,
    })
}

#[cfg(test)]
#[path = "returntomoria_tests.rs"]
mod tests;
