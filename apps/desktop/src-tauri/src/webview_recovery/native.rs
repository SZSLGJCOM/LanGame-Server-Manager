#![cfg(windows)]

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::sync::oneshot;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_PROCESS_FAILED_KIND, COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE,
    COREWEBVIEW2_PROCESS_FAILED_REASON, ICoreWebView2, ICoreWebView2ProcessFailedEventArgs,
    ICoreWebView2ProcessFailedEventArgs2,
};
use webview2_com::ProcessFailedEventHandler;
use windows_core::Interface;

use super::policy::FailureKind;

const NATIVE_OPERATION_TIMEOUT: Duration = Duration::from_secs(3);

pub(super) struct Failure {
    pub generation: u64,
    pub kind: FailureKind,
    pub kind_code: i32,
    pub reason: Option<i32>,
    pub exit_code: Option<i32>,
}

struct Registration {
    core: ICoreWebView2,
    token: i64,
    generation: u64,
    active: Arc<AtomicBool>,
}

thread_local! {
    // COM interfaces stay in the WebView2 STA. Neither the registration nor its
    // event callback retains a Tauri window/AppHandle, avoiding an ownership cycle.
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
    if let Some(registration) = registration {
        // A dead browser can reject COM removal. Stop callback delivery regardless,
        // release our references, and report the actual native removal error.
        registration.active.store(false, Ordering::Release);
        // SAFETY: this helper is called only in a main-thread dispatch or RunEvent.
        unsafe { registration.core.remove_ProcessFailed(registration.token) }.map_err(|error| {
            format!(
                "remove WebView2 ProcessFailed handler for generation {}: {error}",
                registration.generation
            )
        })?;
    }
    Ok(())
}

struct PendingAttachment {
    window: tauri::WebviewWindow,
    generation: u64,
    active: Arc<AtomicBool>,
    committed: bool,
}

impl Drop for PendingAttachment {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        self.active.store(false, Ordering::Release);
        let generation = self.generation;
        let active = Arc::clone(&self.active);
        // Also covers cancellation after the UI sent success but before the
        // awaiting caller consumed it. Identity protects a newer same-generation
        // attachment from this delayed cleanup.
        if let Err(error) = self.window.run_on_main_thread(move || {
            if let Err(error) = clear_registration(Some(generation), Some(&active)) {
                eprintln!("cancel WebView2 handler attachment: {error}");
            }
        }) {
            eprintln!("dispatch cancelled WebView2 handler cleanup: {error}");
        }
    }
}

fn failure_details(generation: u64, args: Option<ICoreWebView2ProcessFailedEventArgs>) -> Failure {
    let mut failure = Failure {
        generation,
        kind: FailureKind::Auxiliary,
        kind_code: -1,
        reason: None,
        exit_code: None,
    };
    let Some(args) = args else {
        eprintln!("read WebView2 ProcessFailed event: native arguments are absent");
        return failure;
    };
    let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
    // SAFETY: WebView2 invokes this handler on its STA; out pointers are valid.
    if let Err(error) = unsafe { args.ProcessFailedKind(&mut kind) } {
        eprintln!("read WebView2 ProcessFailed kind: {error}");
        return failure;
    }
    failure.kind_code = kind.0;
    failure.kind = match kind {
        COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED => FailureKind::BrowserExited,
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED => FailureKind::RendererExited,
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE => {
            FailureKind::RendererUnresponsive
        }
        _ => FailureKind::Auxiliary,
    };
    // Older runtimes may not provide the extended diagnostic interface. The base
    // failure kind remains sufficient to choose recovery in that case.
    if let Ok(details) = args.cast::<ICoreWebView2ProcessFailedEventArgs2>() {
        let mut reason = COREWEBVIEW2_PROCESS_FAILED_REASON::default();
        let mut exit_code = 0;
        // SAFETY: the queried interface belongs to the same STA event arguments.
        if unsafe { details.Reason(&mut reason) }.is_ok() {
            failure.reason = Some(reason.0);
        }
        // SAFETY: as above; the output is used only after a successful call.
        if unsafe { details.ExitCode(&mut exit_code) }.is_ok() {
            failure.exit_code = Some(exit_code);
        }
    }
    failure
}

pub(super) async fn attach(
    window: &tauri::WebviewWindow,
    generation: u64,
    on_failure: Arc<dyn Fn(Failure) + Send + Sync>,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("attach WebView2 failure handler: only the main window is supported".into());
    }
    let active = Arc::new(AtomicBool::new(true));
    let mut pending = PendingAttachment {
        window: window.clone(),
        generation,
        active: Arc::clone(&active),
        committed: false,
    };
    let (reply, response) = oneshot::channel();
    window
        .with_webview(move |platform| {
            if !active.load(Ordering::Acquire) || reply.is_closed() {
                return;
            }
            let result = (|| {
                clear_registration(None, None)?;
                // SAFETY: with_webview dispatches this closure to the WebView2 STA.
                let core = unsafe { platform.controller().CoreWebView2() }
                    .map_err(|error| format!("get CoreWebView2 for failure monitoring: {error}"))?;
                let callback_active = Arc::clone(&active);
                let handler = ProcessFailedEventHandler::create(Box::new(move |_, args| {
                    if callback_active.load(Ordering::Acquire) {
                        let failure = failure_details(generation, args);
                        if callback_active.load(Ordering::Acquire) {
                            // The owner only updates bounded state and notifies its
                            // worker here; native callbacks must never rebuild inline.
                            on_failure(failure);
                        }
                    }
                    Ok(())
                }));
                let mut token = 0;
                // SAFETY: core and handler are created and registered on the STA.
                unsafe { core.add_ProcessFailed(&handler, &mut token) }
                    .map_err(|error| format!("register WebView2 ProcessFailed handler: {error}"))?;
                REGISTRATION.with(|slot| {
                    *slot.borrow_mut() = Some(Registration {
                        core,
                        token,
                        generation,
                        active: Arc::clone(&active),
                    });
                });
                Ok(())
            })();
            let cancelled = !active.load(Ordering::Acquire);
            if (reply.send(result).is_err() || cancelled)
                && let Err(error) = clear_registration(Some(generation), Some(&active))
            {
                eprintln!("roll back unclaimed WebView2 handler attachment: {error}");
            }
        })
        .map_err(|error| format!("dispatch WebView2 handler attachment: {error}"))?;
    await_native_result(response, "attach WebView2 failure handler").await?;
    pending.committed = true;
    Ok(())
}

pub(super) async fn detach(window: &tauri::WebviewWindow, generation: u64) -> Result<(), String> {
    if window.label() != "main" {
        return Err("detach WebView2 failure handler: only the main window is supported".into());
    }
    let (reply, response) = oneshot::channel();
    window
        .run_on_main_thread(move || {
            let result = clear_registration(Some(generation), None);
            let _ = reply.send(result);
        })
        .map_err(|error| format!("dispatch WebView2 handler removal: {error}"))?;
    await_native_result(response, "detach WebView2 failure handler").await
}

async fn await_native_result(
    response: oneshot::Receiver<Result<(), String>>,
    operation: &str,
) -> Result<(), String> {
    match tokio::time::timeout(NATIVE_OPERATION_TIMEOUT, response).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err(format!("{operation}: native result channel closed")),
        Err(_) => Err(format!("{operation}: native operation exceeded 3 seconds")),
    }
}

/// Must be called from the application's UI-thread RunEvent::Exit handler.
pub(super) fn detach_on_ui_thread() {
    if let Err(error) = clear_registration(None, None) {
        eprintln!("remove WebView2 handler during application exit: {error}");
    }
}
