//! Thin desktop/LAN adapters for the public media cache. No game or account data
//! is served here; the network crate validates every upstream resource.
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use app_network::SourcePreference;
use app_network::media_cache::{MediaCache, MediaKind, MediaResponse, media_cache_identity};
use serde::Deserialize;
use tauri::Manager;
use tauri::http::{HeaderName, HeaderValue, Request, Response, StatusCode};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[path = "media_cache_leases.rs"]
mod leases;

pub const LAN_MEDIA_PREFIX: &str = "/__langame/media/";
const MAX_MEDIA_OPERATIONS: usize = 8;
const MAX_URL_BYTES: usize = 8192;
const REQUEST_DEADLINE: Duration = Duration::from_secs(25);
pub type ShutdownCheck = Arc<dyn Fn() -> bool + Send + Sync>;

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MediaPurpose {
    Image,
    Video,
    Hls,
}

impl MediaPurpose {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "image" => Some(Self::Image),
            "video" => Some(Self::Video),
            "hls" => Some(Self::Hls),
            _ => None,
        }
    }

    fn kind(self) -> MediaKind {
        match self {
            Self::Image => MediaKind::Image,
            Self::Video => MediaKind::Video,
            Self::Hls => MediaKind::Hls,
        }
    }
}

#[derive(Clone)]
pub struct MediaSource {
    pub url: String,
    pub purpose: MediaPurpose,
    pub preference: SourcePreference,
    // Playback identity survives disk eviction. The opaque lease owns its lifetime.
    playback: Option<PlaybackGeneration>,
}

#[derive(Clone, Default)]
struct PlaybackGeneration(Arc<Mutex<Option<Representation>>>);

#[derive(PartialEq, Eq)]
struct Representation {
    source: String,
    etag: String,
    total: u64,
}

impl PlaybackGeneration {
    fn accepts(&self, response: &MediaResponse) -> bool {
        if !matches!(response.status, 200 | 206) {
            return true;
        }
        let header = |name: &str| {
            response
                .headers
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.as_str())
        };
        let Some(etag) =
            header("etag").filter(|value| value.starts_with('"') && value.ends_with('"'))
        else {
            return false;
        };
        let total = if response.status == 206 {
            header("content-range")
                .and_then(|value| value.rsplit_once('/'))
                .map(|(_, total)| total)
        } else {
            header("content-length")
        }
        .and_then(|value| value.parse::<u64>().ok());
        let Some(total) = total else {
            return false;
        };
        let representation = Representation {
            source: response.final_url.clone(),
            etag: etag.to_string(),
            total,
        };
        let Ok(mut previous) = self.0.lock() else {
            return false;
        };
        if let Some(previous) = previous.as_ref() {
            *previous == representation
        } else {
            *previous = Some(representation);
            true
        }
    }
}

impl MediaSource {
    fn identity(&self) -> Option<String> {
        if self.url.len() > MAX_URL_BYTES {
            return None;
        }
        media_cache_identity(&self.url, self.purpose.kind())
    }
}

#[derive(Clone)]
pub struct MediaCacheState {
    cache: Arc<MediaCache>,
    leases: Arc<leases::MediaLeases>,
    slots: Arc<Semaphore>,
}

impl MediaCacheState {
    pub fn new(root: PathBuf) -> Self {
        Self {
            cache: Arc::new(MediaCache::new(root)),
            leases: Arc::new(leases::MediaLeases::default()),
            slots: Arc::new(Semaphore::new(MAX_MEDIA_OPERATIONS)),
        }
    }

    pub fn admit(&self) -> Option<OwnedSemaphorePermit> {
        self.slots.clone().try_acquire_owned().ok()
    }

    pub fn lookup(&self, path: &str) -> Option<MediaSource> {
        self.leases.lookup(path)
    }

    pub async fn respond(
        &self,
        source: MediaSource,
        range: Option<String>,
        head: bool,
        shutdown: ShutdownCheck,
    ) -> Response<Vec<u8>> {
        if source.identity().is_none() || range.as_ref().is_some_and(|value| value.len() > 128) {
            return error_response(400, "Invalid media request");
        }
        let request = self.cache.respond(
            &source.url,
            source.purpose.kind(),
            source.preference,
            range.as_deref(),
            head,
        );
        let mut response = tokio::select! {
            result = request => match result {
                Ok(result) => {
                    if source.playback.as_ref().is_some_and(|playback| !playback.accepts(&result)) {
                        error_response(409, "Media changed; restart playback")
                    } else { cached_response(result, head) }
                },
                Err(error) => {
                    eprintln!("Public media cache request failed: {error}");
                    let mut response = error_response(error.status_code(), "Media unavailable");
                    if let Some(value) = error.retry_after().and_then(|value| HeaderValue::from_str(value).ok()) {
                        response.headers_mut().insert("retry-after", value);
                    }
                    response
                }
            },
            _ = tokio::time::sleep(REQUEST_DEADLINE) => error_response(504, "Media request timed out"),
            _ = wait_for_shutdown(shutdown) => error_response(503, "Application is closing"),
        };
        if head {
            response.body_mut().clear();
        }
        response
    }
}

async fn wait_for_shutdown(shutdown: ShutdownCheck) {
    while !shutdown() {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tauri::command]
pub fn register_media_cache_source(
    state: tauri::State<'_, MediaCacheState>,
    url: String,
    kind: MediaPurpose,
    locale: Option<String>,
) -> Result<Option<String>, String> {
    let source = MediaSource {
        url,
        purpose: kind,
        preference: SourcePreference::from_locale(locale.as_deref()),
        playback: (kind == MediaPurpose::Video).then(PlaybackGeneration::default),
    };
    let Some(identity) = source.identity() else {
        return Ok(None);
    };
    state.leases.register(identity, source).map(Some)
}

pub fn handle_protocol(
    context: tauri::UriSchemeContext<'_, tauri::Wry>,
    request: Request<Vec<u8>>,
    responder: tauri::UriSchemeResponder,
) {
    if request.method() == "OPTIONS" {
        responder.respond(error_response(204, ""));
        return;
    }
    if request.method() != "GET" && request.method() != "HEAD" {
        responder.respond(error_response(405, "Method not allowed"));
        return;
    }
    let app = context.app_handle().clone();
    let Some(state) = app
        .try_state::<MediaCacheState>()
        .map(|state| state.inner().clone())
    else {
        responder.respond(error_response(503, "Media cache is unavailable"));
        return;
    };
    let source = if request.uri().path().starts_with(LAN_MEDIA_PREFIX) {
        state.lookup(
            request
                .uri()
                .path_and_query()
                .map_or("", |value| value.as_str()),
        )
    } else {
        protocol_source(&request.uri().to_string())
    };
    let Some(source) = source else {
        responder.respond(error_response(404, "Media reference is invalid or expired"));
        return;
    };
    let Some(permit) = state.admit() else {
        responder.respond(error_response(503, "Media cache is busy"));
        return;
    };
    let range = match request.headers().get("range").map(|value| value.to_str()) {
        Some(Ok(value)) => Some(value.to_owned()),
        Some(Err(_)) => {
            responder.respond(error_response(400, "Invalid byte range"));
            return;
        }
        None => None,
    };
    let head = request.method() == "HEAD";
    let shutdown = app_shutdown_check(app);
    tauri::async_runtime::spawn(async move {
        let _permit = permit;
        responder.respond(state.respond(source, range, head, shutdown).await);
    });
}

pub fn app_shutdown_check<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> ShutdownCheck {
    Arc::new(move || {
        if let Some(state) = app.try_state::<crate::state::DesktopState>() {
            return state.shutdown_in_progress.load(Ordering::SeqCst);
        }
        #[cfg(windows)]
        return crate::runtime_service::interface_is_closing(&app);
        #[cfg(not(windows))]
        true
    })
}

fn protocol_source(uri: &str) -> Option<MediaSource> {
    if uri.len() > MAX_URL_BYTES * 3 {
        return None;
    }
    let url = reqwest::Url::parse(uri).ok()?;
    let mut source = None;
    let mut purpose = None;
    let mut locale = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "url" if source.is_none() => source = Some(value.into_owned()),
            "kind" if purpose.is_none() => purpose = Some(MediaPurpose::parse(&value)?),
            "locale" if locale.is_none() => locale = Some(value.into_owned()),
            _ => return None,
        }
    }
    let source = MediaSource {
        url: source?,
        purpose: purpose?,
        preference: SourcePreference::from_locale(locale.as_deref()),
        playback: None,
    };
    // Native playback must use a registered lease so eviction cannot reset its validator.
    if source.purpose == MediaPurpose::Video {
        return None;
    }
    source.identity().map(|_| source)
}

fn common_headers(response: &mut Response<Vec<u8>>) {
    for (name, value) in [
        ("access-control-allow-origin", "*"),
        ("access-control-allow-methods", "GET, HEAD, OPTIONS"),
        ("access-control-allow-headers", "Range"),
        (
            "access-control-expose-headers",
            "Content-Length, Content-Range, Accept-Ranges, X-LanGame-Media-Origin, X-LanGame-Media-Cache, Retry-After",
        ),
        ("x-content-type-options", "nosniff"),
        // The disk cache owns freshness; a WebView HTTP hit must not hide invalidation.
        ("cache-control", "no-store"),
    ] {
        response
            .headers_mut()
            .insert(name, HeaderValue::from_static(value));
    }
}

fn cached_response(result: MediaResponse, head: bool) -> Response<Vec<u8>> {
    let mut response = Response::new(if head { Vec::new() } else { result.body });
    *response.status_mut() = StatusCode::from_u16(result.status).unwrap_or(StatusCode::BAD_GATEWAY);
    for (key, value) in result.headers {
        if ![
            "content-type",
            "content-length",
            "content-range",
            "accept-ranges",
            "x-langame-media-cache",
        ]
        .contains(&key.to_ascii_lowercase().as_str())
        {
            continue;
        }
        if let (Ok(key), Ok(value)) = (
            HeaderName::from_bytes(key.as_bytes()),
            HeaderValue::from_str(&value),
        ) {
            response.headers_mut().insert(key, value);
        }
    }
    if let Ok(origin) = HeaderValue::from_str(&result.final_url) {
        response
            .headers_mut()
            .insert("x-langame-media-origin", origin);
    }
    common_headers(&mut response);
    response
}

pub fn error_response(status: u16, message: &str) -> Response<Vec<u8>> {
    let mut response = Response::new(message.as_bytes().to_vec());
    *response.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
    response.headers_mut().insert(
        "content-type",
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    common_headers(&mut response);
    response
}

#[cfg(test)]
#[path = "media_cache_tests.rs"]
mod tests;
