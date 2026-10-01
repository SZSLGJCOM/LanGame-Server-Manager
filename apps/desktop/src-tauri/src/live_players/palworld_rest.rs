use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use app_core::{InstanceDetails, ModulePlayerActionSpec};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::http_api::{HttpApiError, request_json};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
pub(crate) const PLAYER_ACTION_IDS: [&str; 2] = ["kick_player", "ban_player"];

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum PalworldCommand {
    Players,
    Announce { message: String },
    Kick { userid: String },
    Ban { userid: String },
    Unban { userid: String },
    Save,
    Shutdown,
}

pub(crate) struct PalworldEndpoint {
    pub(crate) address: SocketAddr,
    pub(crate) password: String,
}

pub(crate) struct ConfigurationError {
    pub(crate) summary: &'static str,
    pub(crate) setting_keys: Vec<String>,
}

pub(crate) fn resolve_endpoint(
    details: &InstanceDetails,
) -> Result<PalworldEndpoint, ConfigurationError> {
    let invalid = |summary, keys: &[&str]| ConfigurationError {
        summary,
        setting_keys: keys.iter().map(|key| String::from(*key)).collect(),
    };
    if details.summary.module_id != "palworld" {
        return Err(invalid(
            "The Palworld instance context does not match.",
            &[],
        ));
    }
    let settings: Value = serde_json::from_str(&details.settings_json)
        .map_err(|_| invalid("The Palworld settings could not be read.", &[]))?;
    if settings.get("rest_api_enabled").and_then(Value::as_bool) != Some(true) {
        return Err(invalid(
            "Enable the Palworld REST API for server management.",
            &["rest_api_enabled"],
        ));
    }
    let password = settings
        .get("admin_password")
        .and_then(Value::as_str)
        .filter(|password| !password.trim().is_empty())
        .ok_or_else(|| {
            invalid(
                "Set the Palworld administrator password for server management.",
                &["admin_password"],
            )
        })?;
    let mut ports = details
        .ports
        .iter()
        .filter(|port| port.name == "rest_api" && port.protocol.eq_ignore_ascii_case("tcp"));
    let port = ports
        .next()
        .ok_or_else(|| invalid("The instance has no REST API TCP port.", &[]))?;
    if port.port == 0 || ports.next().is_some() {
        return Err(invalid(
            "The instance REST API port is invalid or ambiguous.",
            &[],
        ));
    }
    // Palworld has no REST bind-interface setting. This locally owned server
    // is reached on loopback; advertised public addresses never receive credentials.
    Ok(PalworldEndpoint {
        address: SocketAddr::from((Ipv4Addr::LOCALHOST, port.port)),
        password: password.to_owned(),
    })
}

pub(crate) fn render_command(
    action: &ModulePlayerActionSpec,
    target: Option<&str>,
    role: Option<&str>,
    request_id: Option<&str>,
    require_target_binding: bool,
) -> Result<String, String> {
    let target = target.unwrap_or_default();
    if role.is_some_and(|value| !value.is_empty())
        || request_id.is_some_and(|value| !value.is_empty())
    {
        return Err(String::from(
            "Palworld REST actions do not accept roles or request tokens.",
        ));
    }
    let command = match (action.id.as_str(), action.command_template.trim()) {
        ("show_players", "players") => PalworldCommand::Players,
        ("save_world", "save") => PalworldCommand::Save,
        ("broadcast", "announce {{target}}") => PalworldCommand::Announce {
            message: target.to_owned(),
        },
        ("kick_player", "kick {{target}}") => PalworldCommand::Kick {
            userid: target.to_owned(),
        },
        ("ban_player", "ban {{target}}") => PalworldCommand::Ban {
            userid: target.to_owned(),
        },
        ("unban_player", "unban {{target}}") => PalworldCommand::Unban {
            userid: target.to_owned(),
        },
        _ => {
            return Err(String::from(
                "The Palworld REST action contract is unsupported.",
            ));
        }
    };
    let binds_target = command.target().is_some();
    if ((action.target_required || require_target_binding) && !binds_target)
        || (!binds_target && !target.is_empty())
    {
        return Err(String::from(
            "The Palworld REST action target does not match its contract.",
        ));
    }
    command.validate()?;
    // Serialize the typed request; player IDs and broadcast text never become
    // fragments of a command line, URL, or interpolated JSON document.
    serde_json::to_string(&command)
        .map_err(|_| String::from("Unable to encode the Palworld REST action."))
}

impl PalworldCommand {
    fn target(&self) -> Option<&str> {
        match self {
            Self::Announce { message } => Some(message),
            Self::Kick { userid } | Self::Ban { userid } | Self::Unban { userid } => Some(userid),
            _ => None,
        }
    }

    fn validate(&self) -> Result<(), String> {
        if let Some(target) = self.target() {
            let limit = if matches!(self, Self::Announce { .. }) {
                240
            } else {
                128
            };
            if target.trim().is_empty()
                || target.chars().count() > limit
                || target.chars().any(char::is_control)
            {
                return Err(String::from("The Palworld REST action target is invalid."));
            }
        }
        Ok(())
    }

    fn request(&self) -> (&'static str, reqwest::Method, Option<Value>) {
        match self {
            Self::Players => ("/v1/api/players", reqwest::Method::GET, None),
            Self::Announce { message } => (
                "/v1/api/announce",
                reqwest::Method::POST,
                Some(json!({ "message": message })),
            ),
            Self::Kick { userid } => (
                "/v1/api/kick",
                reqwest::Method::POST,
                Some(json!({ "userid": userid })),
            ),
            Self::Ban { userid } => (
                "/v1/api/ban",
                reqwest::Method::POST,
                Some(json!({ "userid": userid })),
            ),
            Self::Unban { userid } => (
                "/v1/api/unban",
                reqwest::Method::POST,
                Some(json!({ "userid": userid })),
            ),
            Self::Save => ("/v1/api/save", reqwest::Method::POST, None),
            Self::Shutdown => (
                "/v1/api/shutdown",
                reqwest::Method::POST,
                Some(json!({ "waittime": 10, "message": "LanGame server shutdown" })),
            ),
        }
    }
}

pub(crate) async fn execute(details: &InstanceDetails, command: &str) -> Result<String, String> {
    let command: PalworldCommand = serde_json::from_str(command)
        .map_err(|_| String::from("The Palworld REST request is not a declared operation."))?;
    command.validate()?;
    let endpoint = resolve_endpoint(details).map_err(|error| error.summary.to_owned())?;
    let (path, method, body) = command.request();
    let response = request_json(
        endpoint.address,
        path,
        Some(&endpoint.password),
        REQUEST_TIMEOUT,
        method,
        body.as_ref(),
    )
    .await
    .map_err(|error| match error {
        HttpApiError::Authentication => {
            String::from("The Palworld REST API rejected the administrator credentials.")
        }
        HttpApiError::Timeout => {
            String::from("The Palworld REST request timed out; its outcome is unconfirmed.")
        }
        HttpApiError::TooLarge => {
            String::from("The Palworld REST response exceeds the size limit.")
        }
        HttpApiError::Transport => {
            String::from("The Palworld REST request could not be confirmed.")
        }
        HttpApiError::HttpStatus(status) => {
            format!("The Palworld REST API rejected the operation with HTTP {status}.")
        }
    })?;
    if matches!(command, PalworldCommand::Players) {
        let players =
            super::palworld::parse_palworld(&details.summary.id, &response, "runtime-query", 0)
                .map_err(|_| String::from("The Palworld REST player response is incomplete."))?;
        return serde_json::to_string(&players.public_snapshot.entries)
            .map_err(|_| String::from("Unable to encode the Palworld player response."));
    }
    Ok(String::from(
        "Palworld REST API confirmed the operation (HTTP 200).",
    ))
}

#[cfg(test)]
#[path = "palworld_rest_tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) use tests::capture_response;
