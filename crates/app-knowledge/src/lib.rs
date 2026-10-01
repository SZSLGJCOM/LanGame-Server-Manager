//! Locally cached publisher documentation and learned multilingual retrieval.
//! URLs come from the shipped source catalog, never from model tool arguments.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use sqlx::SqlitePool;
use tokio::sync::Mutex;
static MODEL_LOAD_SLOTS: std::sync::OnceLock<Arc<tokio::sync::Semaphore>> =
    std::sync::OnceLock::new();

mod content_policy;
pub mod embedding;
mod embedding_runtime;
pub use content_policy::ContentUse;
mod extract;
mod fetch;
mod indexing;
#[cfg(test)]
mod live_tests;
pub mod pdf_worker;
mod retrieval_rank;
mod search;
pub mod sources;
mod store;
#[cfg(test)]
mod store_tests;
mod sync;
mod types;
mod zendesk;
pub use types::*;

#[cfg(test)]
mod knowledge_input_tests;

#[cfg(test)]
mod sync_tests;

#[cfg(test)]
mod discovery_tests;

#[cfg(test)]
mod embedding_benchmark_tests;

#[cfg(test)]
mod embedding_integration_tests;

pub type Result<T> = std::result::Result<T, KnowledgeError>;

#[derive(Debug, thiserror::Error)]
pub enum KnowledgeError {
    #[error("Invalid knowledge input: {0}")]
    Invalid(String),
    #[error("Knowledge unavailable: {0}")]
    Unavailable(String),
    #[error("Knowledge network request failed: {0}")]
    Network(String),
    #[error("Publisher documentation policy: {0}")]
    Policy(String),
    #[error("Knowledge embedding model: {0}")]
    Model(String),
    #[error("Knowledge synchronization was cancelled")]
    Cancelled,
    #[error("Knowledge file operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Knowledge database operation failed: {0}")]
    Database(#[from] sqlx::Error),
}

pub fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Acquire) {
        Err(KnowledgeError::Cancelled)
    } else {
        Ok(())
    }
}

pub fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub struct KnowledgeLibrary {
    root: PathBuf,
    modules_root: PathBuf,
    pool: SqlitePool,
    encoder: Mutex<Option<Arc<embedding::Embedder>>>,
    sync_gate: Mutex<()>,
}

impl KnowledgeLibrary {
    pub async fn open(root: &Path, modules_root: &Path) -> Result<Self> {
        let pool = store::open(root).await?;
        Ok(Self {
            root: root.to_owned(),
            modules_root: modules_root.to_owned(),
            pool,
            encoder: Mutex::new(None),
            sync_gate: Mutex::new(()),
        })
    }

    pub async fn close(&self) {
        self.pool.close().await;
        self.encoder.lock().await.take();
    }

    async fn encoder(&self) -> Result<Arc<embedding::Embedder>> {
        let mut current = self.encoder.lock().await;
        if let Some(encoder) = current.as_ref() {
            return Ok(encoder.clone());
        }
        let path = self.root.join("model").join(embedding::MODEL_SLUG);
        let permit = MODEL_LOAD_SLOTS
            .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(1)))
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                KnowledgeError::Unavailable("The local embedding model is still loading".into())
            })?;
        let model = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            embedding::Embedder::load(&path)
        })
        .await
        .map_err(|error| KnowledgeError::Model(error.to_string()))??;
        let model = Arc::new(model);
        *current = Some(model.clone());
        Ok(model)
    }
}
