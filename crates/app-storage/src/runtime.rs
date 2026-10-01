use super::*;
use crate::instances::effective_instance_install_root;
use crate::save_paths::load_module_descriptor;
use crate::storage_db::{
    connect_pool, fetch_instance_record, load_instance_ports, map_instance_summary,
};
use app_core::ModulePlayerQuerySpec;

#[path = "runtime_ark_readiness.rs"]
mod ark_readiness;
use ark_readiness::analyze_ark_evolved_runtime_health;
#[path = "runtime_game_log.rs"]
mod game_log;
pub use game_log::{
    GameLogDocument, GameLogSnapshot, read_game_log_snapshot, read_instance_game_log_document,
};
#[path = "runtime_console_health_evidence.rs"]
mod console_health_evidence;
#[path = "runtime_enshrouded_health.rs"]
mod enshrouded_health;
#[path = "runtime_health_evidence.rs"]
mod health_evidence;
#[path = "runtime_nightingale_health.rs"]
mod nightingale_health;
#[path = "runtime_sonsoftheforest_health.rs"]
mod sonsoftheforest_health;
#[path = "runtime_soulmask_health.rs"]
mod soulmask_health;
#[path = "runtime_squad_health.rs"]
mod squad_health;
#[path = "runtime_theforest_health.rs"]
mod theforest_health;
#[path = "runtime_unturned_health.rs"]
mod unturned_health;
#[path = "runtime_windrose_health.rs"]
mod windrose_health;
pub use windrose_health::{WindroseNativeStage, windrose_native_stage};
#[path = "runtime_returntomoria_health.rs"]
mod returntomoria_health;
use std::io::{Read, Seek, SeekFrom};
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
#[cfg(windows)]
use std::process::Command;
use std::time::UNIX_EPOCH;

const RUNTIME_HEALTH_SCAN_LINE_LIMIT: usize = 160;
const RUNTIME_STARTUP_SCAN_LINE_LIMIT: usize = 4096;
const RUNTIME_STARTUP_SCAN_BYTE_LIMIT: u64 = 1024 * 1024;
const LOG_TAIL_MIN_READ_BYTES: u64 = 64 * 1024;
const LOG_TAIL_MAX_READ_BYTES: u64 = 1024 * 1024;
const DST_WRAPPER_LOG_DIRECTORY_ENTRY_LIMIT: usize = 4096;
const PLAYER_QUERY_TIMEOUT_MS: u64 = 350;
const ARK_EVOLVED_REQUIRED_UDP_PORT_NAMES: [&str; 2] = ["game", "query"];

pub async fn read_active_instance_run(
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<Option<ActiveInstanceRun>, StorageError> {
    let pool = connect_pool(paths).await?;
    let run = load_active_instance_run(&pool, instance_id).await?;
    pool.close().await;
    Ok(run)
}

pub async fn list_active_instance_runs(
    paths: &StoragePaths,
) -> Result<Vec<ActiveInstanceRunEntry>, StorageError> {
    let pool = connect_pool(paths).await?;

    let rows = sqlx::query(
        r#"
        SELECT instance_id, id, session_id,
               COALESCE(process_key, 'main') AS process_key,
               COALESCE(display_name, process_key, 'Server') AS display_name,
               is_primary, pid, process_creation_time, process_image_path, log_path
        FROM instance_runs
        WHERE status = 'running'
        ORDER BY id DESC
        "#,
    )
    .fetch_all(&pool)
    .await?;

    pool.close().await;
    Ok(rows.iter().map(map_active_instance_run_entry).collect())
}

pub async fn read_instance_runtime_overview(
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<InstanceRuntimeOverview, StorageError> {
    let pool = connect_pool(paths).await?;

    let record = fetch_instance_record(&pool, instance_id).await?;

    let active_run = load_active_instance_run(&pool, instance_id).await?;
    let recent_runs = load_recent_instance_runs(&pool, instance_id, 8).await?;
    let ports = load_instance_ports(&pool, instance_id).await?;
    let player_query = load_module_descriptor(paths, &record.summary.module_id)
        .ok()
        .flatten()
        .and_then(|descriptor| descriptor.runtime.player_query);
    let log_source_path =
        resolve_instance_log_source_path(&record, active_run.as_ref(), &recent_runs).await?;
    let module_id = record.summary.module_id.clone();
    let status = record.summary.status.clone();
    let health_run = active_run.clone();
    let health_ports = ports.clone();
    let config_file_path = record.config_dir.join("instance.json");
    let status_health_root = if module_id == "returntomoria" {
        Some(effective_instance_install_root(&record)?)
    } else {
        None
    };
    let ark_health_instance = if app_core::ark_maps::is_ark(&module_id) {
        Some(crate::instances::read_instance_details(paths, instance_id).await?)
    } else {
        None
    };
    let (health_scan, log_tail, health, query_restriction) =
        tokio::task::spawn_blocking(move || {
            let query_restriction =
                if matches!(module_id.as_str(), "valheim" | "vrising" | "abioticfactor") {
                    let settings =
                        crate::instances::read_instance_settings_json(&config_file_path)?;
                    steam_player_query_visibility_restriction(&module_id, &settings)
                } else {
                    None
                };
            let health_scan =
                read_log_snapshot(log_source_path.clone(), RUNTIME_HEALTH_SCAN_LINE_LIMIT);
            let log_tail = read_log_snapshot(log_source_path, 32);
            let health = ark_health_instance
                .as_ref()
                .and_then(ark_readiness::analyze_ark_maps_runtime_health)
                .unwrap_or_else(|| {
                    analyze_runtime_health_with_startup_evidence(
                        &module_id,
                        &status,
                        &health_scan,
                        health_run.as_ref(),
                        &health_ports,
                    )
                });
            let health = if matches!(health.status.as_str(), "starting" | "ready") {
                status_health_root
                    .as_ref()
                    .and_then(|root| {
                        returntomoria_health::analyze(&status, root, health_run.as_ref())
                    })
                    .unwrap_or(health)
            } else {
                health
            };
            Ok::<_, StorageError>((health_scan, log_tail, health, query_restriction))
        })
        .await
        .map_err(|error| StorageError::BlockingTaskFailed {
            operation: "read runtime health logs",
            message: error.to_string(),
        })??;
    let diagnostics = collect_runtime_diagnostics(
        &record.summary.module_id,
        &record.summary.status,
        &health_scan,
        &health,
        &record.saves_dir,
        active_run.as_ref(),
    );

    pool.close().await;

    let players = if let Some((_, summary)) = query_restriction {
        app_core::RuntimePlayerSnapshot {
            current_players: None,
            max_players: None,
            query: runtime_player_query_state("unsupported", summary),
        }
    } else {
        build_instance_player_snapshot(
            &record.summary.bind_ip,
            &record.summary.status,
            &ports,
            player_query.as_ref(),
        )
        .await
    };

    Ok(InstanceRuntimeOverview {
        stability: build_runtime_stability_snapshot(&recent_runs),
        recent_runs,
        log_tail,
        health,
        diagnostics,
        players,
        performance: app_core::RuntimePerformanceSnapshot::default(),
        startup_queue: app_core::RuntimeStartupQueueSnapshot::default(),
    })
}

async fn build_instance_player_snapshot(
    bind_ip: &str,
    status: &InstanceStatus,
    ports: &[PortBinding],
    player_query: Option<&ModulePlayerQuerySpec>,
) -> app_core::RuntimePlayerSnapshot {
    let mut current_players = None;
    let mut max_players = None;

    let query = match player_query {
        Some(player_query) if supports_live_player_query_protocol(&player_query.protocol) => {
            match resolve_player_query_target(bind_ip, player_query, ports) {
                None => runtime_player_query_state(
                    "misconfigured",
                    format!(
                        "Live player query is declared, but the instance is missing {}.",
                        describe_expected_player_query_binding(player_query)
                    ),
                ),
                Some((_host, _port)) if !matches!(status, InstanceStatus::Running) => {
                    runtime_player_query_state(
                        "stopped",
                        "Live player query is available after the instance starts.",
                    )
                }
                Some((host, port)) => {
                    let protocol = player_query.protocol.clone();
                    let query_host = host.clone();
                    let queried = tokio::task::spawn_blocking(move || {
                        query_live_player_count(&protocol, &query_host, port)
                    })
                    .await
                    .ok()
                    .flatten();
                    if let Some(queried) = queried {
                        current_players = Some(queried.current_players);
                        max_players = (queried.max_players > 0).then_some(queried.max_players);
                        runtime_player_query_state(
                            "ready",
                            "Live player query succeeded and the current player count is up to date.",
                        )
                    } else {
                        runtime_player_query_state(
                            "failed",
                            format!(
                                "Live player query is configured for {host}:{port}, but the latest probe returned no data."
                            ),
                        )
                    }
                }
            }
        }
        _ => runtime_player_query_state(
            "unsupported",
            "This module does not expose a live player query protocol.",
        ),
    };

    app_core::RuntimePlayerSnapshot {
        current_players,
        max_players,
        query,
    }
}

fn runtime_player_query_state(
    status: &str,
    summary: impl Into<String>,
) -> app_core::RuntimePlayerQueryState {
    app_core::RuntimePlayerQueryState {
        status: String::from(status),
        summary: summary.into(),
    }
}

pub fn steam_player_query_visibility_restriction(
    module_id: &str,
    settings_json: &str,
) -> Option<(&'static str, &'static str)> {
    if !matches!(module_id, "valheim" | "vrising" | "abioticfactor") {
        return None;
    }
    let settings: Value = serde_json::from_str(settings_json).ok()?;
    match module_id {
        "valheim" if settings["public_server"].as_u64() == Some(0) => Some((
            "public_server",
            "Valheim private servers do not expose the Steam player query. Public visibility is required for this query.",
        )),
        "vrising" if settings["list_on_steam"].as_bool() == Some(false) => Some((
            "list_on_steam",
            "V Rising does not expose Steam player queries while List On Steam is disabled. Direct game connections remain available.",
        )),
        "abioticfactor" if settings["lan_only"].as_bool() == Some(true) => Some((
            "lan_only",
            "Abiotic Factor does not expose Steam player queries in LAN Only mode. LAN game discovery remains available.",
        )),
        _ => None,
    }
}

pub fn describe_expected_player_query_binding(player_query: &ModulePlayerQuerySpec) -> String {
    let udp_port_names = player_query
        .port_names
        .iter()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();

    match udp_port_names.as_slice() {
        [] => String::from("a matching UDP query port binding"),
        [port_name] => format!("a UDP port binding named `{port_name}`"),
        _ => format!(
            "one of the UDP port bindings `{}`",
            udp_port_names.join("`, `")
        ),
    }
}

pub fn normalize_query_host(bind_ip: &str) -> String {
    match bind_ip.trim() {
        "" | "0.0.0.0" | "::" | "[::]" => String::from("127.0.0.1"),
        value => String::from(value),
    }
}

pub fn resolve_player_query_target(
    bind_ip: &str,
    player_query: &ModulePlayerQuerySpec,
    ports: &[PortBinding],
) -> Option<(String, u16)> {
    if !supports_live_player_query_protocol(&player_query.protocol) {
        return None;
    }

    for port_name in &player_query.port_names {
        if let Some(port) = ports.iter().find(|port| {
            port.protocol.eq_ignore_ascii_case("udp") && port.name.eq_ignore_ascii_case(port_name)
        }) {
            return Some((normalize_query_host(bind_ip), port.port));
        }
    }

    None
}

pub fn supports_live_player_query_protocol(protocol: &str) -> bool {
    protocol.eq_ignore_ascii_case("a2s_info") || protocol.eq_ignore_ascii_case("minecraft_query")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueriedPlayerCount {
    pub current_players: usize,
    pub max_players: usize,
}

pub fn query_live_player_count(
    protocol: &str,
    host: &str,
    port: u16,
) -> Option<QueriedPlayerCount> {
    let addresses = format!("{host}:{port}").to_socket_addrs().ok()?;
    let mut ipv6_fallback = None;

    for address in addresses {
        if address.is_ipv4() {
            if let Some(queried) = query_live_player_count_at(protocol, address) {
                return Some(queried);
            }
        } else if ipv6_fallback.is_none() {
            ipv6_fallback = Some(address);
        }
    }

    if let Some(address) = ipv6_fallback {
        return query_live_player_count_at(protocol, address);
    }

    None
}

fn query_live_player_count_at(protocol: &str, address: SocketAddr) -> Option<QueriedPlayerCount> {
    if protocol.eq_ignore_ascii_case("minecraft_query") {
        return query_minecraft_player_count_at(address);
    }
    query_a2s_player_count(address).ok()
}

pub fn query_a2s_player_count(
    address: SocketAddr,
) -> Result<QueriedPlayerCount, crate::A2sQueryError> {
    let client = crate::A2sClient::connect(
        address,
        std::time::Duration::from_millis(PLAYER_QUERY_TIMEOUT_MS * 2),
    )?;
    let packet = client.info()?;
    parse_a2s_info_payload(&packet[5..]).ok_or(crate::A2sQueryError::InvalidResponse(
        "incomplete INFO payload",
    ))
}

fn query_minecraft_player_count_at(address: SocketAddr) -> Option<QueriedPlayerCount> {
    const SESSION_ID: [u8; 4] = *b"LSGM";

    let socket = match address {
        SocketAddr::V4(_) => UdpSocket::bind("0.0.0.0:0").ok()?,
        SocketAddr::V6(_) => UdpSocket::bind("[::]:0").ok()?,
    };
    socket
        .set_read_timeout(Some(std::time::Duration::from_millis(
            PLAYER_QUERY_TIMEOUT_MS,
        )))
        .ok()?;
    socket
        .set_write_timeout(Some(std::time::Duration::from_millis(
            PLAYER_QUERY_TIMEOUT_MS,
        )))
        .ok()?;
    socket.connect(address).ok()?;

    let mut buffer = [0_u8; 1400];
    let mut challenge_packet = Vec::from([0xfe, 0xfd, 0x09]);
    challenge_packet.extend_from_slice(&SESSION_ID);
    socket.send(&challenge_packet).ok()?;
    let received = socket.recv(&mut buffer).ok()?;
    let challenge = parse_minecraft_query_challenge_response(&buffer[..received], SESSION_ID)?;

    let mut stat_packet = Vec::from([0xfe, 0xfd, 0x00]);
    stat_packet.extend_from_slice(&SESSION_ID);
    stat_packet.extend_from_slice(&challenge.to_be_bytes());
    socket.send(&stat_packet).ok()?;
    let received = socket.recv(&mut buffer).ok()?;
    parse_minecraft_query_basic_response(&buffer[..received], SESSION_ID)
}

pub fn parse_a2s_info_payload(payload: &[u8]) -> Option<QueriedPlayerCount> {
    if payload.is_empty() {
        return None;
    }

    let mut index = 1;
    skip_cstring(payload, &mut index)?;
    skip_cstring(payload, &mut index)?;
    skip_cstring(payload, &mut index)?;
    skip_cstring(payload, &mut index)?;

    if index + 4 > payload.len() {
        return None;
    }

    index += 2;
    Some(QueriedPlayerCount {
        current_players: payload[index] as usize,
        max_players: payload[index + 1] as usize,
    })
}

pub fn parse_minecraft_query_challenge_response(packet: &[u8], session_id: [u8; 4]) -> Option<i32> {
    if packet.len() < 6 || packet[0] != 0x09 || packet[1..5] != session_id {
        return None;
    }

    let mut index = 5;
    read_cstring(packet, &mut index)?.trim().parse::<i32>().ok()
}

pub fn parse_minecraft_query_basic_response(
    packet: &[u8],
    session_id: [u8; 4],
) -> Option<QueriedPlayerCount> {
    if packet.len() < 6 || packet[0] != 0x00 || packet[1..5] != session_id {
        return None;
    }

    let mut index = 5;
    read_cstring(packet, &mut index)?;
    read_cstring(packet, &mut index)?;
    read_cstring(packet, &mut index)?;
    let current_players = read_cstring(packet, &mut index)?
        .trim()
        .parse::<usize>()
        .ok()?;
    let max_players = read_cstring(packet, &mut index)?
        .trim()
        .parse::<usize>()
        .ok()?;

    Some(QueriedPlayerCount {
        current_players,
        max_players,
    })
}

fn read_cstring<'a>(payload: &'a [u8], index: &mut usize) -> Option<&'a str> {
    let start = *index;
    while *index < payload.len() {
        let ch = payload[*index];
        if ch == 0x00 {
            let value = std::str::from_utf8(&payload[start..*index]).ok()?;
            *index += 1;
            return Some(value);
        }
        *index += 1;
    }

    None
}

fn skip_cstring(payload: &[u8], index: &mut usize) -> Option<()> {
    while *index < payload.len() {
        let ch = payload[*index];
        *index += 1;
        if ch == 0x00 {
            return Some(());
        }
    }

    None
}

fn build_runtime_stability_snapshot(
    recent_runs: &[InstanceRunRecord],
) -> app_core::RuntimeStabilitySnapshot {
    let recent_crash_count = recent_runs
        .iter()
        .filter(|run| run.crash_flag || run.exit_code.is_some_and(|code| code != 0))
        .count();
    let last_exit_code = recent_runs.iter().find_map(|run| run.exit_code);
    let status = if recent_crash_count > 0 {
        String::from("warning")
    } else if recent_runs
        .first()
        .map(|run| run.status.eq_ignore_ascii_case("running"))
        .unwrap_or(false)
    {
        String::from("running")
    } else {
        String::from("stable")
    };
    let summary = if recent_crash_count > 0 {
        format!(
            "{recent_crash_count} recent run(s) ended with a crash signal or non-zero exit code."
        )
    } else if let Some(code) = last_exit_code {
        format!("Recent runs look stable; latest recorded exit code was {code}.")
    } else {
        String::from("No recent crash signals were found.")
    };

    app_core::RuntimeStabilitySnapshot {
        status,
        summary,
        recent_crash_count,
        last_exit_code,
        restart_policy_enabled: false,
        restart_limit: 0,
        restart_backoff_ms: 0,
        pending_restart: None,
    }
}

fn runtime_health_reason(code: &str, params: &[(&str, String)]) -> app_core::RuntimeHealthReason {
    app_core::RuntimeHealthReason {
        code: String::from(code),
        params: params
            .iter()
            .map(|(key, value)| (String::from(*key), value.clone()))
            .collect(),
    }
}

fn analyze_runtime_health_with_startup_evidence(
    module_id: &str,
    status: &InstanceStatus,
    recent_log: &LogTailSnapshot,
    active_run: Option<&ActiveInstanceRun>,
    ports: &[PortBinding],
) -> app_core::RuntimeHealth {
    let recent_health = analyze_runtime_health(module_id, status, recent_log, active_run, ports);
    if matches!(
        module_id,
        "enshrouded"
            | "unturned"
            | "squad"
            | "soulmask"
            | "theforest"
            | "sonsoftheforest"
            | "nightingale"
            | "windrose"
    ) && matches!(status, InstanceStatus::Running | InstanceStatus::Starting)
        && recent_health.status != "error"
        && let Some(run) = active_run
        && let Some(path) = run.log_path.as_deref()
        && recent_log.source_path.as_deref() == Some(path)
    {
        // Session transitions can revoke readiness after startup. A bounded
        // head scan alone cannot establish their latest state in a long run.
        let mut evidence = LogTailSnapshot {
            source_path: recent_log.source_path.clone(),
            lines: Vec::new(),
            total_lines: 0,
            truncated: false,
            read_error: None,
        };
        let observed = if module_id == "enshrouded" {
            enshrouded_health::observe(Path::new(path), run.run_id)
                .map(|line| line.into_iter().collect())
        } else {
            let kind = match module_id {
                "squad" => console_health_evidence::SessionKind::Squad,
                "soulmask" => console_health_evidence::SessionKind::Soulmask,
                "theforest" => console_health_evidence::SessionKind::TheForest,
                "sonsoftheforest" => console_health_evidence::SessionKind::SonsOfTheForest,
                "nightingale" => console_health_evidence::SessionKind::Nightingale,
                "windrose" => console_health_evidence::SessionKind::Windrose,
                _ => console_health_evidence::SessionKind::Unturned,
            };
            console_health_evidence::observe(kind, Path::new(path), run.run_id)
        };
        match observed {
            Ok(lines) => {
                evidence.total_lines = lines.len();
                evidence.lines = lines;
            }
            Err(error) => evidence.read_error = Some(error.to_string()),
        }
        return analyze_runtime_health(module_id, status, &evidence, active_run, ports);
    }
    if !matches!(status, InstanceStatus::Running | InstanceStatus::Starting)
        || recent_health.matched_line.is_some()
        || !matches!(
            recent_health.reason.code.as_str(),
            "starting_waiting_logs" | "starting_tasks"
        )
        || !recent_log.truncated
    {
        return recent_health;
    }

    // Process logs belong to one recorded run. Shared native log files may still
    // contain an earlier run, so their startup output cannot establish readiness.
    let Some(run_log_path) = active_run.and_then(|run| run.log_path.as_deref()) else {
        return recent_health;
    };
    if recent_log.source_path.as_deref() != Some(run_log_path) {
        return recent_health;
    }
    let startup_log = read_startup_log_snapshot(run_log_path);
    let startup_health = analyze_runtime_health(module_id, status, &startup_log, active_run, ports);
    if startup_health.status == "ready" || startup_log.read_error.is_some() {
        startup_health
    } else {
        recent_health
    }
}

fn read_startup_log_snapshot(source_path: &str) -> LogTailSnapshot {
    let mut snapshot = LogTailSnapshot {
        source_path: Some(String::from(source_path)),
        lines: Vec::new(),
        total_lines: 0,
        truncated: false,
        read_error: None,
    };
    let result = (|| {
        let mut bytes = Vec::new();
        let file = match crate::managed_console_log::open_log_segments(Path::new(source_path))? {
            Some(mut segments) => {
                if segments
                    .first()
                    .is_none_or(|segment| segment.start_offset != 0)
                {
                    return Err(std::io::Error::other(
                        "Startup log segment expired under the console retention policy.",
                    ));
                }
                segments.remove(0).file
            }
            None => fs::File::open(source_path)?,
        };
        file.take(RUNTIME_STARTUP_SCAN_BYTE_LIMIT + 1)
            .read_to_end(&mut bytes)?;
        let truncated = bytes.len() as u64 > RUNTIME_STARTUP_SCAN_BYTE_LIMIT;
        bytes.truncate(RUNTIME_STARTUP_SCAN_BYTE_LIMIT as usize);
        let content = String::from_utf8_lossy(&bytes)
            .replace("\r\n", "\n")
            .replace('\r', "\n");
        // A byte limit must not turn part of a diagnostic into a ready signal.
        let content = if truncated {
            content
                .rsplit_once('\n')
                .map_or("", |(complete, _)| complete)
        } else {
            &content
        };
        snapshot.lines = content
            .lines()
            .take(RUNTIME_STARTUP_SCAN_LINE_LIMIT)
            .map(String::from)
            .collect();
        snapshot.total_lines = snapshot.lines.len();
        snapshot.truncated = truncated || snapshot.total_lines == RUNTIME_STARTUP_SCAN_LINE_LIMIT;
        Ok::<_, std::io::Error>(())
    })();
    if let Err(error) = result {
        snapshot.read_error = Some(error.to_string());
    }
    snapshot
}

fn analyze_runtime_health(
    module_id: &str,
    status: &InstanceStatus,
    log_snapshot: &LogTailSnapshot,
    active_run: Option<&ActiveInstanceRun>,
    ports: &[PortBinding],
) -> app_core::RuntimeHealth {
    let module_id = module_id.trim().to_ascii_lowercase();

    match status {
        InstanceStatus::Stopped => {
            return app_core::RuntimeHealth {
                status: String::from("stopped"),
                summary: String::from("Server is currently stopped."),
                reason: runtime_health_reason("stopped", &[]),
                matched_line: None,
            };
        }
        InstanceStatus::Stopping => {
            return app_core::RuntimeHealth {
                status: String::from("idle"),
                summary: String::from("Server is stopping."),
                reason: runtime_health_reason("stopping", &[]),
                matched_line: None,
            };
        }
        _ => {}
    }

    if let Some(error) = log_snapshot.read_error.as_deref() {
        return app_core::RuntimeHealth {
            status: if matches!(status, InstanceStatus::Error) {
                String::from("error")
            } else {
                String::from("warning")
            },
            summary: format!("Runtime log could not be read: {error}"),
            reason: runtime_health_reason("log_read_failed", &[("error", String::from(error))]),
            matched_line: None,
        };
    }

    let diagnostic_lines =
        health_evidence::current_run_lines(&module_id, status, log_snapshot, active_run, true);
    let lines = diagnostic_lines.as_ref();

    let world_dictionary_failure = if module_id == "projectzomboid" {
        find_latest_matching_log_line(
            lines,
            &["WorldDictionary: Cannot load world due to WorldDictionary error"],
        )
    } else {
        None
    };
    if let Some(line) =
        world_dictionary_failure.or_else(|| runtime_fatal_log_lines(&module_id, lines).pop())
    {
        return app_core::RuntimeHealth {
            status: String::from("error"),
            summary: String::from("A fatal runtime error pattern was detected in the recent log."),
            reason: runtime_health_reason("fatal_log_pattern", &[]),
            matched_line: Some(line),
        };
    }

    let current_lines =
        health_evidence::current_run_lines(&module_id, status, log_snapshot, active_run, false);
    let readiness_lines = health_evidence::readiness_lines(current_lines.as_ref());
    let lines = readiness_lines.as_ref();

    if module_id == "abioticfactor"
        && let Some(health) = analyze_abiotic_factor_runtime_health(lines)
        && (health.status == "error" || !matches!(status, InstanceStatus::Error))
    {
        return health;
    }

    if module_id == "runescapedragonwilds"
        && let Some(health) = analyze_runescape_dragonwilds_runtime_health(lines)
        && (health.status == "error" || !matches!(status, InstanceStatus::Error))
    {
        return health;
    }

    if matches!(status, InstanceStatus::Error) {
        return app_core::RuntimeHealth {
            status: String::from("error"),
            summary: String::from("The latest run ended in an error state."),
            reason: runtime_health_reason("latest_run_failed", &[]),
            matched_line: None,
        };
    }

    if matches!(
        module_id.as_str(),
        "unturned"
            | "squad"
            | "soulmask"
            | "theforest"
            | "sonsoftheforest"
            | "nightingale"
            | "windrose"
    ) {
        let same_run = active_run.is_some_and(|run| {
            run.run_id > 0 && run.log_path.is_some() && run.log_path == log_snapshot.source_path
        });
        let lines = if same_run { lines } else { &[] };
        return match module_id.as_str() {
            "squad" => squad_health::analyze(lines),
            "soulmask" => soulmask_health::analyze(lines),
            // This bridge emits fixed native-world observations with a
            // LanGame prefix. Keep the same-run/source gate and let only the
            // Forest parser accept its exact state markers.
            "theforest" => theforest_health::analyze(if same_run {
                current_lines.as_ref()
            } else {
                &[]
            }),
            "sonsoftheforest" => sonsoftheforest_health::analyze(lines),
            "nightingale" => nightingale_health::analyze(lines),
            "windrose" => windrose_health::analyze(lines),
            _ => unturned_health::analyze(lines),
        };
    }

    if module_id == "enshrouded"
        && let Some((line, ready)) = lines
            .iter()
            .rev()
            .find_map(|line| enshrouded_session_state(line).map(|ready| (line, ready)))
    {
        return app_core::RuntimeHealth {
            status: String::from(if ready { "ready" } else { "starting" }),
            summary: String::from(if ready {
                "Enshrouded completed its transition into the online game session."
            } else {
                "Enshrouded is transitioning its game session."
            }),
            reason: runtime_health_reason(
                if ready {
                    "ready_signal"
                } else {
                    "starting_tasks"
                },
                &[],
            ),
            matched_line: Some(line.clone()),
        };
    }

    if module_id == "dontstarve"
        && let Some(line) = find_latest_matching_log_line(
            lines,
            &[
                "failed to load modoverrides.lua",
                "failed to load ../worldgenoverride.lua",
            ],
        )
    {
        return app_core::RuntimeHealth {
            status: String::from("warning"),
            summary: String::from(
                "DST is running, but shard Lua config files failed to load. Save the instance once to rewrite the cluster files.",
            ),
            reason: runtime_health_reason("dst_lua_config_failed", &[]),
            matched_line: Some(line),
        };
    }

    if module_id == "arksurvivalevolved"
        && let Some(health) = analyze_ark_evolved_runtime_health(active_run, ports)
    {
        return health;
    }

    let module_ready_markers: &[&str] = match module_id.as_str() {
        "necesse" => &["started server using port"],
        "arksurvivalascended" => &["has successfully started", "advertising for join"],
        "humanitz" => &["loghzsuccess: display: success => session created!"],
        "sevendaystodie" => &["inf [steamworks.net] gameserver.init successful"],
        "satisfactory" => &["server startup time elapsed and saving/level loading is done"],
        "scum" => &["logscum: global stats:"],
        "projectzomboid" => &["*** server started ****"],
        "vrising" => &["[server] startup completed - disabling scene loading systems"],
        _ => &[],
    };
    if let Some(line) = lines
        .iter()
        .rev()
        .find(|line| matches_native_ready_signal(&module_id, line))
        .cloned()
        .or_else(|| find_latest_matching_log_line(lines, module_ready_markers))
        .or_else(|| {
            // These listeners precede world/session readiness in their native logs.
            if matches!(
                module_id.as_str(),
                "corekeeper" | "runescapedragonwilds" | "nightingale"
            ) {
                return None;
            }
            find_latest_matching_log_line(
                lines,
                &[
                    "lan server started on port",
                    "server ready",
                    "ready for connections",
                    "listening on port",
                    "listening on udp",
                    "heartbeat ok",
                    "server startup complete",
                    "load map complete",
                ],
            )
        })
    {
        return app_core::RuntimeHealth {
            status: String::from("ready"),
            summary: String::from("The server reported a ready or listening signal."),
            reason: runtime_health_reason("ready_signal", &[]),
            matched_line: Some(line),
        };
    }

    app_core::RuntimeHealth {
        status: String::from("starting"),
        summary: if lines.is_empty() {
            String::from("The process is running and waiting for startup log output.")
        } else {
            String::from("The process is running and still working through startup tasks.")
        },
        reason: runtime_health_reason(
            if lines.is_empty() {
                "starting_waiting_logs"
            } else {
                "starting_tasks"
            },
            &[],
        ),
        matched_line: None,
    }
}

fn enshrouded_session_state(line: &str) -> Option<bool> {
    let (transition, current) = line
        .trim()
        .strip_prefix("[Session] ")?
        .rsplit_once(" (current='")?;
    let current = current.strip_suffix("')!")?;
    let (phase, states) = transition.split_once(" transition from '")?;
    let (from, to) = states.strip_suffix('\'')?.split_once("' to '")?;
    if !matches!(phase, "started" | "finished") || from.is_empty() || to.is_empty() {
        return None;
    }
    Some(phase == "finished" && current == to && current == "Host_Online")
}

fn matches_native_ready_signal(module_id: &str, line: &str) -> bool {
    let line = line.trim();
    match module_id {
        "corekeeper" => line
            .rsplit_once("timescale = ")
            .is_some_and(|(_, value)| value == "0"),
        "minecraft" => clock_log_message(line)
            .and_then(|message| message.strip_prefix(" [Server thread/INFO]: Done ("))
            .and_then(|message| message.strip_suffix("s)! For help, type \"help\""))
            .and_then(|elapsed| elapsed.parse::<f64>().ok())
            .is_some_and(|elapsed| elapsed.is_finite() && elapsed >= 0.0),
        "valheim" => line
            .strip_suffix(": Game server connected")
            .is_some_and(|timestamp| {
                timestamp.bytes().any(|byte| byte.is_ascii_digit())
                    && timestamp
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || matches!(byte, b'/' | b':' | b' '))
            }),
        "barotrauma" => line == "Server started",
        "palworld" => line
            .strip_prefix("Running Palworld dedicated server on :")
            .filter(|port| !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()))
            .and_then(|port| port.parse::<u16>().ok())
            .is_some_and(|port| port != 0),
        "rimworld" => clock_log_message(line)
            .and_then(|message| message.strip_prefix(" | Listening for users at "))
            .and_then(|endpoint| endpoint.parse::<std::net::SocketAddr>().ok())
            .is_some_and(|endpoint| endpoint.port() != 0),
        _ => false,
    }
}

fn clock_log_message(line: &str) -> Option<&str> {
    let (timestamp, message) = line.strip_prefix('[')?.split_once(']')?;
    let bytes = timestamp.as_bytes();
    (bytes.len() == 8
        && bytes.iter().enumerate().all(|(index, byte)| {
            if matches!(index, 2 | 5) {
                *byte == b':'
            } else {
                byte.is_ascii_digit()
            }
        }))
    .then_some(message)
}

fn analyze_abiotic_factor_runtime_health(lines: &[String]) -> Option<app_core::RuntimeHealth> {
    if let Some(line) = find_latest_matching_log_line(
        lines,
        &[
            "world save integrity state: corrupt",
            "will shut down in 5 minutes due to world save corruption",
        ],
    ) {
        return Some(app_core::RuntimeHealth {
            status: String::from("error"),
            summary: String::from(
                "Abiotic Factor detected world save corruption and the dedicated server is not safe to keep online.",
            ),
            reason: runtime_health_reason("abiotic_world_corrupt", &[]),
            matched_line: Some(line),
        });
    }

    if let Some(line) = find_latest_matching_log_line(lines, &["session short code:"]) {
        return Some(app_core::RuntimeHealth {
            status: String::from("ready"),
            summary: String::from(
                "Abiotic Factor is online and has published a session short code for players to join.",
            ),
            reason: runtime_health_reason("abiotic_session_published", &[]),
            matched_line: Some(line),
        });
    }

    if let Some(line) = find_latest_matching_log_line(
        lines,
        &[
            "listening on port",
            "load map complete",
            "dedicated server is now loading the main map",
        ],
    ) {
        let loading_map = line
            .to_ascii_lowercase()
            .contains("dedicated server is now loading the main map");
        let summary = if loading_map {
            "Abiotic Factor is through world validation and is loading the main facility map."
        } else {
            "Abiotic Factor is listening and preparing its game session."
        };
        return Some(app_core::RuntimeHealth {
            status: String::from("starting"),
            summary: String::from(summary),
            reason: runtime_health_reason(
                if loading_map {
                    "abiotic_loading_map"
                } else {
                    "abiotic_listening"
                },
                &[],
            ),
            matched_line: Some(line),
        });
    }

    if let Some(line) = find_latest_matching_log_line(
        lines,
        &[
            "checking world save for corruption",
            "world save:",
            "could not find any files for the save, this is fine if it's a new save",
        ],
    ) {
        return Some(app_core::RuntimeHealth {
            status: String::from("starting"),
            summary: String::from(
                "Abiotic Factor is validating the selected world save and preparing the map.",
            ),
            reason: runtime_health_reason("abiotic_validating_world", &[]),
            matched_line: Some(line),
        });
    }

    None
}

fn analyze_runescape_dragonwilds_runtime_health(
    lines: &[String],
) -> Option<app_core::RuntimeHealth> {
    for line in lines.iter().rev() {
        let normalized = line.to_ascii_lowercase();
        if normalized.contains("readytojoin") && normalized.contains("value[1]") {
            let map_name = latest_runescape_dragonwilds_map_name(lines);
            let summary = if let Some(map_name) = map_name.as_deref() {
                format!(
                    "RuneScape: Dragonwilds reported ReadyToJoin after creating the GameSession on map {map_name}."
                )
            } else {
                String::from(
                    "RuneScape: Dragonwilds reported ReadyToJoin after creating the GameSession.",
                )
            };
            return Some(app_core::RuntimeHealth {
                status: String::from("ready"),
                summary,
                reason: if let Some(map_name) = map_name {
                    runtime_health_reason("dragonwilds_ready_map", &[("map", map_name)])
                } else {
                    runtime_health_reason("dragonwilds_ready", &[])
                },
                matched_line: Some(line.clone()),
            });
        }

        if normalized.contains("dedicatedserver.ini configuration")
            && normalized.contains("ownerid")
        {
            return Some(app_core::RuntimeHealth {
                status: String::from("error"),
                summary: String::from(
                    "RuneScape: Dragonwilds stopped at DedicatedServer.ini validation because OwnerId is missing or invalid.",
                ),
                reason: runtime_health_reason("dragonwilds_owner_invalid", &[]),
                matched_line: Some(line.clone()),
            });
        }
    }

    None
}

fn latest_runescape_dragonwilds_map_name(lines: &[String]) -> Option<String> {
    lines.iter().rev().find_map(|line| {
        let (_, rest) = line.split_once("MapName[")?;
        let (map_name, _) = rest.split_once(']')?;
        let trimmed = map_name.trim();
        (!trimmed.is_empty()).then(|| String::from(trimmed))
    })
}

fn ark_evolved_required_udp_ports(ports: &[PortBinding]) -> Vec<u16> {
    let mut required_ports = ports
        .iter()
        .filter(|binding| binding.protocol.eq_ignore_ascii_case("udp"))
        .filter(|binding| {
            ARK_EVOLVED_REQUIRED_UDP_PORT_NAMES
                .iter()
                .any(|name| binding.name.eq_ignore_ascii_case(name))
        })
        .map(|binding| binding.port)
        .collect::<Vec<_>>();
    required_ports.sort_unstable();
    required_ports.dedup();
    required_ports
}

fn format_port_list(ports: &[u16]) -> String {
    ports
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(windows)]
fn owned_udp_ports_for_pid(pid: u32) -> Result<HashSet<u16>, String> {
    let output = Command::new("netstat")
        .args(["-ano", "-p", "udp"])
        .output()
        .map_err(|error| format!("failed to run netstat: {error}"))?;
    if !output.status.success() {
        return Err(format!("netstat exited with status {}", output.status));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .filter_map(parse_netstat_udp_port_line)
        .filter_map(|(line_pid, port)| (line_pid == pid).then_some(port))
        .collect())
}

#[cfg(not(windows))]
fn owned_udp_ports_for_pid(_pid: u32) -> Result<HashSet<u16>, String> {
    Err(String::from(
        "runtime UDP port observation is only available on Windows",
    ))
}

#[cfg(windows)]
fn parse_netstat_udp_port_line(line: &str) -> Option<(u32, u16)> {
    let fields = line.split_whitespace().collect::<Vec<_>>();
    if fields.len() < 4 || !fields[0].eq_ignore_ascii_case("udp") {
        return None;
    }

    let pid = fields.last()?.parse::<u32>().ok()?;
    let local_endpoint = fields.get(1)?;
    let port = parse_endpoint_port(local_endpoint)?;
    Some((pid, port))
}

#[cfg(windows)]
fn parse_endpoint_port(endpoint: &str) -> Option<u16> {
    endpoint
        .trim()
        .rsplit_once(':')
        .and_then(|(_, port)| port.parse::<u16>().ok())
}

fn collect_runtime_diagnostics(
    module_id: &str,
    _status: &InstanceStatus,
    log_snapshot: &LogTailSnapshot,
    health: &app_core::RuntimeHealth,
    saves_dir: &Path,
    active_run: Option<&ActiveInstanceRun>,
) -> Vec<app_core::RuntimeDiagnosticSignal> {
    if log_snapshot.read_error.is_some() {
        return Vec::new();
    }

    let module_id = module_id.trim().to_ascii_lowercase();
    let lines = &log_snapshot.lines;

    if module_id == "abioticfactor" {
        return collect_abiotic_factor_runtime_diagnostics(lines, health);
    }

    if module_id == "runescapedragonwilds" {
        return collect_runescape_dragonwilds_runtime_diagnostics(lines, saves_dir, active_run);
    }

    Vec::new()
}

fn collect_abiotic_factor_runtime_diagnostics(
    lines: &[String],
    health: &app_core::RuntimeHealth,
) -> Vec<app_core::RuntimeDiagnosticSignal> {
    let mut diagnostics = Vec::new();

    if let Some(line) = find_latest_matching_log_line(
        lines,
        &["remote console through https is explicitly disabled"],
    ) {
        diagnostics.push(runtime_diagnostic_signal(
            "abiotic_remote_console_https_disabled",
            "info",
            "Abiotic Factor has the built-in HTTPS remote console disabled. That is expected for the current desktop-managed host unless you intentionally plan to expose the game's own remote console.",
            Some(line),
            false,
        ));
    }

    if let Some(line) = find_latest_matching_log_line(
        lines,
        &[
            "baseuserinterface delegates not bound. base interface not valid",
            "basestoreinterface delegates not bound. base interface not valid",
            "basepurchaseinterface delegates not bound. base interface not valid",
            "baseexternaluiinterface delegates not bound. base interface not valid",
            "basevoiceinterface delegates not bound. base interface not valid",
            "baseusercloudinterface delegates not bound. base interface not valid",
            "unable to call method in base interface. base interface not valid",
        ],
    ) {
        diagnostics.push(runtime_diagnostic_signal(
            "abiotic_headless_eos_interfaces_unavailable",
            "info",
            "Abiotic Factor is reporting missing EOS local UI or voice interfaces. This is expected on a headless dedicated server and is not a join blocker by itself.",
            Some(line),
            false,
        ));
    }

    if let Some(line) = find_latest_matching_log_line(
        lines,
        &["can't start an online game for session (gamesession) that hasn't been created"],
    ) {
        let join_code_published = health
            .matched_line
            .as_deref()
            .map(|matched_line| {
                matched_line
                    .to_ascii_lowercase()
                    .contains("session short code:")
            })
            .unwrap_or(false)
            || find_latest_matching_log_line(lines, &["session short code:"]).is_some();
        let (severity, summary, actionable) = if join_code_published {
            (
                "info",
                "Abiotic Factor emitted an online-session startup warning after the room had already published a session short code. Treat this as engine noise unless players are still unable to join.",
                false,
            )
        } else {
            (
                "warning",
                "Abiotic Factor reported an online-session startup failure before the room finished publishing a join code. Check the latest log lines if players cannot discover or join this server.",
                true,
            )
        };
        diagnostics.push(runtime_diagnostic_signal(
            "abiotic_online_session_start_warning",
            severity,
            summary,
            Some(line),
            actionable,
        ));
    }

    diagnostics
}

fn collect_runescape_dragonwilds_runtime_diagnostics(
    lines: &[String],
    saves_dir: &Path,
    active_run: Option<&ActiveInstanceRun>,
) -> Vec<app_core::RuntimeDiagnosticSignal> {
    let mut diagnostics = Vec::new();

    if let Some(line) = find_latest_matching_log_line(
        lines,
        &["discovered 0 console commands using redpointconsolecommand class"],
    ) {
        diagnostics.push(runtime_diagnostic_signal(
            "dragonwilds_no_host_console_commands_discovered",
            "info",
            "RuneScape: Dragonwilds did not expose Redpoint console commands in the smoke log. Keep LanGame player-management actions pending until a host-callable moderation surface is verified.",
            Some(line),
            false,
        ));
    }

    if let Some((query_port, line)) = latest_runescape_dragonwilds_query_port_launch_arg(lines) {
        let summary = format!(
            "RuneScape: Dragonwilds was launched with QueryPort {query_port}, but LanGame keeps live player query disabled until that endpoint returns a stable roster or count protocol."
        );
        diagnostics.push(runtime_diagnostic_signal(
            "dragonwilds_query_port_launch_arg_observed",
            "info",
            &summary,
            Some(line),
            false,
        ));

        if let Some(signal) =
            runescape_dragonwilds_query_port_runtime_binding_signal(active_run, query_port)
        {
            diagnostics.push(signal);
        }
    }

    if let Some(signal) = latest_runescape_dragonwilds_world_save_signal(saves_dir) {
        diagnostics.push(signal);
    }

    diagnostics
}

fn runescape_dragonwilds_query_port_runtime_binding_signal(
    active_run: Option<&ActiveInstanceRun>,
    query_port: u16,
) -> Option<app_core::RuntimeDiagnosticSignal> {
    let pid = active_run.and_then(|run| run.pid)?;
    let owned_udp_ports = owned_udp_ports_for_pid(pid).ok()?;
    Some(runescape_dragonwilds_query_port_binding_signal(
        query_port,
        &owned_udp_ports,
        None,
    ))
}

pub(crate) fn runescape_dragonwilds_query_port_binding_signal(
    query_port: u16,
    owned_udp_ports: &HashSet<u16>,
    matched_line: Option<String>,
) -> app_core::RuntimeDiagnosticSignal {
    if owned_udp_ports.contains(&query_port) {
        return runtime_diagnostic_signal(
            "dragonwilds_query_port_bound",
            "info",
            &format!(
                "RuneScape: Dragonwilds has an owned UDP endpoint for QueryPort {query_port}. LanGame still keeps player query disabled until a stable protocol is verified."
            ),
            matched_line,
            false,
        );
    }

    runtime_diagnostic_signal(
        "dragonwilds_query_port_not_bound",
        "warning",
        &format!(
            "RuneScape: Dragonwilds was launched with QueryPort {query_port}, but the process does not currently own that UDP endpoint. Treat this as discovery metadata only, not a live player query or GM surface."
        ),
        matched_line,
        true,
    )
}

fn latest_runescape_dragonwilds_query_port_launch_arg(lines: &[String]) -> Option<(u16, String)> {
    lines.iter().rev().find_map(|line| {
        let normalized = line.to_ascii_lowercase();
        if !normalized.contains("command line:") || !normalized.contains("-queryport") {
            return None;
        }

        parse_launch_arg_port(line, "-QueryPort")
            .or_else(|| parse_launch_arg_port(line, "-queryport"))
            .map(|port| (port, line.clone()))
    })
}

fn parse_launch_arg_port(line: &str, arg_name: &str) -> Option<u16> {
    let (_, rest) = line.split_once(arg_name)?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('=').unwrap_or(rest).trim_start();
    let digits = rest
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u16>().ok()
}

fn latest_runescape_dragonwilds_world_save_signal(
    saves_dir: &Path,
) -> Option<app_core::RuntimeDiagnosticSignal> {
    let entries = fs::read_dir(saves_dir).ok()?;
    let mut save_count = 0usize;
    let mut latest: Option<(std::time::SystemTime, String, u64)> = None;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let is_save = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.eq_ignore_ascii_case("sav"))
            .unwrap_or(false);
        if !is_save {
            continue;
        }

        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let file_name = entry.file_name().to_string_lossy().into_owned();
        let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
        let size = metadata.len();
        save_count += 1;

        let should_replace = latest
            .as_ref()
            .map(|(latest_modified, latest_name, _)| {
                modified > *latest_modified
                    || (modified == *latest_modified && file_name > *latest_name)
            })
            .unwrap_or(true);
        if should_replace {
            latest = Some((modified, file_name, size));
        }
    }

    let (_, file_name, size) = latest?;
    let summary = format!(
        "RuneScape: Dragonwilds latest world save is {file_name} ({size} bytes); {save_count} .sav files detected in {}. Stop the server before moving or restoring saves.",
        saves_dir.display()
    );
    Some(runtime_diagnostic_signal(
        "dragonwilds_latest_world_save_detected",
        "info",
        &summary,
        None,
        false,
    ))
}

fn runtime_diagnostic_signal(
    code: &str,
    severity: &str,
    summary: &str,
    matched_line: Option<String>,
    actionable: bool,
) -> app_core::RuntimeDiagnosticSignal {
    app_core::RuntimeDiagnosticSignal {
        code: String::from(code),
        severity: String::from(severity),
        summary: String::from(summary),
        matched_line,
        actionable,
    }
}

fn find_latest_matching_log_line(lines: &[String], patterns: &[&str]) -> Option<String> {
    let normalized_patterns = patterns
        .iter()
        .map(|pattern| pattern.to_ascii_lowercase())
        .collect::<Vec<_>>();

    lines.iter().rev().find_map(|line| {
        let normalized_line = line.to_ascii_lowercase();
        normalized_patterns
            .iter()
            .any(|pattern| normalized_line.contains(pattern))
            .then(|| line.clone())
    })
}

pub async fn read_instance_log_document(
    paths: &StoragePaths,
    instance_id: &str,
    max_lines: usize,
    run_id: Option<i64>,
) -> Result<LogTailSnapshot, StorageError> {
    let pool = connect_pool(paths).await?;
    let result = async {
        let record = fetch_instance_record(&pool, instance_id).await?;
        let log_source_path = if let Some(run_id) = run_id {
            game_log::load_instance_run_by_id(&pool, instance_id, run_id)
                .await?
                .log_path
        } else {
            let active_run = load_active_instance_run(&pool, instance_id).await?;
            let recent_runs = load_recent_instance_runs(&pool, instance_id, 8).await?;
            resolve_instance_log_source_path(&record, active_run.as_ref(), &recent_runs).await?
        };
        Ok(read_log_snapshot(log_source_path, max_lines.max(1)))
    }
    .await;
    // Rejected paths must release SQLite's handles before returning too; merely
    // dropping its asynchronous pool can race immediate instance/archive cleanup.
    pool.close().await;
    result
}

pub struct StartedInstanceProcess<'a> {
    pub instance_id: &'a str,
    pub session_id: Option<&'a str>,
    pub process_key: &'a str,
    pub display_name: &'a str,
    pub pid: u32,
    pub log_path: &'a str,
    pub is_primary: bool,
}

pub async fn mark_instance_process_started_with_identity(
    paths: &StoragePaths,
    process: &StartedInstanceProcess<'_>,
    process_identity: Option<&ProcessIdentity>,
) -> Result<InstanceProcessState, StorageError> {
    let StartedInstanceProcess {
        instance_id,
        session_id,
        process_key,
        display_name,
        pid,
        log_path,
        is_primary,
    } = *process;
    let pool = connect_pool(paths).await?;

    let mut tx = pool.begin().await?;
    fetch_instance_record(&mut *tx, instance_id).await?;

    sqlx::query(
        r#"
        UPDATE instances
        SET status = 'running',
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?1
        "#,
    )
    .bind(instance_id)
    .execute(&mut *tx)
    .await?;

    let result = sqlx::query(
        r#"
        INSERT INTO instance_runs (
            instance_id, session_id, process_key, display_name, is_primary,
            pid, process_creation_time, process_image_path, status, started_at, log_path
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'running', CURRENT_TIMESTAMP, ?9)
        "#,
    )
    .bind(instance_id)
    .bind(session_id)
    .bind(process_key)
    .bind(display_name)
    .bind(if is_primary { 1_i64 } else { 0_i64 })
    .bind(i64::from(pid))
    .bind(process_identity.map(|identity| identity.creation_time.to_string()))
    .bind(process_identity.map(|identity| identity.image_path.as_str()))
    .bind(log_path)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    pool.close().await;

    Ok(InstanceProcessState {
        run_id: result.last_insert_rowid(),
        session_id: session_id.map(String::from),
        process_key: String::from(process_key),
        display_name: String::from(display_name),
        pid: Some(pid),
        process_identity: process_identity.cloned(),
        status: String::from("running"),
        started_at: None,
        stopped_at: None,
        exit_code: None,
        crash_flag: false,
        log_path: Some(String::from(log_path)),
        is_primary,
    })
}

pub async fn mark_instance_process_stopped(
    paths: &StoragePaths,
    instance_id: &str,
    run_id: i64,
    exit_code: Option<i32>,
    crash_flag: bool,
) -> Result<InstanceSummary, StorageError> {
    let pool = connect_pool(paths).await?;

    let mut tx = pool.begin().await?;
    fetch_instance_record(&mut *tx, instance_id).await?;

    let run_status = if crash_flag { "error" } else { "stopped" };
    let run_update = sqlx::query(
        r#"
        UPDATE instance_runs
        SET status = ?2,
            stopped_at = CURRENT_TIMESTAMP,
            exit_code = ?3,
            crash_flag = ?4
        WHERE id = ?1 AND instance_id = ?5
        "#,
    )
    .bind(run_id)
    .bind(run_status)
    .bind(exit_code.map(i64::from))
    .bind(if crash_flag { 1_i64 } else { 0_i64 })
    .bind(instance_id)
    .execute(&mut *tx)
    .await?;

    if run_update.rows_affected() == 0 {
        return Err(StorageError::MissingInstanceRun {
            instance_id: String::from(instance_id),
            run_id,
        });
    }

    let rows = load_instance_run_rows(&mut *tx, instance_id, None, Some(96)).await?;
    let current_row = rows
        .iter()
        .find(|row| row.run_id == run_id)
        .cloned()
        .ok_or_else(|| StorageError::MissingInstanceRun {
            instance_id: String::from(instance_id),
            run_id,
        })?;
    let session_key = session_group_key(&current_row);
    let session_rows = rows
        .iter()
        .filter(|row| session_group_key(row) == session_key)
        .cloned()
        .collect::<Vec<_>>();
    let session_status = aggregate_run_status(&session_rows);
    let instance_status = if rows.iter().any(|row| row.status == "running") {
        if session_status == "error" {
            "error"
        } else {
            "running"
        }
    } else if session_status == "error" {
        "error"
    } else {
        "stopped"
    };

    sqlx::query(
        r#"
        UPDATE instances
        SET status = ?2,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?1
        "#,
    )
    .bind(instance_id)
    .bind(instance_status)
    .execute(&mut *tx)
    .await?;

    let row = sqlx::query(
        r#"
        SELECT id, name, module_id, status, bind_ip, autostart,
               (SELECT COUNT(*) FROM instance_ports WHERE instance_id = instances.id) AS port_count,
               (SELECT COUNT(*) FROM instance_runs
                WHERE instance_id = instances.id AND status = 'running') AS active_process_count
        FROM instances
        WHERE id = ?1
        "#,
    )
    .bind(instance_id)
    .fetch_one(&mut *tx)
    .await?;

    let summary = map_instance_summary(&row);

    tx.commit().await?;
    pool.close().await;

    Ok(summary)
}

async fn load_instance_run_rows<'e, E>(
    executor: E,
    instance_id: &str,
    status: Option<&str>,
    limit: Option<i64>,
) -> Result<Vec<StoredInstanceRunRow>, StorageError>
where
    E: Executor<'e, Database = Sqlite>,
{
    let sql = match (status.is_some(), limit.is_some()) {
        (true, true) => {
            r#"
            SELECT id, session_id,
                   COALESCE(process_key, 'main') AS process_key,
                   COALESCE(display_name, process_key, 'Server') AS display_name,
                   is_primary, status, pid, process_creation_time, process_image_path,
                   started_at, stopped_at, exit_code, crash_flag, log_path
            FROM instance_runs
            WHERE instance_id = ?1 AND status = ?2
            ORDER BY id DESC
            LIMIT ?3
            "#
        }
        (true, false) => {
            r#"
            SELECT id, session_id,
                   COALESCE(process_key, 'main') AS process_key,
                   COALESCE(display_name, process_key, 'Server') AS display_name,
                   is_primary, status, pid, process_creation_time, process_image_path,
                   started_at, stopped_at, exit_code, crash_flag, log_path
            FROM instance_runs
            WHERE instance_id = ?1 AND status = ?2
            ORDER BY id DESC
            "#
        }
        (false, true) => {
            r#"
            SELECT id, session_id,
                   COALESCE(process_key, 'main') AS process_key,
                   COALESCE(display_name, process_key, 'Server') AS display_name,
                   is_primary, status, pid, process_creation_time, process_image_path,
                   started_at, stopped_at, exit_code, crash_flag, log_path
            FROM instance_runs
            WHERE instance_id = ?1
            ORDER BY id DESC
            LIMIT ?2
            "#
        }
        (false, false) => {
            r#"
            SELECT id, session_id,
                   COALESCE(process_key, 'main') AS process_key,
                   COALESCE(display_name, process_key, 'Server') AS display_name,
                   is_primary, status, pid, process_creation_time, process_image_path,
                   started_at, stopped_at, exit_code, crash_flag, log_path
            FROM instance_runs
            WHERE instance_id = ?1
            ORDER BY id DESC
            "#
        }
    };

    let rows = match (status, limit) {
        (Some(status), Some(limit)) => {
            sqlx::query(sql)
                .bind(instance_id)
                .bind(status)
                .bind(limit.max(1))
                .fetch_all(executor)
                .await?
        }
        (Some(status), None) => {
            sqlx::query(sql)
                .bind(instance_id)
                .bind(status)
                .fetch_all(executor)
                .await?
        }
        (None, Some(limit)) => {
            sqlx::query(sql)
                .bind(instance_id)
                .bind(limit.max(1))
                .fetch_all(executor)
                .await?
        }
        (None, None) => {
            sqlx::query(sql)
                .bind(instance_id)
                .fetch_all(executor)
                .await?
        }
    };

    Ok(rows.iter().map(map_instance_run_row).collect())
}

fn session_group_key(row: &StoredInstanceRunRow) -> String {
    row.session_id
        .clone()
        .unwrap_or_else(|| format!("legacy-{}", row.run_id))
}

fn sort_process_rows(rows: &mut [StoredInstanceRunRow]) {
    rows.sort_by(|left, right| {
        right
            .is_primary
            .cmp(&left.is_primary)
            .then_with(|| left.run_id.cmp(&right.run_id))
            .then_with(|| left.process_key.cmp(&right.process_key))
    });
}

fn select_primary_process(rows: &[StoredInstanceRunRow]) -> &StoredInstanceRunRow {
    rows.iter()
        .find(|row| row.is_primary)
        .or_else(|| {
            rows.iter()
                .find(|row| row.process_key.eq_ignore_ascii_case("master"))
        })
        .or_else(|| {
            rows.iter()
                .find(|row| row.process_key.eq_ignore_ascii_case("main"))
        })
        .unwrap_or(&rows[0])
}

fn aggregate_run_status(rows: &[StoredInstanceRunRow]) -> String {
    if rows
        .iter()
        .any(|row| row.status == "error" || row.crash_flag)
    {
        return String::from("error");
    }
    if rows.iter().any(|row| row.status == "running") {
        return String::from("running");
    }
    if rows.iter().any(|row| row.status == "stopping") {
        return String::from("stopping");
    }
    String::from("stopped")
}

fn aggregate_exit_code(
    rows: &[StoredInstanceRunRow],
    primary: &StoredInstanceRunRow,
) -> Option<i32> {
    primary
        .exit_code
        .or_else(|| {
            rows.iter()
                .find_map(|row| row.exit_code.filter(|code| *code != 0))
        })
        .or_else(|| rows.iter().find_map(|row| row.exit_code))
}

fn map_process_state(row: &StoredInstanceRunRow) -> InstanceProcessState {
    InstanceProcessState {
        run_id: row.run_id,
        session_id: row.session_id.clone(),
        process_key: row.process_key.clone(),
        display_name: row.display_name.clone(),
        pid: row.pid,
        process_identity: row.process_identity.clone(),
        status: row.status.clone(),
        started_at: row.started_at.clone(),
        stopped_at: row.stopped_at.clone(),
        exit_code: row.exit_code,
        crash_flag: row.crash_flag,
        log_path: row.log_path.clone(),
        is_primary: row.is_primary,
    }
}

fn aggregate_instance_run_record(rows: &[StoredInstanceRunRow]) -> InstanceRunRecord {
    let mut sorted_rows = rows.to_vec();
    sort_process_rows(&mut sorted_rows);
    let primary = select_primary_process(&sorted_rows);
    let processes = sorted_rows
        .iter()
        .map(map_process_state)
        .collect::<Vec<_>>();

    InstanceRunRecord {
        run_id: primary.run_id,
        session_id: primary.session_id.clone(),
        status: aggregate_run_status(&sorted_rows),
        pid: primary.pid,
        started_at: sorted_rows
            .iter()
            .filter_map(|row| row.started_at.clone())
            .min(),
        stopped_at: sorted_rows
            .iter()
            .filter_map(|row| row.stopped_at.clone())
            .max(),
        exit_code: aggregate_exit_code(&sorted_rows, primary),
        crash_flag: sorted_rows.iter().any(|row| row.crash_flag),
        log_path: primary.log_path.clone(),
        process_count: processes.len(),
        processes,
    }
}

fn aggregate_active_instance_run(rows: &[StoredInstanceRunRow]) -> ActiveInstanceRun {
    let record = aggregate_instance_run_record(rows);
    ActiveInstanceRun {
        run_id: record.run_id,
        session_id: record.session_id.clone(),
        pid: record.pid,
        log_path: record.log_path.clone(),
        process_count: record.process_count,
        processes: record.processes,
    }
}

fn aggregate_recent_instance_runs(
    rows: Vec<StoredInstanceRunRow>,
    limit: usize,
) -> Vec<InstanceRunRecord> {
    let mut grouped = HashMap::<String, Vec<StoredInstanceRunRow>>::new();
    let mut order = Vec::<String>::new();

    for row in rows {
        let key = session_group_key(&row);
        if !grouped.contains_key(&key) {
            order.push(key.clone());
        }
        grouped.entry(key).or_default().push(row);
    }

    order
        .into_iter()
        .take(limit.max(1))
        .filter_map(|key| grouped.remove(&key))
        .map(|group| aggregate_instance_run_record(&group))
        .collect()
}
pub(crate) async fn load_active_instance_run<'e, E>(
    executor: E,
    instance_id: &str,
) -> Result<Option<ActiveInstanceRun>, StorageError>
where
    E: Executor<'e, Database = Sqlite>,
{
    let all_rows = load_instance_run_rows(executor, instance_id, None, Some(96)).await?;
    let Some(first_row) = all_rows.iter().find(|row| row.status == "running") else {
        return Ok(None);
    };

    let active_session_key = session_group_key(first_row);
    let session_rows = all_rows
        .into_iter()
        .filter(|row| session_group_key(row) == active_session_key)
        .collect::<Vec<_>>();

    Ok(Some(aggregate_active_instance_run(&session_rows)))
}

async fn load_recent_instance_runs<'e, E>(
    executor: E,
    instance_id: &str,
    limit: i64,
) -> Result<Vec<InstanceRunRecord>, StorageError>
where
    E: Executor<'e, Database = Sqlite>,
{
    let row_limit = (limit.max(1) * 6).clamp(12, 96);
    let rows = load_instance_run_rows(executor, instance_id, None, Some(row_limit)).await?;
    Ok(aggregate_recent_instance_runs(rows, limit.max(1) as usize))
}

/// Counts consecutive failed sessions independently of the short UI history.
/// `stop_after` is the caller's restart limit plus one; both shards belong to one
/// session, so saving a surviving shard never erases the other shard's failure.
pub async fn read_instance_restart_failure_count(
    paths: &StoragePaths,
    instance_id: &str,
    stop_after: usize,
) -> Result<usize, StorageError> {
    let pool = connect_pool(paths).await?;
    fetch_instance_record(&pool, instance_id).await?;
    let rows = sqlx::query(
        r#"
        SELECT MAX(CASE WHEN crash_flag != 0 OR COALESCE(exit_code, 0) != 0
                        THEN 1 ELSE 0 END) AS failed
        FROM instance_runs
        WHERE instance_id = ?1
        GROUP BY CASE WHEN session_id IS NULL THEN 'run:' || id
                      ELSE 'session:' || session_id END
        ORDER BY MAX(id) DESC
        LIMIT ?2
        "#,
    )
    .bind(instance_id)
    .bind(i64::try_from(stop_after.max(1)).unwrap_or(i64::MAX))
    .fetch_all(&pool)
    .await?;
    let count = rows
        .iter()
        .take_while(|row| row.get::<i64, _>("failed") != 0)
        .count();
    pool.close().await;
    Ok(count)
}

async fn resolve_instance_log_source_path(
    record: &StoredInstanceRecord,
    active_run: Option<&ActiveInstanceRun>,
    recent_runs: &[InstanceRunRecord],
) -> Result<Option<String>, StorageError> {
    // DST reuses its native log path across launches. Its modification time
    // cannot associate old readiness output with the current managed run.
    if record.summary.module_id.eq_ignore_ascii_case("dontstarve") {
        if let Some(active) = active_run {
            return Ok(active.log_path.clone());
        }
        let logs_root = instance_logs_dir_from_record(record);
        let recorded = recent_runs.iter().find_map(|run| run.log_path.clone());
        let latest =
            tokio::task::spawn_blocking(move || latest_dst_wrapper_log(&logs_root, recorded))
                .await
                .map_err(|error| StorageError::BlockingTaskFailed {
                    operation: "discover DST startup log",
                    message: error.to_string(),
                })??;
        // Failed starts are not registered runs. Prefer their owned wrapper over
        // the native path, which may still contain a previous launch's output.
        if latest.is_some() {
            return Ok(latest);
        }
    }
    let process_log_path = active_run
        .and_then(|run| run.log_path.clone())
        .or_else(|| recent_runs.iter().find_map(|run| run.log_path.clone()));
    let module_log_path = resolve_module_runtime_log_path(record)?;
    Ok(prefer_runtime_log_path(
        &record.summary.module_id,
        process_log_path,
        module_log_path,
    ))
}

fn dst_wrapper_log_stamp(path: &Path) -> Option<u128> {
    let stamp = path
        .file_name()?
        .to_str()?
        .strip_prefix("run-")?
        .strip_suffix("-master.log")?;
    if stamp.is_empty() || !stamp.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let parsed = stamp.parse::<u128>().ok()?;
    (parsed.to_string() == stamp).then_some(parsed)
}

fn runtime_log_is_link(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn latest_dst_wrapper_log(
    logs_root: &Path,
    recorded: Option<String>,
) -> Result<Option<String>, StorageError> {
    let previous = latest_dst_wrapper_log_in_directory(logs_root, recorded.clone())?;
    let managed = latest_dst_wrapper_log_in_directory(
        &logs_root.join("managed-console"),
        previous.clone().or(recorded),
    )?;
    Ok(managed.or(previous))
}

fn latest_dst_wrapper_log_in_directory(
    logs_root: &Path,
    recorded: Option<String>,
) -> Result<Option<String>, StorageError> {
    for ancestor in logs_root.ancestors() {
        let metadata = match fs::symlink_metadata(ancestor) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(StorageError::ReadPath {
                    path: ancestor.to_path_buf(),
                    source,
                });
            }
        };
        if !metadata.is_dir() || runtime_log_is_link(&metadata) {
            return Err(StorageError::ReadDirectory {
                path: logs_root.to_path_buf(),
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "DST startup log directory must not cross symbolic links or reparse points",
                ),
            });
        }
    }
    let directory_error = |source| StorageError::ReadDirectory {
        path: logs_root.to_path_buf(),
        source,
    };
    let entries = fs::read_dir(logs_root).map_err(directory_error)?;
    let mut latest: Option<(u128, PathBuf)> = None;
    for (index, entry) in entries.enumerate() {
        if index >= DST_WRAPPER_LOG_DIRECTORY_ENTRY_LIMIT {
            return Err(directory_error(std::io::Error::other(format!(
                "DST startup log discovery exceeds its {} directory entry limit",
                DST_WRAPPER_LOG_DIRECTORY_ENTRY_LIMIT,
            ))));
        }
        let entry = entry.map_err(directory_error)?;
        let path = entry.path();
        let Some(stamp) = dst_wrapper_log_stamp(&path) else {
            continue;
        };
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => return Err(StorageError::ReadPath { path, source }),
        };
        if metadata.is_file()
            && !runtime_log_is_link(&metadata)
            && latest
                .as_ref()
                .is_none_or(|(previous, _)| stamp > *previous)
        {
            latest = Some((stamp, path));
        }
    }
    let Some((stamp, path)) = latest else {
        return Ok(None);
    };
    if let Some(recorded) = recorded {
        let recorded_path = Path::new(&recorded);
        let recorded_stamp = dst_wrapper_log_stamp(recorded_path).or_else(|| {
            fs::metadata(recorded_path)
                .ok()?
                .modified()
                .ok()?
                .duration_since(UNIX_EPOCH)
                .ok()
                .map(|time| time.as_millis())
        });
        if recorded_stamp.is_some_and(|recorded_stamp| recorded_stamp >= stamp) {
            return Ok(Some(recorded));
        }
    }
    Ok(Some(path.to_string_lossy().into_owned()))
}

fn resolve_module_runtime_log_path(
    record: &StoredInstanceRecord,
) -> Result<Option<String>, StorageError> {
    let install_root = effective_instance_install_root(record)?;

    Ok(
        known_module_runtime_log_path(&record.summary.module_id, &install_root, record)
            .filter(|path| path.exists())
            .map(|path| path.to_string_lossy().into_owned()),
    )
}

fn known_module_runtime_log_path(
    module_id: &str,
    install_root: &Path,
    record: &StoredInstanceRecord,
) -> Option<PathBuf> {
    let instance_logs_dir = instance_logs_dir_from_record(record);

    if module_id.eq_ignore_ascii_case("corekeeper") {
        return Some(instance_logs_dir.join("CoreKeeperServer.log"));
    }

    if module_id.eq_ignore_ascii_case("dontstarve") {
        return Some(
            record
                .config_dir
                .join("clusters/main/Master/server_log.txt"),
        );
    }

    if module_id.eq_ignore_ascii_case("satisfactory") {
        return Some(
            record
                .config_dir
                .parent()?
                .join("data/Saved/Logs/FactoryGame.log"),
        );
    }

    if module_id.eq_ignore_ascii_case("scum") {
        return Some(install_root.join("SCUM/Saved/Logs/SCUM.log"));
    }

    if module_id.eq_ignore_ascii_case("arksurvivalascended") {
        let instance_log = instance_logs_dir.join("ark-ascended-server.log");
        if instance_log.exists() {
            return Some(instance_log);
        }
        return Some(
            install_root
                .join("ShooterGame")
                .join("Saved")
                .join("Logs")
                .join("ShooterGame.log"),
        );
    }

    if module_id.eq_ignore_ascii_case("arksurvivalevolved") {
        let instance_log = instance_logs_dir.join("ark-evolved-server.log");
        if instance_log.exists() {
            return Some(instance_log);
        }
        return Some(
            install_root
                .join("ShooterGame")
                .join("Saved")
                .join("Logs")
                .join("ShooterGame.log"),
        );
    }

    if module_id.eq_ignore_ascii_case("conanexiles") {
        return Some(instance_logs_dir.join("conan-server.log"));
    }

    if module_id.eq_ignore_ascii_case("runescapedragonwilds") {
        return Some(
            install_root
                .join("RSDragonwilds")
                .join("Saved")
                .join("Logs")
                .join("RSDragonwilds.log"),
        );
    }

    None
}

fn instance_logs_dir_from_record(record: &StoredInstanceRecord) -> PathBuf {
    record
        .config_dir
        .parent()
        .unwrap_or(record.config_dir.as_path())
        .join("logs")
}

fn prefer_runtime_log_path(
    module_id: &str,
    process_log_path: Option<String>,
    module_log_path: Option<String>,
) -> Option<String> {
    let Some(module_log_path) = module_log_path else {
        return process_log_path;
    };
    let module_log = PathBuf::from(&module_log_path);

    if module_runtime_log_is_authoritative(module_id) && log_path_has_content(&module_log) {
        return Some(module_log_path);
    }

    match process_log_path {
        None => Some(module_log_path),
        Some(process_log_path) => {
            let process_log = PathBuf::from(&process_log_path);
            if should_prefer_module_log(&process_log, &module_log) {
                Some(module_log_path)
            } else {
                Some(process_log_path)
            }
        }
    }
}

fn module_runtime_log_is_authoritative(module_id: &str) -> bool {
    module_id.eq_ignore_ascii_case("arksurvivalevolved")
        || module_id.eq_ignore_ascii_case("arksurvivalascended")
        || module_id.eq_ignore_ascii_case("conanexiles")
        || module_id.eq_ignore_ascii_case("runescapedragonwilds")
}

fn should_prefer_module_log(process_log: &Path, module_log: &Path) -> bool {
    let Ok(module_metadata) = fs::metadata(module_log) else {
        return false;
    };
    if module_metadata.len() == 0 {
        return false;
    }
    let Ok(process_metadata) = fs::metadata(process_log) else {
        return true;
    };

    match (
        module_metadata.modified().ok(),
        process_metadata.modified().ok(),
    ) {
        (Some(module_modified), Some(process_modified)) => module_modified >= process_modified,
        (Some(_), None) => true,
        _ => false,
    }
}

fn log_path_has_content(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.len() > 0)
        .unwrap_or(false)
}

fn read_log_snapshot(source_path: Option<String>, max_lines: usize) -> LogTailSnapshot {
    let Some(source_path) = source_path else {
        return LogTailSnapshot {
            source_path: None,
            lines: Vec::new(),
            total_lines: 0,
            truncated: false,
            read_error: None,
        };
    };

    let path = PathBuf::from(&source_path);
    let (content, truncated_by_bytes) = match read_log_tail_window(&path, max_lines) {
        Ok(result) => result,
        Err(error) => {
            return LogTailSnapshot {
                source_path: Some(source_path),
                lines: Vec::new(),
                total_lines: 0,
                truncated: false,
                read_error: Some(error.to_string()),
            };
        }
    };

    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    let normalized = if truncated_by_bytes {
        normalized
            .find('\n')
            .map(|index| normalized[index + 1..].to_string())
            .unwrap_or(normalized)
    } else {
        normalized
    };
    let lines = normalized.lines().map(String::from).collect::<Vec<_>>();
    let total_lines = lines.len();
    let truncated = truncated_by_bytes || total_lines > max_lines;
    let start_index = total_lines.saturating_sub(max_lines.max(1));

    LogTailSnapshot {
        source_path: Some(source_path),
        lines: lines.into_iter().skip(start_index).collect(),
        total_lines,
        truncated,
        read_error: crate::managed_console_log::failure_summary_for_path(&path),
    }
}

pub fn read_log_path_snapshot(source_path: String, max_lines: usize) -> LogTailSnapshot {
    read_log_snapshot(Some(source_path), max_lines.max(1))
}

fn read_log_tail_window(path: &Path, max_lines: usize) -> Result<(String, bool), std::io::Error> {
    if let Some(segments) = crate::managed_console_log::open_log_segments(path)? {
        let limit = log_tail_read_byte_limit(max_lines);
        let expired = segments
            .first()
            .is_some_and(|segment| segment.start_offset > 0);
        let mut remaining = limit;
        let mut chunks = Vec::new();
        let mut truncated = expired;
        for mut segment in segments.into_iter().rev() {
            let length = segment.file.metadata()?.len();
            let count = length.min(remaining);
            truncated |= count < length;
            segment.file.seek(SeekFrom::Start(length - count))?;
            let mut bytes = Vec::new();
            segment.file.take(count).read_to_end(&mut bytes)?;
            remaining = remaining.saturating_sub(bytes.len() as u64);
            chunks.push(bytes);
        }
        let bytes = chunks.into_iter().rev().flatten().collect::<Vec<_>>();
        return Ok((String::from_utf8_lossy(&bytes).into_owned(), truncated));
    }
    let mut file = fs::File::open(path)?;
    let file_len = file.metadata()?.len();
    let read_limit = log_tail_read_byte_limit(max_lines);

    let mut bytes = Vec::new();
    let truncated = file_len > read_limit;
    if truncated {
        file.seek(SeekFrom::Start(file_len.saturating_sub(read_limit)))?;
    }
    file.take(read_limit).read_to_end(&mut bytes)?;

    Ok((String::from_utf8_lossy(&bytes).into_owned(), truncated))
}

fn log_tail_read_byte_limit(max_lines: usize) -> u64 {
    (max_lines.max(1) as u64)
        .saturating_mul(1024)
        .clamp(LOG_TAIL_MIN_READ_BYTES, LOG_TAIL_MAX_READ_BYTES)
}

fn map_instance_run_row(row: &SqliteRow) -> StoredInstanceRunRow {
    StoredInstanceRunRow {
        run_id: row.get("id"),
        session_id: row
            .try_get::<Option<String>, _>("session_id")
            .ok()
            .flatten(),
        process_key: row
            .try_get::<String, _>("process_key")
            .unwrap_or_else(|_| String::from("main")),
        display_name: row
            .try_get::<String, _>("display_name")
            .unwrap_or_else(|_| String::from("Server")),
        pid: row
            .try_get::<Option<i64>, _>("pid")
            .ok()
            .flatten()
            .map(|value| value.max(0) as u32),
        process_identity: map_process_identity(row),
        status: row
            .try_get::<String, _>("status")
            .unwrap_or_else(|_| String::from("unknown")),
        started_at: row
            .try_get::<Option<String>, _>("started_at")
            .ok()
            .flatten(),
        stopped_at: row
            .try_get::<Option<String>, _>("stopped_at")
            .ok()
            .flatten(),
        exit_code: row
            .try_get::<Option<i64>, _>("exit_code")
            .ok()
            .flatten()
            .map(|value| value as i32),
        crash_flag: row.try_get::<i64, _>("crash_flag").unwrap_or(0) > 0,
        log_path: row.try_get::<Option<String>, _>("log_path").ok().flatten(),
        is_primary: row.try_get::<i64, _>("is_primary").unwrap_or(1) > 0,
    }
}

fn map_active_instance_run_entry(row: &SqliteRow) -> ActiveInstanceRunEntry {
    ActiveInstanceRunEntry {
        instance_id: row.get("instance_id"),
        run_id: row.get("id"),
        session_id: row
            .try_get::<Option<String>, _>("session_id")
            .ok()
            .flatten(),
        process_key: row
            .try_get::<String, _>("process_key")
            .unwrap_or_else(|_| String::from("main")),
        display_name: row
            .try_get::<String, _>("display_name")
            .unwrap_or_else(|_| String::from("Server")),
        pid: row
            .try_get::<Option<i64>, _>("pid")
            .ok()
            .flatten()
            .map(|value| value.max(0) as u32),
        process_identity: map_process_identity(row),
        log_path: row.try_get::<Option<String>, _>("log_path").ok().flatten(),
        is_primary: row.try_get::<i64, _>("is_primary").unwrap_or(1) > 0,
    }
}

fn map_process_identity(row: &SqliteRow) -> Option<ProcessIdentity> {
    let creation_time = row
        .try_get::<Option<String>, _>("process_creation_time")
        .ok()
        .flatten()?
        .parse::<u64>()
        .ok()?;
    let image_path = row
        .try_get::<Option<String>, _>("process_image_path")
        .ok()
        .flatten()
        .filter(|path| !path.trim().is_empty())?;
    Some(ProcessIdentity {
        creation_time,
        image_path,
    })
}

#[cfg(test)]
#[path = "runtime_health_tests.rs"]
mod health_tests;

#[cfg(test)]
#[path = "runtime_palworld_health_tests.rs"]
mod palworld_health_tests;

#[cfg(test)]
#[path = "runtime_nightingale_health_tests.rs"]
mod nightingale_health_tests;

#[cfg(test)]
#[path = "runtime_windrose_health_tests.rs"]
mod windrose_health_tests;

#[cfg(test)]
#[path = "runtime_unturned_health_tests.rs"]
mod unturned_health_tests;

#[cfg(test)]
#[path = "runtime_squad_health_tests.rs"]
mod squad_health_tests;

#[cfg(test)]
#[path = "runtime_soulmask_health_tests.rs"]
mod soulmask_health_tests;

#[cfg(test)]
#[path = "runtime_theforest_health_tests.rs"]
mod theforest_health_tests;

#[cfg(test)]
#[path = "runtime_sonsoftheforest_health_tests.rs"]
mod sonsoftheforest_health_tests;

#[cfg(test)]
mod player_query_tests {
    use super::*;

    fn minecraft_player_query_spec(port_names: &[&str]) -> ModulePlayerQuerySpec {
        ModulePlayerQuerySpec {
            protocol: String::from("minecraft_query"),
            port_names: port_names
                .iter()
                .map(|port_name| String::from(*port_name))
                .collect(),
        }
    }

    #[test]
    fn resolve_player_query_target_accepts_minecraft_query_udp_port() {
        let ports = vec![
            app_core::PortBinding {
                name: String::from("game"),
                protocol: String::from("tcp"),
                port: 25565,
            },
            app_core::PortBinding {
                name: String::from("query"),
                protocol: String::from("udp"),
                port: 25565,
            },
        ];

        assert_eq!(
            resolve_player_query_target(
                "0.0.0.0",
                &minecraft_player_query_spec(&["query"]),
                &ports,
            ),
            Some((String::from("127.0.0.1"), 25565))
        );
    }

    #[test]
    fn resolve_player_query_target_uses_core_keeper_query_offset_port() {
        let player_query = ModulePlayerQuerySpec {
            protocol: String::from("a2s_info"),
            port_names: vec![String::from("query")],
        };
        let ports = vec![
            app_core::PortBinding {
                name: String::from("game"),
                protocol: String::from("udp"),
                port: 27_017,
            },
            app_core::PortBinding {
                name: String::from("query"),
                protocol: String::from("udp"),
                port: 27_018,
            },
        ];

        assert_eq!(
            resolve_player_query_target("0.0.0.0", &player_query, &ports),
            Some((String::from("127.0.0.1"), 27_018))
        );
    }

    #[test]
    fn parse_minecraft_query_basic_response_reads_player_counts() {
        let response = [
            b"\x00\x01\x02\x03\x04Lan world\0SMP\0world\x007\x0032\0".as_slice(),
            &[0xdd, 0x63],
            b"127.0.0.1\0".as_slice(),
        ]
        .concat();

        let queried = parse_minecraft_query_basic_response(&response, [1, 2, 3, 4])
            .expect("basic query response should parse");

        assert_eq!(queried.current_players, 7);
        assert_eq!(queried.max_players, 32);
    }
}
