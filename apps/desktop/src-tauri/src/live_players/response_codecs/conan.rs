use super::{
    ListBuilder, RuntimePlayerIdentityKind, attribute, eos_id, identifier, player, steam_id,
};

pub(super) fn parse(text: &str, output: &mut ListBuilder<'_>) -> Result<(), String> {
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let header: Vec<_> = lines
        .next()
        .unwrap_or_default()
        .split('|')
        .map(str::trim)
        .collect();
    if header
        != [
            "Idx",
            "Char name",
            "Player name",
            "User ID",
            "Platform ID",
            "Platform Name",
        ]
    {
        return Err(String::from(
            "Conan Exiles returned an unknown player-list header.",
        ));
    }
    for line in lines {
        let fields: Vec<_> = line.split('|').map(str::trim).collect();
        if fields.len() != 6 || fields[0].parse::<usize>().ok() != Some(output.total) {
            return Err(String::from(
                "Conan Exiles returned an incomplete or ambiguous player row.",
            ));
        }
        let user_id = fields[3];
        if user_id.is_empty()
            || user_id.len() > 128
            || !user_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err(String::from("Conan Exiles returned an invalid user ID."));
        }
        let mut identifiers = vec![identifier(
            RuntimePlayerIdentityKind::ConanUserId,
            user_id,
            true,
        )];
        if fields[5].eq_ignore_ascii_case("Steam") && steam_id(fields[4]) {
            identifiers.push(identifier(
                RuntimePlayerIdentityKind::SteamId,
                fields[4],
                true,
            ));
        } else if fields[5].eq_ignore_ascii_case("Epic") && eos_id(fields[4]) {
            identifiers.push(identifier(
                RuntimePlayerIdentityKind::EosId,
                fields[4],
                true,
            ));
        }
        let name = if fields[1].is_empty() {
            fields[2]
        } else {
            fields[1]
        };
        let mut entry = player(name, identifiers)?;
        if !fields[2].is_empty() {
            super::validate_field(fields[2])?;
            entry.attributes.push(attribute("account_name", fields[2]));
        }
        if !fields[5].is_empty() {
            super::validate_field(fields[5])?;
            entry.attributes.push(attribute("platform", fields[5]));
        }
        output.push(entry, &[("kick_player", user_id), ("ban_player", user_id)])?;
    }
    Ok(())
}
