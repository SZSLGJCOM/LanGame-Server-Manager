//! Bounded metadata parsing and same-resource source recovery.

use super::*;
use serde::de::DeserializeOwned;

pub(crate) async fn fetch_json<T: DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    deadline: InstallDeadline,
) -> Result<T, SteamCmdError> {
    fetch_text_validated(client, url, deadline, METADATA_LIMIT, |text| {
        serde_json::from_str(text).map_err(|error| error.to_string())
    })
    .await
    .map(|(_, value)| value)
}

pub(crate) async fn fetch_text_validated<T>(
    client: &reqwest::Client,
    url: &str,
    deadline: InstallDeadline,
    max_bytes: usize,
    parse: impl Fn(&str) -> Result<T, String>,
) -> Result<(String, T), SteamCmdError> {
    let candidates = attempts(url);
    fetch_text_from_candidates(client, &candidates, deadline, max_bytes, parse).await
}

pub(crate) async fn fetch_text_from_candidates<T>(
    client: &reqwest::Client,
    candidates: &[String],
    deadline: InstallDeadline,
    max_bytes: usize,
    parse: impl Fn(&str) -> Result<T, String>,
) -> Result<(String, T), SteamCmdError> {
    deadline.check_cancelled()?;
    let deadline = deadline.limited_to(REQUEST_BUDGET);
    let max_bytes = max_bytes.min(METADATA_LIMIT);
    let url = candidates.first().map(String::as_str).unwrap_or("metadata");
    let mut last_error = download_error(url, "no source was available");
    for (index, candidate) in candidates.iter().enumerate() {
        let source_deadline = candidate_deadline(deadline, candidates.len() - index);
        // A single source may restart an interrupted body once. Header retries
        // remain owned by app-network and share the same overall deadline.
        for retry in 0..if candidates.len() == 1 { 2 } else { 1 } {
            let result = async {
                let mut response = response(client, candidate, source_deadline).await?;
                if response
                    .content_length()
                    .is_some_and(|size| size > max_bytes as u64)
                {
                    return Err(download_error(
                        candidate,
                        format!("metadata exceeds the {max_bytes} byte limit (at most 4 MiB)"),
                    ));
                }
                let mut bytes = Vec::new();
                while let Some(chunk) =
                    next_chunk(&mut response, candidate, source_deadline).await?
                {
                    if bytes.len().saturating_add(chunk.len()) > max_bytes {
                        return Err(download_error(
                            candidate,
                            format!("metadata exceeds the {max_bytes} byte limit (at most 4 MiB)"),
                        ));
                    }
                    bytes.extend_from_slice(&chunk);
                }
                let text =
                    String::from_utf8(bytes).map_err(|error| download_error(candidate, error))?;
                let value = parse(&text).map_err(|error| download_error(candidate, error))?;
                deadline.check_cancelled()?;
                Ok((text, value))
            }
            .await;
            match finish_candidate(result, candidate, deadline) {
                Ok(value) => {
                    app_network::record_success(candidate);
                    return Ok(value);
                }
                Err(error) => {
                    if !permits_source_fallback(&error) {
                        return Err(error);
                    }
                    app_network::record_failure(candidate);
                    let restart = retry == 0 && is_interrupted(&error);
                    last_error = error;
                    if !restart {
                        break;
                    }
                }
            }
        }
    }
    Err(last_error)
}
