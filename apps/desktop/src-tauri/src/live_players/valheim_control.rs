//! Authenticated current-process Valheim peers and native access-list readback.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use app_core::{
    InstanceDetails, InstanceProgramMode, ModulePlayerListSource, ProcessIdentity,
    RuntimeLivePlayerEntry, RuntimeLivePlayerIdentifier, RuntimeLivePlayerIssueCode,
    RuntimeLivePlayerSnapshot, RuntimeLivePlayerStatus, RuntimePlayerIdentityKind,
};

use super::cache::{CachedLivePlayerSnapshot, LivePlayerCollectionResult};
use super::service::failed_snapshot;

#[path = "valheim_control_io.rs"]
pub(crate) mod io;
#[path = "valheim_control_response.rs"]
mod response;
#[path = "valheim_control_transport.rs"]
mod transport;

#[cfg(test)]
pub(crate) use response::{AccessLists, ValidatedPeer};
pub(crate) use response::{ControlSnapshot, Operation};

#[derive(Clone)]
pub(super) struct Context {
    pub pid: u32,
    pub identity: ProcessIdentity,
    pub root: PathBuf,
}

fn context(details: &InstanceDetails) -> Result<Context, String> {
    if details.summary.module_id != "valheim" {
        return Err("Valheim control requires a Valheim instance.".into());
    }
    let run = details
        .active_run
        .as_ref()
        .ok_or("The Valheim server is stopped.")?;
    let mut primary = run.processes.iter().filter(|process| {
        process.is_primary && process.status == "running" && process.run_id == run.run_id
    });
    let process = primary
        .next()
        .ok_or("The managed Valheim process is unavailable.")?;
    if primary.next().is_some() {
        return Err("The managed Valheim process is ambiguous.".into());
    }
    let pid = process
        .pid
        .filter(|pid| *pid != 0)
        .ok_or("The managed Valheim PID is missing.")?;
    let identity = process
        .process_identity
        .clone()
        .filter(|identity| identity.creation_time != 0)
        .ok_or("The managed Valheim process identity is missing.")?;
    let image = Path::new(&identity.image_path);
    if !image
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("valheim_server.exe"))
    {
        return Err("The managed image is not the Valheim server.".into());
    }
    let instance_root = Path::new(&details.config_file_path)
        .parent()
        .and_then(Path::parent)
        .ok_or("The Valheim instance configuration directory is missing.")?;
    if app_storage::instance_program_mode(instance_root)
        .map_err(|_| "Cannot resolve the Valheim program ownership.")?
        != InstanceProgramMode::Independent
    {
        return Err("Valheim control requires an independent program directory.".into());
    }
    let root = app_storage::resolve_instance_runtime_root(instance_root)
        .map_err(|_| "Cannot resolve the Valheim program directory.")?;
    let _guards = io::pin_directories(&root)?;
    let image_root = image
        .parent()
        .ok_or("The managed Valheim image has no directory.")?;
    let _image_guards = io::pin_directories(image_root)?;
    let metadata = std::fs::symlink_metadata(image)
        .map_err(|_| "Cannot inspect the managed Valheim image.")?;
    if !metadata.is_file() || io::reparse(&metadata) {
        return Err("The managed Valheim image must be an ordinary file.".into());
    }
    if dunce::canonicalize(&root).map_err(|_| "Cannot inspect the Valheim program directory.")?
        != dunce::canonicalize(image_root)
            .map_err(|_| "Cannot inspect the managed Valheim image directory.")?
    {
        return Err("The managed Valheim process belongs to another program directory.".into());
    }
    Ok(Context {
        pid,
        identity,
        root,
    })
}

async fn exchange(
    details: &InstanceDetails,
    operation: Operation,
) -> Result<(Context, ControlSnapshot), String> {
    let details = details.clone();
    tokio::task::spawn_blocking(move || {
        let context = context(&details)?;
        let packet = transport::exchange(&context, operation)?;
        Ok((context, packet))
    })
    .await
    .map_err(|_| "The Valheim control worker failed; the operation outcome is unknown.")?
}

pub(crate) async fn query(details: &InstanceDetails) -> Result<ControlSnapshot, String> {
    exchange(details, Operation::Query)
        .await
        .map(|(_, packet)| packet)
}

/// One submission only. A timeout after publication has an unknown outcome.
pub(crate) async fn reload(details: &InstanceDetails) -> Result<ControlSnapshot, String> {
    exchange(details, Operation::Reload)
        .await
        .map(|(_, packet)| packet)
}

pub(crate) async fn collect(
    details: &InstanceDetails,
    request_id: &str,
    observed_at: u64,
) -> LivePlayerCollectionResult {
    let (identity, packet) = exchange(details, Operation::Query)
        .await
        .map_err(|message| {
            Box::new(failed_snapshot(
                &details.summary.id,
                request_id.to_owned(),
                ModulePlayerListSource::FileIpc,
                RuntimeLivePlayerIssueCode::ExtensionUnavailable,
                message,
                false,
            ))
        })?;
    Ok(project(
        packet,
        &identity,
        &details.summary.id,
        request_id,
        observed_at,
    ))
}

fn project(
    packet: ControlSnapshot,
    context: &Context,
    instance_id: &str,
    request_id: &str,
    observed_at: u64,
) -> CachedLivePlayerSnapshot {
    let entries = packet
        .players
        .into_iter()
        .map(|peer| RuntimeLivePlayerEntry {
            player_key: format!(
                "valheim-native:{}:{}",
                context.identity.creation_time, peer.session_key
            ),
            display_name: peer.name.clone(),
            identifiers: vec![
                RuntimeLivePlayerIdentifier {
                    kind: RuntimePlayerIdentityKind::ValheimPlatformId,
                    value: peer.platform_id,
                    stable: true,
                },
                RuntimeLivePlayerIdentifier {
                    kind: RuntimePlayerIdentityKind::PlayerName,
                    value: peer.name,
                    stable: false,
                },
            ],
            available_action_ids: Vec::new(),
            ping_ms: None,
            session_started_at_unix_ms: None,
            role: peer.admin.then(|| "admin".to_owned()),
            attributes: Vec::new(),
        })
        .collect::<Vec<_>>();
    CachedLivePlayerSnapshot {
        public_snapshot: RuntimeLivePlayerSnapshot {
            snapshot_id: request_id.to_owned(),
            instance_id: instance_id.to_owned(),
            status: RuntimeLivePlayerStatus::Ready,
            source: Some(ModulePlayerListSource::FileIpc),
            observed_at_unix_ms: Some(observed_at),
            expires_at_unix_ms: Some(observed_at.saturating_add(15_000)),
            complete: true,
            truncated: false,
            stale: false,
            current_players: Some(entries.len()),
            max_players: None,
            entries,
            issue: None,
        },
        private_action_bindings: HashMap::new(),
        collected_at: observed_at,
    }
}

#[cfg(test)]
#[path = "valheim_control_tests.rs"]
mod tests;
