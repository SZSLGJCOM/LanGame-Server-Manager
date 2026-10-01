use super::{
    ListBuilder, RuntimeLivePlayerIdentifier, RuntimePlayerIdentityKind, attribute, decimal,
    identifier, player, steam_id, validate_field,
};

pub(super) fn parse(text: &str, output: &mut ListBuilder<'_>) -> Result<(), String> {
    let mut started = false;
    let mut declared_count = None;
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if declared_count.is_some() {
            return Err(String::from(
                "7 Days to Die returned data after its player-list footer.",
            ));
        }
        if let Some(count) = line
            .strip_prefix("Total of ")
            .and_then(|value| value.strip_suffix(" in the game"))
        {
            declared_count =
                Some(count.parse::<usize>().map_err(|_| {
                    String::from("7 Days to Die returned an invalid player count.")
                })?);
            continue;
        }
        let Some((index, rest)) = line.split_once(". id=") else {
            // Telnet includes its greeting and command acknowledgement before command output.
            if !started {
                continue;
            }
            return Err(String::from(
                "7 Days to Die returned an invalid player row.",
            ));
        };
        started = true;
        // ConsoleCmdListPlayers numbers rows from zero; listplayerids had a different contract.
        if index.parse::<usize>().ok() != Some(output.total) {
            return Err(String::from(
                "7 Days to Die player response has an incomplete row sequence.",
            ));
        }
        parse_row(rest, output)?;
    }
    if declared_count != Some(output.total) {
        return Err(String::from(
            "7 Days to Die player response is incomplete or its count does not match.",
        ));
    }
    Ok(())
}

fn parse_row(line: &str, output: &mut ListBuilder<'_>) -> Result<(), String> {
    // Consume fixed fields from the end so commas and field-like text in names cannot shift identity.
    let (rest, ping) = take_last(line, ", ping=")?;
    let ping = ping.parse::<i32>().map_err(|_| invalid_row())?;
    let (rest, ip) = take_last(rest, ", ip=")?;
    // Network transports return an empty address while a client connection is disappearing.
    if !ip.is_empty() && ip != "<unknown>" && ip.parse::<std::net::IpAddr>().is_err() {
        return Err(invalid_row());
    }
    let (rest, cross_id) = take_last(rest, ", crossid=")?;
    let (rest, platform_id) = take_last(rest, ", pltfmid=")?;
    let cross_identity = account_identifier(cross_id)?;
    let platform_identity = account_identifier(platform_id)?;
    let mut rest = rest;
    for delimiter in [
        ", level=",
        ", score=",
        ", players=",
        ", zombies=",
        ", deaths=",
        ", health=",
    ] {
        let (remaining, value) = take_last(rest, delimiter)?;
        if value.parse::<i32>().is_err() {
            return Err(invalid_row());
        }
        rest = remaining;
    }
    let (rest, remote) = take_last(rest, ", remote=")?;
    if !matches!(remote, "True" | "False") {
        return Err(invalid_row());
    }
    let (rest, rotation) = take_last(rest, ", rot=")?;
    let (rest, position) = take_last(rest, ", pos=")?;
    if !vector3(rotation) || !vector3(position) {
        return Err(invalid_row());
    }
    let (session_id, name) = rest.split_once(", ").ok_or_else(invalid_row)?;
    if !decimal(session_id) {
        return Err(invalid_row());
    }

    // Match ClientInfo.InternalId: CrossplatformId ?? PlatformId. Never fall back from an
    // unsupported cross-platform namespace to another account or a reusable entity ID.
    let (canonical, identity) = if cross_id != "<unknown>" {
        (cross_id, cross_identity)
    } else {
        (platform_id, platform_identity)
    };
    let mut identifiers = Vec::new();
    let actionable = identity.is_some();
    if let Some(identity) = identity {
        identifiers.push(identity);
    }
    identifiers.push(identifier(
        RuntimePlayerIdentityKind::SessionId,
        session_id,
        false,
    ));
    let mut entry = player(name, identifiers)?;
    entry.ping_ms = u32::try_from(ping).ok();
    if platform_id != "<unknown>" && platform_id != canonical {
        // Preserve the alternate account as display metadata, not a competing roster identity.
        entry.attributes.push(attribute("platform_id", platform_id));
    }
    // Neither private IP addresses nor location telemetry leave this collector.
    let targets = if actionable {
        vec![("kick_player", canonical), ("ban_player", canonical)]
    } else {
        Vec::new()
    };
    output.push(entry, &targets)
}

fn account_identifier(value: &str) -> Result<Option<RuntimeLivePlayerIdentifier>, String> {
    if value == "<unknown>" {
        return Ok(None);
    }
    validate_field(value)?;
    let (platform, id) = value.split_once('_').ok_or_else(invalid_row)?;
    if platform.is_empty()
        || !platform.bytes().all(|byte| byte.is_ascii_alphabetic())
        || id.is_empty()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
    {
        return Err(invalid_row());
    }
    let kind = match platform {
        "Steam" if steam_id(id) => RuntimePlayerIdentityKind::SteamId,
        "EOS"
            if (8..=32).contains(&id.len()) && id.bytes().all(|byte| byte.is_ascii_hexdigit()) =>
        {
            RuntimePlayerIdentityKind::EosId
        }
        "Steam" | "EOS" => return Err(invalid_row()),
        // The live-player model has no XBL/PSN namespace; leave those rows read-only.
        _ => return Ok(None),
    };
    Ok(Some(identifier(kind, id, true)))
}

fn take_last<'a>(line: &'a str, delimiter: &str) -> Result<(&'a str, &'a str), String> {
    line.rsplit_once(delimiter).ok_or_else(invalid_row)
}

fn vector3(value: &str) -> bool {
    let Some(inner) = value
        .strip_prefix('(')
        .and_then(|value| value.strip_suffix(')'))
    else {
        return false;
    };
    let mut parts = inner.split(',');
    for _ in 0..3 {
        if !parts
            .next()
            .is_some_and(|part| part.trim().parse::<f32>().is_ok_and(f32::is_finite))
        {
            return false;
        }
    }
    parts.next().is_none()
}

fn invalid_row() -> String {
    String::from("7 Days to Die returned an invalid or incomplete listplayers row.")
}
