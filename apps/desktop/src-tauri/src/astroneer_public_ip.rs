//! Prepare ASTRONEER's advertised address before its existing configuration render.
use std::future::Future;
use std::net::Ipv4Addr;
use std::time::Duration;

use app_core::{InstanceDetails, InstanceStatus, UpdateInstanceInput};
use app_platform_win::{BindAddressCandidate, WindowsPlatform};
use app_storage::StorageBootstrap;
use serde_json::{Map, Value};

use crate::state::RuntimeStartReservationLease;

const DISCOVERY_SOURCES: [(&str, &str); 2] = [
    ("ipify", "https://api.ipify.org"),
    ("aws", "https://checkip.amazonaws.com"),
];
const DISCOVERY_BUDGET: Duration = Duration::from_secs(8);
const MAX_RESPONSE_BYTES: usize = 64;
const INVALID_ADDRESS: &str = "ASTRONEER PublicIP must be a usable IPv4 address. Select or enter the player join address in Network settings. LAN and VPN addresses are allowed; address validation does not establish registration or client reachability.";
const DISCOVERY_FAILED: &str = "ASTRONEER could not discover its outbound public IPv4 address. Check access to the address lookup services, or configure PublicIP with the IPv4 address players will use. Lookup failure does not establish whether inbound connections are possible. Server updates and launch have not started.";
const CANCELLED: &str = "ASTRONEER PublicIP preparation was cancelled before server launch.";
const CHANGED: &str = "The ASTRONEER instance changed while preparing PublicIP. Start it again using its current settings.";

/// Address-space eligibility, not proof of host ownership or backend registration.
/// Stable Rust 1.99 cannot call the unstable Ipv4Addr::is_global. This finite
/// predicate follows its fixed b940084d7 table, additionally excluding multicast
/// and IANA's non-global 192.88.99.2. IANA leaves the deprecated /24's other
/// addresses unspecified; they are not silently treated as an extra policy.
pub(crate) fn is_public_unicast(address: Ipv4Addr) -> bool {
    let [a, b, c, d] = address.octets();
    !(a == 0
        || a == 10
        || (a == 100 && (64..=127).contains(&b))
        || a == 127
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 0 && c == 0 && d != 9 && d != 10)
        || (a == 192 && b == 0 && c == 2)
        || (a == 192 && b == 88 && c == 99 && d == 2)
        || (a == 192 && b == 168)
        || (a == 198 && (b == 18 || b == 19))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
        || a >= 224)
}

pub(crate) fn parse_public_address(value: &str) -> Result<Ipv4Addr, String> {
    value
        .parse::<Ipv4Addr>()
        .ok()
        .filter(|address| is_public_unicast(*address))
        .ok_or_else(|| INVALID_ADDRESS.into())
}

// LAN and VPN addresses are legitimate advertised endpoints. Keep Internet
// discovery stricter: an external lookup service must still return a public IP.
pub(crate) fn parse_advertised_address(value: &str) -> Result<Ipv4Addr, String> {
    value
        .parse::<Ipv4Addr>()
        .ok()
        .filter(|address| {
            let [a, b, _, _] = address.octets();
            is_public_unicast(*address)
                || address.is_private()
                || (a == 100 && (64..=127).contains(&b))
        })
        .ok_or_else(|| INVALID_ADDRESS.into())
}

fn address_setting(settings: &Map<String, Value>) -> Result<Option<Ipv4Addr>, String> {
    match settings.get("public_ip") {
        None => Ok(None),
        Some(Value::String(value)) if value.trim().is_empty() => Ok(None),
        Some(Value::String(value)) => parse_advertised_address(value)
            .map(Some)
            .map_err(|_| preflight_error("invalid_public_ip", &[])),
        Some(_) => Err(preflight_error("invalid_public_ip", &[])),
    }
}

// Internal source metadata; the UI has one player join address control.
// Public with an empty address means automatic selection, not a user mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectionMode {
    Public,
    Radmin,
    Manual,
}

fn connection_mode(
    settings: &Map<String, Value>,
    address: Option<Ipv4Addr>,
) -> Result<ConnectionMode, String> {
    match settings.get("connection_mode") {
        // Retain pre-mode addresses without assuming how players reach them.
        None if address.is_some() => Ok(ConnectionMode::Manual),
        None => Ok(ConnectionMode::Public),
        Some(Value::String(mode)) if mode == "public" => Ok(ConnectionMode::Public),
        Some(Value::String(mode)) if mode == "radmin" => Ok(ConnectionMode::Radmin),
        Some(Value::String(mode)) if mode == "manual" => Ok(ConnectionMode::Manual),
        Some(_) => Err(preflight_error("invalid_connection_mode", &[])),
    }
}

fn is_radmin_candidate(candidate: &BindAddressCandidate) -> bool {
    candidate.kind == "overlay" && candidate.family_name.as_deref() == Some("Radmin VPN")
}

fn select_radmin_address(
    candidates: &[BindAddressCandidate],
    configured: Option<Ipv4Addr>,
) -> Result<Ipv4Addr, String> {
    let mut addresses = candidates
        .iter()
        .filter(|candidate| is_radmin_candidate(candidate))
        .filter_map(|candidate| parse_advertised_address(&candidate.address).ok());
    if let Some(address) = configured {
        return addresses
            .any(|candidate| candidate == address)
            .then_some(address)
            .ok_or_else(|| preflight_error("radmin_address_unavailable", &[]));
    }
    let address = addresses
        .next()
        .ok_or_else(|| preflight_error("radmin_unavailable", &[]))?;
    if addresses.any(|candidate| candidate != address) {
        return Err(preflight_error("radmin_address_ambiguous", &[]));
    }
    Ok(address)
}

async fn fresh_bind_address_candidates() -> Result<Vec<BindAddressCandidate>, String> {
    // The UI cache can retain a disconnected adapter. Startup uses a fresh,
    // bounded platform query, without contacting an Internet lookup service.
    tokio::task::spawn_blocking(WindowsPlatform::try_bind_address_candidates)
        .await
        .map_err(|_| preflight_error("adapter_query_failed", &[]))?
        .map_err(|_| preflight_error("adapter_query_failed", &[]))
}

fn settings(instance: &InstanceDetails) -> Result<Map<String, Value>, String> {
    serde_json::from_str(&instance.settings_json)
        .map_err(|_| String::from("ASTRONEER instance settings must be a JSON object."))
}

fn parse_discovery(body: &[u8]) -> Result<Ipv4Addr, String> {
    if body.len() > MAX_RESPONSE_BYTES || !body.is_ascii() {
        return Err(DISCOVERY_FAILED.into());
    }
    let value = std::str::from_utf8(body).map_err(|_| DISCOVERY_FAILED.to_owned())?;
    parse_public_address(value.trim()).map_err(|_| DISCOVERY_FAILED.into())
}

fn discovery_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .no_proxy()
        .local_address(std::net::IpAddr::V4(Ipv4Addr::UNSPECIFIED))
        .redirect(reqwest::redirect::Policy::none())
        .https_only(true)
        .connect_timeout(Duration::from_secs(3))
        .timeout(DISCOVERY_BUDGET)
        .build()
        .map_err(|_| {
            preflight_error(
                "discovery_failed",
                &[DiscoveryFailure::new("ipify", "request", false)],
            )
        })
}

#[derive(Debug, serde::Serialize)]
struct DiscoveryFailure {
    source: &'static str,
    reason: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<u16>,
    #[serde(skip)]
    permits_fallback: bool,
}

impl DiscoveryFailure {
    fn new(source: &'static str, reason: &'static str, permits_fallback: bool) -> Self {
        Self {
            source,
            reason,
            status: None,
            permits_fallback,
        }
    }

    fn network(source: &'static str, error: app_network::NetworkError) -> Self {
        use app_network::NetworkError;
        let permits_fallback = error.permits_source_fallback();
        let (reason, status) = match error {
            NetworkError::Deadline { .. } => ("timeout", None),
            NetworkError::Transport { source, .. } if source.is_timeout() => ("timeout", None),
            NetworkError::Transport { .. } => ("connection", None),
            NetworkError::Status { status, .. } | NetworkError::RetryDeferred { status, .. } => {
                ("http", Some(status.as_u16()))
            }
            NetworkError::BodyTooLarge { .. } => ("response", None),
            NetworkError::Unrepeatable | NetworkError::InvalidSource(_) => ("request", None),
        };
        Self {
            source,
            reason,
            status,
            permits_fallback,
        }
    }
}

fn preflight_error(reason: &str, failures: &[DiscoveryFailure]) -> String {
    let message = match reason {
        "invalid_public_ip" => INVALID_ADDRESS,
        "invalid_connection_mode" => {
            "The saved ASTRONEER address source is invalid. Select the player join address again in Network settings before starting."
        }
        "manual_address_required" => {
            "Enter the player join IPv4 address or select automatic selection in Network settings before starting ASTRONEER."
        }
        "adapter_query_failed" => {
            "ASTRONEER could not read this host's network addresses. Refresh the player join addresses and start again. Public discovery, server updates and launch have not started."
        }
        "radmin_unavailable" => {
            "No eligible active Radmin VPN IPv4 address was found. Connect Radmin VPN and select its address before starting ASTRONEER. Public address discovery was not attempted."
        }
        "radmin_address_unavailable" => {
            "The saved ASTRONEER address is not assigned to an active Radmin VPN adapter. Select the current player join address in Network settings. The saved address was not changed and public discovery was not attempted."
        }
        "radmin_address_ambiguous" => {
            "More than one eligible Radmin VPN IPv4 address was found. Select the address players will use in connection settings before starting ASTRONEER."
        }
        _ => DISCOVERY_FAILED,
    };
    serde_json::json!({
        "code": "astroneer_network_preflight_failed",
        "reason": reason,
        "message": message,
        "failures": failures,
    })
    .to_string()
}

async fn query_source(
    client: reqwest::Client,
    source: &'static str,
    url: &'static str,
    budget: Duration,
) -> Result<Ipv4Addr, DiscoveryFailure> {
    let request = client
        .get(url)
        .build()
        .map_err(|_| DiscoveryFailure::new(source, "request", false))?;
    // Each fixed public service gets a bounded share of the same total budget.
    // No proxy, redirect, credential, address or response body enters diagnostics.
    let response =
        app_network::read_public_bytes_from_source(&client, request, budget, MAX_RESPONSE_BYTES)
            .await
            .map_err(|error| DiscoveryFailure::network(source, error))?;
    parse_discovery(&response.bytes).map_err(|_| DiscoveryFailure::new(source, "response", false))
}

async fn discover_with<F, Q>(mut query: F) -> Result<Ipv4Addr, String>
where
    F: FnMut(&'static str, &'static str, Duration) -> Q,
    Q: Future<Output = Result<Ipv4Addr, DiscoveryFailure>>,
{
    let deadline = tokio::time::Instant::now() + DISCOVERY_BUDGET;
    let mut failures = Vec::new();
    for (index, (source, url)) in DISCOVERY_SOURCES.into_iter().enumerate() {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let budget = remaining / (DISCOVERY_SOURCES.len() - index) as u32;
        let result = tokio::time::timeout(budget, query(source, url, budget))
            .await
            .unwrap_or_else(|_| Err(DiscoveryFailure::new(source, "timeout", true)));
        match result {
            Ok(address) => return Ok(address),
            Err(error) => {
                let permits_fallback = error.permits_fallback;
                failures.push(error);
                if !permits_fallback {
                    break;
                }
            }
        }
    }
    Err(preflight_error("discovery_failed", &failures))
}

async fn discover() -> Result<Ipv4Addr, String> {
    let client = discovery_client()?;
    discover_with(|source, url, budget| query_source(client.clone(), source, url, budget)).await
}

fn ensure_quiescent(instance: &InstanceDetails) -> Result<(), String> {
    if matches!(
        instance.summary.status,
        InstanceStatus::Stopped | InstanceStatus::Error
    ) && instance.summary.active_process_count == 0
        && instance.active_run.is_none()
    {
        Ok(())
    } else {
        Err(CHANGED.into())
    }
}

fn ensure_not_cancelled(reservation: &RuntimeStartReservationLease) -> Result<(), String> {
    if reservation.is_cancelled() {
        Err(CANCELLED.into())
    } else {
        Ok(())
    }
}

pub(crate) async fn prepare_for_start(
    storage: &StorageBootstrap,
    reservation: &RuntimeStartReservationLease,
    instance: InstanceDetails,
) -> Result<InstanceDetails, String> {
    prepare_with_sources(
        storage,
        reservation,
        instance,
        discover(),
        fresh_bind_address_candidates(),
    )
    .await
}

// Kept in this startup adapter so tests replace only external address facts;
// the real storage read, settings CAS, render and returned snapshot stay in use.
pub(crate) async fn prepare_with_sources(
    storage: &StorageBootstrap,
    reservation: &RuntimeStartReservationLease,
    instance: InstanceDetails,
    discovery: impl Future<Output = Result<Ipv4Addr, String>>,
    candidates: impl Future<Output = Result<Vec<BindAddressCandidate>, String>>,
) -> Result<InstanceDetails, String> {
    if instance.summary.module_id != "astroneer" {
        return Ok(instance);
    }
    ensure_not_cancelled(reservation)?;
    ensure_quiescent(&instance)?;
    let mut prepared_settings = settings(&instance)?;
    let configured = address_setting(&prepared_settings)?;
    let (address, source) = match (connection_mode(&prepared_settings, configured)?, configured) {
        (ConnectionMode::Public | ConnectionMode::Manual, Some(_)) => return Ok(instance),
        (ConnectionMode::Manual, None) => {
            return Err(preflight_error("manual_address_required", &[]));
        }
        (ConnectionMode::Public, None) => {
            let candidates = tokio::select! {
                biased;
                _ = reservation.cancelled() => return Err(CANCELLED.into()),
                value = candidates => value?,
            };
            if candidates.iter().any(is_radmin_candidate) {
                (select_radmin_address(&candidates, None)?, "radmin")
            } else {
                let address = tokio::select! {
                    biased;
                    _ = reservation.cancelled() => return Err(CANCELLED.into()),
                    value = discovery => value?,
                };
                // Revalidate injected discovery results too. Local endpoints
                // are accepted only from explicit settings or host adapters.
                if !is_public_unicast(address) {
                    return Err(preflight_error("discovery_failed", &[]));
                }
                (address, "public")
            }
        }
        (ConnectionMode::Radmin, _) => {
            let candidates = tokio::select! {
                biased;
                _ = reservation.cancelled() => return Err(CANCELLED.into()),
                value = candidates => value?,
            };
            (select_radmin_address(&candidates, configured)?, "radmin")
        }
    };
    ensure_not_cancelled(reservation)?;
    let current = app_storage::read_instance_details(&storage.paths, &instance.summary.id)
        .await
        .map_err(|_| CHANGED.to_owned())?;
    ensure_not_cancelled(reservation)?;
    ensure_quiescent(&current)?;
    if serde_json::to_value(&current).map_err(|_| CHANGED.to_owned())?
        != serde_json::to_value(&instance).map_err(|_| CHANGED.to_owned())?
    {
        return Err(CHANGED.into());
    }
    if configured.is_some() {
        return Ok(instance);
    }
    prepared_settings.insert("public_ip".into(), Value::String(address.to_string()));
    // Persist the actual source alongside the address. A subsequent start
    // must recheck an automatically selected Radmin adapter, never switch routes.
    prepared_settings.insert("connection_mode".into(), Value::String(source.into()));
    let input = UpdateInstanceInput {
        id: current.summary.id.clone(),
        bind_ip: current.summary.bind_ip.clone(),
        auto_backup_on_stop: current.auto_backup_on_stop,
        backup_retention_count: current.backup_retention_count,
        settings_json: serde_json::to_string(&prepared_settings).map_err(|_| CHANGED.to_owned())?,
        ports: current.ports.clone(),
    };
    ensure_not_cancelled(reservation)?;
    // The enclosing startup worker owns its instance/program/storage guards.
    // Once this existing transaction begins, it finishes safely on cancellation;
    // a committed address is retained, while launch still rejects cancellation.
    let updated = app_storage::update_instance_if_current(
        &storage.paths,
        input,
        &current.settings_json,
    )
    .await
    .map_err(|_| String::from("ASTRONEER could not persist PublicIP. Check instance storage and start again; no server was launched."))?;
    ensure_not_cancelled(reservation)?;
    Ok(updated)
}

/// Test-harness readback accepts only startup-owned address/default completion;
/// every explicit setting must remain identical.
#[cfg(test)]
pub(crate) fn verify_startup_settings(before: &str, after: &str) -> Result<bool, String> {
    let mut expected: Map<String, Value> =
        serde_json::from_str(before).map_err(|_| CHANGED.to_owned())?;
    let actual: Map<String, Value> = serde_json::from_str(after).map_err(|_| CHANGED.to_owned())?;
    let configured = address_setting(&expected)?;
    let mode = connection_mode(&expected, configured)?;
    let discovered = configured.is_none();
    if discovered && mode == ConnectionMode::Manual {
        return Err(CHANGED.into());
    }
    let actual_address = address_setting(&actual)?.ok_or_else(|| INVALID_ADDRESS.to_owned())?;
    if discovered
        && mode == ConnectionMode::Public
        && actual.get("connection_mode").and_then(Value::as_str) != Some("radmin")
        && !is_public_unicast(actual_address)
    {
        return Err(CHANGED.into());
    }
    if discovered {
        expected.insert("public_ip".into(), actual["public_ip"].clone());
    }
    // Automatic selection may resolve to Radmin or public discovery. Only
    // source completion for a previously blank address is startup-owned.
    if discovered && mode == ConnectionMode::Public {
        if let Some(source) = actual.get("connection_mode").and_then(Value::as_str) {
            if !matches!(source, "public" | "radmin") {
                return Err(CHANGED.into());
            }
            expected.insert("connection_mode".into(), Value::String(source.into()));
        }
    } else if !expected.contains_key("connection_mode")
        && actual.get("connection_mode").and_then(Value::as_str) == Some("manual")
    {
        expected.insert("connection_mode".into(), Value::String("manual".into()));
    }
    if expected != actual {
        return Err(CHANGED.into());
    }
    Ok(discovered)
}

#[cfg(test)]
#[path = "astroneer_public_ip_tests.rs"]
mod tests;
