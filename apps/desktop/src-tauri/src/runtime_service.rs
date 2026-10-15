//! Current-user background runtime. The desktop is a reconnectable client;
//! only this process owns servers, storage mutations and recovery workers.
mod client;
mod events;
#[cfg(test)]
mod existing_acceptance_client;
mod exit_deadline;
mod exit_handoff;
mod exit_intent;
#[cfg(feature = "desktop-reliability")]
mod fixture;
#[cfg(feature = "desktop-reliability")]
pub(crate) use fixture::try_fixture_firewall_boundary;
mod local_commands;
mod security;
mod server;
mod startup_state;
mod wire;

pub(crate) use client::stop_service;
#[cfg(test)]
pub(crate) use existing_acceptance_client::{
    ExistingClient, STOP_ERROR_CATEGORIES, STOP_ERROR_PREFIX, redact_backend_stop_error,
};

pub(crate) fn log_service_event<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    level: &str,
    action: &str,
    message: &str,
) {
    use tauri::Manager;
    let path = if let Some(state) = app.try_state::<crate::state::DesktopState>() {
        state
            .storage_source()
            .resolve_paths()
            .map(|paths| paths.app_log_path())
    } else {
        app.try_state::<crate::state::StorageSource>()
            .ok_or_else(|| String::from("Runtime client storage context is unavailable"))
            .and_then(|source| source.resolve_paths())
            .map(|paths| paths.app_log_path())
    };
    log_service_event_at(
        path.as_deref().map_err(String::as_str),
        level,
        action,
        message,
    );
}

fn log_service_event_at(
    path: Result<&std::path::Path, &str>,
    level: &str,
    action: &str,
    message: &str,
) {
    let entry = serde_json::json!({
        "ts_unix_ms": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64).unwrap_or_default(),
        "level": level, "action": action, "message": message,
        "context": { "pid": std::process::id() },
    });
    let persisted = path
        .map_err(|error| std::io::Error::other(error.to_owned()))
        .and_then(|path| crate::desktop_app_log::append(path, &entry));
    if let Err(error) = persisted {
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

pub(crate) fn setup(
    app: &mut tauri::App,
    storage_source: crate::state::StorageSource,
) -> Result<(), String> {
    setup_with_connector(app, storage_source, client::connect_or_start)
}

fn setup_with_connector(
    app: &mut tauri::App,
    storage_source: crate::state::StorageSource,
    connect: impl FnOnce(
        std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<client::Client, client::StartupFailure>
    + Send
    + 'static,
) -> Result<(), String> {
    use tauri::Manager;
    app.manage(storage_source);
    if !app.manage(startup_state::Startup::default()) {
        return Err(String::from(
            "Runtime service startup state was already initialized",
        ));
    }
    let startup_app = app.handle().clone();
    let cancellation = app.state::<startup_state::Startup>().cancellation();
    // Setup must return before the hidden window can render its loading state.
    // Only this worker installs a client; an active client never changes targets.
    tauri::async_runtime::spawn(async move {
        let result = match tauri::async_runtime::spawn_blocking(move || connect(cancellation)).await
        {
            Ok(Ok(client)) => {
                if startup_app.manage(client) {
                    Ok(())
                } else {
                    Err(String::from(
                        "Runtime service client was already initialized",
                    ))
                }
            }
            Ok(Err(error)) => {
                let mut message = error.message;
                // Failed startup may still own a detached process it created.
                // Retain that exact client for exit cleanup, never product work.
                if let Some(cleanup_client) = error.cleanup_client
                    && !startup_app.manage(*cleanup_client)
                {
                    message.push_str("; Runtime startup cleanup client was already initialized");
                }
                Err(message)
            }
            Err(error) => Err(format!("Runtime service startup worker failed: {error}")),
        };
        let failure = result.as_ref().err().cloned();
        // Client registration precedes publication so every successful waiter
        // can immediately obtain the exact same verified process identity.
        startup_app
            .state::<startup_state::Startup>()
            .complete(result);
        if let Some(error) = failure {
            log_service_event(
                &startup_app,
                "error",
                "runtime.client.startup_failed",
                &error,
            );
        }
    });
    client::relay_events(app.handle().clone());
    Ok(())
}

pub(super) async fn wait_for_client<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<tauri::State<'_, client::Client>, String> {
    use tauri::Manager;
    if let Some(startup) = app.try_state::<startup_state::Startup>() {
        if startup.is_exit_requested() {
            return Err(String::from("Application final exit is in progress"));
        }
        // Registration alone does not grant admission: the installed client
        // may be retained only to clean up a failed startup's detached child.
        startup.wait().await?;
        if startup.is_exit_requested() {
            return Err(String::from("Application final exit is in progress"));
        }
    }
    // Isolated fixtures install their verified client directly without a
    // production startup flight.
    app.try_state::<client::Client>()
        .ok_or_else(|| String::from("Runtime service startup completed without a client"))
}

pub(crate) fn forward(invoke: tauri::ipc::Invoke<tauri::Wry>) {
    client::forward(invoke);
}

pub(crate) fn interface_is_closing<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    use tauri::Manager;
    if let Some(startup) = app.try_state::<startup_state::Startup>() {
        if startup.is_exit_requested() {
            return true;
        }
        return app
            .try_state::<client::Client>()
            .is_some_and(|client| client.is_closing());
    }
    app.try_state::<client::Client>()
        .is_none_or(|client| client.is_closing())
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
        "requested": app.try_state::<startup_state::Startup>()
            .is_some_and(|startup| startup.is_exit_requested())
            || app.try_state::<client::Client>()
                .is_some_and(|client| client.is_exit_requested()),
    })
}

pub(crate) fn request_stop_and_exit(app: tauri::AppHandle) {
    use tauri::Manager;
    // The startup latch spans client installation, so repeated clicks on either
    // side of that boundary cannot restart the final-exit allowance.
    if let Some(startup) = app.try_state::<startup_state::Startup>()
        && !startup.try_begin_exit()
    {
        return;
    }
    let grace = exit_grace(&app);
    let deadline = std::time::Instant::now() + grace;
    let deadline_tick_ms = exit_deadline::deadline_tick_ms(grace);
    let startup_is_pending = app
        .try_state::<startup_state::Startup>()
        .is_some_and(|startup| !startup.is_complete());
    if !startup_is_pending && app.try_state::<client::Client>().is_some() {
        if !begin_connected_exit(app.clone(), deadline, deadline_tick_ms) {
            return;
        }
    } else {
        hide_exiting_interface(&app);
        let pending_app = app.clone();
        tauri::async_runtime::spawn(async move {
            let result = match pending_app.try_state::<startup_state::Startup>() {
                Some(startup) => startup.wait().await,
                None => Err(String::from("Runtime service startup state is unavailable")),
            };
            // Even failed startup may publish a cleanup-only client. Its exact
            // spawned child retains the original click's exit allowance.
            if pending_app.try_state::<client::Client>().is_some() {
                begin_connected_exit(pending_app, deadline, deadline_tick_ms);
            } else {
                // There is no connected target to own. Logging or a frozen
                // WebView must not retain this already-cancelled interface.
                exit_deadline::finish_interface();
                if let Err(error) = result {
                    log_service_event(
                        &pending_app,
                        "info",
                        "app.exit.startup_finished",
                        &format!(
                            "Runtime startup ended after interface exit was requested: {error}"
                        ),
                    );
                }
                pending_app.exit(0);
            }
        });
    }
    announce_exit_request(app, grace);
}

fn begin_connected_exit(
    app: tauri::AppHandle,
    deadline: std::time::Instant,
    deadline_tick_ms: u64,
) -> bool {
    use tauri::Manager;
    let client = app.state::<client::Client>();
    let Some(request) = client.try_begin_exit() else {
        return false;
    };
    let Some(target) = client.service_target() else {
        // A production client captures its target before setup succeeds.
        // Only a detached test client can reach this branch.
        app.exit(1);
        return false;
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
    true
}

fn announce_exit_request(app: tauri::AppHandle, grace: std::time::Duration) {
    use tauri::Emitter;
    tauri::async_runtime::spawn_blocking(move || {
        // A recreated WebView reads the same latched intent through app_exit_status.
        if let Err(error) = app.emit("app-exit-requested", exit_status(&app)) {
            log_service_event(
                &app,
                "warning",
                "app.exit.notification_failed",
                &error.to_string(),
            );
        }
        log_service_event(
            &app,
            "info",
            "app.exit.requested",
            &format!(
                "Final tray exit requested; background save/stop cleanup retains its original {}-second deadline.",
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
        app,
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
    use std::future::poll_fn;
    use std::task::Poll;
    use tauri::Manager;

    #[tokio::test]
    async fn cleanup_client_cannot_admit_work_before_or_after_startup_failure() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        app.manage(startup_state::Startup::default());
        let endpoint =
            security::Endpoint::isolated(&uuid::Uuid::new_v4().simple().to_string()).unwrap();
        app.manage(client::Client::new(endpoint));

        let mut waiting = Box::pin(wait_for_client(app.handle()));
        assert!(
            poll_fn(|cx| Poll::Ready(waiting.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        app.state::<startup_state::Startup>()
            .complete(Err(String::from("Spawned runtime did not become ready")));
        assert_eq!(
            waiting.await.err().expect("failed startup rejects work"),
            "Spawned runtime did not become ready"
        );
        assert_eq!(
            wait_for_client(app.handle())
                .await
                .err()
                .expect("cleanup client stays unavailable to product work"),
            "Spawned runtime did not become ready"
        );
        assert!(app.try_state::<client::Client>().is_some());
    }

    #[tokio::test]
    async fn directly_installed_fixture_client_does_not_require_startup_state() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let endpoint =
            security::Endpoint::isolated(&uuid::Uuid::new_v4().simple().to_string()).unwrap();
        app.manage(client::Client::new(endpoint));

        assert!(wait_for_client(app.handle()).await.is_ok());
    }

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

    #[test]
    fn desktop_media_remains_available_during_startup_until_exit_is_requested() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        app.manage(startup_state::Startup::default());
        let shutdown = crate::media_cache::app_shutdown_check(app.handle().clone());
        assert!(!shutdown());
        assert!(app.state::<startup_state::Startup>().try_begin_exit());
        assert!(shutdown());
    }
}
