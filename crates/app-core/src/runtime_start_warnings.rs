use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStartWarningCode {
    ModUpdateUsingCache,
    ModUpdateSourceRequired,
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

/// A startup warning about verified cached content or an unknown update source.
/// Neither warning establishes freshness or native game readiness. Only a
/// successful StartInstanceResult confirms that the server actually started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeStartWarning {
    pub code: RuntimeStartWarningCode,
    pub provider: RuntimeModProvider,
    pub reason: RuntimeModUpdateReason,
    pub item_ids: Vec<String>,
    pub message: String,
    pub log_path: Option<String>,
    pub map_key: Option<String>,
    pub http_status: Option<u16>,
}
