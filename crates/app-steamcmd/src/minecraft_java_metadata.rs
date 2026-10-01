use serde::Deserialize;

use crate::http_download::fetch_text_from_candidates;
use crate::{InstallDeadline, SteamCmdError};

const METADATA_LIMIT: usize = 4 * 1024 * 1024;
const ARCHIVE_LIMIT: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct JavaPackage {
    pub(super) link: String,
    pub(super) checksum: String,
    pub(super) size: u64,
}

pub(super) async fn fetch_java_package(
    client: &reqwest::Client,
    required_major: u16,
    deadline: InstallDeadline,
) -> Result<JavaPackage, SteamCmdError> {
    // Adoptium's binary API redirects to the same GitHub asset; it is not an
    // independent binary mirror. Only release discovery has a second origin.
    // The fallback may expose a newer GA patch while Adoptium's index catches up.
    fetch_java_package_from_candidates(
        client,
        &metadata_sources(required_major),
        required_major,
        deadline,
    )
    .await
}

fn metadata_sources(major: u16) -> Vec<String> {
    vec![
        format!(
            "https://api.adoptium.net/v3/assets/latest/{major}/hotspot?architecture=x64&image_type=jre&os=windows&vendor=eclipse"
        ),
        format!("https://api.github.com/repos/adoptium/temurin{major}-binaries/releases/latest"),
    ]
}

async fn fetch_java_package_from_candidates(
    client: &reqwest::Client,
    candidates: &[String],
    required_major: u16,
    deadline: InstallDeadline,
) -> Result<JavaPackage, SteamCmdError> {
    fetch_text_from_candidates(client, candidates, deadline, METADATA_LIMIT, |text| {
        parse_package(text, required_major)
    })
    .await
    .map(|(_, package)| package)
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Metadata {
    Adoptium(Vec<AdoptiumAsset>),
    GitHub(GitHubRelease),
}

#[derive(Deserialize)]
struct AdoptiumAsset {
    release_name: String,
    vendor: String,
    version: AdoptiumVersion,
    binary: AdoptiumBinary,
}

#[derive(Deserialize)]
struct AdoptiumVersion {
    major: u16,
}

#[derive(Deserialize)]
struct AdoptiumBinary {
    architecture: String,
    image_type: String,
    jvm_impl: String,
    heap_size: String,
    os: String,
    package: AdoptiumPackage,
}

#[derive(Deserialize)]
struct AdoptiumPackage {
    name: String,
    link: String,
    checksum: String,
    size: u64,
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    state: String,
    browser_download_url: String,
    size: u64,
    digest: Option<String>,
}

fn parse_package(text: &str, major: u16) -> Result<JavaPackage, String> {
    let metadata: Metadata =
        serde_json::from_str(text).map_err(|error| format!("Invalid Temurin metadata: {error}"))?;
    match metadata {
        Metadata::Adoptium(mut assets) => {
            if assets.len() != 1 {
                return Err("Expected exactly one Adoptium Windows x64 JRE package.".into());
            }
            let asset = assets.remove(0);
            let binary = asset.binary;
            if asset.vendor != "eclipse"
                || asset.version.major != major
                || binary.architecture != "x64"
                || binary.image_type != "jre"
                || binary.jvm_impl != "hotspot"
                || binary.heap_size != "normal"
                || binary.os != "windows"
            {
                return Err(
                    "Adoptium package does not match the required Temurin Windows x64 JRE.".into(),
                );
            }
            let package = binary.package;
            validate_package(
                major,
                &asset.release_name,
                &package.name,
                package.link,
                package.checksum,
                package.size,
            )
        }
        Metadata::GitHub(release) => {
            if release.draft
                || release.prerelease
                || !official_release_url(&release.html_url, major, &release.tag_name, None)
            {
                return Err(
                    "Expected a published GA release in the official Adoptium repository.".into(),
                );
            }
            let expected_name = package_name(major, &release.tag_name)?;
            let mut candidates = release
                .assets
                .into_iter()
                .filter(|asset| asset.name == expected_name);
            let asset = candidates
                .next()
                .ok_or("The Temurin release has no matching Windows x64 JRE archive.")?;
            if candidates.next().is_some() || asset.state != "uploaded" {
                return Err(
                    "Expected exactly one uploaded Temurin Windows x64 JRE archive.".into(),
                );
            }
            let checksum = asset
                .digest
                .as_deref()
                .and_then(|digest| digest.strip_prefix("sha256:"))
                .ok_or("The official Temurin release asset has no SHA-256 digest.")?
                .to_owned();
            validate_package(
                major,
                &release.tag_name,
                &asset.name,
                asset.browser_download_url,
                checksum,
                asset.size,
            )
        }
    }
}

fn package_name(major: u16, release: &str) -> Result<String, String> {
    let version = if major == 8 {
        let (update, build) = release
            .strip_prefix("jdk8u")
            .and_then(|value| value.split_once("-b"))
            .filter(|(update, build)| {
                !update.is_empty()
                    && !build.is_empty()
                    && update.bytes().all(|byte| byte.is_ascii_digit())
                    && build.bytes().all(|byte| byte.is_ascii_digit())
            })
            .ok_or("The Temurin release is not a Java 8 GA release.")?;
        format!("8u{update}b{build}")
    } else {
        let version = release
            .strip_prefix("jdk-")
            .ok_or("The Temurin release is not a GA release.")?;
        let (number, build) = version
            .split_once('+')
            .ok_or("The Temurin GA release has no build number.")?;
        let numbers = number.split('.').collect::<Vec<_>>();
        if numbers.first().and_then(|value| value.parse::<u16>().ok()) != Some(major)
            || numbers
                .iter()
                .any(|value| value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()))
            || build.is_empty()
            || !build.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(
                "The Temurin release does not match the required Java major GA version.".into(),
            );
        }
        format!("{number}_{build}")
    };
    Ok(format!(
        "OpenJDK{major}U-jre_x64_windows_hotspot_{version}.zip"
    ))
}

fn official_release_url(value: &str, major: u16, release: &str, name: Option<&str>) -> bool {
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    let expected = match name {
        Some(name) => {
            format!("/adoptium/temurin{major}-binaries/releases/download/{release}/{name}")
        }
        None => format!("/adoptium/temurin{major}-binaries/releases/tag/{release}"),
    };
    url.scheme() == "https"
        && url.host_str() == Some("github.com")
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.path().replace("%2B", "+").replace("%2b", "+") == expected
}

fn validate_package(
    major: u16,
    release: &str,
    name: &str,
    link: String,
    checksum: String,
    size: u64,
) -> Result<JavaPackage, String> {
    if name != package_name(major, release)?
        || !official_release_url(&link, major, release, Some(name))
    {
        return Err("Temurin package URL or filename does not match its official release.".into());
    }
    if checksum.len() != 64
        || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit())
        || !(1..=ARCHIVE_LIMIT).contains(&size)
    {
        return Err(
            "Temurin package requires a valid SHA-256 digest and bounded nonzero size.".into(),
        );
    }
    Ok(JavaPackage {
        link,
        checksum,
        size,
    })
}

#[cfg(test)]
#[path = "minecraft_java_metadata_tests.rs"]
mod tests;
