//! Pinned ARK framework delivery. The publisher and installer share the same
//! archive identities; a mirror response never bypasses the upstream digest.
use std::sync::OnceLock;
use std::time::Duration;

use app_network::{NetworkError, SourcePreference};
use reqwest::{Client, Url};
use serde::Deserialize;
use tokio::time::{Instant, timeout_at};

use super::{InstallPhase, InstallReporter, InstallUpdate, digest};

const MAX_ARCHIVE: usize = 32 * 1024 * 1024;
const SOURCE_BUDGET: Duration = Duration::from_secs(90);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    mirror_repository: String,
    mirror_tag: String,
    packages: Vec<Package>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Package {
    id: String,
    module_id: String,
    version: String,
    file_name: String,
    upstream_url: String,
    mirror_url: String,
    pub(super) size: usize,
    pub(super) sha256: String,
    license_url: String,
}

fn manifest() -> Result<&'static Manifest, String> {
    static MANIFEST: OnceLock<Result<Manifest, String>> = OnceLock::new();
    MANIFEST
        .get_or_init(|| {
            let manifest: Manifest = serde_json::from_str(include_str!(
                "../../../../modules/ark-tools/dependencies.json"
            ))
            .map_err(|error| format!("Invalid bundled ARK dependency manifest: {error}"))?;
            if manifest.schema_version != 1
                || manifest.mirror_repository != "SZSLGJCOM/LanGame-Server-Manager-Releases"
                || manifest.mirror_tag != "ark-tools-dependencies-ase-3.56-asa-2.1"
                || manifest.packages.len() != 2
            {
                return Err("Invalid bundled ARK dependency publication identity".into());
            }
            for (id, module, repository, product) in [
                ("ase", "arksurvivalevolved", "AseApi", "ArkApi"),
                ("asa", "arksurvivalascended", "AsaApi", "AsaApi"),
            ] {
                let entries: Vec<_> = manifest.packages.iter().filter(|p| p.id == id).collect();
                let [package] = entries.as_slice() else {
                    return Err("Missing or duplicate bundled ARK dependency".into());
                };
                let version = package.version.strip_prefix(&format!("{product} "))
                    .ok_or("Invalid bundled ARK framework version")?;
                let license_prefix = format!("https://raw.githubusercontent.com/ArkServerApi/{repository}/");
                let license_commit = package.license_url.strip_prefix(&license_prefix)
                    .and_then(|value| value.strip_suffix("/LICENSE"));
                if package.module_id != module
                    || version.is_empty()
                    || !version.bytes().all(|b| b.is_ascii_digit() || b == b'.')
                    || package.file_name != format!("{product}_{version}.zip")
                    || package.upstream_url != format!("https://github.com/ArkServerApi/{repository}/releases/download/{version}/{}", package.file_name)
                    || package.mirror_url != format!("https://api.gitcode.com/api/v5/repos/{}/releases/{}/attach_files/{}/download", manifest.mirror_repository, manifest.mirror_tag, package.file_name)
                    || package.size == 0 || package.size > MAX_ARCHIVE
                    || package.sha256.len() != 64
                    || !package.sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    || !license_commit.is_some_and(|value| value.len() == 40 && value.bytes().all(|b| b.is_ascii_hexdigit()))
                {
                    return Err(format!("Invalid bundled ARK dependency identity: {id}"));
                }
            }
            Ok(manifest)
        })
        .as_ref()
        .map_err(Clone::clone)
}

pub(super) fn package(module: &str) -> Result<&'static Package, String> {
    manifest()?
        .packages
        .iter()
        .find(|package| package.module_id == module)
        .ok_or_else(|| "This game has no bundled ARK dependency identity".into())
}

fn sources(package: &Package, preference: SourcePreference) -> [&str; 2] {
    match preference {
        SourcePreference::ChinaFirst => [&package.mirror_url, &package.upstream_url],
        SourcePreference::InternationalFirst => [&package.upstream_url, &package.mirror_url],
    }
}

fn allowed_delivery(url: &Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && url.host_str().is_some_and(|host| {
            matches!(
                host,
                "github.com"
                    | "release-assets.githubusercontent.com"
                    | "objects.githubusercontent.com"
                    | "gitcode.com"
                    | "api.gitcode.com"
            ) || host.ends_with(".gitcode.com")
                || host.ends_with(".myhuaweicloud.com")
        })
}

fn client() -> Result<Client, String> {
    Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(30))
        .user_agent("LanGame-Server-Manager/ARK-Tools")
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 || !allowed_delivery(attempt.url()) {
                attempt.error("ARK dependency redirected outside its approved delivery hosts")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|error| error.without_url().to_string())
}

struct Failure {
    detail: String,
    fallback: bool,
}

impl From<NetworkError> for Failure {
    fn from(error: NetworkError) -> Self {
        Self {
            fallback: error.permits_source_fallback(),
            detail: error.to_string(),
        }
    }
}

fn invalid(detail: &str) -> Failure {
    Failure {
        detail: detail.into(),
        fallback: true,
    }
}

fn update(
    report: &InstallReporter,
    phase: InstallPhase,
    source: &str,
    received: usize,
    total: usize,
) {
    report(InstallUpdate {
        phase,
        source: Some(source.into()),
        downloaded_bytes: Some(received as u64),
        total_bytes: Some(total as u64),
    });
}

async fn read_source(
    client: &Client,
    url: &str,
    package: &Package,
    deadline: Instant,
    report: &InstallReporter,
) -> Result<Vec<u8>, Failure> {
    let request = client.get(url).build().map_err(|error| Failure {
        detail: error.without_url().to_string(),
        fallback: false,
    })?;
    let source = request
        .url()
        .host_str()
        .unwrap_or("download source")
        .to_owned();
    update(report, InstallPhase::Connecting, &source, 0, package.size);
    let mut response = app_network::send_with_retry(
        client,
        request,
        deadline.saturating_duration_since(Instant::now()),
    )
    .await?;
    if response
        .content_length()
        .is_some_and(|length| length != package.size as u64)
    {
        return Err(invalid(
            "The ARK dependency size does not match its pinned release",
        ));
    }
    let mut bytes = Vec::with_capacity(package.size);
    update(report, InstallPhase::Downloading, &source, 0, package.size);
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| invalid(&error.without_url().to_string()))?
    {
        if chunk.len() > package.size.saturating_sub(bytes.len()) {
            return Err(invalid("The ARK dependency exceeded its pinned size"));
        }
        bytes.extend_from_slice(&chunk);
        update(
            report,
            InstallPhase::Downloading,
            &source,
            bytes.len(),
            package.size,
        );
    }
    update(
        report,
        InstallPhase::Verifying,
        &source,
        bytes.len(),
        package.size,
    );
    if bytes.len() != package.size || digest(&bytes) != package.sha256 {
        return Err(invalid(
            "The ARK dependency archive does not match the pinned release checksum",
        ));
    }
    if Instant::now() >= deadline {
        return Err(invalid(
            "Download timed out before archive verification completed",
        ));
    }
    Ok(bytes)
}

async fn fetch_from(
    client: &Client,
    package: &Package,
    urls: &[&str],
    budget: Duration,
    report: &InstallReporter,
) -> Result<Vec<u8>, String> {
    let deadline = Instant::now() + budget;
    let mut failures = Vec::new();
    for (index, url) in urls.iter().enumerate() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        // Reserve a bounded opportunity for the other source even if this one
        // accepts a connection but never delivers a complete response body.
        let source_deadline =
            Instant::now() + (remaining / (urls.len() - index) as u32).min(SOURCE_BUDGET);
        let outcome = timeout_at(
            source_deadline,
            read_source(client, url, package, source_deadline, report),
        )
        .await
        .unwrap_or_else(|_| {
            Err(invalid(
                "Download timed out before the complete archive arrived",
            ))
        });
        match outcome {
            Ok(bytes) => return Ok(bytes),
            Err(error) => {
                let source = Url::parse(url)
                    .ok()
                    .and_then(|url| url.host_str().map(str::to_owned))
                    .unwrap_or_else(|| "download source".into());
                failures.push(format!("{source}: {}", error.detail));
                if !error.fallback {
                    break;
                }
            }
        }
    }
    Err(failures.join("; "))
}

pub(super) async fn fetch(
    package: &Package,
    preference: SourcePreference,
    report: &InstallReporter,
) -> Result<Vec<u8>, String> {
    fetch_from(
        &client()?,
        package,
        &sources(package, preference),
        SOURCE_BUDGET * 2,
        report,
    )
    .await
    .map_err(|error| match preference {
        SourcePreference::ChinaFirst => format!("方舟扩展依赖下载失败，请检查网络后重试。{error}"),
        SourcePreference::InternationalFirst => {
            format!("ARK dependency download failed. Check the network and retry. {error}")
        }
    })
}

#[cfg(test)]
#[path = "ark_tools_download_tests.rs"]
mod tests;
