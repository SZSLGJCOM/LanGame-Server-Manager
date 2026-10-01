//! The supported UE game extensions share one nonce-correlated file protocol.
//! Game selection is a closed enum: callers cannot supply arbitrary executable
//! names, relative project directories, paths, or player identity prefixes.
use std::path::{Path, PathBuf};
use std::time::Duration;

use app_core::{
    InstanceDetails, ModulePlayerListSource, RuntimeLivePlayerIssueCode, RuntimeLivePlayerSnapshot,
};

use super::cache::LivePlayerCollectionResult;
use super::service::failed_snapshot;

#[path = "file_ipc_response.rs"]
mod response;
#[path = "file_ipc_transport.rs"]
mod transport;

const MAX_BYTES: usize = 64 * 1024;
const TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Game {
    Windrose,
    Dragonwilds,
    Scum,
}

impl Game {
    fn module_id(self) -> &'static str {
        match self {
            Self::Windrose => "windrose",
            Self::Dragonwilds => "runescapedragonwilds",
            Self::Scum => "scum",
        }
    }
    fn bootstrap_executable(self) -> &'static str {
        match self {
            Self::Windrose => "WindroseServer.exe",
            Self::Dragonwilds => "RSDragonwildsServer.exe",
            Self::Scum => "SCUMServer.exe",
        }
    }
    fn shipping_executable(self) -> &'static str {
        match self {
            Self::Windrose => "WindroseServer-Win64-Shipping.exe",
            Self::Dragonwilds => "RSDragonwildsServer-Win64-Shipping.exe",
            Self::Scum => "SCUMServer.exe",
        }
    }
    fn project_directory(self) -> &'static str {
        match self {
            Self::Windrose => "R5",
            Self::Dragonwilds => "RSDragonwilds",
            Self::Scum => "SCUM",
        }
    }
    fn extension_unavailable(self) -> &'static str {
        match self {
            Self::Scum => {
                "The SCUM LanGame player-query extension is unavailable. Install the compatible UE4SS loader and start the server with LgsmPlayerQuery enabled."
            }
            Self::Windrose => {
                "The Windrose LanGame player-query extension is unavailable. Install the compatible UE4SS loader and start the server with LgsmPlayerQuery enabled."
            }
            Self::Dragonwilds => {
                "The RuneScape: Dragonwilds LanGame player-query extension is unavailable. Install the compatible UE4SS loader and start the server with LgsmPlayerQuery enabled."
            }
        }
    }
    fn process_unavailable(self) -> &'static str {
        match self {
            Self::Scum => "The SCUM process identity changed or could not be verified.",
            Self::Windrose => "The Windrose process identity changed or could not be verified.",
            Self::Dragonwilds => {
                "The RuneScape: Dragonwilds process identity changed or could not be verified."
            }
        }
    }
    fn install_root(self, image: &Path) -> Result<PathBuf, BridgeError> {
        let name = image
            .file_name()
            .and_then(|part| part.to_str())
            .ok_or(BridgeError::Process)?;
        if self != Self::Scum && name.eq_ignore_ascii_case(self.bootstrap_executable()) {
            return image
                .parent()
                .map(Path::to_path_buf)
                .ok_or(BridgeError::Process);
        }
        if name.eq_ignore_ascii_case(self.shipping_executable()) {
            let mut root = image.parent().ok_or(BridgeError::Process)?;
            for expected in ["Win64", "Binaries", self.project_directory()] {
                if !root
                    .file_name()
                    .and_then(|part| part.to_str())
                    .is_some_and(|part| part.eq_ignore_ascii_case(expected))
                {
                    return Err(BridgeError::Process);
                }
                root = root.parent().ok_or(BridgeError::Process)?;
            }
            return Ok(root.to_path_buf());
        }
        Err(BridgeError::Process)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum BridgeError {
    Missing,
    Busy,
    Io,
    Limit,
    Timeout,
    Process,
    Incomplete,
}

pub(crate) async fn collect(
    game: Game,
    details: &InstanceDetails,
    instance_id: &str,
    request_id: &str,
    observed_at: u64,
) -> LivePlayerCollectionResult {
    let failure = |error| bridge_failure(game, instance_id, request_id, error);
    if details.summary.module_id != game.module_id()
        || details.summary.id != instance_id
        || !valid_nonce(request_id)
    {
        return Err(failure(BridgeError::Process));
    }
    let mut processes = details
        .active_run
        .as_ref()
        .ok_or_else(|| failure(BridgeError::Process))?
        .processes
        .iter()
        .filter(|process| process.is_primary && process.status == "running");
    let process = processes
        .next()
        .ok_or_else(|| failure(BridgeError::Process))?;
    if processes.next().is_some() {
        return Err(failure(BridgeError::Process));
    }
    let pid = process.pid.ok_or_else(|| failure(BridgeError::Process))?;
    let identity = process
        .process_identity
        .clone()
        .ok_or_else(|| failure(BridgeError::Process))?;
    let nonce = request_id.to_owned();
    let body = tokio::task::spawn_blocking(move || {
        transport::exchange(game, pid, &identity, &nonce, observed_at)
    })
    .await
    .map_err(|_| failure(BridgeError::Io))?
    .map_err(failure)?;
    response::parse(game, instance_id, request_id, observed_at, &body)
}

fn bridge_failure(
    game: Game,
    instance_id: &str,
    nonce: &str,
    error: BridgeError,
) -> Box<RuntimeLivePlayerSnapshot> {
    let (code, summary) = match error {
        BridgeError::Missing => (
            RuntimeLivePlayerIssueCode::ExtensionUnavailable,
            game.extension_unavailable(),
        ),
        BridgeError::Busy | BridgeError::Io => (
            RuntimeLivePlayerIssueCode::IoFailed,
            "The local player-query exchange could not be read or written.",
        ),
        BridgeError::Limit => (
            RuntimeLivePlayerIssueCode::CaptureLimit,
            "The player-query response exceeded its bounded size.",
        ),
        BridgeError::Timeout => (
            RuntimeLivePlayerIssueCode::CollectionTimeout,
            "The player-query extension did not answer this request in time.",
        ),
        BridgeError::Process => (
            RuntimeLivePlayerIssueCode::ProcessUnavailable,
            game.process_unavailable(),
        ),
        BridgeError::Incomplete => (
            RuntimeLivePlayerIssueCode::ProtocolIncomplete,
            "The player-query extension did not confirm a complete current connection snapshot.",
        ),
    };
    Box::new(failed_snapshot(
        instance_id,
        nonce.to_owned(),
        ModulePlayerListSource::FileIpc,
        code,
        summary,
        code == RuntimeLivePlayerIssueCode::CaptureLimit,
    ))
}

fn valid_nonce(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

#[cfg(test)]
#[path = "dragonwilds_probe_tests.rs"]
mod dragonwilds_probe_tests;
#[cfg(test)]
#[path = "dragonwilds_tests.rs"]
mod dragonwilds_tests;
#[cfg(all(windows, test))]
#[path = "file_ipc_probe_tests.rs"]
mod probe_support;
#[cfg(test)]
#[path = "scum_probe_tests.rs"]
mod scum_probe_tests;
#[cfg(test)]
#[path = "scum_tests.rs"]
mod scum_tests;
#[cfg(test)]
#[path = "windrose_probe_tests.rs"]
mod windrose_probe_tests;
#[cfg(test)]
#[path = "windrose_tests.rs"]
mod windrose_tests;
