//! Main-page IPC -> desktop command -> secured runtime pipe -> real preparation.
use super::*;
use std::collections::HashMap;

pub(super) fn initialize_storage(root: &Path) -> Result<(), String> {
    // This runs before the Tauri application and workers exist. Resolve the real
    // production storage contract entirely within the new fixture namespace.
    unsafe {
        std::env::set_var("LOCALAPPDATA", root.join("localappdata"));
    }
    let user_root = app_storage::StoragePaths::default().app_data_root;
    if !user_root.starts_with(root) {
        return Err("Nexus fixture user data escaped its isolated root".into());
    }
    std::fs::create_dir(root.join("data")).map_err(|error| error.to_string())?;
    let paths = app_storage::StoragePaths::resolve_in_directory(&root.join("data"))
        .map_err(|error| error.to_string())?;
    for path in [
        &paths.app_data_root,
        &paths.settings_path,
        &paths.database_path,
        &paths.logs_root,
        &paths.steamcmd_root,
        &paths.games_root,
        &paths.instances_root,
        &paths.archives_root,
    ] {
        if !path.starts_with(root) {
            return Err("Nexus fixture storage escaped its isolated root".into());
        }
    }
    app_storage::bootstrap_storage().map_err(|error| error.to_string())?;
    Ok(())
}

pub(super) fn assets(root: &Path) -> Result<HashMap<String, (&'static str, Vec<u8>)>, String> {
    std::fs::write(root.join("main.html"), MAIN).map_err(|error| error.to_string())?;
    let sdk = Path::new(env!("CARGO_MANIFEST_DIR")).join("../node_modules/@tauri-apps/api");
    let mut files = HashMap::from([(
        "/main.html".into(),
        ("text/html; charset=utf-8", MAIN.as_bytes().to_vec()),
    )]);
    // Serve the installed SDK itself, including its Channel ordering/serialization.
    for path in ["core.js", "external/tslib/tslib.es6.js"] {
        files.insert(
            format!("/{path}"),
            (
                "text/javascript; charset=utf-8",
                std::fs::read(sdk.join(path)).map_err(|error| error.to_string())?,
            ),
        );
    }
    Ok(files)
}

pub(super) async fn read_js(view: &Webview, script: &str) -> Result<serde_json::Value, String> {
    let (reply, mut response) = tokio::sync::mpsc::channel(1);
    view.eval_with_callback(script, move |value| {
        let _ = reply.try_send(value);
    })
    .map_err(|error| error.to_string())?;
    let value = tokio::time::timeout(Duration::from_secs(3), response.recv())
        .await
        .map_err(|_| "Main IPC observation timed out")?
        .ok_or("Main IPC observation was dropped")?;
    serde_json::from_str(&value).map_err(|error| error.to_string())
}

async fn state(view: &Webview) -> Result<serde_json::Value, String> {
    read_js(view, "window.__nexusMain?.state ?? null").await
}

async fn start(view: &Webview, instance_id: &str) -> Result<(), String> {
    view.eval(format!("window.__nexusMain.start({})", json!(instance_id)))
        .map_err(|error| error.to_string())
}

async fn completed(view: &Webview) -> Result<serde_json::Value, String> {
    let started = Instant::now();
    loop {
        let current = state(view).await?;
        if current["finished"] == true {
            return Ok(current);
        }
        if started.elapsed() > CASE_TIMEOUT {
            return Err(format!("Main IPC operation did not settle: {current}"));
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn preparation_failure(result: &serde_json::Value) -> Result<(), String> {
    if result["error"]["code"] != "nexus_preparation_failed"
        || !result["error"]["message"]
            .as_str()
            .is_some_and(|message| !message.trim().is_empty())
        || result["progress"] != json!(["opening"])
    {
        return Err(format!(
            "Main IPC lost its preflight cause or reported a later phase: {result}"
        ));
    }
    Ok(())
}

async fn prepare_library(app: &tauri::AppHandle, root: &Path) -> Result<String, String> {
    let storage = app
        .state::<crate::state::DesktopState>()
        .bootstrap_storage()?;
    if !storage.paths.app_data_root.starts_with(root) || !storage.paths.games_root.starts_with(root)
    {
        return Err("Main IPC preparation opened storage outside its fixture".into());
    }
    let descriptor = app_modules::discover_modules(&storage.paths.modules_root)
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|module| module.summary.id == "sevendaystodie")
        .ok_or("Nexus IPC fixture requires the 7 Days to Die module")?;
    let source = storage.paths.games_root.join("sevendaystodie");
    std::fs::create_dir_all(&source).map_err(|error| error.to_string())?;
    // Probe and copy these bytes; no server process is ever launched.
    std::fs::write(
        source.join("7DaysToDieServer.exe"),
        b"Synthetic Nexus preparation fixture",
    )
    .map_err(|error| error.to_string())?;
    app_storage::record_library_program_baseline(&source, &descriptor, true, None)
        .map_err(|error| error.to_string())?;
    app_storage::retain_library_program_source(&storage.paths, &source, &descriptor)
        .map_err(|error| error.to_string())?;
    app_storage::sync_modules(&storage.paths, &[descriptor])
        .await
        .map_err(|error| error.to_string())?;
    app_storage::sync_game_installs(
        &storage.paths,
        &[app_storage::GameInstallSyncRecord {
            module_id: "sevendaystodie".into(),
            install_root: source.to_string_lossy().into_owned(),
            install_state: app_core::InstallState::Installed,
            current_version: Some("synthetic-nexus-ipc".into()),
            mark_verified: true,
        }],
    )
    .await
    .map_err(|error| error.to_string())?;
    let created = crate::runtime_service::wait_for_client(app)
        .await?
        .request(
            "create_instance_record",
            json!({"input":{"name":"Nexus main IPC fixture","module_id":"sevendaystodie"}}),
        )
        .await?;
    created["summary"]["id"]
        .as_str()
        .map(str::to_owned)
        .ok_or("Nexus fixture instance was not created".into())
}

async fn child_for_main(
    app: &tauri::AppHandle,
    main: &Webview,
    fixture: &Fixture,
) -> Result<Webview, String> {
    let started = Instant::now();
    loop {
        let current = state(main).await?;
        if current["finished"] == true {
            return Err(format!("Main IPC stopped before loading: {current}"));
        }
        if let Some(id) = current["sessionId"].as_str()
            && let Some(child) = app.get_webview(&format!("{WINDOW_PREFIX}{id}"))
            && fixture.attached.load(Ordering::Acquire)
        {
            ipc_result(&child).await?;
            background_geometry(&child).await?;
            tokio::time::sleep(Duration::from_millis(450)).await;
            background_geometry(&child).await?;
            if state(main).await?["progress"] != json!(["opening"]) {
                return Err(
                    "Main IPC exposed its page or reported progress before content existed".into(),
                );
            }
            return Ok(child);
        }
        if started.elapsed() > CASE_TIMEOUT {
            return Err(format!(
                "Main IPC did not reach its prepared page: {current}"
            ));
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

pub(super) async fn verify(
    app: &tauri::AppHandle,
    root: &Path,
) -> Result<serde_json::Value, String> {
    let main = app
        .get_webview("main")
        .ok_or("Main IPC webview is missing")?;
    let loaded = Instant::now();
    while read_js(&main, "Boolean(window.__nexusMain)").await? != true {
        if loaded.elapsed() > CASE_TIMEOUT {
            return Err("Real Tauri SDK did not load".into());
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    start(&main, "missing-runtime").await?;
    let no_runtime = completed(&main).await?;
    preparation_failure(&no_runtime)?;
    ensure_closed(app).await?;
    let runtime_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::runtime_service::install_nexus_smoke_runtime(&runtime_app)
    })
    .await
    .map_err(|error| error.to_string())??;
    start(&main, "missing-instance").await?;
    let no_instance = completed(&main).await?;
    preparation_failure(&no_instance)?;
    ensure_closed(app).await?;
    let instance_id = prepare_library(app, root).await?;

    let blocked_fixture = Arc::new(Fixture::new(root, "main-ipc-blocked", Flow::Ambiguous)?);
    *app.state::<ActiveFixture>()
        .0
        .lock()
        .map_err(|_| "Active fixture poisoned")? = Some(blocked_fixture.clone());
    start(&main, &instance_id).await?;
    let child = child_for_main(app, &main, &blocked_fixture).await?;
    child
        .eval("window.__nexusSmokeRender = true")
        .map_err(|error| error.to_string())?;
    let blocked_geometry = background_geometry(&child).await?;
    let (blocked, ()) = tokio::try_join!(completed(&main), cases::monitor_background(app, &child))?;
    if blocked["error"]["code"] != "nexus_file_selection_required"
        || blocked["progress"] != json!(["opening"])
    {
        return Err(format!(
            "Main IPC did not report a hidden ambiguous-file failure: {blocked}"
        ));
    }
    *app.state::<ActiveFixture>()
        .0
        .lock()
        .map_err(|_| "Active fixture poisoned")? = None;
    ensure_closed(app).await?;

    let empty_fixture = Arc::new(Fixture::new(
        root,
        "main-ipc-no-controls",
        Flow::NoControls,
    )?);
    *app.state::<ActiveFixture>()
        .0
        .lock()
        .map_err(|_| "Active fixture poisoned")? = Some(empty_fixture.clone());
    start(&main, &instance_id).await?;
    let child = child_for_main(app, &main, &empty_fixture).await?;
    child
        .eval("window.__nexusSmokeRender = true")
        .map_err(|error| error.to_string())?;
    let no_controls_geometry = background_geometry(&child).await?;
    let (no_controls, ()) =
        tokio::try_join!(completed(&main), cases::monitor_background(app, &child))?;
    if no_controls["error"]["code"] != "nexus_website_controls_unavailable"
        || no_controls["progress"] != json!(["opening", "selecting"])
        || !empty_fixture
            .downloads
            .lock()
            .map_err(|_| "Download observations poisoned")?
            .is_empty()
        || empty_fixture
            .website
            .lock()
            .map_err(|_| "Website observations poisoned")?
            .is_some()
    {
        return Err(format!(
            "Main IPC did not report hidden missing controls: {no_controls}"
        ));
    }
    *app.state::<ActiveFixture>()
        .0
        .lock()
        .map_err(|_| "Active fixture poisoned")? = None;
    ensure_closed(app).await?;

    let fixture = Arc::new(Fixture::new(root, "main-ipc-cancel", Flow::Automatic)?);
    *app.state::<ActiveFixture>()
        .0
        .lock()
        .map_err(|_| "Active fixture poisoned")? = Some(fixture.clone());
    start(&main, &instance_id).await?;
    let child = child_for_main(app, &main, &fixture).await?;
    child
        .eval("window.__nexusSmokeRender = true")
        .map_err(|error| error.to_string())?;
    let cancel_at_waiting = async {
        let started = Instant::now();
        loop {
            let current = state(&main).await?;
            if current["finished"] == true {
                return Err(format!(
                    "Main IPC stopped before countdown cancellation: {current}"
                ));
            }
            if current["progress"] == json!(["opening", "selecting", "waiting"]) {
                break;
            }
            if started.elapsed() > CASE_TIMEOUT {
                return Err(format!(
                    "Main IPC did not report real download stages: {current}"
                ));
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        main.eval("window.__nexusMain.cancel()")
            .map_err(|error| error.to_string())?;
        completed(&main).await
    };
    let cancelled_geometry = background_geometry(&child).await?;
    let (cancelled, ()) =
        tokio::try_join!(cancel_at_waiting, cases::monitor_background(app, &child))?;
    if cancelled["error"]["code"] != "nexus_browser_cancelled"
        || cancelled["progress"] != json!(["opening", "selecting", "waiting"])
        || !fixture
            .downloads
            .lock()
            .map_err(|_| "Download observations poisoned")?
            .is_empty()
    {
        return Err(format!(
            "Main IPC countdown cancellation did not settle cleanly: {cancelled}"
        ));
    }
    *app.state::<ActiveFixture>()
        .0
        .lock()
        .map_err(|_| "Active fixture poisoned")? = None;
    ensure_closed(app).await?;
    Ok(
        json!({ "real_sdk_channel": true, "real_main_invoke": true, "runtime_transport": "secured_named_pipe",
        "no_runtime": no_runtime, "missing_instance": no_instance, "blocked": blocked, "no_controls": no_controls,
        "runtime_prepare_succeeded": true, "always_hidden": true, "cancelled": cancelled, "isolated_storage": true,
        "background_geometry": { "blocked": blocked_geometry, "no_controls": no_controls_geometry, "cancelled": cancelled_geometry } }),
    )
}

const MAIN: &str = r#"<!doctype html><meta charset="utf-8"><title>Nexus main IPC fixture</title>
<script type="module">
import { invoke, Channel } from './core.js';
window.__nexusMain = {
  invoke,
  Channel,
  state: null,
  start(instanceId) {
    const state = this.state = { sessionId: null, progress: [], error: null, finished: false };
    (async () => {
      try {
        state.sessionId = await invoke('open_nexus_website_host');
        const progress = new Channel(value => { state.progress.push(value); });
        await invoke('install_nexus_website_mod', { sessionId: state.sessionId, instanceId,
          reference: 'https://www.nexusmods.com/7daystodie/mods/2264', onProgress: progress });
      } catch (error) {
        try { state.error = JSON.parse(String(error)); } catch { state.error = { raw: String(error) }; }
      } finally {
        if (state.sessionId) await invoke('close_nexus_website_host', { sessionId: state.sessionId });
        state.finished = true;
      }
    })();
  },
  cancel() { return invoke('close_nexus_website_host', { sessionId: this.state.sessionId }); }
};
</script>"#;
