//! Native window/tray checks in the runtime fixture's disposable namespace.
use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Value, json};
use tauri::Manager;
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment7;
use webview2_com::take_pwstr;
use windows_core::{Interface, PWSTR};

use super::super::config::{Config, checked_path, same_path};
use crate::runtime_service::client::Client;

const OPERATION_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) fn initialize_environment(config: &Config) -> Result<(), String> {
    config.verify()?;
    let profile = config.root.join("tray-profile");
    checked_path(&profile)?;
    std::fs::create_dir(&profile)
        .map_err(|error| format!("Create tray fixture profile: {error}"))?;
    // This fixture entry precedes every Tauri/runtime thread. Inherited WebView2
    // overrides must not load an unrelated profile or expose a debugging pipe.
    unsafe {
        std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", profile);
        for key in [
            "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
            "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER",
            "WEBVIEW2_PIPE_FOR_SCRIPT_DEBUGGER",
            "WEBVIEW2_WAIT_FOR_SCRIPT_DEBUGGER",
        ] {
            std::env::remove_var(key);
        }
    }
    Ok(())
}

pub(super) fn setup(app: &mut tauri::App, config: &Config) -> Result<(), String> {
    let url = tauri::Url::parse("about:blank").map_err(|error| error.to_string())?;
    tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::External(url))
        .title("LanGame runtime tray fixture")
        .data_directory(config.root.join("tray-profile"))
        .inner_size(480.0, 320.0)
        .focused(false)
        .build()
        .map_err(|error| format!("Create native tray fixture window: {error}"))?;
    crate::setup_tray(app).map_err(|error| format!("Create native fixture tray: {error}"))
}

async fn on_main<T, F>(app: &tauri::AppHandle, operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&tauri::AppHandle) -> Result<T, String> + Send + 'static,
{
    let handle = app.clone();
    let (reply, response) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        if !reply.is_closed() {
            let _ = reply.send(operation(&handle));
        }
    })
    .map_err(|error| format!("Dispatch native tray fixture operation: {error}"))?;
    tokio::time::timeout(OPERATION_TIMEOUT, response)
        .await
        .map_err(|_| "Native tray fixture operation timed out".to_owned())?
        .map_err(|_| "Native tray fixture operation lost its response".to_owned())?
}

async fn wait_visibility(app: &tauri::AppHandle, visible: bool) -> Result<(), String> {
    tokio::time::timeout(OPERATION_TIMEOUT, async {
        loop {
            let current = on_main(app, |app| {
                if app.tray_by_id("main").is_none() {
                    return Err("Native tray disappeared while the client was running".into());
                }
                if app.state::<Client>().is_closing() {
                    return Err("Closing the window marked the runtime client as closing".into());
                }
                app.get_webview_window("main")
                    .ok_or("Native fixture window was destroyed")?
                    .is_visible()
                    .map_err(|error| format!("Read native fixture visibility: {error}"))
            })
            .await?;
            if current == visible {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .map_err(|_| format!("Native fixture window did not reach visible={visible}"))?
}

async fn verify_profile(app: &tauri::AppHandle, profile: PathBuf) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or("Native fixture window is missing")?;
    let (reply, response) = tokio::sync::oneshot::channel();
    window
        .with_webview(move |platform| {
            if reply.is_closed() {
                return;
            }
            let result = (|| -> Result<(), String> {
                let environment: ICoreWebView2Environment7 = platform
                    .environment()
                    .cast()
                    .map_err(|error| error.to_string())?;
                let mut actual = PWSTR::null();
                let folder = unsafe { environment.UserDataFolder(&mut actual) };
                let actual = PathBuf::from(take_pwstr(actual));
                folder.map_err(|error| error.to_string())?;
                same_path(&actual, &profile)
            })();
            let _ = reply.send(result);
        })
        .map_err(|error| format!("Read native tray fixture profile: {error}"))?;
    tokio::time::timeout(OPERATION_TIMEOUT, response)
        .await
        .map_err(|_| "Native tray fixture profile observation timed out".to_owned())?
        .map_err(|_| "Native tray fixture profile observation lost its response".to_owned())?
}

pub(super) async fn exercise(
    app: &tauri::AppHandle,
    config: &Config,
    service_pid: u32,
) -> Result<Value, String> {
    verify_profile(app, config.root.join("tray-profile")).await?;
    on_main(app, |app| {
        for (locale, expected) in [
            (crate::TrayLocale::Chinese, ("打开", "退出")),
            (crate::TrayLocale::English, ("Open", "Exit")),
            (crate::TrayLocale::Chinese, ("打开", "退出")),
        ] {
            crate::set_tray_locale(locale, app.state())?;
            let menu = app.state::<crate::TrayMenu>();
            let show = menu.show.text().map_err(|error| error.to_string())?;
            let exit = menu.exit.text().map_err(|error| error.to_string())?;
            if (show.as_str(), exit.as_str()) != expected {
                return Err(format!("Unexpected native tray labels: {show:?}, {exit:?}"));
            }
        }
        Ok(())
    })
    .await?;
    wait_visibility(app, true).await?;
    on_main(app, |app| {
        app.get_webview_window("main")
            .ok_or("Native fixture window is missing")?
            .close()
            .map_err(|error| format!("Request native fixture window close: {error}"))
    })
    .await?;
    wait_visibility(app, false).await?;
    let client = app.state::<Client>();
    let status = tokio::time::timeout(
        OPERATION_TIMEOUT,
        client.request("runtime_service_status", Value::Null),
    )
    .await
    .map_err(|_| "Hidden client could not reach the runtime service in time".to_owned())??;
    if status["pid"] != json!(service_pid) {
        return Err("Closing to tray replaced the runtime service".into());
    }
    on_main(app, |app| {
        crate::show_main_window(app);
        Ok(())
    })
    .await?;
    wait_visibility(app, true).await?;
    Ok(json!({
        "native_close_requested": true,
        "localized_menu": true,
        "window_hidden": true,
        "tray_retained": true,
        "client_remained_open": true,
        "same_service_pid": service_pid,
        "window_restored": true,
        "isolated_profile_verified": true,
    }))
}
