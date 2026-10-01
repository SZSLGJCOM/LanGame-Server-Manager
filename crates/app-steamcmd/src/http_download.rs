use std::path::{Path, PathBuf};
use std::time::Duration;

use sha1::{Digest, Sha1};
use sha2::Sha256;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

use super::{InstallDeadline, SteamCmdError, create_minecraft_staging_file, operation_timeout};

const METADATA_LIMIT: usize = 4 * 1024 * 1024;
const ARCHIVE_LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const READ_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const REQUEST_BUDGET: Duration = Duration::from_secs(45);

#[derive(Clone, Copy, Default)]
pub(super) struct DownloadIntegrity<'a> {
    pub sha1: Option<&'a str>,
    pub sha256: Option<&'a str>,
    pub size: Option<u64>,
}

pub(super) struct DownloadedFile {
    path: PathBuf,
    source: String,
    persisted: bool,
}

impl DownloadedFile {
    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    // Callers without an upstream digest record health only after extraction
    // and executable verification.
    pub(super) fn record_success(&self) {
        app_network::record_success(&self.source);
    }

    pub(super) fn persist(mut self, destination: &Path) -> Result<(), SteamCmdError> {
        super::publish_minecraft_staging_file(destination, &self.path)?;
        self.persisted = true;
        Ok(())
    }
}

impl Drop for DownloadedFile {
    fn drop(&mut self) {
        if !self.persisted {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

pub(super) fn http_client() -> Result<reqwest::Client, SteamCmdError> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(READ_IDLE_TIMEOUT)
        .user_agent("LanGame Server Manager/0.1.0")
        .build()
        .map_err(|error| download_error("HTTP client", error))
}

fn download_error(url: &str, error: impl std::fmt::Display) -> SteamCmdError {
    let safe_url = reqwest::Url::parse(url)
        .map(|mut parsed| {
            parsed.set_query(None);
            parsed.set_fragment(None);
            let _ = parsed.set_username("");
            let _ = parsed.set_password(None);
            parsed.to_string()
        })
        .unwrap_or_else(|_| String::from("download source"));
    SteamCmdError::HttpDownload {
        url: safe_url,
        detail: error.to_string(),
        permits_source_fallback: true,
    }
}

async fn response(
    client: &reqwest::Client,
    url: &str,
    deadline: InstallDeadline,
) -> Result<reqwest::Response, SteamCmdError> {
    let request = client
        .get(url)
        .build()
        .map_err(|error| download_error(url, error.without_url()))?;
    let budget = deadline
        .expires_at()
        .saturating_duration_since(tokio::time::Instant::now())
        .min(REQUEST_BUDGET);
    if budget.is_zero() {
        return Err(operation_timeout(deadline));
    }
    let response = deadline
        .run(app_network::send_with_retry(client, request, budget))
        .await
        .map_err(|_| operation_timeout(deadline))?
        .map_err(|error| {
            let permits = error.permits_source_fallback();
            let mut failure = download_error(url, error);
            if let SteamCmdError::HttpDownload {
                permits_source_fallback,
                ..
            } = &mut failure
            {
                *permits_source_fallback = permits;
            }
            failure
        })?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(download_error(
            url,
            format!("HTTP {}", response.status().as_u16()),
        ));
    }
    Ok(response)
}

fn attempts(url: &str) -> Vec<String> {
    let mut urls = app_network::official_url_candidates(
        url,
        app_network::SourcePreference::InternationalFirst,
    );
    if urls.is_empty() {
        urls.push(url.to_owned());
    }
    urls
}

fn candidate_deadline(deadline: InstallDeadline, remaining_sources: usize) -> InstallDeadline {
    let remaining = deadline
        .expires_at()
        .saturating_duration_since(tokio::time::Instant::now());
    deadline.limited_to(remaining / u32::try_from(remaining_sources.max(1)).unwrap_or(u32::MAX))
}

fn finish_candidate<T>(
    result: Result<T, SteamCmdError>,
    candidate: &str,
    deadline: InstallDeadline,
) -> Result<T, SteamCmdError> {
    // Cancellation and the owner's deadline terminate the logical operation.
    // Only an individual source's timeout may advance to its verified alternate.
    deadline.check_cancelled()?;
    match result {
        Err(SteamCmdError::OperationTimedOut { .. }) => {
            Err(SteamCmdError::HttpDownloadInterrupted {
                detail: format!(
                    "Source attempt to {} exhausted its download budget",
                    source_origin(candidate)
                ),
            })
        }
        other => other,
    }
}

#[path = "http_metadata.rs"]
mod metadata;
pub(super) use metadata::{fetch_json, fetch_text_from_candidates, fetch_text_validated};

// Only an interrupted stream may restart at the same source. A bad hash,
// oversized body or invalid JSON may try another official source and validate
// the complete response again; disk failures terminate the operation.
fn is_interrupted(error: &SteamCmdError) -> bool {
    matches!(error, SteamCmdError::HttpDownloadInterrupted { .. })
}

fn permits_source_fallback(error: &SteamCmdError) -> bool {
    matches!(
        error,
        SteamCmdError::HttpDownload {
            permits_source_fallback: true,
            ..
        } | SteamCmdError::HttpDownloadInterrupted { .. }
    )
}

async fn next_chunk(
    response: &mut reqwest::Response,
    url: &str,
    deadline: InstallDeadline,
) -> Result<Option<Vec<u8>>, SteamCmdError> {
    let result = deadline
        .run(tokio::time::timeout(READ_IDLE_TIMEOUT, response.chunk()))
        .await
        .map_err(|_| operation_timeout(deadline))?;
    match result {
        Ok(Ok(chunk)) => Ok(chunk.map(|bytes| bytes.to_vec())),
        Ok(Err(error)) => Err(SteamCmdError::HttpDownloadInterrupted {
            detail: format!(
                "Response stream from {} failed: {}",
                source_origin(url),
                error.without_url()
            ),
        }),
        Err(_) => Err(SteamCmdError::HttpDownloadInterrupted {
            detail: format!(
                "Response stream from {} was idle for 30 seconds",
                source_origin(url)
            ),
        }),
    }
}

fn source_origin(url: &str) -> String {
    reqwest::Url::parse(url)
        .map(|url| url.origin().ascii_serialization())
        .unwrap_or_default()
}

#[cfg(test)]
pub(super) async fn download_file(
    client: &reqwest::Client,
    url: &str,
    destination: &Path,
    integrity: DownloadIntegrity<'_>,
    deadline: InstallDeadline,
) -> Result<DownloadedFile, SteamCmdError> {
    download_file_with_progress(client, url, destination, integrity, deadline, |_, _| {}).await
}

pub(super) async fn download_file_with_progress<F>(
    client: &reqwest::Client,
    url: &str,
    destination: &Path,
    integrity: DownloadIntegrity<'_>,
    deadline: InstallDeadline,
    mut on_progress: F,
) -> Result<DownloadedFile, SteamCmdError>
where
    F: FnMut(u64, Option<u64>),
{
    let candidates = attempts(url);
    // Only cancellable network boundaries race the token. The owner must stay
    // alive until queued file writes finish and the staging handle is closed.
    download_file_from_candidates_with_progress(
        client,
        &candidates,
        destination,
        integrity,
        deadline,
        &mut on_progress,
    )
    .await
}

#[cfg(test)]
async fn download_file_from_candidates(
    client: &reqwest::Client,
    candidates: &[String],
    destination: &Path,
    integrity: DownloadIntegrity<'_>,
    deadline: InstallDeadline,
) -> Result<DownloadedFile, SteamCmdError> {
    download_file_from_candidates_with_progress(
        client,
        candidates,
        destination,
        integrity,
        deadline,
        &mut |_, _| {},
    )
    .await
}

pub(super) async fn download_file_from_candidates_with_progress<F>(
    client: &reqwest::Client,
    candidates: &[String],
    destination: &Path,
    integrity: DownloadIntegrity<'_>,
    deadline: InstallDeadline,
    on_progress: &mut F,
) -> Result<DownloadedFile, SteamCmdError>
where
    F: FnMut(u64, Option<u64>),
{
    deadline.check_cancelled()?;
    if let Some(parent) = destination.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|source| SteamCmdError::CreatePath {
                path: parent.to_owned(),
                source,
            })?;
    }
    deadline.check_cancelled()?;
    let (path, file) = create_minecraft_staging_file(destination)?;
    let mut downloaded = DownloadedFile {
        path,
        source: String::new(),
        persisted: false,
    };
    let mut file = tokio::fs::File::from_std(file);
    let result = async {
        let mut last_error = download_error("download source", "no source was available");
        for (index, candidate) in candidates.iter().enumerate() {
            let source_deadline = candidate_deadline(deadline, candidates.len() - index);
            for retry in 0..if candidates.len() == 1 { 2 } else { 1 } {
                deadline.check_cancelled()?;
                file.set_len(0)
                    .await
                    .map_err(|error| download_error(candidate, error))?;
                deadline.check_cancelled()?;
                file.seek(std::io::SeekFrom::Start(0))
                    .await
                    .map_err(|error| download_error(candidate, error))?;
                deadline.check_cancelled()?;
                let result = stream_file(
                    client,
                    candidate,
                    &mut file,
                    &downloaded.path,
                    integrity,
                    source_deadline,
                    on_progress,
                )
                .await;
                match finish_candidate(result, candidate, deadline) {
                    Ok(()) => {
                        file.flush()
                            .await
                            .map_err(|error| download_error(candidate, error))?;
                        file.sync_all()
                            .await
                            .map_err(|error| download_error(candidate, error))?;
                        deadline.check_cancelled()?;
                        downloaded.source = candidate.clone();
                        if integrity.sha1.is_some() || integrity.sha256.is_some() {
                            downloaded.record_success();
                        }
                        return Ok(());
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
    .await;
    // Tokio file writes run on its blocking pool. Dropping the async wrapper
    // alone can leave that worker holding the Windows file handle, preventing
    // the staging guard from deleting a cancelled download. into_std waits for
    // every in-flight operation before returning the final owned handle.
    drop(file.into_std().await);
    result?;
    deadline.check_cancelled()?;
    Ok(downloaded)
}

async fn stream_file<F>(
    client: &reqwest::Client,
    url: &str,
    file: &mut tokio::fs::File,
    file_path: &Path,
    integrity: DownloadIntegrity<'_>,
    deadline: InstallDeadline,
    on_progress: &mut F,
) -> Result<(), SteamCmdError>
where
    F: FnMut(u64, Option<u64>),
{
    let mut response = response(client, url, deadline).await?;
    let advertised_size = response.content_length();
    let limit = integrity.size.unwrap_or(ARCHIVE_LIMIT).min(ARCHIVE_LIMIT);
    if advertised_size.is_some_and(|size| size > limit) {
        return Err(download_error(
            url,
            "download exceeds the expected size or 2 GiB limit",
        ));
    }
    let mut size = 0_u64;
    on_progress(size, advertised_size);
    let mut last_progress = tokio::time::Instant::now();
    let mut sha1 = Sha1::new();
    let mut sha256 = Sha256::new();
    while let Some(chunk) = next_chunk(&mut response, url, deadline).await? {
        size = size.saturating_add(chunk.len() as u64);
        if size > limit {
            return Err(download_error(
                url,
                "download exceeds the expected size or 2 GiB limit",
            ));
        }
        sha1.update(&chunk);
        sha256.update(&chunk);
        deadline.check_cancelled()?;
        file.write_all(&chunk)
            .await
            .map_err(|source| SteamCmdError::WriteMinecraftServerFile {
                path: file_path.to_owned(),
                source,
            })?;
        deadline.check_cancelled()?;
        if last_progress.elapsed() >= Duration::from_millis(200) {
            on_progress(size, advertised_size);
            last_progress = tokio::time::Instant::now();
        }
    }
    if size == 0
        || integrity
            .size
            .or(advertised_size)
            .is_some_and(|expected| size != expected)
    {
        return Err(download_error(
            url,
            format!(
                "download size mismatch: received {size} bytes, expected {:?}",
                integrity.size.or(advertised_size)
            ),
        ));
    }
    let actual_sha1: String = sha1
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let actual_sha256: String = sha256
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    for (algorithm, expected, actual) in [
        ("SHA-1", integrity.sha1, &actual_sha1),
        ("SHA-256", integrity.sha256, &actual_sha256),
    ] {
        if let Some(expected) = expected.filter(|expected| !expected.eq_ignore_ascii_case(actual)) {
            return Err(download_error(
                url,
                format!("{algorithm} checksum mismatch: expected {expected}, got {actual}"),
            ));
        }
    }
    on_progress(size, advertised_size);
    Ok(())
}

#[cfg(test)]
#[path = "http_download_tests.rs"]
mod tests;
