//! Opt-in native acceptance: a fresh browser profile, no production storage or service.
use super::*;
use host::{
    close_curseforge_browser_host, open_curseforge_browser_host, update_curseforge_browser_host,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tauri::{
    WebviewUrl, WebviewWindowBuilder,
    ipc::{Channel, InvokeResponseBody},
};

#[path = "curseforge_browser_capture.rs"]
mod capture;

pub(crate) fn run_if_requested() -> bool {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some("--curseforge-browser-smoke") {
        return false;
    }
    let result = match args.as_slice() {
        [_, root, reference] => run(PathBuf::from(root), reference, false),
        [_, root, reference, option] if option == "--capture-page" =>
            run(PathBuf::from(root), reference, true),
        _ => Err(
            "Usage: --curseforge-browser-smoke <new absolute directory> <ASA project URL> [--capture-page]".into(),
        ),
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
    true
}

fn run(root: PathBuf, reference: &str, capture_page: bool) -> Result<(), String> {
    let url = project_url(reference).ok_or("Expected an official ASA project URL")?;
    if !root.is_absolute() {
        return Err("Fixture directory must be absolute".into());
    }
    // create_dir refuses an existing profile/report, including production directories.
    std::fs::create_dir(&root).map_err(|error| error.to_string())?;
    unsafe {
        std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", root.join("profile"));
        for key in [
            "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
            "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER",
            "WEBVIEW2_PIPE_FOR_SCRIPT_DEBUGGER",
            "WEBVIEW2_WAIT_FOR_SCRIPT_DEBUGGER",
        ] {
            std::env::remove_var(key);
        }
    }
    let mut context = tauri::tauri_build_context!();
    context.config_mut().app.windows.clear();
    let app = tauri::Builder::default()
        .manage(BrowserState::new(root.join("desktop-ui/active.jsonl")))
        .setup(move |app| {
            // Hidden acceptance host: the test itself must not interrupt the desktop.
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External("about:blank".parse().unwrap()))
                .inner_size(if capture_page { 1560.0 } else { 1100.0 }, if capture_page { 1000.0 } else { 800.0 })
                .visible(false).focused(false).skip_taskbar(true).build()?;
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if capture_page {
                    let captured = capture::run(&handle, &url, &root).await;
                    let report = match &captured {
                        Ok(report) => report.clone(),
                        Err(error) => json!({ "status": "failed", "error": error }),
                    };
                    let persisted = std::fs::write(root.join("result.json"), report.to_string());
                    handle.exit(if captured.is_ok() && persisted.is_ok() { 0 } else { 1 });
                    return;
                }
                let state = handle.state::<BrowserState>();
                state.log("mod.reference.browser.started", Some(&url), "Isolated native acceptance");
                let verification_requested = Arc::new(AtomicBool::new(false));
                let observed_verification = verification_requested.clone();
                let progress = Channel::new(move |message| {
                    if matches!(message, InvokeResponseBody::Json(value) if value == "\"verification_required\"") {
                        observed_verification.store(true, Ordering::Relaxed);
                    }
                    Ok(())
                });
                let id = open_curseforge_browser_host(handle.clone(), None, progress).unwrap();
                let session = host::Resolving::begin(&handle, &id).unwrap();
                let mut child_observed = false;
                let mut only_main_window = true;
                let mut host_ever_visible = false;
                let mut child_ever_visible = false;
                let mut main_handle_available = true;
                let result = {
                  let resolving = browse(&handle, &url, &session, Instant::now() + BROWSE_TIMEOUT);
                  tokio::pin!(resolving);
                  loop {
                    tokio::select! {
                        result = &mut resolving => break result,
                        _ = tokio::time::sleep(Duration::from_millis(25)) => {
                            only_main_window &= handle.windows().len() == 1 && handle.get_window("main").is_some();
                            main_handle_available &= main_window(&handle).is_some();
                            host_ever_visible |= handle.get_window("main").unwrap().is_visible().unwrap_or(true);
                            for view in handle.webviews().into_values().filter(|view| is_browser_window(view.label())) {
                                child_observed = true;
                                child_ever_visible |= child_is_visible(&view).await.unwrap_or(true);
                                only_main_window &= view.window().label() == "main";
                                if !only_main_window { close_active_host(&handle); }
                                }
                        }
                    }
                  }
                };
                let (code, mut report) = match result {
                    Ok(project) => {
                        state.log("mod.reference.browser.resolved", Some(&url), &project.id);
                        (0, json!({ "status": "resolved", "url": url, "project_id": project.id, "title": project.title }))
                    }
                    Err(error) => {
                        state.log("mod.reference.browser.failed", Some(&url), &error);
                        (1, json!({ "status": "failed", "url": url, "error": error }))
                    }
                };
                let completed_page_hidden = session.set_progress(host::Progress::Loading).is_ok();
                report["completed_page_hidden"] = json!(completed_page_hidden);
                drop(session);
                report["child_observed"] = json!(child_observed);
                report["only_main_window"] = json!(only_main_window);
                report["host_ever_visible"] = json!(host_ever_visible);
                report["child_ever_visible"] = json!(child_ever_visible);
                report["verification_requested"] = json!(verification_requested.load(Ordering::Relaxed));
                report["main_handle_available"] = json!(main_handle_available);
                report["child_views_remaining"] = json!(handle.webviews().values().filter(|view| is_browser_window(view.label())).count());
                report["main_survived"] = json!(handle.get_webview_window("main").is_some());
                let lifecycle = verify_child_lifecycle(&handle, &url).await;
                report["lifecycle"] = match &lifecycle { Ok(value) => value.clone(), Err(error) => json!({ "error": error }) };
                let persisted = std::fs::write(root.join("result.json"), report.to_string());
                if let Err(error) = &persisted { eprintln!("Write smoke result: {error}"); }
                handle.exit(if persisted.is_ok() && lifecycle.is_ok() && completed_page_hidden && child_observed && only_main_window && !host_ever_visible && !child_ever_visible && main_handle_available { code } else { 1 });
            });
            Ok(())
        })
        .build(context).map_err(|error| error.to_string())?;
    app.run(|_, event| {
        // Closing the page must first produce a cancelled result and flush its log.
        if let tauri::RunEvent::ExitRequested {
            code: None, api, ..
        } = event
        {
            api.prevent_exit();
        }
    });
    Ok(())
}

async fn verify_child_lifecycle(
    app: &tauri::AppHandle,
    url: &str,
) -> Result<serde_json::Value, String> {
    let id = open_curseforge_browser_host(app.clone(), None, Channel::new(|_| Ok(())))?;
    let session = host::Resolving::begin(app, &id)?;
    let view = session.attach(url)?;
    let bounds =
        serde_json::from_value(json!({ "x": 60, "y": 100, "width": 700, "height": 400 })).unwrap();
    // Even stale frontend bounds cannot reveal a page still loading silently.
    update_curseforge_browser_host(app.clone(), id.clone(), Some(bounds)).await?;
    if child_is_visible(&view.0).await? {
        return Err("A background page became visible without requesting verification".into());
    }
    session.set_progress(host::Progress::VerificationRequired)?;
    update_curseforge_browser_host(app.clone(), id.clone(), Some(bounds)).await?;
    let position = view.0.position().map_err(|error| error.to_string())?;
    let size = view.0.size().map_err(|error| error.to_string())?;
    if position.x != 60 || position.y != 100 || size.width != 700 || size.height != 400 {
        return Err(format!("Unexpected child geometry: {position:?} {size:?}"));
    }
    if !child_is_visible(&view.0).await? {
        return Err("Verification did not reveal the embedded page".into());
    }
    session.set_progress(host::Progress::Loading)?;
    update_curseforge_browser_host(app.clone(), id.clone(), Some(bounds)).await?;
    if child_is_visible(&view.0).await? {
        return Err("The next background page inherited a visible verification area".into());
    }
    close_curseforge_browser_host(app.clone(), id).await;
    if session.check_active() != Err(browser_error("cancelled")) {
        return Err("Closing the host did not cancel recognition".into());
    }
    drop(view);
    drop(session);
    if app
        .webviews()
        .values()
        .any(|view| is_browser_window(view.label()))
        || app.get_webview_window("main").is_none()
    {
        return Err("Child close affected the main window or left a view behind".into());
    }
    Ok(
        json!({ "hidden_by_default": true, "verification_shown": true, "next_page_hidden": true,
            "bounds_updated": true, "cancelled": true, "main_survived": true, "child_views_remaining": 0 }),
    )
}

async fn child_is_visible(view: &tauri::Webview) -> Result<bool, String> {
    let (reply, response) = tokio::sync::oneshot::channel();
    view.with_webview(move |platform| {
        let mut visible = Default::default();
        // SAFETY: with_webview runs this query on the WebView2 STA.
        let result = unsafe { platform.controller().IsVisible(&mut visible) }
            .map(|()| visible.as_bool())
            .map_err(|error| error.to_string());
        let _ = reply.send(result);
    })
    .map_err(|error| error.to_string())?;
    tokio::time::timeout(Duration::from_secs(3), response)
        .await
        .map_err(|_| "Timed out querying child visibility".to_owned())?
        .map_err(|_| "Child visibility query was dropped".to_owned())?
}
