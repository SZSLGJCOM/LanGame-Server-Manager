use std::{
    future::Future,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::Duration,
};

use tauri_plugin_updater::Update;

use super::{AppUpdateError, AppUpdateInstallEvent, sources};

const PROGRESS_WINDOW: Duration = Duration::from_secs(30);
const MIN_WINDOW_BYTES: u64 = 1024 * 1024;
// Only the small installer is an automatic update. Match the release publisher's
// bound so an untrusted mirror cannot stream unlimited bytes before verification.
const MAX_INSTALLER_BYTES: u64 = 256 * 1024 * 1024;

async fn require_progress<T>(
    request: impl Future<Output = Result<T, tauri_plugin_updater::Error>>,
    received: &AtomicU64,
    window: Duration,
    minimum: u64,
) -> Result<T, AppUpdateError> {
    tokio::pin!(request);
    let mut previous = 0;
    loop {
        tokio::select! {
            result = &mut request => return result.map_err(AppUpdateError::Updater),
            () = tokio::time::sleep(window) => {
                let total = received.load(Ordering::Relaxed);
                if total.saturating_sub(previous) < minimum {
                    return Err(AppUpdateError::DownloadStalled);
                }
                previous = total;
            }
        }
    }
}

pub(super) async fn download(
    update: &Update,
    emit: impl Fn(AppUpdateInstallEvent),
) -> Result<Vec<u8>, AppUpdateError> {
    let urls = sources::download_sources(&update.download_url, &update.version)?;
    download_from_sources(
        update,
        &urls,
        emit,
        PROGRESS_WINDOW,
        &[MIN_WINDOW_BYTES, 1],
        MAX_INSTALLER_BYTES,
    )
    .await
}

pub(super) async fn download_from_sources(
    update: &Update,
    urls: &[tauri::Url],
    emit: impl Fn(AppUpdateInstallEvent),
    window: Duration,
    minimums: &[u64],
    maximum_bytes: u64,
) -> Result<Vec<u8>, AppUpdateError> {
    let mut last_error = AppUpdateError::DownloadStalled;
    // Prefer a usable source promptly. If all links are slow, allow progress on
    // the second pass; the caller still bounds the entire operation to 30 min.
    for &minimum in minimums {
        for url in urls {
            let mut source = update.clone();
            source.download_url = url.clone();
            let received = AtomicU64::new(0);
            let oversized = tokio::sync::Notify::new();
            let exceeded_limit = AtomicBool::new(false);
            let mut started = false;
            emit(AppUpdateInstallEvent::Started {
                content_length: None,
            });
            let request = require_progress(
                source.download(
                    |chunk_length, content_length| {
                        if !started {
                            emit(AppUpdateInstallEvent::Started { content_length });
                            started = true;
                        }
                        let total = received
                            .fetch_add(chunk_length as u64, Ordering::Relaxed)
                            .saturating_add(chunk_length as u64);
                        if total > maximum_bytes
                            || content_length.is_some_and(|size| size > maximum_bytes)
                        {
                            exceeded_limit.store(true, Ordering::Relaxed);
                            oversized.notify_one();
                        }
                        emit(AppUpdateInstallEvent::Progress { chunk_length });
                    },
                    // The plugin invokes this before signature verification.
                    || {},
                ),
                &received,
                window,
                minimum,
            );
            let result = tokio::select! {
                biased;
                () = oversized.notified() => Err(AppUpdateError::DownloadTooLarge),
                result = request => result,
            };
            match result {
                Ok(bytes) => {
                    if exceeded_limit.load(Ordering::Relaxed) || bytes.len() as u64 > maximum_bytes
                    {
                        last_error = AppUpdateError::DownloadTooLarge;
                        continue;
                    }
                    emit(AppUpdateInstallEvent::Finished);
                    return Ok(bytes);
                }
                Err(error) => last_error = error,
            }
        }
    }
    Err(last_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[tokio::test]
    async fn abandoning_a_stalled_source_drops_its_request() {
        struct Dropped<'a>(&'a AtomicBool);
        impl Drop for Dropped<'_> {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Relaxed);
            }
        }
        let dropped = AtomicBool::new(false);
        let guard = Dropped(&dropped);
        let request = async move {
            let _guard = guard;
            std::future::pending::<Result<(), tauri_plugin_updater::Error>>().await
        };
        let result = require_progress(request, &AtomicU64::new(0), Duration::ZERO, 1).await;
        assert!(matches!(result, Err(AppUpdateError::DownloadStalled)));
        assert!(dropped.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn completed_error_is_preserved_including_signature_failures() {
        let result = require_progress(
            std::future::ready(Err::<(), _>(
                tauri_plugin_updater::Error::MissingSignedVersion,
            )),
            &AtomicU64::new(0),
            Duration::from_secs(30),
            1,
        )
        .await;
        assert!(matches!(
            result,
            Err(AppUpdateError::Updater(
                tauri_plugin_updater::Error::MissingSignedVersion
            ))
        ));
    }
}
