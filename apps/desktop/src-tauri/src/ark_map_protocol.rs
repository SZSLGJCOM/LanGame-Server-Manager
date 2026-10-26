use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Projection {
    pub min_x: f64,
    pub max_x: f64,
    pub min_y: f64,
    pub max_y: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Player {
    pub player_id: u32,
    pub name: String,
    pub account_id: String,
    pub team: i32,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub z: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Snapshot {
    pub map_name: String,
    pub projection: Option<Projection>,
    pub players: Vec<Player>,
}

pub fn snapshot(
    text: &str,
    module: &str,
    request_id: &str,
    expected_map: &str,
) -> Result<Snapshot, String> {
    let payload =
        super::commands_ark_tools::protocol::player_snapshot_payload(text, module, request_id)?;
    let value: Snapshot = serde_json::from_value(payload)
        .map_err(|error| format!("Invalid ARK player snapshot: {error}"))?;
    if !value.map_name.eq_ignore_ascii_case(expected_map) || value.players.len() > 512 {
        return Err(
            "ARK player snapshot belongs to a different map or exceeds 512 players.".into(),
        );
    }
    if let Some(projection) = &value.projection {
        if [
            projection.min_x,
            projection.max_x,
            projection.min_y,
            projection.max_y,
        ]
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 10_000_000.0)
            || projection.max_x <= projection.min_x
            || projection.max_y <= projection.min_y
        {
            return Err("Invalid ARK map projection.".into());
        }
    }
    let mut ids = HashSet::new();
    for player in &value.players {
        let coordinates = [player.x, player.y, player.z];
        if player.player_id == 0
            || !ids.insert(player.player_id)
            || player.name.len() > 1024
            || player.account_id.len() > 256
            || player.name.chars().any(char::is_control)
            || player.account_id.chars().any(char::is_control)
            || coordinates.iter().any(Option::is_some) && coordinates.iter().any(Option::is_none)
            || coordinates
                .into_iter()
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 10_000_000.0)
        {
            return Err("Invalid or duplicate ARK player record.".into());
        }
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "0123456789abcdef0123456789abcdef";
    fn response() -> serde_json::Value {
        serde_json::json!({"version":1,"edition":"ase","action":"players","requestId":ID,"ok":true,
            "mapName":"TheIsland","projection":{"minX":-400000.0,"maxX":400000.0,"minY":-400000.0,"maxY":400000.0},
            "players":[{"playerId":123,"accountId":"76561198000000000","name":"玩家 | Name","team":1000,"x":120.0,"y":240.0,"z":360.0}]})
    }
    fn parse(value: &serde_json::Value) -> Result<Snapshot, String> {
        snapshot(
            &format!("LGSM_ARK_TOOLS {value}"),
            "arksurvivalevolved",
            ID,
            "TheIsland",
        )
    }
    #[test]
    fn accepts_native_player_ids_without_coercing_account_ids() {
        let value = parse(&response()).unwrap();
        assert_eq!(value.players[0].player_id, 123);
        assert_eq!(value.players[0].account_id, "76561198000000000");
        assert_eq!(value.players[0].x, Some(120.0));
    }
    #[test]
    fn rejects_foreign_map_edition_request_and_duplicate_players() {
        for (key, value) in [
            ("mapName", "ScorchedEarth_P"),
            ("edition", "asa"),
            ("requestId", "wrong"),
            ("action", "spawn"),
        ] {
            let mut reply = response();
            reply[key] = value.into();
            assert!(parse(&reply).is_err(), "{key}");
        }
        let mut reply = response();
        let duplicate = reply["players"][0].clone();
        reply["players"].as_array_mut().unwrap().push(duplicate);
        assert!(parse(&reply).is_err());
    }
    #[test]
    fn unspawned_players_have_no_fabricated_coordinates() {
        let mut reply = response();
        for key in ["x", "y", "z"] {
            reply["players"][0][key] = serde_json::Value::Null;
        }
        assert!(parse(&reply).unwrap().players[0].x.is_none());
        reply["players"][0]["z"] = 1.into();
        assert!(parse(&reply).is_err());
    }
    #[test]
    fn rejects_unbounded_records_and_invalid_projection() {
        let mut reply = response();
        reply["players"][0]["x"] = 10000001.into();
        assert!(parse(&reply).is_err());
        reply = response();
        reply["projection"]["maxX"] = (-400000).into();
        assert!(parse(&reply).is_err());
        reply = response();
        reply["players"][0]["name"] = "x".repeat(1025).into();
        assert!(parse(&reply).is_err());
        reply = response();
        reply["players"] = serde_json::json!(vec![reply["players"][0].clone(); 513]);
        assert!(parse(&reply).is_err());
    }

    #[test]
    fn populated_map_survives_fragmented_rcon_without_truncating_players() {
        let mut reply = response();
        reply["players"] = serde_json::json!((1..=300).map(|id| serde_json::json!({
            "playerId":id,"accountId":format!("765611980000{id:05}"),"name":format!("玩家 {id}"),
            "team":1000,"x":100.0,"y":200.0,"z":300.0
        })).collect::<Vec<_>>());
        let envelope = format!("LGSM_ARK_TOOLS {reply}\n");
        assert!(envelope.len() > 16384);
        let mut wire = Vec::new();
        let mut offset = 0;
        while offset < envelope.len() {
            let mut end = (offset + 8192).min(envelope.len());
            while !envelope.is_char_boundary(end) {
                end -= 1;
            }
            crate::runtime_transport::source_rcon_write_packet(
                &mut wire,
                42,
                0,
                &envelope[offset..end],
            )
            .unwrap();
            offset = end;
        }
        crate::runtime_transport::source_rcon_write_packet(&mut wire, 43, 0, "").unwrap();
        let mut stream = std::io::Cursor::new(wire);
        let first = crate::runtime_transport::source_rcon_read_packet(&mut stream).unwrap();
        let joined = crate::runtime_transport::source_rcon_read_command_response(
            &mut stream,
            42,
            43,
            first.body,
        )
        .unwrap();
        let parsed = snapshot(&joined, "arksurvivalevolved", ID, "TheIsland").unwrap();
        assert_eq!(parsed.players.len(), 300);
        assert_eq!(parsed.players[299].name, "玩家 300");
    }
}
