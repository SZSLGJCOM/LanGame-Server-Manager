use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DstShardLayout {
    #[default]
    Standard,
    IslandAdventures,
}

/// Fixed native roles supported by the managed DST cluster. Directory names are
/// also the native shard names and must agree with server.ini and console peers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DstShardSpec {
    pub directory: &'static str,
    pub process_key: &'static str,
    pub game_port: &'static str,
    pub steam_query_port: &'static str,
    pub steam_auth_port: &'static str,
    pub id: &'static str,
}

pub const DST_SHARDS: [DstShardSpec; 4] = [
    DstShardSpec {
        directory: "Master",
        process_key: "master",
        game_port: "master",
        steam_query_port: "steam_query",
        steam_auth_port: "steam_auth",
        id: "1",
    },
    DstShardSpec {
        directory: "Caves",
        process_key: "caves",
        game_port: "caves",
        steam_query_port: "caves_steam_query",
        steam_auth_port: "caves_steam_auth",
        id: "2",
    },
    DstShardSpec {
        directory: "Islands",
        process_key: "islands",
        game_port: "islands",
        steam_query_port: "islands_steam_query",
        steam_auth_port: "islands_steam_auth",
        id: "3",
    },
    DstShardSpec {
        directory: "Volcano",
        process_key: "volcano",
        game_port: "volcano",
        steam_query_port: "volcano_steam_query",
        steam_auth_port: "volcano_steam_auth",
        id: "4",
    },
];

pub fn dst_shard_layout(settings: &Value) -> Result<DstShardLayout, String> {
    match settings.get("shard_layout") {
        None => Ok(DstShardLayout::Standard),
        Some(Value::String(layout)) if layout == "standard" => Ok(DstShardLayout::Standard),
        Some(Value::String(layout)) if layout == "island_adventures" => {
            Ok(DstShardLayout::IslandAdventures)
        }
        Some(_) => Err(String::from(
            "Invalid DST shard_layout; expected standard or island_adventures",
        )),
    }
}

pub fn dst_shards(settings: &Value) -> Result<Vec<&'static DstShardSpec>, String> {
    let count = match dst_shard_layout(settings)? {
        DstShardLayout::IslandAdventures => DST_SHARDS.len(),
        DstShardLayout::Standard => {
            if settings
                .get("enable_caves")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                2
            } else {
                1
            }
        }
    };
    Ok(DST_SHARDS[..count].iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn standard_layout_preserves_the_optional_caves_behavior() {
        assert_eq!(dst_shards(&json!({})).unwrap(), [&DST_SHARDS[0]]);
        assert_eq!(
            dst_shards(&json!({ "enable_caves": true })).unwrap(),
            [&DST_SHARDS[0], &DST_SHARDS[1]]
        );
    }

    #[test]
    fn island_adventures_requires_all_four_roles_even_if_caves_flag_is_false() {
        let shards = dst_shards(&json!({
            "shard_layout": "island_adventures",
            "enable_caves": false
        }))
        .unwrap();
        assert_eq!(
            shards
                .iter()
                .map(|shard| shard.directory)
                .collect::<Vec<_>>(),
            ["Master", "Caves", "Islands", "Volcano"]
        );
        for left in &shards {
            for right in &shards {
                if left.directory != right.directory {
                    assert_ne!(left.id, right.id);
                    assert_ne!(left.game_port, right.game_port);
                    assert_ne!(left.steam_query_port, right.steam_query_port);
                    assert_ne!(left.steam_auth_port, right.steam_auth_port);
                }
            }
        }
    }

    #[test]
    fn invalid_layouts_fail_without_echoing_untrusted_configuration() {
        for layout in [json!("unknown"), json!(null), json!(3), json!({})] {
            let error = dst_shards(&json!({ "shard_layout": layout })).unwrap_err();
            assert!(error.starts_with("Invalid DST shard_layout"));
            assert!(!error.contains("unknown"));
        }
    }
}
