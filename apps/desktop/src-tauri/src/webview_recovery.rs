//! Owns recovery of the desktop view without replacing the backend or its servers.

#[cfg(windows)]
mod actions;
#[cfg(windows)]
mod native;
#[cfg(windows)]
mod native_navigation;
#[cfg(any(windows, test))]
mod policy;

#[cfg(windows)]
use std::sync::{Arc, Mutex};
#[cfg(windows)]
use std::time::{Duration, Instant};
#[cfg(windows)]
use tauri::Manager;

#[cfg(windows)]
use policy::{AttemptResult, Policy};

#[cfg(windows)]
#[derive(Clone)]
pub(crate) struct RecoveryConfig {
    pub window: tauri::utils::config::WindowConfig,
    pub log_path: std::path::PathBuf,
}

#[cfg(windows)]
#[derive(Clone, serde::Serialize)]
pub(crate) struct RecoverySnapshot {
    pub observer_ready: bool,
    pub generation: u64,
    pub recoveries: u64,
    pub failures: u64,
    pub page_loads: u64,
    pub paused: bool,
    pub last_failure_kind: Option<String>,
    pub last_error: Option<String>,
}

#[cfg(windows)]
struct RecoveryState {
    policy: Policy,
    snapshot: RecoverySnapshot,
    diagnostic: Option<native::Failure>,
    stopped: bool,
    window_visible: bool,
}

#[cfg(windows)]
struct Recovery {
    config: RecoveryConfig,
    state: Mutex<RecoveryState>,
    wake: tokio::sync::Notify,
    task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

#[cfg(windows)]
impl Recovery {
    fn lock(&self) -> std::sync::MutexGuard<'_, RecoveryState> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn failure(&self, failure: native::Failure) {
        let mut state = self.lock();
        if state.stopped || failure.generation != state.snapshot.generation {
            return;
        }
        state.snapshot.failures = state.snapshot.failures.saturating_add(1);
        state.snapshot.last_failure_kind = Some(actions::kind_name(failure.kind).into());
        state.policy.record_failure(failure.kind, Instant::now());
        state.snapshot.paused = state.policy.paused();
        // One slot coalesces repeated native notifications; no unbounded event queue.
        state.diagnostic = Some(failure);
        drop(state);
        self.wake.notify_one();
    }

    fn stop(&self) {
        let mut state = self.lock();
        state.stopped = true;
        state.policy.shutdown();
        state.snapshot.observer_ready = false;
        drop(state);
        self.wake.notify_waiters();
        if let Some(task) = self
            .task
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            task.abort();
        }
    }
}

/// Installs exactly one owned worker. Native callbacks only update state and wake it.
#[cfg(windows)]
pub(crate) fn install(app: &tauri::AppHandle, config: RecoveryConfig) -> Result<(), String> {
    if config.window.label != "main" || app.get_webview_window("main").is_none() {
        return Err("WebView recovery requires the existing main window".into());
    }
    let recovery = Arc::new(Recovery {
        config,
        state: Mutex::new(RecoveryState {
            policy: Policy::default(),
            snapshot: RecoverySnapshot {
                observer_ready: false,
                generation: 1,
                recoveries: 0,
                failures: 0,
                page_loads: 0,
                paused: false,
                last_failure_kind: None,
                last_error: None,
            },
            diagnostic: None,
            stopped: false,
            window_visible: true,
        }),
        wake: tokio::sync::Notify::new(),
        task: Mutex::new(None),
    });
    if !app.manage(Arc::clone(&recovery)) {
        return Err("WebView recovery is already installed".into());
    }
    let worker = Arc::clone(&recovery);
    let app = app.clone();
    let task = tauri::async_runtime::spawn(async move { worker.run(app).await });
    *recovery
        .task
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = Some(task);
    Ok(())
}

#[cfg(windows)]
impl Recovery {
    async fn observe(self: &Arc<Self>, app: &tauri::AppHandle) -> Result<(), String> {
        let window = app
            .get_webview_window("main")
            .ok_or("Main window is missing")?;
        let generation = self.lock().snapshot.generation;
        let weak = Arc::downgrade(self);
        native::attach(
            &window,
            generation,
            Arc::new(move |failure| {
                if let Some(recovery) = weak.upgrade() {
                    recovery.failure(failure);
                }
            }),
        )
        .await?;
        let mut state = self.lock();
        if !state.stopped && state.snapshot.generation == generation {
            state.snapshot.observer_ready = true;
        }
        drop(state);
        self.wake.notify_one();
        Ok(())
    }

    async fn run(self: Arc<Self>, app: tauri::AppHandle) {
        if let Err(error) = self.observe(&app).await {
            {
                let mut state = self.lock();
                state.snapshot.last_error = Some(error.clone());
                // Missing native monitoring must not silently disable recovery.
                // Recreate under the same bounded budget used for browser loss.
                state
                    .policy
                    .record_failure(policy::FailureKind::BrowserExited, Instant::now());
            }
            actions::log(
                &self.config.log_path,
                "observer_failed",
                serde_json::json!({"error":error}),
            )
            .await;
        }
        loop {
            let (diagnostic, attempt, stopped, paused) = {
                let mut state = self.lock();
                (
                    state.diagnostic.take(),
                    state.policy.next_attempt(),
                    state.stopped,
                    state.policy.paused(),
                )
            };
            if stopped {
                return;
            }
            if let Some(failure) = diagnostic {
                actions::log(&self.config.log_path, "process_failed", serde_json::json!({
                    "generation":failure.generation, "kind":actions::kind_name(failure.kind),
                    "kind_code":failure.kind_code, "reason":failure.reason, "exit_code":failure.exit_code,
                    "paused":paused,
                })).await;
                if paused {
                    actions::show_paused(&app);
                }
            }
            let Some(attempt) = attempt else {
                self.wake.notified().await;
                continue;
            };
            let delay = attempt.due_at.saturating_duration_since(Instant::now());
            if !delay.is_zero() || actions::application_stopping(&app) {
                tokio::select! {
                    _ = tokio::time::sleep(if delay.is_zero() { Duration::from_millis(100) } else { delay }) => {},
                    _ = self.wake.notified() => {},
                }
                continue;
            }
            let Some(action) = self.lock().policy.begin(attempt.ticket, Instant::now()) else {
                continue;
            };
            let result = actions::recover(&self, &app, action).await;
            let accepted = {
                let mut state = self.lock();
                let accepted = state.policy.complete(
                    attempt.ticket,
                    if result.is_ok() {
                        AttemptResult::Succeeded
                    } else {
                        AttemptResult::Failed
                    },
                    Instant::now(),
                );
                if accepted {
                    match &result {
                        Ok(()) => {
                            state.snapshot.recoveries = state.snapshot.recoveries.saturating_add(1);
                            state.snapshot.last_error = None;
                        }
                        Err(error) => state.snapshot.last_error = Some(error.clone()),
                    }
                }
                state.snapshot.paused = state.policy.paused();
                accepted
            };
            actions::log(
                &self.config.log_path,
                "attempt_completed",
                serde_json::json!({
                    "action":format!("{action:?}"), "accepted":accepted, "error":result.err(),
                }),
            )
            .await;
            if self.lock().policy.paused() {
                actions::show_paused(&app);
            }
        }
    }
}

pub(crate) fn page_loaded(app: &tauri::AppHandle, label: &str, generation: u64) {
    #[cfg(windows)]
    if label == "main"
        && let Some(recovery) = app.try_state::<Arc<Recovery>>()
    {
        let mut state = recovery.lock();
        if state.stopped || state.snapshot.generation != generation {
            return;
        }
        state.snapshot.page_loads = state.snapshot.page_loads.saturating_add(1);
        drop(state);
        recovery.wake.notify_one();
    }
    #[cfg(not(windows))]
    let _ = (app, label, generation);
}

#[cfg(all(windows, feature = "desktop-reliability"))]
pub(crate) fn snapshot(app: &tauri::AppHandle) -> Option<RecoverySnapshot> {
    app.try_state::<Arc<Recovery>>()
        .map(|recovery| recovery.lock().snapshot.clone())
}

/// Retain the user's visibility intent even while a failed window is being replaced.
pub(crate) fn set_window_visible(app: &tauri::AppHandle, visible: bool) {
    #[cfg(windows)]
    if let Some(recovery) = app.try_state::<Arc<Recovery>>() {
        recovery.lock().window_visible = visible;
    }
    #[cfg(not(windows))]
    let _ = (app, visible);
}

/// Only a deliberate tray/show action reopens an exhausted automatic budget.
pub(crate) fn retry_from_user(app: &tauri::AppHandle) {
    #[cfg(windows)]
    if let Some(recovery) = app.try_state::<Arc<Recovery>>() {
        let mut state = recovery.lock();
        if (state.policy.paused() || state.snapshot.last_error.is_some())
            && state.policy.manual_retry(Instant::now())
        {
            state.snapshot.paused = false;
            drop(state);
            recovery.wake.notify_one();
        }
    }
    #[cfg(not(windows))]
    let _ = app;
}

pub(crate) fn shutdown(app: &tauri::AppHandle) {
    #[cfg(windows)]
    if let Some(recovery) = app.try_state::<Arc<Recovery>>() {
        recovery.stop();
    }
    #[cfg(not(windows))]
    let _ = app;
}

pub(crate) fn handle_run_event(app: &tauri::AppHandle, event: &tauri::RunEvent) {
    #[cfg(windows)]
    match event {
        tauri::RunEvent::ExitRequested {
            code: None, api, ..
        } => {
            if app
                .try_state::<Arc<Recovery>>()
                .is_some_and(|recovery| recovery.lock().policy.keep_alive())
            {
                api.prevent_exit();
            }
        }
        tauri::RunEvent::Exit => {
            shutdown(app);
            native_navigation::detach_on_ui_thread();
            native::detach_on_ui_thread();
        }
        _ => {}
    }
    #[cfg(not(windows))]
    let _ = (app, event);
}
