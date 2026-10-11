//! Windows STA registrations for the fixed, same-origin dropped-object frame.
use super::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use tauri::Manager;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2_4, ICoreWebView2File, ICoreWebView2FileSystemHandle,
    ICoreWebView2Frame, ICoreWebView2Frame2, ICoreWebView2FrameCreatedEventArgs,
    ICoreWebView2WebMessageReceivedEventArgs, ICoreWebView2WebMessageReceivedEventArgs2,
};
use webview2_com::{
    CoTaskMemPWSTR, FrameCreatedEventHandler, FrameDestroyedEventHandler,
    FrameWebMessageReceivedEventHandler,
};
use windows_core::{IUnknown, Interface, PWSTR};

struct FrameRegistration {
    frame: ICoreWebView2Frame2,
    message_token: i64,
    destroyed_token: i64,
    active: Rc<Cell<bool>>,
}

impl Drop for FrameRegistration {
    fn drop(&mut self) {
        self.active.set(false);
        // SAFETY: all owners and callbacks remain in the WebView2 STA.
        for result in unsafe {
            [
                self.frame.remove_WebMessageReceived(self.message_token),
                self.frame.remove_Destroyed(self.destroyed_token),
            ]
        } {
            if let Err(error) = result {
                eprintln!("Remove file-drop frame callback failed: {}", error.code().0);
            }
        }
    }
}

struct Registration {
    core: ICoreWebView2,
    extended: ICoreWebView2_4,
    token: i64,
    active: Rc<Cell<bool>>,
    policy: OriginPolicy,
    frame: Option<FrameRegistration>,
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.active.set(false);
        drop(self.frame.take());
        // SAFETY: the core subscription is removed on its owning STA.
        if let Err(error) = unsafe { self.extended.remove_FrameCreated(self.token) } {
            eprintln!("Remove file-drop frame observer failed: {}", error.code().0);
        }
    }
}

thread_local! {
    // A single frame per main WebView. Callbacks capture only active identities,
    // never a core/frame/AppHandle, so native registrations cannot form cycles.
    static REGISTRATION: RefCell<Option<Registration>> = const { RefCell::new(None) };
}

#[cfg(feature = "desktop-reliability")]
#[derive(Default, serde::Serialize)]
struct Diagnostics {
    callbacks: u64,
    drop_requests: u64,
    replies_sent: u64,
    object_count: u32,
    last_stage: &'static str,
    last_error: Option<&'static str>,
}
#[cfg(feature = "desktop-reliability")]
thread_local! { static DIAGNOSTICS: RefCell<Diagnostics> = RefCell::new(Diagnostics::default()); }
#[cfg(feature = "desktop-reliability")]
fn trace(stage: &'static str) {
    DIAGNOSTICS.with(|slot| {
        let mut state = slot.borrow_mut();
        state.last_stage = stage;
        match stage {
            "callback" => state.callbacks = state.callbacks.saturating_add(1),
            "drop_request" => state.drop_requests = state.drop_requests.saturating_add(1),
            "reply_sent" => state.replies_sent = state.replies_sent.saturating_add(1),
            _ => {}
        }
    });
}

fn read_string(
    fill: impl FnOnce(*mut PWSTR) -> windows_core::Result<()>,
    maximum: usize,
) -> Result<String, DropError> {
    let mut pointer = PWSTR::null();
    let read = fill(&mut pointer);
    let _allocated = CoTaskMemPWSTR::from(pointer);
    read.map_err(|_| DropError::PathUnavailable)?;
    if pointer.is_null() {
        return Err(DropError::PathUnavailable);
    }
    let mut length = 0;
    // SAFETY: WebView2 returns NUL-terminated, CoTaskMem-allocated UTF-16. The
    // guard frees it on every return, including the length/encoding error paths.
    unsafe {
        while length <= maximum && *pointer.0.add(length) != 0 {
            length += 1;
        }
        if length > maximum {
            return Err(DropError::InvalidPath);
        }
        String::from_utf16(std::slice::from_raw_parts(pointer.0, length))
            .map_err(|_| DropError::InvalidPath)
    }
}

fn same_frame(left: &ICoreWebView2Frame, right: &ICoreWebView2Frame2) -> bool {
    match (left.cast::<IUnknown>(), right.cast::<IUnknown>()) {
        (Ok(left), Ok(right)) => left.as_raw() == right.as_raw(),
        _ => false,
    }
}

fn frame_owner(
    frame: &ICoreWebView2Frame,
    active: &Rc<Cell<bool>>,
) -> Option<(ICoreWebView2, OriginPolicy)> {
    REGISTRATION.with(|slot| {
        let slot = slot.borrow();
        let current = slot.as_ref()?;
        let registered = current.frame.as_ref()?;
        (current.active.get()
            && active.get()
            && Rc::ptr_eq(active, &registered.active)
            && same_frame(frame, &registered.frame))
        .then(|| (current.core.clone(), current.policy.clone()))
    })
}

fn remove_frame(active: &Rc<Cell<bool>>) {
    let previous = REGISTRATION.with(|slot| {
        let mut slot = slot.borrow_mut();
        let current = slot.as_mut()?;
        if current
            .frame
            .as_ref()
            .is_some_and(|frame| Rc::ptr_eq(&frame.active, active))
        {
            current.frame.take()
        } else {
            None
        }
    });
    drop(previous);
}

fn resolve_objects(
    args: &ICoreWebView2WebMessageReceivedEventArgs,
) -> Result<Vec<String>, DropError> {
    let extended = args
        .cast::<ICoreWebView2WebMessageReceivedEventArgs2>()
        .map_err(|_| DropError::UnsupportedRuntime)?;
    // SAFETY: the collection and its objects belong to this STA callback.
    let objects =
        unsafe { extended.AdditionalObjects() }.map_err(|_| DropError::UnsupportedRuntime)?;
    let mut count = 0;
    unsafe { objects.Count(&mut count) }.map_err(|_| DropError::UnsupportedObject)?;
    #[cfg(feature = "desktop-reliability")]
    DIAGNOSTICS.with(|slot| slot.borrow_mut().object_count = count);
    validate_paths(
        count,
        (0..count).map(|index| {
            let object = unsafe { objects.GetValueAtIndex(index) }
                .map_err(|_| DropError::UnsupportedObject)?;
            if let Ok(handle) = object.cast::<ICoreWebView2FileSystemHandle>() {
                return read_string(|pointer| unsafe { handle.Path(pointer) }, MAX_PATH_UNITS);
            }
            if let Ok(file) = object.cast::<ICoreWebView2File>() {
                return read_string(|pointer| unsafe { file.Path(pointer) }, MAX_PATH_UNITS);
            }
            Err(DropError::UnsupportedObject)
        }),
    )
}

fn permitted(core: &ICoreWebView2, policy: &OriginPolicy, source: &str) -> bool {
    // SAFETY: Source is read only on the core's STA.
    read_string(|pointer| unsafe { core.Source(pointer) }, 16384)
        .is_ok_and(|current| policy.accepts_frame(source, &current))
}

fn receive(
    frame: &ICoreWebView2Frame,
    args: &ICoreWebView2WebMessageReceivedEventArgs,
    active: &Rc<Cell<bool>>,
) -> windows_core::Result<()> {
    #[cfg(feature = "desktop-reliability")]
    trace("callback");
    let Some((core, policy)) = frame_owner(frame, active) else {
        return Ok(());
    };
    let name = read_string(|pointer| unsafe { frame.Name(pointer) }, 128);
    let source = read_string(|pointer| unsafe { args.Source(pointer) }, 16384);
    let (Ok(name), Ok(source)) = (name, source) else {
        return Ok(());
    };
    if name != FRAME_NAME || !permitted(&core, &policy, &source) {
        #[cfg(feature = "desktop-reliability")]
        trace("origin_or_frame_rejected");
        return Ok(());
    }
    let Ok(message) = read_string(
        |pointer| unsafe { args.WebMessageAsJson(pointer) },
        MAX_REQUEST_BYTES,
    ) else {
        return Ok(());
    };
    let Some((request_id, valid)) = parse_request(&message) else {
        return Ok(());
    };
    #[cfg(feature = "desktop-reliability")]
    trace("drop_request");
    let result = valid.and_then(|()| resolve_objects(args));
    #[cfg(feature = "desktop-reliability")]
    DIAGNOSTICS.with(|slot| {
        slot.borrow_mut().last_error = result.as_ref().err().map(|error| error.code())
    });
    // Replies go to the trusted parent, never the frame which may have navigated.
    if !active.get() || !permitted(&core, &policy, &source) {
        return Ok(());
    }
    let wide = reply_json(&request_id, result)
        .encode_utf16()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let posted = unsafe { core.PostWebMessageAsJson(windows_core::PCWSTR(wide.as_ptr())) };
    #[cfg(feature = "desktop-reliability")]
    trace(if posted.is_ok() {
        "reply_sent"
    } else {
        "reply_failed"
    });
    posted
}

fn attach_frame(
    args: &ICoreWebView2FrameCreatedEventArgs,
    owner: &Rc<Cell<bool>>,
) -> windows_core::Result<()> {
    let frame = unsafe { args.Frame() }?;
    if read_string(|pointer| unsafe { frame.Name(pointer) }, 128)
        .ok()
        .as_deref()
        != Some(FRAME_NAME)
    {
        return Ok(());
    }
    let admitted = REGISTRATION.with(|slot| {
        slot.borrow().as_ref().is_some_and(|current| {
            current.active.get() && Rc::ptr_eq(owner, &current.active) && current.frame.is_none()
        })
    });
    if !admitted {
        return Ok(());
    }
    let frame = frame.cast::<ICoreWebView2Frame2>()?;
    let active = Rc::new(Cell::new(true));
    let callback_active = Rc::clone(&active);
    let message = FrameWebMessageReceivedEventHandler::create(Box::new(move |sender, args| {
        if callback_active.get()
            && let (Some(sender), Some(args)) = (sender, args)
        {
            if let Err(error) = receive(&sender, &args, &callback_active) {
                eprintln!("File-drop frame reply failed: {}", error.code().0);
            }
        }
        Ok(())
    }));
    let destroyed_active = Rc::clone(&active);
    let destroyed = FrameDestroyedEventHandler::create(Box::new(move |_, _| {
        destroyed_active.set(false);
        remove_frame(&destroyed_active);
        Ok(())
    }));
    let mut message_token = 0;
    unsafe { frame.add_WebMessageReceived(&message, &mut message_token) }?;
    let mut destroyed_token = 0;
    if let Err(error) = unsafe { frame.add_Destroyed(&destroyed, &mut destroyed_token) } {
        active.set(false);
        if let Err(cleanup) = unsafe { frame.remove_WebMessageReceived(message_token) } {
            eprintln!(
                "Roll back file-drop frame handler failed: {}",
                cleanup.code().0
            );
        }
        return Err(error);
    }
    REGISTRATION.with(|slot| {
        if let Some(current) = slot.borrow_mut().as_mut() {
            current.frame = Some(FrameRegistration {
                frame,
                message_token,
                destroyed_token,
                active,
            });
        }
    });
    #[cfg(feature = "desktop-reliability")]
    trace("frame_attached");
    Ok(())
}

fn policy(window: &tauri::WebviewWindow) -> Result<OriginPolicy, String> {
    let config = window.app_handle().config();
    if let Some(tauri::utils::config::FrontendDist::Url(url)) = &config.build.frontend_dist {
        return OriginPolicy::local(url.as_str());
    }
    if cfg!(debug_assertions)
        && let Some(url) = &config.build.dev_url
    {
        return OriginPolicy::local(url.as_str());
    }
    let https = config
        .app
        .windows
        .iter()
        .find(|item| item.label == "main")
        .is_some_and(|item| item.use_https_scheme);
    Ok(OriginPolicy::assets(https))
}

pub(crate) fn install(window: &tauri::WebviewWindow) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Dropped-file bridge only supports the main view".into());
    }
    let policy = policy(window)?;
    window
        .with_webview(move |platform| {
            let attached = (|| -> windows_core::Result<()> {
                let core = unsafe { platform.controller().CoreWebView2() }?;
                if REGISTRATION.with(|slot| {
                    slot.borrow().as_ref().is_some_and(|current| {
                        current.core.as_raw() == core.as_raw() && current.policy == policy
                    })
                }) {
                    return Ok(());
                }
                detach_on_ui_thread();
                let extended = core.cast::<ICoreWebView2_4>()?;
                let active = Rc::new(Cell::new(true));
                let callback_active = Rc::clone(&active);
                let handler = FrameCreatedEventHandler::create(Box::new(move |_, args| {
                    if callback_active.get()
                        && let Some(args) = args
                    {
                        if let Err(error) = attach_frame(&args, &callback_active) {
                            eprintln!("Attach file-drop frame failed: {}", error.code().0);
                        }
                    }
                    Ok(())
                }));
                let mut token = 0;
                unsafe { extended.add_FrameCreated(&handler, &mut token) }?;
                REGISTRATION.with(|slot| {
                    *slot.borrow_mut() = Some(Registration {
                        core,
                        extended,
                        token,
                        active,
                        policy,
                        frame: None,
                    })
                });
                #[cfg(feature = "desktop-reliability")]
                trace("attached");
                Ok(())
            })();
            if let Err(error) = attached {
                eprintln!("Attach dropped-file bridge failed: {}", error.code().0);
            }
        })
        .map_err(|_| "Dispatch dropped-file bridge attachment failed".into())
}

pub(crate) fn detach_on_ui_thread() {
    let registration = REGISTRATION.with(|slot| slot.borrow_mut().take());
    drop(registration);
    #[cfg(feature = "desktop-reliability")]
    DIAGNOSTICS.with(|slot| *slot.borrow_mut() = Diagnostics::default());
}

#[cfg(feature = "desktop-reliability")]
pub(crate) async fn diagnostics(
    window: &tauri::WebviewWindow,
) -> Result<serde_json::Value, String> {
    let (reply, response) = tokio::sync::oneshot::channel();
    window
        .with_webview(move |_| {
            let mut snapshot = DIAGNOSTICS
                .with(|slot| serde_json::to_value(&*slot.borrow()))
                .unwrap_or_else(|_| serde_json::json!({"diagnostics_unavailable":true}));
            REGISTRATION.with(|slot| {
                snapshot["attached"] = serde_json::json!(slot.borrow().is_some());
                snapshot["frame_attached"] = serde_json::json!(
                    slot.borrow()
                        .as_ref()
                        .is_some_and(|current| current.frame.is_some())
                );
            });
            let _ = reply.send(snapshot);
        })
        .map_err(|_| "Dispatch native drop diagnostics failed")?;
    tokio::time::timeout(std::time::Duration::from_secs(2), response)
        .await
        .map_err(|_| "Native drop diagnostics timed out")?
        .map_err(|_| "Native drop diagnostics response was lost".into())
}
