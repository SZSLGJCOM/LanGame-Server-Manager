use std::path::PathBuf;
use std::time::Duration;

use tauri::Manager;
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment7;
use webview2_com::{CallDevToolsProtocolMethodCompletedHandler, take_pwstr};
use windows_core::{Interface, PCWSTR, PWSTR};

use super::protocol::Action;

/// No debugging listener is exposed. Both observation and fault injection run on
/// the existing WebView2 STA and can only address this fixture's main window.
pub(super) async fn observe_or_crash(
    app: &tauri::AppHandle,
    action: Action,
    profile: PathBuf,
) -> Result<u32, String> {
    let window = app
        .get_webview_window("main")
        .ok_or("Fixture main window is missing")?;
    let (reply, response) = tokio::sync::oneshot::channel();
    window
        .with_webview(move |platform| {
            if reply.is_closed() {
                return;
            }
            let result = (|| -> Result<u32, String> {
                let environment: ICoreWebView2Environment7 = platform
                    .environment()
                    .cast()
                    .map_err(|error| error.to_string())?;
                let mut actual_profile = PWSTR::null();
                let folder_result = unsafe { environment.UserDataFolder(&mut actual_profile) };
                let actual_profile = PathBuf::from(take_pwstr(actual_profile));
                folder_result.map_err(|error| error.to_string())?;
                if !dunce::simplified(&actual_profile)
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&profile.to_string_lossy())
                {
                    return Err("WebView2 did not use the fixture's isolated profile".into());
                }
                let core = unsafe { platform.controller().CoreWebView2() }
                    .map_err(|error| error.to_string())?;
                let mut pid = 0;
                unsafe { core.BrowserProcessId(&mut pid) }.map_err(|error| error.to_string())?;
                if pid == 0 {
                    return Err("WebView2 returned an empty browser PID".into());
                }
                let method = match action {
                    Action::CrashRenderer => Some("Page.crash"),
                    Action::CrashBrowser => Some("Browser.crash"),
                    _ => None,
                };
                if let Some(method) = method {
                    let method = method.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
                    let parameters = [b'{' as u16, b'}' as u16, 0];
                    // Crash completion is not an acknowledgement: the target process
                    // disappears. The shared native ProcessFailed observer is the proof.
                    let completed = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(
                        |result, _| {
                            if let Err(error) = result {
                                eprintln!("Fixture native crash callback: {error}");
                            }
                            Ok(())
                        },
                    ));
                    unsafe {
                        core.CallDevToolsProtocolMethod(
                            PCWSTR(method.as_ptr()),
                            PCWSTR(parameters.as_ptr()),
                            &completed,
                        )
                    }
                    .map_err(|error| format!("Request fixture native crash: {error}"))?;
                }
                Ok(pid)
            })();
            let _ = reply.send(result);
        })
        .map_err(|error| error.to_string())?;
    tokio::time::timeout(Duration::from_secs(3), response)
        .await
        .map_err(|_| "Fixture native operation exceeded three seconds".to_owned())?
        .map_err(|_| "Fixture native operation lost its response".to_owned())?
}
