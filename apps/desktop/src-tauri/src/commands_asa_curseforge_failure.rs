//! Safe startup diagnostics. Native response bodies and URLs never enter this contract.
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Reason {
    ServerUnreachable,
    Authentication,
    AccessDenied,
    Request,
    NotFound,
    Expired,
    Response,
    Cancelled,
    Unknown,
    MissingMods,
    UnavailableMod,
    MissingFile,
    Timeout,
    Http,
    ProcessExited,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Cause {
    reason: Reason,
    http_status: Option<u16>,
}

impl Cause {
    pub(super) fn from_native(text: &str) -> Self {
        // CFCore 1.47 logs descriptions, not its boolean error fields. Only
        // this observed UE request-status description includes the flag.
        // Service JSON.message can replace HTTP descriptions with any text.
        if text == "ApiError: Failed (serverUnreachable)" {
            return Self {
                reason: Reason::ServerUnreachable,
                http_status: None,
            };
        }
        let description = text.strip_prefix("ApiError: ").unwrap_or(text);
        let reason = match description {
            "Request Cancelled" => Reason::Cancelled,
            "User token has expired" => Reason::Authentication,
            "Missing user token or api key missing privileges to access end point" => {
                Reason::AccessDenied
            }
            "The resource expired" => Reason::Expired,
            "Failed to parse JSON response" => Reason::Response,
            _ => Reason::Unknown,
        };
        let http_status = description
            .strip_prefix("Failed to send request to server with error: ")
            .filter(|code| code.len() == 3 && code.bytes().all(|byte| byte.is_ascii_digit()))
            .and_then(|code| code.parse::<u16>().ok())
            .filter(|code| (100..=599).contains(code));
        Self {
            reason: if http_status.is_some() {
                Reason::Http
            } else {
                reason
            },
            http_status,
        }
    }

    fn explanation(self) -> String {
        match self.reason {
            Reason::ServerUnreachable => "CurseForge could not be reached or returned a server error (serverUnreachable); the native log does not distinguish DNS, TLS, connection or timeout failures. Check the server host's network connection and retry.".into(),
            Reason::Authentication => "CurseForge rejected the native updater's authentication. Check this startup's native log and the game server version.".into(),
            Reason::AccessDenied => "CurseForge denied access to the native updater. Check this startup's native log and the requested Mods' availability.".into(),
            Reason::Request => "CurseForge rejected the version-query request. Check the configured Mod IDs.".into(),
            Reason::NotFound => "The requested CurseForge resource was not found. Check the Mod IDs and availability.".into(),
            Reason::Expired => "The requested CurseForge resource has expired. Check the Mod's availability.".into(),
            Reason::Response => "The native updater could not parse the CurseForge response. Inspect this startup's native log.".into(),
            Reason::Cancelled => "The native Mod update operation was cancelled.".into(),
            Reason::Http => self.http_status.map(|status| format!("CurseForge returned HTTP {status}. Check the native log and retry when the service is available.")).unwrap_or_else(|| "The native updater reported an HTTP failure without a status; inspect this startup's native log.".into()),
            _ => "The native updater did not report a recognized cause. Inspect this startup's native log.".into(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Failure {
    Query(Cause),
    MissingMods,
    UnavailableMod,
    MissingFile,
    Install,
}

impl Failure {
    pub(super) fn from_native(detail: &str) -> Option<Self> {
        if let Some(cause) = detail.strip_prefix("Error querying server mods: ") {
            Some(Self::Query(Cause::from_native(cause)))
        } else if detail
            .strip_prefix("Missing installed mods - (")
            .and_then(|counts| counts.strip_suffix(')'))
            .and_then(|counts| counts.split_once('/'))
            .is_some_and(|(installed, required)| decimal(installed) && decimal(required))
        {
            Some(Self::MissingMods)
        } else if detail
            .strip_prefix("Detected an unavailable mod: ")
            .and_then(|item| item.strip_suffix("! Ensuring mod is deleted..."))
            .is_some_and(|item| named_item_id(item).is_some())
        {
            Some(Self::UnavailableMod)
        } else if detail
            .strip_prefix("Mod: ")
            .and_then(|item| item.rsplit_once(" doesn't contain it's main file ("))
            .is_some_and(|(item, file)| {
                file.strip_suffix(")!").is_some_and(decimal) && named_item_id(item).is_some()
            })
        {
            Some(Self::MissingFile)
        } else {
            None
        }
    }

    pub(super) fn error(self) -> String {
        let (stage, reason, status, detail) = match self {
            Self::Query(cause) => ("query", cause.reason, cause.http_status, format!("the native Mod metadata query failed. {}", cause.explanation())),
            Self::MissingMods => ("validation", Reason::MissingMods, None, "the final installed Mod set is incomplete.".into()),
            Self::UnavailableMod => ("validation", Reason::UnavailableMod, None, "a required Mod is unavailable.".into()),
            Self::MissingFile => ("validation", Reason::MissingFile, None, "a required Mod has no published main file.".into()),
            Self::Install => ("install", Reason::Unknown, None, "the native game could not install every requested Mod. The final installation cause was not reported; inspect this startup's native log.".into()),
        };
        Diagnostic::new(stage, reason, status, format!("ASA CurseForge Mod initialization failed: {detail} The native game did not confirm loading every requested Mod; inspect this startup's native log.")).encode()
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Diagnostic {
    code: Code,
    message: String,
    stage: String,
    reason: Reason,
    http_status: Option<u16>,
    native_log_path: Option<String>,
    instance_id: Option<String>,
    map_key: Option<String>,
    before_process_key: Option<String>,
    cleanup_error: Option<String>,
    #[serde(default)]
    cache_verified: Option<bool>,
    #[serde(default)]
    native_process_exited: Option<bool>,
}

#[derive(Deserialize, Serialize)]
enum Code {
    #[serde(rename = "asa_curseforge_startup_failed")]
    StartupFailed,
}

impl Diagnostic {
    fn new(stage: &str, reason: Reason, http_status: Option<u16>, message: String) -> Self {
        Self {
            code: Code::StartupFailed,
            message,
            stage: stage.into(),
            reason,
            http_status,
            native_log_path: None,
            instance_id: None,
            map_key: None,
            before_process_key: None,
            cleanup_error: None,
            cache_verified: None,
            native_process_exited: None,
        }
    }
    fn encode(&self) -> String {
        // Closed fields are JSON-serializable; keep the readable cause if encoding ever fails.
        serde_json::to_string(self).unwrap_or_else(|_| self.message.clone())
    }
    fn decode(text: &str) -> Option<Self> {
        let value: Self = serde_json::from_str(text).ok()?;
        if !matches!(value.stage.as_str(), "query" | "install" | "validation")
            || (value.reason == Reason::Http && value.http_status.is_none())
            || (value.reason != Reason::Http && value.http_status.is_some())
            || value
                .http_status
                .is_some_and(|status| !(100..=599).contains(&status))
        {
            return None;
        }
        Some(value)
    }
}

pub(super) fn named_item_id(detail: &str) -> Option<&str> {
    let (name, id) = detail.rsplit_once(" (")?;
    let id = id.strip_suffix(')')?;
    (!name.is_empty() && decimal(id)).then_some(id)
}
fn decimal(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

pub(crate) fn with_native_log(error: String, path: &Path) -> String {
    if let Some(mut value) = Diagnostic::decode(&error) {
        value.native_log_path = Some(path.to_string_lossy().into_owned());
        value.message = format!("{} Native log: {}", value.message, path.display());
        value.encode()
    } else {
        format!("{error} Native log: {}", path.display())
    }
}

pub(crate) fn for_instance(error: String, instance: &str) -> String {
    let prefix = format!("instance `{instance}` failed strict bind-address verification: ");
    if let Some(mut value) = Diagnostic::decode(&error) {
        value.instance_id = Some(instance.into());
        value.message = format!("{prefix}{}", value.message);
        value.encode()
    } else {
        format!("{prefix}{error}")
    }
}

pub(crate) fn for_map(
    error: String,
    map: &str,
    next: Option<&str>,
    console: Option<&str>,
) -> String {
    let prefix = match next {
        Some(next) => format!("ASA map `{map}` Mod cache startup barrier failed before `{next}`: "),
        None => format!("ASA map `{map}`: "),
    };
    let suffix = console
        .map(|path| format!("; see {path}"))
        .unwrap_or_default();
    if let Some(mut value) = Diagnostic::decode(&error) {
        value.map_key = Some(map.into());
        value.before_process_key = next.map(str::to_owned);
        value.message = format!("{prefix}{}{suffix}", value.message);
        value.encode()
    } else {
        format!("{prefix}{error}{suffix}")
    }
}

pub(crate) fn with_cleanup_note(error: String, note: &str) -> String {
    if let Some(mut value) = Diagnostic::decode(&error) {
        value.cleanup_error = Some(note.trim_start_matches(';').trim().into());
        value.message.push_str(note);
        value.encode()
    } else {
        format!("{error}{note}")
    }
}

pub(crate) fn readable(error: &str) -> String {
    Diagnostic::decode(error)
        .map(|value| value.message)
        .unwrap_or_else(|| error.into())
}

pub(super) fn with_cache_verification(error: String, verified: bool, exited: bool) -> String {
    let Some(mut value) = Diagnostic::decode(&error) else {
        return error;
    };
    value.cache_verified = Some(verified);
    value.native_process_exited = Some(exited);
    value.message.push_str(if verified {
        " Installed Mod content passed cache verification; its revision is not confirmed latest. ASA's native initializer failed, and the manager cannot bypass it or start without the requested Mods."
    } else {
        " A complete usable installed Mod cache could not be verified; it may be missing, damaged or lack integrity metadata."
    });
    if exited {
        value.message.push_str(" The owned ASA process has exited.");
    }
    value.encode()
}

pub(super) fn validation_error(message: String, path: &Path, exited: bool) -> String {
    let reason = if exited {
        Reason::ProcessExited
    } else {
        Reason::Timeout
    };
    let message = format!(
        "{message} The native game did not confirm loading the requested Mods; inspect this startup's native log."
    );
    with_native_log(
        Diagnostic::new("validation", reason, None, message).encode(),
        path,
    )
}

#[cfg(test)]
#[path = "commands_asa_curseforge_failure_tests.rs"]
mod tests;
