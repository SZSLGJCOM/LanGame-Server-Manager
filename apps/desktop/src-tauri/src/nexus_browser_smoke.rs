//! Opt-in native acceptance with an isolated profile and locally served official URLs.
use super::host::{close_nexus_website_host, open_nexus_website_host};
use super::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};
use tauri::{Webview, WebviewWindowBuilder};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_ALLOW, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
    ICoreWebView2_3, ICoreWebView2_4, ICoreWebView2_13, ICoreWebView2Environment,
    ICoreWebView2Environment7, ICoreWebView2WebResourceResponse,
};
use webview2_com::{DownloadStartingEventHandler, WebResourceRequestedEventHandler, take_pwstr};
use windows_core::{BOOL, HRESULT, IUnknown, Interface, PCWSTR, PWSTR};

const PROJECT_URL: &str = "https://www.nexusmods.com/7daystodie/mods/2264";
const DOWNLOAD_URL: &str = "https://cf-files.nexusmods.com/fixture.zip?token=fixture-client-secret";
const COMPONENT_ROUTE: &str = "https://www.nexusmods.com/api/files/1059000088/download";
const FIXTURE_TITLE: &str = "Server at Home at Night";
const FIXTURE_ICON: &str =
    "https://staticdelivery.nexusmods.com/mods/1059/images/9559/9559-1770000000-123456789.png";
const CASE_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Copy)]
enum Flow {
    Automatic,
    Explicit,
    Component,
    Ambiguous,
    Login,
    Verification,
    Dialog,
    NoControls,
}

impl Flow {
    fn name(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::Explicit => "explicit",
            Self::Component => "component",
            Self::Ambiguous => "ambiguous",
            Self::Login => "login",
            Self::Verification => "verification",
            Self::Dialog => "dialog",
            Self::NoControls => "no_controls",
        }
    }
}

#[derive(Clone, serde::Serialize)]
struct WebsiteObservation {
    manual_clicks: u32,
    slow_clicks: u32,
    file_id: String,
    countdown_finished: bool,
    waited_ms: u64,
}

#[path = "nexus_browser_smoke_ipc.rs"]
mod main_ipc;

#[path = "nexus_browser_smoke_cases.rs"]
mod cases;

#[path = "nexus_browser_smoke_geometry.rs"]
mod geometry;
use geometry::background_geometry;

#[path = "nexus_browser_smoke_logs.rs"]
mod logs;

#[path = "nexus_login_smoke.rs"]
mod login;

pub(super) fn current_login_fixture(app: &tauri::AppHandle) -> Option<Arc<login::LoginFixture>> {
    app.try_state::<login::ActiveFixture>()?
        .0
        .lock()
        .ok()?
        .clone()
}

#[path = "nexus_browser_live_probe.rs"]
mod live_probe;

pub(super) fn record_live_automation(app: &tauri::AppHandle, automation: &Automation) {
    live_probe::record_automation(app, automation);
}

#[derive(Default)]
struct ActiveFixture(Mutex<Option<Arc<Fixture>>>);

pub(super) fn current_fixture(app: &tauri::AppHandle) -> Option<Arc<Fixture>> {
    app.try_state::<ActiveFixture>()?.0.lock().ok()?.clone()
}

#[derive(Clone, serde::Serialize)]
struct DownloadObservation {
    official_fixture_url: bool,
    cancelled_by_product: bool,
    website: Option<WebsiteObservation>,
}

pub(super) struct Fixture {
    assets: PathBuf,
    profile: PathBuf,
    destination: PathBuf,
    attached: Arc<AtomicBool>,
    downloads: Arc<Mutex<Vec<DownloadObservation>>>,
    website: Arc<Mutex<Option<WebsiteObservation>>>,
    project_loads: Arc<AtomicUsize>,
    component_redirects: Arc<AtomicUsize>,
}

impl Fixture {
    fn new(root: &Path, case: &str, flow: Flow) -> Result<Self, String> {
        let assets = root.join(case).join("assets");
        let project = assets.join("7daystodie/mods");
        std::fs::create_dir_all(&project).map_err(|error| error.to_string())?;
        // A complete empty ZIP is enough: the native acceptance ends before runtime import.
        std::fs::write(
            assets.join("fixture.zip"),
            b"PK\x05\x06\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
        )
        .map_err(|error| error.to_string())?;
        let mut html = PAGE.replace("$FLOW$", flow.name());
        if matches!(flow, Flow::Automatic) {
            // Real Nexus file listings omit this optional HTML tag. Keep the
            // explicit-file case as coverage for a supplied canonical, too.
            html = html.replace(
                "<link rel=\"canonical\" href=\"https://www.nexusmods.com/7daystodie/mods/2264\">",
                "",
            );
        }
        std::fs::write(project.join("2264"), html).map_err(|error| error.to_string())?;
        let destination = root.join(case).join("downloads/fixture.zip");
        std::fs::create_dir(destination.parent().unwrap()).map_err(|error| error.to_string())?;
        Ok(Self {
            assets,
            profile: root.join("profile"),
            destination,
            attached: Arc::new(AtomicBool::new(false)),
            downloads: Arc::new(Mutex::new(Vec::new())),
            website: Arc::new(Mutex::new(None)),
            project_loads: Arc::new(AtomicUsize::new(0)),
            component_redirects: Arc::new(AtomicUsize::new(0)),
        })
    }

    pub(super) async fn attach(&self, window: &Webview) -> Result<(), String> {
        // Check before navigation, while the newly created page is still blank.
        background_geometry(window).await?;
        let assets = self.assets.clone();
        let profile = self.profile.clone();
        let destination = self.destination.clone();
        let downloads = self.downloads.clone();
        let website = self.website.clone();
        let download_website = self.website.clone();
        let project_loads = self.project_loads.clone();
        let component_redirects = self.component_redirects.clone();
        let html = std::fs::read(self.assets.join("7daystodie/mods/2264"))
            .map_err(|error| error.to_string())?;
        let (reply, response) = tokio::sync::oneshot::channel();
        window
            .with_webview(move |platform| {
                let result = (|| -> Result<(), String> {
                    // SAFETY: with_webview runs these COM calls on WebView2's STA.
                    let environment: ICoreWebView2Environment7 = platform
                        .environment()
                        .cast()
                        .map_err(|error| error.to_string())?;
                    let mut actual = PWSTR::null();
                    let queried = unsafe { environment.UserDataFolder(&mut actual) };
                    let actual = PathBuf::from(take_pwstr(actual));
                    queried.map_err(|error| error.to_string())?;
                    if !dunce::simplified(&actual)
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&profile.to_string_lossy())
                    {
                        return Err("WebView2 did not use the isolated fixture profile".into());
                    }
                    let core = unsafe { platform.controller().CoreWebView2() }
                        .map_err(|error| error.to_string())?;
                    let profiled: ICoreWebView2_13 =
                        core.cast().map_err(|error| error.to_string())?;
                    let browser_profile =
                        unsafe { profiled.Profile() }.map_err(|error| error.to_string())?;
                    let download_folder = wide(&destination.parent().unwrap().to_string_lossy());
                    unsafe {
                        browser_profile
                            .SetDefaultDownloadFolderPath(PCWSTR(download_folder.as_ptr()))
                    }
                    .map_err(|error| error.to_string())?;
                    let mapping: ICoreWebView2_3 =
                        core.cast().map_err(|error| error.to_string())?;
                    let folder = wide(&assets.to_string_lossy());
                    let host = wide("cf-files.nexusmods.com");
                    unsafe {
                        mapping.SetVirtualHostNameToFolderMapping(
                            PCWSTR(host.as_ptr()),
                            PCWSTR(folder.as_ptr()),
                            COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_ALLOW,
                        )
                    }
                    .map_err(|error| error.to_string())?;
                    // A real project URL has no .html suffix. Serve it with an explicit MIME
                    // type instead of relying on folder mapping's extension inference.
                    let filter = wide("https://www.nexusmods.com/*");
                    unsafe {
                        core.AddWebResourceRequestedFilter(
                            PCWSTR(filter.as_ptr()),
                            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                        )
                    }
                    .map_err(|error| error.to_string())?;
                    let response_environment = platform.environment().clone();
                    let mut token = 0;
                    unsafe {
                        core.add_WebResourceRequested(
                            &WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
                                let Some(args) = args else { return Ok(()) };
                                let request = args.Request()?;
                                let mut uri = PWSTR::null();
                                request.Uri(&mut uri)?;
                                let uri = take_pwstr(uri);
                                if uri == COMPONENT_ROUTE {
                                    // This response is a controlled fixture, not a claim
                                    // about the live website's response or API contract.
                                    component_redirects.fetch_add(1, Ordering::AcqRel);
                                    args.SetResponse(&component_redirect_response(
                                        &response_environment,
                                    )?)?;
                                    return Ok(());
                                }
                                let is_project = uri.split('?').next() == Some(PROJECT_URL);
                                if is_project {
                                    project_loads.fetch_add(1, Ordering::AcqRel);
                                }
                                let is_observation =
                                    reqwest::Url::parse(&uri).ok().is_some_and(|url| {
                                        if url.path() != "/__nexus_smoke_progress" {
                                            return false;
                                        }
                                        let values = url
                                            .query_pairs()
                                            .into_owned()
                                            .collect::<std::collections::HashMap<_, _>>();
                                        let value = WebsiteObservation {
                                            manual_clicks: values
                                                .get("manual")
                                                .and_then(|value| value.parse().ok())
                                                .unwrap_or_default(),
                                            slow_clicks: values
                                                .get("slow")
                                                .and_then(|value| value.parse().ok())
                                                .unwrap_or_default(),
                                            file_id: values
                                                .get("file")
                                                .cloned()
                                                .unwrap_or_default(),
                                            countdown_finished: values
                                                .get("done")
                                                .is_some_and(|value| value == "true"),
                                            waited_ms: values
                                                .get("waited")
                                                .and_then(|value| value.parse().ok())
                                                .unwrap_or_default(),
                                        };
                                        if let Ok(mut latest) = website.lock() {
                                            *latest = Some(value);
                                        }
                                        true
                                    });
                                let response = html_response(
                                    &response_environment,
                                    if is_project { &html } else { &[] },
                                    is_project || is_observation,
                                )?;
                                args.SetResponse(&response)?;
                                Ok(())
                            })),
                            &mut token,
                        )
                    }
                    .map_err(|error| error.to_string())?;
                    let downloadable: ICoreWebView2_4 =
                        core.cast().map_err(|error| error.to_string())?;
                    let mut token = 0;
                    unsafe {
                        downloadable.add_DownloadStarting(
                            &DownloadStartingEventHandler::create(Box::new(move |view, args| {
                                let Some(args) = args else { return Ok(()) };
                                let mut cancelled = BOOL::default();
                                args.Cancel(&mut cancelled)?;
                                let operation = args.DownloadOperation()?;
                                let mut uri = PWSTR::null();
                                operation.Uri(&mut uri)?;
                                let uri = take_pwstr(uri);
                                if let Ok(mut observed) = downloads.lock() {
                                    observed.push(DownloadObservation {
                                        official_fixture_url: uri == DOWNLOAD_URL,
                                        cancelled_by_product: cancelled.as_bool(),
                                        website: download_website
                                            .lock()
                                            .ok()
                                            .and_then(|latest| latest.clone()),
                                    });
                                }
                                // A broken product callback must still never write to the user's Downloads.
                                let destination = wide(&destination.to_string_lossy());
                                args.SetResultFilePath(PCWSTR(destination.as_ptr()))?;
                                args.SetCancel(true)?;
                                if let Some(view) = view {
                                    let script = wide("window.__nexusSmokeDownloadObserved = true");
                                    view.ExecuteScript(PCWSTR(script.as_ptr()), None)?;
                                }
                                Ok(())
                            })),
                            &mut token,
                        )
                    }
                    .map_err(|error| error.to_string())?;
                    Ok(())
                })();
                let _ = reply.send(result);
            })
            .map_err(|error| error.to_string())?;
        tokio::time::timeout(Duration::from_secs(3), response)
            .await
            .map_err(|_| "Timed out attaching the native fixture".to_owned())?
            .map_err(|_| "Native fixture attachment lost its response".to_owned())??;
        self.attached.store(true, Ordering::Release);
        Ok(())
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[link(name = "shlwapi")]
unsafe extern "system" {
    fn SHCreateMemStream(initial: *const u8, length: u32) -> *mut std::ffi::c_void;
}

fn html_response(
    environment: &ICoreWebView2Environment,
    html: &[u8],
    found: bool,
) -> windows_core::Result<ICoreWebView2WebResourceResponse> {
    let reason = wide(if found { "OK" } else { "Not Found" });
    let headers = wide(&format!(
        "Content-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\n",
        html.len()
    ));
    // SAFETY: SHCreateMemStream copies the fixture bytes into an owned COM IStream.
    // The base environment ABI accepts that IStream pointer and retains it for the
    // response. IUnknown releases our reference after the response is constructed.
    // Using the ABI here avoids adding a dependency merely to name Windows' IStream.
    unsafe {
        let stream = SHCreateMemStream(html.as_ptr(), html.len() as u32);
        if stream.is_null() {
            return Err(windows_core::Error::from_hresult(HRESULT(
                0x8007000Eu32 as i32,
            )));
        }
        let stream = IUnknown::from_raw(stream);
        let mut response = std::ptr::null_mut();
        (Interface::vtable(environment).CreateWebResourceResponse)(
            environment.as_raw(),
            stream.as_raw(),
            if found { 200 } else { 404 },
            PCWSTR(reason.as_ptr()),
            PCWSTR(headers.as_ptr()),
            &mut response,
        )
        .ok()?;
        Ok(ICoreWebView2WebResourceResponse::from_raw(response))
    }
}

fn component_redirect_response(
    environment: &ICoreWebView2Environment,
) -> windows_core::Result<ICoreWebView2WebResourceResponse> {
    let response = html_response(environment, &[], true)?;
    let reason = wide("Found");
    let name = wide("Location");
    let destination = wide(&format!("{PROJECT_URL}?tab=files&file_id=88"));
    // The native callback must allow the authorized ordinary navigation before
    // this local 302 can return to a separately verified project document.
    unsafe {
        response.SetStatusCode(302)?;
        response.SetReasonPhrase(PCWSTR(reason.as_ptr()))?;
        response
            .Headers()?
            .AppendHeader(PCWSTR(name.as_ptr()), PCWSTR(destination.as_ptr()))?;
    }
    Ok(response)
}

pub(crate) fn run_if_requested() -> bool {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if arguments.first().map(String::as_str) == Some("--nexus-browser-live-probe") {
        let result = match arguments.as_slice() {
            [_, url, root] => live_probe::run(url, PathBuf::from(root), None),
            [_, url, root, flag, previous] if flag == "--profile-from" => {
                live_probe::run(url, PathBuf::from(root), Some(PathBuf::from(previous)))
            }
            _ => Err(
                "Usage: --nexus-browser-live-probe <public project URL> <new absolute directory> [--profile-from <previous probe directory>]"
                    .into(),
            ),
        };
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return true;
    }
    if arguments.first().map(String::as_str) != Some("--nexus-browser-smoke") {
        return false;
    }
    let result = match arguments.as_slice() {
        [_, root] => run(PathBuf::from(root)),
        _ => Err("Usage: --nexus-browser-smoke <new absolute directory>".into()),
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
    true
}

fn run(root: PathBuf) -> Result<(), String> {
    if !root.is_absolute() {
        return Err("Fixture directory must be absolute".into());
    }
    // Refuse an existing profile/report. Every writable runtime path stays below root.
    std::fs::create_dir(&root).map_err(|error| error.to_string())?;
    main_ipc::initialize_storage(&root)?;
    let main_assets = main_ipc::assets(&root)?;
    let main_url = "http://nexussmoke.localhost/main.html".parse().unwrap();
    unsafe {
        std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", root.join("profile"));
        for key in [
            "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
            "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER",
            "WEBVIEW2_PIPE_FOR_SCRIPT_DEBUGGER",
            "WEBVIEW2_WAIT_FOR_SCRIPT_DEBUGGER",
        ] {
            std::env::remove_var(key);
        }
    }
    let main_ready = Arc::new(AtomicBool::new(false));
    let ready = main_ready.clone();
    let mut context = tauri::tauri_build_context!();
    context.config_mut().app.windows.clear();
    let app = tauri::Builder::default()
        .manage(BrowserState::new(root.join("logs/desktop-ui/active.jsonl")))
        .manage(ActiveFixture::default())
        .manage(login::ActiveFixture::default())
        .manage(crate::state::DesktopState::default())
        .register_uri_scheme_protocol("nexussmoke", move |_, request| {
            let (content_type, body) = main_assets
                .get(request.uri().path())
                .map(|(kind, bytes)| (*kind, bytes.clone()))
                .unwrap_or(("text/plain", Vec::new()));
            tauri::http::Response::builder()
                .header("Content-Type", content_type)
                .body(body)
                .expect("fixture response")
        })
        .invoke_handler(crate::command_registry::invoke_handler)
        .on_page_load(move |view, payload| {
            if view.label() == "main" {
                match payload.event() {
                    tauri::webview::PageLoadEvent::Started => close_active_host(view.app_handle()),
                    tauri::webview::PageLoadEvent::Finished => ready.store(true, Ordering::Release),
                }
            }
        })
        .setup(move |app| {
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(main_url))
                .inner_size(1100.0, 800.0)
                .visible(false)
                .focused(false)
                .skip_taskbar(true)
                .build()?;
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let result = cases::verify(&handle, &root, &main_ready).await;
                let report = match &result {
                    Ok(value) => value.clone(),
                    Err(error) => json!({ "status": "failed", "error": error }),
                };
                let saved = std::fs::write(root.join("result.json"), report.to_string());
                if let Err(error) = &saved {
                    eprintln!("Write Nexus smoke report: {error}");
                }
                handle.exit(if result.is_ok() && saved.is_ok() {
                    0
                } else {
                    1
                });
            });
            Ok(())
        })
        .build(context)
        .map_err(|error| error.to_string())?;
    app.run(|_, event| {
        if let tauri::RunEvent::ExitRequested {
            code: None, api, ..
        } = event
        {
            api.prevent_exit();
        }
    });
    Ok(())
}

async fn child_visible(view: &Webview) -> Result<bool, String> {
    let (reply, response) = tokio::sync::oneshot::channel();
    view.with_webview(move |platform| {
        let mut visible = BOOL::default();
        let result = unsafe { platform.controller().IsVisible(&mut visible) }
            .map(|()| visible.as_bool())
            .map_err(|error| error.to_string());
        let _ = reply.send(result);
    })
    .map_err(|error| error.to_string())?;
    tokio::time::timeout(Duration::from_secs(3), response)
        .await
        .map_err(|_| "Child visibility query timed out".to_owned())?
        .map_err(|_| "Child visibility query was dropped".to_owned())?
}

fn target() -> NexusWebsiteTarget {
    NexusWebsiteTarget {
        page_url: PROJECT_URL.into(),
        game_domain: "7daystodie".into(),
        mod_id: "2264".into(),
        accepts: vec!["zip".into()],
    }
}

async fn ensure_closed(app: &tauri::AppHandle) -> Result<(), String> {
    wait_until(
        || {
            !app.webviews()
                .values()
                .any(|view| is_browser_window(view.label()))
        },
        "Native browsing left a webview behind",
    )
    .await?;
    if app.get_webview_window("main").is_none()
        || host::has_session(app)
        || app.windows().len() != 1
    {
        return Err("Closing native browsing damaged the main window or reserved session".into());
    }
    Ok(())
}

async fn wait_until(mut condition: impl FnMut() -> bool, error: &str) -> Result<(), String> {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .map_err(|_| error.to_owned())
}

async fn ipc_result(window: &Webview) -> Result<serde_json::Value, String> {
    let started = Instant::now();
    loop {
        let (reply, mut response) = tokio::sync::mpsc::channel(1);
        window
            .eval_with_callback("window.__nexusSmokeIpc ?? null", move |value| {
                let _ = reply.try_send(value);
            })
            .map_err(|error| error.to_string())?;
        let value = tokio::time::timeout(Duration::from_secs(2), response.recv())
            .await
            .map_err(|_| "IPC probe callback timed out".to_owned())?
            .ok_or_else(|| "IPC probe callback was dropped".to_owned())?;
        let value: serde_json::Value =
            serde_json::from_str(&value).map_err(|error| error.to_string())?;
        if let Some(results) = value.as_array() {
            if results.len() == 3
                && results
                    .iter()
                    .zip([
                        ("app_version", "app_version"),
                        ("open_nexus_website_host", "open_nexus_website_host"),
                        ("plugin:window|is_visible", "window.is_visible"),
                    ])
                    .all(|(result, (command, denial_name))| {
                        result["command"] == command
                            && result["error"].as_str().is_some_and(|error| {
                                error == "Public project pages cannot invoke application commands"
                                    || error == format!("Command {command} not allowed by ACL")
                                    || error.starts_with(&format!("{denial_name} not allowed"))
                            })
                    })
            {
                return Ok(json!({ "application_commands_rejected": 2,
                    "window_commands_rejected": 1, "rejections": results }));
            }
            return Err(format!(
                "Public page IPC was not explicitly rejected: {value}"
            ));
        }
        if started.elapsed() > Duration::from_secs(5) {
            return Err(format!("Public page IPC probe did not complete: {value}"));
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

const PAGE: &str = r#"<!doctype html><html><head><meta charset="utf-8"><title>Nexus native fixture</title>
<link rel="canonical" href="https://www.nexusmods.com/7daystodie/mods/2264"></head><body><main></main>
<script>
(() => {
  const flow = '$FLOW$';
  const project = 'https://www.nexusmods.com/7daystodie/mods/2264';
  // Keep the explicit choice in the native target even when the site returns
  // its file list first. The ordinary link must still choose the requested file.
  if (flow === 'explicit') history.replaceState(null, '', project + '?tab=files');
  if (flow === 'dialog') history.replaceState(null, '', project + '?file_id=88&tab=files');
  if (flow === 'verification' && sessionStorage.getItem('nexusSmokeVerifying')) window.__nexusSmokeRender = true;
  const componentSelected = flow === 'component' && new URL(location.href).searchParams.get('file_id') === '88';
  if (componentSelected) window.__nexusSmokeRender = true;
  const actions = componentSelected ? JSON.parse(sessionStorage.getItem('nexusSmokeComponentActions'))
    : { manual: 0, slow: 0, file: '', done: false, waited: 0 };
  let reports = Promise.resolve();
  const publish = () => {
    if (flow === 'component') sessionStorage.setItem('nexusSmokeComponentActions', JSON.stringify(actions));
    const query = new URLSearchParams(actions).toString();
    reports = reports.then(() => fetch('/__nexus_smoke_progress?' + query));
    return reports;
  };
  const finishMetadata = () => {
    document.querySelector('main').innerHTML = '<h1>Server at Home at Night</h1>';
    const title = document.createElement('meta');
    title.setAttribute('property', 'og:title');
    title.content = 'Server at Home at Night at 7 Days To Die Nexus - Mods and community';
    document.head.appendChild(title);
    const icon = document.createElement('meta');
    icon.setAttribute('property', 'og:image');
    icon.content = 'https://staticdelivery.nexusmods.com/mods/1059/images/9559/9559-1770000000-123456789.png';
    document.head.appendChild(icon);
  };
  window.__nexusSmokeIpc = null;
  Promise.all(['app_version', 'open_nexus_website_host', 'plugin:window|is_visible'].map(async command => {
    try {
      const result = await window.__TAURI_INTERNALS__.invoke(command);
      return { command, result };
    } catch (error) { return { command, error: String(error) }; }
  })).then(result => { window.__nexusSmokeIpc = result; });
  const fileRow = (id) => '<article><h3>File ' + id + '</h3><a href="' + project
    + '?file_id=' + id + '&amp;tab=files">Manual download</a></article>';
  const render = setInterval(() => {
    if (!window.__nexusSmokeRender) return;
    clearInterval(render);
    if (flow !== 'verification') {
      document.body.insertAdjacentHTML('afterbegin', '<div class="rj-header-wrap"><div class="quick-search-holder"><quick-search is-logged-in="' + (flow === 'login' ? 'false' : 'true') + '"></quick-search></div></div>');
    }
    if (flow === 'login') {
      document.querySelector('main').innerHTML = '<h1>Mod Manager</h1><div id="loggedOutPremiumBanner" class="premium-banner"><span>You have to be logged in to download files</span></div>';
      return;
    }
    if (flow === 'verification') {
      document.title = 'Just a moment...';
      document.querySelector('main').innerHTML = '<h1>Security verification</h1><p>Verify you are human</p><button>Verify</button>';
      sessionStorage.setItem('nexusSmokeVerifying', 'true');
      setTimeout(() => location.reload(), 11000);
      return;
    }
    if (flow === 'dialog') {
      document.querySelector('main').innerHTML = '<div role="dialog"><h2>Requirements</h2><button>Slow Download</button></div>';
      return;
    }
    if (flow === 'no_controls') {
      history.replaceState(null, '', project + '?tab=files&token=fixture-client-secret');
      document.querySelector('main').innerHTML = '<h1>Mod Manager</h1>';
      return;
    }
    if (flow === 'component') {
      if (componentSelected) {
        document.querySelector('main').innerHTML = '<h2>Download options</h2><mod-file-download file-id="88" game-domain="/7daystodie" user-is-logged-in="true"></mod-file-download>';
        document.querySelector('mod-file-download').attachShadow({ mode: 'open' }).innerHTML = '<button type="button">Slow download</button>';
      } else {
        // This new control needs verified metadata before native authorizes its UID.
        // The previous cases still exercise completely late title and icon metadata.
        document.querySelector('main').innerHTML = '<h1>Server at Home at Night</h1><div id="mod_files"><div id="file-container-main-files"><h2>Main files</h2><dl class="accordion"><dt data-id="88" id="file-expander-header-88">File 88</dt><dd data-id="88"><div class="tabbed-block"><ul class="accordion-downloads"><li><mod-download-modal placement="files_tab"></mod-download-modal></li></ul></div></dd></dl></div></div>';
        const component = document.querySelector('mod-download-modal');
        component.setAttribute('file', JSON.stringify({ uid:'1059000088', category:1, downloadUrl:'/api/files/1059000088/download' }));
        component.attachShadow({ mode:'open' }).innerHTML = '<a href="/api/files/1059000088/download">Manual download</a>';
      }
      return;
    }
    document.querySelector('main').innerHTML = '<section><h2>Main files</h2>' + fileRow('88')
      + (flow === 'automatic' ? '' : fileRow('99')) + '</section>';
  }, 25);
  document.querySelector('main').addEventListener('click', event => {
    const clicked = event.composedPath()[0];
    const manual = clicked.closest('a');
    if (manual) {
      event.preventDefault();
      actions.manual++;
      if (flow === 'component') {
        actions.file = '88';
        publish().then(() => { location.href = manual.href; });
        return;
      }
      actions.file = new URL(manual.href).searchParams.get('file_id');
      history.replaceState(null, '', manual.href);
      if (flow === 'automatic') {
        document.querySelector('main').innerHTML = '<h2>Download options</h2><mod-file-download file-id="' + actions.file + '" game-domain="/7daystodie" user-is-logged-in="true"></mod-file-download>';
        document.querySelector('mod-file-download').attachShadow({ mode: 'open' }).innerHTML = '<button type="button">Slow download</button>';
      } else {
        document.querySelector('main').innerHTML = '<button type="button">Slow Download</button>';
      }
      publish();
      return;
    }
    const slow = clicked.closest('button');
    if (!slow || slow.disabled) return;
    actions.slow++;
    slow.disabled = true;
    document.querySelector('main').insertAdjacentHTML('beforeend', '<p>Your download will start automatically in 1 second.</p>');
    const started = performance.now();
    publish();
    // Only the site's own timer starts the real request; the product must not
    // manufacture a CDN URL, enable the button, or skip this delay.
    setTimeout(async () => {
      actions.done = true;
      actions.waited = Math.floor(performance.now() - started);
      await publish();
      const frame = document.createElement('iframe');
      frame.hidden = true;
      frame.src = 'https://cf-files.nexusmods.com/fixture.zip?token=fixture-client-secret';
      document.body.appendChild(frame);
    }, 750);
  });
  // Metadata is still deliberately late, after the real native download event.
  const wait = setInterval(() => {
    if (window.__nexusSmokeDownloadObserved) { clearInterval(wait); finishMetadata(); }
  }, 25);
})();
</script></body></html>"#;
