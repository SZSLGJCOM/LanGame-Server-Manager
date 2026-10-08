//! Opt-in acceptance of a published installer using the production downloader.
//! This never installs, starts/stops the app, or changes proxy configuration.
use std::{
    fs::{File, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::PathBuf,
    sync::Mutex,
    time::Instant,
};

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri_plugin_updater::UpdaterExt;

use super::super::{
    AppUpdateError, AppUpdateInstallEvent, UPDATE_DOWNLOAD_TIMEOUT, UPDATE_SOURCE_CHECK_TIMEOUT,
    check_update_sources, download, sources,
};

const VERSION: &str = "0.0.4";
const GITHUB: &str =
    "https://github.com/SZSLGJCOM/LanGame-Server-Manager/releases/download/v0.0.4/";
const GITCODE: &str = "https://api.gitcode.com/api/v5/repos/SZSLGJCOM/LanGame-Server-Manager-Releases/releases/v0.0.4/attach_files/";
const INSTALLER: &str = "LanGame.Server.Manager_0.0.4_x64-setup.exe";

fn source_urls(source: &str) -> Result<(String, Option<String>), &'static str> {
    let github = format!("{GITHUB}{INSTALLER}");
    match source {
        "regional" => Ok((
            "https://langame.cn/updates/server-manager/latest.json".into(),
            None,
        )),
        "github" => Ok((format!("{GITHUB}latest.json"), Some(github))),
        "gitcode" => Ok((
            format!("{GITCODE}latest.json/download"),
            Some(format!("{GITCODE}{INSTALLER}/download")),
        )),
        "gh-proxy" | "ghfast" => {
            let proxy = if source == "gh-proxy" {
                "https://gh-proxy.org/"
            } else {
                "https://ghfast.top/"
            };
            Ok((
                format!("{proxy}{GITHUB}latest.json"),
                Some(format!("{proxy}{github}")),
            ))
        }
        _ => Err("LGSM_LIVE_UPDATE_SOURCE must be regional/github/gitcode/gh-proxy/ghfast"),
    }
}

#[derive(Default, Serialize)]
struct Progress {
    // The production protocol emits a reset and then a length notification.
    // Keep both, including empty failed attempts, instead of equating Started
    // notifications with network attempts or adding failed bytes to success.
    segments: Vec<Segment>,
    received_bytes_all_sources: u64,
    finished: usize,
    installing: usize,
}

#[derive(Serialize)]
struct Segment {
    elapsed_ms: u128,
    content_length: Option<u64>,
    bytes: u64,
    chunks: u64,
}

impl Progress {
    fn record(&mut self, event: AppUpdateInstallEvent, started: Instant) {
        match event {
            AppUpdateInstallEvent::Started { content_length } => self.segments.push(Segment {
                elapsed_ms: started.elapsed().as_millis(),
                content_length,
                bytes: 0,
                chunks: 0,
            }),
            AppUpdateInstallEvent::Progress { chunk_length } => {
                let segment = self.segments.last_mut().expect("Started precedes progress");
                segment.bytes += chunk_length as u64;
                segment.chunks += 1;
                self.received_bytes_all_sources += chunk_length as u64;
            }
            AppUpdateInstallEvent::Finished => self.finished += 1,
            AppUpdateInstallEvent::Installing => self.installing += 1,
        }
    }
}

fn failure_kind(error: &AppUpdateError) -> &'static str {
    use tauri_plugin_updater::Error;
    match error {
        AppUpdateError::CheckTimedOut => "metadata_timeout",
        AppUpdateError::DownloadTimedOut => "download_timeout",
        AppUpdateError::DownloadStalled => "download_stalled",
        AppUpdateError::DownloadTooLarge => "download_too_large",
        AppUpdateError::InvalidDownloadSource => "invalid_download_source",
        AppUpdateError::Updater(Error::Minisign(_)) => "signature_verification_failed",
        AppUpdateError::Updater(Error::MissingSignedVersion) => "missing_signed_version",
        AppUpdateError::Updater(Error::SignedVersionMismatch { .. }) => "signed_version_mismatch",
        AppUpdateError::Updater(Error::Reqwest(error)) if error.is_timeout() => "http_timeout",
        AppUpdateError::Updater(Error::Reqwest(_)) => "http_request_failed",
        _ => "updater_failed",
    }
}

fn report_file() -> File {
    let path =
        PathBuf::from(std::env::var_os("LGSM_LIVE_UPDATE_REPORT").expect("report path required"));
    assert!(path.is_absolute() && path.extension().is_some_and(|ext| ext == "json"));
    let parent = path
        .parent()
        .unwrap()
        .canonicalize()
        .expect("existing report directory");
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    match repository.canonicalize() {
        Ok(repository) => assert!(
            !parent.starts_with(repository),
            "report must be outside the repository"
        ),
        // The same test executable runs on the other acceptance machines;
        // their filesystem need not contain the compilation source checkout.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => panic!("compilation repository path cannot be inspected"),
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(parent.join(path.file_name().unwrap()))
        .expect("report must be a new file")
}

fn save(file: &mut File, value: &serde_json::Value) {
    file.seek(SeekFrom::Start(0)).unwrap();
    file.set_len(0).unwrap();
    serde_json::to_writer_pretty(&mut *file, value).unwrap();
    file.write_all(b"\n").unwrap();
    file.sync_all().unwrap();
}

/// Run explicitly with --ignored --exact and all LGSM_LIVE_UPDATE_* inputs.
/// NETWORK is a declared CN/US matrix label; verify route evidence separately.
/// SHA256 must come from the independently verified, published 0.0.4 release.
#[tokio::test]
#[ignore = "downloads the real published 0.0.4 installer; requires explicit source, network, SHA256 and new report path"]
async fn published_004_download_without_install() {
    let source = std::env::var("LGSM_LIVE_UPDATE_SOURCE").expect("explicit source required");
    let (endpoint, primary) = source_urls(&source).expect("fixed official source required");
    let network =
        std::env::var("LGSM_LIVE_UPDATE_NETWORK").expect("explicit network label required");
    assert!(matches!(network.as_str(), "CN" | "US"));
    let expected = std::env::var("LGSM_LIVE_UPDATE_SHA256").expect("approved SHA256 required");
    assert!(expected.len() == 64 && expected.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let mut report = report_file();
    let mut result = serde_json::json!({
        "probe": "published-installer-production-download", "version": VERSION,
        "source": source, "declaredNetwork": network, "networkRouteVerifiedByProbe": false,
        "expectedSha256": expected.to_ascii_lowercase(), "passed": false,
        "installsOrStartsApplication": false, "phase": "metadata",
        "coverage": "Production download fallback and real updater signature/version verification; no GUI upgrade or network fault injection"
    });
    save(&mut report, &result);

    let mut context = tauri::test::mock_context(tauri::test::noop_assets());
    // UpdaterBuilder gets current_version from the app package. Its constructor
    // is private and it exposes no current_version setter in 2.13.1.
    context.package_info_mut().version = "0.0.3".parse().unwrap();
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../../../tauri.conf.json")).unwrap();
    let updater_config = config["plugins"]["updater"].clone();
    assert_eq!(updater_config["requireSignedVersion"], true);
    context
        .config_mut()
        .plugins
        .0
        .insert("updater".into(), updater_config);
    let app = tauri::test::mock_builder()
        .plugin(
            tauri_plugin_updater::Builder::new()
                .target("windows-x86_64")
                .build(),
        )
        .build(context)
        .unwrap();
    let started = Instant::now();
    let checked = check_update_sources(
        || app.updater_builder(),
        &[endpoint.parse().unwrap()],
        UPDATE_SOURCE_CHECK_TIMEOUT,
    )
    .await;
    let mut update = match checked {
        Ok(Some(update)) if update.version == VERSION => update,
        other => {
            result["failure"] = match other {
                Err(error) => failure_kind(&error),
                _ => "expected_published_version_unavailable",
            }
            .into();
            result["elapsedMs"] = (started.elapsed().as_millis() as u64).into();
            save(&mut report, &result);
            panic!("published update metadata probe failed; sanitized report saved");
        }
    };
    if let Some(primary) = primary {
        // Select only the explicitly chosen official mirror; retain the genuine
        // manifest version and signature. Production admission checks it again.
        update.download_url = primary.parse().unwrap();
    }
    result["candidateSources"] = serde_json::to_value(
        sources::download_sources(&update.download_url, VERSION)
            .unwrap()
            .iter()
            .map(|url| url.as_str())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    result["phase"] = "download".into();
    save(&mut report, &result);
    let progress = Mutex::new(Progress::default());
    let last_progress_log = Mutex::new(Instant::now());
    let downloaded = tokio::time::timeout(
        UPDATE_DOWNLOAD_TIMEOUT,
        download::download(&update, |event| {
            let important = matches!(
                &event,
                AppUpdateInstallEvent::Started { .. } | AppUpdateInstallEvent::Finished
            );
            let mut progress = progress.lock().unwrap();
            progress.record(event, started);
            let mut last = last_progress_log.lock().unwrap();
            if important || last.elapsed().as_secs() >= 5 {
                println!("LGSM_UPDATE_PROBE {}", serde_json::json!({
                    "elapsedMs": started.elapsed().as_millis() as u64,
                    "startedNotifications": progress.segments.len(),
                    "currentSegmentBytes": progress.segments.last().map(|segment| segment.bytes),
                    "totalReceivedBytes": progress.received_bytes_all_sources,
                    "finished": progress.finished
                }));
                *last = Instant::now();
            }
        }),
    )
    .await
    .map_err(|_| AppUpdateError::DownloadTimedOut)
    .and_then(|value| value);
    let progress = progress.into_inner().unwrap();
    result["elapsedMs"] = (started.elapsed().as_millis() as u64).into();
    result["progress"] = serde_json::to_value(&progress).unwrap();
    match downloaded {
        Ok(bytes) => {
            let hash: String = Sha256::digest(&bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            result["bytes"] = bytes.len().into();
            result["sha256"] = hash.clone().into();
            result["passed"] = (hash.eq_ignore_ascii_case(&expected)
                && progress.finished == 1
                && progress.installing == 0)
                .into();
            result["phase"] = "verified_download".into();
        }
        Err(error) => result["failure"] = failure_kind(&error).into(),
    }
    save(&mut report, &result);
    assert_eq!(
        result["passed"], true,
        "download probe failed; sanitized report saved"
    );
}

#[test]
fn probe_only_selects_fixed_official_release_sources() {
    for source in ["regional", "github", "gitcode", "gh-proxy", "ghfast"] {
        let (_, primary) = source_urls(source).unwrap();
        if let Some(primary) = primary {
            assert!(sources::download_sources(&primary.parse().unwrap(), VERSION).is_ok());
        }
    }
    assert!(source_urls("https://example.com/update.exe").is_err());
}
