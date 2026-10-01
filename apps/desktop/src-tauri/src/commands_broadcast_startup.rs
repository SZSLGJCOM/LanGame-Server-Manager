use super::*;
use std::future::Future;
use std::net::SocketAddr;

const STARTUP_CHANNEL_POLL_INTERVAL: Duration = Duration::from_millis(100);
const STARTUP_CHANNEL_CONNECT_TIMEOUT: Duration = Duration::from_millis(250);

pub(super) fn should_wait_for_broadcast_channel(source: &str, initiator: &str) -> bool {
    source == "startup" && initiator != "manual"
}

pub(super) async fn dispatch_startup_broadcast(
    state: &DesktopState,
    details: &InstanceDetails,
    descriptor: &ModuleDescriptor,
    request: &RuntimeTransportRequest<'_>,
) -> Result<(), String> {
    let endpoint = startup_broadcast_tcp_endpoint(details, request)?;
    let run = details.active_run.as_ref().ok_or_else(|| {
        String::from("Startup broadcast cancelled because the instance is no longer running.")
    })?;
    // Use the module startup budget, including its existing 1–300 second
    // bounds; unsupported bind policies do not pass through that validation.
    let timeout = Duration::from_millis(
        descriptor
            .runtime
            .bind_address
            .startup_timeout_ms
            .clamp(1_000, 300_000),
    );
    let check_current_run = || ensure_startup_broadcast_run(state, &details.summary.id, run);
    let _instance_lock = await_startup_broadcast_readiness(timeout, check_current_run, async {
        if let Some(endpoint) = endpoint {
            wait_for_startup_broadcast_listener(endpoint).await?;
        }
        // Do not hold the mutation lock while waiting for the listener. A stop
        // must be able to finish, and its run must never receive this broadcast.
        Ok(state.acquire_instance_mutation(&details.summary.id).await)
    })
    .await?;

    // Connection attempts above send no application bytes. Once ready, submit
    // through the normal transport exactly once, even if its response is lost.
    dispatch_instance_runtime_transport(state, details, request).await
}

fn ensure_startup_broadcast_run(
    state: &DesktopState,
    instance_id: &str,
    expected_run: &ActiveInstanceRun,
) -> Result<(), String> {
    if state.shutdown_in_progress.load(Ordering::SeqCst) {
        return Err(String::from(
            "Startup broadcast cancelled during application shutdown.",
        ));
    }
    let runtime = state
        .runtime_supervisor
        .lock()
        .map_err(|_| String::from("runtime supervisor lock poisoned"))?;
    let current = runtime
        .tracked_instances()
        .into_iter()
        .find(|instance| instance.summary.id == instance_id);
    drop(runtime);
    let current_pid = current
        .filter(|run| run.run_id == expected_run.run_id && run.pid == expected_run.pid)
        .and_then(|run| run.pid);
    let running = match current_pid {
        Some(pid) => process_is_running(pid).map_err(|error| error.to_string())?,
        None => false,
    };
    if !running {
        return Err(String::from(
            "Startup broadcast cancelled because the instance stopped or its server run changed.",
        ));
    }
    Ok(())
}

fn startup_broadcast_tcp_endpoint(
    details: &InstanceDetails,
    request: &RuntimeTransportRequest<'_>,
) -> Result<Option<SocketAddr>, String> {
    let transport = request.transport.to_ascii_lowercase();
    if transport == "palworld_rest" {
        return crate::live_players::palworld_rest::resolve_endpoint(details)
            .map(|endpoint| Some(endpoint.address))
            .map_err(|error| error.summary.to_owned());
    }
    let default_port = match transport.as_str() {
        "source_rcon" | "websocket_rcon" => "rcon",
        "telnet" => "telnet",
        _ => return Ok(None),
    };
    // Invalid or disabled configuration should fail immediately instead of
    // spending the startup budget waiting on a listener that cannot appear.
    let settings: Value = serde_json::from_str(&details.settings_json)
        .map_err(|error| format!("failed to parse broadcast transport settings: {error}"))?;
    let settings = settings.as_object().ok_or_else(|| {
        String::from("instance settings must be a JSON object before sending broadcasts")
    })?;
    if let Some(key) = nonempty_key(request.enabled_setting_key) {
        let enabled = json_bool(settings.get(key)).unwrap_or(transport == "telnet");
        if !enabled {
            return Err(format!(
                "Broadcast transport is disabled by setting `{key}`."
            ));
        }
    }
    if transport != "telnet" {
        let key = nonempty_key(request.password_setting_key).unwrap_or("rcon_password");
        if settings
            .get(key)
            .and_then(Value::as_str)
            .is_none_or(|value| value.trim().is_empty())
        {
            return Err(format!("RCON password setting `{key}` is empty"));
        }
    }
    let port_name = nonempty_key(request.port_name).unwrap_or(default_port);
    let port = details
        .ports
        .iter()
        .find(|port| port.name.eq_ignore_ascii_case(port_name))
        .ok_or_else(|| format!("instance has no `{port_name}` port for broadcast transport"))?;
    if !port.protocol.eq_ignore_ascii_case("tcp") {
        return Err(format!(
            "Broadcast transport port `{port_name}` must be TCP"
        ));
    }
    let host = normalize_query_host(&details.summary.bind_ip)
        .parse::<IpAddr>()
        .map_err(|error| format!("invalid broadcast transport address: {error}"))?;
    Ok(Some(SocketAddr::new(host, port.port)))
}

fn nonempty_key(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

async fn await_startup_broadcast_readiness<T>(
    timeout: Duration,
    mut check_current_run: impl FnMut() -> Result<(), String>,
    readiness: impl Future<Output = Result<T, String>>,
) -> Result<T, String> {
    tokio::time::timeout(timeout, async {
        tokio::pin!(readiness);
        let mut poll = tokio::time::interval(STARTUP_CHANNEL_POLL_INTERVAL);
        loop {
            check_current_run()?;
            tokio::select! {
                result = &mut readiness => {
                    check_current_run()?;
                    return result;
                }
                _ = poll.tick() => {}
            }
        }
    })
    .await
    .map_err(|_| {
        format!(
            "Startup broadcast channel did not become ready within {} ms.",
            timeout.as_millis()
        )
    })?
}

async fn wait_for_startup_broadcast_listener(endpoint: SocketAddr) -> Result<(), String> {
    loop {
        let connection = tokio::task::spawn_blocking(move || {
            TcpStream::connect_timeout(&endpoint, STARTUP_CHANNEL_CONNECT_TIMEOUT)
        })
        .await
        .map_err(|error| format!("broadcast readiness task failed: {error}"))?;
        match connection {
            Ok(stream) => {
                drop(stream);
                return Ok(());
            }
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::ConnectionRefused
                        | ErrorKind::TimedOut
                        | ErrorKind::WouldBlock
                        | ErrorKind::Interrupted
                ) => {}
            Err(error) => {
                return Err(format!(
                    "failed to connect to broadcast channel `{endpoint}`: {error}"
                ));
            }
        }
        tokio::time::sleep(STARTUP_CHANNEL_POLL_INTERVAL).await;
    }
}

#[cfg(test)]
#[path = "commands_broadcast_startup_tests.rs"]
mod tests;
