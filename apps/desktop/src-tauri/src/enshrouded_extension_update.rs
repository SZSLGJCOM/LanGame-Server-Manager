//! Opportunistic cache-only refresh. A live instance is never an update target.
use std::{
    path::PathBuf,
    sync::{Arc, LazyLock},
    time::Duration,
};

use super::*;

const RELEASES: &str = "https://github.com/SZSLGJCOM/LanGame-Server-Manager/releases/download/";
const FEED: &str = "https://github.com/SZSLGJCOM/LanGame-Server-Manager/releases/download/enshrouded-control/manifest.json";
static UPDATE_LOCK: LazyLock<Arc<tokio::sync::Mutex<()>>> =
    LazyLock::new(|| Arc::new(tokio::sync::Mutex::new(())));
type UpdateLease = Arc<tokio::sync::OwnedMutexGuard<()>>;

pub(crate) enum RefreshResult {
    Cached { version: String },
    Unsupported,
}

/// Does not await, queue behind other updates, or receive a runtime directory.
/// Later normal instance starts may check again after this attempt finishes.
pub(crate) fn start_refresh(
    cache_root: PathBuf,
    executable: String,
    finished: impl FnOnce(Result<RefreshResult, String>) + Send + 'static,
) -> bool {
    let Ok(guard) = UPDATE_LOCK.clone().try_lock_owned() else {
        return false;
    };
    tokio::spawn(async move {
        let lease = Arc::new(guard);
        let result = tokio::time::timeout(
            Duration::from_secs(20),
            refresh(cache_root, executable, &lease),
        )
        .await
        .unwrap_or_else(|_| {
            Err("Background extension update timed out; the running server is unchanged.".into())
        });
        // Logging and all signature/cache I/O stay off the async executor.
        let _ = tokio::task::spawn_blocking(move || finished(result)).await;
    });
    true
}

async fn blocking<T: Send + 'static>(
    lease: &UpdateLease,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let lease = lease.clone();
    // A timed-out async caller cannot cancel blocking I/O. Retain its update
    // lease in the worker, so another refresh cannot race a late cache write.
    tokio::task::spawn_blocking(move || {
        let _lease = lease;
        work()
    })
    .await
    .map_err(|_| "The background extension cache worker failed.".to_owned())?
}

async fn refresh(
    cache_root: PathBuf,
    executable: String,
    lease: &UpdateLease,
) -> Result<RefreshResult, String> {
    let directory = cache_root.join("cache/enshrouded-control");
    let (existing, minimum_revision) = {
        let directory = directory.clone();
        let executable = executable.clone();
        blocking(lease, move || cached(&directory, &executable, &key()?)).await?
    };
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(8))
        .user_agent("LanGame-Enshrouded-Control/1")
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let url = attempt.url();
            if attempt.previous().len() < 4
                && url.scheme() == "https"
                && url.username().is_empty()
                && url.password().is_none()
                && url.port().is_none()
                && matches!(
                    url.host_str(),
                    Some("github.com" | "release-assets.githubusercontent.com")
                )
            {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
        .map_err(|_| "Cannot create the extension updater client.")?;
    let proof_url = format!("{FEED}.minisig");
    let (bytes, proof) = tokio::try_join!(
        download(&client, FEED, MAX_MANIFEST),
        download(&client, &proof_url, MAX_SIGNATURE)
    )?;
    let (bytes, proof, manifest) = {
        let directory = directory.clone();
        blocking(lease, move || {
            let manifest = admit_manifest(&bytes, &proof, &key()?, minimum_revision)?;
            remember_manifest(&directory, &bytes, &proof)?;
            Ok((bytes, proof, manifest))
        })
        .await?
    };
    if !manifest
        .profiles
        .iter()
        .any(|p| p.executable_sha256 == executable)
    {
        return Ok(RefreshResult::Unsupported);
    }
    if existing
        .as_ref()
        .is_some_and(|package| package.manifest_bytes == bytes)
    {
        return Ok(RefreshResult::Cached {
            version: manifest.version,
        });
    }
    let url = format!(
        "{RELEASES}enshrouded-control-v{}/{}",
        manifest.version, manifest.dll.asset_name
    );
    let proof_url = format!("{url}.minisig");
    let (dll, dll_proof) = tokio::try_join!(
        download(&client, &url, manifest.dll.size),
        download(&client, &proof_url, MAX_SIGNATURE)
    )?;
    blocking(lease, move || {
        let package = verify_package(bytes, proof, dll, dll_proof, &executable, &key()?)?;
        cache(&directory, &package)?;
        Ok(RefreshResult::Cached {
            version: package.manifest.version,
        })
    })
    .await
}

async fn download(client: &reqwest::Client, url: &str, limit: usize) -> Result<Vec<u8>, String> {
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|_| "Extension update source is unavailable.")?
        .error_for_status()
        .map_err(|_| "Extension update source returned an error.")?;
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err("Extension update download exceeds its size limit.".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Extension update download was interrupted.")?
    {
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err("Extension update download exceeds its size limit.".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn concurrent_start_skips_an_active_check_without_queueing_another() {
        let guard = UPDATE_LOCK.clone().lock_owned().await;
        assert!(!start_refresh(
            PathBuf::from("unused"),
            "f".repeat(64),
            |_| panic!("a skipped check must not run or invoke its callback"),
        ));
        drop(guard);
        // The gate does not remember attempts permanently. A later normal
        // start may check for a newly published compatible package.
        assert!(UPDATE_LOCK.clone().try_lock_owned().is_ok());
    }

    #[tokio::test]
    async fn cancelling_a_background_wait_cannot_release_a_cache_write_lease_early() {
        let gate = Arc::new(tokio::sync::Mutex::new(()));
        let lease = Arc::new(gate.clone().lock_owned().await);
        let (entered, observed) = tokio::sync::oneshot::channel();
        let (release, blocked) = std::sync::mpsc::sync_channel(1);
        let waiting = tokio::spawn(async move {
            blocking(&lease, move || {
                let _ = entered.send(());
                blocked.recv().map_err(|error| error.to_string())?;
                Ok(())
            })
            .await
        });
        observed.await.unwrap();
        waiting.abort();
        let _ = waiting.await;
        assert!(gate.try_lock().is_err());
        release.send(()).unwrap();
        let _guard = tokio::time::timeout(Duration::from_secs(5), gate.lock())
            .await
            .expect("the cache lease should end when its worker finishes");
    }
}
