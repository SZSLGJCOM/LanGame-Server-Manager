use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStartWarningCode {
    ModUpdateUsingCache,
    ModUpdateSourceRequired,
    WorldInitializationIncomplete,
    ManualSaveUnavailable,
    GracefulStopUnavailable,
    ProgramUpdateUsingInstalled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeModProvider {
    SteamWorkshop,
    CurseForge,
    Thunderstore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeModUpdateReason {
    Network,
    Authentication,
    AccessDenied,
    Request,
    NotFound,
    Expired,
    Response,
    Http,
    Download,
    Install,
    Unknown,
}

/// A startup preparation warning. Mod context is absent for native world setup.
/// These warnings do not establish native game readiness. Only a
/// successful StartInstanceResult confirms that the server actually started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeStartWarning {
    pub code: RuntimeStartWarningCode,
    pub provider: Option<RuntimeModProvider>,
    pub reason: Option<RuntimeModUpdateReason>,
    pub item_ids: Vec<String>,
    pub message: String,
    pub log_path: Option<String>,
    pub map_key: Option<String>,
    pub http_status: Option<u16>,
}

impl RuntimeStartWarning {
    pub fn manual_save_unavailable(log_path: Option<String>) -> Self {
        Self {
            code: RuntimeStartWarningCode::ManualSaveUnavailable,
            provider: None,
            reason: None,
            item_ids: Vec::new(),
            message: "Manual saving is unavailable; the game's own saving is unaffected.".into(),
            log_path,
            map_key: None,
            http_status: None,
        }
    }

    pub fn graceful_stop_unavailable(log_path: Option<String>) -> Self {
        Self {
            code: RuntimeStartWarningCode::GracefulStopUnavailable,
            provider: None,
            reason: None,
            item_ids: Vec::new(),
            message: "Normal shutdown is unavailable in the manager; automatic forced termination is disabled.".into(),
            log_path,
            map_key: None,
            http_status: None,
        }
    }

    pub fn world_initialization(message: String, log_path: Option<String>) -> Self {
        Self {
            code: RuntimeStartWarningCode::WorldInitializationIncomplete,
            provider: None,
            reason: None,
            item_ids: Vec::new(),
            message,
            log_path,
            map_key: None,
            http_status: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_world_warning_does_not_claim_a_mod_provider_or_cached_content() {
        let warning =
            RuntimeStartWarning::world_initialization("Stopped before creation".into(), None);
        assert_eq!(
            warning.code,
            RuntimeStartWarningCode::WorldInitializationIncomplete
        );
        assert_eq!(warning.provider, None);
        assert_eq!(warning.reason, None);
        assert!(warning.item_ids.is_empty());
        assert!(warning.map_key.is_none() && warning.http_status.is_none());
        let body = serde_json::to_value(&warning).unwrap();
        assert_eq!(body["code"], "world_initialization_incomplete");
        assert!(body["provider"].is_null() && body["reason"].is_null());
        assert_eq!(
            serde_json::from_value::<RuntimeStartWarning>(body).unwrap(),
            warning
        );
    }
}
