use super::*;
use crate::commands::commands_mods::mod_download::{
    ModDownloadRequest, download_mod_site_file_with_limit_and_cancellation,
};

struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    fn new() -> Result<Arc<Self>, String> {
        let path = std::env::temp_dir().join(format!("langame-nexus-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path)
            .map_err(|error| format!("Could not create the Nexus download directory: {error}"))?;
        Ok(Arc::new(Self(path)))
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        // Only this newly-created UUID directory is owned by the operation.
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(in crate::commands) struct NexusDownloads {
    directory: Arc<TemporaryDirectory>,
    paths: Vec<PathBuf>,
}

impl NexusDownloads {
    pub(in crate::commands) fn paths(&self) -> &[PathBuf] {
        &self.paths
    }
}

struct CancelDownloadOnDrop(app_steamcmd::InstallCancellation);

impl Drop for CancelDownloadOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

pub(in crate::commands) async fn download_selected_files(
    source: &ModuleModSourceSpec,
    accepts: &[String],
    selections: &[NexusFileSelection],
    api_key: &str,
) -> Result<NexusDownloads, String> {
    validate_batch_len(selections.len())?;
    let domain = source_domain(source)?;
    let mut seen = HashSet::new();
    let parsed = selections
        .iter()
        .map(|selection| {
            let reference = parse_reference(&selection.reference, &domain)?;
            let file_id = reference::canonical_id(&selection.file_id)?;
            if reference.is_nxm && reference.file_id.as_ref().is_some_and(|id| id != &file_id) {
                return Err(failure(
                    "nexus_reference_invalid",
                    "The selected file differs from the file in this Nexus link.",
                ));
            }
            if !seen.insert((reference.mod_id.clone(), file_id.clone())) {
                return Err(failure(
                    "nexus_reference_invalid",
                    "The same Nexus file was selected more than once.",
                ));
            }
            Ok((reference, file_id))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let api = api::NexusApi::new(api_key)?;
    let account = api.account().await?;
    let mut prepared = Vec::with_capacity(parsed.len());
    // Revalidate current upstream details at the trusted install boundary; UI reviews are advisory.
    for (reference, file_id) in parsed {
        reference.validate_authorization(&account, unix_seconds()?)?;
        api.mod_info(&reference).await?;
        let file = api.file(&reference, &file_id).await?;
        let choice = file_choice(&file, accepts, &reference)?;
        if !choice.downloadable {
            return Err(failure(
                "nexus_file_unavailable",
                choice
                    .unavailable_reason
                    .as_deref()
                    .unwrap_or("This Nexus file cannot be downloaded."),
            ));
        }
        prepared.push((reference, file));
    }
    let mut downloads = NexusDownloads {
        directory: TemporaryDirectory::new()?,
        paths: Vec::new(),
    };
    let client = download_client()?;
    for (index, (reference, file)) in prepared.into_iter().enumerate() {
        reference.validate_authorization(&account, unix_seconds()?)?;
        let urls = api
            .download_urls(
                &reference,
                &file.file_id.to_string(),
                !account.is_premium && reference.authorization.is_none(),
            )
            .await?;
        let url = urls
            .into_iter()
            .find(|url| official_download_url(url))
            .ok_or_else(|| {
                failure(
                    "nexus_file_unavailable",
                    "Nexus returned no supported official HTTPS download mirror.",
                )
            })?;
        let path = download_one(
            client.clone(),
            downloads.directory.clone(),
            index,
            url,
            file,
        )
        .await?;
        downloads.paths.push(path);
    }
    Ok(downloads)
}

fn official_download_url(value: &str) -> bool {
    if value.len() > 16384 {
        return false;
    }
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    url.scheme() == "https"
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
        && url.host_str().is_some_and(|host| {
            host == "cf-files.nexusmods.com" || host.ends_with(".nexus-cdn.com")
        })
}

fn download_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(concat!("LanGameServerManager/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 || !official_download_url(attempt.url().as_str()) {
                attempt.error("Nexus download redirect is not an approved official HTTPS mirror")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|_| {
            failure(
                "nexus_network_error",
                "Could not create the Nexus file downloader.",
            )
        })
}

async fn download_one(
    client: reqwest::Client,
    directory: Arc<TemporaryDirectory>,
    index: usize,
    url: String,
    file: api::NexusFile,
) -> Result<PathBuf, String> {
    safe_filename(&file.file_name)?;
    let cancellation = app_steamcmd::InstallCancellation::new();
    let _cancel_on_drop = CancelDownloadOnDrop(cancellation.clone());
    // The worker retains the owned directory until the shared stream writer is closed. Dropping
    // the caller cancels cooperatively rather than racing Windows open-file deletion.
    let worker = tokio::spawn(async move {
        let root = directory.0.join(index.to_string());
        fs::create_dir(&root)
            .map_err(|error| format!("Could not create a Nexus file staging directory: {error}"))?;
        let path = download_mod_site_file_with_limit_and_cancellation(
            &client,
            ModDownloadRequest {
                download_url: &url,
                package_name: "nexus",
                // Upstream names and versions may be long. Keep the intermediate component
                // bounded; the official filename is restored only after transfer validation.
                version: "file",
                preferred_filename: Some("payload.bin"),
                fallback_extension: "zip",
                integrity: None,
                max_bytes: Some(MANUAL_MOD_SITE_DOWNLOAD_MAX_BYTES),
                download_root: &root,
            },
            &cancellation,
        ).await.map_err(|_| failure(
            "nexus_network_error",
            "The Nexus file download failed or was cancelled. Check the connection and available disk space; no file was installed.",
        ))?;
        if let Some(expected) = file.exact_size_bytes() {
            let received = fs::metadata(&path)
                .map_err(|error| format!("Could not inspect the Nexus download: {error}"))?
                .len();
            if received != expected {
                return Err(failure(
                    "nexus_file_unavailable",
                    "The downloaded Nexus file size does not match its metadata.",
                ));
            }
        }
        let target = root.join(file.file_name);
        fs::rename(&path, &target).map_err(|error| {
            format!("Could not preserve the downloaded Nexus file name: {error}")
        })?;
        Ok::<_, String>(target)
    });
    worker.await.map_err(|_| {
        failure(
            "nexus_network_error",
            "The Nexus download task stopped unexpectedly.",
        )
    })?
}

#[cfg(test)]
#[path = "nexus_mods_download_tests.rs"]
mod tests;
