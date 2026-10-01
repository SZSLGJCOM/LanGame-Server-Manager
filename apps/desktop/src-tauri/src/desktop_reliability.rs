//! Opt-in real-host acceptance fixture. This entry runs before all production
//! single-instance, storage, updater, tray and background-service initialization.

mod config;
mod log_document;
mod log_worker;
mod native;
mod protocol;
mod server;

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::Manager;

use crate::webview_recovery::{self, RecoveryConfig, RecoverySnapshot};
use config::Config;
use log_worker::{LogWorker, Output};
use protocol::{Action, Observations, Progress, Stage};
use server::{INSTANCE_ID, Server};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

struct Fixture {
    config: Config,
    server: Mutex<Server>,
    progress: Mutex<Progress>,
    output: Arc<Mutex<Output>>,
    log_worker: Mutex<Option<LogWorker>>,
    browser_process_ids: Mutex<Vec<u32>>,
    window_visibility: Mutex<WindowVisibilityEvidence>,
}

#[derive(Clone, Copy, Default, Serialize)]
struct WindowVisibilityEvidence {
    close_hid_window: bool,
    browser_recovery_preserved_hidden: bool,
    reopened_after_browser_recovery: bool,
}

#[derive(Serialize)]
struct Status {
    nonce: String,
    host_pid: u32,
    server_pid: u32,
    instance_id: &'static str,
    run_id: i64,
    log_path: String,
    tail_lines: Vec<String>,
    stage: Stage,
    busy: bool,
    error: Option<String>,
    server_alive: bool,
    command_count: u32,
    recovery: Option<RecoverySnapshot>,
    faults_requested: Vec<String>,
    native_failures: Vec<String>,
    observations: Option<Observations>,
    browser_process_ids: Vec<u32>,
}

impl Fixture {
    fn status(&self, app: &tauri::AppHandle) -> Result<Status, String> {
        let recovery = webview_recovery::snapshot(app);
        let mut progress = lock(&self.progress);
        if let Some(snapshot) = &recovery
            && let Err(error) = progress.update_recovery(snapshot)
        {
            progress.error = Some(error);
        }
        // Server locking can briefly wait for bounded dispatch, so every caller
        // of this method runs in a blocking worker, never the UI/async executor.
        let server = lock(&self.server);
        let output = lock(&self.output);
        Ok(Status {
            nonce: self.config.nonce.clone(),
            host_pid: std::process::id(),
            server_pid: server.pid,
            instance_id: INSTANCE_ID,
            run_id: 1,
            log_path: server.log_path.to_string_lossy().into_owned(),
            tail_lines: output.lines.iter().cloned().collect(),
            stage: progress.stage,
            busy: progress.busy,
            error: progress.error.clone().or_else(|| output.error.clone()),
            server_alive: server.alive()?,
            command_count: server.command_count()?,
            recovery,
            faults_requested: progress.faults_requested.clone(),
            native_failures: progress.native_failures.clone(),
            observations: progress.observations.clone(),
            browser_process_ids: lock(&self.browser_process_ids).clone(),
        })
    }

    fn command(&self) -> Result<(), String> {
        let expected = lock(&self.progress).command_count + 1;
        lock(&self.server).command(expected)?;
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let output = lock(&self.output);
            if let Some(error) = &output.error {
                return Err(error.clone());
            }
            if output.markers == (1..=expected).collect::<Vec<_>>() {
                drop(output);
                lock(&self.progress).command_completed();
                return Ok(());
            }
            drop(output);
            std::thread::sleep(Duration::from_millis(20));
        }
        Err("The real log stream did not observe the accepted command marker".into())
    }

    fn stop(&self) -> Result<(), String> {
        let stopped = lock(&self.server).stop();
        let joined = lock(&self.log_worker)
            .take()
            .map(|mut worker| worker.stop())
            .unwrap_or(Ok(()));
        stopped.and(joined)
    }

    fn finish(&self, app: &tauri::AppHandle) -> Result<(), String> {
        let before = self.status(app)?;
        let visibility = *lock(&self.window_visibility);
        if !visibility.close_hid_window
            || !visibility.browser_recovery_preserved_hidden
            || !visibility.reopened_after_browser_recovery
        {
            return Err("Hidden-window browser recovery evidence is incomplete".into());
        }
        if before.command_count != 3
            || !before.server_alive
            || before.error.is_some()
            || before.native_failures != ["renderer_exited", "browser_exited"]
            || before
                .recovery
                .as_ref()
                .is_none_or(|snapshot| snapshot.recoveries != 2 || snapshot.failures != 2)
        {
            return Err("Final host/server/native recovery evidence is incomplete".into());
        }
        self.stop()?;
        let mut status = self.status(app)?;
        if status.server_alive {
            return Err("Simulated server remained alive after stop".into());
        }
        status.stage = Stage::Finished;
        status.busy = false;
        config::write_new_json(
            &self.config.root.join("report.json"),
            &serde_json::json!({
                "schema_version": 1, "passed": true, "status": status,
                "browser_process_ids": lock(&self.browser_process_ids).clone(),
                "server_exited": true, "output_worker_joined": true,
                "profile_verified": true,
                "window_visibility": visibility,
                "emitted_log_batches": lock(&self.output).batches,
            }),
        )?;
        lock(&self.progress).stage = Stage::Finished;
        Ok(())
    }
}

#[tauri::command]
async fn read_instance_log_document_from_storage(
    state: tauri::State<'_, Arc<Fixture>>,
    instance_id: String,
    max_lines: Option<usize>,
    run_id: Option<i64>,
    source: Option<String>,
) -> Result<app_core::LogTailSnapshot, String> {
    let root = state.config.root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        log_document::read(&root, &instance_id, max_lines, run_id, source.as_deref())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn desktop_reliability_status(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<Fixture>>,
) -> Result<Status, String> {
    let fixture = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || fixture.status(&app))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn desktop_reliability_step(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<Fixture>>,
    nonce: String,
    action: Action,
    observations: Option<Observations>,
) -> Result<Status, String> {
    if nonce != state.config.nonce {
        return Err("Fixture nonce mismatch".into());
    }
    let snapshot = webview_recovery::snapshot(&app).ok_or("Fixture recovery is unavailable")?;
    lock(&state.progress).begin(action, observations, &snapshot)?;
    let fixture = Arc::clone(state.inner());
    // The operation is owned independently of the renderer's IPC observer. A
    // crash or reload cannot cancel a dispatched command or release its lease.
    tauri::async_runtime::spawn(async move {
        let result = execute(&app, &fixture, action).await;
        {
            let mut progress = lock(&fixture.progress);
            progress.busy = false;
            if let Err(error) = &result {
                progress.error = Some(error.clone());
            }
        }
        result?;
        let reader = Arc::clone(&fixture);
        let status_app = app.clone();
        let status = tauri::async_runtime::spawn_blocking(move || reader.status(&status_app))
            .await
            .map_err(|error| error.to_string())??;
        if matches!(action, Action::Finish) {
            webview_recovery::shutdown(&app);
            app.exit(0);
        }
        Ok(status)
    })
    .await
    .map_err(|error| error.to_string())?
}

async fn execute(
    app: &tauri::AppHandle,
    fixture: &Arc<Fixture>,
    action: Action,
) -> Result<(), String> {
    if matches!(action, Action::CrashBrowser)
        || (matches!(action, Action::SendCommand)
            && lock(&fixture.progress).stage == Stage::AfterBrowserCommand)
    {
        let fixture = Arc::clone(fixture);
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            verify_window_visibility(&app, &fixture, action)
        })
        .await
        .map_err(|error| error.to_string())??;
    }
    if !matches!(action, Action::Finish) {
        let pid =
            native::observe_or_crash(app, action, fixture.config.root.join("profile")).await?;
        let mut pids = lock(&fixture.browser_process_ids);
        if !pids.contains(&pid) {
            if pids.len() >= 3 {
                return Err("Fixture observed unexpected additional browser generations".into());
            }
            pids.push(pid);
        }
    }
    let fixture = Arc::clone(fixture);
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || match action {
        Action::SendCommand => fixture.command(),
        Action::Finish => fixture.finish(&app),
        Action::CrashRenderer | Action::CrashBrowser => Ok(()),
    })
    .await
    .map_err(|error| error.to_string())?
}

fn verify_window_visibility(
    app: &tauri::AppHandle,
    fixture: &Fixture,
    action: Action,
) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or("Fixture main window is missing during visibility verification")?;
    let visible = window.is_visible().map_err(|error| error.to_string())?;
    if matches!(action, Action::CrashBrowser) {
        if !visible {
            return Err("Fixture window was already hidden before the close request".into());
        }
        // Exercise the same CloseRequested handler used by the title-bar button.
        window.close().map_err(|error| error.to_string())?;
        wait_for_window_visibility(&window, false)?;
        lock(&fixture.window_visibility).close_hid_window = true;
    } else {
        if visible {
            return Err("Browser recovery unexpectedly showed the hidden fixture window".into());
        }
        lock(&fixture.window_visibility).browser_recovery_preserved_hidden = true;
        crate::show_main_window(app);
        wait_for_window_visibility(&window, true)?;
        lock(&fixture.window_visibility).reopened_after_browser_recovery = true;
    }
    Ok(())
}

fn wait_for_window_visibility(window: &tauri::WebviewWindow, visible: bool) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if window.is_visible().map_err(|error| error.to_string())? == visible {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Err(format!(
        "Fixture native window did not become visible={visible} within five seconds"
    ))
}

pub(crate) fn run_if_requested() -> bool {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    let Some(role) = arguments.first().and_then(|value| value.to_str()) else {
        return false;
    };
    let result = match role {
        "--desktop-reliability" if arguments.len() == 2 => run(Path::new(&arguments[1])),
        server::SERVER_ROLE if arguments.len() == 3 => arguments[2]
            .to_str()
            .ok_or_else(|| "Invalid fixture nonce encoding".into())
            .and_then(|nonce| server::run(Path::new(&arguments[1]), nonce)),
        "--desktop-reliability" | server::SERVER_ROLE => {
            Err("Invalid desktop reliability fixture arguments".into())
        }
        _ => return false,
    };
    if let Err(error) = result {
        eprintln!("Desktop reliability fixture failed: {error}");
        std::process::exit(1);
    }
    true
}

fn run(config_path: &Path) -> Result<(), String> {
    let config = Config::load(config_path)?;
    config.create_root()?;
    // This entry is the first operation in main, before any runtime/reader
    // threads start. These changes affect this fixture process and its children
    // only; inherited WebView2 overrides must never redirect it to a real profile.
    unsafe {
        std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", config.root.join("profile"));
        for key in [
            "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
            "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER",
            "WEBVIEW2_PIPE_FOR_SCRIPT_DEBUGGER",
            "WEBVIEW2_WAIT_FOR_SCRIPT_DEBUGGER",
        ] {
            std::env::remove_var(key);
        }
    }
    let server = Server::spawn(&config)?;
    let fixture = Arc::new(Fixture {
        config: config.clone(),
        server: Mutex::new(server),
        progress: Mutex::new(Progress::default()),
        output: Arc::new(Mutex::new(Output::default())),
        log_worker: Mutex::new(None),
        browser_process_ids: Mutex::new(Vec::with_capacity(3)),
        window_visibility: Mutex::new(WindowVisibilityEvidence::default()),
    });
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    let url = tauri::Url::parse(&config.url).map_err(|error| error.to_string())?;
    context.config_mut().build.dev_url = Some(url.clone());
    context.config_mut().build.frontend_dist = Some(tauri::utils::config::FrontendDist::Url(url));
    let window = tauri::utils::config::WindowConfig {
        label: "main".into(),
        title: "LanGame desktop reliability fixture".into(),
        url: tauri::WebviewUrl::App("/tests/helpers/desktop-reliability.html".into()),
        data_directory: Some(config.root.join("profile")),
        width: 1560.0,
        height: 900.0,
        focus: false,
        ..Default::default()
    };
    let setup_fixture = Arc::clone(&fixture);
    let application = tauri::Builder::default()
        .manage(Arc::clone(&fixture))
        .on_window_event(crate::handle_window_event)
        .invoke_handler(tauri::generate_handler![
            desktop_reliability_status,
            desktop_reliability_step,
            read_instance_log_document_from_storage
        ])
        .on_page_load(|webview, payload| {
            if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished) {
                webview_recovery::page_loaded(webview.app_handle(), webview.label(), 1);
            }
        })
        .setup(move |app| {
            // from_config resolves only relative data_directory values. The
            // explicit builder setter accepts this absolute, isolated profile.
            tauri::WebviewWindowBuilder::from_config(app, &window)?
                .data_directory(setup_fixture.config.root.join("profile"))
                .build()?;
            webview_recovery::install(
                app.handle(),
                RecoveryConfig {
                    window,
                    log_path: setup_fixture
                        .config
                        .root
                        .join("logs/webview-recovery.jsonl"),
                },
            )
            .map_err(std::io::Error::other)?;
            let path = lock(&setup_fixture.server).log_path.clone();
            *lock(&setup_fixture.log_worker) = Some(
                LogWorker::start(
                    app.handle().clone(),
                    path,
                    Arc::clone(&setup_fixture.output),
                )
                .map_err(std::io::Error::other)?,
            );
            Ok(())
        })
        .build(context)
        .map_err(|error| error.to_string())?;
    // run() exits the process directly; run_return lets the fixture join its
    // owners and preserve diagnostic evidence on failed or interrupted runs.
    let exit_code =
        application.run_return(|app, event| webview_recovery::handle_run_event(app, &event));
    let stopped = fixture.stop();
    let complete = lock(&fixture.progress).stage == Stage::Finished;
    if exit_code != 0 || !complete || stopped.is_err() {
        let error = stopped
            .err()
            .or_else(|| lock(&fixture.progress).error.clone())
            .or_else(|| lock(&fixture.output).error.clone())
            .unwrap_or_else(|| "Fixture exited before finishing".into());
        let report = config::write_new_json(
            &config.root.join("failure.json"),
            &serde_json::json!({"error": error, "browser_process_ids": lock(&fixture.browser_process_ids).clone(), "server_pid": lock(&fixture.server).pid}),
        );
        if let Err(report_error) = report {
            eprintln!("Save fixture failure: {report_error}");
        }
        return Err(error);
    }
    Ok(())
}
