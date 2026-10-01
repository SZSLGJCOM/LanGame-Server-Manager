#[cfg(test)]
use std::collections::HashMap;
use std::env;
#[cfg(test)]
use std::fs;
use std::future::Future;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use app_core::{
    AppPathSettingsInput, CreateInstanceInput, ExecuteInstanceManualPlayerActionInput,
    ExecuteInstancePlayerActionInput, UpdateInstanceBroadcastPolicyInput, UpdateInstanceInput,
};
use base64::Engine;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use subtle::ConstantTimeEq;
use tauri::Manager;

use crate::assistant::{AssistantRunInput, AssistantSecretDescriptor};
use crate::commands::{
    self, AssistantConfirmOperationInput, AssistantRequestInput, GenerateInstanceBroadcastInput,
    SendInstanceBroadcastInput,
};
use crate::state::DesktopState;

#[path = "lan_host_media.rs"]
mod media;

#[path = "lan_host_request.rs"]
mod request;
use request::{HttpRequestHead, content_length as request_content_length};

#[path = "lan_host_static.rs"]
mod static_files;
use static_files::serve_static_file;

const DEFAULT_LAN_PORT: u16 = 9088;
const MAX_REQUEST_HEADER_BYTES: usize = 64 * 1024;
const MAX_REQUEST_BODY_BYTES: usize = 16 * 1024 * 1024;
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(15);
const CONNECTION_READ_POLL_INTERVAL: Duration = Duration::from_millis(100);
const LAN_HOST_WORKER_COUNT: usize = 4;
const LAN_HOST_QUEUE_CAPACITY: usize = 16;
const LAN_HOST_POLL_INTERVAL: Duration = Duration::from_millis(25);
const OVERLOAD_RESPONSE_TIMEOUT: Duration = Duration::from_millis(100);
// Match the desktop's content restrictions, with same-origin HTTP transport
// replacing native IPC. HLS uses a blob worker to decode away from the UI thread.
const LAN_CONTENT_SECURITY_POLICY: &str = concat!(
    "default-src 'self'; base-uri 'none'; connect-src 'self' https:; ",
    "font-src 'self' data:; form-action 'none'; frame-src 'none'; frame-ancestors 'none'; ",
    "img-src 'self' blob: data: http: https:; media-src 'self' blob: data: http: https:; ",
    "object-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; worker-src 'self' blob:"
);

#[derive(Clone)]
struct LanAccessToken([u8; 32]);

impl std::fmt::Debug for LanAccessToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("LanAccessToken(<redacted>)")
    }
}

#[derive(Debug)]
struct LanHostConfigError(&'static str);

impl std::fmt::Display for LanHostConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl LanAccessToken {
    fn parse(value: &str) -> Result<Self, LanHostConfigError> {
        let trimmed = value.trim();
        if trimmed != value || trimmed.is_empty() || trimmed.contains('=') {
            return Err(LanHostConfigError("LAN token must be unpadded base64url"));
        }
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(trimmed)
            .map_err(|_| LanHostConfigError("LAN token must be unpadded base64url"))?;
        let bytes: [u8; 32] = decoded
            .try_into()
            .map_err(|_| LanHostConfigError("LAN token must contain exactly 32 bytes"))?;
        Ok(Self(bytes))
    }

    fn matches(&self, candidate: &str) -> bool {
        let Ok(candidate) = Self::parse(candidate) else {
            return false;
        };
        bool::from(self.0.ct_eq(&candidate.0))
    }
}

fn required_lan_access_token(value: Option<&str>) -> Result<LanAccessToken, LanHostConfigError> {
    LanAccessToken::parse(value.ok_or(LanHostConfigError("LAN token is required"))?)
}

#[derive(Clone)]
struct LanHostAccess {
    management_token: LanAccessToken,
}

impl std::fmt::Debug for LanHostAccess {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("LanHostAccess(<redacted>)")
    }
}

#[derive(Debug, Clone)]
pub struct LanHostConfig {
    pub port: u16,
    pub dist_dir: PathBuf,
    access: LanHostAccess,
}

type ConnectionHandler = Arc<dyn Fn(TcpStream) -> Result<(), String> + Send + Sync>;
type ShutdownCheck = Arc<dyn Fn() -> bool + Send + Sync>;

#[derive(Clone, Copy)]
struct ResponseWritePolicy<'a> {
    shutdown_check: Option<&'a ShutdownCheck>,
    deadline: Instant,
}

impl<'a> ResponseWritePolicy<'a> {
    fn connection(shutdown_check: &'a ShutdownCheck) -> Self {
        Self {
            shutdown_check: Some(shutdown_check),
            deadline: Instant::now() + CONNECTION_TIMEOUT,
        }
    }

    fn bounded(timeout: Duration) -> Self {
        Self {
            shutdown_check: None,
            deadline: Instant::now() + timeout,
        }
    }
}

#[derive(Clone, Copy)]
struct ResponseSpec<'a> {
    status: u16,
    content_type: &'a str,
    body: &'a [u8],
    content_length: u64,
}

impl<'a> ResponseSpec<'a> {
    fn new(status: u16, content_type: &'a str, body: &'a [u8]) -> Self {
        Self {
            status,
            content_type,
            body,
            content_length: body.len() as u64,
        }
    }

    fn head(status: u16, content_type: &'a str, content_length: u64) -> Self {
        Self {
            status,
            content_type,
            body: &[],
            content_length,
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct LanHostRunSummary {
    accepted_connections: usize,
    rejected_connections: usize,
}

pub fn is_lan_host_requested() -> bool {
    env::args().any(|arg| arg == "--lan-host")
}

pub fn config_from_env() -> Result<LanHostConfig, String> {
    let port = env::var("LANGAME_LAN_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(DEFAULT_LAN_PORT);
    let dist_dir = resolve_dist_dir()?;
    let token_value = env::var("LANGAME_LAN_TOKEN").ok();
    let management_token =
        required_lan_access_token(token_value.as_deref()).map_err(|error| error.to_string())?;
    let access = LanHostAccess { management_token };

    Ok(LanHostConfig {
        port,
        dist_dir,
        access,
    })
}

pub fn spawn_lan_host(app_handle: tauri::AppHandle, config: LanHostConfig) -> Result<(), String> {
    let dist_dir = config
        .dist_dir
        .canonicalize()
        .map_err(|error| format!("failed to resolve LAN dist dir: {error}"))?;
    let index_path = dist_dir.join("index.html");
    if !index_path.is_file() {
        return Err(format!("LAN frontend is missing: {}", index_path.display()));
    }

    let listener = TcpListener::bind(("0.0.0.0", config.port))
        .map_err(|error| format!("failed to bind LAN host port {}: {error}", config.port))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("failed to configure LAN host listener: {error}"))?;
    let access = Arc::new(config.access);
    let dist_dir = Arc::new(dist_dir);
    let state_handle = app_handle.clone();

    let host_thread = thread::Builder::new()
        .name(String::from("langame-lan-host"))
        .spawn(move || {
            if let Err(error) = run_lan_host(listener, app_handle, dist_dir, access) {
                eprintln!("LanGame LAN host stopped after an error: {error}");
            }
        })
        .map_err(|error| format!("failed to spawn LAN host thread: {error}"))?;

    let state = state_handle.state::<DesktopState>();
    if let Err((register_error, host_thread)) = state.register_lan_host_thread(host_thread) {
        state.shutdown_in_progress.store(true, Ordering::SeqCst);
        return match host_thread.join() {
            Ok(()) => Err(register_error),
            Err(_) => Err(format!(
                "{register_error}; LAN host thread panicked during cleanup"
            )),
        };
    }

    Ok(())
}

fn run_lan_host(
    listener: TcpListener,
    app_handle: tauri::AppHandle,
    dist_dir: Arc<PathBuf>,
    access: Arc<LanHostAccess>,
) -> Result<LanHostRunSummary, String> {
    let shutdown_handle = app_handle.clone();
    let shutdown_check: ShutdownCheck = Arc::new(move || {
        shutdown_handle
            .state::<DesktopState>()
            .shutdown_in_progress
            .load(Ordering::SeqCst)
    });
    let handler_shutdown_check = Arc::clone(&shutdown_check);
    let handler: ConnectionHandler = Arc::new(move |stream| {
        handle_connection(
            stream,
            app_handle.clone(),
            Arc::clone(&dist_dir),
            Arc::clone(&access),
            Arc::clone(&handler_shutdown_check),
        )
    });

    run_lan_listener(
        listener,
        handler,
        shutdown_check,
        LAN_HOST_WORKER_COUNT,
        LAN_HOST_QUEUE_CAPACITY,
        LAN_HOST_POLL_INTERVAL,
    )
}

fn run_lan_listener(
    listener: TcpListener,
    handler: ConnectionHandler,
    shutdown_check: ShutdownCheck,
    worker_count: usize,
    queue_capacity: usize,
    poll_interval: Duration,
) -> Result<LanHostRunSummary, String> {
    if worker_count == 0 {
        return Err(String::from("LAN host worker count must be positive"));
    }
    if queue_capacity == 0 {
        return Err(String::from("LAN host queue capacity must be positive"));
    }
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("failed to configure LAN host listener: {error}"))?;

    let (sender, receiver) = sync_channel(queue_capacity);
    let receiver = Arc::new(Mutex::new(receiver));
    let mut workers = Vec::with_capacity(worker_count);
    for worker_index in 0..worker_count {
        let receiver = Arc::clone(&receiver);
        let handler = Arc::clone(&handler);
        let shutdown_check = Arc::clone(&shutdown_check);
        match thread::Builder::new()
            .name(format!("langame-lan-worker-{worker_index}"))
            .spawn(move || run_lan_worker(receiver, handler, shutdown_check))
        {
            Ok(worker) => workers.push(worker),
            Err(error) => {
                drop(sender);
                let join_result = join_lan_workers(workers);
                return Err(match join_result {
                    Ok(()) => format!("failed to spawn LAN host worker {worker_index}: {error}"),
                    Err(join_error) => format!(
                        "failed to spawn LAN host worker {worker_index}: {error}; {join_error}"
                    ),
                });
            }
        }
    }

    let mut summary = LanHostRunSummary::default();
    let mut listener_error = None;
    while !(shutdown_check)() {
        match listener.accept() {
            Ok((stream, peer_address)) => {
                summary.accepted_connections += 1;
                match sender.try_send(stream) {
                    Ok(()) => {}
                    Err(TrySendError::Full(stream)) => {
                        summary.rejected_connections += 1;
                        eprintln!(
                            "LanGame LAN host rejected {peer_address}: request queue is full"
                        );
                        if let Err(error) = reject_overloaded_connection(stream) {
                            eprintln!(
                                "LanGame LAN host failed to close rejected connection from {peer_address}: {error}"
                            );
                        }
                    }
                    Err(TrySendError::Disconnected(stream)) => {
                        if let Err(error) = close_connection(stream) {
                            eprintln!(
                                "LanGame LAN host failed to close connection after worker shutdown: {error}"
                            );
                        }
                        listener_error = Some(String::from(
                            "LAN host request queue disconnected while accepting a connection",
                        ));
                        break;
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(poll_interval);
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => {
                listener_error = Some(format!("LAN host accept failed: {error}"));
                break;
            }
        }
    }

    drop(sender);
    let join_result = join_lan_workers(workers);
    match (listener_error, join_result) {
        (None, Ok(())) => Ok(summary),
        (Some(error), Ok(())) => Err(error),
        (None, Err(error)) => Err(error),
        (Some(listener_error), Err(join_error)) => Err(format!("{listener_error}; {join_error}")),
    }
}

fn run_lan_worker(
    receiver: Arc<Mutex<Receiver<TcpStream>>>,
    handler: ConnectionHandler,
    shutdown_check: ShutdownCheck,
) -> Result<(), String> {
    loop {
        let stream = receiver
            .lock()
            .map_err(|_| String::from("LAN host request queue lock is poisoned"))?
            .recv();
        let Ok(stream) = stream else {
            return Ok(());
        };
        if (shutdown_check)() {
            close_connection(stream)?;
            return Ok(());
        }
        if let Err(error) = handler(stream) {
            eprintln!("LanGame LAN request failed: {error}");
        }
    }
}

fn join_lan_workers(workers: Vec<thread::JoinHandle<Result<(), String>>>) -> Result<(), String> {
    let mut errors = Vec::new();
    for (worker_index, worker) in workers.into_iter().enumerate() {
        match worker.join() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                errors.push(format!("LAN host worker {worker_index} failed: {error}"))
            }
            Err(_) => errors.push(format!("LAN host worker {worker_index} panicked")),
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn reject_overloaded_connection(mut stream: TcpStream) -> Result<(), String> {
    stream
        .set_write_timeout(Some(OVERLOAD_RESPONSE_TIMEOUT))
        .map_err(|error| format!("failed to set overload response timeout: {error}"))?;
    let response_result = write_text_error(
        &mut stream,
        503,
        "LAN host is busy; retry shortly",
        ResponseWritePolicy::bounded(OVERLOAD_RESPONSE_TIMEOUT),
    );
    let close_result = close_connection(stream);
    response_result.and(close_result)
}

fn close_connection(stream: TcpStream) -> Result<(), String> {
    stream
        .shutdown(Shutdown::Both)
        .map_err(|error| format!("failed to shut down LAN connection: {error}"))
}

fn handle_connection(
    mut stream: TcpStream,
    app_handle: tauri::AppHandle,
    dist_dir: Arc<PathBuf>,
    access: Arc<LanHostAccess>,
    shutdown_check: ShutdownCheck,
) -> Result<(), String> {
    stream
        .set_read_timeout(Some(CONNECTION_READ_POLL_INTERVAL))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(CONNECTION_READ_POLL_INTERVAL))
        .map_err(|error| error.to_string())?;

    let request_deadline = Instant::now() + CONNECTION_TIMEOUT;
    let request = read_request_head(&mut stream, &shutdown_check, request_deadline)?;
    if request
        .path
        .starts_with(crate::media_cache::LAN_MEDIA_PREFIX)
    {
        return media::dispatch(stream, app_handle, request, shutdown_check);
    }
    if request.path.split('?').next() == Some("/__langame/api") {
        return handle_api_request(
            &mut stream,
            app_handle,
            request,
            access,
            &shutdown_check,
            request_deadline,
        );
    }

    if request.method != "GET" && request.method != "HEAD" {
        write_text_error(
            &mut stream,
            405,
            "method not allowed",
            ResponseWritePolicy::connection(&shutdown_check),
        )?;
        return Ok(());
    }

    serve_static_file(
        &mut stream,
        &dist_dir,
        &request.path,
        request.method == "HEAD",
        &shutdown_check,
    )
}

fn handle_api_request(
    stream: &mut TcpStream,
    app_handle: tauri::AppHandle,
    request: HttpRequestHead,
    access: Arc<LanHostAccess>,
    shutdown_check: &ShutdownCheck,
    request_deadline: Instant,
) -> Result<(), String> {
    let Some(content_length) = preflight_api_request(stream, &request, &access, shutdown_check)?
    else {
        return Ok(());
    };

    let body = read_request_body(
        stream,
        &request.buffered_body,
        content_length,
        shutdown_check,
        request_deadline,
    )?;
    if shutdown_check() {
        return Err(String::from("LAN host is shutting down"));
    }
    let payload = serde_json::from_slice::<Value>(&body)
        .map_err(|error| format!("invalid LAN API JSON payload: {error}"))?;
    let command = payload
        .get("command")
        .and_then(Value::as_str)
        .ok_or_else(|| String::from("LAN API payload is missing command"))?;
    let args = payload.get("args").cloned().unwrap_or_else(|| json!({}));
    let result = tauri::async_runtime::block_on(dispatch_command(&app_handle, command, args));

    match result {
        Ok(value) => write_json_response(
            stream,
            200,
            json!({ "ok": true, "value": value }),
            ResponseWritePolicy::connection(shutdown_check),
        ),
        Err(error) => write_json_response(
            stream,
            400,
            json!({ "ok": false, "error": error }),
            ResponseWritePolicy::connection(shutdown_check),
        ),
    }
}

fn preflight_api_request(
    stream: &mut TcpStream,
    request: &HttpRequestHead,
    access: &LanHostAccess,
    shutdown_check: &ShutdownCheck,
) -> Result<Option<usize>, String> {
    if request.method != "POST" {
        write_json_response(
            stream,
            405,
            json!({ "ok": false, "error": "method not allowed" }),
            ResponseWritePolicy::connection(shutdown_check),
        )?;
        return Ok(None);
    }
    if !api_request_is_authorized(access, request) {
        write_json_response(
            stream,
            401,
            json!({ "ok": false, "error": "missing or invalid LAN access token" }),
            ResponseWritePolicy::connection(shutdown_check),
        )?;
        return Ok(None);
    }

    let content_length = match request_content_length(request) {
        Ok(content_length) => content_length,
        Err(error) => {
            write_json_response(
                stream,
                400,
                json!({ "ok": false, "error": error }),
                ResponseWritePolicy::connection(shutdown_check),
            )?;
            return Ok(None);
        }
    };
    if content_length > MAX_REQUEST_BODY_BYTES {
        write_json_response(
            stream,
            413,
            json!({ "ok": false, "error": "request body is too large" }),
            ResponseWritePolicy::connection(shutdown_check),
        )?;
        return Ok(None);
    }
    Ok(Some(content_length))
}

type LanCommandFuture<'a> = Pin<Box<dyn Future<Output = Result<Value, String>> + 'a>>;

// Keep command construction outside the route match. One async match combines
// every command's temporary poll frames and can overflow a LAN worker's stack.
#[inline(never)]
fn boxed_command<'a, F, Fut>(build: F) -> LanCommandFuture<'a>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<Value, String>> + 'a,
{
    Box::pin(build())
}

pub(crate) fn dispatch_command<'a>(
    app_handle: &'a tauri::AppHandle,
    command: &'a str,
    args: Value,
) -> LanCommandFuture<'a> {
    match command {
        "read_ark_cluster" => boxed_command(|| async move {
            to_json(
                commands::commands_ark_clusters::read_ark_cluster(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "input", "input")?,
                )
                .await?,
            )
        }),
        "operate_ark_cluster" => boxed_command(|| async move {
            to_json(
                commands::commands_ark_clusters::operate_ark_cluster(
                    app_handle.clone(),
                    app_handle.state::<DesktopState>(),
                    arg(&args, "input", "input")?,
                )
                .await?,
            )
        }),
        "app_version" => boxed_command(|| async move { to_json(commands::app_version()) }),
        "bootstrap" => boxed_command(|| async move {
            to_json(
                commands::bootstrap(
                    app_handle.clone(),
                    app_handle.state::<DesktopState>(),
                    opt_arg(&args, "includeSystemSnapshot", "include_system_snapshot")?,
                )
                .await?,
            )
        }),
        "read_background_jobs" => boxed_command(|| async move {
            to_json(commands::read_background_jobs(
                app_handle.state::<DesktopState>(),
            )?)
        }),
        "refresh_modules" => boxed_command(|| async move {
            to_json(
                commands::refresh_modules(
                    app_handle.state::<DesktopState>(),
                    opt_arg(
                        &args,
                        "includePreservedProgramCounts",
                        "include_preserved_program_counts",
                    )?,
                )
                .await?,
            )
        }),
        "read_module_details" => boxed_command(|| async move {
            to_json(
                commands::read_module_details(
                    arg(&args, "moduleId", "module_id")?,
                    opt_arg(
                        &args,
                        "includePreservedProgramCounts",
                        "include_preserved_program_counts",
                    )?,
                )
                .await?,
            )
        }),
        "read_module_configuration_icons" => boxed_command(|| async move {
            to_json(
                commands::read_module_configuration_icons(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "moduleId", "module_id")?,
                )
                .await?,
            )
        }),
        "lookup_steam_workshop_items" => boxed_command(|| async move {
            to_json(
                commands::lookup_steam_workshop_items(
                    arg(&args, "ids", "ids")?,
                    opt_arg(&args, "locale", "locale")?,
                )
                .await?,
            )
        }),
        "read_steam_workshop_item_details" => boxed_command(|| async move {
            to_json(
                commands::read_steam_workshop_item_details(
                    arg(&args, "id", "id")?,
                    opt_arg(&args, "locale", "locale")?,
                )
                .await?,
            )
        }),
        "register_media_cache_source" => boxed_command(|| async move {
            to_json(crate::media_cache::register_media_cache_source(
                app_handle.state::<crate::media_cache::MediaCacheState>(),
                arg(&args, "url", "url")?,
                arg(&args, "kind", "kind")?,
                opt_arg(&args, "locale", "locale")?,
            )?)
        }),
        "search_steam_workshop_items" => boxed_command(|| async move {
            to_json(
                commands::search_steam_workshop_items(
                    arg(&args, "appId", "app_id")?,
                    opt_arg(&args, "query", "query")?,
                    opt_arg(&args, "sort", "sort")?,
                    opt_arg(&args, "page", "page")?,
                    opt_arg(&args, "locale", "locale")?,
                    opt_arg(&args, "browseKind", "browse_kind")?,
                )
                .await?,
            )
        }),
        "read_dontstarve_mod_configuration_specs" => boxed_command(|| async move {
            to_json(
                commands::read_dontstarve_mod_configuration_specs(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "instanceId", "instance_id")?,
                    arg(&args, "ids", "ids")?,
                    opt_arg(&args, "locale", "locale")?,
                )
                .await?,
            )
        }),
        "read_project_zomboid_workshop_mods_snapshot" => boxed_command(|| async move {
            to_json(
                commands::read_project_zomboid_workshop_mods_snapshot(
                    arg(&args, "instanceId", "instance_id")?,
                    arg(&args, "ids", "ids")?,
                )
                .await?,
            )
        }),
        "fetch_steam_news_for_app" => boxed_command(|| async move {
            to_json(
                commands::fetch_steam_news_for_app(
                    arg(&args, "appId", "app_id")?,
                    opt_arg(&args, "count", "count")?,
                    opt_arg(&args, "locale", "locale")?,
                )
                .await?,
            )
        }),
        "fetch_steam_store_about" => boxed_command(|| async move {
            to_json(
                commands::fetch_steam_store_about(
                    arg(&args, "appId", "app_id")?,
                    opt_arg(&args, "locale", "locale")?,
                )
                .await?,
            )
        }),
        "fetch_steam_review_summary" => boxed_command(|| async move {
            to_json(
                commands::fetch_steam_review_summary(
                    arg(&args, "appId", "app_id")?,
                    opt_arg(&args, "locale", "locale")?,
                )
                .await?,
            )
        }),
        "open_external_url" => boxed_command(|| async move {
            to_json(commands::open_external_url(arg(&args, "url", "url")?).await?)
        }),
        "open_local_path" => boxed_command(|| async move {
            to_json(commands::open_local_path(arg(&args, "path", "path")?)?)
        }),
        "probe_steamcmd_status" => {
            boxed_command(|| async move { to_json(commands::probe_steamcmd_status()?) })
        }
        "ensure_steamcmd_ready" => boxed_command(|| async move {
            to_json(
                commands::ensure_steamcmd_ready(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "operationId", "operation_id")?,
                )
                .await?,
            )
        }),
        "read_steamcmd_prepare_progress" => boxed_command(|| async move {
            to_json(commands::read_steamcmd_prepare_progress(
                app_handle.state::<DesktopState>(),
                arg(&args, "operationId", "operation_id")?,
            )?)
        }),
        "cancel_steamcmd_preparation" => boxed_command(|| async move {
            to_json(commands::cancel_steamcmd_preparation(
                app_handle.state::<DesktopState>(),
                arg(&args, "operationId", "operation_id")?,
            )?)
        }),
        "cancel_installation_job" => boxed_command(|| async move {
            to_json(commands::cancel_installation_job(
                app_handle.state::<DesktopState>(),
                arg(&args, "jobId", "job_id")?,
            )?)
        }),
        "update_app_settings" => boxed_command(|| async move {
            to_json(
                commands::update_app_settings(
                    app_handle.state::<DesktopState>(),
                    arg::<AppPathSettingsInput>(&args, "input", "input")?,
                )
                .await?,
            )
        }),
        "pick_directory_path" => boxed_command(|| async move {
            to_json(commands::pick_directory_path(opt_arg(
                &args,
                "currentPath",
                "current_path",
            )?)?)
        }),
        "install_module_game" => boxed_command(|| async move {
            to_json(
                commands::install_module_game(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "moduleId", "module_id")?,
                )
                .await?,
            )
        }),
        "validate_module_game" => boxed_command(|| async move {
            to_json(
                commands::validate_module_game(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "moduleId", "module_id")?,
                )
                .await?,
            )
        }),
        "update_instance_program" => boxed_command(|| async move {
            to_json(
                commands::update_instance_program(
                    app_handle.clone(),
                    arg(&args, "instanceId", "instance_id")?,
                    arg(&args, "validate", "validate")?,
                )
                .await?,
            )
        }),
        "uninstall_module_game" => boxed_command(|| async move {
            to_json(
                commands::uninstall_module_game(
                    app_handle.clone(),
                    arg(&args, "moduleId", "module_id")?,
                )
                .await?,
            )
        }),
        "download_steam_workshop_items" => boxed_command(|| async move {
            to_json(
                commands::download_steam_workshop_items(
                    app_handle.clone(),
                    arg(&args, "instanceId", "instance_id")?,
                    arg(&args, "ids", "ids")?,
                    opt_arg(&args, "missingOnly", "missing_only")?,
                    opt_arg(&args, "locale", "locale")?,
                )
                .await?,
            )
        }),
        "read_steam_workshop_installation_status" => boxed_command(|| async move {
            to_json(
                commands::read_steam_workshop_installation_status(
                    arg(&args, "instanceId", "instance_id")?,
                    arg(&args, "ids", "ids")?,
                )
                .await?,
            )
        }),
        "stage_manual_mod_files" => boxed_command(|| async move {
            to_json(
                commands::stage_manual_mod_files(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "instanceId", "instance_id")?,
                    arg(&args, "sourcePaths", "source_paths")?,
                )
                .await?,
            )
        }),
        "install_manual_mod_references" => boxed_command(|| async move {
            to_json(
                commands::install_manual_mod_references(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "instanceId", "instance_id")?,
                    arg(&args, "references", "references")?,
                )
                .await?,
            )
        }),
        "read_manual_mod_inventory" => boxed_command(|| async move {
            to_json(
                commands::read_manual_mod_inventory(arg(&args, "instanceId", "instance_id")?)
                    .await?,
            )
        }),
        "resolve_manual_mod_references" => boxed_command(|| async move {
            to_json(
                commands::resolve_manual_mod_references(
                    arg(&args, "instanceId", "instance_id")?,
                    arg(&args, "references", "references")?,
                )
                .await?,
            )
        }),
        "preview_instance_launch" => boxed_command(|| async move {
            to_json(
                commands::preview_instance_launch(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "instanceId", "instance_id")?,
                )
                .await?,
            )
        }),
        "start_instance_process" => boxed_command(|| async move {
            to_json(
                commands::start_instance_process(
                    app_handle.clone(),
                    app_handle.state::<DesktopState>(),
                    arg(&args, "instanceId", "instance_id")?,
                    opt_arg(&args, "expectedWorldStart", "expected_world_start")?,
                )
                .await?,
            )
        }),
        "stop_instance_process" => boxed_command(|| async move {
            to_json(
                commands::stop_instance_process(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "instanceId", "instance_id")?,
                )
                .await?,
            )
        }),
        "read_instance_live_players" => boxed_command(|| async move {
            to_json(
                commands::read_instance_live_players(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "instanceId", "instance_id")?,
                )
                .await?,
            )
        }),
        "refresh_instance_live_players" => boxed_command(|| async move {
            to_json(
                commands::refresh_instance_live_players(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "instanceId", "instance_id")?,
                )
                .await?,
            )
        }),
        "execute_instance_player_action" => boxed_command(|| async move {
            to_json(
                commands::execute_instance_player_action(
                    app_handle.state::<DesktopState>(),
                    arg::<ExecuteInstancePlayerActionInput>(&args, "input", "input")?,
                )
                .await?,
            )
        }),
        "execute_instance_manual_player_action" => boxed_command(|| async move {
            to_json(
                commands::execute_instance_manual_player_action(
                    app_handle.state::<DesktopState>(),
                    arg::<ExecuteInstanceManualPlayerActionInput>(&args, "input", "input")?,
                )
                .await?,
            )
        }),
        "send_instance_runtime_command" => boxed_command(|| async move {
            to_json(
                commands::send_instance_runtime_command(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "input", "input")?,
                )
                .await?,
            )
        }),
        "send_instance_gm_command" => boxed_command(|| async move {
            to_json(
                commands::send_instance_gm_command(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "input", "input")?,
                )
                .await?,
            )
        }),
        "ensure_storage_ready" => boxed_command(|| async move {
            to_json(commands::ensure_storage_ready(app_handle.state::<DesktopState>()).await?)
        }),
        "sync_modules_to_storage" => boxed_command(|| async move {
            to_json(commands::sync_modules_to_storage(app_handle.state::<DesktopState>()).await?)
        }),
        "list_instances_from_storage" => boxed_command(|| async move {
            to_json(
                commands::list_instances_from_storage(app_handle.state::<DesktopState>()).await?,
            )
        }),
        "read_instance_connection_info_from_storage" => boxed_command(|| async move {
            to_json(
                commands::commands_instance_network::read_instance_connection_info_from_storage(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "instanceIds", "instance_ids")?,
                )
                .await?,
            )
        }),
        "read_instance_details_from_storage" => boxed_command(|| async move {
            to_json(
                commands::read_instance_details_from_storage(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "instanceId", "instance_id")?,
                )
                .await?,
            )
        }),
        "inspect_module_programs" => boxed_command(|| async move {
            to_json(
                commands::commands_program_inventory::inspect_module_programs(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "input", "input")?,
                )
                .await?,
            )
        }),
        "inspect_instance_removal" => boxed_command(|| async move {
            to_json(
                commands::commands_program_inventory::inspect_instance_removal(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "input", "input")?,
                )
                .await?,
            )
        }),
        "read_instance_isolation" => boxed_command(|| async move {
            to_json(
                commands::read_instance_isolation(
                    app_handle.state::<DesktopState>(),
                    arg::<commands::commands_instance_isolation::ReadInstanceIsolationInput>(
                        &args, "input", "input",
                    )?,
                )
                .await?,
            )
        }),
        "create_instance_backup" => boxed_command(|| async move {
            to_json(
                commands::create_instance_backup(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "instanceId", "instance_id")?,
                )
                .await?,
            )
        }),
        "list_instance_backups" => boxed_command(|| async move {
            to_json(
                commands::list_instance_backups(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "instanceId", "instance_id")?,
                )
                .await?,
            )
        }),
        "rename_instance_backup" => boxed_command(|| async move {
            to_json(
                commands::rename_instance_backup(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "instanceId", "instance_id")?,
                    arg(&args, "backupId", "backup_id")?,
                    opt_arg(&args, "displayName", "display_name")?,
                )
                .await?,
            )
        }),
        "delete_instance_backup" => boxed_command(|| async move {
            to_json(
                commands::delete_instance_backup(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "instanceId", "instance_id")?,
                    arg(&args, "backupId", "backup_id")?,
                )
                .await?,
            )
        }),
        "restore_instance_backup" => boxed_command(|| async move {
            to_json(
                commands::restore_instance_backup(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "instanceId", "instance_id")?,
                    arg(&args, "backupId", "backup_id")?,
                    opt_arg(&args, "locale", "locale")?,
                )
                .await?,
            )
        }),
        "read_instance_runtime_overview_from_storage" => boxed_command(|| async move {
            to_json(
                commands::read_instance_runtime_overview_from_storage(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "instanceId", "instance_id")?,
                )
                .await?,
            )
        }),
        "read_instance_runtime_window_snapshot" => boxed_command(|| async move {
            to_json(
                commands::read_instance_runtime_window_snapshot(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "instanceId", "instance_id")?,
                )
                .await?,
            )
        }),
        "suppress_instance_runtime_windows" => boxed_command(|| async move {
            to_json(
                commands::suppress_instance_runtime_windows(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "instanceId", "instance_id")?,
                )
                .await?,
            )
        }),
        "read_palworld_operator_snapshot" => boxed_command(|| async move {
            to_json(
                commands::read_palworld_operator_snapshot(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "instanceId", "instance_id")?,
                )
                .await?,
            )
        }),
        "read_sevendaystodie_operator_snapshot" => boxed_command(|| async move {
            to_json(
                commands::read_sevendaystodie_operator_snapshot(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "instanceId", "instance_id")?,
                )
                .await?,
            )
        }),
        "read_instance_log_document_from_storage" => boxed_command(|| async move {
            to_json(
                commands::read_instance_log_document_from_storage(
                    app_handle.clone(),
                    app_handle.state::<DesktopState>(),
                    arg(&args, "instanceId", "instance_id")?,
                    opt_arg(&args, "maxLines", "max_lines")?,
                    opt_arg(&args, "runId", "run_id")?,
                    opt_arg(&args, "source", "source")?,
                )
                .await?,
            )
        }),
        "create_instance_record" => boxed_command(|| async move {
            to_json(
                commands::create_instance_record(
                    app_handle.state::<DesktopState>(),
                    arg::<CreateInstanceInput>(&args, "input", "input")?,
                    opt_arg(&args, "programMode", "program_mode")?,
                )
                .await?,
            )
        }),
        "update_instance_record_if_current" => boxed_command(|| async move {
            to_json(
                commands::update_instance_record_if_current(
                    app_handle.state::<DesktopState>(),
                    arg::<UpdateInstanceInput>(&args, "input", "input")?,
                    arg(&args, "expectedSettingsJson", "expected_settings_json")?,
                )
                .await?,
            )
        }),
        "remove_instance_workshop_collection" => boxed_command(|| async move {
            to_json(
                commands::remove_instance_workshop_collection(
                    app_handle.state::<DesktopState>(),
                    arg::<UpdateInstanceInput>(&args, "input", "input")?,
                    arg(&args, "expectedSettingsJson", "expected_settings_json")?,
                    arg(&args, "collectionId", "collection_id")?,
                    arg(&args, "memberIds", "member_ids")?,
                    opt_arg(&args, "retainCollection", "retain_collection")?,
                )
                .await?,
            )
        }),
        "update_instance_autostart" => boxed_command(|| async move {
            to_json(
                commands::commands_autostart::update_instance_autostart(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "instanceId", "instance_id")?,
                    arg(&args, "autostart", "autostart")?,
                )
                .await?,
            )
        }),
        "apply_instance_player_access_mutation" => boxed_command(|| async move {
            to_json(
                commands::apply_instance_player_access_mutation(
                    app_handle.state::<DesktopState>(),
                    arg::<app_storage::ApplyInstancePlayerAccessMutationInput>(
                        &args, "input", "input",
                    )?,
                )
                .await?,
            )
        }),
        "delete_instance_record" => boxed_command(|| async move {
            to_json(
                commands::delete_instance_record(
                    app_handle.clone(),
                    arg(&args, "instanceId", "instance_id")?,
                )
                .await?,
            )
        }),
        "import_dontstarve_world_data" => boxed_command(|| async move {
            to_json(
                commands::import_dontstarve_world_data(
                    app_handle.state::<DesktopState>(),
                    arg(&args, "instanceId", "instance_id")?,
                    arg(&args, "sourcePath", "source_path")?,
                    opt_arg(&args, "locale", "locale")?,
                )
                .await?,
            )
        }),
        "assistant_secret_status" => boxed_command(|| async move {
            to_json(commands::assistant_secret_status(arg::<
                AssistantSecretDescriptor,
            >(
                &args,
                "descriptor",
                "descriptor",
            )?)?)
        }),
        "assistant_store_secret" => boxed_command(|| async move {
            to_json(commands::assistant_store_secret(
                arg::<AssistantSecretDescriptor>(&args, "descriptor", "descriptor")?,
                arg(&args, "apiKey", "api_key")?,
            )?)
        }),
        "assistant_clear_secret" => boxed_command(|| async move {
            to_json(commands::assistant_clear_secret(arg::<
                AssistantSecretDescriptor,
            >(
                &args,
                "descriptor",
                "descriptor",
            )?)?)
        }),
        "assistant_list_ollama_models" => boxed_command(|| async move {
            to_json(
                commands::assistant_list_ollama_models(opt_arg(&args, "baseUrl", "base_url")?)
                    .await?,
            )
        }),
        "assistant_run" => boxed_command(|| async move {
            to_json(
                commands::assistant_run(arg::<AssistantRunInput>(&args, "input", "input")?).await?,
            )
        }),
        "assistant_check_connection" => boxed_command(|| async move {
            to_json(
                crate::commands_assistant_connection::assistant_check_connection(arg(
                    &args, "input", "input",
                )?)
                .await?,
            )
        }),
        "assistant_cancel_connection_check" => boxed_command(|| async move {
            to_json(
                crate::commands_assistant_connection::assistant_cancel_connection_check(arg(
                    &args,
                    "requestId",
                    "request_id",
                )?)?,
            )
        }),
        "assistant_execute_operation" => boxed_command(|| async move {
            to_json(
                commands::assistant_execute_operation(
                    app_handle.clone(),
                    app_handle.state::<DesktopState>(),
                    arg::<AssistantRequestInput>(&args, "input", "input")?,
                )
                .await?,
            )
        }),
        "assistant_create_conversation" => boxed_command(|| async move {
            to_json(
                commands::assistant_create_conversation(
                    app_handle.state::<DesktopState>(),
                    arg::<commands::commands_assistant_ops::AssistantConversationCreateInput>(
                        &args, "input", "input",
                    )?,
                )
                .await?,
            )
        }),
        "assistant_list_conversations" => boxed_command(|| async move {
            to_json(
                commands::assistant_list_conversations(
                    app_handle.state::<DesktopState>(),
                    arg::<commands::commands_assistant_ops::AssistantConversationCreateInput>(
                        &args, "input", "input",
                    )?,
                )
                .await?,
            )
        }),
        "assistant_get_conversation_state" => boxed_command(|| async move {
            to_json(
                commands::assistant_get_conversation_state(
                    app_handle.state::<DesktopState>(),
                    arg::<commands::commands_assistant_ops::AssistantConversationStateInput>(
                        &args, "input", "input",
                    )?,
                )
                .await?,
            )
        }),
        "assistant_resume_conversation" => boxed_command(|| async move {
            to_json(
                commands::assistant_resume_conversation(
                    app_handle.clone(),
                    app_handle.state::<DesktopState>(),
                    arg::<commands::commands_assistant_ops::AssistantResumeConversationInput>(
                        &args, "input", "input",
                    )?,
                )
                .await?,
            )
        }),
        "assistant_cancel_turn" => boxed_command(|| async move {
            to_json(
                commands::assistant_cancel_turn(
                    app_handle.state::<DesktopState>(),
                    arg::<commands::commands_assistant_ops::AssistantConversationControlInput>(
                        &args, "input", "input",
                    )?,
                )
                .await?,
            )
        }),
        "assistant_delete_conversation" => boxed_command(|| async move {
            to_json(
                commands::assistant_delete_conversation(
                    app_handle.state::<DesktopState>(),
                    arg::<commands::commands_assistant_ops::AssistantConversationControlInput>(
                        &args, "input", "input",
                    )?,
                )
                .await?,
            )
        }),
        "assistant_confirm_operation" => boxed_command(|| async move {
            to_json(
                commands::assistant_confirm_operation(
                    app_handle.clone(),
                    app_handle.state::<DesktopState>(),
                    arg::<AssistantConfirmOperationInput>(&args, "input", "input")?,
                )
                .await?,
            )
        }),
        "generate_instance_broadcast" => boxed_command(|| async move {
            to_json(
                commands::generate_instance_broadcast(
                    app_handle.state::<DesktopState>(),
                    arg::<GenerateInstanceBroadcastInput>(&args, "input", "input")?,
                )
                .await?,
            )
        }),
        "send_instance_broadcast" => boxed_command(|| async move {
            to_json(
                commands::send_instance_broadcast(
                    app_handle.state::<DesktopState>(),
                    arg::<SendInstanceBroadcastInput>(&args, "input", "input")?,
                )
                .await?,
            )
        }),
        "read_instance_broadcast_policy" => boxed_command(|| async move {
            to_json(
                commands::read_instance_broadcast_policy(arg(&args, "instanceId", "instance_id")?)
                    .await?,
            )
        }),
        "update_instance_broadcast_policy" => boxed_command(|| async move {
            to_json(
                commands::update_instance_broadcast_policy(
                    app_handle.state::<DesktopState>(),
                    arg::<UpdateInstanceBroadcastPolicyInput>(&args, "input", "input")?,
                )
                .await?,
            )
        }),
        "list_instance_broadcast_events" => boxed_command(|| async move {
            to_json(
                commands::list_instance_broadcast_events(
                    arg(&args, "instanceId", "instance_id")?,
                    opt_arg(&args, "limit", "limit")?,
                )
                .await?,
            )
        }),
        "overlay_families" => {
            boxed_command(|| async move { to_json(commands::overlay_families()) })
        }
        "bind_address_candidates" => boxed_command(|| async move {
            to_json(commands::bind_address_candidates(app_handle.state::<DesktopState>()).await?)
        }),
        "log_frontend_event" => boxed_command(|| async move {
            to_json(
                commands::log_frontend_event(
                    arg(&args, "level", "level")?,
                    arg(&args, "action", "action")?,
                    arg(&args, "message", "message")?,
                    opt_arg(&args, "context", "context")?,
                )
                .await?,
            )
        }),
        _ => boxed_command(|| async move { Err(format!("unknown LanGame command: {command}")) }),
    }
}

fn read_request_head(
    stream: &mut TcpStream,
    shutdown_check: &ShutdownCheck,
    request_deadline: Instant,
) -> Result<HttpRequestHead, String> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 8192];
    let header_end = loop {
        let bytes_read = read_request_chunk(stream, &mut chunk, shutdown_check, request_deadline)?;
        if bytes_read == 0 {
            return Err(String::from(
                "connection closed before request was complete",
            ));
        }
        buffer.extend_from_slice(&chunk[..bytes_read]);
        if let Some(index) = find_header_end(&buffer) {
            if index + 4 > MAX_REQUEST_HEADER_BYTES {
                return Err(String::from("request header is too large"));
            }
            break index;
        }
        if buffer.len() > MAX_REQUEST_HEADER_BYTES {
            return Err(String::from("request header is too large"));
        }
    };

    request::parse_head(&buffer[..header_end], &buffer[header_end + 4..])
}

fn read_request_body(
    stream: &mut TcpStream,
    buffered_body: &[u8],
    content_length: usize,
    shutdown_check: &ShutdownCheck,
    request_deadline: Instant,
) -> Result<Vec<u8>, String> {
    let mut chunk = [0_u8; 8192];
    let mut body = buffered_body[..buffered_body.len().min(content_length)].to_vec();
    while body.len() < content_length {
        let remaining = content_length - body.len();
        let read_len = remaining.min(chunk.len());
        let bytes_read = read_request_chunk(
            stream,
            &mut chunk[..read_len],
            shutdown_check,
            request_deadline,
        )?;
        if bytes_read == 0 {
            return Err(String::from(
                "connection closed before request body was complete",
            ));
        }
        body.extend_from_slice(&chunk[..bytes_read]);
    }
    body.truncate(content_length);
    Ok(body)
}

fn read_request_chunk(
    stream: &mut TcpStream,
    buffer: &mut [u8],
    shutdown_check: &ShutdownCheck,
    request_deadline: Instant,
) -> Result<usize, String> {
    loop {
        if shutdown_check() {
            return Err(String::from("LAN host is shutting down"));
        }
        if Instant::now() >= request_deadline {
            return Err(String::from("LAN request timed out"));
        }

        match stream.read(buffer) {
            Ok(bytes_read) => return Ok(bytes_read),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) => {}
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn api_request_is_authorized(access: &LanHostAccess, request: &HttpRequestHead) -> bool {
    request
        .headers
        .get("x-langame-token")
        .is_some_and(|candidate| access.management_token.matches(candidate))
}

fn arg<T>(args: &Value, camel_name: &str, snake_name: &str) -> Result<T, String>
where
    T: DeserializeOwned,
{
    let value = args
        .get(camel_name)
        .or_else(|| args.get(snake_name))
        .ok_or_else(|| format!("missing argument `{camel_name}`"))?;
    serde_json::from_value(value.clone())
        .map_err(|error| format!("invalid argument `{camel_name}`: {error}"))
}

fn opt_arg<T>(args: &Value, camel_name: &str, snake_name: &str) -> Result<Option<T>, String>
where
    T: DeserializeOwned,
{
    let Some(value) = args.get(camel_name).or_else(|| args.get(snake_name)) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    serde_json::from_value(value.clone())
        .map(Some)
        .map_err(|error| format!("invalid argument `{camel_name}`: {error}"))
}

fn to_json<T>(value: T) -> Result<Value, String>
where
    T: Serialize,
{
    serde_json::to_value(value).map_err(|error| error.to_string())
}

fn write_json_response(
    stream: &mut TcpStream,
    status: u16,
    body: Value,
    policy: ResponseWritePolicy<'_>,
) -> Result<(), String> {
    let body = serde_json::to_vec(&body).map_err(|error| error.to_string())?;
    write_response(
        stream,
        ResponseSpec::new(status, "application/json; charset=utf-8", &body),
        policy,
    )
}

fn write_text_error(
    stream: &mut TcpStream,
    status: u16,
    body: &str,
    policy: ResponseWritePolicy<'_>,
) -> Result<(), String> {
    write_response(
        stream,
        ResponseSpec::new(status, "text/plain; charset=utf-8", body.as_bytes()),
        policy,
    )
}

fn write_response(
    stream: &mut impl Write,
    response: ResponseSpec<'_>,
    policy: ResponseWritePolicy<'_>,
) -> Result<(), String> {
    let mut headers = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\n",
        response.status,
        status_reason(response.status),
        response.content_type,
        response.content_length
    );
    headers.push_str("Content-Security-Policy: ");
    headers.push_str(LAN_CONTENT_SECURITY_POLICY);
    headers.push_str("\r\nX-Frame-Options: DENY\r\nReferrer-Policy: no-referrer\r\n");
    if response.content_type.starts_with("application/json") {
        headers.push_str("Cache-Control: no-store\r\n");
    }
    headers.push_str("\r\n");
    write_response_bytes(stream, headers.as_bytes(), policy)?;
    write_response_bytes(stream, response.body, policy)?;
    flush_response(stream, policy)
}

fn write_response_bytes(
    stream: &mut impl Write,
    mut bytes: &[u8],
    policy: ResponseWritePolicy<'_>,
) -> Result<(), String> {
    while !bytes.is_empty() {
        ensure_response_write_active(policy)?;
        match stream.write(bytes) {
            Ok(0) => return Err(String::from("connection closed while writing LAN response")),
            Ok(written) => bytes = &bytes[written..],
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) => {}
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

fn flush_response(stream: &mut impl Write, policy: ResponseWritePolicy<'_>) -> Result<(), String> {
    loop {
        ensure_response_write_active(policy)?;
        match stream.flush() {
            Ok(()) => return Ok(()),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) => {}
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn ensure_response_write_active(policy: ResponseWritePolicy<'_>) -> Result<(), String> {
    if policy.shutdown_check.is_some_and(|check| check()) {
        return Err(String::from("LAN host is shutting down"));
    }
    if Instant::now() >= policy.deadline {
        return Err(String::from("LAN response timed out"));
    }
    Ok(())
}

fn status_reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "OK",
    }
}

#[cfg(test)]
#[path = "lan_host_tests.rs"]
mod tests;

fn resolve_dist_dir() -> Result<PathBuf, String> {
    if let Ok(value) = env::var("LANGAME_LAN_DIST_DIR") {
        let path = PathBuf::from(value);
        if path.join("index.html").is_file() {
            return Ok(path);
        }
    }

    let mut candidates = Vec::new();
    if let Ok(current_dir) = env::current_dir() {
        candidates.push(current_dir.join("apps").join("desktop").join("dist"));
        candidates.push(current_dir.join("dist"));
    }
    if let Ok(exe_path) = env::current_exe()
        && let Some(exe_dir) = exe_path.parent()
    {
        candidates.push(exe_dir.join("dist"));
        if let Some(target_dir) = exe_dir.parent()
            && let Some(root_dir) = target_dir.parent()
        {
            candidates.push(root_dir.join("apps").join("desktop").join("dist"));
        }
    }

    candidates
        .into_iter()
        .find(|path| path.join("index.html").is_file())
        .ok_or_else(|| {
            String::from(
                "could not find apps\\desktop\\dist\\index.html; build the desktop frontend first",
            )
        })
}
