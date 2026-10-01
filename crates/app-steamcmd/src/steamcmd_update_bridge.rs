//! A short-lived, loopback-only endpoint lets the native bootstrapper consume
//! the exact signed manifest already fetched over HTTPS by our bounded client.
//! Packages stay in its SHA-256-verified cache; no remote proxy is exposed.
use std::path::Path;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

use super::ownership::configured_steamcmd_ownership;
use super::steamcmd_update_cache::update_error;
use super::steamcmd_update_manifest::parse_manifest;
use super::{InstallDeadline, SteamCmdError, SteamCmdOwnership, operation_timeout};

const MANIFEST_LIMIT: usize = 64 * 1024;

pub(super) struct UpdateBridge {
    pub url: String,
    task: Option<JoinHandle<()>>,
}

impl UpdateBridge {
    pub(super) async fn for_managed_root(
        root: &Path,
        deadline: InstallDeadline,
    ) -> Result<Option<Self>, SteamCmdError> {
        deadline.check_cancelled()?;
        if configured_steamcmd_ownership(root) != SteamCmdOwnership::Managed {
            return Ok(None);
        }
        // Preparation already fetched and applied this manifest. Keep native
        // runscript launches on that version instead of repeating a remote
        // bootstrap check after the preparation endpoint has closed.
        let manifest = read_cached_manifest(root, deadline).await?;
        deadline.check_cancelled()?;
        Self::start(manifest).await.map(Some)
    }

    pub(super) async fn start(manifest: String) -> Result<Self, SteamCmdError> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|error| {
                update_error(format!("Cannot open SteamCMD update endpoint: {error}"))
            })?;
        let address = listener.local_addr().map_err(|error| {
            update_error(format!("Cannot inspect SteamCMD update endpoint: {error}"))
        })?;
        let task = tokio::spawn(async move {
            // One native attempt and its fresh verification need only a few
            // requests. Bound both connection count and per-connection work.
            for _ in 0..64 {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let _ =
                    tokio::time::timeout(Duration::from_secs(3), respond(stream, &manifest)).await;
            }
        });
        Ok(Self {
            url: format!("http://{address}"),
            task: Some(task),
        })
    }

    pub(super) async fn close(mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
            let _ = task.await;
        }
    }
}

async fn read_cached_manifest(
    root: &Path,
    deadline: InstallDeadline,
) -> Result<String, SteamCmdError> {
    let cache = root.join("package");
    let path = cache.join("steam_cmd_win64.manifest");
    for (entry, directory) in [(&cache, true), (&path, false)] {
        let metadata = deadline
            .run(tokio::fs::symlink_metadata(entry))
            .await
            .map_err(|_| operation_timeout(deadline))?
            .map_err(|error| {
                update_error(format!("Cannot inspect cached SteamCMD manifest: {error}"))
            })?;
        if metadata.file_type().is_symlink()
            || (directory && !metadata.is_dir())
            || (!directory && (!metadata.is_file() || metadata.len() > MANIFEST_LIMIT as u64))
        {
            return Err(update_error(
                "SteamCMD manifest cache has an invalid file type or size",
            ));
        }
    }
    deadline.check_cancelled()?;
    let mut file = tokio::fs::File::open(&path)
        .await
        .map_err(|error| update_error(format!("Cannot read cached SteamCMD manifest: {error}")))?;
    let result = async {
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            deadline.check_cancelled()?;
            let count = deadline
                .run(file.read(&mut buffer))
                .await
                .map_err(|_| operation_timeout(deadline))?
                .map_err(|error| {
                    update_error(format!("Cannot read cached SteamCMD manifest: {error}"))
                })?;
            if count == 0 {
                break;
            }
            if bytes.len() + count > MANIFEST_LIMIT {
                return Err(update_error(
                    "Cached SteamCMD manifest exceeds the 64 KiB limit",
                ));
            }
            bytes.extend_from_slice(&buffer[..count]);
        }
        let manifest = String::from_utf8(bytes)
            .map_err(|_| update_error("Cached SteamCMD manifest is not valid UTF-8"))?;
        parse_manifest(&manifest, "win64").map_err(update_error)?;
        deadline.check_cancelled()?;
        Ok(manifest)
    }
    .await;
    // A cancelled Tokio read can still own a blocking file worker on Windows.
    // Settle it before releasing the operation and its file ownership.
    drop(file.into_std().await);
    result
}

impl Drop for UpdateBridge {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn respond(mut stream: TcpStream, manifest: &str) -> std::io::Result<()> {
    let mut header = Vec::new();
    let mut buffer = [0_u8; 512];
    while !header.windows(4).any(|window| window == b"\r\n\r\n") {
        let count = stream.read(&mut buffer).await?;
        if count == 0 || header.len() + count > 4096 {
            return Ok(());
        }
        header.extend_from_slice(&buffer[..count]);
    }
    let request = header
        .split(|byte| *byte == b'\n')
        .next()
        .unwrap_or_default();
    let valid = request == b"GET /steam_cmd_win64 HTTP/1.1\r"
        || request == b"GET /steam_cmd_win64 HTTP/1.0\r";
    let (status, body) = if valid {
        ("200 OK", manifest)
    } else {
        ("404 Not Found", "")
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len(),
    );
    stream.write_all(response.as_bytes()).await?;
    stream.write_all(body.as_bytes()).await?;
    stream.shutdown().await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_manifest() -> String {
        format!(
            "\"win64\" {{ \"version\" \"123\" \"steamcmd_win64\" {{ \"file\" \"steamcmd_win64.zip.hash\" \"size\" \"12\" \"sha2\" \"{}\" \"IsBootstrapperPackage\" \"1\" }} }}\r\n",
            "a".repeat(64),
        )
    }

    fn temporary_root() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "langame-update-bridge-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ))
    }

    fn deadline() -> InstallDeadline {
        InstallDeadline::new("update bridge test", Duration::from_secs(10))
    }

    #[tokio::test]
    async fn managed_commands_serve_the_validated_cached_manifest() {
        let root = temporary_root();
        crate::prepare_configured_steamcmd_root(&root).unwrap();
        std::fs::create_dir(root.join("package")).unwrap();
        let path = root.join("package/steam_cmd_win64.manifest");
        let manifest = fixture_manifest();
        std::fs::write(&path, &manifest).unwrap();
        let bridge = UpdateBridge::for_managed_root(&root, deadline())
            .await
            .unwrap()
            .unwrap();
        // Opening the bridge must not retain a cache handle or depend on a
        // subsequent file read while the native process is using its manifest.
        std::fs::remove_file(&path).unwrap();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let url = format!("{}/steam_cmd_win64", bridge.url);
        assert_eq!(
            client.get(&url).send().await.unwrap().text().await.unwrap(),
            manifest
        );
        bridge.close().await;
        assert!(client.get(url).send().await.is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn external_commands_do_not_require_a_managed_manifest() {
        let root = temporary_root();
        assert!(
            UpdateBridge::for_managed_root(&root, deadline())
                .await
                .unwrap()
                .is_none()
        );
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("steamcmd.exe"), b"external fixture").unwrap();
        assert!(
            UpdateBridge::for_managed_root(&root, deadline())
                .await
                .unwrap()
                .is_none()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn managed_manifest_rejects_missing_malformed_oversized_and_non_file_cache() {
        let root = temporary_root();
        crate::prepare_configured_steamcmd_root(&root).unwrap();
        std::fs::create_dir(root.join("package")).unwrap();
        let path = root.join("package/steam_cmd_win64.manifest");
        assert!(
            UpdateBridge::for_managed_root(&root, deadline())
                .await
                .is_err()
        );
        for bytes in [
            b"invalid manifest".to_vec(),
            vec![b' '; MANIFEST_LIMIT + 1],
            vec![0xff],
        ] {
            std::fs::write(&path, bytes).unwrap();
            assert!(
                UpdateBridge::for_managed_root(&root, deadline())
                    .await
                    .is_err()
            );
        }
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(
            UpdateBridge::for_managed_root(&root, deadline())
                .await
                .is_err()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn cancelled_cached_manifest_read_preserves_the_manifest() {
        let root = temporary_root();
        crate::prepare_configured_steamcmd_root(&root).unwrap();
        std::fs::create_dir(root.join("package")).unwrap();
        let path = root.join("package/steam_cmd_win64.manifest");
        let manifest = fixture_manifest();
        std::fs::write(&path, &manifest).unwrap();
        let token = crate::InstallCancellation::new();
        token.cancel();
        let result = token
            .scope(UpdateBridge::for_managed_root(&root, deadline()))
            .await;
        assert!(matches!(
            result,
            Err(SteamCmdError::InstallCancelled { .. })
        ));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), manifest);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn serves_exact_manifest_only_and_closes_listener() {
        let manifest = "\"win64\" { \"version\" \"123\" }\r\n";
        let bridge = UpdateBridge::start(manifest.into()).await.unwrap();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let url = bridge.url.clone();
        let response = client
            .get(format!("{url}/steam_cmd_win64"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.text().await.unwrap(), manifest);
        for path in [
            "/steam_cmd_win32",
            "/steam_cmd_win64?url=example",
            "/steamcmd_test.zip",
            "/",
        ] {
            assert_eq!(
                client
                    .get(format!("{url}{path}"))
                    .send()
                    .await
                    .unwrap()
                    .status(),
                404
            );
        }
        bridge.close().await;
        assert!(
            client
                .get(format!("{url}/steam_cmd_win64"))
                .send()
                .await
                .is_err()
        );
    }
}
