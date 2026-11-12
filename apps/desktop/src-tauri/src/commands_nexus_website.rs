//! Normal Nexus website selection and authorized CDN downloads, without API credentials.
use super::commands_mod_staging::{ManualModImportPresentation, stage_website_mod_source};
use super::commands_mods::{acquire_stopped_instance_mod_mutation, resolve_manual_mod_target};
use super::nexus_mods::{download, reference};
use super::*;

pub(crate) const NEXUS_WEBSITE_PROGRESS_EVENT: &str = "nexus-website-progress";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NexusWebsiteProgress {
    instance_id: String,
    operation_id: String,
    attempt: u32,
    sequence: u64,
    phase: &'static str,
    received_bytes: u64,
    total_bytes: Option<u64>,
    elapsed_seconds: u64,
}

struct WebsiteProgress {
    instance_id: String,
    operation_id: String,
    started: std::time::Instant,
    attempt: u32,
    sequence: u64,
    received_bytes: u64,
    total_bytes: Option<u64>,
}

impl WebsiteProgress {
    fn new(instance_id: String, operation_id: String) -> Self {
        Self {
            instance_id,
            operation_id,
            started: std::time::Instant::now(),
            attempt: 0,
            sequence: 0,
            received_bytes: 0,
            total_bytes: None,
        }
    }

    fn observe(&mut self, progress: download::WebsiteDownloadProgress) -> NexusWebsiteProgress {
        let phase = match progress {
            download::WebsiteDownloadProgress::Downloading(progress) => {
                self.attempt = progress.attempt;
                self.received_bytes = progress.received_bytes;
                self.total_bytes = progress.total_bytes;
                "downloading"
            }
            download::WebsiteDownloadProgress::Verifying => "verifying",
        };
        self.snapshot(phase)
    }

    fn snapshot(&mut self, phase: &'static str) -> NexusWebsiteProgress {
        self.sequence += 1;
        NexusWebsiteProgress {
            instance_id: self.instance_id.clone(),
            operation_id: self.operation_id.clone(),
            attempt: self.attempt,
            sequence: self.sequence,
            phase,
            received_bytes: self.received_bytes,
            total_bytes: self.total_bytes,
            elapsed_seconds: self.started.elapsed().as_secs(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct NexusWebsiteTarget {
    pub(crate) page_url: String,
    pub(crate) game_domain: String,
    pub(crate) mod_id: String,
    pub(crate) accepts: Vec<String>,
}

fn website_target(
    source: &app_core::ModuleModSourceSpec,
    accepts: Vec<String>,
    value: &str,
) -> Result<NexusWebsiteTarget, String> {
    let domain = reference::source_domain(source)?;
    let reference = reference::parse_reference(value, &domain)?;
    // An nxm reference identifies a website selection here. Its API-download
    // authorization is neither reused nor forwarded to the browser or storage.
    let mut page_url = format!("{}?tab=files", reference.page_url());
    if let Some(file_id) = reference.file_id {
        page_url.push_str("&file_id=");
        page_url.push_str(&file_id);
    }
    Ok(NexusWebsiteTarget {
        page_url,
        game_domain: reference.domain,
        mod_id: reference.mod_id,
        accepts,
    })
}

// Only local preparation and runtime transport checks use this wrapper.
// Website/network errors must never expose signed URLs through this message.
pub(crate) fn preparation_error(message: String) -> String {
    local_operation_error("nexus_preparation_failed", message, None)
}

fn installation_error(message: String) -> String {
    local_operation_error("nexus_installation_failed", message, None)
}

fn installation_context_error(message: String, context: &'static str) -> String {
    // Module/configuration parsing and storage recovery can include source text.
    // Keep their local stage identifiable without exposing that arbitrary text.
    local_operation_error("nexus_installation_failed", message, Some(context))
}

fn local_operation_error(code: &str, message: String, safe_context: Option<&str>) -> String {
    if serde_json::from_str::<Value>(&message)
        .ok()
        .and_then(|value| value.get("code").and_then(Value::as_str).map(str::to_owned))
        .is_some_and(|code| code.starts_with("nexus_"))
    {
        return message;
    }
    json!({ "code": code, "message": safe_context.unwrap_or(&message) }).to_string()
}

#[tauri::command]
pub(crate) async fn prepare_nexus_website_install(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    reference: String,
) -> Result<NexusWebsiteTarget, String> {
    let _operation = state
        .begin_storage_context_operation("Nexus website preparation")
        .map_err(preparation_error)?;
    let storage = state.bootstrap_storage().map_err(preparation_error)?;
    let (source, accepts) = super::commands_nexus_mods::instance_source(&storage, &instance_id)
        .await
        .map_err(preparation_error)?;
    let target = website_target(&source, accepts, &reference)?;
    let _instance_lock = acquire_stopped_instance_mod_mutation(&state, &storage, &instance_id)
        .await
        .map_err(preparation_error)?;
    resolve_manual_mod_target(&storage, instance_id, true)
        .await
        .map_err(preparation_error)?;
    Ok(target)
}

#[tauri::command]
pub(crate) async fn install_nexus_website_download(
    app: tauri::AppHandle,
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    operation_id: String,
    reference: String,
    download_url: String,
    file_name: String,
    title: Option<String>,
    icon_url: Option<String>,
) -> Result<ManualModStageResult, String> {
    uuid::Uuid::parse_str(&operation_id).map_err(|_| {
        installation_error("The Nexus website operation identity is invalid.".into())
    })?;
    let progress = Arc::new(StdMutex::new(WebsiteProgress::new(
        instance_id.clone(),
        operation_id,
    )));
    let operation = state
        .begin_storage_context_operation("Nexus website Mod installation")
        .map_err(|error| {
            installation_context_error(
                error,
                "Could not reserve the local storage operation for Nexus installation.",
            )
        })?;
    let storage = state.bootstrap_storage().map_err(|error| {
        installation_context_error(
            error,
            "Could not open local storage for Nexus installation.",
        )
    })?;
    let (source, accepts) = super::commands_nexus_mods::instance_source(&storage, &instance_id)
        .await
        .map_err(|error| {
            installation_context_error(
                error,
                "Could not read this instance's local Nexus installation configuration.",
            )
        })?;
    let website = website_target(&source, accepts.clone(), &reference)?;
    // Reject a bad website event before changing the instance program or
    // requesting any network resource. The prepared URL cannot be serialized.
    let download = download::prepare_website_download(download_url, file_name, &accepts)?;
    let progress_observer = progress.clone();
    let progress_app = app.clone();
    let download = download.observe(Arc::new(move |update| {
        let payload = progress_observer
            .lock()
            .ok()
            .map(|mut state| state.observe(update));
        if let Some(payload) = payload {
            // Event observation cannot claim, cancel or fail this owned download.
            let _ = progress_app.emit(NEXUS_WEBSITE_PROGRESS_EVENT, payload);
        }
    }));
    let presentation = ManualModImportPresentation {
        display_name: super::mod_presentations::normalize_display_name(title.as_deref()),
        icon_url: super::mod_presentations::verified_icon_url(icon_url.as_deref()),
    };
    let instance_lock = acquire_stopped_instance_mod_mutation(&state, &storage, &instance_id)
        .await
        .map_err(installation_error)?;
    let instance_lock = super::commands_program_storage::prepare_instance_mod_program(
        &state.install_locks,
        &storage,
        &operation,
        &instance_id,
        instance_lock,
    )
    .await
    .map_err(|error| {
        installation_context_error(
            error,
            "Could not prepare this instance's local server program for Nexus installation.",
        )
    })?;
    let target = resolve_manual_mod_target(&storage, instance_id, true)
        .await
        .map_err(|error| {
            installation_context_error(
                error,
                "Could not resolve this instance's local Nexus installation folder.",
            )
        })?;
    let downloads = tokio::select! {
        biased;
        _ = operation.cancelled() => return Err(installation_error("Nexus website download was cancelled before publication.".into())),
        result = download::download_website_file(download) => result?,
    };
    let payload = progress
        .lock()
        .ok()
        .map(|mut state| state.snapshot("installing"));
    if let Some(payload) = payload {
        let _ = app.emit(NEXUS_WEBSITE_PROGRESS_EVENT, payload);
    }
    let cancellation = operation.cancellation_token();
    spawn_blocking_storage_context_task(&operation, move || {
        let _instance_lock = instance_lock;
        let result = publish_website_download(
            target,
            downloads.paths()[0].clone(),
            presentation,
            Some(&website.mod_id),
            &cancellation,
        );
        // Downloads remain owned until publication or rollback has completed.
        drop(downloads);
        result
    })
    .await
    .map_err(|_| {
        installation_error("The Nexus website installation worker did not complete.".into())
    })?
}

fn publish_website_download(
    target: ResolvedManualModTarget,
    source: PathBuf,
    presentation: ManualModImportPresentation,
    nexus_project_id: Option<&str>,
    cancellation: &std::sync::atomic::AtomicBool,
) -> Result<ManualModStageResult, String> {
    if cancellation.load(std::sync::atomic::Ordering::Acquire) {
        return Err(installation_error(
            "Nexus website installation was cancelled before publication.".into(),
        ));
    }
    stage_website_mod_source(target, source, presentation, nexus_project_id)
        .map_err(installation_error)
}

#[cfg(test)]
#[path = "commands_nexus_website_tests.rs"]
mod tests;
