//! Native Microsoft packages may need a different address family at a CDN hop.
//! Recovery keeps the observed official target, TLS policy and owner's budget.
use super::{
    DownloadIntegrity, DownloadedFile, download_error, download_file_with_header_budget,
    download_file_with_progress, http_client_builder,
};
use crate::{InstallDeadline, SteamCmdError};
use reqwest::{Client, Url, redirect::Policy};
use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;
use std::sync::{Arc, Mutex};

pub(crate) enum NativeDownloadEvent {
    Route { ipv4: bool, origin: String },
    Progress { bytes: u64, total: Option<u64> },
}

struct RedirectTrace {
    target: Url,
    depth: usize,
    rejected: bool,
}

fn trusted_url(url: &Url) -> bool {
    url.scheme() == "https"
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && matches!(
            url.host_str(),
            Some("aka.ms" | "download.microsoft.com" | "download.visualstudio.microsoft.com")
        )
}

impl RedirectTrace {
    fn observe(&mut self, target: &Url, previous: usize) -> bool {
        if self.rejected || previous > 10 || !trusted_url(target) {
            self.rejected = true;
            return false;
        }
        // The shared request retry can visit the first alias again and fail
        // there. Retain the furthest official hop already observed in this
        // operation, rather than replacing a CDN target with that alias.
        if previous >= self.depth {
            self.target = target.clone();
            self.depth = previous;
        }
        true
    }

    fn recovery_target(&self, error: &SteamCmdError) -> Option<Url> {
        if !self.rejected
            && matches!(
                error,
                SteamCmdError::HttpDownload {
                    permits_ip_family_retry: true,
                    ..
                }
            )
        {
            Some(self.target.clone())
        } else {
            None
        }
    }
}

fn client(trace: Arc<Mutex<RedirectTrace>>, ipv4: bool) -> Result<Client, SteamCmdError> {
    let policy = Policy::custom(move |attempt| {
        let allowed = trace
            .lock()
            .is_ok_and(|mut trace| trace.observe(attempt.url(), attempt.previous().len()));
        if !allowed {
            return attempt.error("Microsoft runtime redirect is not trusted or exceeds ten hops");
        }
        Policy::default().redirect(attempt)
    });
    let builder = http_client_builder().redirect(policy);
    let builder = if ipv4 {
        builder.local_address(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
    } else {
        builder
    };
    builder
        .build()
        .map_err(|error| download_error("native HTTP client", error))
}

pub(crate) async fn download_native_package_with_progress<F>(
    url: &str,
    destination: &Path,
    integrity: DownloadIntegrity<'_>,
    deadline: InstallDeadline,
    mut on_event: F,
) -> Result<DownloadedFile, SteamCmdError>
where
    F: FnMut(NativeDownloadEvent),
{
    deadline.check_cancelled()?;
    let initial = Url::parse(url).map_err(|error| download_error(url, error))?;
    if !trusted_url(&initial) {
        return Err(download_error(url, "Microsoft runtime URL is not trusted"));
    }
    let trace = Arc::new(Mutex::new(RedirectTrace {
        target: initial.clone(),
        depth: 0,
        rejected: false,
    }));
    let primary = client(trace.clone(), false)?;
    let remaining = deadline
        .expires_at()
        .saturating_duration_since(tokio::time::Instant::now());
    // Reserve a recovery window only while waiting for response headers. A
    // healthy response body retains the full owner deadline, even on slow links.
    let primary_headers = remaining / 2;
    on_event(NativeDownloadEvent::Route {
        ipv4: false,
        origin: initial.origin().ascii_serialization(),
    });
    let result = download_file_with_header_budget(
        &primary,
        url,
        destination,
        integrity,
        deadline,
        primary_headers,
        |bytes, total| on_event(NativeDownloadEvent::Progress { bytes, total }),
    )
    .await;
    let error = match result {
        Ok(downloaded) => return Ok(downloaded),
        Err(error) => error,
    };
    // The first staging file has been closed and removed by its owner. Neither
    // cancellation nor the parent's deadline may start another request.
    deadline.check_cancelled()?;
    let target = trace
        .lock()
        .ok()
        .and_then(|trace| trace.recovery_target(&error));
    let Some(target) = target else {
        return Err(error);
    };
    let ipv4 = client(trace, true)?;
    on_event(NativeDownloadEvent::Route {
        ipv4: true,
        origin: target.origin().ascii_serialization(),
    });
    download_file_with_progress(
        &ipv4,
        target.as_str(),
        destination,
        integrity,
        deadline,
        |bytes, total| on_event(NativeDownloadEvent::Progress { bytes, total }),
    )
    .await
}

#[cfg(test)]
#[path = "http_native_download_tests.rs"]
mod tests;
