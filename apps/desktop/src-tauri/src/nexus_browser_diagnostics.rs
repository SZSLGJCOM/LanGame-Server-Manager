//! Bounded, content-free diagnostics for the normal hidden website workflow.
use super::{NexusWebsiteTarget, PageObservation, matches_project, rendered_current_page};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum ReadyState {
    Loading,
    Interactive,
    Complete,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum ControlKind {
    Manual,
    Slow,
    Download,
    Login,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum ControlTag {
    A,
    Button,
    Input,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Control {
    kind: ControlKind,
    tag: ControlTag,
    displayed: bool,
    disabled: bool,
    matches_project: bool,
    file_id: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PageDiagnostics {
    ready_state: ReadyState,
    file_id: Option<String>,
    canonical_count: u32,
    main_heading_count: u32,
    manual_control_count: u32,
    matching_manual_count: u32,
    slow_control_count: u32,
    matching_slow_count: u32,
    login_notice_count: u32,
    visible_dialog_count: u32,
    challenge_frame_present: bool,
    controls: Vec<Control>,
}

pub(super) fn deserialize_diagnostics<'de, D>(
    deserializer: D,
) -> Result<Option<PageDiagnostics>, D::Error>
where
    D: Deserializer<'de>,
{
    // Diagnostics are optional. Malformed extra observations must never change
    // whether the existing page/automation response can complete a download.
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(value
        .and_then(|value| serde_json::from_value::<PageDiagnostics>(value).ok())
        .map(|mut value| {
            value.file_id = value.file_id.as_deref().and_then(public_id);
            value.controls.truncate(8);
            for control in &mut value.controls {
                control.file_id = control.file_id.as_deref().and_then(public_id);
            }
            value
        }))
}

fn public_id(value: &str) -> Option<String> {
    if value.is_empty() || value.len() > 20 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value
        .parse::<u64>()
        .ok()
        .filter(|id| *id > 0)
        .map(|id| id.to_string())
}

fn local_id(value: &str) -> Option<String> {
    (!value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte)))
    .then(|| value.to_owned())
}

fn page_identity(value: &str) -> Option<Value> {
    let url = reqwest::Url::parse(value).ok()?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return None;
    }
    if url.host_str() == Some("users.nexusmods.com") {
        return Some(json!({ "kind": "login" }));
    }
    if !matches!(url.host_str(), Some("www.nexusmods.com" | "nexusmods.com")) {
        return None;
    }
    let parts = url.path().trim_matches('/').split('/').collect::<Vec<_>>();
    let parts = parts.strip_prefix(&["games"]).unwrap_or(&parts);
    let (domain, id, path_file) = match parts {
        [domain, "mods", id] | [domain, "mods", id, "files"] => (*domain, *id, None),
        [domain, "mods", id, "files", file] => (*domain, *id, Some(*file)),
        _ => return None,
    };
    let domain = local_id(domain)?;
    let mod_id = public_id(id)?;
    let files = url
        .query_pairs()
        .filter(|(key, _)| key == "file_id")
        .map(|(_, value)| value.into_owned())
        .collect::<Vec<_>>();
    let file = if files.len() > 1 {
        None
    } else {
        files
            .first()
            .map(String::as_str)
            .or(path_file)
            .and_then(public_id)
    };
    let file =
        file.filter(|file| path_file.is_none_or(|path| public_id(path).as_ref() == Some(file)));
    Some(json!({ "kind": "project", "game_domain": domain, "mod_id": mod_id, "file_id": file }))
}

fn fixed_error_code(message: &str) -> &'static str {
    let value = serde_json::from_str::<Value>(message).ok();
    match value.as_ref().and_then(|value| value["code"].as_str()) {
        Some("nexus_browser_unavailable") => "nexus_browser_unavailable",
        Some("nexus_browser_busy") => "nexus_browser_busy",
        Some("nexus_browser_cancelled") => "nexus_browser_cancelled",
        Some("nexus_browser_page_unavailable") => "nexus_browser_page_unavailable",
        Some("nexus_browser_timed_out") => "nexus_browser_timed_out",
        Some("nexus_website_login_required") => "nexus_website_login_required",
        Some("nexus_website_verification_required") => "nexus_website_verification_required",
        Some("nexus_file_selection_required") => "nexus_file_selection_required",
        Some("nexus_website_file_quarantined") => "nexus_website_file_quarantined",
        Some("nexus_website_action_required") => "nexus_website_action_required",
        Some("nexus_website_controls_unavailable") => "nexus_website_controls_unavailable",
        Some("nexus_reference_invalid") => "nexus_reference_invalid",
        Some("nexus_game_mismatch") => "nexus_game_mismatch",
        Some("nexus_file_unavailable") => "nexus_file_unavailable",
        Some("nexus_download_expired") => "nexus_download_expired",
        Some("nexus_format_unsupported") => "nexus_format_unsupported",
        Some("nexus_network_error") => "nexus_network_error",
        Some("nexus_rate_limited") => "nexus_rate_limited",
        Some("nexus_preparation_failed") => "nexus_preparation_failed",
        Some("nexus_invalid_response") => "nexus_invalid_response",
        Some("nexus_installation_failed") => "nexus_installation_failed",
        _ => "nexus_operation_failed",
    }
}

#[derive(Clone, Default, Serialize)]
struct Context {
    session_id: Option<String>,
    instance_id: Option<String>,
    target: Option<Value>,
    phase: Option<&'static str>,
    page: Option<Value>,
}

pub(super) struct AttemptLog {
    path: Option<PathBuf>,
    context: Mutex<Context>,
}

impl AttemptLog {
    pub(super) fn new(session_id: &str, path: Option<PathBuf>) -> Self {
        Self {
            path,
            context: Mutex::new(Context {
                session_id: local_id(session_id),
                ..Context::default()
            }),
        }
    }

    pub(super) fn start(&self, instance_id: &str, reference: &str) {
        if let Ok(mut context) = self.context.lock() {
            context.instance_id = local_id(instance_id);
            context.target = page_identity(reference);
        }
        self.emit("mod.nexus.website.started", "info", None);
    }

    pub(super) fn target(&self, target: &NexusWebsiteTarget) {
        if let Ok(mut context) = self.context.lock() {
            context.target = page_identity(&target.page_url);
        }
    }

    pub(super) fn observe(
        &self,
        observation: &PageObservation,
        current: &reqwest::Url,
        target: &NexusWebsiteTarget,
    ) {
        let state = match observation.page.state.as_str() {
            "pending" => "pending",
            "ready" => "ready",
            "invalid" => "invalid",
            "challenge" => "challenge",
            _ => "unknown",
        };
        let page = json!({
            "metadata_state": state, "has_content": observation.rendered.has_content,
            "current": page_identity(current.as_str()), "matches_target": matches_project(current.as_str(), target),
            "rendered_current": rendered_current_page(&observation.rendered, current, target),
            "canonical_matches_target": observation.page.canonical_url.as_deref().is_some_and(|url| matches_project(url, target)),
            "title_present": observation.page.title.as_ref().is_some_and(|title| !title.trim().is_empty()),
            "icon_present": observation.page.icon_url.as_ref().is_some_and(|icon| !icon.is_empty()),
            "diagnostics": observation.diagnostics,
        });
        if let Ok(mut context) = self.context.lock() {
            context.page = Some(page);
        }
    }

    pub(super) fn stage(&self, phase: &str) {
        let phase = match phase {
            "opening" => "opening",
            "selecting" => "selecting",
            "waiting" => "waiting",
            "verifying" => "verifying",
            "installing" => "installing",
            _ => return,
        };
        let changed = if let Ok(mut context) = self.context.lock() {
            if context.phase == Some(phase) {
                false
            } else {
                context.phase = Some(phase);
                true
            }
        } else {
            false
        };
        if changed {
            self.emit("mod.nexus.website.stage", "info", None);
        }
    }

    pub(super) fn blocked(&self, message: &str) {
        self.emit(
            "mod.nexus.website.blocked",
            "warn",
            Some(fixed_error_code(message)),
        );
    }

    pub(super) fn finish(&self, failure: Option<&str>) {
        match failure {
            Some(message) => self.emit(
                "mod.nexus.website.failed",
                "error",
                Some(fixed_error_code(message)),
            ),
            None => self.emit("mod.nexus.website.completed", "info", None),
        }
    }

    fn emit(&self, action: &str, level: &str, code: Option<&str>) {
        let Some(path) = &self.path else {
            return;
        };
        let Ok(context) = self.context.lock().map(|context| context.clone()) else {
            return;
        };
        let mut context = serde_json::to_value(context).unwrap_or_else(|_| json!({}));
        context["pid"] = json!(std::process::id());
        context["code"] = json!(code);
        let entry = json!({
            "ts_unix_ms": SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_millis()).unwrap_or_default(),
            "level": level, "action": action, "message": "Nexus website download", "context": context,
        });
        if crate::desktop_app_log::append(path, &entry).is_err() {
            // Diagnostics cannot break an otherwise valid download, and even a
            // logging failure must not echo arbitrary website/runtime errors.
            eprintln!("Nexus website diagnostic log write failed");
        }
    }
}

#[cfg(test)]
#[path = "nexus_browser_diagnostics_tests.rs"]
mod tests;
