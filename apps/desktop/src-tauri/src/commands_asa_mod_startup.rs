use super::*;

pub(super) fn requires_barrier<'a>(
    module_id: &str,
    process_count: usize,
    args: impl Iterator<Item = &'a String>,
) -> bool {
    if module_id != "arksurvivalascended" || process_count < 2 {
        return false;
    }
    let mut args = args.peekable();
    while let Some(argument) = args.next() {
        let (key, value) = argument.split_once('=').map_or_else(
            || (argument.as_str(), args.peek().map(|value| value.as_str())),
            |(key, value)| (key, Some(value)),
        );
        if (key.eq_ignore_ascii_case("-mods") || key.eq_ignore_ascii_case("-passivemods"))
            && value.is_some_and(|value| !value.trim().is_empty() && !value.starts_with('-'))
        {
            return true;
        }
    }
    false
}

fn required_game_listener(
    instance: &InstanceDetails,
    process_key: &str,
) -> Result<Vec<PortBinding>, String> {
    let projected = app_core::ark_maps::project_process(instance, Some(process_key))?;
    let mut game = projected
        .ports
        .into_iter()
        .find(|port| port.name == "game" && port.protocol.eq_ignore_ascii_case("udp"))
        .ok_or_else(|| format!("ASA map `{process_key}` is missing its UDP game port."))?;
    game.name = format!("{process_key}:game");
    Ok(vec![game])
}

pub(super) struct StartupTarget<'a> {
    pub instance: &'a InstanceDetails,
    pub process_key: &'a str,
}

/// ASA maps share one native Mods download cache. Native initialization finishes
/// Mod installation/loading before opening the game listener, so do not start
/// another cache writer until the preceding map owns that listener. This gate
/// does not publish health readiness; complete bind/advertising checks still run.
pub(super) async fn wait_until_previous_map_ready(
    state: &DesktopState,
    storage: &StorageBootstrap,
    target: StartupTarget<'_>,
    policy: &ModuleBindAddressSpec,
    processes: &mut [(ProcessLaunchPlan, app_runtime::SpawnedProcess)],
    context: super::super::commands_runtime_supervision::StartupVerificationContext<'_>,
) -> Result<(), String> {
    let super::super::commands_runtime_supervision::StartupVerificationContext {
        source,
        reservation,
        asa_session: startup_images,
    } = context;
    let StartupTarget {
        instance,
        process_key: next_process_key,
    } = target;
    let (previous_key, previous_log_path) = processes
        .last()
        .map(|(plan, _)| (plan.process_key.clone(), plan.log_path.clone()))
        .ok_or("ASA Mod cache barrier has no preceding map")?;
    let startup_console_path = processes
        .first()
        .ok_or("ASA Mod cache barrier has no startup console")?
        .0
        .log_path
        .clone();
    let required_ports = required_game_listener(instance, &previous_key)?;
    let expected_address = normalize_strict_bind_address(&instance.summary.bind_ip)?;
    let targets = build_window_inspection_targets_from_spawned_processes(processes);
    let ports = required_ports
        .iter()
        .map(|port| port.port)
        .collect::<Vec<_>>();
    let started = Instant::now();
    let context = json!({
        "source": source, "instance_id": instance.summary.id,
        "process_key": previous_key, "next_process_key": next_process_key,
        "required_ports": required_ports, "timeout_ms": policy.startup_timeout_ms,
    });
    append_desktop_app_log(
        storage,
        "info",
        "instance.asa_mod_cache.waiting",
        "Waiting for the preceding ASA map to finish native Mod initialization before starting another map.",
        context.clone(),
    );
    let _ = append_startup_console_line(
        &startup_console_path,
        &format!(
            "ASA Mod cache: waiting for map `{}` game listener before starting `{next_process_key}`.",
            previous_key
        ),
        "",
    );

    let budget = super::super::commands_asa_startup_budget::StartupBudget::prepare(
        &instance.summary.module_id,
        processes,
        Duration::from_millis(policy.startup_timeout_ms),
        reservation,
        startup_images,
        &required_ports,
    )
    .await?;
    let result = wait_for_owned_listener_with_budget(
        Duration::from_millis(policy.startup_timeout_ms),
        Duration::from_millis(500),
        reservation,
        || ensure_runtime_start_allowed(state, storage, &instance.summary.id, source, reservation),
        || {
            let targets = targets.clone();
            let ports = ports.clone();
            let required_ports = required_ports.clone();
            let expected_address = expected_address.clone();
            async move {
                tokio::task::spawn_blocking(move || {
                    // Earlier maps must remain alive too; PID reuse is not a
                    // running owned process and must never satisfy this gate.
                    for target in &targets {
                        if !process_matches_identity(target.pid, &target.process_identity)
                            .map_err(|error| format!("ASA Mod cache barrier could not inspect map `{}`: {error}", target.process_key))?
                        {
                            return Err(format!("ASA map `{}` (pid {}) exited before Mod cache initialization completed.", target.process_key, target.pid));
                        }
                    }
                    let inspection = WindowsPlatform::inspect_process_network_endpoints(&targets, &ports)?;
                    for target in &targets {
                        if !process_matches_identity(target.pid, &target.process_identity)
                            .map_err(|error| error.to_string())?
                        {
                            return Err(format!("ASA map `{}` exited or changed identity during listener inspection", target.process_key));
                        }
                    }
                    Ok(super::super::commands_runtime_ark::evaluate_bind_ports(
                        &required_ports, &expected_address, &inspection.endpoints))
                }).await.map_err(|error| format!("ASA Mod cache inspection worker failed: {error}"))?
            }
        },
        Some(budget),
    ).await;
    let mut context = context;
    context["elapsed_ms"] = json!(started.elapsed().as_millis());
    match result {
        Ok(()) => {
            append_desktop_app_log(
                storage,
                "info",
                "instance.asa_mod_cache.verified",
                "The preceding ASA map owns its game listener; the next map may initialize Mods.",
                context,
            );
            let _ = append_startup_console_line(
                &startup_console_path,
                &format!(
                    "ASA Mod cache: map `{}` owns its game listener; starting `{next_process_key}`.",
                    previous_key
                ),
                "",
            );
            Ok(())
        }
        Err(error) => {
            let error = format!(
                "ASA map `{}` Mod cache startup barrier failed before `{next_process_key}`: {error}; see {}",
                previous_key, previous_log_path
            );
            append_desktop_app_log(
                storage,
                "error",
                "instance.asa_mod_cache.failed",
                &error,
                context,
            );
            Err(error)
        }
    }
}

#[cfg(test)]
async fn wait_for_owned_listener<Observe, Observation, Allowed>(
    timeout: Duration,
    sample_interval: Duration,
    reservation: &RuntimeStartReservationLease,
    mut allowed: Allowed,
    mut observe: Observe,
) -> Result<(), String>
where
    Observe: FnMut() -> Observation,
    Observation: std::future::Future<Output = Result<StrictBindEvaluation, String>>,
    Allowed: FnMut() -> Result<(), String>,
{
    wait_for_owned_listener_with_budget(
        timeout,
        sample_interval,
        reservation,
        &mut allowed,
        &mut observe,
        None,
    )
    .await
}

async fn wait_for_owned_listener_with_budget<Observe, Observation, Allowed>(
    timeout: Duration,
    sample_interval: Duration,
    reservation: &RuntimeStartReservationLease,
    mut allowed: Allowed,
    mut observe: Observe,
    mut budget: Option<super::super::commands_asa_startup_budget::StartupBudget<'_>>,
) -> Result<(), String>
where
    Observe: FnMut() -> Observation,
    Observation: std::future::Future<Output = Result<StrictBindEvaluation, String>>,
    Allowed: FnMut() -> Result<(), String>,
{
    let deadline = tokio::time::Instant::now() + timeout;
    let mut ready_samples = 0_u8;
    loop {
        allowed()?;
        let current_deadline = if let Some(budget) = &mut budget {
            let remaining = tokio::select! {
                biased;
                _ = reservation.cancelled() => return Err(String::from("ASA Mod cache startup wait was cancelled.")),
                result = budget.remaining() => result?,
            };
            tokio::time::Instant::now() + remaining
        } else {
            deadline
        };
        let timeout_message = budget.as_ref().map_or_else(
            || {
                format!(
                    "owned game listener was not verified within {} ms",
                    timeout.as_millis()
                )
            },
            |budget| budget.timeout_message(),
        );
        let evaluation = tokio::select! {
            biased;
            _ = reservation.cancelled() => return Err(String::from("ASA Mod cache startup wait was cancelled.")),
            result = tokio::time::timeout_at(current_deadline, observe()) => result
                .map_err(|_| timeout_message.clone())??,
        };
        allowed()?;
        if let Some(budget) = &mut budget {
            budget.record_evaluation(&evaluation)?;
        }
        match evaluation {
            StrictBindEvaluation::Ready => {
                ready_samples += 1;
                if ready_samples >= 3 {
                    return Ok(());
                }
            }
            StrictBindEvaluation::Failed { message } => return Err(message),
            StrictBindEvaluation::Pending { .. } => ready_samples = 0,
        }
        if tokio::time::Instant::now() >= current_deadline {
            return Err(timeout_message);
        }
        wait_for_runtime_start_delay(
            reservation,
            sample_interval
                .min(current_deadline.saturating_duration_since(tokio::time::Instant::now())),
        )
        .await;
    }
}

#[cfg(test)]
#[path = "commands_asa_mod_startup_tests.rs"]
mod tests;
