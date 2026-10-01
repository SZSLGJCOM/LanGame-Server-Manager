use super::commands_global_player_counts::{
    GlobalPlayerCountSnapshot, collect_global_player_counts,
};
use super::*;
use app_storage::resolve_player_query_target as resolve_player_query_target_from_parts;

#[path = "commands_system_storage_paths.rs"]
mod system_storage_paths;

#[cfg(test)]
pub(super) use app_storage::{
    parse_a2s_info_payload, parse_minecraft_query_basic_response,
    parse_minecraft_query_challenge_response,
};

#[derive(Debug, Default, Clone, Copy)]
struct InstanceProcessMemorySnapshot {
    memory_bytes: u64,
    memory_percent: f32,
    process_count: usize,
    thread_count: usize,
    handle_count: usize,
}

#[derive(Debug, Clone)]
pub(super) struct ModuleRuntimeCapabilities {
    pub(super) player_query: Option<ModulePlayerQuerySpec>,
    pub(super) player_count_source: app_core::ModulePlayerCountSource,
    pub(super) performance: RuntimePerformancePolicy,
}

async fn collect_instance_process_memory() -> InstanceProcessMemorySnapshot {
    let storage = match bootstrap_storage() {
        Ok(storage) => storage,
        Err(_) => return InstanceProcessMemorySnapshot::default(),
    };
    let active_runs = match list_active_instance_runs(&storage.paths).await {
        Ok(active_runs) => active_runs,
        Err(_) => return InstanceProcessMemorySnapshot::default(),
    };
    let pids = active_runs
        .into_iter()
        .filter_map(|run| run.pid)
        .collect::<Vec<_>>();
    let process_metrics = read_process_memory_metrics(&pids);
    let memory_bytes = process_metrics
        .iter()
        .map(|process| process.working_set_bytes)
        .sum::<u64>();
    let thread_count = process_metrics
        .iter()
        .map(|process| process.thread_count as usize)
        .sum::<usize>();
    let handle_count = process_metrics
        .iter()
        .map(|process| process.handle_count as usize)
        .sum::<usize>();
    InstanceProcessMemorySnapshot {
        memory_bytes,
        memory_percent: 0.0,
        process_count: process_metrics.len(),
        thread_count,
        handle_count,
    }
}

fn with_instance_memory_percent(
    mut snapshot: InstanceProcessMemorySnapshot,
    total_memory_bytes: u64,
) -> InstanceProcessMemorySnapshot {
    snapshot.memory_percent = if total_memory_bytes > 0 {
        (snapshot.memory_bytes as f64 / total_memory_bytes as f64 * 100.0) as f32
    } else {
        0.0
    };
    snapshot
}

pub(crate) fn extract_instance_player_capacity(settings_json: &str) -> Option<usize> {
    let value: Value = serde_json::from_str(settings_json).ok()?;
    const PLAYER_CAPACITY_SETTING_KEYS: &[&str] = &["max_players", "max_server_players"];

    PLAYER_CAPACITY_SETTING_KEYS
        .iter()
        .find_map(|key| value.get(*key).and_then(json_usize))
}

fn json_usize(value: &Value) -> Option<usize> {
    match value {
        Value::Number(number) => number.as_u64().map(|value| value as usize),
        Value::String(text) => text.trim().parse::<usize>().ok(),
        _ => None,
    }
}

pub(super) fn build_runtime_performance_diagnostic(
    details: &InstanceDetails,
    module_policy: Option<&RuntimePerformancePolicy>,
) -> Option<RuntimeDiagnosticSignal> {
    details.active_run.as_ref()?;
    let settings = serde_json::from_str(&details.settings_json).ok()?;
    let default_policy = RuntimePerformancePolicy::default();
    let policy = resolve_runtime_performance_policy_for_instance(
        module_policy.unwrap_or(&default_policy),
        &settings,
        &details.summary.id,
    );
    Some(runtime_performance_diagnostic_signal(&policy))
}

fn runtime_performance_diagnostic_signal(
    policy: &RuntimePerformancePolicy,
) -> RuntimeDiagnosticSignal {
    let affinity = policy
        .cpu_affinity_mask
        .map(|mask| format!("0x{mask:X}"))
        .unwrap_or_else(|| String::from("all available CPUs"));
    RuntimeDiagnosticSignal {
        code: String::from("runtime_performance_policy"),
        severity: String::from("info"),
        summary: format!(
            "Runtime performance policy is active: priority={:?}, affinity={}, instance stagger={}ms, child process stagger={}ms.",
            policy.priority_class,
            affinity,
            policy.startup_stagger_ms,
            policy.child_process_stagger_ms
        ),
        matched_line: None,
        actionable: false,
    }
}

pub(super) fn runtime_performance_state_diagnostic_signal(
    performance: &RuntimePerformanceSnapshot,
) -> Option<RuntimeDiagnosticSignal> {
    match performance.status.as_str() {
        "untracked" => Some(RuntimeDiagnosticSignal {
            code: String::from("runtime_performance_untracked"),
            severity: String::from("warning"),
            summary: String::from(
                "This instance has active run records, but the current LanGame session is not supervising the process tree. Restart it from LanGame to restore performance policy refresh, process-tree stop, and guarded recovery.",
            ),
            matched_line: None,
            actionable: true,
        }),
        "pending" => Some(RuntimeDiagnosticSignal {
            code: String::from("runtime_performance_pending"),
            severity: String::from("info"),
            summary: String::from(
                "LanGame resolved the runtime performance policy, but the process PID is not available yet.",
            ),
            matched_line: None,
            actionable: false,
        }),
        _ => None,
    }
}

pub(super) fn build_runtime_performance_snapshot(
    state: &tauri::State<'_, DesktopState>,
    details: &InstanceDetails,
    module_policy: Option<&RuntimePerformancePolicy>,
) -> RuntimePerformanceSnapshot {
    let settings = serde_json::from_str(&details.settings_json).unwrap_or(Value::Null);
    let default_policy = RuntimePerformancePolicy::default();
    let (policy, preview) = resolve_runtime_performance_policy_with_preview_for_instance(
        module_policy.unwrap_or(&default_policy),
        &settings,
        &details.summary.id,
    );

    let Some(active_run) = details.active_run.as_ref() else {
        return RuntimePerformanceSnapshot {
            applied_resource_limits: None,
            status: String::from("stopped"),
            summary: preview.summary.clone(),
            policy,
            preview,
            process_count: 0,
            processes: Vec::new(),
        };
    };

    let managed_snapshots = state
        .runtime_supervisor
        .lock()
        .ok()
        .map(|runtime| runtime.performance_snapshots(&details.summary.id))
        .unwrap_or_default();

    if !managed_snapshots.is_empty() {
        let mut snapshot = summarize_runtime_performance_snapshots(
            String::from("managed"),
            policy,
            preview,
            managed_snapshots,
        );
        match state
            .runtime_resource_admission
            .limits_for_instance(&details.summary.id)
        {
            Ok(limits) => snapshot.applied_resource_limits = limits,
            Err(error) => snapshot
                .summary
                .push_str(&format!(" Resource-limit observation failed: {error}")),
        }
        return snapshot;
    }

    let process_snapshots = active_run
        .processes
        .iter()
        .filter_map(|process| {
            let pid = process.pid?;
            Some(RuntimeProcessPerformanceSnapshot {
                process_key: process.process_key.clone(),
                display_name: process.display_name.clone(),
                pid,
                policy: policy.clone(),
                application: None,
            })
        })
        .collect::<Vec<_>>();

    if process_snapshots.is_empty() {
        RuntimePerformanceSnapshot {
            applied_resource_limits: None,
            status: String::from("pending"),
            summary: String::from(
                "Runtime performance policy is resolved, but no process PID is available yet.",
            ),
            policy,
            preview,
            process_count: active_run.process_count,
            processes: Vec::new(),
        }
    } else {
        RuntimePerformanceSnapshot {
            applied_resource_limits: None,
            status: String::from("untracked"),
            summary: String::from(
                "The instance has an active run record, but this app session is not supervising the process tree; restart management is limited.",
            ),
            policy,
            preview,
            process_count: process_snapshots.len(),
            processes: process_snapshots,
        }
    }
}

fn summarize_runtime_performance_snapshots(
    status: String,
    policy: RuntimePerformancePolicy,
    preview: app_core::RuntimePerformancePolicyPreview,
    processes: Vec<RuntimeProcessPerformanceSnapshot>,
) -> RuntimePerformanceSnapshot {
    let process_count = processes.len();
    let warning_count = processes
        .iter()
        .filter_map(|process| process.application.as_ref())
        .map(|application| application.warnings.len())
        .sum::<usize>();
    let targeted_process_count = processes
        .iter()
        .filter_map(|process| process.application.as_ref())
        .map(|application| application.targeted_process_count)
        .sum::<usize>();
    let priority_applied_count = processes
        .iter()
        .filter_map(|process| process.application.as_ref())
        .map(|application| application.priority_applied_count)
        .sum::<usize>();
    let affinity_applied_count = processes
        .iter()
        .filter_map(|process| process.application.as_ref())
        .map(|application| application.affinity_applied_count)
        .sum::<usize>();

    let summary = if warning_count > 0 {
        format!(
            "Performance policy is supervising {process_count} tracked process record(s), with {warning_count} warning(s)."
        )
    } else if affinity_applied_count > 0 {
        format!(
            "Performance policy is active across {targeted_process_count} process tree member(s); priority applied {priority_applied_count} time(s), affinity applied {affinity_applied_count} time(s)."
        )
    } else {
        format!(
            "Performance policy is active across {targeted_process_count} process tree member(s); priority applied {priority_applied_count} time(s)."
        )
    };

    RuntimePerformanceSnapshot {
        applied_resource_limits: None,
        status,
        summary,
        policy,
        preview,
        process_count,
        processes,
    }
}

pub(super) async fn build_runtime_startup_queue_snapshot(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    startup_policy: &RuntimePerformancePolicy,
    planned_process_count: usize,
) -> RuntimeStartupQueueSnapshot {
    let (active_run_count, active_process_count) = list_active_instance_runs(&storage.paths)
        .await
        .map(|runs| summarize_active_runtime_entries(&runs))
        .unwrap_or((0, 0));
    let (tracked_instance_count, tracked_process_count) = state
        .runtime_supervisor
        .lock()
        .ok()
        .map(|runtime| {
            let tracked = runtime.tracked_instances();
            let process_count = tracked.iter().map(|instance| instance.process_count).sum();
            (tracked.len(), process_count)
        })
        .unwrap_or((0, 0));
    let projected_process_count = planned_process_count.max(1);
    let (
        next_start_delay_ms,
        projected_effective_stagger_ms,
        projected_queued_start_count,
        last_startup_schedule,
    ) = state
        .startup_scheduler
        .lock()
        .ok()
        .map(|scheduler| {
            let preview = scheduler.preview_start(RuntimeStartupReservation {
                base_stagger_ms: startup_policy.startup_stagger_ms,
                running_instance_count: active_run_count,
                active_process_count,
                process_count: projected_process_count,
            });
            (
                u64::try_from(preview.delay.as_millis()).unwrap_or(u64::MAX),
                preview.effective_stagger_ms,
                preview.queued_start_count,
                scheduler.last_startup_schedule(),
            )
        })
        .unwrap_or((0, 0, 0, None));
    let (pending_restart_count, next_restart_delay_ms, next_restart) = state
        .runtime_restart_scheduler
        .lock()
        .ok()
        .map(|scheduler| {
            (
                scheduler.pending_count(),
                u64::try_from(scheduler.next_restart_delay().as_millis()).unwrap_or(u64::MAX),
                scheduler.next_restart(),
            )
        })
        .unwrap_or((0, 0, None));
    let status = if next_start_delay_ms > 0 {
        String::from("queued")
    } else if pending_restart_count > 0 {
        String::from("restart_pending")
    } else {
        String::from("ready")
    };
    let summary = if next_start_delay_ms > 0 {
        format!(
            "Startup scheduler is cooling down for {next_start_delay_ms}ms with {active_run_count} active instance(s), {active_process_count} active process record(s), {tracked_process_count} tracked process(es), and {pending_restart_count} pending restart(s). The next start would use a {projected_effective_stagger_ms}ms effective stagger."
        )
    } else if pending_restart_count > 0 {
        format!(
            "Startup scheduler is ready with {active_run_count} active instance(s), {active_process_count} active process record(s), and {tracked_process_count} tracked process(es); next guarded restart is due in {next_restart_delay_ms}ms. The next start would use a {projected_effective_stagger_ms}ms effective stagger."
        )
    } else {
        format!(
            "Startup scheduler is ready with {active_run_count} active instance(s), {active_process_count} active process record(s), and {tracked_process_count} tracked process(es). The next start would use a {projected_effective_stagger_ms}ms effective stagger."
        )
    };

    RuntimeStartupQueueSnapshot {
        status,
        summary,
        active_run_count,
        active_process_count,
        tracked_instance_count,
        tracked_process_count,
        next_start_delay_ms,
        projected_effective_stagger_ms,
        projected_queued_start_count,
        projected_process_count,
        pending_restart_count,
        next_restart_delay_ms,
        next_restart,
        last_startup_schedule,
    }
}

pub(super) fn summarize_active_runtime_entries(
    active_runs: &[ActiveInstanceRunEntry],
) -> (usize, usize) {
    let active_run_count = active_runs
        .iter()
        .map(|run| run.instance_id.as_str())
        .collect::<HashSet<_>>()
        .len();
    (active_run_count, active_runs.len())
}

fn parse_settings_json(settings_json: &str) -> Value {
    serde_json::from_str(settings_json).unwrap_or(Value::Null)
}

#[derive(Debug, Clone)]
pub(super) struct RuntimeRestartPolicy {
    pub(super) enabled: bool,
    pub(super) max_restarts: usize,
    pub(super) backoff_ms: u64,
    pub(super) only_nonzero_exit: bool,
}

#[derive(Debug, Clone)]
pub(super) struct RuntimeRestartCandidate {
    pub(super) instance_id: String,
    pub(super) instance_name: String,
    pub(super) policy: RuntimeRestartPolicy,
    pub(super) recent_crash_count: usize,
    pub(super) exit_code: Option<i32>,
}

pub(super) fn runtime_restart_policy_from_settings(settings_json: &str) -> RuntimeRestartPolicy {
    let settings = parse_settings_json(settings_json);
    let policy = settings.get("runtime_restart").and_then(Value::as_object);
    RuntimeRestartPolicy {
        enabled: policy
            .and_then(|policy| policy.get("enabled"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        max_restarts: policy
            .and_then(|policy| policy.get("max_restarts"))
            .and_then(Value::as_u64)
            .unwrap_or(3)
            .clamp(1, 10) as usize,
        backoff_ms: policy
            .and_then(|policy| policy.get("backoff_ms"))
            .and_then(Value::as_u64)
            .unwrap_or(5_000)
            .min(300_000),
        only_nonzero_exit: policy
            .and_then(|policy| policy.get("only_nonzero_exit"))
            .and_then(Value::as_bool)
            .unwrap_or(true),
    }
}

pub(super) fn json_bool(value: Option<&Value>) -> Option<bool> {
    match value {
        Some(Value::Bool(flag)) => Some(*flag),
        Some(Value::String(text)) => match text.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => Some(true),
            "false" | "0" | "no" | "off" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn json_u16(value: Option<&Value>) -> Option<u16> {
    match value {
        Some(Value::Number(number)) => number.as_u64().and_then(|value| u16::try_from(value).ok()),
        Some(Value::String(text)) => text.trim().parse::<u16>().ok(),
        _ => None,
    }
}

pub(super) fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn is_instance_running(details: &InstanceDetails) -> bool {
    matches!(details.summary.status, InstanceStatus::Running)
}

fn find_port_binding<'a>(
    details: &'a InstanceDetails,
    protocol: &str,
    names: &[&str],
) -> Option<&'a app_core::PortBinding> {
    names.iter().find_map(|name| {
        details.ports.iter().find(|port| {
            port.protocol.eq_ignore_ascii_case(protocol) && port.name.eq_ignore_ascii_case(name)
        })
    })
}

fn find_port_number(details: &InstanceDetails, protocol: &str, names: &[&str]) -> Option<u16> {
    find_port_binding(details, protocol, names).map(|port| port.port)
}

fn resolve_palworld_operator_host(details: &InstanceDetails, _settings: &Value) -> String {
    normalize_query_host(&details.summary.bind_ip)
}

struct TcpProbeResult {
    reachable: bool,
    status: String,
    detail: String,
}

fn probe_tcp_listener(host: &str, port: u16) -> TcpProbeResult {
    let address_text = format!("{host}:{port}");
    let addresses = match address_text.to_socket_addrs() {
        Ok(addresses) => {
            let mut ipv4 = Vec::new();
            let mut other = Vec::new();
            for address in addresses {
                if address.is_ipv4() {
                    ipv4.push(address);
                } else {
                    other.push(address);
                }
            }
            ipv4.extend(other);
            ipv4
        }
        Err(error) => {
            return TcpProbeResult {
                reachable: false,
                status: String::from("resolve_failed"),
                detail: format!("Could not resolve `{address_text}`: {error}"),
            };
        }
    };

    if addresses.is_empty() {
        return TcpProbeResult {
            reachable: false,
            status: String::from("resolve_failed"),
            detail: format!("No socket addresses could be resolved for `{address_text}`."),
        };
    }

    let timeout = Duration::from_millis(900);
    let mut last_error = TcpProbeResult {
        reachable: false,
        status: String::from("connect_error"),
        detail: format!("No TCP connection attempt has completed for `{address_text}`."),
    };

    for address in addresses {
        match TcpStream::connect_timeout(&address, timeout) {
            Ok(stream) => {
                let _ = stream.shutdown(Shutdown::Both);
                return TcpProbeResult {
                    reachable: true,
                    status: String::from("reachable"),
                    detail: format!("TCP listener accepted a connection on `{address}`."),
                };
            }
            Err(error) => {
                let status = match error.kind() {
                    ErrorKind::ConnectionRefused => "refused",
                    ErrorKind::TimedOut => "timeout",
                    _ => "connect_error",
                };
                last_error = TcpProbeResult {
                    reachable: false,
                    status: String::from(status),
                    detail: format!("TCP probe to `{address}` failed: {error}"),
                };
            }
        }
    }

    last_error
}

async fn probe_tcp_listener_async(host: &str, port: u16) -> TcpProbeResult {
    let host = host.to_owned();
    match tokio::task::spawn_blocking(move || probe_tcp_listener(&host, port)).await {
        Ok(probe) => probe,
        Err(error) => TcpProbeResult {
            reachable: false,
            status: String::from("probe_task_failed"),
            detail: format!("TCP probe task failed: {error}"),
        },
    }
}

async fn query_live_player_count_async(
    protocol: &str,
    host: &str,
    port: u16,
) -> Option<app_storage::QueriedPlayerCount> {
    let protocol = protocol.to_owned();
    let host = host.to_owned();
    match tokio::task::spawn_blocking(move || {
        app_storage::query_live_player_count(&protocol, &host, port)
    })
    .await
    {
        Ok(result) => result,
        Err(error) => {
            eprintln!("live player query task failed: {error}");
            None
        }
    }
}

fn build_operator_http_client(
    builder: reqwest::ClientBuilder,
) -> Result<reqwest::Client, reqwest::Error> {
    // Operator endpoints belong to the selected local instance. System proxies
    // must not redirect their probes away from the listener checked over TCP.
    builder
        .no_proxy()
        .timeout(Duration::from_millis(1200))
        .redirect(reqwest::redirect::Policy::none())
        .build()
}

async fn build_palworld_rest_probe(
    details: &InstanceDetails,
    host: &str,
    settings: &Value,
) -> PalworldOperatorServiceProbe {
    let enabled = json_bool(settings.get("rest_api_enabled")).unwrap_or(false);
    let port = find_port_number(details, "tcp", &["rest_api"]);
    let endpoint = if enabled {
        port.map(|port| format!("http://{host}:{port}/"))
    } else {
        None
    };

    if !enabled {
        return PalworldOperatorServiceProbe {
            key: String::from("rest_api"),
            transport: String::from("http"),
            enabled: false,
            configured: port.is_some(),
            endpoint,
            reachable: false,
            status: String::from("disabled"),
            detail: String::from("REST API is disabled in instance settings."),
            http_status: None,
        };
    }

    let Some(port) = port else {
        return PalworldOperatorServiceProbe {
            key: String::from("rest_api"),
            transport: String::from("http"),
            enabled: true,
            configured: false,
            endpoint: None,
            reachable: false,
            status: String::from("not_configured"),
            detail: String::from("REST API is enabled, but no TCP rest_api port is registered."),
            http_status: None,
        };
    };

    let endpoint = Some(format!("http://{host}:{port}/"));
    if !is_instance_running(details) {
        return PalworldOperatorServiceProbe {
            key: String::from("rest_api"),
            transport: String::from("http"),
            enabled: true,
            configured: true,
            endpoint,
            reachable: false,
            status: String::from("waiting_for_start"),
            detail: String::from("REST API will be probed after the instance starts."),
            http_status: None,
        };
    }

    let tcp_probe = probe_tcp_listener_async(host, port).await;
    if !tcp_probe.reachable {
        return PalworldOperatorServiceProbe {
            key: String::from("rest_api"),
            transport: String::from("http"),
            enabled: true,
            configured: true,
            endpoint,
            reachable: false,
            status: tcp_probe.status,
            detail: tcp_probe.detail,
            http_status: None,
        };
    }

    let client = match build_operator_http_client(reqwest::Client::builder()) {
        Ok(client) => client,
        Err(error) => {
            return PalworldOperatorServiceProbe {
                key: String::from("rest_api"),
                transport: String::from("http"),
                enabled: true,
                configured: true,
                endpoint,
                reachable: true,
                status: String::from("http_client_error"),
                detail: format!(
                    "REST API listener is reachable, but the HTTP client could not be created: {error}"
                ),
                http_status: None,
            };
        }
    };

    let request_url = format!("http://{host}:{port}/");
    match client.get(&request_url).send().await {
        Ok(response) => {
            let status = response.status();
            PalworldOperatorServiceProbe {
                key: String::from("rest_api"),
                transport: String::from("http"),
                enabled: true,
                configured: true,
                endpoint,
                reachable: true,
                status: String::from("reachable"),
                detail: format!(
                    "HTTP probe returned status {} from `{request_url}`.",
                    status.as_u16()
                ),
                http_status: Some(status.as_u16()),
            }
        }
        Err(error) => {
            let status = if error.is_timeout() {
                "http_timeout"
            } else {
                "http_error"
            };
            PalworldOperatorServiceProbe {
                key: String::from("rest_api"),
                transport: String::from("http"),
                enabled: true,
                configured: true,
                endpoint,
                reachable: false,
                status: String::from(status),
                detail: format!(
                    "REST API listener accepted TCP, but HTTP probing `{request_url}` failed: {error}"
                ),
                http_status: None,
            }
        }
    }
}

async fn build_palworld_rcon_probe(
    details: &InstanceDetails,
    host: &str,
    settings: &Value,
) -> PalworldOperatorServiceProbe {
    let enabled = json_bool(settings.get("rcon_enabled")).unwrap_or(false);
    let port = find_port_number(details, "tcp", &["rcon"]);
    let endpoint = if enabled {
        port.map(|port| format!("{host}:{port}"))
    } else {
        None
    };

    if !enabled {
        return PalworldOperatorServiceProbe {
            key: String::from("rcon"),
            transport: String::from("tcp"),
            enabled: false,
            configured: port.is_some(),
            endpoint,
            reachable: false,
            status: String::from("disabled"),
            detail: String::from("RCON is disabled in instance settings."),
            http_status: None,
        };
    }

    let Some(port) = port else {
        return PalworldOperatorServiceProbe {
            key: String::from("rcon"),
            transport: String::from("tcp"),
            enabled: true,
            configured: false,
            endpoint: None,
            reachable: false,
            status: String::from("not_configured"),
            detail: String::from("RCON is enabled, but no TCP rcon port is registered."),
            http_status: None,
        };
    };

    let endpoint = Some(format!("{host}:{port}"));
    if !is_instance_running(details) {
        return PalworldOperatorServiceProbe {
            key: String::from("rcon"),
            transport: String::from("tcp"),
            enabled: true,
            configured: true,
            endpoint,
            reachable: false,
            status: String::from("waiting_for_start"),
            detail: String::from("RCON will be probed after the instance starts."),
            http_status: None,
        };
    }

    let tcp_probe = probe_tcp_listener_async(host, port).await;
    PalworldOperatorServiceProbe {
        key: String::from("rcon"),
        transport: String::from("tcp"),
        enabled: true,
        configured: true,
        endpoint,
        reachable: tcp_probe.reachable,
        status: tcp_probe.status,
        detail: tcp_probe.detail,
        http_status: None,
    }
}

async fn build_palworld_game_probe(
    details: &InstanceDetails,
    host: &str,
    settings: &Value,
) -> PalworldOperatorServiceProbe {
    let port = find_port_number(details, "udp", &["query", "game"])
        .or_else(|| json_u16(settings.get("public_port")));
    let endpoint = port.map(|port| format!("{host}:{port}"));
    let configured = port.is_some();

    if !configured {
        return PalworldOperatorServiceProbe {
            key: String::from("game"),
            transport: String::from("udp"),
            enabled: true,
            configured: false,
            endpoint: None,
            reachable: false,
            status: String::from("not_configured"),
            detail: String::from("No UDP query or game port is registered for Palworld."),
            http_status: None,
        };
    }

    if !is_instance_running(details) {
        return PalworldOperatorServiceProbe {
            key: String::from("game"),
            transport: String::from("udp"),
            enabled: true,
            configured: true,
            endpoint,
            reachable: false,
            status: String::from("waiting_for_start"),
            detail: String::from("Live UDP querying runs only while the instance is running."),
            http_status: None,
        };
    }

    let queried = match port {
        Some(port) => query_live_player_count_async("a2s_info", host, port).await,
        None => None,
    };
    match queried {
        Some(queried) => PalworldOperatorServiceProbe {
            key: String::from("game"),
            transport: String::from("udp"),
            enabled: true,
            configured: true,
            endpoint,
            reachable: true,
            status: String::from("reachable"),
            detail: format!(
                "UDP live query responded with {}/{} players.",
                queried.current_players, queried.max_players
            ),
            http_status: None,
        },
        None => PalworldOperatorServiceProbe {
            key: String::from("game"),
            transport: String::from("udp"),
            enabled: true,
            configured: true,
            endpoint,
            reachable: false,
            status: String::from("no_query_response"),
            detail: String::from("No live UDP query response has been observed yet."),
            http_status: None,
        },
    }
}

pub(super) async fn build_palworld_operator_snapshot(
    details: &InstanceDetails,
) -> PalworldOperatorSnapshot {
    let settings = parse_settings_json(&details.settings_json);
    let host = resolve_palworld_operator_host(details, &settings);
    let (game, rcon, rest_api) = tokio::join!(
        build_palworld_game_probe(details, &host, &settings),
        build_palworld_rcon_probe(details, &host, &settings),
        build_palworld_rest_probe(details, &host, &settings),
    );
    let services = vec![game, rcon, rest_api];

    PalworldOperatorSnapshot {
        instance_id: details.summary.id.clone(),
        checked_at_unix_ms: now_unix_ms(),
        services,
    }
}

fn resolve_sevendaystodie_operator_host(details: &InstanceDetails, _settings: &Value) -> String {
    normalize_query_host(&details.summary.bind_ip)
}

async fn build_sevendaystodie_game_udp_probe(
    details: &InstanceDetails,
    host: &str,
) -> OperatorServiceProbe {
    let port = find_port_number(details, "udp", &["game_udp"]);
    let endpoint = port.map(|port| format!("{host}:{port}"));

    let Some(port) = port else {
        return OperatorServiceProbe {
            key: String::from("game_udp"),
            transport: String::from("udp"),
            enabled: true,
            configured: false,
            endpoint: None,
            reachable: false,
            status: String::from("not_configured"),
            detail: String::from("No UDP game_udp port is registered for 7 Days to Die."),
            http_status: None,
        };
    };

    if !is_instance_running(details) {
        return OperatorServiceProbe {
            key: String::from("game_udp"),
            transport: String::from("udp"),
            enabled: true,
            configured: true,
            endpoint,
            reachable: false,
            status: String::from("waiting_for_start"),
            detail: String::from("A2S live querying runs only while the 7DTD instance is running."),
            http_status: None,
        };
    }

    match query_live_player_count_async("a2s_info", host, port).await {
        Some(queried) => OperatorServiceProbe {
            key: String::from("game_udp"),
            transport: String::from("udp"),
            enabled: true,
            configured: true,
            endpoint,
            reachable: true,
            status: String::from("reachable"),
            detail: format!(
                "A2S query responded with {}/{} players.",
                queried.current_players, queried.max_players
            ),
            http_status: None,
        },
        None => OperatorServiceProbe {
            key: String::from("game_udp"),
            transport: String::from("udp"),
            enabled: true,
            configured: true,
            endpoint,
            reachable: false,
            status: String::from("no_query_response"),
            detail: String::from(
                "No A2S response has been observed on the 7DTD game UDP port yet.",
            ),
            http_status: None,
        },
    }
}

async fn build_sevendaystodie_game_tcp_probe(
    details: &InstanceDetails,
    host: &str,
) -> OperatorServiceProbe {
    let port = find_port_number(details, "tcp", &["game_tcp"]);
    let endpoint = port.map(|port| format!("{host}:{port}"));

    let Some(port) = port else {
        return OperatorServiceProbe {
            key: String::from("game_tcp"),
            transport: String::from("tcp"),
            enabled: true,
            configured: false,
            endpoint: None,
            reachable: false,
            status: String::from("not_configured"),
            detail: String::from("No TCP game_tcp port is registered for 7 Days to Die."),
            http_status: None,
        };
    };

    if !is_instance_running(details) {
        return OperatorServiceProbe {
            key: String::from("game_tcp"),
            transport: String::from("tcp"),
            enabled: true,
            configured: true,
            endpoint,
            reachable: false,
            status: String::from("waiting_for_start"),
            detail: String::from("TCP listener probing runs after the 7DTD instance starts."),
            http_status: None,
        };
    }

    let tcp_probe = probe_tcp_listener_async(host, port).await;
    OperatorServiceProbe {
        key: String::from("game_tcp"),
        transport: String::from("tcp"),
        enabled: true,
        configured: true,
        endpoint,
        reachable: tcp_probe.reachable,
        status: tcp_probe.status,
        detail: tcp_probe.detail,
        http_status: None,
    }
}

async fn build_sevendaystodie_web_dashboard_probe(
    details: &InstanceDetails,
    host: &str,
    settings: &Value,
) -> OperatorServiceProbe {
    let enabled = json_bool(settings.get("web_dashboard_enabled")).unwrap_or(false);
    let port = find_port_number(details, "tcp", &["web_dashboard"]);
    let endpoint = if enabled {
        port.map(|port| format!("http://{host}:{port}/"))
    } else {
        None
    };

    if !enabled {
        return OperatorServiceProbe {
            key: String::from("web_dashboard"),
            transport: String::from("http"),
            enabled: false,
            configured: port.is_some(),
            endpoint,
            reachable: false,
            status: String::from("disabled"),
            detail: String::from("The 7DTD Web Dashboard is disabled in instance settings."),
            http_status: None,
        };
    }

    let Some(port) = port else {
        return OperatorServiceProbe {
            key: String::from("web_dashboard"),
            transport: String::from("http"),
            enabled: true,
            configured: false,
            endpoint: None,
            reachable: false,
            status: String::from("not_configured"),
            detail: String::from(
                "Web Dashboard is enabled, but no TCP web_dashboard port is registered.",
            ),
            http_status: None,
        };
    };

    let endpoint = Some(format!("http://{host}:{port}/"));
    if !is_instance_running(details) {
        return OperatorServiceProbe {
            key: String::from("web_dashboard"),
            transport: String::from("http"),
            enabled: true,
            configured: true,
            endpoint,
            reachable: false,
            status: String::from("waiting_for_start"),
            detail: String::from("Web Dashboard will be probed after the 7DTD instance starts."),
            http_status: None,
        };
    }

    let tcp_probe = probe_tcp_listener_async(host, port).await;
    if !tcp_probe.reachable {
        return OperatorServiceProbe {
            key: String::from("web_dashboard"),
            transport: String::from("http"),
            enabled: true,
            configured: true,
            endpoint,
            reachable: false,
            status: tcp_probe.status,
            detail: tcp_probe.detail,
            http_status: None,
        };
    }

    let client = match build_operator_http_client(reqwest::Client::builder()) {
        Ok(client) => client,
        Err(error) => {
            return OperatorServiceProbe {
                key: String::from("web_dashboard"),
                transport: String::from("http"),
                enabled: true,
                configured: true,
                endpoint,
                reachable: true,
                status: String::from("http_client_error"),
                detail: format!(
                    "Web Dashboard listener is reachable, but the HTTP client could not be created: {error}"
                ),
                http_status: None,
            };
        }
    };

    let request_url = format!("http://{host}:{port}/");
    match client.get(&request_url).send().await {
        Ok(response) => {
            let status = response.status();
            OperatorServiceProbe {
                key: String::from("web_dashboard"),
                transport: String::from("http"),
                enabled: true,
                configured: true,
                endpoint,
                reachable: true,
                status: String::from("reachable"),
                detail: format!(
                    "HTTP probe returned status {} from `{request_url}`.",
                    status.as_u16()
                ),
                http_status: Some(status.as_u16()),
            }
        }
        Err(error) => {
            let status = if error.is_timeout() {
                "http_timeout"
            } else {
                "http_error"
            };
            OperatorServiceProbe {
                key: String::from("web_dashboard"),
                transport: String::from("http"),
                enabled: true,
                configured: true,
                endpoint,
                reachable: false,
                status: String::from(status),
                detail: format!(
                    "Web Dashboard listener accepted TCP, but HTTP probing `{request_url}` failed: {error}"
                ),
                http_status: None,
            }
        }
    }
}

async fn build_sevendaystodie_telnet_probe(
    details: &InstanceDetails,
    host: &str,
    settings: &Value,
) -> OperatorServiceProbe {
    let enabled = json_bool(settings.get("telnet_enabled")).unwrap_or(true);
    let port = find_port_number(details, "tcp", &["telnet"]);
    let endpoint = if enabled {
        port.map(|port| format!("{host}:{port}"))
    } else {
        None
    };

    if !enabled {
        return OperatorServiceProbe {
            key: String::from("telnet"),
            transport: String::from("tcp"),
            enabled: false,
            configured: port.is_some(),
            endpoint,
            reachable: false,
            status: String::from("disabled"),
            detail: String::from("The 7DTD Telnet service is disabled in instance settings."),
            http_status: None,
        };
    }

    let Some(port) = port else {
        return OperatorServiceProbe {
            key: String::from("telnet"),
            transport: String::from("tcp"),
            enabled: true,
            configured: false,
            endpoint: None,
            reachable: false,
            status: String::from("not_configured"),
            detail: String::from("Telnet is enabled, but no TCP telnet port is registered."),
            http_status: None,
        };
    };

    let endpoint = Some(format!("{host}:{port}"));
    if !is_instance_running(details) {
        return OperatorServiceProbe {
            key: String::from("telnet"),
            transport: String::from("tcp"),
            enabled: true,
            configured: true,
            endpoint,
            reachable: false,
            status: String::from("waiting_for_start"),
            detail: String::from("Telnet listener probing runs after the 7DTD instance starts."),
            http_status: None,
        };
    }

    let tcp_probe = probe_tcp_listener_async(host, port).await;
    OperatorServiceProbe {
        key: String::from("telnet"),
        transport: String::from("tcp"),
        enabled: true,
        configured: true,
        endpoint,
        reachable: tcp_probe.reachable,
        status: tcp_probe.status,
        detail: tcp_probe.detail,
        http_status: None,
    }
}

pub(super) async fn build_sevendaystodie_operator_snapshot(
    details: &InstanceDetails,
) -> SevenDaysOperatorSnapshot {
    let settings = parse_settings_json(&details.settings_json);
    let host = resolve_sevendaystodie_operator_host(details, &settings);
    let (game_udp, game_tcp, web_dashboard, telnet) = tokio::join!(
        build_sevendaystodie_game_udp_probe(details, &host),
        build_sevendaystodie_game_tcp_probe(details, &host),
        build_sevendaystodie_web_dashboard_probe(details, &host, &settings),
        build_sevendaystodie_telnet_probe(details, &host, &settings),
    );
    let services = vec![game_udp, game_tcp, web_dashboard, telnet];

    SevenDaysOperatorSnapshot {
        instance_id: details.summary.id.clone(),
        checked_at_unix_ms: now_unix_ms(),
        operator_host: host,
        services,
    }
}

pub(super) fn resolve_player_query_target(
    details: &InstanceDetails,
    player_query: Option<&ModulePlayerQuerySpec>,
) -> Option<(String, u16)> {
    let player_query = player_query?;
    resolve_player_query_target_from_parts(&details.summary.bind_ip, player_query, &details.ports)
}

pub(super) fn load_module_runtime_capability_map(
    modules_root: &Path,
) -> Result<HashMap<String, ModuleRuntimeCapabilities>, String> {
    Ok(discover_modules(modules_root)
        .map_err(|error| format!("Failed to load module runtime capabilities: {error}"))?
        .into_iter()
        .map(|descriptor| {
            (
                descriptor.summary.id,
                ModuleRuntimeCapabilities {
                    player_query: descriptor.runtime.player_query,
                    player_count_source: descriptor.runtime.player_count_source,
                    performance: descriptor.runtime.performance,
                },
            )
        })
        .collect())
}

pub(super) async fn collect_system_snapshot(
    app: &tauri::AppHandle,
    state: &DesktopState,
    latest: &AppState,
) -> Result<SystemSnapshot, String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    let waiter = state.system_snapshot_refresh.subscribe();
    if let Some(snapshot) = state
        .system_snapshot_cache
        .lock()
        .map_err(|_| String::from("system snapshot cache lock poisoned"))?
        .fresh(SYSTEM_SNAPSHOT_CACHE_TTL)
    {
        return Ok(with_current_running_instances(snapshot, latest));
    }

    let (generation, start_refresh) = {
        let mut cache = state
            .system_snapshot_cache
            .lock()
            .map_err(|_| String::from("system snapshot cache lock poisoned"))?;
        if let Some(snapshot) = cache.fresh(SYSTEM_SNAPSHOT_CACHE_TTL) {
            return Ok(with_current_running_instances(snapshot, latest));
        }

        if cache.try_begin_refresh() {
            (state.system_snapshot_refresh.begin(), true)
        } else {
            (state.system_snapshot_refresh.current_generation(), false)
        }
    };

    if start_refresh {
        let host_monitor = Arc::clone(&state.host_monitor);
        let settings = latest.settings.clone();
        let instances = latest.instances.clone();
        let app = app.clone();
        let refresh = crate::state::spawn_timed_cache_refresh(
            Arc::clone(&state.system_snapshot_cache),
            "system snapshot",
            async move {
                collect_system_snapshot_uncached(app, host_monitor, settings, instances)
                    .await
                    .inspect_err(|error| {
                        if let Ok(storage) = bootstrap_storage() {
                            append_desktop_app_log(
                                &storage,
                                "warning",
                                "system.snapshot.refresh_failed",
                                "Failed to refresh system metrics",
                                json!({ "error": error }),
                            );
                        } else {
                            eprintln!("Failed to refresh system metrics: {error}");
                        }
                    })
            },
        );
        state.system_snapshot_refresh.track(generation, refresh);
    }

    // Joiners wait for the same refresh instead of returning the old sample
    // immediately. A deadline drops only this waiter, never the cache worker.
    let result = waiter.wait(generation, deadline).await;
    if let Err(error) = &result
        && error.is_worker_failure()
    {
        return Err(error.to_string());
    }
    let cached = state
        .system_snapshot_cache
        .lock()
        .map_err(|_| String::from("system snapshot cache lock poisoned"))?
        .latest();
    if let Some(snapshot) = cached {
        return Ok(with_current_running_instances(snapshot, latest));
    }
    result.map_err(|error| error.to_string())?;
    Err(String::from(
        "system snapshot refresh completed without a sample",
    ))
}

async fn collect_system_snapshot_uncached(
    app: tauri::AppHandle,
    host_monitor: Arc<StdMutex<WindowsHostMonitor>>,
    settings: AppSettings,
    instances: Vec<InstanceSummary>,
) -> Result<SystemSnapshot, String> {
    let monitored_path = preferred_snapshot_path(&settings).to_string();
    let storage_paths = system_storage_paths::monitored_storage_paths(&settings).await?;
    let host_snapshot_task = tauri::async_runtime::spawn_blocking(move || {
        host_monitor
            .lock()
            .map_err(|_| String::from("host monitor lock poisoned"))
            .map(|mut monitor| monitor.capture_paths(&monitored_path, &storage_paths))
    });
    let instance_process_memory_task = collect_instance_process_memory();
    let player_counts_task = collect_global_player_counts(&app, &instances);

    let (host_snapshot, instance_process_memory, player_counts) = tokio::join!(
        host_snapshot_task,
        instance_process_memory_task,
        player_counts_task
    );
    let host_snapshot = host_snapshot.map_err(|error| error.to_string())??;
    let player_counts = player_counts?;
    let instance_process_memory =
        with_instance_memory_percent(instance_process_memory, host_snapshot.memory_total_bytes);
    let running_instances = count_running_instances(&instances);

    Ok(build_system_snapshot(
        host_snapshot,
        instance_process_memory,
        running_instances,
        player_counts,
    ))
}

pub(super) fn lightweight_system_snapshot(
    latest: &AppState,
    cached: Option<SystemSnapshot>,
) -> SystemSnapshot {
    // Metadata reloads must retain the actual observation, including its age.
    // Host collection owns the separate cache; app_state holds only defaults.
    with_current_running_instances(cached.unwrap_or_else(|| latest.snapshot.clone()), latest)
}

fn with_current_running_instances(
    mut snapshot: SystemSnapshot,
    latest: &AppState,
) -> SystemSnapshot {
    let running_instances = count_running_instances(&latest.instances);
    snapshot.running_instances = running_instances;
    snapshot
}

fn build_system_snapshot(
    host_snapshot: HostMetricsSnapshot,
    instance_process_memory: InstanceProcessMemorySnapshot,
    running_instances: usize,
    player_counts: GlobalPlayerCountSnapshot,
) -> SystemSnapshot {
    SystemSnapshot {
        telemetry: host_snapshot.telemetry,
        disk_volumes: host_snapshot.disk_volumes,
        memory_commit_used_bytes: host_snapshot.memory_commit_used_bytes,
        memory_commit_limit_bytes: host_snapshot.memory_commit_limit_bytes,
        cpu_percent: host_snapshot.cpu_percent,
        cpu_name: host_snapshot.cpu_name,
        cpu_frequency_mhz: host_snapshot.cpu_frequency_mhz,
        cpu_max_frequency_mhz: host_snapshot.cpu_max_frequency_mhz,
        cpu_physical_cores: host_snapshot.cpu_physical_cores,
        cpu_logical_cores: host_snapshot.cpu_logical_cores,
        cpu_single_core_peak_percent: host_snapshot.cpu_single_core_peak_percent,
        cpu_performance_percent: host_snapshot.cpu_performance_percent,
        cpu_cores: host_snapshot
            .cpu_cores
            .into_iter()
            .map(|core| CpuCoreSnapshot {
                name: core.name,
                utility_percent: core.utility_percent,
                performance_percent: core.performance_percent,
                frequency_mhz: core.frequency_mhz,
            })
            .collect(),
        memory_percent: host_snapshot.memory_percent,
        memory_total_bytes: host_snapshot.memory_total_bytes,
        memory_available_bytes: host_snapshot.memory_available_bytes,
        memory_modules: host_snapshot
            .memory_modules
            .into_iter()
            .map(|module| MemoryModuleSnapshot {
                bank_label: module.bank_label,
                device_locator: module.device_locator,
                manufacturer: module.manufacturer,
                part_number: module.part_number,
                capacity_bytes: module.capacity_bytes,
                speed_mts: module.speed_mts,
                configured_clock_mts: module.configured_clock_mts,
                configured_voltage_mv: module.configured_voltage_mv,
                memory_type: module.memory_type,
                inferred_cas_latency: module.inferred_cas_latency,
                timing_summary: module.timing_summary,
            })
            .collect(),
        disk_used_percent: host_snapshot.disk_used_percent,
        disk_used_bytes: host_snapshot.disk_used_bytes,
        disk_total_bytes: host_snapshot.disk_total_bytes,
        disk_label: host_snapshot.disk_label,
        disk_volume_id: host_snapshot.disk_volume_id,
        disk_model: host_snapshot.disk_model,
        disk_volume_name: host_snapshot.disk_volume_name,
        disk_file_system: host_snapshot.disk_file_system,
        disk_read_bps: host_snapshot.disk_read_bps,
        disk_write_bps: host_snapshot.disk_write_bps,
        disk_read_latency_ms: host_snapshot.disk_read_latency_ms,
        disk_write_latency_ms: host_snapshot.disk_write_latency_ms,
        disk_queue_length: host_snapshot.disk_queue_length,
        network_receive_bps: host_snapshot.network_receive_bps,
        network_transmit_bps: host_snapshot.network_transmit_bps,
        network_adapters: host_snapshot
            .network_adapters
            .into_iter()
            .map(|adapter| NetworkAdapterSnapshot {
                rate_status: adapter.rate_status,
                name: adapter.name,
                description: adapter.description,
                status: adapter.status,
                family_name: adapter.family_name,
                ipv4_addresses: adapter.ipv4_addresses,
                mac_address: adapter.mac_address,
                link_speed_bps: adapter.link_speed_bps,
                received_bytes: adapter.received_bytes,
                transmitted_bytes: adapter.transmitted_bytes,
                receive_bps: adapter.receive_bps,
                transmit_bps: adapter.transmit_bps,
            })
            .collect(),
        instance_process_memory_bytes: instance_process_memory.memory_bytes,
        instance_process_memory_percent: instance_process_memory.memory_percent,
        instance_process_count: instance_process_memory.process_count,
        instance_process_threads: instance_process_memory.thread_count,
        instance_process_handles: instance_process_memory.handle_count,
        running_instances,
        total_online_players: player_counts.total_online_players,
        total_player_capacity: player_counts.total_player_capacity,
        player_count_queried_instances: player_counts.queried_instances,
        player_count_queryable_instances: player_counts.queryable_instances,
    }
}

pub(super) fn count_running_instances(instances: &[InstanceSummary]) -> usize {
    instances
        .iter()
        .filter(|instance| matches!(instance.status, InstanceStatus::Running))
        .count()
}

#[cfg(test)]
#[path = "commands_operator_http_tests.rs"]
mod operator_http_tests;

#[cfg(test)]
#[path = "commands_system_snapshot_tests.rs"]
mod system_snapshot_tests;
