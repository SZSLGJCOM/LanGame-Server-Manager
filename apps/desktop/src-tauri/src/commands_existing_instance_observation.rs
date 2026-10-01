//! Test-only bounded observation. Receipts retain counts and safe reason codes,
//! never window titles, game output, launch arguments, or query response bodies.
use super::{ExistingClient, InstanceDetails, InstanceRuntimeOverview, RuntimeWindowSnapshot};
use serde::Serialize;
use serde_json::json;
use std::future::Future;
use std::time::{Duration, Instant};

const OBSERVATION_SECONDS: u64 = 30;
const SAMPLE_TIMEOUT_SECONDS: u64 = 5;

#[derive(Default, Serialize)]
pub(super) struct WindowEvidence {
    samples: usize,
    tracked_samples: usize,
    unpublished_samples: usize,
    inspection_failures: usize,
    max_visible_window_count: usize,
}

impl WindowEvidence {
    pub(super) fn sound(&self) -> bool {
        self.inspection_failures == 0 && self.max_visible_window_count == 0
    }

    fn record(&mut self, snapshot: Result<RuntimeWindowSnapshot, String>) {
        self.samples += 1;
        match snapshot {
            Ok(window) => {
                self.max_visible_window_count =
                    self.max_visible_window_count.max(window.windows.len());
                if window.inspected_process_count > 0 {
                    self.tracked_samples += 1;
                    if !matches!(window.status.as_str(), "clear" | "detected") {
                        self.inspection_failures += 1;
                    }
                } else if window.status == "stopped" {
                    // The API cannot inspect a bootstrap before active-run
                    // publication. Count that evidence gap explicitly.
                    self.unpublished_samples += 1;
                } else {
                    self.inspection_failures += 1;
                }
            }
            Err(_) => self.inspection_failures += 1,
        }
    }
}

pub(super) async fn watch_windows<T>(
    client: &ExistingClient,
    id: &str,
    evidence: &mut WindowEvidence,
    future: impl Future<Output = Result<T, String>>,
) -> Result<T, String> {
    tokio::pin!(future);
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            result = &mut future => return result,
            _ = interval.tick() => {
                let snapshot = tokio::time::timeout(
                    Duration::from_secs(SAMPLE_TIMEOUT_SECONDS),
                    super::read(client, "read_instance_runtime_window_snapshot", json!({"instanceId": id})),
                ).await.unwrap_or_else(|_| Err("window_sample_timed_out".into()));
                evidence.record(snapshot);
            }
        }
    }
}

#[derive(Default, Serialize)]
pub(super) struct SteadyEvidence {
    required_ms: u64,
    elapsed_ms: u128,
    samples: usize,
    startup_samples: usize,
    startup_wait_ms: u128,
    pub(super) passed: bool,
    failure: Option<String>,
}

pub(super) async fn observe_steady(
    client: &ExistingClient,
    expected: &InstanceDetails,
    readiness_deadline: Instant,
    evidence: &mut SteadyEvidence,
) -> Result<(), String> {
    let began = Instant::now();
    evidence.required_ms = OBSERVATION_SECONDS * 1000;
    let mut readiness = ReadinessWindow {
        deadline: readiness_deadline,
        ready_since: None,
    };
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        let sample = tokio::time::timeout(
            Duration::from_secs(SAMPLE_TIMEOUT_SECONDS),
            steady_sample(client, expected),
        )
        .await
        .unwrap_or(Err("steady_sample_timed_out"));
        evidence.elapsed_ms = began.elapsed().as_millis();
        let ready = match sample {
            Ok(ready) => ready,
            Err(reason) => {
                evidence.failure = Some(reason.into());
                return Err(reason.into());
            }
        };
        if ready {
            evidence.samples += 1;
        } else {
            evidence.startup_samples += 1;
        }
        match readiness.observe(ready, Instant::now()) {
            Ok(true) => {
                evidence.passed = true;
                return Ok(());
            }
            Ok(false) => {}
            Err(reason) => {
                evidence.failure = Some(reason.into());
                return Err(reason.into());
            }
        }
        evidence.startup_wait_ms = readiness
            .ready_since
            .unwrap_or_else(Instant::now)
            .saturating_duration_since(began)
            .as_millis();
    }
}

// Port readiness can precede level loading. Wait within the declared startup
// budget, then require a full uninterrupted ready window; never reset on loss.
struct ReadinessWindow {
    deadline: Instant,
    ready_since: Option<Instant>,
}

impl ReadinessWindow {
    fn observe(&mut self, ready: bool, now: Instant) -> Result<bool, &'static str> {
        if let Some(since) = self.ready_since {
            if !ready {
                return Err("steady_health_not_ready");
            }
            return Ok(
                now.saturating_duration_since(since) >= Duration::from_secs(OBSERVATION_SECONDS)
            );
        }
        if now >= self.deadline {
            return Err("startup_health_readiness_timed_out");
        }
        if ready {
            self.ready_since = Some(now);
        }
        Ok(false)
    }
}

async fn steady_sample(
    client: &ExistingClient,
    expected: &InstanceDetails,
) -> Result<bool, &'static str> {
    let id = &expected.summary.id;
    let actual: InstanceDetails = super::read(
        client,
        "read_instance_details_from_storage",
        json!({"instanceId": id}),
    )
    .await
    .map_err(|_| "steady_instance_readback_failed")?;
    if !same_active_run(expected, &actual) {
        return Err("steady_active_run_changed_or_exited");
    }
    for (pid, identity) in super::process_identities(expected) {
        if app_runtime::inspect_process_identity(pid)
            .map_err(|_| "steady_process_identity_unavailable")?
            .as_ref()
            != Some(&identity)
        {
            return Err("steady_process_identity_changed_or_exited");
        }
    }
    let overview: InstanceRuntimeOverview = super::read(
        client,
        "read_instance_runtime_overview_from_storage",
        json!({"instanceId": id}),
    )
    .await
    .map_err(|_| "steady_health_readback_failed")?;
    let ready = match overview.health.status.as_str() {
        "ready" => true,
        "starting" => false,
        _ => return Err("steady_health_failed"),
    };
    let window: RuntimeWindowSnapshot = super::read(
        client,
        "read_instance_runtime_window_snapshot",
        json!({"instanceId": id}),
    )
    .await
    .map_err(|_| "steady_window_readback_failed")?;
    if !window.windows.is_empty() {
        return Err("steady_visible_window_detected");
    }
    if window.status != "clear" || window.inspected_process_count == 0 {
        return Err("steady_window_inspection_unavailable");
    }
    Ok(ready)
}

fn same_active_run(expected: &InstanceDetails, actual: &InstanceDetails) -> bool {
    let (Some(expected_run), Some(actual_run)) = (&expected.active_run, &actual.active_run) else {
        return false;
    };
    let expected_identities = super::process_identities(expected);
    let actual_identities = super::process_identities(actual);
    matches!(actual.summary.status, super::InstanceStatus::Running)
        && expected_run.run_id == actual_run.run_id
        && expected_run.session_id == actual_run.session_id
        && !expected_identities.is_empty()
        && expected_identities.len() == actual_run.processes.len()
        && actual_identities.len() == expected_identities.len()
        && expected_identities
            .iter()
            .all(|identity| actual_identities.contains(identity))
        && actual_run
            .processes
            .iter()
            .all(|process| process.status == "running")
}

#[derive(Serialize)]
pub(super) struct ReadinessFailure {
    code: &'static str,
    pending_probes: Vec<String>,
}

impl ReadinessFailure {
    pub(super) fn from_reason(reason: &str) -> Self {
        let mut failure = Self {
            code: "native_probe_failed",
            pending_probes: Vec::new(),
        };
        if let Some(pending) = reason.strip_prefix("native readiness timed out; pending probes: ") {
            failure.code = "native_readiness_timed_out";
            // Only the fixed probe-ID portion is safe to retain. Subsequent
            // query errors can contain a native response or private address.
            if let Some((ids, _)) = pending.split_once("; player query: ") {
                failure.pending_probes = ids
                    .split(", ")
                    .take(128)
                    .filter(|id| {
                        !id.is_empty()
                            && id.len() <= 128
                            && id.bytes().all(|byte| {
                                byte.is_ascii_alphanumeric()
                                    || matches!(byte, b'-' | b'_' | b':' | b'/')
                            })
                    })
                    .map(str::to_owned)
                    .collect();
            }
        } else {
            failure.code = match reason {
                "managed process exited before native readiness completed" => {
                    "native_process_exited"
                }
                "declared primary process has no stable managed identity" => {
                    "native_primary_identity_missing"
                }
                "process identity check failed" => "native_process_identity_unavailable",
                "native log cannot be read" => "native_log_unreadable",
                "HumanitZ info probe requires RCON enabled" => "native_rcon_disabled",
                "HumanitZ info probe requires a generated RCON password" => {
                    "native_rcon_password_missing"
                }
                _ => "native_probe_failed",
            };
        }
        failure
    }
}

#[test]
fn existing_acceptance_readiness_diagnostic_excludes_native_response_data() {
    let failure = ReadinessFailure::from_reason(
        "native readiness timed out; pending probes: game-endpoint, map-2:rcon/tcp; player query: [password=fixture-password]; owned declared endpoints: [127.0.0.1]",
    );
    assert_eq!(failure.code, "native_readiness_timed_out");
    assert_eq!(failure.pending_probes, ["game-endpoint", "map-2:rcon/tcp"]);
    let serialized = serde_json::to_string(&failure).unwrap();
    assert!(!serialized.contains("fixture-password"));
    assert!(!serialized.contains("127.0.0.1"));
    assert_eq!(
        ReadinessFailure::from_reason("unrecognized password=secret").code,
        "native_probe_failed"
    );
}

#[test]
fn existing_acceptance_window_history_cannot_be_cleared_by_a_later_sample() {
    let snapshot = |status: &str, inspected_process_count| RuntimeWindowSnapshot {
        instance_id: "fixture".into(),
        observed_at_unix_ms: 0,
        status: status.into(),
        summary: String::new(),
        inspected_process_count,
        last_suppression_attempt: None,
        windows: Vec::new(),
    };
    let mut evidence = WindowEvidence::default();
    evidence.record(Ok(snapshot("stopped", 0)));
    assert_eq!(evidence.unpublished_samples, 1);
    let mut visible = snapshot("detected", 1);
    visible.windows.push(app_core::RuntimeWindowSurface {
        process_key: "main".into(),
        display_name: String::new(),
        relation: "self".into(),
        pid: 100,
        process_name: String::new(),
        window_handle: "fixture".into(),
        title: "private text must not be retained".into(),
        class_name: String::new(),
    });
    evidence.record(Ok(visible));
    evidence.record(Ok(snapshot("clear", 1)));
    assert!(!evidence.sound());
    assert_eq!(evidence.max_visible_window_count, 1);
    assert!(
        !serde_json::to_string(&evidence)
            .unwrap()
            .contains("private text")
    );
    let mut unavailable = WindowEvidence::default();
    unavailable.record(Ok(snapshot("unavailable", 0)));
    unavailable.record(Ok(snapshot("clear", 1)));
    assert!(!unavailable.sound());
}

#[test]
fn existing_acceptance_waits_for_real_health_before_measuring_steady_window() {
    let now = Instant::now();
    let mut state = ReadinessWindow {
        deadline: now + Duration::from_secs(180),
        ready_since: None,
    };
    assert_eq!(state.observe(false, now), Ok(false));
    assert_eq!(
        state.observe(false, now + Duration::from_secs(60)),
        Ok(false)
    );
    assert_eq!(
        state.observe(true, now + Duration::from_secs(90)),
        Ok(false)
    );
    assert_eq!(
        state.observe(true, now + Duration::from_secs(119)),
        Ok(false)
    );
    assert_eq!(
        state.observe(true, now + Duration::from_secs(120)),
        Ok(true)
    );
}

#[test]
fn existing_acceptance_startup_budget_and_ready_loss_never_reset() {
    let now = Instant::now();
    let mut state = ReadinessWindow {
        deadline: now + Duration::from_secs(180),
        ready_since: None,
    };
    assert_eq!(
        state.observe(false, now + Duration::from_secs(180)),
        Err("startup_health_readiness_timed_out")
    );
    assert_eq!(
        state.observe(true, now + Duration::from_secs(181)),
        Err("startup_health_readiness_timed_out")
    );
    let mut state = ReadinessWindow {
        deadline: now + Duration::from_secs(180),
        ready_since: None,
    };
    assert_eq!(state.observe(true, now), Ok(false));
    assert_eq!(
        state.observe(false, now + Duration::from_secs(5)),
        Err("steady_health_not_ready")
    );
    assert_eq!(state.ready_since, Some(now));
}
