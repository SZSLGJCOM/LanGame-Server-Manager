use super::*;
use crate::live_players::cache::CachedLivePlayerSnapshot;

pub(super) async fn collect_direct_players(
    state: &tauri::State<'_, DesktopState>,
    details: &InstanceDetails,
    descriptor: &ModuleDescriptor,
    observed_at: u64,
) -> LivePlayerCollectionResult {
    use app_core::ModulePlayerListSource as Source;
    let instance_id = &details.summary.id;
    let request_id = new_request_id();
    let list = descriptor
        .runtime
        .player_list
        .as_ref()
        .ok_or_else(|| Box::new(unsupported_snapshot(instance_id, request_id.clone())))?;
    let failure = |code, summary: &str, truncated| {
        Box::new(failed_snapshot(
            instance_id,
            request_id.clone(),
            list.source,
            code,
            summary,
            truncated,
        ))
    };
    match (list.source, list.response_codec) {
        (Source::FileIpc, ModulePlayerListCodec::ScumPlayers) => {
            return crate::live_players::file_ipc::collect(
                crate::live_players::file_ipc::Game::Scum,
                details,
                instance_id,
                &request_id,
                observed_at,
            )
            .await;
        }
        (Source::FileIpc, ModulePlayerListCodec::WindrosePlayers) => {
            return crate::live_players::file_ipc::collect(
                crate::live_players::file_ipc::Game::Windrose,
                details,
                instance_id,
                &request_id,
                observed_at,
            )
            .await;
        }
        (Source::FileIpc, ModulePlayerListCodec::DragonwildsPlayers) => {
            return crate::live_players::file_ipc::collect(
                crate::live_players::file_ipc::Game::Dragonwilds,
                details,
                instance_id,
                &request_id,
                observed_at,
            )
            .await;
        }
        (Source::NativeConsole, ModulePlayerListCodec::ReturnToMoriaPlayers) => {
            return crate::live_players::returntomoria::collect_returntomoria(
                details,
                instance_id,
                &request_id,
                observed_at,
            )
            .await;
        }
        (Source::TcpConsole, ModulePlayerListCodec::SoulmaskPlayers) => {
            return crate::live_players::soulmask::collect_soulmask(
                details,
                instance_id,
                &request_id,
                observed_at,
            )
            .await;
        }
        (Source::HttpApi, ModulePlayerListCodec::SatisfactoryFrmPlayers) => {
            return crate::live_players::satisfactory::collect_satisfactory(
                details,
                instance_id,
                &request_id,
                observed_at,
            )
            .await;
        }
        (Source::TcpConsole, ModulePlayerListCodec::AstroneerPlayers) => {
            return crate::live_players::astroneer::collect_astroneer(
                details,
                instance_id,
                &request_id,
                observed_at,
            )
            .await;
        }
        (Source::HttpApi, ModulePlayerListCodec::PalworldPlayers) => {
            return crate::live_players::palworld::collect_palworld(
                details,
                instance_id,
                &request_id,
                observed_at,
                &list.player_action_ids,
            )
            .await;
        }
        (Source::HttpApi, ModulePlayerListCodec::NightingalePlayers) => {
            return crate::live_players::nightingale::collect_nightingale(
                details,
                instance_id,
                &request_id,
                observed_at,
            )
            .await;
        }
        (Source::ServerQuery, ModulePlayerListCodec::A2sPlayers) => {
            let query = descriptor
                .runtime
                .player_query
                .as_ref()
                .filter(|query| query.protocol == "a2s_info")
                .ok_or_else(|| {
                    failure(
                        RuntimeLivePlayerIssueCode::QueryUnavailable,
                        "This module has no A2S query port contract.",
                        false,
                    )
                })?;
            return crate::live_players::server_query::collect_a2s_players(
                details,
                query,
                &request_id,
                observed_at,
            )
            .await;
        }
        (Source::RuntimeAction | Source::ConsoleLog, _) => {}
        _ => {
            return Err(failure(
                RuntimeLivePlayerIssueCode::AdapterUnavailable,
                "No collector is configured for this player-list source.",
                false,
            ));
        }
    }
    let action = list
        .action_id
        .as_ref()
        .and_then(|id| {
            descriptor
                .runtime
                .player_actions
                .iter()
                .find(|action| &action.id == id)
        })
        .ok_or_else(|| {
            failure(
                RuntimeLivePlayerIssueCode::RuntimeActionUnavailable,
                "The player-list command is not declared.",
                false,
            )
        })?;
    if let Some(keys) = missing_query_settings(details, action) {
        return Err(misconfigured_snapshot(
            instance_id,
            request_id.clone(),
            list.source,
            RuntimeLivePlayerIssueCode::RuntimeActionUnavailable,
            "Configure the server's remote command interface to read players.",
            keys,
        )
        .into());
    }
    let run_id = details
        .active_run
        .as_ref()
        .ok_or_else(|| {
            failure(
                RuntimeLivePlayerIssueCode::ProcessUnavailable,
                "The server is not running.",
                false,
            )
        })?
        .run_id;
    if list.source == Source::ConsoleLog {
        let log_path = details
            .active_run
            .as_ref()
            .and_then(|run| {
                run.processes.iter().find(|process| {
                    action
                        .process_key
                        .as_ref()
                        .map_or(process.is_primary, |key| key == &process.process_key)
                })
            })
            .and_then(|process| process.log_path.as_deref())
            .filter(|path| !path.is_empty())
            .ok_or_else(|| {
                failure(
                    RuntimeLivePlayerIssueCode::LogUnavailable,
                    "The server has no readable console log.",
                    false,
                )
            })?;
        let command_request_id = (list.response_codec == ModulePlayerListCodec::BarotraumaPlayers)
            .then_some(request_id.as_str());
        return crate::live_players::console_log::collect_console_players(
            crate::live_players::console_log::ConsoleCollection {
                instance_id,
                request_id: &request_id,
                codec: list.response_codec,
                log_path: std::path::Path::new(log_path),
                observed_at,
            },
            |submission, deadline| async move {
                dispatch_declared_runtime_action_until(
                    state,
                    DeclaredRuntimeActionRequest {
                        instance_id,
                        expected_run_id: run_id,
                        action_id: &action.id,
                        target: None,
                        role: None,
                        request_id: command_request_id,
                        require_target_binding: false,
                    },
                    deadline,
                    submission,
                )
                .await
                .map(|_| ())
            },
        )
        .await;
    }
    let response = dispatch_declared_runtime_action(
        state,
        DeclaredRuntimeActionRequest {
            instance_id,
            expected_run_id: run_id,
            action_id: &action.id,
            target: None,
            role: None,
            request_id: None,
            require_target_binding: false,
        },
    )
    .await
    .map_err(|error| {
        let code = if error.to_ascii_lowercase().contains("auth")
            || error.to_ascii_lowercase().contains("password")
        {
            RuntimeLivePlayerIssueCode::AuthenticationFailed
        } else {
            RuntimeLivePlayerIssueCode::QueryUnavailable
        };
        failure(
            code,
            "The server did not complete its player-list command.",
            false,
        )
    })?;
    let text = response.response_text.as_deref().ok_or_else(|| {
        failure(
            RuntimeLivePlayerIssueCode::ProtocolIncomplete,
            "The player-list command returned no complete response.",
            false,
        )
    })?;
    if list.response_codec == ModulePlayerListCodec::MinecraftPlayers {
        return crate::live_players::minecraft::parse_minecraft(
            instance_id,
            text,
            &request_id,
            observed_at,
        );
    }
    let parsed = crate::live_players::response_codecs::parse(
        list.response_codec,
        text,
        &request_id,
        observed_at,
        &list.player_action_ids,
    )
    .map_err(|_| {
        failure(
            RuntimeLivePlayerIssueCode::ProtocolIncomplete,
            "The server returned an unrecognized or incomplete player list.",
            false,
        )
    })?;
    Ok(CachedLivePlayerSnapshot {
        public_snapshot: RuntimeLivePlayerSnapshot {
            snapshot_id: request_id,
            instance_id: instance_id.to_owned(),
            status: app_core::RuntimeLivePlayerStatus::Ready,
            source: Some(list.source),
            observed_at_unix_ms: Some(observed_at),
            expires_at_unix_ms: None,
            complete: parsed.complete,
            truncated: parsed.truncated,
            stale: false,
            current_players: parsed.current_players,
            max_players: parsed.max_players,
            entries: parsed.entries,
            issue: None,
        },
        private_action_bindings: parsed.bindings,
        collected_at: observed_at,
    })
}

pub(super) fn missing_query_settings(
    details: &InstanceDetails,
    action: &app_core::ModulePlayerActionSpec,
) -> Option<Vec<String>> {
    let Ok(settings) = serde_json::from_str::<serde_json::Value>(&details.settings_json) else {
        return Some(vec![String::from("settings_json")]);
    };
    let mut keys = Vec::new();
    if let Some(key) = &action.enabled_setting_key
        && settings.get(key).and_then(serde_json::Value::as_bool) != Some(true)
    {
        keys.push(key.clone());
    }
    if let Some(key) = &action.password_setting_key
        && settings
            .get(key)
            .and_then(serde_json::Value::as_str)
            .is_none_or(|value| value.trim().is_empty())
    {
        keys.push(key.clone());
    }
    if let Some(name) = &action.port_name
        && !details
            .ports
            .iter()
            .any(|port| &port.name == name && port.port > 0)
    {
        keys.push(name.clone());
    }
    (!keys.is_empty()).then_some(keys)
}
