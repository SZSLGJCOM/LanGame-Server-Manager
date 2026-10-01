//! Persistent cache for verified public media. One MediaCache owns its dedicated root.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex as AsyncMutex, OnceCell, Semaphore};
use tokio::time::Instant;

use crate::{SourcePreference, official_url_candidates};

mod full;
mod http;
mod meta;
mod ranges;
mod store;

const OBJECT_LIMIT: usize = 16 * 1024 * 1024;
const BLOCK_SIZE: u64 = 1024 * 1024;

fn request_budget() -> Duration {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct RequestPolicy {
        cache_request_budget_ms: u64,
    }
    static BUDGET: OnceLock<Duration> = OnceLock::new();
    *BUDGET.get_or_init(|| {
        let policy: RequestPolicy =
            serde_json::from_str(include_str!("../media-request-policy.json"))
                .expect("embedded media request policy must be valid");
        Duration::from_millis(policy.cache_request_budget_ms)
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Image,
    Video,
    Hls,
}

pub fn media_cache_identity(url: &str, kind: MediaKind) -> Option<String> {
    crate::policy::media_identity(
        url,
        if kind == MediaKind::Image {
            "image"
        } else {
            "video"
        },
    )
}

#[derive(Debug)]
pub struct MediaResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub final_url: String,
}

#[derive(Debug, thiserror::Error)]
pub enum MediaCacheError {
    #[error("media URL is outside the verified public media policy")]
    Rejected,
    #[error("media cache request queue is full")]
    Busy,
    #[error("media request exceeded its time budget")]
    Timeout,
    #[error("media response exceeds the 16 MiB object limit")]
    TooLarge,
    #[error("the requested media byte range is not satisfiable")]
    Range,
    #[error("media representation changed; restart playback")]
    GenerationChanged,
    #[error("media origin returned HTTP {status}")]
    Http {
        status: u16,
        retry_after: Option<String>,
    },
    #[error("media transport failed: {0}")]
    Network(reqwest::Error),
    #[error("invalid media response: {0}")]
    InvalidResponse(String),
    #[error("media cache storage failed: {0}")]
    Storage(String),
}

impl MediaCacheError {
    pub fn status_code(&self) -> u16 {
        match self {
            Self::Rejected => 403,
            Self::Range => 416,
            Self::TooLarge => 413,
            Self::GenerationChanged => 409,
            Self::Timeout => 504,
            Self::Http { status, .. } => *status,
            Self::InvalidResponse(_) => 502,
            Self::Busy | Self::Network(_) | Self::Storage(_) => 503,
        }
    }
    pub fn retry_after(&self) -> Option<&str> {
        match self {
            Self::Http { retry_after, .. } => retry_after.as_deref(),
            _ => None,
        }
    }
    fn offline(&self) -> bool {
        matches!(self, Self::Network(_) | Self::Timeout)
    }
    fn fallback(&self) -> bool {
        matches!(
            self,
            Self::Network(_) | Self::Timeout | Self::InvalidResponse(_)
        ) || matches!(
            self,
            Self::Http {
                status: 404 | 408 | 500 | 502 | 503 | 504,
                retry_after: None
            }
        )
    }
}

type Resolver =
    dyn Fn(&str, MediaKind, SourcePreference) -> Option<(String, Vec<String>)> + Send + Sync;
type Clock = dyn Fn() -> u64 + Send + Sync;

struct Inner {
    root: PathBuf,
    store: OnceCell<Arc<store::Store>>,
    client: OnceCell<reqwest::Client>,
    locks: Mutex<HashMap<String, Weak<AsyncMutex<()>>>>,
    admitted: Semaphore,
    workers: Semaphore,
    resolve: Arc<Resolver>,
    clock: Arc<Clock>,
    disk_limit: u64,
    entry_limit: usize,
}

#[derive(Clone)]
pub struct MediaCache {
    inner: Arc<Inner>,
}

impl MediaCache {
    pub fn new(root: PathBuf) -> Self {
        Self {
            inner: Arc::new(Inner {
                root,
                store: OnceCell::new(),
                client: OnceCell::new(),
                locks: Mutex::new(HashMap::new()),
                admitted: Semaphore::new(64),
                workers: Semaphore::new(4),
                resolve: Arc::new(|url, kind, preference| {
                    Some((
                        media_cache_identity(url, kind)?,
                        official_url_candidates(url, preference),
                    ))
                }),
                clock: Arc::new(|| {
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                }),
                disk_limit: 2 * 1024 * 1024 * 1024,
                entry_limit: 4096,
            }),
        }
    }

    pub async fn respond(
        &self,
        url: &str,
        kind: MediaKind,
        preference: SourcePreference,
        range: Option<&str>,
        head: bool,
    ) -> Result<MediaResponse, MediaCacheError> {
        let (identity, sources) =
            (self.inner.resolve)(url, kind, preference).ok_or(MediaCacheError::Rejected)?;
        let requested_range = range.map(ranges::RequestedRange::parse).transpose()?;
        if kind == MediaKind::Image && requested_range.is_some() {
            return Err(MediaCacheError::Range);
        }
        let _admitted = self
            .inner
            .admitted
            .try_acquire()
            .map_err(|_| MediaCacheError::Busy)?;
        let deadline = Instant::now() + request_budget();
        let key = store::digest(identity.as_bytes());
        let lock = {
            let mut locks = self
                .inner
                .locks
                .lock()
                .map_err(|_| MediaCacheError::Storage("request lock poisoned".into()))?;
            locks.retain(|_, lock| lock.strong_count() > 0);
            if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
                lock
            } else {
                let lock = Arc::new(AsyncMutex::new(()));
                locks.insert(key.clone(), Arc::downgrade(&lock));
                lock
            }
        };
        tokio::time::timeout_at(deadline, async {
            let _same_resource = lock.lock().await;
            let store = self
                .inner
                .store
                .get_or_try_init(|| async {
                    let root = self.inner.root.clone();
                    let (disk, entries) = (self.inner.disk_limit, self.inner.entry_limit);
                    tokio::task::spawn_blocking(move || store::Store::open(root, disk, entries))
                        .await
                        .map_err(|error| MediaCacheError::Storage(error.to_string()))?
                })
                .await?;
            let client = self
                .inner
                .client
                .get_or_try_init(|| async {
                    reqwest::Client::builder()
                        .redirect(reqwest::redirect::Policy::none())
                        .connect_timeout(Duration::from_secs(8))
                        .user_agent("LanGame Server Manager media cache")
                        .build()
                        .map_err(|error| MediaCacheError::Network(error.without_url()))
                })
                .await?;
            let context = http::Context {
                cache: self,
                store,
                client,
                identity: &identity,
                key: &key,
                kind,
                preference,
                deadline,
            };
            let mut entry = store.get(&key, context.now()).await?;
            if entry
                .as_ref()
                .is_some_and(|entry| !context.allowed(&entry.source))
            {
                store.remove(&key).await?;
                entry = None;
            }
            if let Some(range) = requested_range {
                ranges::respond(&context, &sources, entry, range, head).await
            } else {
                full::respond(&context, &sources, entry, head).await
            }
        })
        .await
        .map_err(|_| MediaCacheError::Timeout)?
    }
}

fn response(
    entry: &meta::Entry,
    body: Vec<u8>,
    span: Option<(u64, u64)>,
    head: bool,
    state: &str,
) -> MediaResponse {
    let length = span
        .map(|(start, end)| end - start + 1)
        .unwrap_or(entry.total);
    let mut headers = vec![
        ("Content-Type".into(), entry.headers.content_type.clone()),
        ("Content-Length".into(), length.to_string()),
        ("Accept-Ranges".into(), "bytes".into()),
        ("X-LanGame-Media-Cache".into(), state.into()),
    ];
    if let Some(value) = &entry.headers.etag {
        headers.push(("ETag".into(), value.clone()));
    }
    if let Some(value) = &entry.headers.last_modified {
        headers.push(("Last-Modified".into(), value.clone()));
    }
    if let Some(value) = &entry.headers.cache_control {
        headers.push(("Cache-Control".into(), value.clone()));
    }
    if state == "stale" {
        headers.push(("Warning".into(), "110 - \"Response is stale\"".into()));
    }
    if let Some((start, end)) = span {
        headers.push((
            "Content-Range".into(),
            format!("bytes {start}-{end}/{}", entry.total),
        ));
    }
    MediaResponse {
        status: if span.is_some() { 206 } else { 200 },
        headers,
        body: if head { Vec::new() } else { body },
        final_url: entry.source.clone(),
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
fn test_cache(
    root: PathBuf,
    resolve: Arc<Resolver>,
    clock: Arc<Clock>,
    disk_limit: u64,
    entry_limit: usize,
) -> MediaCache {
    let mut cache = MediaCache::new(root);
    let inner = Arc::get_mut(&mut cache.inner).expect("new test cache has one owner");
    inner.resolve = resolve;
    inner.clock = clock;
    inner.disk_limit = disk_limit;
    inner.entry_limit = entry_limit;
    inner
        .client
        .set(
            reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("test client"),
        )
        .expect("empty test client cell");
    cache
}

#[cfg(test)]
#[path = "media_cache/browser_tests.rs"]
mod browser_tests;
