//! Prepare ASTRONEER's advertised address before its existing configuration render.
use std::future::Future;
use std::net::Ipv4Addr;
use std::time::Duration;

use app_core::{InstanceDetails, InstanceStatus, UpdateInstanceInput};
use app_storage::StorageBootstrap;
use serde_json::{Map, Value};

use crate::state::RuntimeStartReservationLease;

const DISCOVERY_URL: &str = "https://api.ipify.org";
const DISCOVERY_BUDGET: Duration = Duration::from_secs(8);
const MAX_RESPONSE_BYTES: usize = 64;
const INVALID_ADDRESS: &str = "ASTRONEER PublicIP must be a public unicast IPv4 address. Clear PublicIP to discover it automatically on the next start, or enter this server's public IPv4 address.";
const DISCOVERY_FAILED: &str = "ASTRONEER could not discover a public IPv4 address within eight seconds. Check the internet connection and access to api.ipify.org, or enter this server's public IPv4 address in PublicIP and start again.";
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

fn address_setting(settings: &Map<String, Value>) -> Result<Option<Ipv4Addr>, String> {
    match settings.get("public_ip") {
        None => Ok(None),
        Some(Value::String(value)) if value.trim().is_empty() => Ok(None),
        Some(Value::String(value)) => parse_public_address(value).map(Some),
        Some(_) => Err(INVALID_ADDRESS.into()),
    }
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
        .redirect(reqwest::redirect::Policy::none())
        .https_only(true)
        .connect_timeout(Duration::from_secs(3))
        .timeout(DISCOVERY_BUDGET)
        .build()
        .map_err(|_| DISCOVERY_FAILED.into())
}

async fn discover() -> Result<Ipv4Addr, String> {
    let client = discovery_client()?;
    let request = client
        .get(DISCOVERY_URL)
        .build()
        .map_err(|_| DISCOVERY_FAILED.to_owned())?;
    // Existing GET header retries share this one deadline with the entire body.
    // No proxy, redirect, alternate origin, credential, or response body is logged.
    let response = app_network::read_public_bytes_from_source(
        &client,
        request,
        DISCOVERY_BUDGET,
        MAX_RESPONSE_BYTES,
    )
    .await
    .map_err(|_| DISCOVERY_FAILED.to_owned())?;
    parse_discovery(&response.bytes)
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
    prepare_with_discovery(storage, reservation, instance, discover()).await
}

// Kept in this startup adapter so tests replace only the external network fact;
// the real storage read, settings CAS, render and returned snapshot stay in use.
pub(crate) async fn prepare_with_discovery(
    storage: &StorageBootstrap,
    reservation: &RuntimeStartReservationLease,
    instance: InstanceDetails,
    discovery: impl Future<Output = Result<Ipv4Addr, String>>,
) -> Result<InstanceDetails, String> {
    if instance.summary.module_id != "astroneer" {
        return Ok(instance);
    }
    ensure_not_cancelled(reservation)?;
    ensure_quiescent(&instance)?;
    let mut prepared_settings = settings(&instance)?;
    if address_setting(&prepared_settings)?.is_some() {
        return Ok(instance);
    }
    let address = tokio::select! {
        biased;
        _ = reservation.cancelled() => return Err(CANCELLED.into()),
        value = discovery => value?,
    };
    // Revalidate even injected test results. Never replace failed discovery with
    // a local route address, and never mutate settings on a discovery error.
    if !is_public_unicast(address) {
        return Err(DISCOVERY_FAILED.into());
    }
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
    prepared_settings.insert("public_ip".into(), Value::String(address.to_string()));
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

/// Test-harness readback accepts only the startup-owned field mutation; every
/// other setting must remain identical. An explicit address must be reused.
#[cfg(test)]
pub(crate) fn verify_startup_settings(before: &str, after: &str) -> Result<bool, String> {
    let mut expected: Map<String, Value> =
        serde_json::from_str(before).map_err(|_| CHANGED.to_owned())?;
    let actual: Map<String, Value> = serde_json::from_str(after).map_err(|_| CHANGED.to_owned())?;
    let discovered = address_setting(&expected)?.is_none();
    address_setting(&actual)?.ok_or_else(|| INVALID_ADDRESS.to_owned())?;
    if discovered {
        expected.insert("public_ip".into(), actual["public_ip"].clone());
    }
    if expected != actual {
        return Err(CHANGED.into());
    }
    Ok(discovered)
}

#[cfg(test)]
#[path = "astroneer_public_ip_tests.rs"]
mod tests;
