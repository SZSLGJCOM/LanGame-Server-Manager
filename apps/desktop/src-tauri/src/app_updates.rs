use std::{future::Future, sync::Mutex, time::Duration};

use serde::Serialize;
use tauri::{AppHandle, State, ipc::Channel};
use tauri_plugin_updater::{Update, UpdaterExt};
use thiserror::Error;

const UPDATE_CHECK_TIMEOUT: Duration = Duration::from_secs(30);
// Allow slow connections to transfer the complete update installer.
const UPDATE_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);

#[derive(Default)]
pub struct PendingAppUpdate(Mutex<Option<Update>>);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateMetadata {
    version: String,
    current_version: String,
    date: Option<String>,
    body: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AppUpdateCheckResult {
    Current { current_version: String },
    Available { update: AppUpdateMetadata },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum AppUpdateInstallEvent {
    Started { content_length: Option<u64> },
    Progress { chunk_length: usize },
    Finished,
    Installing,
}

#[derive(Debug, Error)]
pub enum AppUpdateError {
    #[error("failed to access updater: {0}")]
    Updater(#[from] tauri_plugin_updater::Error),
    #[error("desktop update state lock poisoned")]
    StatePoisoned,
    #[error("no pending application update is ready to install")]
    NoPendingUpdate,
    #[error("checking for application updates timed out. Check your connection and try again.")]
    CheckTimedOut,
    #[error(
        "downloading the application update timed out. Servers have not been stopped. Check your connection and check for updates again."
    )]
    DownloadTimedOut,
    #[error("cannot stop the runtime service before updating: {0}")]
    RuntimeShutdown(String),
    #[cfg(windows)]
    #[error(
        "application installation failed after servers were saved and stopped: {0}. Reopen LanGame to reconnect to its runtime service before continuing."
    )]
    InstallationAfterShutdown(String),
}

impl Serialize for AppUpdateError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

async fn with_update_timeout<T>(
    timeout: Duration,
    timeout_error: AppUpdateError,
    request: impl Future<Output = Result<T, tauri_plugin_updater::Error>>,
) -> Result<T, AppUpdateError> {
    tokio::time::timeout(timeout, request)
        .await
        .map_err(|_| timeout_error)?
        .map_err(AppUpdateError::Updater)
}

#[tauri::command]
pub async fn check_app_update(
    app: AppHandle,
    pending_update: State<'_, PendingAppUpdate>,
) -> Result<AppUpdateCheckResult, AppUpdateError> {
    let updater = app.updater()?;
    let update = with_update_timeout(
        UPDATE_CHECK_TIMEOUT,
        AppUpdateError::CheckTimedOut,
        updater.check(),
    )
    .await?;
    let current_version = app.package_info().version.to_string();
    let metadata = update.as_ref().map(|update| AppUpdateMetadata {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
        date: update.date.map(|date| date.to_string()),
        body: update.body.clone(),
    });

    *pending_update
        .0
        .lock()
        .map_err(|_| AppUpdateError::StatePoisoned)? = update;

    Ok(match metadata {
        Some(update) => AppUpdateCheckResult::Available { update },
        None => AppUpdateCheckResult::Current { current_version },
    })
}

#[tauri::command]
pub async fn install_app_update(
    app: AppHandle,
    pending_update: State<'_, PendingAppUpdate>,
    on_event: Channel<AppUpdateInstallEvent>,
) -> Result<(), AppUpdateError> {
    let update = pending_update
        .0
        .lock()
        .map_err(|_| AppUpdateError::StatePoisoned)?
        .take()
        .ok_or(AppUpdateError::NoPendingUpdate)?;

    let mut started = false;
    let bytes = with_update_timeout(
        UPDATE_DOWNLOAD_TIMEOUT,
        AppUpdateError::DownloadTimedOut,
        update.download(
            |chunk_length, content_length| {
                if !started {
                    let _ = on_event.send(AppUpdateInstallEvent::Started { content_length });
                    started = true;
                }
                let _ = on_event.send(AppUpdateInstallEvent::Progress { chunk_length });
            },
            || {
                let _ = on_event.send(AppUpdateInstallEvent::Finished);
            },
        ),
    )
    .await?;

    let _ = on_event.send(AppUpdateInstallEvent::Installing);
    #[cfg(windows)]
    crate::runtime_service::stop_service(&app)
        .await
        .map_err(AppUpdateError::RuntimeShutdown)?;
    #[cfg(windows)]
    update
        .install(bytes)
        .map_err(|error| AppUpdateError::InstallationAfterShutdown(error.to_string()))?;
    #[cfg(not(windows))]
    update.install(bytes)?;
    #[cfg(not(windows))]
    crate::commands::request_app_restart_shutdown(app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stalled_update_requests_expire_and_release_the_request() {
        use std::sync::atomic::{AtomicBool, Ordering};

        struct DropSignal<'a>(&'a AtomicBool);
        impl Drop for DropSignal<'_> {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }

        for download in [false, true] {
            let dropped = AtomicBool::new(false);
            let signal = DropSignal(&dropped);
            let request = async move {
                let _signal = signal;
                std::future::pending::<Result<(), tauri_plugin_updater::Error>>().await
            };
            let timeout_error = if download {
                AppUpdateError::DownloadTimedOut
            } else {
                AppUpdateError::CheckTimedOut
            };
            let result = tokio::time::timeout(
                Duration::from_secs(1),
                with_update_timeout(Duration::ZERO, timeout_error, request),
            )
            .await
            .expect("a stalled update request must finish within its deadline");

            if download {
                assert!(matches!(result, Err(AppUpdateError::DownloadTimedOut)));
            } else {
                assert!(matches!(result, Err(AppUpdateError::CheckTimedOut)));
            }
            assert!(
                dropped.load(Ordering::SeqCst),
                "the request must be cancelled"
            );

            let retry = with_update_timeout(
                Duration::from_secs(1),
                AppUpdateError::CheckTimedOut,
                std::future::ready(Ok(42)),
            )
            .await;
            assert_eq!(retry.expect("a later request must remain usable"), 42);
        }
    }

    #[tokio::test]
    async fn update_request_failure_preserves_the_original_error() {
        let error = with_update_timeout(
            Duration::from_secs(1),
            AppUpdateError::DownloadTimedOut,
            std::future::ready(Err::<(), _>(tauri_plugin_updater::Error::Network(
                "connection interrupted".into(),
            ))),
        )
        .await
        .expect_err("the failed download must not reach installation");
        assert!(matches!(
            error,
            AppUpdateError::Updater(tauri_plugin_updater::Error::Network(message))
                if message == "connection interrupted"
        ));
    }

    #[test]
    fn no_pending_update_error_is_user_readable() {
        let error = AppUpdateError::NoPendingUpdate;
        assert_eq!(
            error.to_string(),
            "no pending application update is ready to install"
        );
    }

    #[test]
    fn download_progress_event_reports_content_length() {
        let event = AppUpdateInstallEvent::Started {
            content_length: Some(2048),
        };
        let json = serde_json::to_string(&event).expect("serialize event");
        assert!(json.contains("started"));
        assert!(json.contains("content_length"));
        assert!(json.contains("2048"));
    }
}
