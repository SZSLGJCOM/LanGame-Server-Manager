use app_core::{
    ModulePlayerActionSpec, ModulePlayerListCodec, ModulePlayerListScope, ModulePlayerListSource,
    ModulePlayerListSpec, RuntimePlayerIdentityKind,
};

use super::ModuleTomlPlayerList;

pub(super) fn player_list_spec_from_toml(
    player_list: ModuleTomlPlayerList,
    player_actions: &[ModulePlayerActionSpec],
) -> Result<ModulePlayerListSpec, String> {
    let scope = match player_list.scope.as_deref().unwrap_or("online") {
        "online" => ModulePlayerListScope::Online,
        value => return Err(format!("scope must be 'online', got '{value}'")),
    };
    let source = parse_enum(player_list.source.as_deref(), "source")?;
    let response_codec = parse_enum(player_list.response_codec.as_deref(), "response_codec")?;
    let identity_kind = parse_enum(player_list.identity_kind.as_deref(), "identity_kind")?;
    validate_source(source, response_codec, identity_kind)?;
    let action_id = player_list
        .action_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned);
    if matches!(
        source,
        ModulePlayerListSource::HttpApi
            | ModulePlayerListSource::ServerQuery
            | ModulePlayerListSource::TcpConsole
            | ModulePlayerListSource::NativeConsole
            | ModulePlayerListSource::FileIpc
    ) {
        if action_id.is_some()
            || (response_codec != ModulePlayerListCodec::PalworldPlayers
                && player_list
                    .player_action_ids
                    .as_ref()
                    .is_some_and(|ids| !ids.is_empty()))
        {
            return Err(String::from(
                "query-only sources must not declare runtime actions",
            ));
        }
    } else {
        let list_action_id = action_id
            .as_deref()
            .ok_or_else(|| String::from("action_id is required"))?;
        let list_action = player_actions
            .iter()
            .find(|action| action.id == list_action_id)
            .ok_or_else(|| format!("action_id '{list_action_id}' is not declared"))?;
        let list_action_placeholders = template_placeholder_names(&list_action.command_template)
            .map_err(|message| format!("list action '{list_action_id}' has {message}"))?;

        if list_action.target_required || list_action_placeholders.contains("target") {
            return Err(String::from("list action must be target-free"));
        }
        if list_action.destructive {
            return Err(String::from("list action must be non-destructive"));
        }
        if source == ModulePlayerListSource::StructuredLog {
            if list_action.transport != "stdin" {
                return Err(String::from(
                    "structured_log list action must use stdin transport",
                ));
            }
            if list_action_placeholders.len() != 1
                || !list_action_placeholders.contains("request_id")
            {
                return Err(String::from(
                    "structured_log list action must bind only the reserved {{request_id}} placeholder",
                ));
            }
        }
        if source == ModulePlayerListSource::RuntimeAction {
            if !matches!(
                list_action.transport.as_str(),
                "source_rcon" | "humanitz_rcon" | "websocket_rcon" | "telnet" | "battleye_rcon"
            ) {
                return Err(String::from(
                    "runtime_action player lists require a direct response transport",
                ));
            }
            if !list_action_placeholders.is_empty() {
                return Err(String::from(
                    "direct player-list actions must not have placeholders",
                ));
            }
        }
        if source == ModulePlayerListSource::ConsoleLog {
            let valid_placeholders = if response_codec == ModulePlayerListCodec::BarotraumaPlayers {
                list_action_placeholders.len() == 1
                    && list_action_placeholders.contains("request_id")
            } else {
                list_action_placeholders.is_empty()
            };
            if list_action.transport != "stdin" || !valid_placeholders {
                return Err(String::from(
                    "console_log player lists require stdin and the codec's request-correlation contract",
                ));
            }
        }
    }
    let player_action_ids = player_list.player_action_ids.unwrap_or_default();
    if source == ModulePlayerListSource::ConsoleLog && !player_action_ids.is_empty() {
        return Err(String::from("uncorrelated console names are read-only"));
    }
    let mut seen_action_ids = std::collections::HashSet::new();
    for action_id in &player_action_ids {
        if !seen_action_ids.insert(action_id.as_str()) {
            return Err(format!("duplicate player_action_ids entry '{action_id}'"));
        }
        let action = player_actions
            .iter()
            .find(|action| action.id == *action_id)
            .ok_or_else(|| format!("player action '{action_id}' is not declared"))?;
        if response_codec == ModulePlayerListCodec::PalworldPlayers {
            let command = match action_id.as_str() {
                "kick_player" => "kick {{target}}",
                "ban_player" => "ban {{target}}",
                _ => {
                    return Err(format!(
                        "Palworld player rows do not support action '{action_id}'"
                    ));
                }
            };
            if action.transport != "palworld_rest"
                || action.command_template != command
                || action.port_name.as_deref() != Some("rest_api")
                || action.password_setting_key.as_deref() != Some("admin_password")
                || action.enabled_setting_key.as_deref() != Some("rest_api_enabled")
                || action.process_key.is_some()
                || !action.destructive
            {
                return Err(format!(
                    "Palworld player action '{action_id}' must use its authenticated REST contract"
                ));
            }
        }
        let action_placeholders = template_placeholder_names(&action.command_template)
            .map_err(|message| format!("player action '{action_id}' has {message}"))?;
        if !action.target_required {
            return Err(format!("player action '{action_id}' must require a target"));
        }
        if !action_placeholders.contains("target") {
            return Err(format!(
                "player action '{action_id}' must bind {{{{target}}}}"
            ));
        }
        if action_placeholders.contains("request_id") {
            return Err(format!(
                "player action '{action_id}' must not bind {{{{request_id}}}}"
            ));
        }
        if action_placeholders.contains("role") {
            return Err(format!(
                "player action '{action_id}' must not require a role in the live-player row contract"
            ));
        }
        if let Some(unsupported) = action_placeholders
            .iter()
            .copied()
            .find(|placeholder| *placeholder != "target")
        {
            return Err(format!(
                "player action '{action_id}' contains unsupported placeholder '{{{{{unsupported}}}}}'"
            ));
        }
    }

    let refresh_interval_ms = player_list.refresh_interval_ms.unwrap_or(30_000);
    if !(15_000..=120_000).contains(&refresh_interval_ms) {
        return Err(String::from(
            "refresh_interval_ms must be within 15000..=120000",
        ));
    }

    Ok(ModulePlayerListSpec {
        scope,
        source,
        action_id,
        player_action_ids,
        response_codec,
        identity_kind,
        refresh_interval_ms,
    })
}

fn parse_enum<T: serde::de::DeserializeOwned>(
    value: Option<&str>,
    field: &str,
) -> Result<T, String> {
    let value = value.ok_or_else(|| format!("{field} is required"))?;
    let deserializer = serde::de::value::StrDeserializer::<serde::de::value::Error>::new(value);
    T::deserialize(deserializer).map_err(|_| format!("unknown {field} '{value}'"))
}

fn validate_source(
    source: ModulePlayerListSource,
    codec: ModulePlayerListCodec,
    identity: RuntimePlayerIdentityKind,
) -> Result<(), String> {
    use ModulePlayerListCodec as Codec;
    use ModulePlayerListSource as Source;
    use RuntimePlayerIdentityKind as Identity;
    let valid = match codec {
        Codec::DstClientTableV1 => {
            source == Source::StructuredLog && identity == Identity::KleiUserId
        }
        Codec::PalworldPlayers => source == Source::HttpApi && identity == Identity::PalworldUserId,
        Codec::NightingalePlayers => source == Source::HttpApi && identity == Identity::PlayerName,
        Codec::AstroneerPlayers => {
            source == Source::TcpConsole && identity == Identity::AstroneerGuid
        }
        Codec::SoulmaskPlayers => source == Source::TcpConsole && identity == Identity::SteamId,
        Codec::SatisfactoryFrmPlayers => {
            source == Source::HttpApi && identity == Identity::PlayerName
        }
        Codec::A2sPlayers => source == Source::ServerQuery && identity == Identity::PlayerName,
        Codec::BarotraumaPlayers => source == Source::ConsoleLog && identity == Identity::SessionId,
        Codec::ReturnToMoriaPlayers => {
            source == Source::NativeConsole && identity == Identity::PlayerName
        }
        Codec::WindrosePlayers | Codec::DragonwildsPlayers | Codec::ScumPlayers => {
            source == Source::FileIpc && identity == Identity::SessionId
        }
        Codec::NecessePlayers | Codec::RomesteadPlayers | Codec::TerrariaPlayers => {
            source == Source::ConsoleLog && identity == Identity::PlayerName
        }
        Codec::MinecraftPlayers => {
            source == Source::RuntimeAction && identity == Identity::MinecraftUuid
        }
        Codec::RustPlayerList => source == Source::RuntimeAction && identity == Identity::SteamId,
        Codec::ArkListPlayers => {
            source == Source::RuntimeAction
                && matches!(identity, Identity::ArkAccountId | Identity::EosId)
        }
        Codec::ZomboidPlayers => {
            source == Source::RuntimeAction && identity == Identity::PlayerName
        }
        Codec::ConanListPlayers => {
            source == Source::RuntimeAction && identity == Identity::ConanUserId
        }
        Codec::HumanitzPlayers => {
            source == Source::RuntimeAction && identity == Identity::PlayerName
        }
        Codec::SevenDaysPlayers | Codec::SquadListPlayers => {
            source == Source::RuntimeAction
                && matches!(
                    identity,
                    Identity::SteamId | Identity::EosId | Identity::SessionId
                )
        }
    };
    if valid {
        Ok(())
    } else {
        Err(format!(
            "player-list source and identity_kind do not match response_codec {codec:?}"
        ))
    }
}

fn template_placeholder_names(template: &str) -> Result<std::collections::HashSet<&str>, String> {
    let mut names = std::collections::HashSet::new();
    let mut remaining = template;

    while let Some(start) = remaining.find("{{") {
        let placeholder = &remaining[start + 2..];
        let end = placeholder
            .find("}}")
            .ok_or_else(|| String::from("an unterminated placeholder"))?;
        let name = &placeholder[..end];
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err(String::from("an invalid placeholder name"));
        }
        names.insert(name);
        remaining = &placeholder[end + 2..];
    }

    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(transport: &str, command: &str) -> ModulePlayerActionSpec {
        toml::from_str(&format!(
            "id = 'list'\nlabel = 'List players'\ntransport = '{transport}'\ncommand_template = '{command}'"
        )).expect("fixture action")
    }

    fn spec(source: &str, codec: &str, identity: &str, extra: &str) -> ModuleTomlPlayerList {
        toml::from_str(&format!(
            "source = '{source}'\nresponse_codec = '{codec}'\nidentity_kind = '{identity}'\n{extra}"
        ))
        .expect("fixture list")
    }

    #[test]
    fn query_only_lists_cannot_claim_actions_or_another_sources_identity() {
        let valid = spec("server_query", "a2s_players", "player_name", "");
        assert!(player_list_spec_from_toml(valid, &[]).is_ok());
        for (source, codec, identity) in [
            ("tcp_console", "astroneer_players", "astroneer_guid"),
            ("tcp_console", "soulmask_players", "steam_id"),
            ("native_console", "return_to_moria_players", "player_name"),
            ("file_ipc", "windrose_players", "session_id"),
            ("file_ipc", "dragonwilds_players", "session_id"),
            ("http_api", "satisfactory_frm_players", "player_name"),
        ] {
            assert!(player_list_spec_from_toml(spec(source, codec, identity, ""), &[]).is_ok());
            assert!(
                player_list_spec_from_toml(
                    spec(source, codec, identity, "action_id = 'list'"),
                    &[action("stdin", "players")]
                )
                .is_err()
            );
            assert!(
                player_list_spec_from_toml(
                    spec(source, codec, identity, "player_action_ids = ['kick']"),
                    &[]
                )
                .is_err()
            );
            assert!(
                player_list_spec_from_toml(spec("server_query", codec, identity, ""), &[]).is_err()
            );
        }
        for invalid in [
            spec("server_query", "a2s_players", "steam_id", ""),
            spec(
                "server_query",
                "a2s_players",
                "player_name",
                "action_id = 'list'",
            ),
            spec(
                "http_api",
                "palworld_players",
                "palworld_user_id",
                "player_action_ids = ['kick']",
            ),
            spec(
                "runtime_action",
                "palworld_players",
                "palworld_user_id",
                "action_id = 'list'",
            ),
        ] {
            assert!(player_list_spec_from_toml(invalid, &[]).is_err());
        }
    }

    #[test]
    fn palworld_rows_require_verified_rest_identity_actions() {
        let make = || {
            spec(
                "http_api",
                "palworld_players",
                "palworld_user_id",
                "player_action_ids = ['kick_player']",
            )
        };
        let mut kick = action("palworld_rest", "kick {{target}}");
        kick.id = String::from("kick_player");
        kick.target_required = true;
        kick.destructive = true;
        kick.port_name = Some(String::from("rest_api"));
        kick.password_setting_key = Some(String::from("admin_password"));
        kick.enabled_setting_key = Some(String::from("rest_api_enabled"));
        assert!(player_list_spec_from_toml(make(), &[kick.clone()]).is_ok());
        for mutation in 0..7 {
            let mut invalid = kick.clone();
            match mutation {
                0 => invalid.transport = String::from("source_rcon"),
                1 => invalid.command_template = String::from("kick another-player"),
                2 => invalid.port_name = Some(String::from("rcon")),
                3 => invalid.password_setting_key = None,
                4 => invalid.enabled_setting_key = None,
                5 => invalid.destructive = false,
                _ => invalid.target_required = false,
            }
            assert!(player_list_spec_from_toml(make(), &[invalid]).is_err());
        }
        let foreign = spec(
            "http_api",
            "nightingale_players",
            "player_name",
            "player_action_ids = ['kick_player']",
        );
        assert!(player_list_spec_from_toml(foreign, &[kick]).is_err());
    }

    #[test]
    fn console_name_lists_require_plain_stdin_and_are_read_only() {
        let make = || {
            spec(
                "console_log",
                "necesse_players",
                "player_name",
                "action_id = 'list'",
            )
        };
        assert!(player_list_spec_from_toml(make(), &[action("stdin", "players")]).is_ok());
        for invalid in [
            action("source_rcon", "players"),
            action("stdin", "players {{request_id}}"),
            action("stdin", "players {{target}}"),
        ] {
            assert!(player_list_spec_from_toml(make(), &[invalid]).is_err());
        }
        let bound = spec(
            "console_log",
            "necesse_players",
            "player_name",
            "action_id = 'list'\nplayer_action_ids = ['kick']",
        );
        assert!(player_list_spec_from_toml(bound, &[action("stdin", "players")]).is_err());
    }

    #[test]
    fn barotrauma_requires_a_correlated_read_only_console_command() {
        let make = || {
            spec(
                "console_log",
                "barotrauma_players",
                "session_id",
                "action_id = 'list'",
            )
        };
        assert!(
            player_list_spec_from_toml(
                make(),
                &[action(
                    "stdin",
                    "clientlist LGM_PLAYER_QUERY_{{request_id}}"
                )]
            )
            .is_ok()
        );
        for invalid in [
            action("stdin", "clientlist"),
            action("stdin", "clientlist {{request_id}} {{target}}"),
            action("source_rcon", "clientlist {{request_id}}"),
        ] {
            assert!(player_list_spec_from_toml(make(), &[invalid]).is_err());
        }
        let bound = spec(
            "console_log",
            "barotrauma_players",
            "session_id",
            "action_id = 'list'\nplayer_action_ids = ['kick']",
        );
        assert!(
            player_list_spec_from_toml(
                bound,
                &[action(
                    "stdin",
                    "clientlist LGM_PLAYER_QUERY_{{request_id}}"
                )]
            )
            .is_err()
        );
    }

    #[test]
    fn direct_lists_require_a_response_transport_including_humanitz() {
        let make = || {
            spec(
                "runtime_action",
                "humanitz_players",
                "player_name",
                "action_id = 'list'",
            )
        };
        assert!(player_list_spec_from_toml(make(), &[action("humanitz_rcon", "info")]).is_ok());
        assert!(player_list_spec_from_toml(make(), &[action("stdin", "info")]).is_err());
        assert!(
            player_list_spec_from_toml(make(), &[action("humanitz_rcon", "info {{target}}")])
                .is_err()
        );
    }
}
