//! User-opened login uses the real host/IPC and a fresh, local official-origin fixture.
use super::*;
use webview2_com::NavigationStartingEventHandler;

#[derive(Default)]
pub(super) struct ActiveFixture(pub(super) Mutex<Option<Arc<LoginFixture>>>);

pub(crate) struct LoginFixture {
    profile: PathBuf,
    destination: PathBuf,
    pub(super) downloads: Arc<Mutex<Vec<bool>>>,
    external_requests: Arc<AtomicUsize>,
    external_kinds: Arc<Mutex<Vec<&'static str>>>,
    external_navigation_cancelled: Arc<Mutex<Vec<bool>>>,
    external_document_executions: Arc<AtomicUsize>,
}

impl LoginFixture {
    fn new(root: &Path) -> Result<Self, String> {
        let directory = root.join("login-downloads");
        std::fs::create_dir(&directory).map_err(|error| error.to_string())?;
        Ok(Self {
            profile: root.join("profile"),
            destination: directory.join("login-fixture.zip"),
            downloads: Arc::default(),
            external_requests: Arc::default(),
            external_kinds: Arc::default(),
            external_navigation_cancelled: Arc::default(),
            external_document_executions: Arc::default(),
        })
    }

    pub(crate) async fn attach(&self, view: &Webview) -> Result<(), String> {
        let profile = self.profile.clone();
        let destination = self.destination.clone();
        let downloads = self.downloads.clone();
        let external_requests = self.external_requests.clone();
        let external_kinds = self.external_kinds.clone();
        let external_navigation_cancelled = self.external_navigation_cancelled.clone();
        let external_document_executions = self.external_document_executions.clone();
        let (reply, response) = tokio::sync::oneshot::channel();
        view.with_webview(move |platform| {
            let result = (|| -> Result<(), String> {
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
                    return Err(
                        "Login did not use the same isolated default WebView2 profile".into(),
                    );
                }
                let core = unsafe { platform.controller().CoreWebView2() }
                    .map_err(|error| error.to_string())?;
                let mut token = 0;
                unsafe {
                    core.add_NavigationStarting(
                        &NavigationStartingEventHandler::create(Box::new(move |_, args| {
                            let Some(args) = args else {
                                return Ok(());
                            };
                            let mut uri = PWSTR::null();
                            args.Uri(&mut uri)?;
                            if reqwest::Url::parse(&take_pwstr(uri))
                                .is_ok_and(|url| url.host_str() == Some("outside.invalid"))
                            {
                                let mut cancelled = BOOL::default();
                                args.Cancel(&mut cancelled)?;
                                if let Ok(mut values) = external_navigation_cancelled.lock() {
                                    values.push(cancelled.as_bool());
                                }
                            }
                            Ok(())
                        })),
                        &mut token,
                    )
                }
                .map_err(|error| error.to_string())?;
                let filter = wide("*");
                unsafe {
                    core.AddWebResourceRequestedFilter(
                        PCWSTR(filter.as_ptr()),
                        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                    )
                }
                .map_err(|error| error.to_string())?;
                let environment = platform.environment().clone();
                let mut token = 0;
                unsafe {
                    core.add_WebResourceRequested(
                        &WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
                            let Some(args) = args else {
                                return Ok(());
                            };
                            let request = args.Request()?;
                            let mut uri = PWSTR::null();
                            request.Uri(&mut uri)?;
                            let uri = take_pwstr(uri);
                            let url = reqwest::Url::parse(&uri).ok();
                            let host = url.as_ref().and_then(|url| url.host_str());
                            let path = url.as_ref().map(|url| url.path());
                            // Keep the real Tauri IPC response and ACL rejection intact.
                            if host == Some("ipc.localhost") {
                                return Ok(());
                            }
                            let body = match (host, path) {
                                (Some("users.nexusmods.com"), Some("/auth/sign_in")) => {
                                    LOGIN_PAGE.as_bytes()
                                }
                                (Some("users.nexusmods.com"), Some("/account/security")) => {
                                    ACCOUNT_PAGE.as_bytes()
                                }
                                (Some("www.nexusmods.com"), Some("/")) => {
                                    SIGNED_OUT_PAGE.as_bytes()
                                }
                                (Some("www.nexusmods.com"), Some("/login-fixture.zip")) => {
                                    args.SetResponse(&zip_response(&environment)?)?;
                                    return Ok(());
                                }
                                (Some("outside.invalid"), Some("/__executed")) => {
                                    external_document_executions.fetch_add(1, Ordering::AcqRel);
                                    &[]
                                }
                                (Some("outside.invalid"), _) => {
                                    external_requests.fetch_add(1, Ordering::AcqRel);
                                    EXTERNAL_PAGE.as_bytes()
                                }
                                _ => {
                                    if !matches!(
                                        host,
                                        Some(
                                            "users.nexusmods.com"
                                                | "www.nexusmods.com"
                                                | "nexusmods.com"
                                        )
                                    ) {
                                        external_requests.fetch_add(1, Ordering::AcqRel);
                                        if let Ok(mut kinds) = external_kinds.lock()
                                            && kinds.len() < 8
                                        {
                                            kinds.push(
                                                match (url.as_ref().map(|url| url.scheme()), host) {
                                                    (_, Some("outside.invalid")) => {
                                                        "test-external-host"
                                                    }
                                                    (_, Some("ipc.localhost")) => "tauri-ipc",
                                                    (Some("about"), _) => "about",
                                                    (Some("data"), _) => "data",
                                                    _ => "other",
                                                },
                                            );
                                        }
                                    }
                                    &[]
                                }
                            };
                            args.SetResponse(&html_response(
                                &environment,
                                body,
                                !body.is_empty(),
                            )?)?;
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
                        &DownloadStartingEventHandler::create(Box::new(move |_, args| {
                            let Some(args) = args else {
                                return Ok(());
                            };
                            let mut cancelled = BOOL::default();
                            args.Cancel(&mut cancelled)?;
                            if let Ok(mut events) = downloads.lock() {
                                events.push(cancelled.as_bool());
                            }
                            let path = wide(&destination.to_string_lossy());
                            args.SetResultFilePath(PCWSTR(path.as_ptr()))?;
                            args.SetCancel(true)?;
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
            .map_err(|_| "Login fixture attachment timed out")?
            .map_err(|_| "Login fixture attachment was dropped")??;
        Ok(())
    }
}

fn zip_response(
    environment: &ICoreWebView2Environment,
) -> windows_core::Result<ICoreWebView2WebResourceResponse> {
    let bytes = b"PK\x05\x06\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";
    let reason = wide("OK");
    let headers = wide(
        "Content-Type: application/zip\r\nContent-Length: 22\r\nContent-Disposition: attachment; filename=login-fixture.zip\r\nCache-Control: no-store\r\n",
    );
    unsafe {
        let stream = SHCreateMemStream(bytes.as_ptr(), bytes.len() as u32);
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
            200,
            PCWSTR(reason.as_ptr()),
            PCWSTR(headers.as_ptr()),
            &mut response,
        )
        .ok()?;
        Ok(ICoreWebView2WebResourceResponse::from_raw(response))
    }
}

async fn invoke(
    main: &Webview,
    command: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    main.eval(format!("window.__nexusLoginReply=null; (()=>{{const command={}, args={}; if(command==='open_nexus_login_host'){{window.__nexusLoginAuthChanges??=[]; args.onAuthChange=new window.__nexusMain.Channel(value=>window.__nexusLoginAuthChanges.push(value));}} return window.__nexusMain.invoke(command,args);}})().then(value=>window.__nexusLoginReply={{value}}).catch(error=>{{try{{window.__nexusLoginReply={{error:JSON.parse(String(error))}};}}catch{{window.__nexusLoginReply={{error:{{code:'unexpected'}}}};}}}})", json!(command), args))
        .map_err(|error| error.to_string())?;
    let start = Instant::now();
    loop {
        let result = main_ipc::read_js(main, "window.__nexusLoginReply").await?;
        if !result.is_null() {
            return Ok(result);
        }
        if start.elapsed() > CASE_TIMEOUT {
            return Err("Login main IPC did not settle".into());
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn visible(view: &Webview, expected: bool) -> Result<(), String> {
    let started = Instant::now();
    while child_visible(view).await? != expected {
        if started.elapsed() > Duration::from_secs(5) {
            return Err("Login child visibility did not settle".into());
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    Ok(())
}

async fn bounds(main: &Webview, id: &str, value: serde_json::Value) -> Result<(), String> {
    let result = invoke(
        main,
        "update_nexus_login_host",
        json!({"sessionId": id, "bounds": value}),
    )
    .await?;
    if result.get("error").is_some() {
        return Err(format!("Login bounds failed: {result}"));
    }
    Ok(())
}

async fn wait_auth(main: &Webview, expected: &str) -> Result<(), String> {
    let started = Instant::now();
    loop {
        let cached = invoke(main, "read_nexus_website_auth_state", json!({})).await?;
        let latest = main_ipc::read_js(main, "window.__nexusLoginAuthChanges?.at(-1)").await?;
        if cached["value"] == expected && latest == expected {
            return Ok(());
        }
        if started.elapsed() > Duration::from_secs(5) {
            return Err(format!(
                "Official authentication observation did not reach {expected}"
            ));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

pub(super) async fn verify(
    app: &tauri::AppHandle,
    root: &Path,
) -> Result<serde_json::Value, String> {
    let main = app.get_webview("main").ok_or("Main view missing")?;
    let loaded = Instant::now();
    while main_ipc::read_js(&main, "Boolean(window.__nexusMain)").await? != true {
        if loaded.elapsed() > CASE_TIMEOUT {
            return Err("Real Tauri SDK did not load for the login fixture".into());
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    if invoke(&main, "read_nexus_website_auth_state", json!({})).await?["value"] != "unknown" {
        return Err("A new process invented website authentication".into());
    }
    let fixture = Arc::new(LoginFixture::new(root)?);
    *app.state::<ActiveFixture>()
        .0
        .lock()
        .map_err(|_| "Login fixture poisoned")? = Some(fixture.clone());
    let result = invoke(&main, "open_nexus_login_host", json!({"bounds": null})).await?;
    let id = result["value"]
        .as_str()
        .ok_or_else(|| format!("Login open failed: {result}"))?
        .to_owned();
    if app.webviews().len() != 1 || app.windows().len() != 1 {
        return Err("Login opened before the user supplied its slot".into());
    }
    let busy = invoke(&main, "open_nexus_website_host", json!({})).await?;
    if busy["error"]["code"] != "nexus_browser_busy"
        || SessionLease::begin(app, id.clone()).err() != Some(error("nexus_browser_cancelled"))
    {
        return Err("Login reservation allowed a background download".into());
    }
    bounds(&main, &id, json!({"x":60,"y":100,"width":700,"height":400})).await?;
    let view = app
        .get_webview(&format!("{WINDOW_PREFIX}login-{id}"))
        .ok_or("Login child missing")?;
    let ipc = ipc_result(&view).await?;
    visible(&view, true).await?;
    if main_ipc::read_js(&main, "window.__nexusLoginAuthChanges").await? != json!(["unknown"]) {
        return Err("Login form was mistaken for a completed authentication".into());
    }
    if view.window().label() != "main"
        || app.windows().len() != 1
        || crate::curseforge_browser::main_window(app).is_none()
    {
        return Err("Login lost its main parent or opened a top-level window".into());
    }
    if main_ipc::read_js(&view, "window.__loginFixtureClicks").await? != 0 {
        return Err("The application automatically interacted with login".into());
    }
    bounds(&main, &id, json!({"x":80,"y":120,"width":650,"height":420})).await?;
    let position = view.position().map_err(|error| error.to_string())?;
    let size = view.size().map_err(|error| error.to_string())?;
    if position.x != 80 || position.y != 120 || size.width != 650 || size.height != 420 {
        return Err("Login bounds did not update".into());
    }
    bounds(&main, &id, serde_json::Value::Null).await?;
    visible(&view, false).await?;
    bounds(
        &main,
        &id,
        json!({"x":1000,"y":700,"width":650,"height":420}),
    )
    .await?;
    visible(&view, false).await?;
    bounds(&main, &id, json!({"x":60,"y":100,"width":700,"height":400})).await?;
    visible(&view, true).await?;
    // This click represents the test user, not a production auto-login action.
    view.eval("document.getElementById('fixture-login').click()")
        .map_err(|error| error.to_string())?;
    wait_until(
        || {
            view.url().is_ok_and(|url| {
                url.host_str() == Some("users.nexusmods.com") && url.path() == "/account/security"
            })
        },
        "Official login redirect failed",
    )
    .await?;
    let start = Instant::now();
    while main_ipc::read_js(
        &view,
        "document.getElementById('main-content-wrapper') !== null",
    )
    .await?
        != true
    {
        if start.elapsed() > CASE_TIMEOUT {
            return Err("Official login destination did not load".into());
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    if main_ipc::read_js(
        &view,
        "document.querySelectorAll('quick-search, mobile-quick-search').length",
    )
    .await?
        != 0
    {
        return Err("The observed users account fixture contains an unrelated WWW header".into());
    }
    wait_auth(&main, "signed_in").await?;
    if main_ipc::read_js(&main, "window.__nexusLoginAuthChanges").await?
        != json!(["unknown", "signed_in"])
    {
        return Err("Official users account did not emit one signed-in transition".into());
    }
    view.navigate("https://www.nexusmods.com/".parse().unwrap())
        .map_err(|error| error.to_string())?;
    wait_auth(&main, "signed_out").await?;
    if main_ipc::read_js(&main, "window.__nexusLoginAuthChanges").await?
        != json!(["unknown", "signed_in", "signed_out"])
    {
        return Err("A new signed-out observation did not replace prior signed-in state".into());
    }
    view.eval("window.__nexusPopup=window.open('https://outside.invalid/','_blank'); document.getElementById('fixture-download').click()").map_err(|error| error.to_string())?;
    wait_until(
        || {
            fixture
                .downloads
                .lock()
                .is_ok_and(|events| !events.is_empty())
        },
        "Login download cancellation was not observed",
    )
    .await?;
    if *fixture
        .downloads
        .lock()
        .map_err(|_| "Login observations poisoned")?
        != [true]
        || fixture.destination.exists()
        || app.windows().len() != 1
    {
        return Err("Login allowed a popup or native file write".into());
    }
    view.navigate("https://outside.invalid/".parse().unwrap())
        .map_err(|error| error.to_string())?;
    wait_until(
        || {
            fixture
                .external_navigation_cancelled
                .lock()
                .is_ok_and(|values| !values.is_empty())
        },
        "External navigation never reached the native cancellation observer",
    )
    .await?;
    tokio::time::sleep(Duration::from_millis(250)).await;
    let document = main_ipc::read_js(&view, "({official:location.hostname==='www.nexusmods.com',fixturePresent:!!document.getElementById('fixture-download'),externalExecuted:window.__nexusExternalExecuted===true,popupClosed:!window.__nexusPopup||window.__nexusPopup.closed})").await?;
    let cancellations = fixture
        .external_navigation_cancelled
        .lock()
        .map_err(|_| "Login navigation observations poisoned")?
        .clone();
    // WebView2 may issue a speculative GET before NavigationStarting is cancelled.
    // Verify its Cancel flag and retained document, rather than zero HTTP requests:
    // https://learn.microsoft.com/microsoft-edge/webview2/reference/win32/icorewebview2navigationstartingeventargs#get_cancel
    if !view
        .url()
        .is_ok_and(|url| url.host_str() == Some("www.nexusmods.com"))
        || cancellations.is_empty()
        || cancellations.iter().any(|cancelled| !cancelled)
        || document["official"] != true
        || document["fixturePresent"] != true
        || document["externalExecuted"] != false
        || document["popupClosed"] != true
        || fixture.external_document_executions.load(Ordering::Acquire) != 0
    {
        let host = view.url().ok().map(|url| match url.host_str() {
            Some("www.nexusmods.com") => "www",
            Some("users.nexusmods.com") => "users",
            Some("outside.invalid") => "test-external-host",
            _ => "other",
        });
        return Err(format!(
            "Login followed an unapproved external page: current_host={host:?}, document={document}, cancellations={cancellations:?}, external_requests={}, request_kinds={:?}",
            fixture.external_requests.load(Ordering::Acquire),
            fixture
                .external_kinds
                .lock()
                .map_err(|_| "Login request observations poisoned")?
        ));
    }
    let closed = invoke(&main, "close_nexus_login_host", json!({"sessionId": id})).await?;
    if closed.get("error").is_some() {
        return Err("Login close IPC failed".into());
    }
    ensure_closed(app).await?;
    let changes_after_close = main_ipc::read_js(&main, "window.__nexusLoginAuthChanges").await?;
    tokio::time::sleep(Duration::from_millis(1250)).await;
    if main_ipc::read_js(&main, "window.__nexusLoginAuthChanges").await? != changes_after_close
        || invoke(&main, "read_nexus_website_auth_state", json!({})).await?["value"] != "signed_out"
    {
        return Err("Closing login lost its last observation or emitted a later update".into());
    }

    let download_id = open_nexus_website_host(app.clone())?;
    if invoke(&main, "open_nexus_login_host", json!({"bounds":null})).await?["error"]["code"]
        != "nexus_browser_busy"
    {
        return Err("Login opened during a reserved background download".into());
    }
    let session = SessionLease::begin(app, download_id)?;
    let child = session.attach(tauri::webview::WebviewBuilder::new(
        format!("{WINDOW_PREFIX}profile-check"),
        WebviewUrl::External("about:blank".parse().unwrap()),
    ))?;
    let same_profile = Fixture::new(root, "login-profile-check", Flow::Automatic)?;
    same_profile.attach(&child.view).await?;
    child
        .view
        .navigate(PROJECT_URL.parse().unwrap())
        .map_err(|error| error.to_string())?;
    ipc_result(&child.view).await?;
    background_geometry(&child.view).await?;
    if main_ipc::read_js(
        &child.view,
        "localStorage.getItem('langame-nexus-smoke-session') === 'fixture-session'",
    )
    .await?
        != true
    {
        return Err(
            "The background download did not reuse the login view's default profile".into(),
        );
    }
    drop(child);
    drop(session);
    ensure_closed(app).await?;

    let opened = invoke(&main, "open_nexus_login_host", json!({"bounds":null})).await?;
    let next_id = opened["value"].as_str().ok_or("Login could not reopen")?;
    bounds(
        &main,
        next_id,
        json!({"x":60,"y":100,"width":700,"height":400}),
    )
    .await?;
    crate::curseforge_browser::main_window(app)
        .ok_or("Main handle missing during login")?
        .reload()
        .map_err(|error| error.to_string())?;
    ensure_closed(app).await?;
    *app.state::<ActiveFixture>()
        .0
        .lock()
        .map_err(|_| "Login fixture poisoned")? = None;
    Ok(
        json!({"real_main_ipc":true,"only_main_window":true,"shown_only_after_slot":true,
        "bounds_updated":true,"null_and_outside_bounds_hidden":true,"no_automatic_login":true,
        "official_redirect_allowed":true,"external_navigation_denied":true,"popup_denied":true,
        "external_requests_observed":fixture.external_requests.load(Ordering::Acquire),
        "external_navigation_cancelled":cancellations,"official_document_retained":true,
        "external_document_executions":fixture.external_document_executions.load(Ordering::Acquire),
        "native_download_cancelled":true,"default_profile_reused":true,"no_credentials_read":true,
        "mutual_exclusion":true,"close_and_main_reload_cleaned":true,"ipc":ipc,
        "auth_channel":changes_after_close,"auth_poll_stopped_on_close":true,"auth_cache_retained":true,
        "users_account_security_observed":true,"users_account_without_legacy_header":true}),
    )
}

const LOGIN_PAGE: &str = r#"<!doctype html><meta charset="utf-8"><title>Login fixture</title>
<h1>Sign in</h1><button id="fixture-login">Complete fixture login</button>
<script>
window.__loginFixtureClicks=0;
document.getElementById('fixture-login').onclick=()=>{window.__loginFixtureClicks++;location.href='https://users.nexusmods.com/account/security';};
window.__nexusSmokeIpc=null;
Promise.all(['app_version','open_nexus_website_host','plugin:window|is_visible'].map(async command=>{
  try{return {command,result:await window.__TAURI_INTERNALS__.invoke(command)};}catch(error){return {command,error:String(error)};}
})).then(result=>window.__nexusSmokeIpc=result);
</script>"#;

const ACCOUNT_PAGE: &str = r#"<!doctype html><meta charset="utf-8"><title>Account fixture</title>
<style>.d-none{display:none}.d-block{display:block}</style>
<div id="main-content-wrapper" class="content-wrapper d-flex justify-content-between flex-column">
  <div class="top-half"><div class="jumbotron jumbotron-border-bottom"><div class="links"><div class="right-link">
    <a class="d-none d-md-block btn btn-outline-primary" href="/auth/sign_out">Sign out</a>
    <a class="d-block d-md-none mobile-signout btn btn-outline-primary" href="/auth/sign_out">Sign out</a>
  </div></div></div></div>
</div>"#;

const SIGNED_OUT_PAGE: &str = r#"<!doctype html><meta charset="utf-8"><title>Public fixture</title>
<div class="rj-header-wrap"><div class="quick-search-holder"><quick-search is-logged-in="false"></quick-search></div></div>
<h1>Public website</h1><a id="fixture-download" href="/login-fixture.zip">Download fixture</a>
<script>localStorage.setItem('langame-nexus-smoke-session','fixture-session');</script>"#;

const EXTERNAL_PAGE: &str = r#"<!doctype html><title>Forbidden fixture</title>
<script>window.__nexusExternalExecuted=true;fetch('/__executed');</script>"#;
