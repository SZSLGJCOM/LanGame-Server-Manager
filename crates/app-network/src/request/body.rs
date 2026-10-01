use std::time::Duration;

use reqwest::{Client, Request, Url};
use tokio::time::{Instant, timeout_at};

use super::{NetworkError, request_candidates, send_with_retry};
use crate::{SourcePreference, official_url_candidates, record_failure};

#[derive(Debug)]
pub struct PublicBytes {
    pub url: Url,
    pub bytes: Vec<u8>,
}

/// Read public metadata under one deadline covering headers, retries and body.
/// The caller must use a Client without default credentials/cookies and validate
/// the payload before recording source health. An incomplete body is discarded.
pub async fn read_public_bytes(
    client: &Client,
    request: Request,
    budget: Duration,
    max_bytes: usize,
    preference: SourcePreference,
) -> Result<PublicBytes, NetworkError> {
    read_from_sources(client, request, budget, max_bytes, |url| {
        official_url_candidates(url, preference)
    })
    .await
}

/// Read exactly one public origin when the caller must validate the response's
/// identity/content before deciding whether to try another official source.
/// Header retries and the entire body still share the supplied deadline.
pub async fn read_public_bytes_from_source(
    client: &Client,
    request: Request,
    budget: Duration,
    max_bytes: usize,
) -> Result<PublicBytes, NetworkError> {
    read_from_sources(client, request, budget, max_bytes, |url| {
        vec![url.to_owned()]
    })
    .await
}

pub(super) async fn read_from_sources(
    client: &Client,
    request: Request,
    budget: Duration,
    max_bytes: usize,
    resolve: impl FnOnce(&str) -> Vec<String>,
) -> Result<PublicBytes, NetworkError> {
    let deadline = Instant::now() + budget;
    let values = request_candidates(&request, resolve);
    let mut last_error = None;
    for (index, value) in values.iter().enumerate() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        let source_budget = remaining / u32::try_from(values.len() - index).unwrap_or(u32::MAX);
        let source_deadline = (Instant::now() + source_budget).min(deadline);
        let mut next = request.try_clone().ok_or(NetworkError::Unrepeatable)?;
        *next.url_mut() = value
            .parse()
            .map_err(|_| NetworkError::InvalidSource(value.clone()))?;
        let origin = next.url().origin().ascii_serialization();
        let result = timeout_at(source_deadline, async {
            let mut response = send_with_retry(client, next, source_budget).await?;
            let url = response.url().clone();
            if response
                .content_length()
                .is_some_and(|size| size > max_bytes as u64)
            {
                return Err(NetworkError::BodyTooLarge {
                    max_bytes,
                    origin: origin.clone(),
                });
            }
            let mut bytes = Vec::new();
            while let Some(chunk) =
                response
                    .chunk()
                    .await
                    .map_err(|error| NetworkError::Transport {
                        origin: origin.clone(),
                        source: error.without_url(),
                    })?
            {
                if Instant::now() >= source_deadline {
                    return Err(NetworkError::Deadline {
                        attempts: index + 1,
                        origin: origin.clone(),
                    });
                }
                if chunk.len() > max_bytes.saturating_sub(bytes.len()) {
                    return Err(NetworkError::BodyTooLarge {
                        max_bytes,
                        origin: origin.clone(),
                    });
                }
                bytes.extend_from_slice(&chunk);
            }
            if Instant::now() >= source_deadline {
                return Err(NetworkError::Deadline {
                    attempts: index + 1,
                    origin: origin.clone(),
                });
            }
            Ok(PublicBytes { url, bytes })
        })
        .await
        .unwrap_or_else(|_| {
            Err(NetworkError::Deadline {
                attempts: index + 1,
                origin,
            })
        });
        match result {
            Ok(value) => return Ok(value),
            Err(error) => {
                if !error.permits_source_fallback() {
                    return Err(error);
                }
                record_failure(value);
                last_error = Some(error);
            }
        }
    }
    Err(last_error.unwrap_or_else(|| NetworkError::Deadline {
        attempts: 0,
        origin: request.url().origin().ascii_serialization(),
    }))
}
