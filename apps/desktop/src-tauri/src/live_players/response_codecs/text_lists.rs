use super::{ListBuilder, RuntimePlayerIdentityKind, identifier, player};

pub(super) fn zomboid(text: &str, output: &mut ListBuilder<'_>) -> Result<(), String> {
    let normalized = text.replace(" <LINE> ", "\n");
    let remainder = normalized
        .strip_prefix("Players connected (")
        .ok_or_else(|| String::from("Project Zomboid did not return a player-list header."))?;
    let (count, body) = remainder
        .split_once("):")
        .ok_or_else(|| String::from("Project Zomboid returned an invalid player-list header."))?;
    let count = count
        .parse::<usize>()
        .map_err(|_| String::from("Project Zomboid returned an invalid player count."))?;
    for line in body.lines().filter(|line| !line.trim().is_empty()) {
        let name = line
            .strip_prefix('-')
            .ok_or_else(|| String::from("Project Zomboid returned an invalid player row."))?;
        let entry = player(
            name,
            vec![identifier(
                RuntimePlayerIdentityKind::PlayerName,
                name,
                false,
            )],
        )?;
        output.push(
            entry,
            &[
                ("kick_user", name),
                ("ban_user", name),
                ("add_to_whitelist", name),
            ],
        )?;
    }
    if count != output.total {
        return Err(String::from(
            "Project Zomboid player count does not match its response.",
        ));
    }
    Ok(())
}
