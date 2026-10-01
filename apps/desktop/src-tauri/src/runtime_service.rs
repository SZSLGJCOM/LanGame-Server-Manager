//! Current-user background runtime. The desktop is a reconnectable client;
//! only this process owns servers, storage mutations and recovery workers.
mod client;
mod events;
#[cfg(test)]
mod existing_acceptance_client;
mod exit_deadline;
mod exit_handoff;
#[cfg(feature = "desktop-reliability")]
mod fixture;
#[cfg(feature = "desktop-reliability")]
pub(crate) use fixture::try_fixture_firewall_boundary;
mod local_commands;
mod security;
mod server;
mod wire;

pub(crate) use client::stop_service;
#[cfg(test)]
pub(crate) use existing_acceptance_client::{
    ExistingClient, STOP_ERROR_CATEGORIES, STOP_ERROR_PREFIX, redact_backend_stop_error,
};

pub(crate) fn log_service_event(level: &str, action: &str, message: &str) {
    let entry = serde_json::json!({
        "ts_unix_ms": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64).unwrap_or_default(),
        "level": level, "action": action, "message": message,
        "context": { "pid": std::process::id() },
    });
    if let Err(error) =
        crate::desktop_app_log::append(&app_storage::StoragePaths::default().app_log_path(), &entry)
    {
        use std::io::Write;
        let _ = writeln!(
            std::io::stderr(),
            "Runtime service diagnostic logging failed: {error}"
        );
    }
}

pub(crate) fn run_if_requested() -> bool {
    if exit_handoff::run_if_requested() {
        return true;
    }
    #[cfg(feature = "desktop-reliability")]
    if fixture::run_if_requested() {
        return true;
    }
    #[cfg(not(feature = "desktop-reliability"))]
    if std::env::args()
        .skip(1)
        .any(|arg| arg.starts_with("--runtime-service-fixture"))
    {
        eprintln!("Runtime service acceptance requires a desktop-reliability feature build");
        std::process::exit(2);
    }
    if !std::env::args().any(|arg| arg == "--runtime-service") {
        return false;
    }
    if let Err(error) = server::run() {
        eprintln!("LanGame runtime service failed: {error}");
        std::process::exit(1);
    }
    true
}

pub(crate) fn setup(app: &mut tauri::App) -> Result<(), String> {
    use tauri::Manager;
    app.manage(client::connect_or_start()?);
    client::relay_events(app.handle().clone());
    Ok(())
}

pub(crate) fn forward(invoke: tauri::ipc::Invoke<tauri::Wry>) {
    client::forward(invoke);
}

pub(crate) fn interface_is_closing<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    use tauri::Manager;
    app.try_state::<client::Client>()
        .is_none_or(|client| client.is_closing())
}

pub(crate) fn is_local_command(command: &str) -> bool {
    matches!(
        command,
        "set_tray_locale"
            | "app_exit_status"
            | "check_app_update"
            | "install_app_update"
            | "register_media_cache_source"
            | "pick_directory_path"
            | "open_external_url"
            | "open_local_path"
    )
}

fn exit_grace(app: &tauri::AppHandle) -> std::time::Duration {
    #[cfg(feature = "desktop-reliability")]
    return fixture::exit_grace_timeout(app).unwrap_or(exit_deadline::EXIT_GRACE_TIMEOUT);
    #[cfg(not(feature = "desktop-reliability"))]
    {
        let _ = app;
        exit_deadline::EXIT_GRACE_TIMEOUT
    }
}

pub(crate) fn exit_status(app: &tauri::AppHandle) -> serde_json::Value {
    use tauri::Manager;
    serde_json::json!({
        "requested": app.state::<client::Client>().is_exit_requested(),
    })
}

pub(crate) fn request_stop_and_exit(app: tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    let grace = exit_grace(&app);
    let deadline = std::time::Instant::now() + grace;
    let deadline_tick_ms = exit_deadline::deadline_tick_ms(grace);
    let client = app.state::<client::Client>();
    let Some(request) = client.try_begin_exit() else {
        return;
    };
    let Some(target) = client.service_target() else {
        // A production client captures its target before setup succeeds.
        // Only a detached test client can reach this branch.
        app.exit(1);
        return;
    };
    exit_deadline::arm(target, deadline);
    request.commit();
    // Transfer deadline ownership to an independent native process before this
    // interface exits. Neither service IPC nor game saves can delay that handoff.
    let shutdown_app = app.clone();
    if let Err(error) = std::thread::Builder::new()
        .name("exit-ownership-handoff".into())
        .spawn(move || complete_exit_handoff(shutdown_app, deadline_tick_ms))
    {
        retain_local_exit_owner(&app, deadline_tick_ms, &error.to_string());
    }
    tauri::async_runtime::spawn_blocking(move || {
        // A recreated WebView reads the same latched intent through app_exit_status.
        if let Err(error) = app.emit("app-exit-requested", exit_status(&app)) {
            log_service_event(
                "warning",
                "app.exit.notification_failed",
                &error.to_string(),
            );
        }
        log_service_event(
            "info",
            "app.exit.requested",
            &format!(
                "Final tray exit requested; interface ownership is handed off immediately and background save/stop cleanup retains its {}-second deadline.",
                grace.as_secs()
            ),
        );
    });
}

fn complete_exit_handoff(app: tauri::AppHandle, deadline_tick_ms: u64) {
    use tauri::Manager;
    match app.state::<client::Client>().handoff_exit(deadline_tick_ms) {
        Ok(()) => {
            app.state::<client::Client>().close_interface();
            exit_deadline::finish_interface();
            app.exit(0);
        }
        Err(error) => retain_local_exit_owner(&app, deadline_tick_ms, &error),
    }
}

fn retain_local_exit_owner(app: &tauri::AppHandle, deadline_tick_ms: u64, error: &str) {
    use tauri::Manager;
    // Exceptional failure retains the already-armed local watchdog. Hide the UI
    // while retaining its process until service ownership is proven or cutoff.
    let fallback = app.clone();
    tauri::async_runtime::spawn(async move {
        if fallback
            .state::<client::Client>()
            .stop_for_tray_exit(deadline_tick_ms)
            .await
            .is_ok()
        {
            exit_deadline::finish_interface();
            fallback.exit(0);
        }
    });
    hide_exiting_interface(app);
    log_service_event(
        "error",
        "app.exit.handoff_failed",
        &format!(
            "Interface hidden; local cleanup owner retained until shutdown is accepted or its deadline: {error}"
        ),
    );
}

fn hide_exiting_interface(app: &tauri::AppHandle) {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    let _ = app.remove_tray_by_id("main");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::Manager;

    #[test]
    fn desktop_media_remains_available_without_backend_state_until_interface_closes() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let endpoint =
            security::Endpoint::isolated(&uuid::Uuid::new_v4().simple().to_string()).unwrap();
        app.manage(client::Client::new(endpoint));
        assert!(app.try_state::<crate::state::DesktopState>().is_none());
        let shutdown = crate::media_cache::app_shutdown_check(app.handle().clone());
        assert!(!shutdown());
        app.state::<client::Client>().close_interface();
        assert!(shutdown());
    }
}
