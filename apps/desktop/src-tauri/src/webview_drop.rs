//! Resolves only browser-provided dropped objects; installation stays behind IPC.
#![cfg(any(windows, test))]

use serde::Deserialize;

const REQUEST_TYPE: &str = "lgsm:resolve-dropped-files";
const RESPONSE_TYPE: &str = "lgsm:dropped-files";
const FRAME_NAME: &str = "lgsm-file-drop";
const FRAME_PATH: &str = "/lgsm-file-drop.html";
const MAX_REQUEST_BYTES: usize = 4096;
const MAX_OBJECTS: u32 = 256;
const MAX_PATH_UNITS: usize = 32767;
const MAX_TOTAL_PATH_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DropError {
    InvalidRequest,
    TooManyFiles,
    UnsupportedRuntime,
    UnsupportedObject,
    InvalidPath,
    PathUnavailable,
}

impl DropError {
    fn code(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::TooManyFiles => "too_many_files",
            Self::UnsupportedRuntime => "unsupported_runtime",
            Self::UnsupportedObject => "unsupported_object",
            Self::InvalidPath => "invalid_path",
            Self::PathUnavailable => "path_unavailable",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    #[serde(rename = "type")]
    message_type: String,
    #[serde(rename = "requestId")]
    request_id: String,
}

// Ignore other WebView messages, including Tauri IPC. A valid correlation ID
// allows a fixed error response; malformed IDs are never reflected to content.
fn parse_request(raw: &str) -> Option<(String, Result<(), DropError>)> {
    if raw.len() > MAX_REQUEST_BYTES {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    if value.get("type")?.as_str()? != REQUEST_TYPE {
        return None;
    }
    let id = value.get("requestId")?.as_str()?;
    if id.len() != 36 || uuid::Uuid::parse_str(id).is_err() {
        return None;
    }
    let valid = serde_json::from_value::<Request>(value.clone())
        .map(|request| {
            debug_assert_eq!(request.message_type, REQUEST_TYPE);
            debug_assert_eq!(request.request_id, id);
        })
        .map_err(|_| DropError::InvalidRequest);
    Some((id.to_owned(), valid))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OriginPolicy {
    origin: String,
}

impl OriginPolicy {
    fn local(url: &str) -> Result<Self, String> {
        let url = tauri::Url::parse(url).map_err(|_| "Invalid dropped-file bridge origin")?;
        if !matches!(url.scheme(), "http" | "https")
            || !matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(
                "Dropped-file bridge requires the configured local application origin".into(),
            );
        }
        Ok(Self {
            origin: url.origin().ascii_serialization(),
        })
    }

    fn assets(https: bool) -> Self {
        Self {
            origin: format!("{}://tauri.localhost", if https { "https" } else { "http" }),
        }
    }

    fn matches(&self, source: &str) -> bool {
        tauri::Url::parse(source).is_ok_and(|url| {
            url.username().is_empty()
                && url.password().is_none()
                && url.origin().ascii_serialization() == self.origin
        })
    }

    fn accepts(&self, source: &str, current: &str) -> bool {
        self.matches(source) && self.matches(current)
    }

    fn accepts_frame(&self, source: &str, current: &str) -> bool {
        self.accepts(source, current)
            && tauri::Url::parse(source).is_ok_and(|url| {
                url.path() == FRAME_PATH && url.query().is_none() && url.fragment().is_none()
            })
    }
}

#[cfg(windows)]
fn validate_paths(
    count: u32,
    paths: impl IntoIterator<Item = Result<String, DropError>>,
) -> Result<Vec<String>, DropError> {
    use std::path::{Component, Path, Prefix};
    if count == 0 {
        return Err(DropError::InvalidRequest);
    }
    if count > MAX_OBJECTS {
        return Err(DropError::TooManyFiles);
    }
    let mut result = Vec::with_capacity(count as usize);
    let mut bytes = 0;
    for path in paths {
        let path = path?;
        if path.is_empty() {
            return Err(DropError::PathUnavailable);
        }
        let native = Path::new(&path);
        if path.encode_utf16().count() > MAX_PATH_UNITS
            || path.chars().any(char::is_control)
            || !native.is_absolute()
            || !matches!(native.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::UNC(_, _) | Prefix::VerbatimDisk(_) | Prefix::VerbatimUNC(_, _)))
            || native
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
            return Err(DropError::InvalidPath);
        }
        bytes += path.len();
        if bytes > MAX_TOTAL_PATH_BYTES {
            return Err(DropError::InvalidPath);
        }
        if !result.contains(&path) {
            result.push(path);
        }
    }
    Ok(result)
}

#[cfg(windows)]
fn reply_json(request_id: &str, result: Result<Vec<String>, DropError>) -> String {
    match result {
        Ok(paths) => serde_json::json!({"type":RESPONSE_TYPE,"requestId":request_id,"paths":paths}),
        Err(error) => serde_json::json!({"type":RESPONSE_TYPE,"requestId":request_id,"paths":[],"error":error.code()}),
    }.to_string()
}

#[cfg(windows)]
mod native;

#[cfg(all(windows, feature = "desktop-reliability"))]
pub(crate) use native::diagnostics;
#[cfg(windows)]
pub(crate) use native::{detach_on_ui_thread, install};

#[cfg(test)]
#[path = "webview_drop_tests.rs"]
mod tests;
