//! A normal Nexus website session supplies the user's selected download; no API key is used.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{
    Manager, WebviewUrl,
    ipc::Channel,
    webview::{DownloadEvent, WebviewBuilder},
};

use crate::commands::ManualModStageResult;
use crate::commands::commands_nexus_website::{NexusWebsiteTarget, preparation_error};
use crate::commands::mod_presentations::{normalize_display_name, verified_icon_url};

const WINDOW_PREFIX: &str = "nexus-website-";
const AUTOMATIC_TIMEOUT: Duration = Duration::from_secs(90);
const PAGE_LOAD_TIMEOUT: Duration = Duration::from_secs(30);
const VERIFICATION_TIMEOUT: Duration = Duration::from_secs(30);

#[path = "nexus_browser_diagnostics.rs"]
mod diagnostics;

#[path = "nexus_browser_auth.rs"]
pub(crate) mod auth;

#[path = "nexus_browser_host.rs"]
pub(crate) mod host;
use host::SessionLease;
pub(crate) use host::{BrowserState, close_active_host, retained_main_window};

pub(crate) fn is_browser_window(label: &str) -> bool {
    label.starts_with(WINDOW_PREFIX)
}

fn error(code: &str) -> String {
    json!({ "code": code }).to_string()
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page {
    url: String,
    canonical_url: Option<String>,
    game_domain: Option<String>,
    mod_id: Option<String>,
    title: Option<String>,
    icon_url: Option<String>,
    state: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenderedPage {
    url: String,
    has_content: bool,
}

#[derive(Deserialize)]
struct PageObservation {
    page: Page,
    rendered: RenderedPage,
    automation: Automation,
    #[serde(default, deserialize_with = "diagnostics::deserialize_diagnostics")]
    diagnostics: Option<diagnostics::PageDiagnostics>,
    #[serde(default)]
    auth: Option<auth::AuthState>,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum AutomationState {
    Loading,
    Working,
    Blocked,
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum AutomationPhase {
    Selecting,
    Waiting,
    Verifying,
}

impl AutomationPhase {
    fn name(self) -> &'static str {
        match self {
            Self::Selecting => "selecting",
            Self::Waiting => "waiting",
            Self::Verifying => "verifying",
        }
    }
}

#[derive(Deserialize)]
struct Automation {
    state: AutomationState,
    phase: Option<AutomationPhase>,
    reason: Option<String>,
    #[serde(rename = "fileSelection")]
    file_selection: Option<FileSelection>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct FileSelection {
    file_id: String,
    file_uid: String,
}

fn blocked_error(reason: Option<&str>) -> String {
    error(match reason {
        Some("login") => "nexus_website_login_required",
        Some("verification") => "nexus_website_verification_required",
        Some("file_selection") => "nexus_file_selection_required",
        Some("file_quarantined") => "nexus_website_file_quarantined",
        Some("project_identity") => "nexus_browser_page_unavailable",
        Some("website_dialog") => "nexus_website_action_required",
        Some("download_not_started") => "nexus_browser_timed_out",
        _ => "nexus_website_controls_unavailable",
    })
}

fn explicit_file_id(target: &NexusWebsiteTarget) -> Result<Option<String>, String> {
    let page =
        reqwest::Url::parse(&target.page_url).map_err(|_| error("nexus_reference_invalid"))?;
    Ok(page
        .query_pairs()
        .find(|(key, _)| key == "file_id")
        .map(|(_, value)| value.into_owned()))
}

fn automatic_context(target: &NexusWebsiteTarget) -> Result<serde_json::Value, String> {
    let file_id = explicit_file_id(target)?;
    Ok(json!({ "gameDomain": target.game_domain, "modId": target.mod_id, "fileId": file_id }))
}

fn interactive_url(url: &reqwest::Url, target: &NexusWebsiteTarget) -> bool {
    matches_project(url.as_str(), target)
        || (url.scheme() == "https"
            && url.host_str() == Some("users.nexusmods.com")
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none())
}

fn rendered_current_page(
    rendered: &RenderedPage,
    current: &reqwest::Url,
    target: &NexusWebsiteTarget,
) -> bool {
    let mut current = current.clone();
    current.set_query(None);
    current.set_fragment(None);
    rendered.has_content && interactive_url(&current, target) && rendered.url == current.as_str()
}

fn observation_phase(
    observation: &PageObservation,
    current: &reqwest::Url,
    target: &NexusWebsiteTarget,
) -> Option<AutomationPhase> {
    if observation.page.state == "challenge" {
        // A verification shell may have no rendered text or controls. Its
        // explicit marker still belongs to this exact current project URL.
        let mut public_current = current.clone();
        public_current.set_query(None);
        public_current.set_fragment(None);
        return (matches_project(current.as_str(), target)
            && observation.page.url == public_current.as_str())
        .then_some(AutomationPhase::Verifying);
    }
    rendered_current_page(&observation.rendered, current, target)
        .then_some(observation.automation.phase)
        .flatten()
}

fn selection_timeout(
    elapsed: Duration,
    content_seen: bool,
    verification_elapsed: Option<Duration>,
) -> Option<&'static str> {
    if verification_elapsed.is_some_and(|elapsed| elapsed >= VERIFICATION_TIMEOUT) {
        Some("nexus_website_verification_required")
    } else if elapsed >= AUTOMATIC_TIMEOUT {
        Some("nexus_browser_timed_out")
    } else if !content_seen && verification_elapsed.is_none() && elapsed >= PAGE_LOAD_TIMEOUT {
        Some("nexus_browser_page_unavailable")
    } else {
        None
    }
}

fn matches_project(value: &str, target: &NexusWebsiteTarget) -> bool {
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    if url.scheme() != "https"
        || !matches!(url.host_str(), Some("nexusmods.com" | "www.nexusmods.com"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return false;
    }
    let parts = url.path().trim_matches('/').split('/').collect::<Vec<_>>();
    let parts = parts.strip_prefix(&["games"]).unwrap_or(&parts);
    match parts {
        [domain, "mods", id] | [domain, "mods", id, "files"] | [domain, "mods", id, "files", _] => {
            *domain == target.game_domain && *id == target.mod_id
        }
        _ => false,
    }
}

fn download_url(value: &reqwest::Url) -> bool {
    value.as_str().len() <= 16384
        && value.scheme() == "https"
        && value.username().is_empty()
        && value.password().is_none()
        && value.port().is_none()
        && value.fragment().is_none()
        && value.host_str().is_some_and(|host| {
            host == "cf-files.nexusmods.com" || host.ends_with(".nexus-cdn.com")
        })
}

fn selected_website_route(url: &reqwest::Url, selection: Option<&FileSelection>) -> bool {
    selection.is_some_and(|selection| {
        url.scheme() == "https"
            && url.host_str() == Some("www.nexusmods.com")
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && url.path() == format!("/api/files/{}/download", selection.file_uid)
    })
}

fn navigation_allowed(
    url: &reqwest::Url,
    target: &NexusWebsiteTarget,
    selection: Option<&FileSelection>,
) -> bool {
    matches_project(url.as_str(), target)
        || selected_website_route(url, selection)
        || download_url(url)
        || (url.scheme() == "https"
            && url.host_str() == Some("users.nexusmods.com")
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none())
}

fn retain_selection_for_navigation(
    selection: &mut Option<FileSelection>,
    url: &reqwest::Url,
    target: &NexusWebsiteTarget,
) {
    if !matches_project(url.as_str(), target)
        && !selected_website_route(url, selection.as_ref())
        && !download_url(url)
    {
        *selection = None;
    }
}

fn verified_file_selection(
    observation: &PageObservation,
    current: &reqwest::Url,
    target: &NexusWebsiteTarget,
    observed_generation: u64,
    current_generation: u64,
) -> Option<FileSelection> {
    if observed_generation != current_generation
        || !matches!(observation.automation.state, AutomationState::Working)
        || observation.automation.phase != Some(AutomationPhase::Selecting)
        || !matches_project(current.as_str(), target)
        || !rendered_current_page(&observation.rendered, current, target)
        || verified_page(observation.page.clone(), target).is_none()
    {
        return None;
    }
    let selection = observation.automation.file_selection.as_ref()?;
    for value in [&selection.file_id, &selection.file_uid] {
        let number = value.parse::<u64>().ok()?;
        if number == 0 || number.to_string() != *value {
            return None;
        }
    }
    if explicit_file_id(target)
        .ok()?
        .is_some_and(|file_id| file_id != selection.file_id)
    {
        return None;
    }
    Some(selection.clone())
}

fn verified_page(mut page: Page, target: &NexusWebsiteTarget) -> Option<Page> {
    if page.state != "ready"
        || !matches_project(&page.url, target)
        || !page
            .canonical_url
            .as_deref()
            .is_some_and(|url| matches_project(url, target))
        || page.game_domain.as_deref() != Some(&target.game_domain)
        || page.mod_id.as_deref() != Some(&target.mod_id)
    {
        return None;
    }
    page.title = normalize_display_name(page.title.as_deref());
    page.icon_url = verified_icon_url(page.icon_url.as_deref());
    page.title.as_ref()?;
    Some(page)
}

// Contains a short-lived signed URL. Deliberately has no Debug or Serialize implementation.
struct SelectedDownload {
    url: String,
    file_name: String,
    page: Page,
}

struct RequestedDownload {
    url: String,
    file_name: String,
}

fn requested_download(
    url: &reqwest::Url,
    destination: &std::path::Path,
) -> Result<RequestedDownload, String> {
    if !download_url(url) {
        return Err(error("nexus_file_unavailable"));
    }
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| error("nexus_file_unavailable"))?
        .to_owned();
    Ok(RequestedDownload {
        url: url.to_string(),
        file_name,
    })
}

async fn select_download(
    session: &SessionLease,
    target: NexusWebsiteTarget,
    on_progress: &(dyn Fn(&str) -> Result<(), String> + Sync),
) -> Result<SelectedDownload, String> {
    #[cfg(all(windows, feature = "desktop-reliability"))]
    let fixture = smoke::current_fixture(&session.app);
    select_download_with_fixture(
        session,
        target,
        on_progress,
        #[cfg(all(windows, feature = "desktop-reliability"))]
        fixture.as_deref(),
    )
    .await
}

async fn select_download_with_fixture(
    session: &SessionLease,
    target: NexusWebsiteTarget,
    on_progress: &(dyn Fn(&str) -> Result<(), String> + Sync),
    #[cfg(all(windows, feature = "desktop-reliability"))] fixture: Option<&smoke::Fixture>,
) -> Result<SelectedDownload, String> {
    session.diagnostics.target(&target);
    session.check()?;
    let mut observed = None::<Page>;
    let mut pending = None::<RequestedDownload>;
    let mut content_seen = false;
    let mut last_phase = None;
    let mut verification_started = None::<Instant>;
    let observation = format!(
        "const page = {}; const rendered = {}; const auth = {};",
        include_str!("nexus_page_extract.js"),
        include_str!("nexus_browser_readiness.js"),
        auth::SCRIPT,
    );
    let script = format!(
        "(authorizedSelection) => {{ {observation} const target = {}; const fileControls = ({})(target); const automation = ({})(target, page, rendered, authorizedSelection, fileControls); let diagnostics = null; if (automation.state === 'blocked') {{ try {{ diagnostics = ({})(target, fileControls); }} catch {{}} }} return {{ page, rendered, automation, diagnostics, auth }}; }}",
        automatic_context(&target)?,
        include_str!("nexus_browser_file_controls.js"),
        include_str!("nexus_browser_automatic.js"),
        include_str!("nexus_browser_diagnostics.js"),
    );
    // Once the website has selected a download, only inspect project identity.
    // Never click another button while metadata catches up with DownloadStarting.
    let read_only = format!(
        "(() => {{ {observation} return {{ page, rendered, automation: {{ state: 'loading' }}, diagnostics: null, auth }}; }})()"
    );
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    let claimed = AtomicBool::new(false);
    let navigation_target = target.clone();
    let navigation_generation = Arc::new(AtomicU64::new(0));
    let next_generation = navigation_generation.clone();
    let authorized_selection = Arc::new(Mutex::new(None::<FileSelection>));
    let navigation_selection = authorized_selection.clone();
    let popup_selection = authorized_selection.clone();
    let new_window_target = target.clone();
    let app = session.app.clone();
    let label = format!("{WINDOW_PREFIX}{}", session.id);
    let popup_label = label.clone();
    let cancelled = session.cancelled.clone();
    let fixture_mode = {
        #[cfg(all(windows, feature = "desktop-reliability"))]
        {
            fixture.is_some()
        }
        #[cfg(not(all(windows, feature = "desktop-reliability")))]
        {
            false
        }
    };
    let builder = WebviewBuilder::new(
        &label,
        WebviewUrl::External(
            (if fixture_mode {
                "about:blank"
            } else {
                target.page_url.as_str()
            })
            .parse()
            .map_err(|_| error("nexus_reference_invalid"))?,
        ),
    )
    .focused(false)
    .on_navigation(move |url| {
        let Ok(mut selection) = navigation_selection.lock() else {
            return false;
        };
        let allowed = navigation_allowed(url, &navigation_target, selection.as_ref());
        retain_selection_for_navigation(&mut selection, url, &navigation_target);
        if allowed {
            next_generation.fetch_add(1, Ordering::AcqRel);
        }
        allowed
    })
    .on_new_window(move |url, _| {
        let allowed = popup_selection.lock().is_ok_and(|selection| {
            navigation_allowed(&url, &new_window_target, selection.as_ref())
        });
        if allowed && let Some(window) = app.get_webview(&popup_label) {
            let _ = window.navigate(url);
        }
        tauri::webview::NewWindowResponse::Deny
    })
    .on_download(move |_, event| {
        if let DownloadEvent::Requested { url, destination } = event {
            if cancelled.load(Ordering::Acquire) || claimed.swap(true, Ordering::AcqRel) {
                return false;
            }
            let _ = sender.try_send(requested_download(&url, destination));
        }
        // The runtime's bounded writer owns the transfer, temporary file and installation.
        // Browser cookies are neither read nor exported.
        false
    });
    let child = session.attach(builder)?;
    let window = &child.view;
    #[cfg(all(windows, feature = "desktop-reliability"))]
    if let Some(fixture) = fixture {
        fixture.attach(window).await?;
        window
            .navigate(
                target
                    .page_url
                    .parse()
                    .map_err(|_| error("nexus_reference_invalid"))?,
            )
            .map_err(|_| error("nexus_browser_unavailable"))?;
    }
    let started = Instant::now();
    loop {
        session.check()?;
        if session.app.get_webview(window.label()).is_none() {
            return Err(error("nexus_browser_cancelled"));
        }
        if !window
            .url()
            .is_ok_and(|url| matches_project(url.as_str(), &target))
        {
            // History changes can leave the project without a native callback.
            *authorized_selection
                .lock()
                .map_err(|_| error("nexus_browser_unavailable"))? = None;
            observed = None;
        }
        if let Ok(selected) = receiver.try_recv() {
            pending = Some(selected?);
            // SPA history changes need not emit native navigation callbacks.
            // Re-read the current project after the website's download action.
            observed = None;
            *authorized_selection
                .lock()
                .map_err(|_| error("nexus_browser_unavailable"))? = None;
        }
        if pending.is_some() && observed.is_some() {
            if !window
                .url()
                .is_ok_and(|url| matches_project(url.as_str(), &target))
            {
                return Err(error("nexus_browser_page_unavailable"));
            }
            let selected = pending.take().expect("download exists");
            return Ok(SelectedDownload {
                url: selected.url,
                file_name: selected.file_name,
                page: observed.take().expect("project identity verified"),
            });
        }
        if let Some(code) = selection_timeout(
            started.elapsed(),
            content_seen,
            verification_started.map(|started| started.elapsed()),
        ) {
            let failure = error(code);
            session.diagnostics.blocked(&failure);
            return Err(failure);
        }
        if window.url().is_ok_and(|url| interactive_url(&url, &target)) {
            let generation = navigation_generation.load(Ordering::Acquire);
            let evaluation = if pending.is_some() {
                read_only.clone()
            } else {
                let selection = authorized_selection
                    .lock()
                    .map_err(|_| error("nexus_browser_unavailable"))?
                    .clone();
                format!("({script})({})", json!(selection))
            };
            let (sender, mut response) = tokio::sync::mpsc::channel(1);
            window
                .eval_with_callback(&evaluation, move |value| {
                    let _ = sender.try_send(value);
                })
                .map_err(|_| error("nexus_browser_unavailable"))?;
            if let Ok(Some(value)) =
                tokio::time::timeout(Duration::from_secs(2), response.recv()).await
                && value.len() <= 16384
                && let Ok(current) = window.url()
                && let Ok(observation) = serde_json::from_str::<PageObservation>(&value)
                && navigation_generation.load(Ordering::Acquire) == generation
            {
                if observation.automation.file_selection.is_some() {
                    // Share this lock with NavigationStarting so a navigation
                    // cannot invalidate the generation between checking and storing.
                    let mut selection = authorized_selection
                        .lock()
                        .map_err(|_| error("nexus_browser_unavailable"))?;
                    *selection = verified_file_selection(
                        &observation,
                        &current,
                        &target,
                        generation,
                        navigation_generation.load(Ordering::Acquire),
                    );
                }
                if matches_project(current.as_str(), &target)
                    && matches_project(&observation.page.url, &target)
                {
                    session.observe_auth(observation.auth)?;
                }
                session.diagnostics.observe(&observation, &current, &target);
                #[cfg(all(windows, feature = "desktop-reliability"))]
                smoke::record_live_automation(&session.app, &observation.automation);
                if rendered_current_page(&observation.rendered, &current, &target) {
                    content_seen = true;
                    if matches!(observation.automation.state, AutomationState::Blocked) {
                        let failure = blocked_error(observation.automation.reason.as_deref());
                        session.diagnostics.blocked(&failure);
                        return Err(failure);
                    }
                }
                if let Some(phase) = observation_phase(&observation, &current, &target) {
                    if phase == AutomationPhase::Verifying {
                        verification_started.get_or_insert_with(Instant::now);
                    } else {
                        verification_started = None;
                    }
                    if last_phase != Some(phase) {
                        session.diagnostics.stage(phase.name());
                        on_progress(phase.name())?;
                        last_phase = Some(phase);
                    }
                }
                if matches_project(current.as_str(), &target) {
                    observed = verified_page(observation.page, &target);
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[tauri::command]
pub(crate) async fn install_nexus_website_mod(
    app: tauri::AppHandle,
    session_id: String,
    instance_id: String,
    reference: String,
    on_progress: Channel<String>,
) -> Result<ManualModStageResult, String> {
    let session = match SessionLease::begin(&app, session_id.clone()) {
        Ok(session) => session,
        Err(failure) => {
            let log = app.state::<BrowserState>().attempt_log(&session_id);
            log.start(&instance_id, &reference);
            log.finish(Some(&failure));
            return Err(failure);
        }
    };
    session.diagnostics.start(&instance_id, &reference);
    let result = install_in_session(&app, &session, instance_id, reference, on_progress).await;
    session
        .diagnostics
        .finish(result.as_ref().err().map(String::as_str));
    result
}

async fn install_in_session(
    app: &tauri::AppHandle,
    session: &SessionLease,
    instance_id: String,
    reference: String,
    on_progress: Channel<String>,
) -> Result<ManualModStageResult, String> {
    session.diagnostics.stage("opening");
    on_progress
        .send("opening".into())
        .map_err(|_| error("nexus_browser_cancelled"))?;
    #[cfg(windows)]
    let target: NexusWebsiteTarget = serde_json::from_value(
        crate::runtime_service::wait_for_client(app)
            .await
            .map_err(preparation_error)?
            .request(
                "prepare_nexus_website_install",
                json!({ "instanceId": instance_id, "reference": reference }),
            )
            .await
            .map_err(preparation_error)?,
    )
    .map_err(|_| error("nexus_invalid_response"))?;
    #[cfg(not(windows))]
    let target = crate::commands::commands_nexus_website::prepare_nexus_website_install(
        app.state(),
        instance_id.clone(),
        reference,
    )
    .await?;
    session.check()?;
    let selected = select_download(session, target, &|phase| {
        on_progress
            .send(phase.into())
            .map_err(|_| error("nexus_browser_cancelled"))
    })
    .await?;
    session.check()?;
    session.diagnostics.stage("installing");
    on_progress
        .send("installing".into())
        .map_err(|_| error("nexus_browser_cancelled"))?;
    // The child view has closed. Keep this reservation until the runtime owner
    // finishes the committed installation, independently of frontend navigation.
    #[cfg(windows)]
    {
        let value = crate::runtime_service::wait_for_client(app)
            .await?
            .request(
                "install_nexus_website_download",
                json!({
                    "instanceId": instance_id, "reference": selected.page.canonical_url,
                    "operationId": session.id,
                    "downloadUrl": selected.url, "fileName": selected.file_name,
                    "title": selected.page.title, "iconUrl": selected.page.icon_url,
                }),
            )
            .await?;
        serde_json::from_value(value).map_err(|_| error("nexus_invalid_response"))
    }
    #[cfg(not(windows))]
    crate::commands::commands_nexus_website::install_nexus_website_download(
        app.clone(),
        app.state(),
        instance_id,
        session.id.clone(),
        selected
            .page
            .canonical_url
            .ok_or_else(|| error("nexus_reference_invalid"))?,
        selected.url,
        selected.file_name,
        selected.page.title,
        selected.page.icon_url,
    )
    .await
}

#[cfg(test)]
#[path = "nexus_browser_tests.rs"]
mod tests;

#[cfg(all(windows, feature = "desktop-reliability"))]
#[path = "nexus_browser_smoke.rs"]
pub(crate) mod smoke;
