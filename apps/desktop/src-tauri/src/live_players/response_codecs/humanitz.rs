use std::collections::HashSet;

use super::{ListBuilder, MAX_RESPONSE_ROWS, player, validate_field};

pub(crate) fn response_is_complete(text: &str) -> Result<bool, String> {
    // A packet may end in the middle of a player name. Only a line terminator
    // establishes that name's end. The native zero-player marker has no final newline.
    let terminated = text.ends_with('\n');
    if !terminated && !text.ends_with("No players connected") {
        return Ok(false);
    }
    Ok(info_names(text)?.is_some_and(|names| terminated || names.is_empty()))
}

pub(super) fn parse(text: &str, output: &mut ListBuilder<'_>) -> Result<(), String> {
    let names = info_names(text)?.ok_or_else(|| {
        String::from("HumanitZ info response has not returned its declared players.")
    })?;
    for name in names {
        // Steam display names need not be unique and are not account identities.
        output.push(player(name, Vec::new())?, &[])?;
    }
    Ok(())
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
