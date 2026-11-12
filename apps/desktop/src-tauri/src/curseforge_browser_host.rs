//! A native child webview lives only inside the main UI's recognition area.
use super::*;
use tauri::ipc::Channel;
use tauri::{PhysicalPosition, PhysicalSize, Rect, Webview, WebviewUrl, webview::WebviewBuilder};

#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Progress {
    Loading,
    VerificationRequired,
}

#[derive(Clone, Copy, Deserialize)]
pub(crate) struct Bounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl Bounds {
    fn validate(self) -> Result<Self, String> {
        if [self.x, self.y, self.width, self.height]
            .iter()
            .all(|value| value.is_finite() && *value >= 0.0 && *value <= 32768.0)
            && self.width >= 1.0
            && self.height >= 1.0
        {
            Ok(self)
        } else {
            Err(browser_error("browser_unavailable"))
        }
    }

    fn rect(self) -> Rect {
        Rect {
            position: PhysicalPosition::new(self.x as i32, self.y as i32).into(),
            size: PhysicalSize::new(self.width as u32, self.height as u32).into(),
        }
    }
}

pub(super) struct Session {
    id: String,
    parent: WebviewWindow,
    bounds: Option<Bounds>,
    view: Option<Webview>,
    resolving: bool,
    cancelled: bool,
    verification_required: bool,
    on_progress: Channel<Progress>,
}

// Tauri's get_webview_window stops recognizing a window while it has a child
// webview. Retain its original handle until the child is closed, so tray,
// shutdown and renderer recovery keep addressing the same main window.
pub(crate) fn main_window(app: &tauri::AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("main").or_else(|| {
        app.get_webview("main")?;
        let parent = app.try_state::<BrowserState>().and_then(|state| {
            let host = state.host.lock().ok()?;
            Some(host.as_ref()?.parent.clone())
        });
        // Never hold one provider's host lock while entering the other's state.
        parent.or_else(|| crate::nexus_browser::retained_main_window(app))
    })
}

#[tauri::command]
// No native window creation or blocking UI query occurs here. Keep reservation
// synchronous with the IPC/navigation callbacks so an old renderer cannot
// reserve a host after its Started callback has already cancelled that page.
pub(crate) fn open_curseforge_browser_host(
    app: tauri::AppHandle,
    bounds: Option<Bounds>,
    on_progress: Channel<Progress>,
) -> Result<String, String> {
    let bounds = bounds.map(Bounds::validate).transpose()?;
    let state = app.state::<BrowserState>();
    let mut host = state
        .host
        .lock()
        .map_err(|_| browser_error("browser_unavailable"))?;
    if host.is_some() {
        return Err(browser_error("busy"));
    }
    let parent = app
        .get_webview_window("main")
        .ok_or_else(|| browser_error("browser_unavailable"))?;
    let id = uuid::Uuid::new_v4().to_string();
    *host = Some(Session {
        id: id.clone(),
        parent,
        bounds,
        view: None,
        resolving: false,
        cancelled: false,
        verification_required: false,
        on_progress,
    });
    Ok(id)
}

fn place(view: &Webview, bounds: Option<Bounds>, size: PhysicalSize<u32>) -> Result<(), String> {
    let result = (|| -> tauri::Result<()> {
        if let Some(bounds) = bounds {
            // A resize can overtake a frontend measurement. Hide until its next
            // measurement instead of drawing over an adjacent control.
            if bounds.x + bounds.width <= f64::from(size.width) + 1.0
                && bounds.y + bounds.height <= f64::from(size.height) + 1.0
            {
                view.set_bounds(bounds.rect())?;
                return view.show();
            }
        }
        view.hide()
    })();
    result.map_err(|_| browser_error("browser_unavailable"))
}

#[tauri::command]
pub(crate) async fn update_curseforge_browser_host(
    app: tauri::AppHandle,
    session_id: String,
    bounds: Option<Bounds>,
) -> Result<(), String> {
    let bounds = bounds.map(Bounds::validate).transpose()?;
    // Native size queries wait for the UI thread; never hold our host mutex
    // while waiting, as UI callbacks also need the session for cancellation.
    let size = app
        .get_window("main")
        .ok_or_else(|| browser_error("cancelled"))?
        .inner_size()
        .map_err(|_| browser_error("browser_unavailable"))?;
    let state = app.state::<BrowserState>();
    let mut host = state
        .host
        .lock()
        .map_err(|_| browser_error("browser_unavailable"))?;
    if let Some(host) = host
        .as_mut()
        .filter(|host| host.id == session_id && !host.cancelled)
    {
        host.bounds = bounds;
        if let Some(view) = &host.view {
            place(view, bounds.filter(|_| host.verification_required), size)?;
        }
    }
    Ok(())
}

fn close_view(view: Webview) {
    if view.app_handle().get_webview(view.label()).is_some() {
        let _ = view.close();
    }
}

fn close(app: &tauri::AppHandle, id: Option<&str>, finished: bool) {
    let Some(state) = app.try_state::<BrowserState>() else {
        return;
    };
    let Ok(mut host) = state.host.lock() else {
        return;
    };
    if let Some(session) = host
        .as_mut()
        .filter(|session| id.is_none_or(|id| session.id == id))
    {
        session.cancelled = true;
        if let Some(view) = session.view.take() {
            close_view(view);
        }
        if finished || !session.resolving {
            *host = None;
        }
    }
}

pub(crate) fn close_active_host(app: &tauri::AppHandle) {
    close(app, None, false);
}

#[tauri::command]
pub(crate) async fn close_curseforge_browser_host(app: tauri::AppHandle, session_id: String) {
    close(&app, Some(&session_id), false);
}

pub(super) struct Resolving {
    app: tauri::AppHandle,
    id: String,
}

impl Resolving {
    pub(super) fn set_progress(&self, progress: Progress) -> Result<(), String> {
        let channel = {
            let state = self.app.state::<BrowserState>();
            let mut host = state
                .host
                .lock()
                .map_err(|_| browser_error("browser_unavailable"))?;
            let host = host
                .as_mut()
                .filter(|host| host.id == self.id && !host.cancelled)
                .ok_or_else(|| browser_error("cancelled"))?;
            host.verification_required = matches!(progress, Progress::VerificationRequired);
            if !host.verification_required {
                // A new page never inherits the previous page's on-screen slot.
                host.bounds = None;
                if let Some(view) = &host.view
                    && self.app.get_webview(view.label()).is_some()
                {
                    view.hide()
                        .map_err(|_| browser_error("browser_unavailable"))?;
                }
            }
            host.on_progress.clone()
        };
        channel
            .send(progress)
            .map_err(|_| browser_error("cancelled"))
    }

    pub(super) fn begin(app: &tauri::AppHandle, id: &str) -> Result<Self, String> {
        let state = app.state::<BrowserState>();
        let mut host = state
            .host
            .lock()
            .map_err(|_| browser_error("browser_unavailable"))?;
        let host = host
            .as_mut()
            .filter(|host| host.id == id && !host.cancelled)
            .ok_or_else(|| browser_error("cancelled"))?;
        if host.resolving {
            return Err(browser_error("busy"));
        }
        host.resolving = true;
        Ok(Self {
            app: app.clone(),
            id: id.to_owned(),
        })
    }

    pub(super) fn check_active(&self) -> Result<(), String> {
        let state = self.app.state::<BrowserState>();
        let host = state
            .host
            .lock()
            .map_err(|_| browser_error("browser_unavailable"))?;
        if host
            .as_ref()
            .is_some_and(|host| host.id == self.id && !host.cancelled)
            && self.app.get_webview("main").is_some()
        {
            Ok(())
        } else {
            Err(browser_error("cancelled"))
        }
    }

    pub(super) fn attach(&self, url: &str) -> Result<ChildView, String> {
        self.check_active()?;
        self.set_progress(Progress::Loading)?;
        let parent = main_window(&self.app).ok_or_else(|| browser_error("cancelled"))?;
        let allowed = url.to_owned();
        let label = format!("{WINDOW_PREFIX}{}", uuid::Uuid::new_v4().simple());
        let builder = WebviewBuilder::new(
            &label,
            WebviewUrl::External(url.parse().map_err(|_| browser_error("invalid_page"))?),
        )
        .focused(false)
        .on_navigation(move |next| same_page(next.as_str(), &allowed))
        .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
        .on_download(|_, _| false);
        // Keep a usable page viewport while loading in the background. The
        // child starts entirely outside the client area and is then hidden.
        let view = parent
            .as_ref()
            .window()
            .add_child(
                builder,
                PhysicalPosition::new(-1100, -800),
                PhysicalSize::new(1050, 760),
            )
            .map_err(|_| browser_error("browser_unavailable"))?;
        let child = ChildView(view);
        let state = self.app.state::<BrowserState>();
        let mut host = state
            .host
            .lock()
            .map_err(|_| browser_error("browser_unavailable"))?;
        let host = host
            .as_mut()
            .filter(|host| host.id == self.id && !host.cancelled)
            .ok_or_else(|| browser_error("cancelled"))?;
        child
            .0
            .hide()
            .map_err(|_| browser_error("browser_unavailable"))?;
        host.view = Some(child.0.clone());
        Ok(child)
    }
}

impl Drop for Resolving {
    fn drop(&mut self) {
        close(&self.app, Some(&self.id), true);
    }
}

pub(super) struct ChildView(pub(super) Webview);
impl Drop for ChildView {
    fn drop(&mut self) {
        close_view(self.0.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_bounds_reject_invalid_native_coordinates() {
        let good = Bounds {
            x: 10.0,
            y: 20.0,
            width: 640.0,
            height: 480.0,
        };
        assert!(good.validate().is_ok());
        for x in [-1.0, f64::NAN, f64::INFINITY, 32769.0] {
            assert!(Bounds { x, ..good }.validate().is_err());
        }
        for height in [0.0, -1.0, f64::NAN] {
            assert!(Bounds { height, ..good }.validate().is_err());
        }
    }
}
