use std::collections::HashSet;

use super::{
    ListBuilder, MAX_RESPONSE_ROWS, RuntimePlayerIdentityKind, identifier, player, validate_field,
};

pub(crate) fn response_is_complete(text: &str) -> Result<bool, String> {
    // The caller has read a complete length-prefixed RCON frame, including both
    // NUL terminators. Native info ends at the final name without a newline;
    // validate its header and declared count instead of waiting for another frame.
    Ok(info_names(text)?.is_some())
}

pub(super) fn parse(text: &str, output: &mut ListBuilder<'_>) -> Result<(), String> {
    // This envelope is produced by the dedicated transport from two responses on
    // one authenticated connection. Names are never joined across the responses.
    let response: serde_json::Value = serde_json::from_str(text)
        .map_err(|_| String::from("HumanitZ player response is missing its counted capture."))?;
    let info = response.get("info").and_then(serde_json::Value::as_str);
    let players = response.get("players").and_then(serde_json::Value::as_str);
    let (Some(info), Some(players)) = (info, players) else {
        return Err(String::from(
            "HumanitZ player capture is missing its responses.",
        ));
    };
    let count = info_player_count(info)?
        .ok_or_else(|| String::from("HumanitZ info response is incomplete."))?;
    if !players_response_is_complete(players, count)? {
        return Err(String::from("HumanitZ Players response is incomplete."));
    }
    for line in players.lines().filter(|line| !line.trim().is_empty()) {
        let (name, net_id) = player_row(line)?;
        let entry = player(
            name,
            vec![identifier(
                RuntimePlayerIdentityKind::HumanitzNetId,
                net_id,
                true,
            )],
        )?;
        output.push(entry, &[("kick_player", net_id)])?;
    }
    Ok(())
}

pub(crate) fn info_player_count(text: &str) -> Result<Option<usize>, String> {
    Ok(info_names(text)?.map(|names| names.len()))
}

pub(crate) fn players_response_is_complete(text: &str, count: usize) -> Result<bool, String> {
    let rows: Vec<_> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if rows.len() > count || count > MAX_RESPONSE_ROWS {
        return Err(String::from(
            "HumanitZ Players response exceeds its declared count.",
        ));
    }
    for line in &rows {
        player_row(line)?;
    }
    Ok(rows.len() == count)
}

fn player_row(line: &str) -> Result<(&str, &str), String> {
    let invalid = || String::from("HumanitZ Players response contains an invalid player row.");
    // Work from the right: names and clan names can contain spaces and brackets.
    let (rest, days) = line.rsplit_once(" DPassed:").ok_or_else(invalid)?;
    let (rest, clan) = rest.rsplit_once(" Clan:").ok_or_else(invalid)?;
    let (rest, level) = rest.rsplit_once(") Lv:").ok_or_else(invalid)?;
    let (name, net_id) = rest.rsplit_once(" (").ok_or_else(invalid)?;
    if ![days, level].iter().all(|value| {
        !value.is_empty() && value.len() <= 10 && value.bytes().all(|byte| byte.is_ascii_digit())
    }) || !app_core::is_humanitz_net_id(net_id)
    {
        return Err(invalid());
    }
    validate_field(name)?;
    validate_field(clan)?;
    Ok((name, net_id))
}

fn info_names(text: &str) -> Result<Option<Vec<&str>>, String> {
    let mut fields = HashSet::new();
    let mut count = None;
    let mut in_players = false;
    let mut names = Vec::new();
    let mut empty_marker = false;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        if in_players {
            if count == Some(0) && line == "No players connected" && !empty_marker {
                empty_marker = true;
                continue;
            }
            validate_field(line)?;
            names.push(line);
            if names.len() > count.unwrap_or(0) {
                return Err(String::from(
                    "HumanitZ info response contains more players than declared.",
                ));
            }
            continue;
        }
        if line == "Players:" {
            if count.is_none()
                || !["Name", "Season", "Weather", "Time", "AI", "FPS"]
                    .iter()
                    .all(|key| fields.contains(key))
            {
                return Err(String::from(
                    "HumanitZ info response is missing its server header or player count.",
                ));
            }
            in_players = true;
            continue;
        }
        if let Some(number) = line.strip_suffix(" connected.") {
            if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(String::from(
                    "HumanitZ info response contains an invalid player count.",
                ));
            }
            let parsed = number
                .parse::<usize>()
                .ok()
                .filter(|value| *value <= MAX_RESPONSE_ROWS)
                .ok_or_else(|| {
                    String::from("HumanitZ info response contains an invalid player count.")
                })?;
            if count.replace(parsed).is_some() {
                return Err(String::from(
                    "HumanitZ info response repeats its player count.",
                ));
            }
            continue;
        }
        let (key, value) = line
            .split_once(": ")
            .ok_or_else(|| String::from("HumanitZ returned an unknown info response."))?;
        if !matches!(key, "Name" | "Season" | "Weather" | "Time" | "AI" | "FPS")
            || value.trim().is_empty()
            || !fields.insert(key)
        {
            return Err(String::from(
                "HumanitZ returned an unknown or repeated info field.",
            ));
        }
    }
    Ok((in_players && count == Some(names.len())).then_some(names))
}

#[cfg(test)]
#[path = "humanitz_tests.rs"]
mod tests;
