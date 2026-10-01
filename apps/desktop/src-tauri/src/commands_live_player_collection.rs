use super::*;
#[path = "commands_live_player_sources.rs"]
mod sources;
use sources::collect_direct_players;

pub(super) async fn refresh_live_players_uncached(
    state: &tauri::State<'_, DesktopState>,
    instance_id: &str,
    expected_key: &LivePlayerCacheKey,
    requested_at: u64,
) -> LivePlayerCollectionResult {
    let _mutation = state.acquire_instance_mutation(instance_id).await;
    let context = load_persisted_live_player_context(instance_id).await;
    let (_, details, descriptor) = context.map_err(|_| {
        failed_snapshot(
            instance_id,
            new_request_id(),
            app_core::ModulePlayerListSource::StructuredLog,
            RuntimeLivePlayerIssueCode::IoFailed,
            "Unable to reload server state for player collection.",
            false,
        )
    })?;
    let Some(player_list) = descriptor.runtime.player_list.as_ref() else {
        return Err(failed_snapshot(
            instance_id,
            new_request_id(),
            app_core::ModulePlayerListSource::StructuredLog,
            RuntimeLivePlayerIssueCode::RuntimeActionUnavailable,
            "The module no longer declares an online-player source.",
            false,
        )
        .into());
    };
    let source = player_list.source;
    let Some(run) = details.active_run.as_ref() else {
        return Err(failed_snapshot(
            instance_id,
            new_request_id(),
            source,
            RuntimeLivePlayerIssueCode::ProcessUnavailable,
            "The server stopped before its player list could be collected.",
            false,
        )
        .into());
    };
    if build_cache_key(&details, &descriptor).as_ref() != Some(expected_key) {
        return Err(failed_snapshot(
            instance_id,
            new_request_id(),
            source,
            RuntimeLivePlayerIssueCode::ProcessUnavailable,
            "The server run or player capability changed before refresh started.",
            false,
        )
        .into());
    }

    if source != app_core::ModulePlayerListSource::StructuredLog {
        let process_key = player_list
            .action_id
            .as_ref()
            .and_then(|id| {
                descriptor
                    .runtime
                    .player_actions
                    .iter()
                    .find(|action| &action.id == id)
            })
            .and_then(|action| action.process_key.as_deref());
        let process = run
            .processes
            .iter()
            .find(|process| {
                process_key.map_or(process.is_primary, |key| process.process_key == key)
                    && process.pid.is_some()
                    && process.status.eq_ignore_ascii_case("running")
            })
            .ok_or_else(|| {
                Box::new(failed_snapshot(
                    instance_id,
                    new_request_id(),
                    source,
                    RuntimeLivePlayerIssueCode::ProcessUnavailable,
                    "The server has no active player-query process.",
                    false,
                ))
            })?;
        verify_collection_process(state, instance_id, run.run_id, process, source)?;
        let collected = collect_direct_players(state, &details, &descriptor, requested_at).await?;
        verify_collection_process(state, instance_id, run.run_id, process, source)?;
        return Ok(collected);
    }

    let Some(list_action_id) = player_list.action_id.as_deref() else {
        return Err(misconfigured_snapshot(
            instance_id,
            new_request_id(),
            source,
            RuntimeLivePlayerIssueCode::RuntimeActionUnavailable,
            "The module does not declare its player-list action.",
            Vec::new(),
        )
        .into());
    };
    let Some(list_action) = descriptor
        .runtime
        .player_actions
        .iter()
        .find(|action| action.id == list_action_id)
    else {
        return Err(misconfigured_snapshot(
            instance_id,
            new_request_id(),
            source,
            RuntimeLivePlayerIssueCode::RuntimeActionUnavailable,
            "The declared player-list action is unavailable.",
            Vec::new(),
        )
        .into());
    };
    let Some(process_key) = list_action
        .process_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Err(misconfigured_snapshot(
            instance_id,
            new_request_id(),
            source,
            RuntimeLivePlayerIssueCode::ProcessUnavailable,
            "The player-list action does not select a server process.",
            Vec::new(),
        )
        .into());
    };
    let Some(process) = select_running_process(&details, process_key) else {
        return Err(misconfigured_snapshot(
            instance_id,
            new_request_id(),
            source,
            RuntimeLivePlayerIssueCode::ProcessUnavailable,
            "The server process used for player collection is not running.",
            Vec::new(),
        )
        .into());
    };
    verify_collection_process(state, instance_id, run.run_id, process, source)?;
    let Some(log_path) = process
        .log_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    else {
        return Err(misconfigured_snapshot(
            instance_id,
            new_request_id(),
            source,
            RuntimeLivePlayerIssueCode::LogUnavailable,
            "The server process has no readable player log.",
            Vec::new(),
        )
        .into());
    };

    let request_id = new_request_id();
    let correlated_request_id = request_id.as_str();
    let run_id = run.run_id;
    let validated_action_ids = player_list.player_action_ids.clone();
    let collected = match player_list.response_codec {
        ModulePlayerListCodec::DstClientTableV1 => {
            collect_dst_structured_log(
                instance_id,
                source,
                &request_id,
                &log_path,
                &validated_action_ids,
                requested_at,
                |submission_tracker, confirmation_deadline| async move {
                    dispatch_declared_runtime_action_until(
                        state,
                        DeclaredRuntimeActionRequest {
                            instance_id,
                            expected_run_id: run_id,
                            action_id: list_action_id,
                            target: None,
                            role: None,
                            request_id: Some(correlated_request_id),
                            require_target_binding: false,
                        },
                        confirmation_deadline,
                        submission_tracker,
                    )
                    .await
                    .map(|_| ())
                },
            )
            .await
        }
        _ => Err(failed_snapshot(
            instance_id,
            request_id.clone(),
            source,
            RuntimeLivePlayerIssueCode::RuntimeActionUnavailable,
            "The module has no collector for this player-list source.",
            false,
        )
        .into()),
    }?;
    verify_collection_process(state, instance_id, run_id, process, source)?;
    Ok(collected)
}

pub(super) fn validate_cached_collection_context(
    state: &DesktopState,
    details: &InstanceDetails,
    descriptor: &ModuleDescriptor,
    expected_key: &LivePlayerCacheKey,
) -> Result<(), Box<RuntimeLivePlayerSnapshot>> {
    let instance_id = &details.summary.id;
    let Some(list) = descriptor.runtime.player_list.as_ref() else {
        return Err(unsupported_snapshot(instance_id, new_request_id()).into());
    };
    let Some(run) = details.active_run.as_ref() else {
        return Err(stopped_snapshot(instance_id, new_request_id(), list.source).into());
    };
    if build_cache_key(details, descriptor).as_ref() != Some(expected_key) {
        return Err(failed_snapshot(
            instance_id,
            new_request_id(),
            list.source,
            RuntimeLivePlayerIssueCode::ProcessUnavailable,
            "The server run or player capability changed during collection.",
            false,
        )
        .into());
    }
    let action = list.action_id.as_ref().and_then(|id| {
        descriptor
            .runtime
            .player_actions
            .iter()
            .find(|action| &action.id == id)
    });
    if let Some(action) = action
        && let Some(keys) = sources::missing_query_settings(details, action)
    {
        return Err(misconfigured_snapshot(
            instance_id,
            new_request_id(),
            list.source,
            RuntimeLivePlayerIssueCode::RuntimeActionUnavailable,
            "Configure the server's remote command interface to read players.",
            keys,
        )
        .into());
    }
    let process_key = action.and_then(|action| action.process_key.as_deref());
    let process = run.processes.iter().find(|process| {
        process_key.map_or(process.is_primary, |key| process.process_key == key)
            && process.pid.is_some()
            && process.status.eq_ignore_ascii_case("running")
    });
    let Some(process) = process else {
        return Err(failed_snapshot(
            instance_id,
            new_request_id(),
            list.source,
            RuntimeLivePlayerIssueCode::ProcessUnavailable,
            "The server has no active player-query process.",
            false,
        )
        .into());
    };
    verify_collection_process(state, instance_id, run.run_id, process, list.source)
}

fn verify_collection_process(
    state: &DesktopState,
    instance_id: &str,
    run_id: i64,
    process: &app_core::InstanceProcessState,
    source: app_core::ModulePlayerListSource,
) -> Result<(), Box<RuntimeLivePlayerSnapshot>> {
    let failure = |code| {
        Box::new(failed_snapshot(
            instance_id,
            new_request_id(),
            source,
            code,
            if code == RuntimeLivePlayerIssueCode::ProcessUntracked {
                "The server is running, but this LanGame session is not supervising it. Stop it normally in LanGame, then start it again to restore player queries."
            } else {
                "The player collection process is unavailable or no longer matches the recorded server run."
            },
            false,
        ))
    };
    let unavailable = RuntimeLivePlayerIssueCode::ProcessUnavailable;
    let pid = process.pid.ok_or_else(|| failure(unavailable))?;
    let mut supervisor = state
        .runtime_supervisor
        .lock()
        .map_err(|_| failure(unavailable))?;
    if supervisor.is_tracked(instance_id) {
        return match supervisor.matches_running_process(
            instance_id,
            run_id,
            &process.process_key,
            pid,
        ) {
            Ok(true) => Ok(()),
            Ok(false) | Err(_) => Err(failure(unavailable)),
        };
    }
    drop(supervisor);

    // A surviving process does not restore child handles or stdin ownership.
    // Inspect its full identity only to explain why collection remains blocked.
    let observed = inspect_process_identity(pid).map_err(|_| failure(unavailable))?;
    Err(failure(untracked_collection_process_issue(
        process.process_identity.as_ref(),
        observed.as_ref(),
    )))
}

fn untracked_collection_process_issue(
    recorded: Option<&ProcessIdentity>,
    observed: Option<&ProcessIdentity>,
) -> RuntimeLivePlayerIssueCode {
    match (recorded, observed) {
        (Some(recorded), Some(observed)) if process_identities_match(recorded, observed) => {
            RuntimeLivePlayerIssueCode::ProcessUntracked
        }
        _ => RuntimeLivePlayerIssueCode::ProcessUnavailable,
    }
}

#[cfg(test)]
mod process_tests {
    use super::*;

    fn identity(creation_time: u64, image_path: &str) -> ProcessIdentity {
        ProcessIdentity {
            creation_time,
            image_path: image_path.to_owned(),
        }
    }

    #[test]
    fn live_player_process_matching_untracked_identity_requires_restart() {
        let recorded = identity(10, "server.exe");
        let code = untracked_collection_process_issue(Some(&recorded), Some(&recorded));
        assert_eq!(code, RuntimeLivePlayerIssueCode::ProcessUntracked);
        assert_eq!(
            serde_json::to_value(code).unwrap(),
            json!("process_untracked")
        );
    }

    #[test]
    fn live_player_process_missing_or_unverifiable_identity_remains_unavailable() {
        let recorded = identity(10, "server.exe");
        for (recorded, observed) in [
            (Some(&recorded), None),
            (None, Some(&recorded)),
            (None, None),
        ] {
            assert_eq!(
                untracked_collection_process_issue(recorded, observed),
                RuntimeLivePlayerIssueCode::ProcessUnavailable,
            );
        }
    }

    #[test]
    fn live_player_process_reused_pid_never_reports_the_original_server_as_running() {
        let recorded = identity(10, "server.exe");
        for observed in [identity(11, "server.exe"), identity(10, "other-server.exe")] {
            assert_eq!(
                untracked_collection_process_issue(Some(&recorded), Some(&observed)),
                RuntimeLivePlayerIssueCode::ProcessUnavailable,
            );
        }
    }
}

#[cfg(test)]
#[path = "commands_live_player_context_tests.rs"]
mod cache_context_tests;
