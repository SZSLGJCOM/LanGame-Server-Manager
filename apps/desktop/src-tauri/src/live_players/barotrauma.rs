use std::collections::{HashMap, HashSet};

use app_core::{RuntimeLivePlayerEntry, RuntimeLivePlayerIdentifier, RuntimePlayerIdentityKind};

use super::response_codecs::ParsedPlayerList;

const DELIMITER: &str = "***************";

/// Official DebugConsole.clientlist emits a command echo, opening delimiter,
/// one row per ConnectedClients member and a closing delimiter, including zero.
/// Input is the plain-text ConPTY transcript captured after dispatch.
pub(crate) fn parse(lines: &[&str], request_id: &str) -> Result<ParsedPlayerList, String> {
    let expected_echo = format!("clientlist LGM_PLAYER_QUERY_{request_id}");
    let mut echoes = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| **line == expected_echo);
    let (start, _) = echoes
        .next()
        .ok_or("Barotrauma has not echoed the clientlist command.")?;
    if echoes.next().is_some() {
        return Err("Barotrauma returned overlapping clientlist responses.".into());
    }
    if lines.get(start + 1) != Some(&DELIMITER) {
        return Err("Barotrauma has not returned the opening clientlist delimiter.".into());
    }
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 2)
        .find_map(|(index, line)| (*line == DELIMITER).then_some(index))
        .ok_or("Barotrauma has not returned the closing clientlist delimiter.")?;
    let count = end - start - 2;
    if count > 4096 {
        return Err("Barotrauma returned too many client rows.".into());
    }
    let mut entries = Vec::new();
    let mut sessions = HashSet::new();
    for line in &lines[start + 2..end] {
        let row = line
            .strip_prefix("- ")
            .ok_or("Barotrauma client rows are interleaved or malformed.")?;
        let (session, row) = row
            .split_once(": ")
            .ok_or("Barotrauma client session is missing.")?;
        if session.is_empty()
            || !session.bytes().all(|byte| byte.is_ascii_digit())
            || !session
                .parse::<u16>()
                .is_ok_and(|value| value.to_string() == session)
            || !sessions.insert(session)
        {
            return Err("Barotrauma client sessions are malformed or duplicated.".into());
        }
        let (row, ping) = row
            .rsplit_once(", ping ")
            .ok_or("Barotrauma client latency is missing.")?;
        let ping = ping
            .strip_suffix(" ms")
            .and_then(|value| value.parse::<i32>().ok())
            .ok_or("Barotrauma client latency is malformed.")?;
        let (row, account) = row
            .rsplit_once(", ")
            .ok_or("Barotrauma client account field is missing.")?;
        let (label, endpoint) = row
            .rsplit_once(", ")
            .ok_or("Barotrauma client endpoint is missing.")?;
        for value in [label, account, endpoint] {
            if value.trim().is_empty() || value.len() > 4096 || value.chars().any(char::is_control)
            {
                return Err("Barotrauma client fields are malformed.".into());
            }
        }
        // The game appends ` playing <Character.LogName>` without escaping the
        // player name. Preserve its label rather than inventing a name boundary.
        if entries.len() < 256 {
            entries.push(RuntimeLivePlayerEntry {
                player_key: format!("barotrauma:{request_id}:{session}"),
                display_name: label.to_owned(),
                identifiers: vec![RuntimeLivePlayerIdentifier {
                    kind: RuntimePlayerIdentityKind::SessionId,
                    value: session.to_owned(),
                    stable: false,
                }],
                available_action_ids: Vec::new(),
                ping_ms: u32::try_from(ping).ok(),
                session_started_at_unix_ms: None,
                role: None,
                attributes: Vec::new(),
            });
        }
    }
    Ok(ParsedPlayerList {
        complete: entries.len() == count,
        truncated: entries.len() != count,
        entries,
        bindings: HashMap::new(),
        current_players: Some(count),
        max_players: None,
    })
}

#[cfg(test)]
#[path = "barotrauma_tests.rs"]
mod tests;
