use super::*;
use sha2::Digest;

pub(super) async fn fetch_modrinth_project_names(
    client: &reqwest::Client,
    project_id: &str,
    reference: &str,
) -> Result<Vec<String>, String> {
    if !is_modrinth_project_token(project_id) {
        return Err("Modrinth metadata has an invalid canonical project ID".into());
    }
    let request = client
        .get(format!("https://api.modrinth.com/v2/project/{project_id}"))
        .build()
        .map_err(|error| error.to_string())?;
    let response = app_network::read_public_bytes(
        client,
        request,
        Duration::from_secs(12),
        512 * 1024,
        app_network::SourcePreference::InternationalFirst,
    )
    .await
    .map_err(|error| format!("failed to identify Modrinth project: {error}"))?;
    #[derive(serde::Deserialize)]
    struct Project {
        id: String,
        slug: String,
    }
    let project: Project = serde_json::from_slice(&response.bytes)
        .map_err(|error| format!("invalid Modrinth project metadata: {error}"))?;
    if project.id != project_id || !is_modrinth_project_token(&project.slug) {
        return Err("Modrinth project metadata does not match the selected version".into());
    }
    Ok(vec![project.id, project.slug, reference.to_string()])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModDownloadDigest {
    Sha512([u8; 64]),
    Sha1([u8; 20]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ModDownloadIntegrity {
    size: u64,
    digest: ModDownloadDigest,
}

impl ModDownloadIntegrity {
    pub(super) fn from_modrinth_file(file: &ModrinthVersionFile) -> Result<Self, String> {
        let digest = if let Some(value) = file.hashes.get("sha512") {
            ModDownloadDigest::Sha512(parse_mod_download_digest(value, "SHA-512")?)
        } else if let Some(value) = file.hashes.get("sha1") {
            ModDownloadDigest::Sha1(parse_mod_download_digest(value, "SHA-1")?)
        } else {
            return Err(String::from(
                "Modrinth file metadata has no supported checksum",
            ));
        };
        Ok(Self {
            size: file.size,
            digest,
        })
    }
}

fn parse_mod_download_digest<const N: usize>(
    value: &str,
    algorithm: &str,
) -> Result<[u8; N], String> {
    if value.len() != N * 2 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!(
            "Modrinth file metadata has an invalid {algorithm} checksum"
        ));
    }
    let mut digest = [0; N];
    for (index, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| format!("Modrinth file metadata has an invalid {algorithm} checksum"))?;
    }
    Ok(digest)
}

enum ModDownloadHasher {
    Sha512(sha2::Sha512),
    Sha1(sha1::Sha1),
}

impl ModDownloadHasher {
    fn new(integrity: ModDownloadIntegrity) -> Self {
        match integrity.digest {
            ModDownloadDigest::Sha512(_) => Self::Sha512(sha2::Sha512::new()),
            ModDownloadDigest::Sha1(_) => Self::Sha1(sha1::Sha1::new()),
        }
    }

    fn update(&mut self, bytes: &[u8]) {
        match self {
            Self::Sha512(hasher) => hasher.update(bytes),
            Self::Sha1(hasher) => hasher.update(bytes),
        }
    }

    fn verify(self, integrity: ModDownloadIntegrity, received_bytes: u64) -> Result<(), String> {
        if received_bytes != integrity.size {
            return Err(mod_download_metadata_size_error(
                received_bytes,
                integrity.size,
            ));
        }
        let (matches, algorithm) = match (self, integrity.digest) {
            (Self::Sha512(hasher), ModDownloadDigest::Sha512(expected)) => {
                (hasher.finalize()[..] == expected, "SHA-512")
            }
            (Self::Sha1(hasher), ModDownloadDigest::Sha1(expected)) => {
                (hasher.finalize()[..] == expected, "SHA-1")
            }
            _ => {
                return Err(String::from(
                    "mod package checksum algorithm changed during download",
                ));
            }
        };
        if !matches {
            return Err(format!("mod package {algorithm} checksum mismatch"));
        }
        Ok(())
    }
}

enum ModDownloadWriteMessage {
    Chunk(Vec<u8>),
    Finish {
        prepared: tokio::sync::oneshot::Sender<Result<(), String>>,
        commit: std::sync::mpsc::Receiver<()>,
    },
}

struct DownloadedFileCleanup {
    path: PathBuf,
    armed: bool,
}

impl DownloadedFileCleanup {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn keep(mut self) -> PathBuf {
        self.armed = false;
        self.path.clone()
    }
}

impl Drop for DownloadedFileCleanup {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct ModDownloadRequest<'a> {
    pub(super) download_url: &'a str,
    pub(super) package_name: &'a str,
    pub(super) version: &'a str,
    pub(super) preferred_filename: Option<&'a str>,
    pub(super) fallback_extension: &'a str,
    pub(super) integrity: Option<ModDownloadIntegrity>,
    pub(super) max_bytes: u64,
    pub(super) download_root: &'a Path,
}

enum ModDownloadFailure {
    Network(String),
    Interrupted(String),
    Terminal(String),
}

impl From<String> for ModDownloadFailure {
    fn from(value: String) -> Self {
        Self::Terminal(value)
    }
}

pub(super) async fn download_mod_site_file_with_limit(
    client: &reqwest::Client,
    request: ModDownloadRequest<'_>,
) -> Result<PathBuf, String> {
    if let Some(integrity) = request.integrity
        && integrity.size > request.max_bytes
    {
        return Err(mod_download_size_limit_error(
            integrity.size,
            request.max_bytes,
        ));
    }
    let candidates = app_network::official_url_candidates(
        request.download_url,
        app_network::SourcePreference::InternationalFirst,
    );
    let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
    let mut failures = Vec::new();
    'sources: for candidate in &candidates {
        // Header retries already belong to app-network. Only an interrupted body
        // may restart once at a single source, without replaying terminal headers.
        for retry in 0..if candidates.len() == 1 { 2 } else { 1 } {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break 'sources;
            }
            let attempt = ModDownloadRequest {
                download_url: candidate,
                ..request
            };
            match tokio::time::timeout(remaining, download_mod_file_from_source(client, attempt))
                .await
            {
                Ok(Ok(path)) => {
                    app_network::record_success(candidate);
                    return Ok(path);
                }
                Ok(Err(ModDownloadFailure::Terminal(error))) => return Err(error),
                Ok(Err(ModDownloadFailure::Network(error))) => {
                    app_network::record_failure(candidate);
                    failures.push(error);
                    break;
                }
                Ok(Err(ModDownloadFailure::Interrupted(error))) => {
                    app_network::record_failure(candidate);
                    failures.push(error);
                    if retry > 0 {
                        break;
                    }
                }
                Err(_) => {
                    failures.push(String::from("mod package download budget expired"));
                    break 'sources;
                }
            }
        }
    }
    Err(format!(
        "mod package could not be downloaded: {}",
        failures.join("; ")
    ))
}

async fn download_mod_file_from_source(
    client: &reqwest::Client,
    request: ModDownloadRequest<'_>,
) -> Result<PathBuf, ModDownloadFailure> {
    let ModDownloadRequest {
        download_url,
        package_name,
        version,
        preferred_filename,
        fallback_extension,
        integrity,
        max_bytes,
        download_root,
    } = request;
    let http_request = client
        .get(download_url)
        .build()
        .map_err(|error| format!("failed to prepare mod package download: {error}"))?;
    let mut response = app_network::send_with_retry(client, http_request, Duration::from_secs(20))
        .await
        .map_err(|error| {
            if error.permits_source_fallback() {
                ModDownloadFailure::Network(format!("failed to download mod package: {error}"))
            } else {
                ModDownloadFailure::Terminal(format!("failed to download mod package: {error}"))
            }
        })?;
    if let Some(length) = response.content_length()
        && length > max_bytes
    {
        return Err(mod_download_size_limit_error(length, max_bytes).into());
    }

    let source_filename = preferred_filename
        .map(sanitize_mod_site_filename)
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| {
            format!(
                "{}.{}",
                sanitize_mod_site_filename(package_name),
                sanitize_mod_site_filename(fallback_extension)
            )
        });
    let file_name = format!(
        "langame-mod-{}-{}-{}-{}",
        sanitize_mod_site_filename(package_name),
        sanitize_mod_site_filename(version),
        uuid::Uuid::new_v4().simple(),
        source_filename
    );
    let path = download_root.join(file_name);
    let writer_path = path.clone();
    let (message_sender, message_receiver) = tokio::sync::mpsc::channel(2);
    let (ready_sender, ready_receiver) = tokio::sync::oneshot::channel();
    let writer = tokio::task::spawn_blocking(move || {
        write_downloaded_mod_file(writer_path, message_receiver, ready_sender, integrity)
    });
    ready_receiver
        .await
        .map_err(|_| String::from("mod package download writer stopped unexpectedly"))??;
    let cleanup = DownloadedFileCleanup::new(path);

    let mut received_bytes = 0u64;
    loop {
        let chunk = match response.chunk().await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break,
            Err(error) => {
                drop(message_sender);
                let _ = wait_for_mod_download_writer(writer).await;
                return Err(ModDownloadFailure::Interrupted(format!(
                    "failed to read mod package download: {}",
                    error.without_url()
                )));
            }
        };
        received_bytes = received_bytes
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| String::from("mod package download size overflowed"))?;
        if received_bytes > max_bytes {
            drop(message_sender);
            let _ = wait_for_mod_download_writer(writer).await;
            return Err(mod_download_size_limit_error(received_bytes, max_bytes).into());
        }
        if let Some(expected) = integrity
            && received_bytes > expected.size
        {
            drop(message_sender);
            let _ = wait_for_mod_download_writer(writer).await;
            return Err(mod_download_metadata_size_error(received_bytes, expected.size).into());
        }
        if message_sender
            .send(ModDownloadWriteMessage::Chunk(chunk.to_vec()))
            .await
            .is_err()
        {
            return wait_for_mod_download_writer(writer)
                .await
                .and(Err(String::from(
                    "mod package download writer stopped unexpectedly",
                )))
                .map_err(Into::into);
        }
    }

    let (prepared_sender, prepared_receiver) = tokio::sync::oneshot::channel();
    let (commit_sender, commit_receiver) = std::sync::mpsc::channel();
    if message_sender
        .send(ModDownloadWriteMessage::Finish {
            prepared: prepared_sender,
            commit: commit_receiver,
        })
        .await
        .is_err()
    {
        return wait_for_mod_download_writer(writer)
            .await
            .and(Err(String::from(
                "mod package download writer stopped unexpectedly",
            )))
            .map_err(Into::into);
    }
    let prepared = prepared_receiver
        .await
        .map_err(|_| String::from("mod package download writer stopped unexpectedly"))
        .and_then(|result| result);
    if let Err(error) = prepared {
        drop(commit_sender);
        drop(message_sender);
        let _ = wait_for_mod_download_writer(writer).await;
        return Err(error.into());
    }
    commit_sender
        .send(())
        .map_err(|_| String::from("mod package download writer stopped unexpectedly"))?;
    wait_for_mod_download_writer(writer).await?;
    Ok(cleanup.keep())
}

fn mod_download_size_limit_error(received_bytes: u64, max_bytes: u64) -> String {
    format!(
        "mod package is too large to install automatically ({} MiB > {} MiB)",
        received_bytes / 1024 / 1024,
        max_bytes / 1024 / 1024
    )
}

fn mod_download_metadata_size_error(received_bytes: u64, expected_bytes: u64) -> String {
    format!(
        "mod package size does not match metadata ({received_bytes} bytes received, {expected_bytes} expected)"
    )
}

fn write_downloaded_mod_file(
    path: PathBuf,
    mut messages: tokio::sync::mpsc::Receiver<ModDownloadWriteMessage>,
    ready: tokio::sync::oneshot::Sender<Result<(), String>>,
    integrity: Option<ModDownloadIntegrity>,
) -> Result<(), String> {
    let file = match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(file) => file,
        Err(error) => {
            let message = format!(
                "failed to create downloaded mod package {}: {}",
                path.display(),
                error
            );
            let _ = ready.send(Err(message.clone()));
            return Err(message);
        }
    };
    if ready.send(Ok(())).is_err() {
        drop(file);
        let _ = fs::remove_file(&path);
        return Err(String::from("mod package download was cancelled"));
    }

    let mut file = Some(file);
    let mut committed = false;
    let mut hasher = integrity.map(ModDownloadHasher::new);
    let mut received_bytes = 0u64;
    let result = loop {
        match messages.blocking_recv() {
            Some(ModDownloadWriteMessage::Chunk(chunk)) => {
                let Some(output) = file.as_mut() else {
                    break Err(String::from(
                        "mod package download writer was already finalized",
                    ));
                };
                if let Err(error) = output.write_all(&chunk) {
                    break Err(format!("failed to write downloaded mod package: {error}"));
                }
                let Some(total) = received_bytes.checked_add(chunk.len() as u64) else {
                    break Err(String::from("mod package download size overflowed"));
                };
                received_bytes = total;
                if let Some(hasher) = &mut hasher {
                    hasher.update(&chunk);
                }
            }
            Some(ModDownloadWriteMessage::Finish { prepared, commit }) => {
                if let Some((hasher, expected)) = hasher.take().zip(integrity)
                    && let Err(error) = hasher.verify(expected, received_bytes)
                {
                    let _ = prepared.send(Err(error.clone()));
                    break Err(error);
                }
                let Some(output) = file.take() else {
                    break Err(String::from(
                        "mod package download writer was already finalized",
                    ));
                };
                let sync_result = output
                    .sync_all()
                    .map_err(|error| format!("failed to flush downloaded mod package: {error}"));
                drop(output);
                if prepared.send(sync_result.clone()).is_err() || sync_result.is_err() {
                    break sync_result;
                }
                committed = commit.recv().is_ok();
                break if committed {
                    Ok(())
                } else {
                    Err(String::from("mod package download was cancelled"))
                };
            }
            None => break Err(String::from("mod package download was cancelled")),
        }
    };
    drop(file);
    if !committed {
        let _ = fs::remove_file(&path);
    }
    result
}

async fn wait_for_mod_download_writer(
    writer: tokio::task::JoinHandle<Result<(), String>>,
) -> Result<(), String> {
    writer
        .await
        .map_err(|error| format!("mod package download writer failed: {error}"))?
}

#[cfg(test)]
#[path = "commands_mod_download_tests.rs"]
mod tests;
