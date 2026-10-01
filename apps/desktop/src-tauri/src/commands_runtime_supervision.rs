use super::*;

#[path = "commands_runtime_reconciliation.rs"]
mod runtime_reconciliation;
#[path = "commands_runtime_restart.rs"]
mod runtime_restart;
pub(super) use runtime_restart::{
    cancel_pending_runtime_restart, execute_due_runtime_restarts,
    schedule_runtime_restart_candidates, take_due_runtime_restarts,
};

#[tauri::command]
pub fn overlay_families() -> Vec<OverlayFamily> {
    WindowsPlatform::supported_overlay_families()
}

#[tauri::command]
pub async fn bind_address_candidates(
    state: tauri::State<'_, DesktopState>,
) -> Result<Vec<BindAddressCandidate>, String> {
    if let Some(candidates) = state
        .bind_address_cache
        .lock()
        .map_err(|_| String::from("bind address cache lock poisoned"))?
        .fresh(BIND_ADDRESS_CACHE_TTL)
    {
        return Ok(candidates);
    }

    let latest_cached = {
        let mut cache = state
            .bind_address_cache
            .lock()
            .map_err(|_| String::from("bind address cache lock poisoned"))?;
        if let Some(candidates) = cache.fresh(BIND_ADDRESS_CACHE_TTL) {
            return Ok(candidates);
        }
        let latest_cached = cache.latest();
        if !cache.try_begin_refresh() {
            return Ok(latest_cached.unwrap_or_else(default_bind_address_candidates));
        }
        latest_cached
    };

    let result = crate::state::spawn_timed_cache_refresh(
        Arc::clone(&state.bind_address_cache),
        "bind address",
        async {
            tauri::async_runtime::spawn_blocking(WindowsPlatform::bind_address_candidates)
                .await
                .map_err(|error| error.to_string())
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    result.or_else(|error| latest_cached.ok_or(error))
}

pub(super) fn default_bind_address_candidates() -> Vec<BindAddressCandidate> {
    vec![BindAddressCandidate {
        address: String::from("0.0.0.0"),
        kind: String::from("all"),
        adapter_name: None,
        family_name: None,
    }]
}

#[tauri::command]
pub async fn log_frontend_event(
    level: String,
    action: String,
    message: String,
    context: Option<serde_json::Value>,
) -> Result<(), String> {
    static SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);
    let permit = SLOTS
        .try_acquire()
        .map_err(|_| String::from("Diagnostic log writer is busy"))?;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        let storage = bootstrap_storage().map_err(|error| error.to_string())?;
        write_desktop_app_log(
            &storage,
            &level,
            &action,
            &message,
            json!({ "source": "frontend", "context": context.unwrap_or_else(|| json!({})) }),
        )
        .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Diagnostic log task failed: {error}"))?
}

pub(super) fn reconcile_runtime_state<'a>(
    state: &'a tauri::State<'_, DesktopState>,
) -> std::pin::Pin<Box<impl std::future::Future<Output = Result<(), String>> + 'a>> {
    // Reconciliation is shared by nested previews and mutations. Keep its
    // recovery state on the heap instead of enlarging every caller's stack.
    Box::pin(async move {
        let _storage_context_operation =
            state.begin_storage_context_operation("runtime state reconciliation")?;
        let storage = bootstrap_storage().map_err(|error| error.to_string())?;

        let reconciliation = Arc::clone(&state.runtime_reconciliation);
        let admission = reconciliation.acquire().await;
        let supervisor = Arc::clone(&state.runtime_supervisor);
        let tracked = supervisor
            .lock()
            .map_err(|_| String::from("runtime supervisor lock poisoned"))?
            .tracked_instances();
        let mut mutations = HashMap::new();
        if reconciliation.next()?.is_none() {
            for tracked in tracked {
                if let Some(guard) = state
                    .try_acquire_instance_mutation(&tracked.summary.id)
                    .await
                {
                    mutations.insert(tracked.summary.id, Arc::new(guard));
                }
            }
        }
        let eligible = mutations.keys().cloned().collect::<HashSet<_>>();
        let performance_refreshes = reconciliation
            .collect(
                Arc::clone(&admission),
                _storage_context_operation.clone(),
                mutations,
                move || {
                    let mut runtime = supervisor
                        .lock()
                        .map_err(|_| String::from("runtime supervisor lock poisoned"))?;
                    let exited = runtime
                        .reap_exited_for(&eligible)
                        .map_err(|error| error.to_string())?;
                    let refreshed =
                        runtime.refresh_performance_policies(RUNTIME_PERFORMANCE_REFRESH_INTERVAL);
                    Ok::<_, String>((exited, refreshed))
                },
            )
            .await?;

        log_runtime_performance_refreshes(&storage, &performance_refreshes);

        while let Some(pending) = reconciliation.next()? {
            let exited = pending.exited;
            let mutation = pending.mutation;
            let expected_exit = state
                .runtime_restart_scheduler
                .lock()
                .map_err(|_| String::from("runtime restart scheduler lock poisoned"))?
                .exit_is_expected(&exited.summary.id, exited.run_id);
            if !pending.persisted {
                let worker_storage = storage.clone();
                let worker_exit = exited.clone();
                let persisted = reconciliation
                    .persist(
                        Arc::clone(&admission),
                        _storage_context_operation.clone(),
                        Arc::clone(&mutation),
                        exited.clone(),
                        async move {
                            let (_, settings_json) = app_storage::read_instance_stored_settings(
                                &worker_storage.paths,
                                &worker_exit.summary.id,
                            )
                            .await
                            .map_err(|error| error.to_string())?;
                            let policy = runtime_restart_policy_from_settings(&settings_json);
                            let crash_flag = runtime_restart::exit_marks_failed_session(
                                expected_exit,
                                worker_exit.exit_code,
                                &policy,
                            );
                            runtime_reconciliation::persist_current_exit(
                                &worker_storage,
                                &worker_exit,
                                crash_flag,
                            )
                            .await
                        },
                    )
                    .await?;
                if !persisted {
                    continue;
                }
            }
            // A cancelled consumer may resume after a manual start. Never schedule
            // an earlier session's recovery against its replacement.
            let active = read_active_instance_run(&storage.paths, &exited.summary.id)
                .await
                .map_err(|error| error.to_string())?;
            if active.as_ref().is_some_and(|run| {
                run.session_id != exited.session_id
                    || (run.session_id.is_none()
                        && !run
                            .processes
                            .iter()
                            .any(|process| process.run_id == exited.run_id))
            }) {
                reconciliation.acknowledge(&exited)?;
                continue;
            }
            state
                .live_player_registry
                .invalidate_instance(&exited.summary.id);
            super::commands_runtime_lifecycle::finish_runtime_log_stream_for_process(
                state,
                &exited.summary.id,
                &exited.log_path,
            );

            let candidate = runtime_restart::build_runtime_restart_candidate(
                &storage,
                &exited.summary.id,
                &exited.summary.name,
                expected_exit,
                exited.exit_code,
            )
            .await?;
            // No cancellation point separates scheduling and acknowledgement.
            schedule_runtime_restart_candidates(state, &storage, candidate.into_iter().collect())?;
            reconciliation.acknowledge(&exited)?;
        }

        let tracked_instance_ids = {
            let runtime = state
                .runtime_supervisor
                .lock()
                .map_err(|_| String::from("runtime supervisor lock poisoned"))?;
            runtime
                .tracked_instances()
                .into_iter()
                .map(|instance| instance.summary.id)
                .collect::<HashSet<_>>()
        };

        for active_run in list_active_instance_runs(&storage.paths)
            .await
            .map_err(|error| error.to_string())?
        {
            if tracked_instance_ids.contains(&active_run.instance_id) {
                continue;
            }
            let Some(_mutation) = state
                .try_acquire_instance_mutation(&active_run.instance_id)
                .await
            else {
                // Stop/start owners finish their own persistence; stale recovery
                // must not invent an unknown exit while they hold the same run.
                continue;
            };

            let (identity_matches, stale_reason) =
                match (active_run.pid, active_run.process_identity.as_ref()) {
                    (Some(pid), Some(expected)) => match inspect_process_identity(pid) {
                        Ok(Some(actual))
                            if restored_process_identity_matches(expected, &actual) =>
                        {
                            (true, "identity_matches")
                        }
                        Ok(Some(_)) => (false, "process_identity_mismatch"),
                        Ok(None) => (false, "process_missing"),
                        Err(_) => (false, "process_identity_unavailable"),
                    },
                    (Some(_), None) => (false, "recorded_identity_missing"),
                    (None, _) => (false, "recorded_pid_missing"),
                };
            if identity_matches {
                continue;
            }

            state
                .live_player_registry
                .invalidate_instance(&active_run.instance_id);
            mark_instance_process_stopped(
                &storage.paths,
                &active_run.instance_id,
                active_run.run_id,
                None,
                false,
            )
            .await
            .map_err(|error| error.to_string())?;
            if let Some(log_path) = active_run.log_path.as_deref() {
                super::commands_runtime_lifecycle::stop_runtime_log_stream_for_process(
                    state,
                    &active_run.instance_id,
                    log_path,
                );
            }
            append_desktop_app_log(
                &storage,
                "warn",
                "instance.runtime.reconciled_stale_process",
                "A process from a previous desktop session could not be proven to have the recorded identity, so it was reconciled as stopped without sending a signal.",
                json!({
                    "instance_id": active_run.instance_id.as_str(),
                    "run_id": active_run.run_id,
                    "pid": active_run.pid,
                    "reason": stale_reason,
                }),
            );
        }

        // Restart execution can enter other runtime workflows. Release reconciliation
        // admission before taking those lifecycle locks again.
        drop(admission);
        let due_restarts = take_due_runtime_restarts(state);
        execute_due_runtime_restarts(state, &storage, due_restarts).await;

        let instances = list_instances(&storage.paths)
            .await
            .map_err(|error| error.to_string())?;
        update_state_instances(state, instances)
    })
}

fn restored_process_identity_matches(
    recorded: &ProcessIdentity,
    observed: &ProcessIdentity,
) -> bool {
    process_identities_match(recorded, observed)
}

#[cfg(test)]
mod process_identity_tests {
    use super::*;

    #[test]
    fn dst_shards_use_their_own_native_name_ugc_directory_and_game_port() {
        for shard in &app_core::dst_shards::DST_SHARDS {
            assert_eq!(rewrite_dst_shard_arg("Master", shard), shard.directory);
            assert_eq!(
                rewrite_dst_shard_arg("{{paths.data_dir}}/ugc/Master", shard),
                format!("{{{{paths.data_dir}}}}/ugc/{}", shard.directory)
            );
            assert_eq!(
                rewrite_dst_shard_arg("{{ports.master.port}}", shard),
                format!("{{{{ports.{}.port}}}}", shard.game_port)
            );
            assert_eq!(rewrite_dst_shard_arg("-console", shard), "-console");
        }
    }

    #[test]
    fn reused_pid_with_different_identity_is_classified_as_stale() {
        let recorded = ProcessIdentity {
            creation_time: 10,
            image_path: String::from(r"c:\servers\game.exe"),
        };
        let observed_for_same_pid = ProcessIdentity {
            creation_time: 11,
            image_path: recorded.image_path.clone(),
        };

        assert!(!restored_process_identity_matches(
            &recorded,
            &observed_for_same_pid
        ));
    }
}

pub(super) fn log_runtime_performance_refreshes(
    storage: &StorageBootstrap,
    refreshes: &[app_runtime::RuntimePerformanceRefresh],
) {
    for refresh in refreshes {
        if !refresh.target_count_changed && refresh.application.warnings.is_empty() {
            continue;
        }

        let action = if refresh.application.warnings.is_empty() {
            "instance.runtime_performance.refreshed"
        } else {
            "instance.runtime_performance.refresh_warning"
        };
        let level = if refresh.application.warnings.is_empty() {
            "info"
        } else {
            "warn"
        };

        append_desktop_app_log(
            storage,
            level,
            action,
            "Runtime performance policy refreshed for a tracked process tree",
            json!({
                "instance_id": refresh.instance_id.as_str(),
                "instance_name": refresh.instance_name.as_str(),
                "process_key": refresh.process_key.as_str(),
                "display_name": refresh.display_name.as_str(),
                "pid": refresh.application.pid,
                "priority_class": &refresh.application.priority_class,
                "cpu_affinity_mask": refresh.application.cpu_affinity_mask,
                "apply_to_child_processes": refresh.application.apply_to_child_processes,
                "targeted_process_count": refresh.application.targeted_process_count,
                "priority_applied_count": refresh.application.priority_applied_count,
                "affinity_applied_count": refresh.application.affinity_applied_count,
                "target_count_changed": refresh.target_count_changed,
                "warnings": &refresh.application.warnings,
            }),
        );
    }
}

pub(super) fn module_window_policy(module: &ModuleDetails) -> ProcessWindowPolicy {
    module
        .process
        .as_ref()
        .map(|process| process.window_policy.clone())
        .unwrap_or_default()
}

pub(super) fn build_window_inspection_targets_from_instance(
    instance: &InstanceDetails,
) -> Vec<WindowInspectionTarget> {
    instance
        .active_run
        .as_ref()
        .map(|active_run| {
            active_run
                .processes
                .iter()
                .filter_map(|process| {
                    Some(WindowInspectionTarget {
                        pid: process.pid?,
                        process_key: process.process_key.clone(),
                        display_name: process.display_name.clone(),
                        process_identity: process.process_identity.clone()?,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

pub(super) fn build_window_inspection_targets_from_registered_processes(
    processes: &[app_core::InstanceProcessState],
) -> Vec<WindowInspectionTarget> {
    processes
        .iter()
        .filter_map(|process| {
            Some(WindowInspectionTarget {
                pid: process.pid?,
                process_key: process.process_key.clone(),
                display_name: process.display_name.clone(),
                process_identity: process.process_identity.clone()?,
            })
        })
        .collect()
}

pub(super) fn build_window_inspection_targets_from_spawned_processes(
    processes: &[(ProcessLaunchPlan, app_runtime::SpawnedProcess)],
) -> Vec<WindowInspectionTarget> {
    processes
        .iter()
        .map(|(plan, process)| WindowInspectionTarget {
            pid: process.pid,
            process_key: plan.process_key.clone(),
            display_name: plan.display_name.clone(),
            process_identity: process.process_identity.clone(),
        })
        .collect()
}

pub(super) fn build_runtime_window_suppression_result(
    instance_id: &str,
    source: &str,
    inspected_process_count: usize,
    visible_window_count_before: usize,
    suppressed_window_count: usize,
    remaining_visible_window_count: usize,
    summary: impl Into<String>,
) -> RuntimeWindowSuppressionResult {
    RuntimeWindowSuppressionResult {
        instance_id: instance_id.to_string(),
        source: source.to_string(),
        attempted_at_unix_ms: now_unix_ms(),
        inspected_process_count,
        visible_window_count_before,
        suppressed_window_count,
        remaining_visible_window_count,
        summary: summary.into(),
    }
}

pub(super) fn runtime_window_suppression_attempt_status(
    result: &RuntimeWindowSuppressionResult,
) -> &'static str {
    if result.visible_window_count_before == 0 {
        "idle"
    } else if result.remaining_visible_window_count == 0 {
        "clear"
    } else {
        "partial"
    }
}

pub(super) fn runtime_window_suppression_attempt_from_result(
    result: &RuntimeWindowSuppressionResult,
) -> RuntimeWindowSuppressionAttempt {
    RuntimeWindowSuppressionAttempt {
        instance_id: result.instance_id.clone(),
        source: result.source.clone(),
        status: runtime_window_suppression_attempt_status(result).to_string(),
        attempted_at_unix_ms: result.attempted_at_unix_ms,
        inspected_process_count: result.inspected_process_count,
        visible_window_count_before: result.visible_window_count_before,
        suppressed_window_count: result.suppressed_window_count,
        remaining_visible_window_count: result.remaining_visible_window_count,
        summary: result.summary.clone(),
    }
}

pub(super) fn build_runtime_window_suppression_failed_attempt(
    instance_id: &str,
    source: &str,
    attempted_at_unix_ms: u128,
    inspected_process_count: usize,
    summary: impl Into<String>,
) -> RuntimeWindowSuppressionAttempt {
    RuntimeWindowSuppressionAttempt {
        instance_id: instance_id.to_string(),
        source: source.to_string(),
        status: String::from("failed"),
        attempted_at_unix_ms,
        inspected_process_count,
        visible_window_count_before: 0,
        suppressed_window_count: 0,
        remaining_visible_window_count: 0,
        summary: summary.into(),
    }
}

pub(super) fn summarize_window_suppression_result(
    inspected_process_count: usize,
    visible_window_count_before: usize,
    suppressed_window_count: usize,
    remaining_visible_window_count: usize,
) -> String {
    if inspected_process_count == 0 {
        return String::from(
            "LanGame does not have any runtime PIDs to inspect, so no native windows were suppressed.",
        );
    }

    if visible_window_count_before == 0 {
        return format!(
            "LanGame did not find any visible Windows surfaces across {} tracked runtime process(es).",
            inspected_process_count
        );
    }

    if suppressed_window_count > 0 && remaining_visible_window_count == 0 {
        return format!(
            "Suppressed {} visible Windows surface(s) across {} tracked runtime process(es).",
            suppressed_window_count, inspected_process_count
        );
    }

    if suppressed_window_count > 0 {
        return format!(
            "Suppressed {} visible Windows surface(s), but {} still remain visible across {} tracked runtime process(es).",
            suppressed_window_count, remaining_visible_window_count, inspected_process_count
        );
    }

    format!(
        "Found {} visible Windows surface(s), but none could be suppressed across {} tracked runtime process(es).",
        visible_window_count_before, inspected_process_count
    )
}

pub(super) fn suppress_runtime_windows_for_targets(
    instance_id: &str,
    targets: &[WindowInspectionTarget],
    source: &str,
) -> Result<RuntimeWindowSuppressionResult, String> {
    if targets.is_empty() {
        return Ok(build_runtime_window_suppression_result(
            instance_id,
            source,
            0,
            0,
            0,
            0,
            "LanGame does not have any runtime PIDs to inspect, so no native windows were suppressed.",
        ));
    }

    let result = WindowsPlatform::suppress_visible_window_surfaces(targets)?;
    let remaining_visible_window_count = result.remaining_windows.len();
    Ok(build_runtime_window_suppression_result(
        instance_id,
        source,
        result.inspected_process_count,
        result.visible_window_count_before,
        result.suppressed_window_count,
        remaining_visible_window_count,
        summarize_window_suppression_result(
            result.inspected_process_count,
            result.visible_window_count_before,
            result.suppressed_window_count,
            remaining_visible_window_count,
        ),
    ))
}

pub(super) fn try_auto_suppress_background_windows(
    storage: &StorageBootstrap,
    instance: &InstanceDetails,
    module: &ModuleDetails,
    registered_processes: &[app_core::InstanceProcessState],
) {
    if !matches!(
        module_window_policy(module),
        ProcessWindowPolicy::Background
    ) {
        return;
    }

    let targets = build_window_inspection_targets_from_registered_processes(registered_processes);
    match suppress_runtime_windows_for_targets(&instance.summary.id, &targets, "automatic") {
        Ok(result) => append_desktop_app_log(
            storage,
            "info",
            RUNTIME_WINDOW_AUTO_SUPPRESSION_ACTION,
            &result.summary,
            json!({
                "instance_id": instance.summary.id,
                "instance_name": instance.summary.name,
                "visible_window_count_before": result.visible_window_count_before,
                "suppressed_window_count": result.suppressed_window_count,
                "remaining_visible_window_count": result.remaining_visible_window_count,
                "inspected_process_count": result.inspected_process_count,
            }),
        ),
        Err(error) => append_desktop_app_log(
            storage,
            "error",
            RUNTIME_WINDOW_AUTO_SUPPRESSION_FAILED_ACTION,
            &error,
            json!({
                "instance_id": instance.summary.id,
                "instance_name": instance.summary.name,
                "process_count": registered_processes.len(),
            }),
        ),
    }
}

pub(super) fn try_startup_window_guard_background_windows(
    storage: &StorageBootstrap,
    instance: &InstanceDetails,
    module: &ModuleDetails,
    targets: Vec<WindowInspectionTarget>,
) {
    if !matches!(
        module_window_policy(module),
        ProcessWindowPolicy::Background
    ) || targets.is_empty()
    {
        return;
    }

    let storage = storage.clone();
    let instance_id = instance.summary.id.clone();
    let instance_name = instance.summary.name.clone();
    std::thread::spawn(move || {
        run_startup_window_guard(storage, instance_id, instance_name, targets);
    });
}

pub(super) fn run_startup_window_guard(
    storage: StorageBootstrap,
    instance_id: String,
    instance_name: String,
    targets: Vec<WindowInspectionTarget>,
) {
    let mut attempts = 0usize;
    let mut max_visible_window_count = 0usize;
    let mut total_suppressed_window_count = 0usize;
    let mut last_result = None;

    for _ in 0..STARTUP_WINDOW_GUARD_ATTEMPTS {
        attempts += 1;
        match suppress_runtime_windows_for_targets(&instance_id, &targets, "startup_guard") {
            Ok(result) => {
                max_visible_window_count =
                    max_visible_window_count.max(result.visible_window_count_before);
                total_suppressed_window_count += result.suppressed_window_count;
                last_result = Some(result);
            }
            Err(error) => {
                append_desktop_app_log(
                    &storage,
                    "error",
                    RUNTIME_WINDOW_AUTO_SUPPRESSION_FAILED_ACTION,
                    &error,
                    json!({
                        "instance_id": instance_id,
                        "instance_name": instance_name,
                        "process_count": targets.len(),
                        "inspected_process_count": targets.len(),
                        "phase": "startup_guard",
                        "attempts": attempts,
                    }),
                );
                return;
            }
        }

        std::thread::sleep(Duration::from_millis(STARTUP_WINDOW_GUARD_INTERVAL_MS));
    }

    let Some(result) = last_result else {
        return;
    };
    if max_visible_window_count == 0
        && total_suppressed_window_count == 0
        && result.remaining_visible_window_count == 0
    {
        return;
    }

    let message = format!(
        "Startup window guard inspected {} tracked runtime process(es) over {} attempt(s), suppressed {} visible Windows surface(s), and left {} visible.",
        result.inspected_process_count,
        attempts,
        total_suppressed_window_count,
        result.remaining_visible_window_count
    );
    append_desktop_app_log(
        &storage,
        "info",
        RUNTIME_WINDOW_AUTO_SUPPRESSION_ACTION,
        &message,
        json!({
            "instance_id": instance_id,
            "instance_name": instance_name,
            "visible_window_count_before": max_visible_window_count,
            "suppressed_window_count": total_suppressed_window_count,
            "remaining_visible_window_count": result.remaining_visible_window_count,
            "inspected_process_count": result.inspected_process_count,
            "phase": "startup_guard",
            "attempts": attempts,
        }),
    );
}

pub(super) fn build_runtime_window_snapshot(
    storage: &StorageBootstrap,
    instance: &InstanceDetails,
) -> RuntimeWindowSnapshot {
    let observed_at_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let last_suppression_attempt =
        read_latest_runtime_window_suppression_attempt(storage, &instance.summary.id);

    let Some(_active_run) = instance.active_run.as_ref() else {
        return RuntimeWindowSnapshot {
            instance_id: instance.summary.id.clone(),
            observed_at_unix_ms,
            status: String::from("stopped"),
            summary: String::from(
                "The instance is not running, so LanGame did not inspect any visible process windows.",
            ),
            inspected_process_count: 0,
            last_suppression_attempt,
            windows: Vec::new(),
        };
    };

    let targets = build_window_inspection_targets_from_instance(instance);

    if targets.is_empty() {
        return RuntimeWindowSnapshot {
            instance_id: instance.summary.id.clone(),
            observed_at_unix_ms,
            status: String::from("unavailable"),
            summary: String::from(
                "LanGame does not have any active runtime PIDs for this instance yet, so visible window inspection is unavailable.",
            ),
            inspected_process_count: 0,
            last_suppression_attempt,
            windows: Vec::new(),
        };
    }

    match WindowsPlatform::inspect_visible_window_surfaces(&targets) {
        Ok(result) if result.windows.is_empty() => RuntimeWindowSnapshot {
            instance_id: instance.summary.id.clone(),
            observed_at_unix_ms,
            status: String::from("clear"),
            summary: format!(
                "No visible top-level Windows surfaces were detected across {} tracked runtime process(es).",
                result.inspected_process_count
            ),
            inspected_process_count: result.inspected_process_count,
            last_suppression_attempt,
            windows: Vec::new(),
        },
        Ok(result) => RuntimeWindowSnapshot {
            instance_id: instance.summary.id.clone(),
            observed_at_unix_ms,
            status: String::from("detected"),
            summary: format!(
                "Detected {} visible top-level Windows surface(s) across {} tracked runtime process(es).",
                result.windows.len(),
                result.inspected_process_count
            ),
            inspected_process_count: result.inspected_process_count,
            last_suppression_attempt,
            windows: result.windows,
        },
        Err(error) => RuntimeWindowSnapshot {
            instance_id: instance.summary.id.clone(),
            observed_at_unix_ms,
            status: String::from("unavailable"),
            summary: format!("LanGame could not inspect visible Windows surfaces: {error}"),
            inspected_process_count: 0,
            last_suppression_attempt,
            windows: Vec::new(),
        },
    }
}

pub(super) fn update_state_instances(
    state: &tauri::State<'_, DesktopState>,
    instances: Vec<InstanceSummary>,
) -> Result<(), String> {
    update_desktop_state_instances(state, instances)
}

pub(super) fn update_desktop_state_instances(
    state: &DesktopState,
    instances: Vec<InstanceSummary>,
) -> Result<(), String> {
    let running_instances = instances
        .iter()
        .filter(|instance| matches!(instance.status, InstanceStatus::Running))
        .count();

    let mut state_guard = state
        .app_state
        .write()
        .map_err(|_| String::from("desktop state lock poisoned"))?;
    state_guard.instances = instances;
    state_guard.snapshot.running_instances = running_instances;
    Ok(())
}

pub(super) fn preferred_snapshot_path(settings: &app_core::AppSettings) -> &str {
    if !settings.games_root.trim().is_empty() {
        &settings.games_root
    } else if !settings.servers_root.trim().is_empty() {
        &settings.servers_root
    } else if !settings.steamcmd_root.trim().is_empty() {
        &settings.steamcmd_root
    } else {
        "."
    }
}

pub(super) fn mutate_app_state<F>(
    state: &tauri::State<'_, DesktopState>,
    mutate: F,
) -> Result<(), String>
where
    F: FnOnce(&mut AppState),
{
    let mut state_guard = state
        .app_state
        .write()
        .map_err(|_| String::from("desktop state lock poisoned"))?;
    mutate(&mut state_guard);
    Ok(())
}

pub(super) fn set_module_install_state(
    state: &tauri::State<'_, DesktopState>,
    module_id: &str,
    install_state: InstallState,
) -> Result<(), String> {
    mutate_app_state(state, |app_state| {
        if let Some(module) = app_state
            .modules
            .iter_mut()
            .find(|module| module.id == module_id)
        {
            module.install_state = install_state;
        }
    })
}

pub(super) fn insert_background_job(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    job: BackgroundJob,
) -> Result<(), String> {
    let _storage_context_operation = state.begin_storage_context_operation("background job")?;
    ensure_storage_context_snapshot_current(state, storage, "background job")?;
    mutate_app_state(state, |app_state| {
        app_state.jobs.retain(|existing| existing.id != job.id);
        app_state.jobs.insert(0, job);
        let mut retained_results = 0;
        app_state.jobs.retain(|job| {
            if matches!(job.status, JobStatus::Pending | JobStatus::Running) {
                return true;
            }
            retained_results += 1;
            retained_results <= 24
        });
    })
}

pub(super) fn update_background_job<F>(
    state: &tauri::State<'_, DesktopState>,
    job_id: &str,
    update: F,
) -> Result<(), String>
where
    F: FnOnce(&mut BackgroundJob),
{
    mutate_app_state(state, |app_state| {
        if let Some(job) = app_state.jobs.iter_mut().find(|job| job.id == job_id) {
            update(job);
        }
    })
}

pub(super) fn find_instance_summary(
    instances: &[InstanceSummary],
    instance_id: &str,
) -> Option<InstanceSummary> {
    instances
        .iter()
        .find(|instance| instance.id == instance_id)
        .cloned()
}

pub(super) fn reserve_runtime_startup_slot(
    state: &tauri::State<'_, DesktopState>,
    launch_plans: &[ProcessLaunchPlan],
    running_instance_count: usize,
    active_process_count: usize,
) -> Result<RuntimeStartupSchedule, String> {
    let policy = launch_plans
        .first()
        .map(|plan| &plan.launch_plan.performance_policy)
        .cloned()
        .unwrap_or_default();
    let mut scheduler = state
        .startup_scheduler
        .lock()
        .map_err(|_| String::from("runtime startup scheduler lock poisoned"))?;
    let slot = scheduler.reserve_start(RuntimeStartupReservation {
        base_stagger_ms: policy.startup_stagger_ms,
        running_instance_count,
        active_process_count,
        process_count: launch_plans.len(),
    });
    let delay_ms = u64::try_from(slot.delay.as_millis()).unwrap_or(u64::MAX);

    let schedule = RuntimeStartupSchedule {
        delay_ms,
        instance_stagger_ms: policy.startup_stagger_ms,
        effective_stagger_ms: slot.effective_stagger_ms,
        child_process_stagger_ms: policy.child_process_stagger_ms,
        running_instance_count,
        active_process_count,
        process_count: launch_plans.len(),
        queued_start_count: slot.queued_start_count,
        reason: if delay_ms > 0 {
            format!(
                "Another instance start was recently reserved; startup is staggered to reduce simultaneous CPU, disk, and port pressure. Effective stagger is {}ms with {} tracked instance(s), {} active process(es), and {} new process launch plan(s).",
                slot.effective_stagger_ms,
                running_instance_count,
                active_process_count,
                launch_plans.len()
            )
        } else {
            format!(
                "Startup slot was available immediately. Effective stagger is {}ms with {} tracked instance(s), {} active process(es), and {} new process launch plan(s).",
                slot.effective_stagger_ms,
                running_instance_count,
                active_process_count,
                launch_plans.len()
            )
        },
    };
    scheduler.record_startup_schedule(schedule.clone());

    Ok(schedule)
}

pub(super) fn skipped_runtime_performance_application(pid: u32) -> RuntimePerformanceApplication {
    let default_policy = RuntimePerformancePolicy::default();
    RuntimePerformanceApplication {
        pid,
        priority_class: default_policy.priority_class,
        cpu_affinity_mask: default_policy.cpu_affinity_mask,
        apply_to_child_processes: default_policy.apply_to_child_processes,
        targeted_process_count: 0,
        priority_applied_count: 0,
        affinity_applied_count: 0,
        warnings: vec![String::from(
            "Runtime performance policy was not applied because the process record was incomplete.",
        )],
    }
}

pub(super) fn derive_run_log_path(
    instance: &InstanceDetails,
    process_key: &str,
    stamp_override: Option<u128>,
) -> PathBuf {
    let config_file_path = PathBuf::from(&instance.config_file_path);
    let instance_root = config_file_path
        .parent()
        .and_then(|path| path.parent())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_LANGAME_INSTANCES_ROOT));
    let logs_root = instance_root.join("logs");
    let logs_root = if cfg!(windows) {
        logs_root.join("managed-console")
    } else {
        logs_root
    };
    let stamp = stamp_override.unwrap_or_else(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0)
    });
    let suffix = if process_key.is_empty() {
        String::from("main")
    } else {
        process_key.to_ascii_lowercase().replace(' ', "-")
    };
    logs_root.join(format!("run-{stamp}-{suffix}.log"))
}

pub(super) fn assign_process_log_paths_with_stamp(
    instance: &InstanceDetails,
    launch_plans: &mut [ProcessLaunchPlan],
    stamp: u128,
) {
    for plan in launch_plans {
        let mut path = derive_run_log_path(instance, &plan.process_key, Some(stamp));
        // ShellExecute's elevated cmd redirection retains its own file handle;
        // those output files cannot participate in the managed output pump.
        if cfg!(windows)
            && plan.launch_plan.requires_admin
            && let (Some(logs_root), Some(name)) =
                (path.parent().and_then(Path::parent), path.file_name())
        {
            path = logs_root.join(name);
        }
        plan.log_path = path.to_string_lossy().into_owned();
    }
}

pub(super) fn build_process_launch_plans_for_instance(
    settings: &app_core::AppSettings,
    module: &ModuleDetails,
    instance: &InstanceDetails,
) -> Result<Vec<ProcessLaunchPlan>, String> {
    let private_root = super::commands_runtime_lifecycle::private_runtime_install_root(instance)?;
    let install_root_override = Some(private_root.as_str());
    let mut launch_plans = if app_core::ark_maps::is_ark(&module.summary.id) {
        super::commands_runtime_ark::build_launch_plans(
            settings,
            module,
            instance,
            install_root_override,
        )?
    } else if module.summary.id == "dontstarve" {
        let configuration: serde_json::Value =
            serde_json::from_str(&instance.settings_json).map_err(|error| error.to_string())?;
        app_storage::validate_dst_shard_mod_requirements(&configuration)?;
        app_core::dst_shards::dst_shards(&configuration)?
            .into_iter()
            .map(|shard| {
                Ok(ProcessLaunchPlan {
                    process_key: String::from(shard.process_key),
                    display_name: String::from(shard.directory),
                    log_path: String::new(),
                    launch_plan: build_dst_shard_launch_plan(
                        settings,
                        module,
                        instance,
                        install_root_override,
                        shard,
                    )?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?
    } else {
        vec![ProcessLaunchPlan {
            process_key: String::from("main"),
            display_name: String::from("Server"),
            log_path: String::new(),
            launch_plan: build_launch_plan_with_override(
                settings,
                module,
                instance,
                install_root_override,
            )
            .map_err(|error| error.to_string())?,
        }]
    };

    for plan in &mut launch_plans {
        plan.launch_plan.uses_private_runtime =
            super::commands_program_storage::program_mode(instance)?
                == app_storage::InstanceProgramMode::Independent;
    }
    Ok(launch_plans)
}

pub(super) fn build_dst_shard_launch_plan(
    settings: &app_core::AppSettings,
    module: &ModuleDetails,
    instance: &InstanceDetails,
    install_root_override: Option<&str>,
    shard: &app_core::dst_shards::DstShardSpec,
) -> Result<LaunchPlan, String> {
    let mut shard_module = module.clone();
    let process = shard_module
        .process
        .as_mut()
        .ok_or_else(|| String::from("dontstarve module does not declare a [process] section"))?;
    process.args_template = process
        .args_template
        .iter()
        .map(|segment| rewrite_dst_shard_arg(segment, shard))
        .collect();

    build_launch_plan_with_override(settings, &shard_module, instance, install_root_override)
        .map_err(|error| error.to_string())
}

pub(super) fn ensure_module_installed_for_start(module: &ModuleDetails) -> Result<(), String> {
    if matches!(module.summary.install_state, InstallState::Installed) {
        return Ok(());
    }

    Err(json!({
        "code": "module_not_ready",
        "module_id": module.summary.id,
        "module_name": module.summary.name,
        "install_state": module.summary.install_state,
        "message": format!(
            "module `{}` is not ready to start; current install state is {:?}. Finish, repair, or rescan the game install before launching an instance.",
            module.summary.id, module.summary.install_state
        ),
    })
    .to_string())
}

pub(super) async fn ensure_firewall_rules_for_start(
    storage: &StorageBootstrap,
    instance: &InstanceDetails,
    source: &str,
) -> Result<(), String> {
    #[cfg(all(test, windows))]
    if super::tests::assistant_lifecycle_workflow_tests::fixture_firewall_applies(
        storage, instance,
    )? {
        return Ok(());
    }
    if instance.ports.is_empty() {
        return Ok(());
    }

    let firewall_instance_id = instance.summary.id.clone();
    let firewall_instance_name = instance.summary.name.clone();
    let firewall_ports = instance.ports.clone();
    let normalized_bind_address = normalize_strict_bind_address(&instance.summary.bind_ip)?;
    let firewall_local_address =
        (!is_wildcard_bind_address(&normalized_bind_address)).then_some(normalized_bind_address);
    let apply_local_address = firewall_local_address.clone();

    match run_firewall_rule_apply_on_blocking_thread(
        firewall_instance_id,
        firewall_instance_name,
        firewall_ports,
        move |instance_id, instance_name, ports| {
            WindowsPlatform::ensure_instance_firewall_rules(
                &instance_id,
                &instance_name,
                &ports,
                apply_local_address.as_deref(),
            )
        },
    )
    .await
    {
        Ok(results) => {
            append_desktop_app_log(
                storage,
                "info",
                "instance.firewall_rules.ready",
                "Windows Firewall inbound allow rules are ready for instance ports",
                json!({
                    "source": source,
                    "instance_id": instance.summary.id.as_str(),
                    "instance_name": instance.summary.name.as_str(),
                    "local_address": firewall_local_address.as_deref().unwrap_or("Any"),
                    "rules": results,
                }),
            );
            Ok(())
        }
        Err(error) => {
            let decision = firewall_rule_failure_start_decision(
                &instance.summary.name,
                &error,
                firewall_local_address.as_deref(),
            );
            append_desktop_app_log(
                storage,
                decision.log_level,
                "instance.firewall_rules.failed",
                &decision.message,
                json!({
                    "source": source,
                    "instance_id": instance.summary.id.as_str(),
                    "instance_name": instance.summary.name.as_str(),
                    "ports": &instance.ports,
                    "local_address": firewall_local_address.as_deref().unwrap_or("Any"),
                    "continue_start": decision.continue_start,
                }),
            );
            if decision.continue_start {
                return Ok(());
            }
            Err(logged_error_message(storage, decision.message))
        }
    }
}

pub(super) async fn run_firewall_rule_apply_on_blocking_thread<F>(
    instance_id: String,
    instance_name: String,
    ports: Vec<app_core::PortBinding>,
    apply: F,
) -> Result<Vec<app_platform_win::WindowsFirewallRuleApplyResult>, String>
where
    F: FnOnce(
            String,
            String,
            Vec<app_core::PortBinding>,
        ) -> Result<Vec<app_platform_win::WindowsFirewallRuleApplyResult>, String>
        + Send
        + 'static,
{
    tokio::task::spawn_blocking(move || apply(instance_id, instance_name, ports))
        .await
        .map_err(|error| format!("firewall rule worker failed: {error}"))?
}

pub(super) struct FirewallRuleFailureStartDecision {
    pub(super) continue_start: bool,
    pub(super) log_level: &'static str,
    pub(super) message: String,
}

pub(super) fn firewall_rule_failure_start_decision(
    instance_name: &str,
    error: &str,
    local_address: Option<&str>,
) -> FirewallRuleFailureStartDecision {
    if let Some(local_address) = local_address {
        return FirewallRuleFailureStartDecision {
            continue_start: false,
            log_level: "error",
            message: format!(
                "failed to restrict Windows Firewall rules for `{instance_name}` to local address {local_address}: {error}; startup was blocked before the game process could listen on a broader interface"
            ),
        };
    }

    FirewallRuleFailureStartDecision {
        continue_start: true,
        log_level: "warn",
        message: format!(
            "failed to configure Windows Firewall allow rules for `{instance_name}`: {error}; continuing startup. Run LanGame Server Manager as administrator or create inbound allow rules for the instance ports manually."
        ),
    }
}

pub(super) fn append_prestart_update_console_line(
    log_path: impl AsRef<Path>,
    detail: &str,
    output_excerpt: &str,
) -> Result<(), String> {
    append_instance_console_line(log_path, "startup update", detail, output_excerpt)
}

pub(super) fn append_startup_console_line(
    log_path: impl AsRef<Path>,
    detail: &str,
    output_excerpt: &str,
) -> Result<(), String> {
    append_instance_console_line(log_path, "startup", detail, output_excerpt)
}

pub(super) fn append_instance_console_line(
    log_path: impl AsRef<Path>,
    channel: &str,
    detail: &str,
    output_excerpt: &str,
) -> Result<(), String> {
    let log_path = log_path.as_ref();
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "failed to create instance console log directory {}: {}",
                parent.display(),
                error
            )
        })?;
    }

    // Instance names and diagnostics can contain line breaks. Every physical
    // manager line must retain its origin, including bare CR console updates,
    // so embedded text cannot become an apparent native readiness signal.
    let lines = [detail, output_excerpt]
        .into_iter()
        .flat_map(|text| text.split(['\r', '\n']))
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return Ok(());
    }

    let mut file =
        app_storage::managed_console_log::ManagedConsoleLog::open(log_path).map_err(|error| {
            format!(
                "failed to open instance console log {}: {}",
                log_path.display(),
                error
            )
        })?;
    for line in lines {
        writeln!(file, "[LanGame {channel}] {line}").map_err(|error| {
            format!(
                "failed to write instance console log {}: {}",
                log_path.display(),
                error
            )
        })?;
    }

    Ok(())
}

pub(super) fn ensure_launch_plans_ready(launch_plans: &[ProcessLaunchPlan]) -> Result<(), String> {
    #[derive(serde::Serialize)]
    struct BlockingIssue<'a> {
        process_key: &'a str,
        display_name: &'a str,
        #[serde(flatten)]
        issue: &'a app_core::LaunchValidationIssue,
    }

    let blocking = launch_plans
        .iter()
        .flat_map(|plan| {
            plan.launch_plan
                .validation_issues
                .iter()
                .filter(|issue| issue.severity.eq_ignore_ascii_case("error"))
                .map(|issue| BlockingIssue {
                    process_key: &plan.process_key,
                    display_name: &plan.display_name,
                    issue,
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    if blocking.is_empty() {
        return Ok(());
    }

    let message = format!(
        "Launch preflight failed:\n- {}",
        blocking
            .iter()
            .map(|item| format!("{}: {}", item.display_name, item.issue.message))
            .collect::<Vec<_>>()
            .join("\n- ")
    );
    Err(serde_json::json!({
        "code": "launch_preflight_failed",
        "issues": blocking,
        "message": message,
    })
    .to_string())
}

fn rewrite_dst_shard_arg(segment: &str, shard: &app_core::dst_shards::DstShardSpec) -> String {
    match segment {
        "Master" => String::from(shard.directory),
        "{{paths.data_dir}}/ugc/Master" => {
            format!("{{{{paths.data_dir}}}}/ugc/{}", shard.directory)
        }
        "{{ports.master.port}}" => format!("{{{{ports.{}.port}}}}", shard.game_port),
        other => String::from(other),
    }
}

pub(super) fn dst_caves_enabled(settings_json: &str) -> Result<bool, String> {
    let settings: serde_json::Value =
        serde_json::from_str(settings_json).map_err(|error| error.to_string())?;
    Ok(app_core::dst_shards::dst_shards(&settings)?
        .iter()
        .any(|shard| shard.process_key == "caves"))
}

pub(super) fn generate_session_id(instance_id: &str) -> String {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    format!("{instance_id}-{stamp}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum StrictBindEvaluation {
    Pending { missing_port_names: Vec<String> },
    Ready,
    Failed { message: String },
}

pub(super) fn ensure_instance_bind_policy_allowed(
    instance: &InstanceDetails,
    policy: &ModuleBindAddressSpec,
) -> Result<(), String> {
    let expected_address = normalize_strict_bind_address(&instance.summary.bind_ip)?;
    if is_wildcard_bind_address(&expected_address) {
        return Ok(());
    }
    if policy.mode != ModuleBindAddressMode::Strict {
        return Err(format!(
            "module `{}` does not declare strict bind-address support; use 0.0.0.0 for this game",
            instance.summary.module_id
        ));
    }
    ensure_bind_required_setting_enabled(instance, policy)?;
    let _ = resolve_strict_bind_ports(instance, policy)?;
    Ok(())
}

fn ensure_bind_required_setting_enabled(
    instance: &InstanceDetails,
    policy: &ModuleBindAddressSpec,
) -> Result<(), String> {
    let Some(setting_key) = policy.required_setting_key.as_deref() else {
        return Ok(());
    };
    let settings = serde_json::from_str::<Value>(&instance.settings_json).map_err(|error| {
        format!(
            "instance `{}` settings cannot be read for bind-address capability `{setting_key}`: {error}",
            instance.summary.id
        )
    })?;
    let value = setting_key
        .split('.')
        .try_fold(&settings, |current, segment| current.get(segment));
    if value.and_then(Value::as_bool) == Some(true) {
        return Ok(());
    }

    Err(format!(
        "instance `{}` requires boolean setting `{setting_key}` to be enabled before using a specific bind address",
        instance.summary.id
    ))
}

pub(super) async fn verify_spawned_process_bind_address(
    state: &DesktopState,
    storage: &StorageBootstrap,
    instance: &InstanceDetails,
    policy: &ModuleBindAddressSpec,
    spawned_processes: &mut [(ProcessLaunchPlan, app_runtime::SpawnedProcess)],
    source: &str,
    start_reservation: &RuntimeStartReservationLease,
) -> Result<(), String> {
    let expected_address = normalize_strict_bind_address(&instance.summary.bind_ip)?;
    if is_wildcard_bind_address(&expected_address)
        && !super::commands_runtime_ark::has_multiple_maps(instance)?
    {
        return Ok(());
    }
    if policy.mode != ModuleBindAddressMode::Strict {
        return Err(format!(
            "module `{}` does not declare strict bind-address support; use 0.0.0.0 for this game",
            instance.summary.module_id
        ));
    }

    let required_ports = resolve_strict_bind_ports(instance, policy)?;
    let targets = build_window_inspection_targets_from_spawned_processes(spawned_processes);
    if targets.is_empty() {
        return Err(format!(
            "instance `{}` has no managed process available for bind-address verification",
            instance.summary.id
        ));
    }
    let port_numbers = required_ports
        .iter()
        .map(|port| port.port)
        .collect::<Vec<_>>();
    let timeout = Duration::from_millis(policy.startup_timeout_ms);
    let started_at = Instant::now();
    let mut consecutive_ready_samples = 0_u8;

    loop {
        if state.shutdown_in_progress.load(Ordering::SeqCst) || start_reservation.is_cancelled() {
            return Err(format!(
                "instance `{}` bind-address verification was cancelled because application shutdown is in progress",
                instance.summary.id
            ));
        }
        for (plan, spawned) in spawned_processes.iter_mut() {
            match process_is_running(spawned.pid) {
                Ok(true) => {}
                Ok(false) => {
                    return Err(format!(
                        "instance `{}` process `{}` (pid {}) exited before bind-address verification completed",
                        instance.summary.id, plan.display_name, spawned.pid
                    ));
                }
                Err(error) => {
                    return Err(format!(
                        "failed to inspect instance `{}` process `{}` (pid {}) during bind-address verification: {error}",
                        instance.summary.id, plan.display_name, spawned.pid
                    ));
                }
            }
        }
        let inspection_targets = targets.clone();
        let inspection_ports = port_numbers.clone();
        let inspection = tokio::task::spawn_blocking(move || {
            WindowsPlatform::inspect_process_network_endpoints(
                &inspection_targets,
                &inspection_ports,
            )
        })
        .await
        .map_err(|error| format!("bind-address verification worker failed: {error}"))??;

        let evaluation = if app_core::ark_maps::is_ark(&instance.summary.module_id) {
            super::commands_runtime_ark::evaluate_bind_ports(
                &required_ports,
                &expected_address,
                &inspection.endpoints,
            )
        } else {
            evaluate_strict_bind_endpoints(
                &expected_address,
                &required_ports,
                &inspection.endpoints,
            )
        };
        match evaluation {
            StrictBindEvaluation::Ready => {
                consecutive_ready_samples = consecutive_ready_samples.saturating_add(1);
                if consecutive_ready_samples < 3 {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    continue;
                }
                append_desktop_app_log(
                    storage,
                    "info",
                    "instance.bind_address.verified",
                    "Managed process listeners match the selected bind address.",
                    json!({
                        "source": source,
                        "instance_id": instance.summary.id.as_str(),
                        "instance_name": instance.summary.name.as_str(),
                        "expected_address": expected_address,
                        "required_ports": required_ports,
                        "inspected_process_count": inspection.inspected_process_count,
                        "endpoints": inspection.endpoints,
                        "consecutive_ready_samples": consecutive_ready_samples,
                        "elapsed_ms": started_at.elapsed().as_millis(),
                    }),
                );
                return Ok(());
            }
            StrictBindEvaluation::Failed { message } => {
                append_desktop_app_log(
                    storage,
                    "error",
                    "instance.bind_address.failed",
                    &message,
                    json!({
                        "source": source,
                        "instance_id": instance.summary.id.as_str(),
                        "instance_name": instance.summary.name.as_str(),
                        "expected_address": expected_address,
                        "required_ports": required_ports,
                        "inspected_process_count": inspection.inspected_process_count,
                        "endpoints": inspection.endpoints,
                    }),
                );
                return Err(message);
            }
            StrictBindEvaluation::Pending { missing_port_names } => {
                consecutive_ready_samples = 0;
                if started_at.elapsed() >= timeout {
                    let message = format!(
                        "instance `{}` did not expose required listener(s) {} on {} within {} ms",
                        instance.summary.id,
                        missing_port_names.join(", "),
                        expected_address,
                        policy.startup_timeout_ms
                    );
                    append_desktop_app_log(
                        storage,
                        "error",
                        "instance.bind_address.timed_out",
                        &message,
                        json!({
                            "source": source,
                            "instance_id": instance.summary.id.as_str(),
                            "instance_name": instance.summary.name.as_str(),
                            "expected_address": expected_address,
                            "missing_port_names": missing_port_names,
                            "required_ports": required_ports,
                            "inspected_process_count": inspection.inspected_process_count,
                            "endpoints": inspection.endpoints,
                        }),
                    );
                    return Err(message);
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

pub(super) fn evaluate_strict_bind_endpoints(
    expected_address: &str,
    required_ports: &[PortBinding],
    endpoints: &[ProcessNetworkEndpoint],
) -> StrictBindEvaluation {
    let expected_address = match normalize_strict_bind_address(expected_address) {
        Ok(address) => address,
        Err(message) => return StrictBindEvaluation::Failed { message },
    };
    let mut missing_port_names = Vec::new();

    for required in required_ports {
        let matching = endpoints
            .iter()
            .filter(|endpoint| {
                endpoint.local_port == required.port
                    && endpoint.protocol.eq_ignore_ascii_case(&required.protocol)
            })
            .collect::<Vec<_>>();
        if let Some(wrong) = matching
            .iter()
            .find(|endpoint| endpoint.local_address != expected_address)
        {
            return StrictBindEvaluation::Failed {
                message: format!(
                    "listener `{}` ({}/{}) is bound to {} instead of selected address {} (pid {})",
                    required.name,
                    required.protocol,
                    required.port,
                    wrong.local_address,
                    expected_address,
                    wrong.owning_pid
                ),
            };
        }
        if matching.is_empty() {
            missing_port_names.push(required.name.clone());
        }
    }

    if missing_port_names.is_empty() {
        StrictBindEvaluation::Ready
    } else {
        StrictBindEvaluation::Pending { missing_port_names }
    }
}

pub(super) fn resolve_strict_bind_ports(
    instance: &InstanceDetails,
    policy: &ModuleBindAddressSpec,
) -> Result<Vec<PortBinding>, String> {
    if app_core::ark_maps::is_ark(&instance.summary.module_id) {
        return super::commands_runtime_ark::required_bind_ports(instance, policy);
    }
    policy
        .port_names
        .iter()
        .map(|name| {
            instance
                .ports
                .iter()
                .find(|port| port.name == *name)
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "instance `{}` is missing bind verification port `{name}`",
                        instance.summary.id
                    )
                })
        })
        .collect()
}

pub(super) fn normalize_strict_bind_address(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    let parsed = trimmed
        .parse::<IpAddr>()
        .map_err(|_| format!("invalid bind address `{trimmed}`"))?;
    Ok(match parsed {
        IpAddr::V6(ipv6) => ipv6
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(ipv6))
            .to_string(),
        other => other.to_string(),
    })
}

pub(super) fn is_wildcard_bind_address(value: &str) -> bool {
    matches!(value.trim(), "0.0.0.0" | "::")
}

#[cfg(test)]
mod strict_bind_verification_tests {
    use super::*;

    fn game_port() -> PortBinding {
        PortBinding {
            name: String::from("game"),
            protocol: String::from("udp"),
            port: 14159,
        }
    }

    fn endpoint(address: &str) -> ProcessNetworkEndpoint {
        ProcessNetworkEndpoint {
            protocol: String::from("udp"),
            local_address: String::from(address),
            local_port: 14159,
            owning_pid: 33996,
            process_key: String::from("main"),
            relation: String::from("tracked_process"),
        }
    }

    #[test]
    fn strict_bind_accepts_only_the_selected_address() {
        assert_eq!(
            evaluate_strict_bind_endpoints(
                "192.168.31.150",
                &[game_port()],
                &[endpoint("192.168.31.150")],
            ),
            StrictBindEvaluation::Ready
        );
    }

    #[test]
    fn strict_bind_rejects_wildcard_listener() {
        let evaluation = evaluate_strict_bind_endpoints(
            "192.168.31.150",
            &[game_port()],
            &[endpoint("0.0.0.0")],
        );

        assert!(matches!(evaluation, StrictBindEvaluation::Failed { .. }));
    }

    #[test]
    fn strict_bind_waits_until_required_listener_appears() {
        assert_eq!(
            evaluate_strict_bind_endpoints("192.168.31.150", &[game_port()], &[]),
            StrictBindEvaluation::Pending {
                missing_port_names: vec![String::from("game")],
            }
        );
    }

    #[test]
    fn strict_bind_normalizes_ipv4_mapped_ipv6_address() {
        assert_eq!(
            normalize_strict_bind_address("::ffff:192.168.31.150").unwrap(),
            "192.168.31.150"
        );
    }
}

pub(super) fn detect_startup_exit(
    spawned_processes: &mut [(ProcessLaunchPlan, app_runtime::SpawnedProcess)],
    grace_period: Duration,
) -> Result<Option<(usize, Option<i32>)>, String> {
    for (index, (plan, spawned)) in spawned_processes.iter_mut().enumerate() {
        if let Some(exit_code) =
            stabilize_spawned_process(&plan.launch_plan.executable_path, spawned, grace_period)
                .map_err(|error| error.to_string())?
        {
            return Ok(Some((index, Some(exit_code))));
        }
    }

    Ok(None)
}

#[derive(Debug)]
pub(super) struct SpawnedProcessStopStatus {
    pub(super) process_key: String,
    pub(super) pid: u32,
    pub(super) exit_code: Option<i32>,
    pub(super) still_running: bool,
    pub(super) error: Option<String>,
}

#[derive(Debug)]
pub(super) struct SpawnedProcessStopReport {
    pub(super) statuses: Vec<SpawnedProcessStopStatus>,
}

impl SpawnedProcessStopReport {
    pub(super) fn has_survivors(&self) -> bool {
        self.statuses.iter().any(|status| status.still_running)
    }

    pub(super) fn issue_summary(&self) -> Option<String> {
        let issues = self
            .statuses
            .iter()
            .filter_map(|status| {
                if !status.still_running && status.error.is_none() {
                    return None;
                }
                let state = if status.still_running {
                    "is still running"
                } else {
                    "exited after a cleanup error"
                };
                Some(format!(
                    "{} pid {} {state}{}",
                    status.process_key,
                    status.pid,
                    status
                        .error
                        .as_deref()
                        .map(|error| format!(": {error}"))
                        .unwrap_or_default()
                ))
            })
            .collect::<Vec<_>>();
        (!issues.is_empty()).then(|| issues.join("; "))
    }
}

pub(super) fn stop_spawned_processes(
    spawned_processes: &mut [(ProcessLaunchPlan, app_runtime::SpawnedProcess)],
) -> SpawnedProcessStopReport {
    let statuses = spawned_processes
        .iter_mut()
        .map(|(plan, spawned)| {
            let pid = spawned.pid;
            let process_identity = spawned.process_identity.clone();
            let mut exit_code = None;
            let mut errors = Vec::new();
            match stop_spawned_process(spawned) {
                Ok(code) => exit_code = code,
                Err(error) => errors.push(error.to_string()),
            }

            let mut still_running = match process_matches_identity(pid, &process_identity) {
                Ok(running) => running,
                Err(error) => {
                    errors.push(format!("failed to verify process exit: {error}"));
                    true
                }
            };
            if still_running {
                if let Err(error) = kill_process_by_pid(pid, &process_identity) {
                    errors.push(format!("forced process-tree cleanup failed: {error}"));
                }
                let wait_started = Instant::now();
                while wait_started.elapsed() < Duration::from_secs(3) {
                    match process_matches_identity(pid, &process_identity) {
                        Ok(false) => {
                            still_running = false;
                            break;
                        }
                        Ok(true) => std::thread::sleep(Duration::from_millis(100)),
                        Err(error) => {
                            errors.push(format!("failed to verify forced cleanup: {error}"));
                            break;
                        }
                    }
                }
                if still_running {
                    still_running =
                        process_matches_identity(pid, &process_identity).unwrap_or(true);
                }
            }

            SpawnedProcessStopStatus {
                process_key: plan.process_key.clone(),
                pid,
                exit_code,
                still_running,
                error: (!errors.is_empty()).then(|| errors.join("; ")),
            }
        })
        .collect();
    SpawnedProcessStopReport { statuses }
}

pub(super) fn desktop_app_log_path(storage: &StorageBootstrap) -> PathBuf {
    storage.paths.app_log_path()
}

pub(super) fn read_latest_runtime_window_suppression_attempt(
    storage: &StorageBootstrap,
    instance_id: &str,
) -> Option<RuntimeWindowSuppressionAttempt> {
    read_latest_runtime_window_suppression_attempt_from_paths(
        &desktop_app_log_path(storage),
        Some(&storage.paths.logs_root.join("desktop-app.log")),
        instance_id,
    )
}

#[cfg(test)]
pub(super) fn read_latest_runtime_window_suppression_attempt_from_log(
    log_path: &Path,
    instance_id: &str,
) -> Option<RuntimeWindowSuppressionAttempt> {
    read_latest_runtime_window_suppression_attempt_from_paths(log_path, None, instance_id)
}

fn read_latest_runtime_window_suppression_attempt_from_paths(
    log_path: &Path,
    previous_log: Option<&Path>,
    instance_id: &str,
) -> Option<RuntimeWindowSuppressionAttempt> {
    let lines = crate::desktop_app_log::recent_lines(
        log_path,
        previous_log,
        DESKTOP_APP_LOG_TAIL_LINE_LIMIT,
        DESKTOP_APP_LOG_TAIL_BYTE_LIMIT,
    )
    .ok()?;

    for line in lines.into_iter().rev() {
        let Ok(entry) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(result) = runtime_window_suppression_attempt_from_log_entry(&entry, instance_id)
        {
            return Some(result);
        }
    }

    None
}

pub(super) fn runtime_window_suppression_attempt_from_log_entry(
    entry: &Value,
    instance_id: &str,
) -> Option<RuntimeWindowSuppressionAttempt> {
    let action = entry.get("action")?.as_str()?;
    let (source, status) = runtime_window_suppression_attempt_metadata_from_action(action)?;
    let context = entry.get("context")?;
    if context.get("instance_id").and_then(Value::as_str)? != instance_id {
        return None;
    }

    let attempted_at_unix_ms = entry
        .get("ts_unix_ms")
        .and_then(Value::as_u64)
        .map(u128::from)
        .unwrap_or_else(now_unix_ms);
    let summary = entry
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    if status == "failed" {
        let inspected_process_count = context
            .get("inspected_process_count")
            .and_then(Value::as_u64)
            .or_else(|| context.get("process_count").and_then(Value::as_u64))
            .map(|value| value as usize)
            .unwrap_or(0);
        return Some(build_runtime_window_suppression_failed_attempt(
            instance_id,
            source,
            attempted_at_unix_ms,
            inspected_process_count,
            summary,
        ));
    }

    let result = RuntimeWindowSuppressionResult {
        instance_id: instance_id.to_string(),
        source: source.to_string(),
        attempted_at_unix_ms,
        inspected_process_count: context
            .get("inspected_process_count")
            .and_then(Value::as_u64)
            .map(|value| value as usize)
            .unwrap_or(0),
        visible_window_count_before: context
            .get("visible_window_count_before")
            .and_then(Value::as_u64)
            .map(|value| value as usize)
            .unwrap_or(0),
        suppressed_window_count: context
            .get("suppressed_window_count")
            .and_then(Value::as_u64)
            .map(|value| value as usize)
            .unwrap_or(0),
        remaining_visible_window_count: context
            .get("remaining_visible_window_count")
            .and_then(Value::as_u64)
            .map(|value| value as usize)
            .unwrap_or(0),
        summary,
    };

    Some(runtime_window_suppression_attempt_from_result(&result))
}

pub(super) fn runtime_window_suppression_attempt_metadata_from_action(
    action: &str,
) -> Option<(&'static str, &'static str)> {
    match action {
        RUNTIME_WINDOW_MANUAL_SUPPRESSION_ACTION => Some(("manual", "completed")),
        RUNTIME_WINDOW_AUTO_SUPPRESSION_ACTION => Some(("automatic", "completed")),
        RUNTIME_WINDOW_MANUAL_SUPPRESSION_FAILED_ACTION => Some(("manual", "failed")),
        RUNTIME_WINDOW_AUTO_SUPPRESSION_FAILED_ACTION => Some(("automatic", "failed")),
        _ => None,
    }
}

pub(super) fn append_desktop_app_log(
    storage: &StorageBootstrap,
    level: &str,
    action: &str,
    message: &str,
    context: serde_json::Value,
) {
    if let Err(error) = write_desktop_app_log(storage, level, action, message, context) {
        eprintln!(
            "failed to persist desktop diagnostic log {}: {error}",
            desktop_app_log_path(storage).display()
        );
    }
}

pub(super) fn write_desktop_app_log(
    storage: &StorageBootstrap,
    level: &str,
    action: &str,
    message: &str,
    context: serde_json::Value,
) -> std::io::Result<()> {
    let log_path = desktop_app_log_path(storage);
    let entry = json!({
        "ts_unix_ms": SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0),
        "level": level,
        "action": action,
        "message": message,
        "context": context,
    });

    crate::desktop_app_log::append(&log_path, &entry)
}

pub(super) fn logged_error_message(
    storage: &StorageBootstrap,
    message: impl Into<String>,
) -> String {
    let message = message.into();
    let log_path = desktop_app_log_path(storage);
    if let Some(failure) = crate::desktop_app_log::failure_summary(&log_path) {
        return format!("{message}. {failure} Log path: {}", log_path.display());
    }
    format!("{message}. See app log: {}", log_path.display())
}
pub(super) fn ps_string_literal(value: &str) -> String {
    let escaped = value.replace('\'', "''");
    format!("'{escaped}'")
}

pub(super) fn show_directory_picker(current_path: Option<&str>) -> Result<Option<String>, String> {
    let initial_path = current_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ps_string_literal)
        .unwrap_or_else(|| String::from("$null"));
    let script = format!(
        concat!(
            "$ErrorActionPreference='Stop'\n",
            "Add-Type -AssemblyName System.Windows.Forms | Out-Null\n",
            "$dialog = New-Object System.Windows.Forms.FolderBrowserDialog\n",
            "$dialog.Description = 'Select folder'\n",
            "$dialog.ShowNewFolderButton = $true\n",
            "$initial = {initial_path}\n",
            "if ($initial -and (Test-Path -LiteralPath $initial)) {{ $dialog.SelectedPath = $initial }}\n",
            "$result = $dialog.ShowDialog()\n",
            "if ($result -eq [System.Windows.Forms.DialogResult]::OK -and $dialog.SelectedPath) {{\n",
            "  [Console]::OutputEncoding = [System.Text.Encoding]::UTF8\n",
            "  Write-Output $dialog.SelectedPath\n",
            "}}\n"
        ),
        initial_path = initial_path,
    );

    let mut command = ProcessCommand::new("powershell");
    apply_no_window(&mut command);
    let output = command
        .arg("-Sta")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(script)
        .output()
        .map_err(|error| format!("failed to open folder picker: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            return Err(String::from("failed to open folder picker"));
        }
        return Err(format!("failed to open folder picker: {stderr}"));
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .rev()
        .find(|line| !line.is_empty())
        .map(String::from))
}
