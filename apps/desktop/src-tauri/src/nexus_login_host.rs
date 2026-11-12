//! User-operated website login, sharing the download browser's default profile.
use super::*;
use crate::nexus_browser::auth::{self, AuthState};
use serde::Deserialize;
use std::{sync::atomic::AtomicU64, time::Duration};
use tauri::{Rect, WebviewUrl, ipc::Channel, webview::PageLoadEvent};

const LOGIN_URL: &str =
    "https://users.nexusmods.com/auth/sign_in?redirect_url=https%3A%2F%2Fwww.nexusmods.com%2F";

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
            Err(error("nexus_browser_unavailable"))
        }
    }

    fn fits(self, size: PhysicalSize<u32>) -> bool {
        self.x + self.width <= f64::from(size.width) + 1.0
            && self.y + self.height <= f64::from(size.height) + 1.0
    }

    fn rect(self) -> Rect {
        Rect {
            position: PhysicalPosition::new(self.x as i32, self.y as i32).into(),
            size: PhysicalSize::new(self.width as u32, self.height as u32).into(),
        }
    }
}

pub(super) struct LoginState {
    bounds: Option<Bounds>,
    attaching: bool,
    loaded: bool,
    generation: Arc<AtomicU64>,
    on_auth_change: Channel<String>,
    last_auth: AuthState,
}

fn allowed(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.as_str().len() <= 16384
        && matches!(
            url.host_str(),
            Some("nexusmods.com" | "www.nexusmods.com" | "users.nexusmods.com")
        )
}

#[tauri::command]
pub(crate) fn open_nexus_login_host(
    app: tauri::AppHandle,
    bounds: Option<Bounds>,
    on_auth_change: Channel<String>,
) -> Result<String, String> {
    // Reserve synchronously, like CurseForge. A stale renderer cannot reserve
    // a child after its main-page navigation callback has cancelled the session.
    let bounds = bounds.map(Bounds::validate).transpose()?;
    let id = reserve(
        &app,
        SessionKind::Login(LoginState {
            bounds,
            attaching: false,
            loaded: false,
            generation: Arc::new(AtomicU64::new(0)),
            on_auth_change: on_auth_change.clone(),
            last_auth: AuthState::Unknown,
        }),
    )?;
    if on_auth_change
        .send(AuthState::Unknown.name().into())
        .is_err()
    {
        close(&app, Some(&id), false);
        return Err(error("nexus_browser_cancelled"));
    }
    Ok(id)
}

#[tauri::command]
pub(crate) async fn update_nexus_login_host(
    app: tauri::AppHandle,
    session_id: String,
    bounds: Option<Bounds>,
) -> Result<(), String> {
    let bounds = bounds.map(Bounds::validate).transpose()?;
    let parent = {
        let state = app.state::<BrowserState>();
        let mut active = state
            .session
            .lock()
            .map_err(|_| error("nexus_browser_unavailable"))?;
        let Some(session) = active.as_mut().filter(|session| {
            session.id == session_id && !session.cancelled.load(Ordering::Acquire)
        }) else {
            return Ok(());
        };
        let SessionKind::Login(login) = &mut session.kind else {
            return Ok(());
        };
        login.bounds = bounds;
        if session.view.is_none() && bounds.is_some() && !login.attaching {
            login.attaching = true;
            Some((session.parent.clone(), login.generation.clone()))
        } else {
            None
        }
    };
    if let Some((parent, generation)) = parent {
        match create_view(&app, &session_id, &parent, generation.clone()).await {
            Ok(mut pending) => {
                let state = app.state::<BrowserState>();
                let mut active = state
                    .session
                    .lock()
                    .map_err(|_| error("nexus_browser_unavailable"))?;
                if let Some(session) = active.as_mut().filter(|session| {
                    session.id == session_id && !session.cancelled.load(Ordering::Acquire)
                }) && let SessionKind::Login(login) = &mut session.kind
                {
                    login.attaching = false;
                    session.view = pending.0.take();
                    if let Some(view) = session.view.clone() {
                        tauri::async_runtime::spawn(observe_auth(
                            app.clone(),
                            session_id.clone(),
                            view,
                            generation,
                            session.cancelled.clone(),
                        ));
                    }
                }
                // If navigation/close overtook creation, PendingView closes it.
            }
            Err(failure) => {
                close(&app, Some(&session_id), false);
                return Err(failure);
            }
        }
    }
    apply_bounds(&app, &session_id)
}

fn apply_bounds(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    // Native queries may wait for the UI thread. Never keep the session lock
    // while doing so; page-load and close callbacks need the same reservation.
    let size = app
        .get_window("main")
        .ok_or_else(|| error("nexus_browser_cancelled"))?
        .inner_size()
        .map_err(|_| error("nexus_browser_unavailable"))?;
    let state = app.state::<BrowserState>();
    let active = state
        .session
        .lock()
        .map_err(|_| error("nexus_browser_unavailable"))?;
    let Some(session) = active
        .as_ref()
        .filter(|session| session.id == id && !session.cancelled.load(Ordering::Acquire))
    else {
        return Ok(());
    };
    let SessionKind::Login(login) = &session.kind else {
        return Ok(());
    };
    let Some(view) = &session.view else {
        return Ok(());
    };
    let result = if let Some(bounds) = login
        .bounds
        .filter(|bounds| login.loaded && bounds.fits(size))
    {
        view.set_bounds(bounds.rect()).and_then(|()| view.show())
    } else {
        view.hide()
    };
    result.map_err(|_| error("nexus_browser_unavailable"))
}

fn page_load(view: &Webview, id: &str, finished: bool) {
    if !finished {
        return;
    }
    let app = view.app_handle();
    let Some(state) = app.try_state::<BrowserState>() else {
        return;
    };
    let Ok(mut active) = state.session.lock() else {
        return;
    };
    let Some(session) = active
        .as_mut()
        .filter(|session| session.id == id && !session.cancelled.load(Ordering::Acquire))
    else {
        return;
    };
    let SessionKind::Login(login) = &mut session.kind else {
        return;
    };
    login.loaded = true;
    drop(active);
    let app = app.clone();
    let id = id.to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        let _ = apply_bounds(&app, &id);
    });
}

struct PendingView(Option<Webview>);

impl Drop for PendingView {
    fn drop(&mut self) {
        if let Some(view) = &self.0 {
            close_view(view);
        }
    }
}

async fn create_view(
    app: &tauri::AppHandle,
    id: &str,
    parent: &WebviewWindow,
    generation: Arc<AtomicU64>,
) -> Result<PendingView, String> {
    // Keep the untrusted-page label under the existing IPC-denied prefix.
    let label = format!("{}login-{id}", crate::nexus_browser::WINDOW_PREFIX);
    let popup_app = app.clone();
    let popup_label = label.clone();
    let page_id = id.to_owned();
    let builder = WebviewBuilder::new(&label, WebviewUrl::External("about:blank".parse().unwrap()))
        .focused(false)
        .on_navigation(move |url| {
            let accepted = allowed(url);
            if accepted {
                generation.fetch_add(1, Ordering::AcqRel);
            }
            accepted
        })
        .on_new_window(move |url, _| {
            // Official target=_blank links stay in this user-opened child.
            if allowed(&url)
                && let Some(view) = popup_app.get_webview(&popup_label)
            {
                let _ = view.navigate(url);
            }
            tauri::webview::NewWindowResponse::Deny
        })
        .on_page_load(move |view, payload| {
            page_load(
                &view,
                &page_id,
                matches!(payload.event(), PageLoadEvent::Finished) && allowed(payload.url()),
            )
        })
        .on_download(|_, _| false);
    let view = parent
        .as_ref()
        .window()
        .add_child(
            builder,
            PhysicalPosition::new(-1100, -800),
            PhysicalSize::new(1050, 760),
        )
        .map_err(|_| error("nexus_browser_unavailable"))?;
    let pending = PendingView(Some(view));
    let view = pending.0.as_ref().expect("new login child");
    view.hide()
        .map_err(|_| error("nexus_browser_unavailable"))?;
    #[cfg(all(windows, feature = "desktop-reliability"))]
    if let Some(fixture) = crate::nexus_browser::smoke::current_login_fixture(app) {
        fixture.attach(view).await?;
    }
    // No custom data directory or profile: login and hidden download children
    // use the application's same default WebView2 profile. Credentials stay in it.
    view.navigate(LOGIN_URL.parse().unwrap())
        .map_err(|_| error("nexus_browser_unavailable"))?;
    Ok(pending)
}

async fn observe_auth(
    app: tauri::AppHandle,
    id: String,
    view: Webview,
    generation: Arc<AtomicU64>,
    cancelled: Arc<AtomicBool>,
) {
    while !cancelled.load(Ordering::Acquire) && app.get_webview(view.label()).is_some() {
        let observed_generation = generation.load(Ordering::Acquire);
        let (reply, mut response) = tokio::sync::mpsc::channel(1);
        if view
            .eval_with_callback(auth::SCRIPT, move |value| {
                let _ = reply.try_send(value);
            })
            .is_err()
        {
            break;
        }
        if let Ok(Some(value)) = tokio::time::timeout(Duration::from_secs(2), response.recv()).await
            && value.len() <= 64
            && let Ok(Some(observed)) = serde_json::from_str::<Option<AuthState>>(&value)
        {
            let channel = {
                let state = app.state::<BrowserState>();
                let Ok(mut active) = state.session.lock() else {
                    break;
                };
                let Some(session) = active
                    .as_mut()
                    .filter(|session| session.id == id && !cancelled.load(Ordering::Acquire))
                else {
                    break;
                };
                let SessionKind::Login(login) = &mut session.kind else {
                    break;
                };
                if generation.load(Ordering::Acquire) != observed_generation {
                    None
                } else if state.remember_auth(observed).is_err() {
                    break;
                } else if login.last_auth != observed {
                    login.last_auth = observed;
                    Some(login.on_auth_change.clone())
                } else {
                    None
                }
            };
            if let Some(channel) = channel
                && !cancelled.load(Ordering::Acquire)
                && channel.send(observed.name().into()).is_err()
            {
                break;
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[tauri::command]
pub(crate) fn close_nexus_login_host(app: tauri::AppHandle, session_id: String) {
    let matching = app
        .state::<BrowserState>()
        .session
        .lock()
        .is_ok_and(|active| {
            active.as_ref().is_some_and(|session| {
                session.id == session_id && matches!(session.kind, SessionKind::Login(_))
            })
        });
    if matching {
        close(&app, Some(&session_id), false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nexus_login_navigation_accepts_only_official_https_hosts() {
        for value in [
            LOGIN_URL,
            "https://nexusmods.com/",
            "https://www.nexusmods.com/7daystodie/mods/2264",
        ] {
            assert!(allowed(&value.parse().unwrap()));
        }
        for value in [
            "http://users.nexusmods.com/auth/sign_in",
            "https://users.nexusmods.com.evil.test/",
            "https://users.nexusmods.com:444/",
            "https://cf-files.nexusmods.com/file.zip",
            "file:///C:/private",
            "https://accounts.example.test/",
        ] {
            assert!(!allowed(&value.parse().unwrap()), "{value}");
        }
        let mut with_credentials: reqwest::Url = LOGIN_URL.parse().unwrap();
        with_credentials.set_username("user").unwrap();
        with_credentials.set_password(Some("fixture")).unwrap();
        assert!(!allowed(&with_credentials));
    }

    #[test]
    fn nexus_login_bounds_validate_physical_coordinates_and_client_fit() {
        let good = Bounds {
            x: 10.0,
            y: 20.0,
            width: 640.0,
            height: 480.0,
        };
        assert!(good.validate().unwrap().fits(PhysicalSize::new(900, 700)));
        assert!(!good.fits(PhysicalSize::new(600, 400)));
        for x in [-1.0, f64::NAN, f64::INFINITY, 32769.0] {
            assert!(Bounds { x, ..good }.validate().is_err());
        }
        for height in [0.0, -1.0, f64::NAN] {
            assert!(Bounds { height, ..good }.validate().is_err());
        }
    }
}
