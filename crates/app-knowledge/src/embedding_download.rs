use std::future::Future;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use reqwest::{Client, Url, redirect::Policy};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{CHECKPOINT_REVISION, MODEL_ID};
use crate::{KnowledgeError, Result, check_cancel};

pub(super) struct ModelFile {
    pub(super) name: &'static str,
    pub(super) remote_name: &'static str,
    pub(super) size: u64,
    pub(super) sha256: &'static str,
}

pub(super) const FILES: [ModelFile; 3] = [
    ModelFile {
        name: "model.onnx",
        remote_name: "onnx/model_quint8_avx2.onnx",
        size: 98_247_878,
        sha256: "a6022dd8220ea6f6595562a1328ee216f4a94faa55362f2f4747c80f1e78772e",
    },
    ModelFile {
        name: "tokenizer.json",
        remote_name: "tokenizer.json",
        size: 25_301_672,
        sha256: "4f2842d568e2724370aec203652a42ac783c7937f8347a1a2cc7506d71f1582f",
    },
    ModelFile {
        name: "config.json",
        remote_name: "config.json",
        size: 1_216,
        sha256: "de948b0bdc6f356afad7a84b276d8dd7e7fe10fb9add1bb5e610621c28e41ebc",
    },
];
pub const DOWNLOAD_BYTES: u64 = 123_550_766;
const LICENSE: &[u8] = include_bytes!("embedding_license.txt");
const LICENSE_NAME: &str = "LICENSE.txt";
static DOWNLOAD_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Cheap availability indication. `load` and `ensure_model` verify every byte;
/// status polling must not hash the complete model on each UI refresh.
pub fn installed(model_dir: &Path) -> bool {
    FILES
        .iter()
        .all(|spec| regular_file_size(&model_dir.join(spec.name)) == Some(spec.size))
        && regular_file_size(&model_dir.join(LICENSE_NAME)) == Some(LICENSE.len() as u64)
}

fn regular_file_size(path: &Path) -> Option<u64> {
    std::fs::symlink_metadata(path)
        .ok()
        .filter(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        .map(|metadata| metadata.len())
}

pub(super) fn read_verified(model_dir: &Path, spec: &ModelFile) -> Result<Vec<u8>> {
    let path = model_dir.join(spec.name);
    if regular_file_size(&path) != Some(spec.size) {
        return Err(KnowledgeError::Model(format!(
            "Missing or invalid {}",
            spec.name
        )));
    }
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::with_capacity(spec.size as usize);
    file.take(spec.size + 1).read_to_end(&mut bytes)?;
    validate_digest(
        spec,
        bytes.len() as u64,
        &Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
    )?;
    Ok(bytes)
}

pub(super) fn verify_license(model_dir: &Path) -> Result<()> {
    let path = model_dir.join(LICENSE_NAME);
    if regular_file_size(&path) != Some(LICENSE.len() as u64) || std::fs::read(path)? != LICENSE {
        return Err(KnowledgeError::Model(
            "Missing or invalid model license".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_digest(spec: &ModelFile, size: u64, digest: &str) -> Result<()> {
    if size != spec.size || digest != spec.sha256 {
        return Err(KnowledgeError::Model(format!(
            "Integrity check failed for {}",
            spec.name
        )));
    }
    Ok(())
}

pub(super) fn allowed_model_url(url: &Url) -> bool {
    // Exact official download hosts from https://huggingface.co/.well-known/meta.json
    // (2026-10-01), alongside official LFS bridge hosts.
    // Hub aliases are not alternate model origins; do not admit arbitrary subdomains.
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && matches!(
            url.host_str(),
            Some(
                "huggingface.co"
                    | "cdn-lfs.huggingface.co"
                    | "cdn-lfs.hf.co"
                    | "cdn-lfs-us-1.huggingface.co"
                    | "cdn-lfs-eu-1.huggingface.co"
                    | "cas-bridge.xethub.hf.co"
                    | "cdn-lfs-us-1.hf.co"
                    | "cdn-lfs-eu-1.hf.co"
                    | "transfer.xethub.hf.co"
                    | "transfer.xethub-eu.hf.co"
                    | "aws.cdn.hf.co"
                    | "us.aws.cdn.hf.co"
                    | "eu.aws.cdn.hf.co"
                    | "us-east-1.aws.cdn.hf.co"
                    | "us-west-2.aws.cdn.hf.co"
                    | "eu-west-3.aws.cdn.hf.co"
                    | "ap-southeast-1.aws.cdn.hf.co"
                    | "us.gcp.cdn.hf.co"
                    | "us-east1.us.gcp.cdn.hf.co"
                    | "us-central1.us.gcp.cdn.hf.co"
                    | "us-west4.us.gcp.cdn.hf.co"
                    | "europe-west4.us.gcp.cdn.hf.co"
                    | "asia-southeast1.us.gcp.cdn.hf.co"
            )
        )
}

fn model_client() -> Result<Client> {
    Client::builder()
        .user_agent("LanGame-Official-Knowledge/1")
        .https_only(true)
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(600))
        .redirect(Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 || !allowed_model_url(attempt.url()) {
                attempt.error("Embedding download redirect is not an approved HTTPS model host")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(network_error)
}

fn network_error(error: reqwest::Error) -> KnowledgeError {
    KnowledgeError::Network(error.without_url().to_string())
}

async fn cancellable<T>(cancel: &AtomicBool, future: impl Future<Output = Result<T>>) -> Result<T> {
    check_cancel(cancel)?;
    tokio::pin!(future);
    loop {
        tokio::select! {
            result = &mut future => { check_cancel(cancel)?; return result; }
            _ = tokio::time::sleep(Duration::from_millis(100)) => check_cancel(cancel)?,
        }
    }
}

/// Downloads public, immutable weights. No credentials, model code, or executables
/// are read or invoked. One bounded in-process download owns the temporary files.
pub async fn ensure_model(
    model_dir: &Path,
    cancel: &AtomicBool,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<()> {
    let work = async {
        let _guard = cancellable(cancel, async { Ok(DOWNLOAD_GATE.lock().await) }).await?;
        tokio::fs::create_dir_all(model_dir).await?;
        let client = model_client()?;
        let mut completed = 0;
        progress(0, DOWNLOAD_BYTES);
        for spec in &FILES {
            check_cancel(cancel)?;
            if !existing_valid(model_dir, spec, cancel).await? {
                for attempt in 0..3 {
                    let result =
                        download_one(&client, model_dir, spec, completed, cancel, progress).await;
                    match result {
                        Ok(()) => break,
                        Err(KnowledgeError::Network(_)) if attempt < 2 => {
                            cancellable(cancel, async {
                                let jitter = u64::from(uuid::Uuid::new_v4().as_bytes()[0]);
                                tokio::time::sleep(Duration::from_millis(
                                    (1000 << attempt) + jitter,
                                ))
                                .await;
                                Ok(())
                            })
                            .await?;
                        }
                        Err(error) => return Err(error),
                    }
                }
            }
            completed += spec.size;
            progress(completed, DOWNLOAD_BYTES);
        }
        check_cancel(cancel)?;
        publish_license(model_dir).await?;
        Ok(())
    };
    tokio::time::timeout(Duration::from_secs(30 * 60), cancellable(cancel, work))
        .await
        .map_err(|_| {
            KnowledgeError::Network("Embedding download exceeded its 30 minute budget".into())
        })?
}

async fn existing_valid(model_dir: &Path, spec: &ModelFile, cancel: &AtomicBool) -> Result<bool> {
    let path = model_dir.join(spec.name);
    let metadata = match tokio::fs::symlink_metadata(&path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(KnowledgeError::Model(format!(
            "{} is not a regular model file",
            spec.name
        )));
    }
    if metadata.len() != spec.size {
        return Ok(false);
    }
    let mut file = tokio::fs::File::open(path).await?;
    let mut digest = Sha256::new();
    let mut bytes = 0;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        check_cancel(cancel)?;
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        bytes += read as u64;
        if bytes > spec.size {
            return Ok(false);
        }
        digest.update(&buffer[..read]);
    }
    Ok(validate_digest(
        spec,
        bytes,
        &digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
    )
    .is_ok())
}

struct PartialFile(PathBuf);

impl PartialFile {
    async fn create(model_dir: &Path, name: &str) -> Result<(std::fs::File, Self)> {
        let path = model_dir.join(format!(".{name}.{}.part", uuid::Uuid::new_v4()));
        // Creation and guard construction share the blocking task. If its
        // awaiter is cancelled during open, dropping the returned tuple closes
        // the handle before removing the newly created file.
        tokio::task::spawn_blocking(move || {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            Ok((file, Self(path)))
        })
        .await
        .map_err(|error| KnowledgeError::Model(error.to_string()))?
    }
}

impl Drop for PartialFile {
    fn drop(&mut self) {
        // The async file handle is declared after this guard and closes first.
        let _ = std::fs::remove_file(&self.0);
    }
}

async fn download_one(
    client: &Client,
    model_dir: &Path,
    spec: &ModelFile,
    completed: u64,
    cancel: &AtomicBool,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<()> {
    let url = format!(
        "https://huggingface.co/{MODEL_ID}/resolve/{CHECKPOINT_REVISION}/{}",
        spec.remote_name
    );
    let mut response = request_model_file(client, &url, cancel).await?;
    if response
        .content_length()
        .is_some_and(|length| length != spec.size)
    {
        return Err(KnowledgeError::Model(format!(
            "Unexpected download length for {}",
            spec.name
        )));
    }
    let (file, partial) = PartialFile::create(model_dir, spec.name).await?;
    let mut file = tokio::fs::File::from_std(file);
    let mut bytes = 0;
    let mut digest = Sha256::new();
    while let Some(chunk) = cancellable(cancel, async {
        response.chunk().await.map_err(network_error)
    })
    .await?
    {
        bytes += chunk.len() as u64;
        if bytes > spec.size {
            return Err(KnowledgeError::Model(format!(
                "Oversized download for {}",
                spec.name
            )));
        }
        file.write_all(&chunk).await?;
        digest.update(&chunk);
        progress(completed + bytes, DOWNLOAD_BYTES);
    }
    validate_digest(
        spec,
        bytes,
        &digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
    )?;
    check_cancel(cancel)?;
    file.flush().await?;
    file.sync_all().await?;
    drop(file);
    tokio::fs::rename(&partial.0, model_dir.join(spec.name)).await?;
    Ok(())
}

pub(super) async fn request_model_file(
    client: &Client,
    url: &str,
    cancel: &AtomicBool,
) -> Result<reqwest::Response> {
    cancellable(cancel, async {
        let request = client
            .get(url)
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .build()
            .map_err(|error| KnowledgeError::Unavailable(error.without_url().to_string()))?;
        app_network::send_with_retry(client, request, Duration::from_secs(45))
            .await
            // Header retries belong to the shared policy. Its final failures,
            // including server deferral, must not restart the outer body loop.
            .map_err(|error| KnowledgeError::Unavailable(error.to_string()))
    })
    .await
}

async fn publish_license(model_dir: &Path) -> Result<()> {
    let (file, partial) = PartialFile::create(model_dir, LICENSE_NAME).await?;
    let mut file = tokio::fs::File::from_std(file);
    file.write_all(LICENSE).await?;
    file.flush().await?;
    file.sync_all().await?;
    drop(file);
    tokio::fs::rename(&partial.0, model_dir.join(LICENSE_NAME)).await?;
    Ok(())
}
