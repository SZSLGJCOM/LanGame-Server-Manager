use super::*;
use std::collections::HashMap;

type Endpoints = HashMap<(u32, String), HashSet<u16>>;

#[path = "runtime_ark_log_evidence.rs"]
mod log_evidence;

pub(super) fn analyze_ark_maps_runtime_health(
    instance: &InstanceDetails,
) -> Option<app_core::RuntimeHealth> {
    let settings: Value = serde_json::from_str(&instance.settings_json).ok()?;
    if settings
        .get("additional_maps")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
        || !matches!(
            instance.summary.status,
            InstanceStatus::Running | InstanceStatus::Starting
        )
    {
        return None;
    }
    let endpoints = match observe_endpoints() {
        Ok(endpoints) => endpoints,
        Err(error) => {
            return Some(health(
                "warning",
                "log_read_failed",
                format!("ARK map endpoints could not be inspected: {error}"),
            ));
        }
    };
    Some(analyze_maps_with(instance, &endpoints, current_native_log))
}

fn analyze_maps_with(
    instance: &InstanceDetails,
    endpoints: &Endpoints,
    mut log_is_current: impl FnMut(&InstanceProcessState, &Path) -> bool,
) -> app_core::RuntimeHealth {
    let maps = match app_core::ark_maps::processes(instance) {
        Ok(maps) => maps,
        Err(error) => return health("error", "latest_run_failed", error),
    };
    let Some(active) = instance.active_run.as_ref() else {
        return health(
            "starting",
            "starting_waiting_logs",
            "Waiting for the ARK map processes to start.",
        );
    };
    let enabled_keys = maps
        .iter()
        .map(|map| map.process_key.clone())
        .collect::<Vec<_>>();
    let mut pending = Vec::new();
    for map in &maps {
        let matching = active
            .processes
            .iter()
            .filter(|process| process.process_key == map.process_key)
            .collect::<Vec<_>>();
        let [process] = matching.as_slice() else {
            pending.push(map.display_name.clone());
            continue;
        };
        if process.crash_flag || process.exit_code.is_some_and(|code| code != 0) {
            return health(
                "error",
                "latest_run_failed",
                format!("ARK map {} exited with an error.", map.display_name),
            );
        }
        let Some(pid) = process.pid.filter(|_| process.status == "running") else {
            pending.push(map.display_name.clone());
            continue;
        };
        let projected = match app_core::ark_maps::project_process(instance, Some(&map.process_key))
        {
            Ok(projected) => projected,
            Err(error) => return health("error", "latest_run_failed", error),
        };
        let path = Path::new(&map.native_log_path);
        if !log_is_current(process, path) {
            pending.push(map.display_name.clone());
            continue;
        }
        let log = read_log_snapshot(
            Some(map.native_log_path.clone()),
            RUNTIME_HEALTH_SCAN_LINE_LIMIT,
        );
        if let Some(error) = log.read_error {
            return health(
                "warning",
                "log_read_failed",
                format!(
                    "ARK map {} log could not be read: {error}",
                    map.display_name
                ),
            );
        }
        // A stable native log can retain a prior run's failure. Keep unparsed
        // diagnostics conservative, but exclude timestamped lines from before
        // the current registered process.
        let current_lines = log
            .lines
            .iter()
            .filter(|line| {
                native_line_timestamp(line).is_none() || native_line_is_current(process, line)
            })
            .cloned()
            .collect::<Vec<_>>();
        if let Some(line) =
            runtime_fatal_log_lines(&instance.summary.module_id, &current_lines).pop()
        {
            let mut failed = health(
                "error",
                "fatal_log_pattern",
                format!(
                    "ARK map {} reported a fatal runtime error.",
                    map.display_name
                ),
            );
            failed.matched_line = Some(line);
            return failed;
        }
        let settings: Value = match serde_json::from_str(&projected.settings_json) {
            Ok(settings) => settings,
            Err(error) => return health("error", "latest_run_failed", error.to_string()),
        };
        let required: &[(&str, &str)] = if instance.summary.module_id == "arksurvivalevolved" {
            &[("game", "udp"), ("query", "udp")]
        } else {
            &[("game", "udp")]
        };
        let ports_ready = required
            .iter()
            .all(|(name, protocol)| owns_binding(&projected.ports, endpoints, pid, name, protocol))
            && (settings.get("rcon_enabled").and_then(Value::as_bool) != Some(true)
                || owns_binding(&projected.ports, endpoints, pid, "rcon", "tcp"));
        let native_ready = if instance.summary.module_id == "arksurvivalevolved" {
            true
        } else {
            match log_evidence::observe(
                path,
                process,
                active.run_id,
                &enabled_keys,
                has_current_ready_line(process, &log.lines),
            ) {
                Ok(ready) => ready,
                Err(error) => {
                    return health(
                        "warning",
                        "log_read_failed",
                        format!(
                            "ARK map {} readiness evidence could not be inspected: {error}",
                            map.display_name
                        ),
                    );
                }
            }
        };
        if !ports_ready || !native_ready {
            pending.push(map.display_name.clone());
        }
    }
    if pending.is_empty() {
        health(
            "ready",
            "ready_signal",
            format!("All {} ARK map servers are ready for players.", maps.len()),
        )
    } else {
        health(
            "starting",
            "starting_tasks",
            format!("Waiting for ARK map servers: {}.", pending.join(", ")),
        )
    }
}

fn health(status: &str, reason: &str, summary: impl Into<String>) -> app_core::RuntimeHealth {
    let summary = summary.into();
    app_core::RuntimeHealth {
        status: status.into(),
        reason: if reason == "log_read_failed" {
            runtime_health_reason(reason, &[("error", summary.clone())])
        } else {
            runtime_health_reason(reason, &[])
        },
        summary,
        matched_line: None,
    }
}

pub(super) fn native_line_is_current(process: &InstanceProcessState, line: &str) -> bool {
    // Registration follows the native endpoint gate and may be later than the
    // ready line. Only the captured OS creation token marks the actual run.
    let Some(started) = process_creation_unix_millis(process) else {
        return false;
    };
    native_line_timestamp(line).is_some_and(|observed| observed >= started)
}

fn has_current_ready_line(process: &InstanceProcessState, lines: &[String]) -> bool {
    lines.iter().any(|line| {
        let normalized = line.to_ascii_lowercase();
        (normalized.contains("has successfully started")
            || normalized.contains("advertising for join"))
            && native_line_is_current(process, line)
    })
}

pub(super) fn native_line_timestamp(line: &str) -> Option<u64> {
    let native = line.strip_prefix('[').and_then(|value| value.get(..24))?;
    let bytes = native.as_bytes();
    if !bytes.iter().enumerate().all(|(index, byte)| match index {
        4 | 7 | 13 | 16 => *byte == b'.',
        10 => *byte == b'-',
        19 => *byte == b':',
        23 => *byte == b']',
        _ => byte.is_ascii_digit(),
    }) {
        return None;
    }
    let number = |start: usize, end: usize| native[start..end].parse::<u64>().ok();
    let (year, month, day) = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
    let (hour, minute, second, millis) = (
        number(11, 13)?,
        number(14, 16)?,
        number(17, 19)?,
        number(20, 23)?,
    );
    if year < 1970 || !(1..=12).contains(&month) || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days_in_month = match month {
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if day == 0 || day > days_in_month {
        return None;
    }
    // Gregorian civil dates use March as the first month so each leap day
    // belongs to the end of its 400-year era. 719468 aligns that era to Unix.
    let adjusted_year = year as i64 - i64::from(month <= 2);
    let era = adjusted_year / 400;
    let year_of_era = adjusted_year - era * 400;
    let march_month = month as i64 + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * march_month + 2) / 5 + day as i64 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let unix_days = u64::try_from(era * 146_097 + day_of_era - 719_468).ok()?;
    Some(((unix_days * 24 + hour) * 60 * 60 + minute * 60 + second) * 1000 + millis)
}

fn process_creation_unix_millis(process: &InstanceProcessState) -> Option<u64> {
    const WINDOWS_UNIX_EPOCH_TICKS: u64 = 116_444_736_000_000_000;
    process
        .process_identity
        .as_ref()?
        .creation_time
        .checked_sub(WINDOWS_UNIX_EPOCH_TICKS)
        // Native timestamps have millisecond precision; truncate the finer OS
        // token to the same unit rather than discarding the entire second.
        .map(|ticks| ticks / 10_000)
}

fn owns_binding(
    ports: &[PortBinding],
    endpoints: &Endpoints,
    pid: u32,
    name: &str,
    protocol: &str,
) -> bool {
    ports
        .iter()
        .find(|port| port.name == name && port.protocol.eq_ignore_ascii_case(protocol))
        .is_some_and(|port| {
            endpoints
                .get(&(pid, protocol.into()))
                .is_some_and(|owned| owned.contains(&port.port))
        })
}

fn current_native_log(process: &InstanceProcessState, path: &Path) -> bool {
    // Native log names are reused. File age alone cannot prove readiness;
    // bind the output to the creation token of this registered process.
    let Some(started_millis) = process_creation_unix_millis(process) else {
        return false;
    };
    let started_seconds = started_millis / 1000;
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .is_ok_and(|modified| {
            modified
                .duration_since(UNIX_EPOCH)
                .is_ok_and(|age| age.as_secs() >= started_seconds)
        })
}

#[cfg(windows)]
fn observe_endpoints() -> Result<Endpoints, String> {
    // Inspect once for the whole group; querying netstat once per map would
    // multiply a polling operation by the cluster size.
    let output = Command::new("netstat")
        .args(["-ano"])
        .output()
        .map_err(|error| format!("failed to run netstat: {error}"))?;
    if !output.status.success() {
        return Err(format!("netstat exited with status {}", output.status));
    }
    Ok(parse_endpoints(&String::from_utf8_lossy(&output.stdout)))
}

#[cfg(not(windows))]
fn observe_endpoints() -> Result<Endpoints, String> {
    Err("ARK native endpoint observation requires Windows".into())
}

fn parse_endpoints(text: &str) -> Endpoints {
    let mut result = Endpoints::new();
    for line in text.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 4 {
            continue;
        }
        let protocol = fields[0].to_ascii_lowercase();
        if protocol != "udp" && protocol != "tcp" {
            continue;
        }
        let Some(pid) = fields.last().and_then(|value| value.parse::<u32>().ok()) else {
            continue;
        };
        let Some(port) = fields[1]
            .rsplit_once(':')
            .and_then(|(_, port)| port.parse::<u16>().ok())
        else {
            continue;
        };
        if protocol == "tcp"
            && fields
                .get(3)
                .is_none_or(|state| !state.eq_ignore_ascii_case("LISTENING"))
        {
            continue;
        }
        result.entry((pid, protocol)).or_default().insert(port);
    }
    result
}

pub(super) fn analyze_ark_evolved_runtime_health(
    active_run: Option<&ActiveInstanceRun>,
    ports: &[PortBinding],
) -> Option<app_core::RuntimeHealth> {
    let pid = active_run.and_then(|run| run.pid)?;
    let required_ports = ark_evolved_required_udp_ports(ports);
    if required_ports.is_empty() {
        return None;
    }

    let owned_udp_ports = owned_udp_ports_for_pid(pid).ok()?;
    let mut bound_ports = Vec::new();
    let mut missing_ports = Vec::new();

    for port in required_ports {
        if owned_udp_ports.contains(&port) {
            bound_ports.push(port);
        } else {
            missing_ports.push(port);
        }
    }

    if missing_ports.is_empty() {
        return Some(app_core::RuntimeHealth {
            status: String::from("ready"),
            summary: format!(
                "ARK: Survival Evolved has bound its required UDP ports ({}) and is ready for players.",
                format_port_list(&bound_ports)
            ),
            reason: runtime_health_reason(
                "ark_udp_ready",
                &[("ports", format_port_list(&bound_ports))],
            ),
            matched_line: None,
        });
    }

    if bound_ports.is_empty() {
        return None;
    }

    Some(app_core::RuntimeHealth {
        status: String::from("starting"),
        summary: format!(
            "ARK: Survival Evolved has bound UDP port{} {} and is still waiting on {}.",
            if bound_ports.len() == 1 { "" } else { "s" },
            format_port_list(&bound_ports),
            format_port_list(&missing_ports)
        ),
        reason: runtime_health_reason(
            "ark_udp_pending",
            &[
                ("bound", format_port_list(&bound_ports)),
                ("missing", format_port_list(&missing_ports)),
            ],
        ),
        matched_line: None,
    })
}

#[cfg(test)]
#[path = "runtime_ark_readiness_tests.rs"]
mod tests;
