use super::*;
use crate::http_download::{
    DownloadedFile, download_file_from_candidates_with_progress, http_client_builder,
    permits_source_fallback,
};
use app_network::SourcePreference;
use std::sync::atomic::{AtomicU64, Ordering};

const PACKAGE_BYTES: u64 = 61_159_864;
const MIRROR_ORIGIN: &str = "https://gh-proxy.org";
const FIRST_WINDOW: Duration = Duration::from_secs(30);
const FIRST_WINDOW_BYTES: u64 = 1024 * 1024;

fn official_url() -> String {
    format!(
        "https://github.com/tModLoader/tModLoader/releases/download/{TML_RELEASE}/tModLoader.zip"
    )
}

fn ordered_urls(preference: SourcePreference) -> [String; 2] {
    let upstream = official_url();
    // Only this compile-time, checksum-pinned public asset is eligible.
    let mirror = format!("{MIRROR_ORIGIN}/{upstream}");
    match preference {
        SourcePreference::ChinaFirst => [mirror, upstream],
        SourcePreference::InternationalFirst => [upstream, mirror],
    }
}

#[cfg(any(windows, test))]
fn preference_for(region: Option<&str>, locale: Option<&str>) -> SourcePreference {
    match region.filter(|iso| {
        iso.len() == 2
            && iso.bytes().all(|c| c.is_ascii_alphabetic())
            && !iso.eq_ignore_ascii_case("XX")
    }) {
        Some(iso) if iso.eq_ignore_ascii_case("CN") => SourcePreference::ChinaFirst,
        Some(_) => SourcePreference::InternationalFirst,
        None => SourcePreference::from_locale(locale),
    }
}

#[cfg(windows)]
fn host_preference() -> SourcePreference {
    use windows_sys::Win32::Globalization::{
        GEO_ISO2, GEOCLASS_NATION, GEOID_NOT_AVAILABLE, GetGeoInfoW, GetUserDefaultLocaleName,
        GetUserGeoID,
    };
    fn name(read: impl FnOnce(*mut u16, i32) -> i32) -> Option<String> {
        let mut buffer = [0u16; 85]; // LOCALE_NAME_MAX_LENGTH, including the terminating NUL.
        let length = usize::try_from(read(buffer.as_mut_ptr(), buffer.len() as i32)).ok()?;
        let value = buffer.get(..length)?.strip_suffix(&[0])?;
        String::from_utf16(value).ok()
    }
    // These APIs also support early Windows 10 and Windows Server releases.
    let geo = unsafe { GetUserGeoID(GEOCLASS_NATION) };
    let region = (geo != GEOID_NOT_AVAILABLE)
        .then(|| name(|out, count| unsafe { GetGeoInfoW(geo, GEO_ISO2, out, count, 0) }))
        .flatten();
    let locale = name(|out, count| unsafe { GetUserDefaultLocaleName(out, count) });
    preference_for(region.as_deref(), locale.as_deref())
}

#[cfg(not(windows))]
fn host_preference() -> SourcePreference {
    SourcePreference::InternationalFirst
}

fn release_redirect(initial: &reqwest::Url, target: &reqwest::Url) -> bool {
    // The mirror serves the file itself. Upstream redirects to a signed release
    // asset. Neither route is allowed to contact an arbitrary redirect target.
    initial.as_str() == official_url()
        && target.scheme() == "https"
        && target.host_str() == Some("release-assets.githubusercontent.com")
        && target.port().is_none()
        && target.username().is_empty()
        && target.password().is_none()
        && target
            .path()
            .starts_with("/github-production-release-asset/")
}

fn client() -> Result<reqwest::Client, SteamCmdError> {
    http_client_builder()
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() <= 4
                && attempt
                    .previous()
                    .first()
                    .is_some_and(|initial| release_redirect(initial, attempt.url()))
            {
                attempt.follow()
            } else {
                attempt.error("Unexpected tModLoader release redirect")
            }
        }))
        .build()
        .map_err(|error| preparation_error(format!("无法准备 tModLoader 下载：{error}")))
}

pub(super) async fn download(
    destination: &Path,
    deadline: InstallDeadline,
    progress: &mut impl FnMut(InstallProgressUpdate),
) -> Result<DownloadedFile, SteamCmdError> {
    let urls = ordered_urls(host_preference());
    let client = client()?;
    let integrity = DownloadIntegrity {
        sha256: Some(TML_SHA256),
        size: Some(PACKAGE_BYTES),
        ..Default::default()
    };
    download_ordered(
        &client,
        &urls,
        destination,
        integrity,
        deadline,
        FIRST_WINDOW,
        progress,
    )
    .await
}

async fn download_ordered(
    client: &reqwest::Client,
    urls: &[String; 2],
    destination: &Path,
    integrity: DownloadIntegrity<'_>,
    deadline: InstallDeadline,
    first_window: Duration,
    progress: &mut impl FnMut(InstallProgressUpdate),
) -> Result<DownloadedFile, SteamCmdError> {
    deadline.check_cancelled()?;
    let first = if urls[0].starts_with(MIRROR_ORIGIN) {
        "gh-proxy.org"
    } else {
        "GitHub"
    };
    progress(InstallProgressUpdate::stage(
        InstallPhase::Preparing,
        format!("正在连接 {first} 下载 tModLoader…"),
    ));
    let detail = format!("正在从 {first} 下载 tModLoader…");
    let primary_deadline = deadline.limited_to(
        deadline
            .expires_at()
            .saturating_duration_since(tokio::time::Instant::now())
            / 2,
    );
    let result = preferred_download(
        client,
        &urls[..1],
        destination,
        integrity,
        primary_deadline,
        first_window,
        |bytes, total| progress(InstallProgressUpdate::download(&detail, bytes, total)),
    )
    .await;
    deadline.check_cancelled()?;
    let alternates = match result {
        Ok(Some(file)) => return Ok(file),
        Ok(None) => {
            // The first source was alive but slow. Keep it as a last resort
            // if the alternate is unavailable, without another speed cutoff.
            progress(InstallProgressUpdate::stage(
                InstallPhase::Preparing,
                "首选来源下载较慢，正在尝试另一个 tModLoader 来源…",
            ));
            vec![urls[1].clone(), urls[0].clone()]
        }
        Err(error)
            if permits_source_fallback(&error)
                || matches!(error, SteamCmdError::OperationTimedOut { .. }) =>
        {
            progress(InstallProgressUpdate::stage(
                InstallPhase::Preparing,
                "首选来源下载失败，正在尝试另一个 tModLoader 来源…",
            ));
            vec![urls[1].clone()]
        }
        Err(error) => return Err(error),
    };
    download_file_from_candidates_with_progress(
        client,
        &alternates,
        destination,
        integrity,
        deadline,
        &mut |bytes, total| {
            progress(InstallProgressUpdate::download(
                "正在下载 tModLoader…",
                bytes,
                total,
            ))
        },
    )
    .await
}

/// Sample actual bytes from the normal GET. Before trying another source we
/// cancel and await the first downloader so all Windows staging handles close.
async fn preferred_download(
    client: &reqwest::Client,
    primary: &[String],
    destination: &Path,
    integrity: DownloadIntegrity<'_>,
    deadline: InstallDeadline,
    first_window: Duration,
    mut report: impl FnMut(u64, Option<u64>),
) -> Result<Option<DownloadedFile>, SteamCmdError> {
    let owner = InstallCancellation::current();
    let owner_cancelled = async {
        match owner {
            Some(token) => token.cancelled().await,
            None => std::future::pending().await,
        }
    };
    let attempt = InstallCancellation::new();
    let received = AtomicU64::new(0);
    let response_started = tokio::sync::Notify::new();
    let mut count = |bytes, total| {
        if bytes == 0 {
            response_started.notify_one();
        }
        received.store(bytes, Ordering::Relaxed);
        report(bytes, total);
    };
    let download = attempt.scope(download_file_from_candidates_with_progress(
        client,
        primary,
        destination,
        integrity,
        deadline,
        &mut count,
    ));
    tokio::pin!(download);
    let slow = async {
        response_started.notified().await;
        tokio::time::sleep(first_window).await;
        if received.load(Ordering::Relaxed) >= FIRST_WINDOW_BYTES {
            std::future::pending::<()>().await;
        }
    };
    let deferred = tokio::select! {
        biased;
        _ = owner_cancelled => false,
        result = &mut download => return result.map(Some),
        _ = slow => true,
    };
    attempt.cancel();
    match download.await {
        Err(SteamCmdError::InstallCancelled { .. }) if deferred => Ok(None),
        other => other.map(Some),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_sources_follow_host_region_before_language() {
        let cn = ordered_urls(preference_for(Some("CN"), Some("en-US")));
        let outside = ordered_urls(preference_for(Some("SG"), Some("zh-CN")));
        assert_eq!(cn[0], format!("https://gh-proxy.org/{}", official_url()));
        assert_eq!(cn[1], official_url());
        assert_eq!(outside, [cn[1].clone(), cn[0].clone()]);
        assert_eq!(
            preference_for(None, Some("zh-CN")),
            SourcePreference::ChinaFirst
        );
        assert_eq!(
            preference_for(Some("XX"), None),
            SourcePreference::InternationalFirst
        );
    }

    #[test]
    fn only_the_official_route_can_redirect_to_the_release_cdn() {
        let urls = ordered_urls(SourcePreference::InternationalFirst);
        let github = reqwest::Url::parse(&urls[0]).unwrap();
        let mirror = reqwest::Url::parse(&urls[1]).unwrap();
        let cdn = reqwest::Url::parse("https://release-assets.githubusercontent.com/github-production-release-asset/fixture?sig=signature").unwrap();
        assert!(release_redirect(&github, &cdn));
        assert!(!release_redirect(&mirror, &cdn));
        assert!(!release_redirect(&github, &mirror));
        assert!(!release_redirect(
            &github,
            &reqwest::Url::parse("http://127.0.0.1/private").unwrap()
        ));
    }
}

#[cfg(test)]
#[path = "tmodloader_download_tests.rs"]
mod download_tests;
