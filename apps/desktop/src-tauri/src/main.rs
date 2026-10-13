#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_updates;
mod archive_entry;
mod assistant;
mod assistant_sessions;
mod astroneer_console;
mod command_registry;
mod commands;
mod commands_assistant_connection;
mod commands_knowledge;
mod desktop_app_log;
#[cfg(all(windows, feature = "desktop-reliability"))]
mod desktop_reliability;
mod desktop_window;
mod dst_mods;
#[cfg(all(test, feature = "custom-protocol"))]
mod embedded_frontend_tests;
mod knowledge_runtime;
mod lan_directory;
mod lan_host;
mod live_players;
mod media_cache;
mod project_zomboid_mods;
mod runtime_astroneer_health;
mod runtime_log_stream;
#[cfg(windows)]
mod runtime_service;
mod runtime_transport;
mod runtime_transport_humanitz;
mod startup_storage;
mod state;
mod steam_workshop;
mod steamcmd_preparation;
mod text_decode;
mod webview_drop;
mod webview_recovery;

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::Path;

use tauri::Manager;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};

const INSTANCE_LEASE_FILE_NAME: &str = "cn.langame.servermanager.lock";
const TRAY_MENU_SHOW: &str = "tray_show_main_window";
const TRAY_MENU_EXIT: &str = "tray_exit_app";

#[derive(Clone, Copy, serde::Deserialize)]
enum TrayLocale {
    #[serde(rename = "zh-CN")]
    Chinese,
    #[serde(rename = "en-US")]
    English,
}

impl TrayLocale {
    fn labels(self) -> (&'static str, &'static str) {
        match self {
            Self::Chinese => ("打开", "退出"),
            Self::English => ("Open", "Exit"),
        }
    }
}

struct TrayMenu {
    show: MenuItem<tauri::Wry>,
    exit: MenuItem<tauri::Wry>,
}

#[tauri::command]
fn set_tray_locale(locale: TrayLocale, menu: tauri::State<'_, TrayMenu>) -> Result<(), String> {
    let (show, exit) = locale.labels();
    menu.show
        .set_text(show)
        .map_err(|error| error.to_string())?;
    menu.exit.set_text(exit).map_err(|error| error.to_string())
}

#[tauri::command]
fn app_exit_status(app: tauri::AppHandle) -> serde_json::Value {
    #[cfg(windows)]
    return runtime_service::exit_status(&app);
    #[cfg(not(windows))]
    {
        let _ = app;
        serde_json::json!({ "requested": false })
    }
}

#[derive(Debug)]
struct ProcessInstanceLease {
    file: Option<File>,
}

impl ProcessInstanceLease {
    fn is_secondary(&self) -> bool {
        self.file.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(any(not(windows), test))]
enum WindowCloseAction {
    AllowClose,
    RequestAppExitShutdown,
}

fn resolve_desktop_storage() -> Result<app_storage::StoragePaths, app_storage::StorageError> {
    startup_storage::resolve()
}

fn main() {
    #[cfg(windows)]
    if let Some(code) = app_runtime::run_elevated_launcher_if_requested() {
        std::process::exit(code);
    }
    if let Some(code) = app_knowledge::pdf_worker::run_if_requested() {
        std::process::exit(code);
    }
    #[cfg(all(windows, feature = "desktop-reliability"))]
    if desktop_reliability::run_if_requested() {
        return;
    }
    if let Some(code) = app_runtime::native_player_console::run_native_player_console_helper() {
        std::process::exit(code);
    }
    #[cfg(windows)]
    if runtime_service::run_if_requested() {
        return;
    }
    let instance_lease = acquire_desktop_instance_lease()
        .unwrap_or_else(|error| panic!("failed to acquire the process instance lease: {error}"));
    let secondary_instance = instance_lease.is_secondary();
    #[cfg(windows)]
    let storage_source =
        state::StorageSource::UserDataRoot(app_storage::StoragePaths::default().app_data_root);
    // A native error dialog pumps window messages. Resolve storage before a WebView
    // can send IPC to the runtime client that setup has not installed yet.
    let storage_paths = if secondary_instance {
        None
    } else {
        match resolve_desktop_storage() {
            Ok(paths) => Some(paths),
            Err(error) => {
                eprintln!("Failed to initialize LanGame data directory: {error}");
                std::process::exit(1);
            }
        }
    };

    let run_result = tauri::Builder::default()
        .register_asynchronous_uri_scheme_protocol("lgsm-media", media_cache::handle_protocol)
        .plugin(tauri_plugin_single_instance::init(
            |app, _arguments, _cwd| {
                show_main_window(app);
            },
        ))
        .manage(app_updates::PendingAppUpdate::default())
        .setup(move |app| {
            // The plugin can briefly miss a launch before its Windows message window exists.
            // The file lease keeps that process from touching shared storage.
            let Some(storage_paths) = storage_paths else {
                std::process::exit(0);
            };
            #[cfg(not(windows))]
            if !app.manage(state::DesktopState::default()) {
                return Err(std::io::Error::other("desktop state was already initialized").into());
            }
            #[cfg(windows)]
            runtime_service::setup(app, storage_source).map_err(std::io::Error::other)?;
            app.manage(media_cache::MediaCacheState::new(
                storage_paths.app_data_root.join("cache/media"),
            ));
            app.handle()
                .plugin(tauri_plugin_updater::Builder::new().build())?;
            setup_tray(app)?;
            let main_window = app
                .get_webview_window("main")
                .ok_or_else(|| std::io::Error::other("main window is missing"))?;
            desktop_window::prepare(&main_window).map_err(std::io::Error::other)?;
            #[cfg(windows)]
            webview_drop::install(&main_window).map_err(std::io::Error::other)?;
            main_window.show()?;
            #[cfg(windows)]
            {
                let window = app
                    .config()
                    .app
                    .windows
                    .iter()
                    .find(|window| window.label == "main")
                    .cloned()
                    .ok_or_else(|| std::io::Error::other("main window configuration missing"))?;
                webview_recovery::install(
                    app.handle(),
                    webview_recovery::RecoveryConfig {
                        window,
                        // The service owns desktop-app logs. Rotation has one
                        // process owner per directory, including WebView failures.
                        log_path: storage_paths.logs_root.join("desktop-ui/active.jsonl"),
                    },
                )
                .map_err(std::io::Error::other)?;
            }
            #[cfg(not(windows))]
            if lan_host::is_lan_host_requested() {
                let config = lan_host::config_from_env().map_err(std::io::Error::other)?;
                lan_host::spawn_lan_host(app.handle().clone(), config)
                    .map_err(std::io::Error::other)?;
            }
            #[cfg(not(windows))]
            if let Err(error) = lan_directory::spawn_lan_directory_broadcaster(app.handle().clone())
            {
                eprintln!("LanGame LAN directory is unavailable: {error}");
            }
            #[cfg(not(windows))]
            {
                commands::spawn_runtime_heartbeat(app.handle().clone());
                knowledge_runtime::spawn_knowledge_scheduler(app.handle().clone());
                commands::commands_managed_save::spawn_managed_save_worker(app.handle().clone());
            }
            Ok(())
        })
        .on_page_load(|webview, payload| {
            if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished) {
                #[cfg(windows)]
                if webview.label() == "main"
                    && let Some(window) = webview.app_handle().get_webview_window("main")
                    && let Err(error) = webview_drop::install(&window)
                {
                    eprintln!("Restore dropped-file bridge after page load: {error}");
                }
                webview_recovery::page_loaded(webview.app_handle(), webview.label(), 1);
            }
        })
        .on_window_event(handle_window_event)
        .invoke_handler(command_registry::invoke_handler)
        .build(tauri::tauri_build_context!());

    match run_result {
        Ok(app) => app.run(|handle, event| {
            #[cfg(windows)]
            if matches!(event, tauri::RunEvent::Exit) {
                webview_drop::detach_on_ui_thread();
            }
            webview_recovery::handle_run_event(handle, &event);
        }),
        Err(error) => panic!("failed to run LanGame desktop shell: {error}"),
    }
    drop(instance_lease);
}

fn handle_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    if window.label() != "main" {
        return;
    }
    #[cfg(windows)]
    if matches!(event, tauri::WindowEvent::Destroyed) {
        webview_drop::detach_on_ui_thread();
    }
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        let app_handle = window.app_handle();
        #[cfg(windows)]
        {
            // The tray belongs to this process, so closing must keep the shell alive.
            api.prevent_close();
            match window.hide() {
                Ok(()) => webview_recovery::set_window_visible(app_handle, false),
                Err(error) => eprintln!("Failed to hide the main window to the tray: {error}"),
            }
        }
        #[cfg(not(windows))]
        match window_close_action(commands::app_exit_shutdown_completed(app_handle)) {
            WindowCloseAction::AllowClose => (),
            WindowCloseAction::RequestAppExitShutdown => {
                api.prevent_close();
                commands::request_app_exit_shutdown(app_handle.clone());
            }
        }
    }
}

fn acquire_process_instance_lease() -> io::Result<ProcessInstanceLease> {
    acquire_process_instance_lease_at(&std::env::temp_dir().join(INSTANCE_LEASE_FILE_NAME))
}

fn acquire_desktop_instance_lease() -> io::Result<ProcessInstanceLease> {
    #[cfg(windows)]
    return acquire_process_instance_lease_at(
        &std::env::temp_dir().join("cn.langame.servermanager.shell.lock"),
    );
    #[cfg(not(windows))]
    acquire_process_instance_lease()
}

fn acquire_process_instance_lease_at(path: &Path) -> io::Result<ProcessInstanceLease> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    match file.try_lock() {
        Ok(()) => Ok(ProcessInstanceLease { file: Some(file) }),
        Err(TryLockError::WouldBlock) => Ok(ProcessInstanceLease { file: None }),
        Err(TryLockError::Error(error)) => Err(error),
    }
}

fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    // The UI reapplies its saved preference on mount and every language change.
    let (show_label, exit_label) = TrayLocale::Chinese.labels();
    let show = MenuItem::with_id(app, TRAY_MENU_SHOW, show_label, true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let exit = MenuItem::with_id(app, TRAY_MENU_EXIT, exit_label, true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &separator, &exit])?;
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/icon.png"))?;

    TrayIconBuilder::with_id("main")
        .tooltip("LanGame Server Manager")
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app_handle, event| match event.id().as_ref() {
            TRAY_MENU_SHOW => show_main_window(app_handle),
            TRAY_MENU_EXIT => {
                #[cfg(windows)]
                runtime_service::request_stop_and_exit(app_handle.clone());
                #[cfg(not(windows))]
                commands::request_app_exit_shutdown(app_handle.clone());
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                ..
            }
            | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } => show_main_window(tray.app_handle()),
            _ => {}
        })
        .build(app)?;

    app.manage(TrayMenu { show, exit });
    Ok(())
}

fn show_main_window(app_handle: &tauri::AppHandle) {
    webview_recovery::set_window_visible(app_handle, true);
    webview_recovery::retry_from_user(app_handle);
    if let Some(window) = app_handle.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[cfg(any(not(windows), test))]
fn window_close_action(shutdown_completed: bool) -> WindowCloseAction {
    if shutdown_completed {
        WindowCloseAction::AllowClose
    } else {
        WindowCloseAction::RequestAppExitShutdown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_instance_lease_is_exclusive_and_reusable_after_drop() {
        let path = std::env::temp_dir().join(format!(
            "langame-single-instance-{}-{}.lock",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));

        let first = acquire_process_instance_lease_at(&path).expect("first lease");
        assert!(!first.is_secondary());

        let second = acquire_process_instance_lease_at(&path).expect("contended lease");
        assert!(second.is_secondary());

        drop(first);
        let third = acquire_process_instance_lease_at(&path).expect("lease after primary drop");
        assert!(!third.is_secondary());

        drop(third);
        std::fs::remove_file(path).expect("remove process lease fixture");
    }

    #[test]
    fn close_request_starts_app_exit_shutdown_before_shutdown_completed() {
        assert_eq!(
            window_close_action(false),
            WindowCloseAction::RequestAppExitShutdown
        );
    }

    #[test]
    fn close_request_allows_window_close_after_shutdown_completed() {
        assert_eq!(window_close_action(true), WindowCloseAction::AllowClose);
    }
}
