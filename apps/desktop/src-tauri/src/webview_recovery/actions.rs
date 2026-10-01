use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tauri::Manager;

use super::policy::{Action, FailureKind};
use super::{Recovery, native, native_navigation};

const OPERATION_TIMEOUT: Duration = Duration::from_secs(15);

pub(super) fn kind_name(kind: FailureKind) -> &'static str {
    match kind {
        FailureKind::BrowserExited => "browser_exited",
        FailureKind::RendererExited => "renderer_exited",
        FailureKind::RendererUnresponsive => "renderer_unresponsive",
        FailureKind::Auxiliary => "auxiliary",
    }
}

pub(super) fn application_stopping(app: &tauri::AppHandle) -> bool {
    app.try_state::<crate::state::DesktopState>()
        .is_some_and(|state| state.shutdown_in_progress.load(Ordering::SeqCst))
}

pub(super) async fn log(path: &Path, event: &str, context: serde_json::Value) {
    let path = path.to_path_buf();
    let entry = serde_json::json!({
        "ts_unix_ms": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
        "action": format!("desktop.webview.{event}"),
        "context": context,
    });
    match tauri::async_runtime::spawn_blocking(move || {
        crate::desktop_app_log::append(&path, &entry)
    })
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(error)) => eprintln!("persist WebView recovery diagnostic: {error}"),
        Err(error) => eprintln!("join WebView recovery diagnostic writer: {error}"),
    }
}

pub(super) fn show_paused(app: &tauri::AppHandle) {
    if let Some(tray) = app.tray_by_id("main")
        && let Err(error) = tray.set_tooltip(Some(
            "LanGame 界面恢复已暂停；点击托盘重试 / Click to retry",
        ))
    {
        eprintln!("update recovery tray status: {error}");
    }
    if let Some(window) = app.get_webview_window("main")
        && let Err(error) = window.set_title("LanGame — 界面恢复已暂停，请点击托盘重试")
    {
        eprintln!("update recovery window status: {error}");
    }
}

fn ensure_active(recovery: &Recovery, app: &tauri::AppHandle) -> Result<(), String> {
    if recovery.lock().stopped || application_stopping(app) {
        Err("WebView recovery interrupted by application shutdown".into())
    } else {
        Ok(())
    }
}

pub(super) async fn recover(
    recovery: &Arc<Recovery>,
    app: &tauri::AppHandle,
    action: Action,
) -> Result<(), String> {
    ensure_active(recovery, app)?;
    let previous_loads = match action {
        Action::Reload => {
            let window = app
                .get_webview_window("main")
                .ok_or("Main window is missing")?;
            let generation = recovery.lock().snapshot.generation;
            native_navigation::reload(&window, generation).await?;
            None
        }
        Action::Recreate => Some(recreate(recovery, app).await?),
    };
    let deadline = tokio::time::Instant::now() + OPERATION_TIMEOUT;
    while let Some(previous_loads) = previous_loads {
        ensure_active(recovery, app)?;
        {
            let state = recovery.lock();
            if state.snapshot.observer_ready && state.snapshot.page_loads > previous_loads {
                break;
            }
        }
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => return Err("WebView page load exceeded 15 seconds".into()),
            _ = recovery.wake.notified() => {},
        }
    }
    if let Some(tray) = app.tray_by_id("main") {
        tray.set_tooltip(Some("LanGame Server Manager"))
            .map_err(|error| format!("restore tray status: {error}"))?;
    }
    if let Some(window) = app.get_webview_window("main") {
        window
            .set_title(&recovery.config.window.title)
            .map_err(|error| format!("restore window title: {error}"))?;
    }
    Ok(())
}

async fn recreate(recovery: &Arc<Recovery>, app: &tauri::AppHandle) -> Result<u64, String> {
    let old_generation = {
        let mut state = recovery.lock();
        let generation = state.snapshot.generation;
        state.snapshot.generation = generation.saturating_add(1);
        state.snapshot.observer_ready = false;
        generation
    };
    if let Some(window) = app.get_webview_window("main") {
        if let Err(error) = native::detach(&window, old_generation).await {
            // Browser exit can invalidate COM before unregistration. Local delivery
            // is disabled by detach even then; destruction must still proceed.
            log(
                &recovery.config.log_path,
                "detach_failed",
                serde_json::json!({"error":error}),
            )
            .await;
        }
        ensure_active(recovery, app)?;
        window
            .destroy()
            .map_err(|error| format!("destroy failed WebView: {error}"))?;
    }
    let deadline = tokio::time::Instant::now() + OPERATION_TIMEOUT;
    while app.get_webview_window("main").is_some() {
        ensure_active(recovery, app)?;
        if tokio::time::Instant::now() >= deadline {
            return Err("WebView destruction exceeded 15 seconds".into());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let loads = recovery.lock().snapshot.page_loads;
    create_window(recovery, app).await?;
    recovery.observe(app).await?;
    Ok(loads)
}

struct PendingCreation {
    active: Arc<AtomicBool>,
    created: Arc<AtomicBool>,
    owner: Arc<Recovery>,
    app: tauri::AppHandle,
    generation: u64,
    committed: bool,
}

impl Drop for PendingCreation {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
        if self.committed {
            return;
        }
        let owner = Arc::clone(&self.owner);
        let created = Arc::clone(&self.created);
        let generation = self.generation;
        let app = self.app.clone();
        // Cancellation can also occur after the UI sent success but before the
        // worker consumed it. The generation protects a subsequent replacement.
        if let Err(error) = self.app.run_on_main_thread(move || {
            if created.swap(false, Ordering::AcqRel)
                && owner.lock().snapshot.generation == generation
                && let Some(window) = app.get_webview_window("main")
                && let Err(error) = window.destroy()
            {
                eprintln!("destroy abandoned WebView creation: {error}");
            }
        }) {
            eprintln!("dispatch abandoned WebView cleanup: {error}");
        }
    }
}

async fn create_window(recovery: &Arc<Recovery>, app: &tauri::AppHandle) -> Result<(), String> {
    let active = Arc::new(AtomicBool::new(true));
    let created = Arc::new(AtomicBool::new(false));
    let owner = Arc::clone(recovery);
    let app_handle = app.clone();
    let generation = recovery.lock().snapshot.generation;
    let mut pending = PendingCreation {
        active: Arc::clone(&active),
        created: Arc::clone(&created),
        owner: Arc::clone(recovery),
        app: app.clone(),
        generation,
        committed: false,
    };
    let (reply, response) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        if !active.load(Ordering::Acquire)
            || reply.is_closed()
            || owner.lock().snapshot.generation != generation
            || ensure_active(&owner, &app_handle).is_err()
        {
            return;
        }
        let result = tauri::WebviewWindowBuilder::from_config(&app_handle, &owner.config.window)
            .and_then(|mut builder| {
                // WindowConfig resolves relative profiles; the isolated native
                // acceptance host supplies a checked absolute directory instead.
                if let Some(path) = &owner.config.window.data_directory
                    && path.is_absolute()
                {
                    builder = builder.data_directory(path.clone());
                }
                builder
                    .visible(false)
                    .on_page_load(move |webview, payload| {
                        if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished) {
                            super::page_loaded(webview.app_handle(), webview.label(), generation);
                        }
                    })
                    .build()
            });
        match result {
            Ok(window) => {
                created.store(true, Ordering::Release);
                // A synchronous native build may finish after timeout/cancellation.
                // Its unclaimed window cannot survive into the next attempt.
                if !active.load(Ordering::Acquire)
                    || reply.is_closed()
                    || owner.lock().snapshot.generation != generation
                    || ensure_active(&owner, &app_handle).is_err()
                {
                    created.store(false, Ordering::Release);
                    if let Err(error) = window.destroy() {
                        eprintln!("destroy cancelled WebView creation: {error}");
                    }
                    return;
                }
                if let Err(error) = crate::desktop_window::prepare(&window).and_then(|()| {
                    if owner.lock().window_visible {
                        window
                            .show()
                            .map_err(|error| format!("show recovered desktop window: {error}"))
                    } else {
                        Ok(())
                    }
                }) {
                    // PendingCreation owns destruction after a failed placement.
                    let _ = reply.send(Err(format!("prepare recovered desktop window: {error}")));
                    return;
                }
                if reply.send(Ok(())).is_err() {
                    created.store(false, Ordering::Release);
                    if let Err(error) = window.destroy() {
                        eprintln!("destroy unclaimed WebView creation: {error}");
                    }
                }
            }
            Err(error) => {
                let _ = reply.send(Err(format!("recreate WebView: {error}")));
            }
        }
    })
    .map_err(|error| format!("dispatch WebView creation: {error}"))?;
    tokio::time::timeout(OPERATION_TIMEOUT, response)
        .await
        .map_err(|_| "WebView creation exceeded 15 seconds".to_string())?
        .map_err(|_| "WebView creation result channel closed".to_string())??;
    pending.committed = true;
    Ok(())
}
