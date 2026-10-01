const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const root = path.resolve(__dirname, "..", "..", "..");
const cargoTomlPath = path.join(root, "apps", "desktop", "src-tauri", "Cargo.toml");
const mainPath = path.join(root, "apps", "desktop", "src-tauri", "src", "main.rs");

test("desktop registers the pinned single-instance plugin before application state", () => {
  const cargoToml = fs.readFileSync(cargoTomlPath, "utf8");
  const mainSource = fs.readFileSync(mainPath, "utf8");

  assert.match(cargoToml, /^tauri-plugin-single-instance = "=2\.4\.4"$/mu);

  const pluginRegistration = mainSource.indexOf(
    ".plugin(tauri_plugin_single_instance::init("
  );
  const firstManagedState = mainSource.indexOf(".manage(");
  const setup = mainSource.indexOf(".setup(");
  const updaterRegistration = mainSource.indexOf(
    ".plugin(tauri_plugin_updater::Builder::new().build())"
  );

  assert.notEqual(pluginRegistration, -1, "single-instance plugin must be registered");
  assert.ok(
    pluginRegistration < firstManagedState,
    "single-instance plugin must be registered before managed application state"
  );
  assert.ok(pluginRegistration < setup, "single-instance plugin must be registered before setup");
  assert.ok(
    pluginRegistration < updaterRegistration,
    "single-instance plugin must be registered before the updater plugin"
  );
  assert.doesNotMatch(
    mainSource,
    /^\s*\.manage\(state::DesktopState::default\(\)\)/mu,
    "DesktopState must not be constructed before the single-instance plugin rejects duplicates"
  );
});

test("a secondary launch restores and focuses the existing main window", () => {
  const mainSource = fs.readFileSync(mainPath, "utf8");

  assert.match(
    mainSource,
    /tauri_plugin_single_instance::init\(\s*\|app, _arguments, _cwd\| \{\s*show_main_window\(app\);\s*\},?\s*\)/u
  );
  assert.match(
    mainSource,
    /fn show_main_window\([\s\S]*?window\.show\(\)[\s\S]*?window\.unminimize\(\)[\s\S]*?window\.set_focus\(\)/u
  );
});

test("Windows stores server ownership in the service and connects the primary shell before its UI", () => {
  const mainSource = fs.readFileSync(mainPath, "utf8");
  const serverSource = fs.readFileSync(path.join(path.dirname(mainPath), "runtime_service", "server.rs"), "utf8");
  const setup = mainSource.indexOf(".setup(move |app|");
  const secondaryGuard = mainSource.indexOf("if secondary_instance {");
  const client = mainSource.indexOf("runtime_service::setup(app)", setup);
  const updater = mainSource.indexOf("tauri_plugin_updater::Builder::new().build()", setup);
  const tray = mainSource.indexOf("setup_tray(app)?", setup);

  assert.notEqual(setup, -1, "the primary setup hook must remain registered");
  assert.ok(setup < secondaryGuard, "the secondary-instance guard must run inside setup");
  assert.ok(
    secondaryGuard < client,
    "the secondary-instance guard must run before connecting to the backend"
  );
  assert.match(
    mainSource.slice(secondaryGuard, client),
    /std::process::exit\(0\)/u,
    "a secondary process that reaches setup must exit synchronously"
  );
  for (const [service, position] of [
    ["updater", updater],
    ["tray", tray]
  ]) {
    assert.notEqual(position, -1, `${service} initialization must remain registered`);
    assert.ok(client < position, `the backend connection must precede ${service} initialization`);
  }
  assert.match(mainSource, /#\[cfg\(not\(windows\)\)\]\s*if !app\.manage\(state::DesktopState::default\(\)\)/u);
  const state = serverSource.indexOf("app.manage(crate::state::DesktopState::default())");
  const firstPipe = serverSource.indexOf("security::create_pipe(&endpoint, true)", serverSource.indexOf("pub(super) fn run"));
  assert.ok(firstPipe >= 0 && firstPipe < state, "secure the current-user endpoint before opening storage");
  for (const service of ["start_with_pipe(app.handle()", "crate::lan_host::is_lan_host_requested()", "crate::commands::spawn_runtime_heartbeat("]) {
    assert.ok(serverSource.indexOf(service, state) > state, `backend state must precede ${service}`);
  }
});

test("the process lease is acquired before Builder and held through run", () => {
  const mainSource = fs.readFileSync(mainPath, "utf8");
  const lease = mainSource.indexOf("let instance_lease = acquire_desktop_instance_lease()");
  const builder = mainSource.indexOf("tauri::Builder::default()");
  const build = mainSource.indexOf(".build(tauri::generate_context!())");
  const run = mainSource.indexOf("Ok(app) => app.run(");
  const release = mainSource.indexOf("drop(instance_lease)");

  assert.match(
    mainSource,
    /const INSTANCE_LEASE_FILE_NAME: &str = "cn\.langame\.servermanager\.lock";/u
  );
  for (const [stage, position] of Object.entries({ lease, builder, build, run, release })) {
    assert.notEqual(position, -1, `${stage} must remain in the primary process lifecycle`);
  }
  assert.ok(lease < builder, "the OS lease must be acquired before Tauri Builder initialization");
  assert.ok(builder < build && build < run, "the built Tauri application must enter its event loop");
  assert.ok(run < release, "the primary lease must remain alive until the event loop returns");
  assert.match(mainSource, /cn\.langame\.servermanager\.shell\.lock/u);
});

test("tray exit surfaces shutdown failures in the main window", () => {
  const mainSource = fs.readFileSync(mainPath, "utf8");
  const lifecycleSource = fs.readFileSync(
    path.join(root, "apps", "desktop", "src-tauri", "src", "commands_runtime_lifecycle.rs"),
    "utf8"
  );
  const appSource = fs.readFileSync(path.join(root, "apps", "desktop", "src", "App.tsx"), "utf8");

  assert.match(mainSource, /Self::Chinese => \("打开", "退出"\)/u);
  assert.match(mainSource, /Self::English => \("Open", "Exit"\)/u);
  assert.match(mainSource, /runtime_service::request_stop_and_exit\(app_handle.clone\(\)\)/u);
  assert.match(lifecycleSource, /const APP_SHUTDOWN_FAILED_EVENT: &str = "app-shutdown-failed";/u);
  assert.match(lifecycleSource, /report_app_shutdown_failure\(app_handle, &state, completion, &error\)/u);
  assert.match(appSource, /listen<\{ message: string \}>\("app-shutdown-failed"/u);
  assert.match(appSource, /message\("activity\.appShutdownFailed"/u);
});

test("Windows close keeps the interface and tray alive while explicit exit stops the service", () => {
  const mainSource = fs.readFileSync(mainPath, "utf8");
  const service = fs.readFileSync(path.join(path.dirname(mainPath), "runtime_service.rs"), "utf8");
  const header = fs.readFileSync(path.join(root, "apps", "desktop", "src", "components", "AppHeader.tsx"), "utf8");
  const close = mainSource.slice(mainSource.indexOf("fn handle_window_event("), mainSource.indexOf("fn acquire_process_instance_lease("));
  assert.match(close, /api\.prevent_close\(\)/u);
  assert.match(close, /window\.hide\(\)/u);
  assert.doesNotMatch(close, /close_interface|app\.exit\(|stop_service/u);
  assert.match(service, /exit_deadline::arm\(target, deadline\);\s*request\.commit\(\)/u);
  assert.match(service, /handoff_exit\(deadline_tick_ms\)\s*\{\s*Ok\(\(\)\) => \{\s*app\.state::<client::Client>\(\)\.close_interface\(\);\s*exit_deadline::finish_interface\(\);\s*app\.exit\(0\)/u);
  assert.match(service, /Err\(error\) => retain_local_exit_owner\(&app, deadline_tick_ms, &error\)/u);
  assert.match(service, /stop_for_tray_exit\(deadline_tick_ms\)\s*\.await\s*\.is_ok\(\)/u);
  assert.match(header, /function hideToTray\(\)\s*\{\s*void runWindowCommand\(\(window\) => window\.close\(\)\)/u);
  assert.match(header, /title=\{t\("common\.minimizeToTray"\)\}/u);
});
