use super::{
    ListBuilder, RuntimePlayerIdentityKind, decimal, eos_id, identifier, player, steam_id,
};

pub(super) fn parse(text: &str, output: &mut ListBuilder<'_>) -> Result<(), String> {
    if text.trim() == "No Players Connected" {
        return Ok(());
    }
    for (expected_index, line) in text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
    {
        let (index, rest) = line
            .trim()
            .split_once(". ")
            .ok_or_else(|| String::from("ARK ListPlayers contains an invalid player row."))?;
        if index.parse::<usize>().ok() != Some(expected_index) {
            return Err(String::from(
                "ARK ListPlayers has an incomplete row sequence.",
            ));
        }
        // Split at the final comma because display names may contain commas.
        let (name, id) = rest
            .rsplit_once(", ")
            .ok_or_else(|| String::from("ARK ListPlayers is missing a player identity."))?;
        let kind = if steam_id(id) {
            RuntimePlayerIdentityKind::SteamId
        } else if id.len() == 19 && decimal(id) {
            RuntimePlayerIdentityKind::ArkAccountId
        } else if eos_id(id) {
            RuntimePlayerIdentityKind::EosId
        } else {
            return Err(String::from(
                "ARK ListPlayers contains an invalid account ID.",
            ));
        };
        let entry = player(name, vec![identifier(kind, id, true)])?;
        output.push(entry, &[("kick_player", id), ("ban_player", id)])?;
    }
    Ok(())
}
