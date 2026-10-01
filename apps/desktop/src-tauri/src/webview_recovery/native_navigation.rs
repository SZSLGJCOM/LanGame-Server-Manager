#![cfg(windows)]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::sync::oneshot;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_WEB_ERROR_STATUS, ICoreWebView2, ICoreWebView2NavigationCompletedEventArgs,
    ICoreWebView2NavigationStartingEventArgs,
};
use webview2_com::{NavigationCompletedEventHandler, NavigationStartingEventHandler};
use windows_core::BOOL;

const RELOAD_TIMEOUT: Duration = Duration::from_secs(15);

pub(super) fn detach_on_ui_thread() {
    if let Err(error) = clear_registration(None, None) {
        eprintln!("remove WebView2 navigation handlers during application exit: {error}");
    }
}

struct Navigation {
    armed: bool,
    id: Option<u64>,
    reply: Option<oneshot::Sender<Result<(), String>>>,
}

impl Navigation {
    fn finish(&mut self, result: Result<(), String>) {
        if let Some(reply) = self.reply.take() {
            let _ = reply.send(result);
        }
    }
}

struct Registration {
    core: ICoreWebView2,
    starting_token: Option<i64>,
    completed_token: Option<i64>,
    generation: u64,
    active: Arc<AtomicBool>,
    navigation: Rc<RefCell<Navigation>>,
}

thread_local! {
    // Only COM interfaces and UI-local callback state live here. No handler owns
    // an AppHandle/window, and no COM interface crosses the WebView2 STA.
    static REGISTRATION: RefCell<Option<Registration>> = const { RefCell::new(None) };
}

fn clear_registration(
    generation: Option<u64>,
    expected_active: Option<&Arc<AtomicBool>>,
) -> Result<(), String> {
    let registration = REGISTRATION.with(|slot| {
        let mut slot = slot.borrow_mut();
        let matches = slot.as_ref().is_some_and(|registration| {
            generation.is_none_or(|value| value == registration.generation)
                && expected_active.is_none_or(|value| Arc::ptr_eq(value, &registration.active))
        });
        if matches { slot.take() } else { None }
    });
    let Some(registration) = registration else {
        return Ok(());
    };
    registration.active.store(false, Ordering::Release);
    registration.navigation.borrow_mut().finish(Err(format!(
        "WebView2 reload navigation interrupted for generation {}",
        registration.generation
    )));
    let mut errors = Vec::new();
    // Release the TLS borrow before COM calls; attempt both removals even if a
    // dead browser rejects the first one. Disabled callbacks remain harmless.
    if let Some(token) = registration.starting_token {
        // SAFETY: this helper is invoked only by UI-thread dispatch.
        if let Err(error) = unsafe { registration.core.remove_NavigationStarting(token) } {
            errors.push(format!(
                "remove WebView2 NavigationStarting handler: {error}"
            ));
        }
    }
    if let Some(token) = registration.completed_token {
        // SAFETY: core and token belong to this STA registration.
        if let Err(error) = unsafe { registration.core.remove_NavigationCompleted(token) } {
            errors.push(format!(
                "remove WebView2 NavigationCompleted handler: {error}"
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

struct PendingReload {
    window: tauri::WebviewWindow,
    generation: u64,
    active: Arc<AtomicBool>,
    cleaned: bool,
}

impl Drop for PendingReload {
    fn drop(&mut self) {
        if self.cleaned {
            return;
        }
        self.active.store(false, Ordering::Release);
        let generation = self.generation;
        let active = Arc::clone(&self.active);
        // Cancellation includes the interval after the UI sends a result but
        // before the worker receives it. Identity protects a newer operation.
        if let Err(error) = self.window.run_on_main_thread(move || {
            if let Err(error) = clear_registration(Some(generation), Some(&active)) {
                eprintln!("cancel WebView2 reload navigation: {error}");
            }
        }) {
            eprintln!("dispatch cancelled WebView2 reload cleanup: {error}");
        }
    }
}

fn starting(
    navigation: &RefCell<Navigation>,
    args: Option<ICoreWebView2NavigationStartingEventArgs>,
) {
    if !navigation.borrow().armed || navigation.borrow().reply.is_none() {
        return;
    }
    let awaiting_start = navigation.borrow().id.is_none();
    let result = (|| {
        let args = args.ok_or("WebView2 NavigationStarting arguments are absent")?;
        if awaiting_start {
            let mut redirected = BOOL::default();
            // SAFETY: the base event interface exposes this on all supported runtimes.
            unsafe { args.IsRedirected(&mut redirected) }
                .map_err(|error| format!("read WebView2 navigation redirect state: {error}"))?;
            // A pending old navigation may redirect after Reload was requested.
            // Only a new non-redirect start can establish this reload's ID.
            if redirected.as_bool() {
                return Ok(None);
            }
        }
        let mut id = 0;
        // SAFETY: WebView2 invokes this callback with STA event arguments.
        unsafe { args.NavigationId(&mut id) }
            .map_err(|error| format!("read WebView2 starting NavigationId: {error}"))?;
        Ok(Some(id))
    })();
    let mut navigation = navigation.borrow_mut();
    match result {
        Ok(Some(id)) => match navigation.id {
            None => navigation.id = Some(id),
            Some(expected) if expected == id => {} // Redirects preserve the ID.
            Some(_) => navigation.finish(Err(
                "WebView2 reload was superseded by another navigation".into(),
            )),
        },
        Ok(None) => {}
        Err(error) => navigation.finish(Err(error)),
    }
}

fn completed(
    navigation: &RefCell<Navigation>,
    args: Option<ICoreWebView2NavigationCompletedEventArgs>,
) {
    let expected = {
        let navigation = navigation.borrow();
        if !navigation.armed || navigation.reply.is_none() {
            return;
        }
        navigation.id
    };
    // A completion from before Reload cannot satisfy this operation.
    let Some(expected) = expected else {
        return;
    };
    let result = (|| {
        let args = args.ok_or("WebView2 NavigationCompleted arguments are absent")?;
        let mut id = 0;
        // SAFETY: these out pointers and interfaces remain on the callback STA.
        unsafe { args.NavigationId(&mut id) }
            .map_err(|error| format!("read WebView2 completed NavigationId: {error}"))?;
        if id != expected {
            return Ok(None);
        }
        let mut success = BOOL::default();
        // SAFETY: as above; do not infer success from a page-load notification.
        unsafe { args.IsSuccess(&mut success) }
            .map_err(|error| format!("read WebView2 navigation success: {error}"))?;
        if success.as_bool() {
            return Ok(Some(()));
        }
        let mut status = COREWEBVIEW2_WEB_ERROR_STATUS::default();
        // SAFETY: diagnostics contain only a numeric status, never a URL.
        unsafe { args.WebErrorStatus(&mut status) }
            .map_err(|error| format!("read failed WebView2 navigation status: {error}"))?;
        Err(format!(
            "WebView2 reload navigation failed with status {}",
            status.0
        ))
    })();
    match result {
        Ok(Some(())) => navigation.borrow_mut().finish(Ok(())),
        Ok(None) => {}
        Err(error) => navigation.borrow_mut().finish(Err(error)),
    }
}

fn register_and_reload(
    core: ICoreWebView2,
    generation: u64,
    active: &Arc<AtomicBool>,
    navigation: &Rc<RefCell<Navigation>>,
) -> Result<(), String> {
    REGISTRATION.with(|slot| {
        *slot.borrow_mut() = Some(Registration {
            core: core.clone(),
            starting_token: None,
            completed_token: None,
            generation,
            active: Arc::clone(active),
            navigation: Rc::clone(navigation),
        });
    });
    let callback_active = Arc::clone(active);
    let callback_navigation = Rc::clone(navigation);
    let starting_handler = NavigationStartingEventHandler::create(Box::new(move |_, args| {
        if callback_active.load(Ordering::Acquire) {
            starting(&callback_navigation, args);
        }
        Ok(())
    }));
    let mut token = 0;
    // SAFETY: registration and both handlers are constructed on the WebView2 STA.
    unsafe { core.add_NavigationStarting(&starting_handler, &mut token) }
        .map_err(|error| format!("register WebView2 NavigationStarting handler: {error}"))?;
    REGISTRATION.with(|slot| {
        if let Some(registration) = slot.borrow_mut().as_mut() {
            registration.starting_token = Some(token);
        }
    });
    let callback_active = Arc::clone(active);
    let callback_navigation = Rc::clone(navigation);
    let completed_handler = NavigationCompletedEventHandler::create(Box::new(move |_, args| {
        if callback_active.load(Ordering::Acquire) {
            completed(&callback_navigation, args);
        }
        Ok(())
    }));
    // SAFETY: the completion token belongs to this same STA/core.
    unsafe { core.add_NavigationCompleted(&completed_handler, &mut token) }
        .map_err(|error| format!("register WebView2 NavigationCompleted handler: {error}"))?;
    REGISTRATION.with(|slot| {
        if let Some(registration) = slot.borrow_mut().as_mut() {
            registration.completed_token = Some(token);
        }
    });
    if !active.load(Ordering::Acquire) {
        return Err("WebView2 reload was cancelled before navigation".into());
    }
    // Arm only after both handlers exist, immediately before issuing Reload.
    navigation.borrow_mut().armed = true;
    // SAFETY: this is the UI dispatch, never an event handler.
    unsafe { core.Reload() }.map_err(|error| format!("start WebView2 reload navigation: {error}"))
}

pub(super) async fn reload(window: &tauri::WebviewWindow, generation: u64) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + RELOAD_TIMEOUT;
    if window.label() != "main" {
        return Err("reload WebView2: only the main window is supported".into());
    }
    let active = Arc::new(AtomicBool::new(true));
    let mut pending = PendingReload {
        window: window.clone(),
        generation,
        active: Arc::clone(&active),
        cleaned: false,
    };
    let (setup_reply, setup_response) = oneshot::channel();
    let (navigation_reply, navigation_response) = oneshot::channel();
    let ui_active = Arc::clone(&active);
    window
        .with_webview(move |platform| {
            if !ui_active.load(Ordering::Acquire) || setup_reply.is_closed() {
                return;
            }
            let result = (|| {
                clear_registration(None, None)?;
                // SAFETY: with_webview dispatches to the WebView2 STA.
                let core = unsafe { platform.controller().CoreWebView2() }
                    .map_err(|error| format!("get CoreWebView2 for reload: {error}"))?;
                let navigation = Rc::new(RefCell::new(Navigation {
                    armed: false,
                    id: None,
                    reply: Some(navigation_reply),
                }));
                register_and_reload(core, generation, &ui_active, &navigation)
            })();
            let failed = result.is_err();
            let unclaimed = setup_reply.send(result).is_err();
            if (failed || unclaimed || !ui_active.load(Ordering::Acquire))
                && let Err(error) = clear_registration(Some(generation), Some(&ui_active))
            {
                eprintln!("roll back WebView2 reload navigation: {error}");
            }
        })
        .map_err(|error| format!("dispatch WebView2 reload navigation: {error}"))?;

    let result = match tokio::time::timeout_at(deadline, async {
        // Reload itself must succeed, even if a callback arrived synchronously.
        setup_response
            .await
            .map_err(|_| "WebView2 reload setup channel closed".to_string())??;
        navigation_response
            .await
            .map_err(|_| "WebView2 reload navigation channel closed".to_string())?
    })
    .await
    {
        Ok(result) => result,
        Err(_) => Err("WebView2 reload navigation exceeded 15 seconds".into()),
    };

    active.store(false, Ordering::Release);
    let (cleanup_reply, cleanup_response) = oneshot::channel();
    window
        .run_on_main_thread(move || {
            let _ = cleanup_reply.send(clear_registration(Some(generation), Some(&active)));
        })
        .map_err(|error| {
            format!("dispatch WebView2 reload cleanup: {error}; navigation result: {result:?}")
        })?;
    let cleanup = match tokio::time::timeout_at(deadline, cleanup_response).await {
        Ok(Ok(result)) => {
            pending.cleaned = true;
            result
        }
        Ok(Err(_)) => Err("WebView2 reload cleanup channel closed".into()),
        Err(_) => Err("WebView2 reload cleanup exceeded the 15 second operation deadline".into()),
    };
    match (result, cleanup) {
        (Ok(()), result) | (result, Ok(())) => result,
        (Err(error), Err(cleanup)) => Err(format!("{error}; {cleanup}")),
    }
}
