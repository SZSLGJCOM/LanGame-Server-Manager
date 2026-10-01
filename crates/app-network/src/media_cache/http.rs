use std::sync::Arc;

use reqwest::{Method, header};
use tokio::time::Instant;

use super::{
    MediaCache, MediaCacheError, MediaKind, OBJECT_LIMIT, SourcePreference, meta::Headers,
    store::Store,
};

pub(super) struct Context<'a> {
    pub cache: &'a MediaCache,
    pub store: &'a Arc<Store>,
    pub client: &'a reqwest::Client,
    pub identity: &'a str,
    pub key: &'a str,
    pub kind: MediaKind,
    pub preference: SourcePreference,
    pub deadline: Instant,
}

pub(super) struct Reply {
    pub url: String,
    pub status: u16,
    pub headers: Headers,
    pub length: Option<u64>,
    pub content_range: Option<String>,
    pub body: Vec<u8>,
}

impl Context<'_> {
    pub fn now(&self) -> u64 {
        (self.cache.inner.clock)()
    }
    pub fn allowed(&self, url: &str) -> bool {
        (self.cache.inner.resolve)(url, self.kind, self.preference)
            .is_some_and(|(identity, _)| identity == self.identity)
    }
    pub fn source_deadline(&self, remaining_sources: usize) -> Instant {
        (Instant::now()
            + self.deadline.saturating_duration_since(Instant::now())
                / u32::try_from(remaining_sources.max(1)).unwrap_or(u32::MAX))
        .min(self.deadline)
    }
    pub async fn fetch(
        &self,
        source: &str,
        range: Option<&str>,
        conditional: Option<(&str, &str)>,
        head: bool,
        pinned: bool,
        deadline: Instant,
    ) -> Result<Reply, MediaCacheError> {
        tokio::time::timeout_at(deadline, async {
            // Upstream concurrency must not block a verified disk-cache hit.
            let _worker = self
                .cache
                .inner
                .workers
                .acquire()
                .await
                .map_err(|_| MediaCacheError::Busy)?;
            self.fetch_inner(source, range, conditional, head, pinned)
                .await
        })
        .await
        .map_err(|_| MediaCacheError::Timeout)?
    }
    async fn fetch_inner(
        &self,
        source: &str,
        range: Option<&str>,
        conditional: Option<(&str, &str)>,
        head: bool,
        pinned: bool,
    ) -> Result<Reply, MediaCacheError> {
        let mut url = reqwest::Url::parse(source).map_err(|_| MediaCacheError::Rejected)?;
        for redirect in 0..=3 {
            if !self.allowed(url.as_str()) {
                return Err(MediaCacheError::Rejected);
            }
            let mut request = self
                .client
                .request(if head { Method::HEAD } else { Method::GET }, url.clone())
                .header(header::ACCEPT_ENCODING, "identity");
            if let Some(range) = range {
                request = request.header(header::RANGE, range);
            }
            if let Some((key, value)) = conditional {
                request = request.header(key, value);
            }
            let mut response = request
                .send()
                .await
                .map_err(|error| MediaCacheError::Network(error.without_url()))?;
            let status = response.status().as_u16();
            if matches!(status, 301 | 302 | 303 | 307 | 308) {
                let target = response
                    .headers()
                    .get(header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| url.join(value).ok())
                    .ok_or_else(|| {
                        MediaCacheError::InvalidResponse("redirect without a valid location".into())
                    })?;
                if !self.allowed(target.as_str()) {
                    return Err(MediaCacheError::Rejected);
                }
                if pinned && target != url {
                    return Err(MediaCacheError::GenerationChanged);
                }
                if redirect == 3 {
                    return Err(MediaCacheError::InvalidResponse(
                        "too many media redirects".into(),
                    ));
                }
                url = target;
                continue;
            }
            if !matches!(status, 200 | 206 | 304 | 416) {
                let retry_after = response
                    .headers()
                    .get(header::RETRY_AFTER)
                    .and_then(|value| value.to_str().ok())
                    .filter(|value| value.len() <= 128)
                    .map(str::to_string);
                return Err(MediaCacheError::Http {
                    status,
                    retry_after,
                });
            }
            let headers = Headers::read(response.headers());
            let length = response.content_length();
            let content_range = response
                .headers()
                .get(header::CONTENT_RANGE)
                .and_then(|value| value.to_str().ok())
                .filter(|value| value.len() <= 128)
                .map(str::to_string);
            let mut body = Vec::new();
            if !head && matches!(status, 200 | 206) {
                if length.is_some_and(|length| length > OBJECT_LIMIT as u64) {
                    return Err(MediaCacheError::TooLarge);
                }
                while let Some(chunk) = response
                    .chunk()
                    .await
                    .map_err(|error| MediaCacheError::Network(error.without_url()))?
                {
                    if chunk.len() > OBJECT_LIMIT.saturating_sub(body.len()) {
                        return Err(MediaCacheError::TooLarge);
                    }
                    body.extend_from_slice(&chunk);
                }
                if length.is_some_and(|length| length != body.len() as u64) {
                    return Err(MediaCacheError::InvalidResponse(
                        "truncated media body".into(),
                    ));
                }
                let content_type = headers.content_type.split(';').next().unwrap_or_default();
                let acceptable = if self.kind == MediaKind::Image {
                    content_type.starts_with("image/")
                } else {
                    content_type.starts_with("video/")
                        || content_type.starts_with("audio/")
                        || matches!(
                            content_type,
                            "application/octet-stream"
                                | "application/vnd.apple.mpegurl"
                                | "application/x-mpegURL"
                                | "application/x-mpegurl"
                                | "binary/octet-stream"
                        )
                };
                if !acceptable {
                    return Err(MediaCacheError::InvalidResponse(
                        "origin did not return media content".into(),
                    ));
                }
            }
            return Ok(Reply {
                url: url.to_string(),
                status,
                headers,
                length,
                content_range,
                body,
            });
        }
        Err(MediaCacheError::InvalidResponse(
            "unreachable redirect state".into(),
        ))
    }
}
