use std::time::Duration;

use reqwest::{Client, Method, Request, Response, StatusCode};
use thiserror::Error;
use tokio::time::{Instant, sleep, timeout_at};

use crate::{SourcePreference, official_url_candidates, record_failure};

mod body;
pub use body::{PublicBytes, read_public_bytes, read_public_bytes_from_source};

#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("public network request could not be cloned for a bounded retry")]
    Unrepeatable,
    #[error("public network request budget expired after {attempts} attempt(s) to {origin}")]
    Deadline { attempts: usize, origin: String },
    #[error("public network request to {origin} failed: {source}")]
    Transport {
        origin: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("official source returned HTTP {status} from {origin}")]
    Status { status: StatusCode, origin: String },
    #[error("official source deferred requests (HTTP {status}) from {origin}; retry later")]
    RetryDeferred {
        status: StatusCode,
        origin: String,
        retry_after: Option<Duration>,
    },
    #[error("public response from {origin} exceeds the {max_bytes} byte limit")]
    BodyTooLarge { max_bytes: usize, origin: String },
    #[error("official source policy contains an invalid URL: {0}")]
    InvalidSource(String),
}

impl NetworkError {
    /// Throttling, authorization, and explicit server retry windows apply to the
    /// logical request, not just one CDN host. Never evade them by switching hosts.
    pub fn permits_source_fallback(&self) -> bool {
        match self {
            Self::Transport { .. } | Self::Deadline { .. } => true,
            Self::Status { status, .. } => {
                *status == StatusCode::NOT_FOUND
                    || (transient(*status) && *status != StatusCode::TOO_MANY_REQUESTS)
            }
            Self::RetryDeferred { .. }
            | Self::Unrepeatable
            | Self::InvalidSource(_)
            | Self::BodyTooLarge { .. } => false,
        }
    }
}

fn public_lookup_count_field(request: &Request) -> Option<&'static str> {
    if request.method() != Method::POST
        || !matches!(
            request.url().host_str(),
            Some("api.steampowered.com" | "api.steamchina.com")
        )
    {
        return None;
    }
    match request.url().path() {
        "/ISteamRemoteStorage/GetPublishedFileDetails/v1/" => Some("itemcount"),
        "/ISteamRemoteStorage/GetCollectionDetails/v1/" => Some("collectioncount"),
        _ => None,
    }
}

fn public_lookup_form(request: &Request) -> bool {
    let Some(count_field) = public_lookup_count_field(request) else {
        return false;
    };
    let content_type = request
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if content_type.split(';').next().map(str::trim) != Some("application/x-www-form-urlencoded") {
        return false;
    }
    let Some(body) = request.body().and_then(|body| body.as_bytes()) else {
        return false;
    };
    if body.len() > 16 * 1024 {
        return false;
    }
    let Ok(form) = std::str::from_utf8(body) else {
        return false;
    };
    // Url's form decoder handles escaped field names as well as '+' and '%xx'.
    let mut url = request.url().clone();
    url.set_query(Some(form));
    let mut count = None;
    let mut indices = std::collections::HashSet::new();
    for (key, value) in url.query_pairs() {
        if key == count_field {
            let Ok(value) = value.parse::<usize>() else {
                return false;
            };
            if count.replace(value).is_some() || !(1..=64).contains(&value) {
                return false;
            }
        } else {
            let Some(index) = key
                .strip_prefix("publishedfileids[")
                .and_then(|key| key.strip_suffix(']'))
            else {
                return false;
            };
            let Ok(index) = index.parse::<usize>() else {
                return false;
            };
            if index >= 64
                || !indices.insert(index)
                || value.is_empty()
                || !value.bytes().all(|byte| byte.is_ascii_digit())
                || value.parse::<u64>().ok().is_none_or(|value| value == 0)
            {
                return false;
            }
        }
    }
    count.is_some_and(|count| {
        indices.len() == count && (0..count).all(|index| indices.contains(&index))
    })
}

fn may_retry(request: &Request) -> bool {
    matches!(*request.method(), Method::GET | Method::HEAD) || public_lookup_form(request)
}

fn public_headers(request: &Request) -> bool {
    // Unknown custom headers can contain credentials. Preserve their original
    // destination instead of guessing every vendor's credential name.
    request.headers().keys().all(|name| {
        matches!(
            name.as_str(),
            "accept"
                | "accept-encoding"
                | "accept-language"
                | "cache-control"
                | "content-type"
                | "content-length"
                | "if-modified-since"
                | "if-none-match"
                | "pragma"
                | "range"
                | "user-agent"
        )
    })
}

fn request_candidates(request: &Request, resolve: impl FnOnce(&str) -> Vec<String>) -> Vec<String> {
    let body_is_public = match *request.method() {
        Method::GET | Method::HEAD => request.body().is_none(),
        Method::POST => public_lookup_form(request),
        _ => false,
    };
    if public_headers(request) && body_is_public {
        resolve(request.url().as_str())
    } else {
        vec![request.url().to_string()]
    }
}

fn transient(status: StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 429 | 500 | 502 | 503 | 504)
}

fn transient_transport(error: &reqwest::Error) -> bool {
    error.is_connect() || error.is_timeout() || error.is_request() || error.is_body()
}

fn retry_delay(response: &Response) -> Option<Duration> {
    match response.headers().get(reqwest::header::RETRY_AFTER) {
        Some(header) => header
            .to_str()
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .map(Duration::from_secs),
        None => Some(Duration::from_millis(250)),
    }
}

// Preserve the server's deferral for callers that coordinate later requests.
// Use its Date when available so workstation clock skew cannot shorten a window.
fn server_retry_delay(response: &Response) -> Option<Duration> {
    let value = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let until = httpdate::parse_http_date(value).ok()?;
    let now = response
        .headers()
        .get(reqwest::header::DATE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| httpdate::parse_http_date(value).ok())
        .unwrap_or_else(std::time::SystemTime::now);
    Some(until.duration_since(now).unwrap_or_default())
}

/// Retry the same read-only request at most once. The caller owns body streaming,
/// validation, cancellation and any restart after a partial response body.
pub async fn send_with_retry(
    client: &Client,
    request: Request,
    budget: Duration,
) -> Result<Response, NetworkError> {
    let origin = request.url().origin().ascii_serialization();
    let deadline = Instant::now() + budget;
    let limit = if may_retry(&request) { 2 } else { 1 };
    for attempt in 0..limit {
        let next = request.try_clone().ok_or(NetworkError::Unrepeatable)?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(NetworkError::Deadline {
                attempts: attempt,
                origin,
            });
        }
        let attempt_deadline = Instant::now() + remaining / (limit - attempt) as u32;
        let result = timeout_at(attempt_deadline, client.execute(next)).await;
        let delay = match result {
            Ok(Ok(response)) if response.status().is_success() => return Ok(response),
            Ok(Ok(response)) => {
                let status = response.status();
                let has_retry_after = response
                    .headers()
                    .contains_key(reqwest::header::RETRY_AFTER);
                // A later transport failure must never erase an earlier 429.
                if status == StatusCode::TOO_MANY_REQUESTS {
                    return Err(if has_retry_after {
                        NetworkError::RetryDeferred {
                            status,
                            origin,
                            retry_after: server_retry_delay(&response),
                        }
                    } else {
                        NetworkError::Status { status, origin }
                    });
                }
                let Some(delay) = retry_delay(&response) else {
                    // HTTP-date or invalid Retry-After is never retried early.
                    return Err(NetworkError::RetryDeferred {
                        status,
                        origin,
                        retry_after: server_retry_delay(&response),
                    });
                };
                if has_retry_after
                    && (delay >= deadline.saturating_duration_since(Instant::now())
                        || (!delay.is_zero() && (attempt + 1 == limit || !transient(status))))
                {
                    return Err(NetworkError::RetryDeferred {
                        status,
                        origin,
                        retry_after: server_retry_delay(&response),
                    });
                }
                if !transient(status) || attempt + 1 == limit {
                    return Err(NetworkError::Status { status, origin });
                }
                delay
            }
            Ok(Err(error)) => {
                if !transient_transport(&error) || attempt + 1 == limit {
                    return Err(NetworkError::Transport {
                        origin,
                        source: error.without_url(),
                    });
                }
                Duration::from_millis(250)
            }
            Err(_) if attempt + 1 < limit => Duration::from_millis(250),
            Err(_) => {
                return Err(NetworkError::Deadline {
                    attempts: attempt + 1,
                    origin,
                });
            }
        };
        if delay >= deadline.saturating_duration_since(Instant::now()) {
            return Err(NetworkError::Deadline {
                attempts: attempt + 1,
                origin,
            });
        }
        timeout_at(deadline, sleep(delay))
            .await
            .map_err(|_| NetworkError::Deadline {
                attempts: attempt + 1,
                origin: origin.clone(),
            })?;
    }
    Err(NetworkError::Deadline {
        attempts: limit,
        origin,
    })
}

/// Apply the official source policy to public requests. The client must not have
/// default authentication headers or a cookie store: reqwest does not expose
/// those settings on Request. Explicit non-public headers/forms stay on origin.
/// This returns after headers; use read_public_bytes for bounded metadata bodies.
pub async fn send_read_only(
    client: &Client,
    request: Request,
    budget: Duration,
    preference: SourcePreference,
) -> Result<Response, NetworkError> {
    send_from_sources(client, request, budget, |url| {
        official_url_candidates(url, preference)
    })
    .await
}

async fn send_from_sources(
    client: &Client,
    request: Request,
    budget: Duration,
    resolve: impl FnOnce(&str) -> Vec<String>,
) -> Result<Response, NetworkError> {
    let deadline = Instant::now() + budget;
    let values = request_candidates(&request, resolve);
    let count = values.len();
    let mut last_error = None;
    for (index, value) in values.into_iter().enumerate() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        let mut next = request.try_clone().ok_or(NetworkError::Unrepeatable)?;
        *next.url_mut() = value
            .parse()
            .map_err(|_| NetworkError::InvalidSource(value.clone()))?;
        let source_budget = remaining / u32::try_from(count - index).unwrap_or(u32::MAX);
        match send_with_retry(client, next, source_budget).await {
            Ok(response) => return Ok(response),
            Err(error) => {
                if !error.permits_source_fallback() {
                    return Err(error);
                }
                record_failure(&value);
                last_error = Some(error);
            }
        }
    }
    Err(last_error.unwrap_or_else(|| NetworkError::Deadline {
        attempts: 0,
        origin: request.url().origin().ascii_serialization(),
    }))
}

#[cfg(test)]
mod tests;
