//! Native acceptance: downloads render outside the main client, including failures.
use super::*;

#[derive(Default)]
pub(super) struct Progress(Mutex<Vec<String>>);

impl Progress {
    fn receive(&self, value: &str) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|_| "Progress observation poisoned")?
            .push(value.into());
        Ok(())
    }

    fn values(&self) -> Result<Vec<String>, String> {
        let values = self
            .0
            .lock()
            .map_err(|_| "Progress observation poisoned")?
            .clone();
        if values
            .iter()
            .any(|value| !matches!(value.as_str(), "selecting" | "waiting" | "verifying"))
            || values.windows(2).any(|pair| pair[0] == pair[1])
        {
            return Err(format!(
                "Website progress was invalid or duplicated: {values:?}"
            ));
        }
        Ok(values)
    }
}

pub(super) async fn verify(
    app: &tauri::AppHandle,
    root: &Path,
    ready: &AtomicBool,
) -> Result<serde_json::Value, String> {
    wait_until(
        || ready.load(Ordering::Acquire),
        "Fixture main page did not load",
    )
    .await?;
    let login = login::verify(app, root).await?;
    let main_ipc = main_ipc::verify(app, root).await?;
    let automatic = download(app, root, "automatic", Flow::Automatic).await?;
    let explicit = download(app, root, "explicit", Flow::Explicit).await?;
    let component = download(app, root, "component", Flow::Component).await?;
    let mut blocked = Vec::new();
    for (flow, code) in [
        (Flow::Ambiguous, "nexus_file_selection_required"),
        (Flow::Login, "nexus_website_login_required"),
        (Flow::Dialog, "nexus_website_action_required"),
        (Flow::Verification, "nexus_website_verification_required"),
    ] {
        blocked.push(blocked_case(app, root, flow, code).await?);
    }
    let mut cancelled = Vec::new();
    let mut cancelled_geometry = Vec::new();
    for cause in ["automatic", "command", "child_close", "main_reload"] {
        let geometry = cancel(app, root, cause).await?;
        cancelled.push(cause);
        cancelled_geometry.push(json!({ "cause": cause, "geometry": geometry }));
    }
    let diagnostics = logs::verify(root, &main_ipc)?;
    Ok(
        json!({ "status": "passed", "isolated_profile": true, "always_hidden": true,
        "automatic": automatic, "explicit_file": explicit, "file_component": component, "blocked": blocked,
        "cancelled": cancelled, "cancelled_geometry": cancelled_geometry,
        "background_geometry_monitored": true, "main_ipc": main_ipc, "main_survived": true,
        "top_level_windows": 1, "child_views_remaining": 0, "diagnostics": diagnostics, "login": login }),
    )
}

async fn attached(app: &tauri::AppHandle, fixture: &Fixture, id: &str) -> Result<Webview, String> {
    wait_until(
        || fixture.attached.load(Ordering::Acquire),
        "Native fixture did not attach",
    )
    .await?;
    let view = app
        .get_webview(&format!("{WINDOW_PREFIX}{id}"))
        .ok_or("Website child disappeared")?;
    if app.windows().len() != 1
        || view.window().label() != "main"
        || crate::curseforge_browser::main_window(app).is_none()
    {
        return Err("Hidden website created another window or lost its parent handle".into());
    }
    background_geometry(&view).await?;
    ipc_result(&view).await?;
    Ok(view)
}

pub(super) async fn monitor_background(
    app: &tauri::AppHandle,
    view: &Webview,
) -> Result<(), String> {
    while app.get_webview(view.label()).is_some() {
        match background_geometry(view).await {
            Ok(_) => (),
            Err(_) if app.get_webview(view.label()).is_none() => break,
            Err(error) => return Err(error),
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    Ok(())
}

async fn render(view: &Webview, progress: &Progress) -> Result<(), String> {
    tokio::time::sleep(Duration::from_millis(450)).await;
    background_geometry(view).await?;
    if !progress.values()?.is_empty() {
        return Err("Blank loading exposed the website or fabricated progress".into());
    }
    view.eval("window.__nexusSmokeRender = true")
        .map_err(|error| error.to_string())
}

async fn download(
    app: &tauri::AppHandle,
    root: &Path,
    name: &str,
    flow: Flow,
) -> Result<serde_json::Value, String> {
    let fixture = Fixture::new(root, name, flow)?;
    let id = open_nexus_website_host(app.clone())?;
    let session = SessionLease::begin(app, id.clone())?;
    let progress = Progress::default();
    let receive = |phase: &str| progress.receive(phase);
    let mut target = target();
    if matches!(flow, Flow::Explicit) {
        target.page_url.push_str("?tab=files&file_id=99");
    }
    let browse = select_download_with_fixture(&session, target, &receive, Some(&fixture));
    let inspect = async {
        let view = attached(app, &fixture, &id).await?;
        let geometry = background_geometry(&view).await?;
        tokio::try_join!(render(&view, &progress), monitor_background(app, &view))?;
        Ok::<_, String>(geometry)
    };
    let (selected, geometry) =
        tokio::time::timeout(CASE_TIMEOUT, async { tokio::try_join!(browse, inspect) })
            .await
            .map_err(|_| format!("Hidden automatic download timed out: {name}"))??;
    if selected.url != DOWNLOAD_URL
        || selected.file_name != "fixture.zip"
        || selected.page.title.as_deref() != Some(FIXTURE_TITLE)
        || selected.page.icon_url.as_deref() != Some(FIXTURE_ICON)
        || selected.page.canonical_url.as_deref() != Some(PROJECT_URL)
    {
        return Err("Automatic selection lost the filename, title or icon".into());
    }
    session.check()?;
    if !host::has_session(app) || app.get_webview(&format!("{WINDOW_PREFIX}{id}")).is_some() {
        return Err(
            "Selected download did not close its child and retain the owned reservation".into(),
        );
    }
    let events = fixture
        .downloads
        .lock()
        .map_err(|_| "Download observations poisoned")?
        .clone();
    if events.len() != 1 || !events[0].official_fixture_url || !events[0].cancelled_by_product {
        return Err("Expected one real download cancelled by the product".into());
    }
    let website = events[0]
        .website
        .as_ref()
        .ok_or("Download skipped the ordinary website controls")?;
    let file = if matches!(flow, Flow::Explicit) {
        "99"
    } else {
        "88"
    };
    if website.manual_clicks != 1
        || website.slow_clicks != 1
        || website.file_id != file
        || !website.countdown_finished
        || website.waited_ms < 700
    {
        return Err(format!(
            "Website buttons/countdown were bypassed or repeated: {}",
            json!(website)
        ));
    }
    if std::fs::read_dir(fixture.destination.parent().unwrap())
        .map_err(|error| error.to_string())?
        .next()
        .is_some()
    {
        return Err("The website browser wrote a file".into());
    }
    let phases = progress.values()?;
    if phases != ["selecting", "waiting"] {
        return Err(format!("Automatic stages were incomplete: {phases:?}"));
    }
    let component_redirects = fixture.component_redirects.load(Ordering::Acquire);
    if component_redirects != usize::from(matches!(flow, Flow::Component)) {
        return Err(format!(
            "Unexpected authorized website route count: {component_redirects}"
        ));
    }
    drop(session);
    ensure_closed(app).await?;
    if auth::read_nexus_website_auth_state(app.clone())? != auth::AuthState::SignedIn {
        return Err("The background website did not update the public authentication cache".into());
    }
    Ok(
        json!({ "always_hidden": true, "geometry": geometry, "progress": phases, "native_download_events": events,
        "metadata_after_download": true, "icon_preserved": true, "browser_file_written": false,
        "canonical_fallback": matches!(flow, Flow::Automatic), "resolved_project_url": selected.page.canonical_url,
        "open_shadow_download_control": matches!(flow, Flow::Automatic | Flow::Component), "auth_cache_updated": true,
        "authorized_website_route_redirect_fixture": matches!(flow, Flow::Component), "website_route_redirects":component_redirects,
        "selected_child_closed": true, "owned_install_reservation_retained": true }),
    )
}

async fn blocked_case(
    app: &tauri::AppHandle,
    root: &Path,
    flow: Flow,
    code: &str,
) -> Result<serde_json::Value, String> {
    let fixture = Fixture::new(root, &format!("blocked-{}", flow.name()), flow)?;
    let id = open_nexus_website_host(app.clone())?;
    let session = SessionLease::begin(app, id.clone())?;
    let progress = Progress::default();
    let receive = |phase: &str| progress.receive(phase);
    let browse = select_download_with_fixture(&session, target(), &receive, Some(&fixture));
    let inspect = async {
        let view = attached(app, &fixture, &id).await?;
        let geometry = background_geometry(&view).await?;
        tokio::try_join!(render(&view, &progress), monitor_background(app, &view))?;
        Ok::<_, String>(geometry)
    };
    let started = Instant::now();
    let (result, inspected) = tokio::time::timeout(Duration::from_secs(36), async {
        tokio::join!(browse, inspect)
    })
    .await
    .map_err(|_| format!("Hidden blocked flow did not terminate: {}", flow.name()))?;
    let geometry = inspected?;
    if result.err().as_deref() != Some(error(code).as_str()) {
        return Err(format!("Wrong blocked result for {}", flow.name()));
    }
    if matches!(flow, Flow::Login)
        && auth::read_nexus_website_auth_state(app.clone())? != auth::AuthState::SignedOut
    {
        return Err("The background login wall retained a stale signed-in observation".into());
    }
    let phases = progress.values()?;
    let loads = fixture.project_loads.load(Ordering::Acquire);
    if matches!(flow, Flow::Verification)
        && (phases != ["verifying"] || loads < 3 || started.elapsed() < Duration::from_secs(29))
    {
        return Err(format!(
            "Verification did not honor its cross-document deadline and single progress event: phases={phases:?}, project_loads={loads}, elapsed_ms={}",
            started.elapsed().as_millis()
        ));
    }
    if !fixture
        .downloads
        .lock()
        .map_err(|_| "Download observations poisoned")?
        .is_empty()
        || fixture
            .website
            .lock()
            .map_err(|_| "Website observations poisoned")?
            .is_some()
    {
        return Err("A blocked website clicked a control or selected a download".into());
    }
    drop(session);
    ensure_closed(app).await?;
    Ok(
        json!({ "case": flow.name(), "code": code, "always_hidden": true, "geometry": geometry,
        "progress": phases, "project_loads": loads, "no_download": true, "no_control_clicked": true }),
    )
}

async fn cancel(
    app: &tauri::AppHandle,
    root: &Path,
    cause: &str,
) -> Result<geometry::BackgroundGeometry, String> {
    let fixture = Fixture::new(root, &format!("cancel-{cause}"), Flow::Automatic)?;
    let id = open_nexus_website_host(app.clone())?;
    let session = SessionLease::begin(app, id.clone())?;
    let progress = Progress::default();
    let receive = |phase: &str| progress.receive(phase);
    let browse = select_download_with_fixture(&session, target(), &receive, Some(&fixture));
    let trigger = async {
        let view = attached(app, &fixture, &id).await?;
        let geometry = background_geometry(&view).await?;
        let cancel_action = async {
            if cause == "automatic" {
                render(&view, &progress).await?;
                wait_until(
                    || {
                        fixture.website.lock().is_ok_and(|latest| {
                            latest.as_ref().is_some_and(|value| value.slow_clicks == 1)
                        })
                    },
                    "Cancellation never reached the ordinary countdown",
                )
                .await?;
            }
            background_geometry(&view).await?;
            match cause {
                "automatic" | "command" => close_nexus_website_host(app.clone(), id.clone()),
                "child_close" => view.close().map_err(|error| error.to_string())?,
                "main_reload" => crate::curseforge_browser::main_window(app)
                    .ok_or("Main window missing")?
                    .reload()
                    .map_err(|error| error.to_string())?,
                _ => unreachable!(),
            }
            Ok::<_, String>(())
        };
        tokio::try_join!(cancel_action, monitor_background(app, &view))?;
        Ok::<_, String>(geometry)
    };
    let (result, triggered) =
        tokio::time::timeout(CASE_TIMEOUT, async { tokio::join!(browse, trigger) })
            .await
            .map_err(|_| format!("Hidden cancellation timed out: {cause}"))?;
    let geometry = triggered?;
    if result.err() != Some(error("nexus_browser_cancelled")) {
        return Err(format!("Hidden cancellation failed: {cause}"));
    }
    if !fixture
        .downloads
        .lock()
        .map_err(|_| "Download observations poisoned")?
        .is_empty()
    {
        return Err("Cancelled website selected a download".into());
    }
    progress.values()?;
    drop(session);
    ensure_closed(app).await?;
    Ok(geometry)
}
