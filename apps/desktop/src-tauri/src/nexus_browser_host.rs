//! Shared reservation for hidden downloads and the explicitly opened login view.
use super::auth::AuthState;
use super::diagnostics::AttemptLog;
use super::error;
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tauri::{
    Manager, PhysicalPosition, PhysicalSize, Webview, WebviewWindow, webview::WebviewBuilder,
};

#[path = "nexus_login_host.rs"]
pub(crate) mod login;

#[derive(Default)]
pub(crate) struct BrowserState {
    session: Mutex<Option<Session>>,
    log_path: Option<PathBuf>,
    pub(super) auth_state: Mutex<AuthState>,
}

impl BrowserState {
    pub(crate) fn new(log_path: PathBuf) -> Self {
        Self {
            session: Mutex::new(None),
            log_path: Some(log_path),
            auth_state: Mutex::default(),
        }
    }

    pub(super) fn attempt_log(&self, session_id: &str) -> AttemptLog {
        AttemptLog::new(session_id, self.log_path.clone())
    }

    pub(super) fn remember_auth(&self, observed: AuthState) -> Result<(), String> {
        *self
            .auth_state
            .lock()
            .map_err(|_| error("nexus_browser_unavailable"))? = observed;
        Ok(())
    }
}

struct Session {
    id: String,
    parent: WebviewWindow,
    view: Option<Webview>,
    running: bool,
    cancelled: Arc<AtomicBool>,
    kind: SessionKind,
}

enum SessionKind {
    Download,
    Login(login::LoginState),
}

// The shared main-window lookup calls this only after releasing the other provider's lock.
pub(crate) fn retained_main_window(app: &tauri::AppHandle) -> Option<WebviewWindow> {
    let state = app.try_state::<BrowserState>()?;
    let active = state.session.lock().ok()?;
    Some(active.as_ref()?.parent.clone())
}

#[tauri::command]
pub(crate) fn open_nexus_website_host(app: tauri::AppHandle) -> Result<String, String> {
    reserve(&app, SessionKind::Download)
}

fn reserve(app: &tauri::AppHandle, kind: SessionKind) -> Result<String, String> {
    // Resolve the retained handle before taking our own lock. Native queries and
    // the other provider's fallback must never run under this session mutex.
    let parent = crate::curseforge_browser::main_window(app)
        .ok_or_else(|| error("nexus_browser_unavailable"))?;
    let state = app.state::<BrowserState>();
    let mut active = state
        .session
        .lock()
        .map_err(|_| error("nexus_browser_unavailable"))?;
    if active.is_some() {
        return Err(error("nexus_browser_busy"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    *active = Some(Session {
        id: id.clone(),
        parent,
        view: None,
        running: false,
        cancelled: Arc::new(AtomicBool::new(false)),
        kind,
    });
    Ok(id)
}

fn close_view(view: &Webview) {
    if view.app_handle().get_webview(view.label()).is_some() {
        let _ = view.close();
    }
}

fn close(app: &tauri::AppHandle, id: Option<&str>, finished: bool) {
    let Some(state) = app.try_state::<BrowserState>() else {
        return;
    };
    let Ok(mut active) = state.session.lock() else {
        return;
    };
    let Some(session) = active
        .as_mut()
        .filter(|session| id.is_none_or(|id| session.id == id))
    else {
        return;
    };
    session.cancelled.store(true, Ordering::Release);
    if let Some(view) = session.view.take() {
        close_view(&view);
    }
    if finished || !session.running {
        *active = None;
    }
}

pub(crate) fn close_active_host(app: &tauri::AppHandle) {
    close(app, None, false);
}

#[tauri::command]
pub(crate) fn close_nexus_website_host(app: tauri::AppHandle, session_id: String) {
    close(&app, Some(&session_id), false);
}

pub(super) struct SessionLease {
    pub(super) app: tauri::AppHandle,
    pub(super) id: String,
    pub(super) cancelled: Arc<AtomicBool>,
    pub(super) diagnostics: AttemptLog,
}

impl SessionLease {
    pub(super) fn observe_auth(&self, observed: Option<AuthState>) -> Result<(), String> {
        let Some(observed) = observed else {
            return Ok(());
        };
        let state = self.app.state::<BrowserState>();
        let active = state
            .session
            .lock()
            .map_err(|_| error("nexus_browser_unavailable"))?;
        if active.as_ref().is_some_and(|session| {
            session.id == self.id && !session.cancelled.load(Ordering::Acquire)
        }) {
            state.remember_auth(observed)?;
        }
        Ok(())
    }
    pub(super) fn begin(app: &tauri::AppHandle, id: String) -> Result<Self, String> {
        let state = app.state::<BrowserState>();
        let mut active = state
            .session
            .lock()
            .map_err(|_| error("nexus_browser_unavailable"))?;
        let session = active
            .as_mut()
            .filter(|session| {
                session.id == id
                    && !session.running
                    && matches!(session.kind, SessionKind::Download)
            })
            .ok_or_else(|| error("nexus_browser_cancelled"))?;
        if session.cancelled.load(Ordering::Acquire) {
            return Err(error("nexus_browser_cancelled"));
        }
        session.running = true;
        Ok(Self {
            app: app.clone(),
            diagnostics: state.attempt_log(&id),
            id,
            cancelled: session.cancelled.clone(),
        })
    }

    pub(super) fn check(&self) -> Result<(), String> {
        if self.cancelled.load(Ordering::Acquire) || self.app.get_webview("main").is_none() {
            Err(error("nexus_browser_cancelled"))
        } else {
            Ok(())
        }
    }

    pub(super) fn attach(&self, builder: WebviewBuilder<tauri::Wry>) -> Result<ChildView, String> {
        self.check()?;
        let parent = {
            let state = self.app.state::<BrowserState>();
            let active = state
                .session
                .lock()
                .map_err(|_| error("nexus_browser_unavailable"))?;
            active
                .as_ref()
                .filter(|session| session.id == self.id)
                .map(|session| session.parent.clone())
                .ok_or_else(|| error("nexus_browser_cancelled"))?
        };
        // Keep the download child entirely outside the parent's client area from
        // creation until close. Do not hide its controller: WebView2 stops rendering
        // hidden controllers and can throttle the ordinary website's loading work.
        // Explicit login has its own bounds/show/hide lifecycle in the login module.
        let view = parent
            .as_ref()
            .window()
            .add_child(
                builder.focused(false),
                PhysicalPosition::new(-1100, -800),
                PhysicalSize::new(1050, 760),
            )
            .map_err(|_| error("nexus_browser_unavailable"))?;
        let child = ChildView {
            view,
            session_id: self.id.clone(),
        };
        let state = self.app.state::<BrowserState>();
        let mut active = state
            .session
            .lock()
            .map_err(|_| error("nexus_browser_unavailable"))?;
        let session = active
            .as_mut()
            .filter(|session| session.id == self.id && !session.cancelled.load(Ordering::Acquire))
            .ok_or_else(|| error("nexus_browser_cancelled"))?;
        session.view = Some(child.view.clone());
        Ok(child)
    }
}

impl Drop for SessionLease {
    fn drop(&mut self) {
        close(&self.app, Some(&self.id), true);
    }
}

pub(super) struct ChildView {
    pub(super) view: Webview,
    session_id: String,
}

impl Drop for ChildView {
    fn drop(&mut self) {
        if let Some(state) = self.view.app_handle().try_state::<BrowserState>()
            && let Ok(mut active) = state.session.lock()
            && let Some(session) = active
                .as_mut()
                .filter(|session| session.id == self.session_id)
        {
            session.view = None;
        }
        // Closing the selected website does not cancel the runtime-owned installation.
        close_view(&self.view);
    }
}

#[cfg(all(windows, feature = "desktop-reliability"))]
pub(super) fn has_session(app: &tauri::AppHandle) -> bool {
    app.state::<BrowserState>()
        .session
        .lock()
        .is_ok_and(|session| session.is_some())
}
