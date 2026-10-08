use tauri::Url;

use super::AppUpdateError;

pub(super) const GITHUB_FEED: &str =
    "https://github.com/SZSLGJCOM/LanGame-Server-Manager/releases/latest/download/latest.json";
const GITHUB_REPOSITORY: &str =
    "https://github.com/SZSLGJCOM/LanGame-Server-Manager/releases/download/";
const PUBLIC_PROXIES: [&str; 2] = ["https://gh-proxy.org/", "https://ghfast.top/"];

fn append_unique(urls: &mut Vec<Url>, value: &str) -> Result<(), AppUpdateError> {
    let url = Url::parse(value).map_err(tauri_plugin_updater::Error::from)?;
    if !urls.contains(&url) {
        urls.push(url);
    }
    Ok(())
}

/// An empty release configuration must remain disabled, including local builds.
pub(super) fn check_sources(configured: &[Url]) -> Result<Vec<Url>, AppUpdateError> {
    if configured.is_empty() {
        return Err(tauri_plugin_updater::Error::EmptyEndpoints.into());
    }
    let mut urls = configured.to_vec();
    append_unique(&mut urls, GITHUB_FEED)?;
    for proxy in PUBLIC_PROXIES {
        append_unique(&mut urls, &format!("{proxy}{GITHUB_FEED}"))?;
    }
    Ok(urls)
}

fn stable_version(version: &str) -> bool {
    version.len() <= 64
        && version.split('.').count() == 3
        && version.split('.').all(|part| {
            !part.is_empty()
                && (part == "0" || !part.starts_with('0'))
                && part.bytes().all(|byte| byte.is_ascii_digit())
        })
}

pub(super) fn download_sources(primary: &Url, version: &str) -> Result<Vec<Url>, AppUpdateError> {
    if !stable_version(version) {
        return Err(AppUpdateError::InvalidDownloadSource);
    }
    let origin =
        format!("{GITHUB_REPOSITORY}v{version}/LanGame.Server.Manager_{version}_x64-setup.exe");
    let proxy_urls = PUBLIC_PROXIES.map(|proxy| format!("{proxy}{origin}"));
    let secure = primary.scheme() == "https"
        && primary.username().is_empty()
        && primary.password().is_none()
        && primary.port().is_none()
        && primary.query().is_none()
        && primary.fragment().is_none();
    let name = format!("LanGame.Server.Manager_{version}_x64-setup.exe");
    let gitcode_web = format!(
        "https://gitcode.com/SZSLGJCOM/LanGame-Server-Manager-Releases/releases/download/v{version}/{name}"
    );
    let gitcode_api = format!(
        "https://api.gitcode.com/api/v5/repos/SZSLGJCOM/LanGame-Server-Manager-Releases/releases/v{version}/attach_files/{name}/download"
    );
    let official = primary.as_str() == origin
        || proxy_urls.iter().any(|url| primary.as_str() == url)
        || primary.as_str() == gitcode_web
        || primary.as_str() == gitcode_api;
    if !secure || !official {
        return Err(AppUpdateError::InvalidDownloadSource);
    }
    let mut urls = vec![primary.clone()];
    append_unique(&mut urls, &origin)?;
    for url in proxy_urls {
        append_unique(&mut urls, &url)?;
    }
    Ok(urls)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_build_does_not_silently_enable_updates() {
        assert!(check_sources(&[]).is_err());
    }

    #[test]
    fn regional_feed_precedes_independent_github_and_proxy_feeds() {
        let regional = Url::parse("https://langame.cn/updates/server-manager/latest.json")
            .expect("fixture URL");
        let urls = check_sources(&[regional.clone(), GITHUB_FEED.parse().unwrap()]).unwrap();
        assert_eq!(urls.len(), 4);
        assert_eq!(urls[0], regional);
        assert_eq!(urls[1].as_str(), GITHUB_FEED);
        assert!(urls[2].as_str().starts_with(PUBLIC_PROXIES[0]));
        assert!(urls[3].as_str().starts_with(PUBLIC_PROXIES[1]));
    }

    #[test]
    fn every_fallback_keeps_the_exact_version_and_small_package() {
        let primary: Url = "https://gitcode.com/SZSLGJCOM/LanGame-Server-Manager-Releases/releases/download/v0.0.4/LanGame.Server.Manager_0.0.4_x64-setup.exe".parse().unwrap();
        let urls = download_sources(&primary, "0.0.4").unwrap();
        assert_eq!(urls.len(), 4);
        assert_eq!(urls[0], primary);
        for url in urls {
            assert!(
                url.as_str()
                    .ends_with("/v0.0.4/LanGame.Server.Manager_0.0.4_x64-setup.exe")
            );
        }
    }

    #[test]
    fn unsafe_or_unrelated_downloads_cannot_enter_the_fallback_chain() {
        for url in [
            "http://github.com/SZSLGJCOM/LanGame-Server-Manager/releases/download/v0.0.4/LanGame.Server.Manager_0.0.4_x64-setup.exe",
            "https://github.com/other/repository/releases/download/v0.0.4/LanGame.Server.Manager_0.0.4_x64-setup.exe",
            "https://gitcode.com/SZSLGJCOM/LanGame-Server-Manager-Releases/releases/download/v0.0.3/LanGame.Server.Manager_0.0.3_x64-setup.exe",
            "https://localhost/update.exe",
        ] {
            assert!(download_sources(&url.parse().unwrap(), "0.0.4").is_err());
        }
        assert!(download_sources(&GITHUB_FEED.parse().unwrap(), "../0.0.4").is_err());
    }

    #[test]
    fn github_primary_is_not_downloaded_twice_in_one_pass() {
        let primary: Url =
            format!("{GITHUB_REPOSITORY}v0.0.4/LanGame.Server.Manager_0.0.4_x64-setup.exe")
                .parse()
                .unwrap();
        assert_eq!(download_sources(&primary, "0.0.4").unwrap().len(), 3);
    }

    #[test]
    fn gitcode_documented_download_endpoint_is_scoped_to_one_official_asset() {
        let url = "https://api.gitcode.com/api/v5/repos/SZSLGJCOM/LanGame-Server-Manager-Releases/releases/v0.0.4/attach_files/LanGame.Server.Manager_0.0.4_x64-setup.exe/download";
        assert_eq!(
            download_sources(&url.parse().unwrap(), "0.0.4")
                .unwrap()
                .len(),
            4
        );
        for invalid in [
            format!("{url}?access_token=example"),
            url.replace("v0.0.4/", "v0.0.3/"),
            url.replace("SZSLGJCOM/", "other/"),
            url.replace("api.gitcode.com", "api.gitcode.com.example.org"),
        ] {
            assert!(download_sources(&invalid.parse().unwrap(), "0.0.4").is_err());
        }
    }
}
