//! Resolve public project pages in a normal browser, without an API key or proxy.
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::json;
use tauri::{Manager, WebviewWindow};

#[path = "curseforge_browser_host.rs"]
pub(crate) mod host;
pub(crate) use host::{close_active_host, main_window};

use crate::commands::mod_curseforge_reference::parse_reference;
use crate::commands::mod_presentations::{normalize_display_name, verified_icon_url};
use crate::commands::{
    ManualModPresentation, ManualModReferenceItem, ManualModReferenceResolveResult,
};

const WINDOW_PREFIX: &str = "curseforge-project-";
const BROWSE_TIMEOUT: Duration = Duration::from_secs(120);
const VERIFICATION_GRACE: Duration = Duration::from_secs(5);
const CACHE_TTL: Duration = Duration::from_secs(30 * 60);
const MAX_CACHE: usize = 256;

#[derive(Clone)]
struct Project {
    id: String,
    title: Option<String>,
    icon_url: Option<String>,
    observed: Instant,
}

pub(crate) struct BrowserState {
    // One interactive flow at a time. Cache is session-only and never holds page HTML.
    cache: tokio::sync::Mutex<HashMap<String, Project>>,
    host: std::sync::Mutex<Option<host::Session>>,
    log_path: PathBuf,
}

impl BrowserState {
    pub(crate) fn new(log_path: PathBuf) -> Self {
        Self {
            cache: tokio::sync::Mutex::new(HashMap::new()),
            host: std::sync::Mutex::new(None),
            log_path,
        }
    }

    fn log(&self, action: &str, url: Option<&str>, detail: &str) {
        let entry = json!({
            "ts_unix_ms": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
            "level": if action.ends_with("failed") { "error" } else { "info" },
            "action": action, "message": detail, "context": { "url": url }
        });
        if let Err(error) = crate::desktop_app_log::append(&self.log_path, &entry) {
            eprintln!("CurseForge diagnostic log failed: {error}");
        }
    }
}

pub(crate) fn is_browser_window(label: &str) -> bool {
    label.starts_with(WINDOW_PREFIX)
}

pub(crate) fn browser_error(reason: &str) -> String {
    json!({ "code": "curseforge_reference_failed", "reason": reason,
        "message": format!("CurseForge project page could not be resolved ({reason})") })
    .to_string()
}

fn project_url(reference: &str) -> Option<String> {
    let parsed = parse_reference(reference)?;
    if parsed.game_slug.as_deref() != Some("ark-survival-ascended") || parsed.project_id.is_some() {
        return None;
    }
    Some(format!(
        "https://www.curseforge.com/ark-survival-ascended/mods/{}",
        parsed.mod_slug?
    ))
}

fn same_page(value: &str, expected: &str) -> bool {
    reqwest::Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some("www.curseforge.com")
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none()
            && format!(
                "https://www.curseforge.com{}",
                url.path().trim_end_matches('/')
            ) == expected
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page {
    url: String,
    canonical_url: Option<String>,
    project_id: Option<String>,
    title: Option<String>,
    icon_url: Option<String>,
    state: String,
}

fn read_project(page: Page, expected: &str) -> Result<Option<Project>, String> {
    if !same_page(&page.url, expected)
        || page
            .canonical_url
            .as_deref()
            .is_some_and(|url| !same_page(url, expected))
        || page.state == "invalid"
    {
        return Err(browser_error("invalid_page"));
    }
    if page.state != "ready" {
        return Ok(None);
    }
    if page.canonical_url.is_none()
        || page
            .title
            .as_deref()
            .is_none_or(|title| title.trim().is_empty())
    {
        return Err(browser_error("invalid_page"));
    }
    let id = page
        .project_id
        .filter(|id| id.len() <= 10 && id.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|id| id.parse::<u32>().ok())
        .filter(|id| *id > 0)
        .map(|id| id.to_string())
        .ok_or_else(|| browser_error("invalid_page"))?;
    Ok(Some(Project {
        id,
        title: normalize_display_name(page.title.as_deref()),
        icon_url: verified_icon_url(page.icon_url.as_deref()),
        observed: Instant::now(),
    }))
}

#[derive(Default)]
struct VerificationGate {
    since: Option<Instant>,
    requested: bool,
}

impl VerificationGate {
    fn observe(&mut self, challenged: bool, now: Instant) -> bool {
        if !challenged {
            self.since = None;
            return false;
        }
        let since = *self.since.get_or_insert(now);
        if !self.requested && now.duration_since(since) >= VERIFICATION_GRACE {
            self.requested = true;
            return true;
        }
        false
    }
}

async fn browse(
    app: &tauri::AppHandle,
    url: &str,
    session: &host::Resolving,
    deadline: Instant,
) -> Result<Project, String> {
    if Instant::now() >= deadline {
        return Err(browser_error("timeout"));
    }
    let view = session.attach(url)?;
    let mut challenge_reported = false;
    let mut verification = VerificationGate::default();
    while Instant::now() < deadline {
        session.check_active()?;
        #[cfg(windows)]
        if app.try_state::<crate::state::StorageSource>().is_some()
            && crate::runtime_service::interface_is_closing(app)
        {
            return Err(browser_error("cancelled"));
        }
        let current = view.0.url().map_err(|_| browser_error("cancelled"))?;
        // about:blank may briefly precede the initial navigation.
        if current.as_str() != "about:blank" {
            if !same_page(current.as_str(), url) {
                return Err(browser_error("invalid_page"));
            }
            let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
            view.0
                .eval_with_callback(include_str!("curseforge_page_extract.js"), move |value| {
                    let _ = sender.try_send(value);
                })
                .map_err(|_| browser_error("browser_unavailable"))?;
            let wait = deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_secs(3));
            if let Ok(Some(value)) = tokio::time::timeout(wait, receiver.recv()).await
                && value.len() <= 8192
                && let Ok(page) = serde_json::from_str::<Page>(&value)
            {
                session.check_active()?;
                let current = view.0.url().map_err(|_| browser_error("cancelled"))?;
                if !same_page(current.as_str(), url) {
                    return Err(browser_error("invalid_page"));
                }
                let challenged = page.state == "challenge";
                if let Some(project) = read_project(page, url)? {
                    return Ok(project);
                }
                if challenged && !challenge_reported {
                    app.state::<BrowserState>().log(
                        "mod.reference.browser.challenge",
                        Some(url),
                        "Waiting for website verification",
                    );
                    challenge_reported = true;
                }
                if verification.observe(challenged, Instant::now()) {
                    session.set_progress(host::Progress::VerificationRequired)?;
                    app.state::<BrowserState>().log(
                        "mod.reference.browser.verification.required",
                        Some(url),
                        "Website verification is waiting in the import area",
                    );
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Err(browser_error("timeout"))
}

async fn resolve_runtime(
    app: &tauri::AppHandle,
    instance_id: String,
    references: Vec<String>,
    presentations: Vec<ManualModPresentation>,
) -> Result<ManualModReferenceResolveResult, String> {
    #[cfg(windows)]
    {
        let value = crate::runtime_service::wait_for_client(app)
            .await?
            .request(
                "resolve_manual_mod_references",
                json!({ "instanceId": instance_id, "references": references, "presentations": presentations }),
            )
            .await?;
        serde_json::from_value(value)
            .map_err(|error| format!("Invalid mod reference result: {error}"))
    }
    #[cfg(not(windows))]
    crate::commands::resolve_manual_mod_references(
        app.state(),
        instance_id,
        references,
        Some(presentations),
    )
    .await
}

#[tauri::command]
pub(crate) async fn resolve_curseforge_browser_references(
    app: tauri::AppHandle,
    instance_id: String,
    references: Vec<String>,
    browser_session: Option<String>,
) -> Result<ManualModReferenceResolveResult, String> {
    let references = crate::commands::normalize_manual_mod_references(references)?;
    if !references
        .iter()
        .any(|reference| project_url(reference).is_some())
    {
        return resolve_runtime(&app, instance_id, references, Vec::new()).await;
    }
    let session = host::Resolving::begin(
        &app,
        browser_session
            .as_deref()
            .ok_or_else(|| browser_error("browser_unavailable"))?,
    )?;
    // Check the actual module before showing an external page; callers cannot select a different game's resolver.
    #[cfg(windows)]
    let module = crate::runtime_service::wait_for_client(&app)
        .await?
        .request(
            "read_instance_details_from_storage",
            json!({ "instanceId": instance_id }),
        )
        .await?["summary"]["module_id"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    #[cfg(not(windows))]
    let module =
        crate::commands::read_instance_details_from_storage(app.state(), instance_id.clone())
            .await?
            .summary
            .module_id;
    if module != "arksurvivalascended" {
        return Err(browser_error("invalid_page"));
    }
    let state = app.state::<BrowserState>();
    let mut cache = state.cache.try_lock().map_err(|_| browser_error("busy"))?;
    cache.retain(|_, project| project.observed.elapsed() < CACHE_TTL);
    let deadline = Instant::now() + BROWSE_TIMEOUT;
    let mut rewritten = Vec::new();
    let mut items = Vec::new();
    for reference in references {
        session.check_active()?;
        let Some(url) = project_url(&reference) else {
            rewritten.push(reference.clone());
            items.push((reference, None, None));
            continue;
        };
        let result = if let Some(project) = cache.get(&url) {
            Ok(project.clone())
        } else {
            state.log(
                "mod.reference.browser.started",
                Some(&url),
                "Reading official project page",
            );
            let result = browse(&app, &url, &session, deadline).await;
            // Close the verification area before processing cached IDs or
            // waiting for the runtime. The native page has already closed.
            session.set_progress(host::Progress::Loading)?;
            result
        };
        match result {
            Ok(project) => {
                state.log("mod.reference.browser.resolved", Some(&url), &project.id);
                if cache.len() >= MAX_CACHE {
                    cache.clear();
                }
                cache.insert(url, project.clone());
                rewritten.push(project.id.clone());
                items.push((reference, Some(project), None));
            }
            Err(error) => {
                state.log("mod.reference.browser.failed", Some(&url), &error);
                if error == browser_error("cancelled") {
                    return Err(error);
                }
                items.push((reference, None, Some(error)));
            }
        }
    }
    if rewritten.is_empty() {
        return Err(items
            .iter()
            .find_map(|(_, _, error)| error.clone())
            .unwrap_or_else(|| browser_error("invalid_page")));
    }
    let presentations = items
        .iter()
        .filter_map(|(_, project, _)| {
            project.as_ref().map(|project| ManualModPresentation {
                id: project.id.clone(),
                title: project.title.clone(),
                icon_url: project.icon_url.clone(),
            })
        })
        .collect();
    let result = resolve_runtime(&app, instance_id, rewritten, presentations).await?;
    session.check_active()?;
    merge_result(result, items)
}

fn merge_result(
    mut result: ManualModReferenceResolveResult,
    items: Vec<(String, Option<Project>, Option<String>)>,
) -> Result<ManualModReferenceResolveResult, String> {
    if result.module_id != "arksurvivalascended" {
        return Err(browser_error("invalid_page"));
    }
    result.items = items
        .into_iter()
        .map(|(reference, project, error)| {
            if let Some(error) = error {
                return Ok(ManualModReferenceItem {
                    reference,
                    status: "failed".into(),
                    resolved_id: None,
                    title: None,
                    icon_url: None,
                    message: Some(error),
                });
            }
            let key = project
                .as_ref()
                .map_or(reference.as_str(), |project| project.id.as_str());
            let mut item = result
                .items
                .iter()
                .find(|item| item.reference.eq_ignore_ascii_case(key))
                .ok_or("Runtime did not return the requested mod reference")?
                .clone();
            item.reference = reference;
            if let Some(project) = project {
                item.title = project.title;
                if project.icon_url.is_some() {
                    item.icon_url = project.icon_url;
                }
            }
            Ok(item)
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(result)
}

#[cfg(test)]
#[path = "curseforge_browser_tests.rs"]
mod tests;

#[cfg(all(windows, feature = "desktop-reliability"))]
#[path = "curseforge_browser_smoke.rs"]
pub(crate) mod smoke;
