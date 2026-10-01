use serde_json::Value;

use super::{ListBuilder, RuntimePlayerIdentityKind, identifier, player, steam_id};

pub(super) fn parse(text: &str, now: u64, output: &mut ListBuilder<'_>) -> Result<(), String> {
    let value: Value = serde_json::from_str(text)
        .map_err(|_| String::from("Rust playerlist did not return valid JSON."))?;
    let rows = value
        .as_array()
        .ok_or_else(|| String::from("Rust playerlist must return a JSON array."))?;
    for row in rows {
        let id = row
            .get("SteamID")
            .and_then(Value::as_str)
            .filter(|value| steam_id(value))
            .ok_or_else(|| String::from("Rust playerlist contains an invalid Steam ID."))?;
        let name = row
            .get("DisplayName")
            .and_then(Value::as_str)
            .ok_or_else(|| String::from("Rust playerlist is missing a player name."))?;
        let mut entry = player(
            name,
            vec![identifier(RuntimePlayerIdentityKind::SteamId, id, true)],
        )?;
        if let Some(ping) = row.get("Ping") {
            entry.ping_ms = Some(
                ping.as_u64()
                    .and_then(|value| u32::try_from(value).ok())
                    .ok_or_else(|| String::from("Rust playerlist contains an invalid ping."))?,
            );
        }
        if let Some(seconds) = row.get("ConnectedSeconds") {
            let seconds = seconds
                .as_f64()
                .filter(|value| value.is_finite() && *value >= 0.0 && *value <= 1_000_000_000.0)
                .ok_or_else(|| {
                    String::from("Rust playerlist contains an invalid connection duration.")
                })?;
            entry.session_started_at_unix_ms = now.checked_sub((seconds * 1000.0) as u64);
        }
        // Address and anti-cheat telemetry in the raw response are deliberately not projected.
        output.push(entry, &[("kick_player", id)])?;
    }
    Ok(())
}
