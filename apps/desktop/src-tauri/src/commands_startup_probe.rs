//! Bounded, read-only protocol evidence for the startup readiness engine.
use app_core::InstanceDetails;
pub(crate) use app_storage::ProcessProbeTarget as StartupProcessTarget;
pub(super) use app_storage::poll_owned_tcp;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::astroneer_console::{Query, startup};
use crate::live_players::file_ipc::unreal_save_process::{
    self as process, GameProfile, NativeProcess,
};

const QUERY_TIMEOUT: Duration = Duration::from_secs(3);

// Do not derive Debug: the native console credential must never enter logs.
pub(crate) enum StartupProbe {
    A2s {
        port: u16,
    },
    Astroneer {
        port: u16,
        password: String,
        root: PathBuf,
        allow_empty_world: bool,
        native: Option<NativeProcess>,
    },
}

impl StartupProbe {
    pub fn a2s(port: u16) -> Self {
        Self::A2s { port }
    }

    pub fn astroneer(instance: &InstanceDetails) -> Result<Self, String> {
        if instance.summary.module_id != "astroneer" {
            return Err("ASTRONEER startup probe requires an ASTRONEER instance.".into());
        }
        let settings: Value = serde_json::from_str(&instance.settings_json)
            .map_err(|_| "ASTRONEER startup console settings are invalid.")?;
        let password = settings
            .get("console_password")
            .and_then(Value::as_str)
            .filter(|value| {
                !value.trim().is_empty()
                    && value.chars().count() <= 128
                    && !value.chars().any(char::is_control)
            })
            .ok_or("ASTRONEER startup console credential is invalid.")?
            .to_owned();
        let mut ports = instance.ports.iter().filter(|port| port.name == "console");
        let port = ports
            .next()
            .ok_or("ASTRONEER startup console port is missing.")?;
        if port.port == 0 || !port.protocol.eq_ignore_ascii_case("tcp") || ports.next().is_some() {
            return Err("ASTRONEER startup console port is invalid or ambiguous.".into());
        }
        let allow_empty_world =
            !crate::commands::commands_astroneer_startup::automatic_world_loading(&settings)?;
        let instance_root = crate::commands::commands_program_storage::instance_root(instance)?;
        let root = app_storage::resolve_instance_runtime_root(instance_root)
            .map_err(|error| error.to_string())?;
        let root = std::fs::canonicalize(root)
            .map_err(|_| "ASTRONEER startup runtime directory is unavailable.")?;
        Ok(Self::Astroneer {
            port: port.port,
            password,
            root,
            allow_empty_world,
            native: None,
        })
    }

    /// Call from the engine's blocking worker. One poll has one short protocol
    /// budget; the engine owns cancellation, retry cadence and the startup lease.
    pub fn poll(&mut self, target: &StartupProcessTarget) -> Result<bool, String> {
        match self {
            Self::A2s { port } => app_storage::poll_owned_a2s_info(target, *port),
            Self::Astroneer {
                port,
                password,
                root,
                allow_empty_world,
                native,
            } => poll_astroneer(root, native, *port, password, *allow_empty_world, target),
        }
    }
}

fn poll_astroneer(
    root: &Path,
    native: &mut Option<NativeProcess>,
    port: u16,
    password: &str,
    allow_empty_world: bool,
    target: &StartupProcessTarget,
) -> Result<bool, String> {
    if let Some(expected) = native.as_ref() {
        process::verify(
            GameProfile::Astroneer,
            root,
            target.pid,
            &target.identity,
            expected,
        )?;
    } else {
        *native =
            process::capture_optional(GameProfile::Astroneer, root, target.pid, &target.identity)?;
    }
    let Some(native) = native.as_ref() else {
        return Ok(false);
    };
    // The managed launcher owns the lifetime; the actual Shipping child owns
    // the console. Never send the credential to a launcher or a same-port peer.
    let responder = StartupProcessTarget {
        pid: native.pid,
        identity: native.identity.clone(),
        host: "127.0.0.1".into(),
    };
    let result = poll_owned_tcp(&responder, port, || {
        query_astroneer(port, password, &responder, allow_empty_world)
    });
    process::verify(
        GameProfile::Astroneer,
        root,
        target.pid,
        &target.identity,
        native,
    )?;
    result
}

fn query_astroneer(
    port: u16,
    password: &str,
    target: &StartupProcessTarget,
    allow_empty_world: bool,
) -> Result<bool, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "Cannot initialize the startup query runtime.")?;
    runtime.block_on(async {
        let observed = tokio::time::timeout(QUERY_TIMEOUT, async {
            let games =
                startup::query_checked(port, password, Query::Games, target.pid, &target.identity)
                    .await?;
            let statistics = startup::query_checked(
                port,
                password,
                Query::Statistics,
                target.pid,
                &target.identity,
            )
            .await?;
            Ok::<_, startup::QueryError>(astroneer_ready(&games, &statistics, allow_empty_world))
        })
        .await;
        match observed {
            Ok(Ok(ready)) => Ok(ready),
            Ok(Err(startup::QueryError::Ownership(message))) => Err(message),
            Ok(Err(startup::QueryError::Unavailable(_))) | Err(_) => Ok(false),
        }
    })
}

fn astroneer_ready(games: &Value, statistics: &Value, allow_empty_world: bool) -> bool {
    crate::runtime_astroneer_health::selected_world_matches(games, statistics)
        || (allow_empty_world
            && games.get("gameList").is_some_and(Value::is_array)
            && games.get("activeSaveName").and_then(Value::as_str) == Some("")
            && statistics.get("saveGameName").and_then(Value::as_str) == Some(""))
}

#[cfg(test)]
#[path = "commands_startup_probe_tests.rs"]
mod tests;
