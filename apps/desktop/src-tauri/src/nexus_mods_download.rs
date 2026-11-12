use super::*;
use crate::commands::commands_mods::mod_download::{
    ModDownloadFailure, ModDownloadObserver, ModDownloadProgress, ModDownloadRequest,
    download_mod_site_file_with_slow_transfer_budget,
};

struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    fn new() -> Result<Arc<Self>, String> {
        let path = std::env::temp_dir().join(format!("langame-nexus-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path)
            .map_err(|error| download_io_error("create_download_directory", &error))?;
        Ok(Arc::new(Self(path)))
    }
}

fn download_io_error(step: &'static str, error: &std::io::Error) -> String {
    // Only fixed operation labels and OS error categories leave this boundary.
    // File paths, response text and short-lived website URLs are never included.
    json!({
        "code": "nexus_installation_failed",
        "message": format!("Nexus local file operation {step} failed ({:?}, OS {:?}).", error.kind(), error.raw_os_error()),
        "local_file_step": step,
        "io_kind": format!("{:?}", error.kind()),
        "os_error": error.raw_os_error(),
    }).to_string()
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

// Website-generated download URLs can contain short-lived authorization. They
// remain memory-only and are never included in diagnostics or serialization.
pub(in crate::commands) struct WebsiteDownload {
    url: String,
    file_name: String,
    needs_archive_identification: bool,
    observer: Option<WebsiteDownloadObserver>,
}

#[derive(Clone, Copy)]
pub(in crate::commands) enum WebsiteDownloadProgress {
    Downloading(ModDownloadProgress),
    Verifying,
}

pub(in crate::commands) type WebsiteDownloadObserver =
    Arc<dyn Fn(WebsiteDownloadProgress) + Send + Sync>;

impl WebsiteDownload {
    pub(in crate::commands) fn observe(mut self, observer: WebsiteDownloadObserver) -> Self {
        self.observer = Some(observer);
        self
    }
}

pub(in crate::commands) fn prepare_website_download(
    url: String,
    file_name: String,
    accepts: &[String],
) -> Result<WebsiteDownload, String> {
    if !official_download_url(&url) {
        return Err(failure(
            "nexus_file_unavailable",
            "The website download must use an official Nexus HTTPS file server.",
        ));
    }
    safe_filename(&file_name)?;
    let needs_archive_identification = !accepted_filename(&file_name, accepts);
    if needs_archive_identification
        && !accepts.iter().any(|extension| {
            extension
                .trim()
                .trim_start_matches('.')
                .eq_ignore_ascii_case("zip")
        })
    {
        return Err(failure(
            "nexus_format_unsupported",
            "This server does not accept this file type or archive imports.",
        ));
    }
    Ok(WebsiteDownload {
        url,
        file_name,
        needs_archive_identification,
        observer: None,
    })
}

pub(in crate::commands) async fn download_website_file(
    download: WebsiteDownload,
) -> Result<NexusDownloads, String> {
    download_website_file_with_client(download, download_client()?).await
}

async fn download_website_file_with_client(
    download: WebsiteDownload,
    client: reqwest::Client,
) -> Result<NexusDownloads, String> {
    let directory = TemporaryDirectory::new()?;
    let observer = download.observer;
    let transfer_observer = observer.clone().map(|observer| {
        Arc::new(move |progress| observer(WebsiteDownloadProgress::Downloading(progress)))
            as ModDownloadObserver
    });
    let path = download_payload(
        client,
        directory.clone(),
        0,
        download.url,
        download.file_name,
        None,
        transfer_observer,
    )
    .await?;
    if let Some(observer) = observer {
        observer(WebsiteDownloadProgress::Verifying);
    }
    let path = if download.needs_archive_identification {
        let retained_directory = directory.clone();
        tokio::task::spawn_blocking(move || {
            let _owned_download = retained_directory;
            identify_website_archive(path)
        })
        .await
        .map_err(|_| {
            failure(
                "nexus_file_unavailable",
                "The Nexus archive identification worker did not complete.",
            )
        })??
    } else {
        path
    };
    Ok(NexusDownloads {
        directory,
        paths: vec![path],
    })
}

fn identify_website_archive(path: PathBuf) -> Result<PathBuf, String> {
    // The website may suggest a display title such as "Camera Tweaks" instead
    // of a filename. Only the downloaded archive proves its format.
    if let Some(format) =
        crate::commands::commands_mod_staging::archive_formats::inspect_additional_mod_archive(
            &path,
        )
        .map_err(|error| failure("nexus_installation_failed", &error))?
    {
        return prepare_identified_archive(path, format.extension());
    }
    let invalid = || {
        failure(
            "nexus_format_unsupported",
            "The downloaded file could not be identified as a supported archive.",
        )
    };
    let file = crate::commands::commands_mod_staging::open_preflighted_mod_archive(&path)
        .map_err(|_| invalid())?;
    let archive = zip::ZipArchive::new(file).map_err(|_| invalid())?;
    drop(archive);
    prepare_identified_archive(path, "zip")
}

fn prepare_identified_archive(path: PathBuf, extension: &'static str) -> Result<PathBuf, String> {
    // Use a short owned name rather than appending to a potentially full-length
    // website suggestion. Entry validation and publication stay in normal staging.
    let target = path.with_file_name(format!("payload.{extension}"));
    if path != target {
        fs::rename(&path, &target)
            .map_err(|error| download_io_error("prepare_identified_archive", &error))?;
    }
    Ok(target)
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

fn payload_download_failure(error: ModDownloadFailure) -> String {
    // The shared downloader already separates transport failures from HTTP and
    // local validation/write failures. Never serialize its URL-bearing detail.
    match error {
        ModDownloadFailure::Network(_) | ModDownloadFailure::Interrupted(_) => failure(
            "nexus_network_error",
            "The Nexus file transfer failed. Check the connection and retry; no file was installed.",
        ),
        ModDownloadFailure::Http(_) => failure(
            "nexus_file_unavailable",
            "The Nexus file server did not accept this download request. Retry from the website; no file was installed.",
        ),
        ModDownloadFailure::SizeLimit { max_bytes, .. } => failure(
            "nexus_installation_failed",
            &format!(
                "The Nexus website file exceeds the {} MiB automatic download limit. No file was installed.",
                max_bytes / 1024 / 1024
            ),
        ),
        ModDownloadFailure::TimeLimit => failure(
            "nexus_installation_failed",
            "The Nexus file transfer reached its four hour download limit. No file was installed.",
        ),
        ModDownloadFailure::Terminal(_) => failure(
            "nexus_installation_failed",
            "The Nexus download could not be validated or saved locally. No file was installed.",
        ),
        ModDownloadFailure::Cancelled => failure(
            "nexus_installation_failed",
            "The Nexus file download was cancelled before installation.",
        ),
    }
}

async fn download_one(
    client: reqwest::Client,
    directory: Arc<TemporaryDirectory>,
    index: usize,
    url: String,
    file: api::NexusFile,
) -> Result<PathBuf, String> {
    let expected_size = file.exact_size_bytes();
    download_payload(
        client,
        directory,
        index,
        url,
        file.file_name,
        expected_size,
        None,
    )
    .await
}

async fn download_payload(
    client: reqwest::Client,
    directory: Arc<TemporaryDirectory>,
    index: usize,
    url: String,
    file_name: String,
    expected_size: Option<u64>,
    observer: Option<ModDownloadObserver>,
) -> Result<PathBuf, String> {
    safe_filename(&file_name)?;
    let cancellation = app_steamcmd::InstallCancellation::new();
    let _cancel_on_drop = CancelDownloadOnDrop(cancellation.clone());
    // The worker retains the owned directory until the shared stream writer is closed. Dropping
    // the caller cancels cooperatively rather than racing Windows open-file deletion.
    let worker = tokio::spawn(async move {
        let root = directory.0.join(index.to_string());
        fs::create_dir(&root)
            .map_err(|error| download_io_error("create_file_directory", &error))?;
        let path = download_mod_site_file_with_slow_transfer_budget(
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
            observer,
        )
        .await
        .map_err(payload_download_failure)?;
        if let Some(expected) = expected_size {
            let received = fs::metadata(&path)
                .map_err(|error| download_io_error("inspect_download_size", &error))?
                .len();
            if received != expected {
                return Err(failure(
                    "nexus_file_unavailable",
                    "The downloaded Nexus file size does not match its metadata.",
                ));
            }
        }
        let target = root.join(file_name);
        fs::rename(&path, &target)
            .map_err(|error| download_io_error("restore_suggested_name", &error))?;
        Ok::<_, String>(target)
    });
    worker.await.map_err(|_| {
        failure(
            "nexus_installation_failed",
            "The Nexus download task stopped unexpectedly.",
        )
    })?
}

#[cfg(test)]
#[path = "nexus_mods_download_tests.rs"]
mod tests;
