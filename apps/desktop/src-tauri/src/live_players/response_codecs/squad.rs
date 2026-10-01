use super::{
    ListBuilder, RuntimePlayerIdentityKind, attribute, decimal, eos_id, identifier, player,
    steam_id,
};

pub(super) fn parse(text: &str, output: &mut ListBuilder<'_>) -> Result<(), String> {
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    if lines.next() != Some("----- Active Players -----") {
        return Err(String::from(
            "Squad ListPlayers is missing its active-player header.",
        ));
    }
    let mut disconnected = false;
    let mut disconnected_count = 0;
    for line in lines {
        if line == "----- Recently Disconnected Players [Max of 15] -----" {
            if disconnected {
                return Err(String::from(
                    "Squad ListPlayers repeats its disconnected-player section.",
                ));
            }
            disconnected = true;
            continue;
        }
        if disconnected {
            disconnected_count += 1;
            if disconnected_count > 15 || !line.starts_with("ID: ") {
                return Err(String::from(
                    "Squad ListPlayers returned an invalid disconnected-player section.",
                ));
            }
            super::validate_field(line)?;
            continue;
        }
        let (rest, role) = take_last(line, " | Role: ")?;
        let (rest, leader) = take_last(rest, " | Is Leader: ")?;
        if leader != "True" && leader != "False" {
            return Err(String::from(
                "Squad ListPlayers returned an invalid leader flag.",
            ));
        }
        let (rest, squad) = take_last(rest, " | Squad ID: ")?;
        let (rest, team) = take_last(rest, " | Team ID: ")?;
        if (squad != "N/A" && !decimal(squad)) || (team != "N/A" && !decimal(team)) {
            return Err(String::from(
                "Squad ListPlayers returned an invalid team or squad ID.",
            ));
        }
        let (identity_section, name) = rest
            .split_once(" | Name: ")
            .ok_or_else(|| String::from("Squad ListPlayers is missing a player name."))?;
        let (session, platform_ids) = identity_section
            .strip_prefix("ID: ")
            .and_then(|value| value.split_once(" | Online IDs:"))
            .ok_or_else(|| {
                String::from("Squad ListPlayers returned an invalid identity section.")
            })?;
        if !decimal(session) {
            return Err(String::from(
                "Squad ListPlayers returned an invalid session ID.",
            ));
        }
        let (steam, eos) = parse_platform_ids(platform_ids)?;
        let mut identifiers = vec![identifier(
            RuntimePlayerIdentityKind::SessionId,
            session,
            false,
        )];
        if let Some(id) = steam.as_deref() {
            identifiers.push(identifier(RuntimePlayerIdentityKind::SteamId, id, true));
        }
        if let Some(id) = eos.as_deref() {
            identifiers.push(identifier(RuntimePlayerIdentityKind::EosId, id, true));
        }
        let mut entry = player(name, identifiers)?;
        entry.attributes = vec![
            attribute("team", team),
            attribute("squad", squad),
            attribute("leader", leader),
        ];
        if !role.is_empty() {
            super::validate_field(role)?;
            entry.role = Some(role.to_owned());
        }
        // The session index is for display. Stable IDs avoid targeting a reused player slot.
        let target = steam.as_deref().or(eos.as_deref());
        let targets = target
            .map(|id| vec![("kick_player", id), ("ban_player", id)])
            .unwrap_or_default();
        output.push(entry, &targets)?;
    }
    if !disconnected {
        return Err(String::from(
            "Squad ListPlayers is missing its disconnected-player section.",
        ));
    }
    Ok(())
}

fn take_last<'a>(line: &'a str, delimiter: &str) -> Result<(&'a str, &'a str), String> {
    // An empty role is valid for spectators; trim() may have removed its trailing space.
    if delimiter == " | Role: "
        && let Some(rest) = line.strip_suffix(" | Role:")
    {
        return Ok((rest, ""));
    }
    line.rsplit_once(delimiter)
        .ok_or_else(|| String::from("Squad ListPlayers returned an incomplete player row."))
}

fn parse_platform_ids(text: &str) -> Result<(Option<String>, Option<String>), String> {
    let normalized = text.replace(':', ": ");
    let mut tokens = normalized.split_whitespace();
    let mut steam = None;
    let mut eos = None;
    while let Some(platform) = tokens.next() {
        let value = tokens
            .next()
            .ok_or_else(|| String::from("Squad ListPlayers returned an incomplete platform ID."))?;
        if !platform.ends_with(':') {
            return Err(String::from(
                "Squad ListPlayers returned an invalid platform ID.",
            ));
        }
        if platform.eq_ignore_ascii_case("steam:") {
            if !steam_id(value) || steam.replace(value.to_owned()).is_some() {
                return Err(String::from(
                    "Squad ListPlayers returned an invalid Steam ID.",
                ));
            }
        } else if platform.eq_ignore_ascii_case("EOS:") {
            if !eos_id(value) || eos.replace(value.to_owned()).is_some() {
                return Err(String::from(
                    "Squad ListPlayers returned an invalid EOS ID.",
                ));
            }
        } else {
            super::validate_field(value)?;
        }
    }
    Ok((steam, eos))
}
