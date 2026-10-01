use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    MediaCacheError, OBJECT_LIMIT,
    meta::{Blob, Entry},
};

const MARKER: &str = ".lgsm-media-cache";
const INDEX_LIMIT: u64 = 8 * 1024 * 1024;

pub(super) fn digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        result.push(HEX[usize::from(byte >> 4)] as char);
        result.push(HEX[usize::from(byte & 15)] as char);
    }
    result
}
fn error(value: impl std::fmt::Display) -> MediaCacheError {
    MediaCacheError::Storage(value.to_string())
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Index {
    entries: BTreeMap<String, Entry>,
}

pub(super) struct Store {
    root: PathBuf,
    index: Mutex<Index>,
    disk_limit: u64,
    entry_limit: usize,
    last_touch_flush: AtomicU64,
}

impl Store {
    pub fn open(
        root: PathBuf,
        disk_limit: u64,
        entry_limit: usize,
    ) -> Result<Arc<Self>, MediaCacheError> {
        std::fs::create_dir_all(&root).map_err(error)?;
        if std::fs::symlink_metadata(&root)
            .map_err(error)?
            .file_type()
            .is_symlink()
        {
            return Err(error("cache root must not be a link"));
        }
        let root = root.canonicalize().map_err(error)?;
        let marker = root.join(MARKER);
        if !marker.exists() {
            if std::fs::read_dir(&root).map_err(error)?.next().is_some() {
                return Err(error(
                    "refusing to use an unmarked nonempty media cache directory",
                ));
            }
            std::fs::write(&marker, b"LGSM media cache 1\n").map_err(error)?;
        }
        if regular_file(&marker)?.is_none()
            || std::fs::read(&marker).map_err(error)? != b"LGSM media cache 1\n"
        {
            return Err(error("invalid media cache ownership marker"));
        }
        let index_path = root.join("index.json");
        let mut index: Index = match regular_file(&index_path)? {
            Some(length) if length <= INDEX_LIMIT => {
                serde_json::from_slice(&std::fs::read(&index_path).map_err(error)?)
                    .unwrap_or_default()
            }
            _ => Index::default(),
        };
        index.entries.retain(|key, entry| {
            valid_hash(key)
                && entry.blocks.len() <= 2048
                && entry.blobs().all(|blob| {
                    valid_blob(&blob.file)
                        && blob.file.starts_with(&format!("blob-{key}-"))
                        && valid_hash(&blob.sha256)
                        && blob.size <= OBJECT_LIMIT as u64
                })
        });
        let store = Arc::new(Self {
            root,
            index: Mutex::new(index),
            disk_limit,
            entry_limit,
            last_touch_flush: AtomicU64::new(0),
        });
        {
            let mut index = store.index.lock().map_err(error)?;
            store.evict(&mut index)?;
            store.save(&index)?;
            store.cleanup(&index)?;
        }
        Ok(store)
    }

    pub async fn get(
        self: &Arc<Self>,
        key: &str,
        now: u64,
    ) -> Result<Option<Entry>, MediaCacheError> {
        let (store, key) = (Arc::clone(self), key.to_string());
        tokio::task::spawn_blocking(move || {
            let mut index = store.index.lock().map_err(error)?;
            let entry = index.entries.get_mut(&key).map(|entry| {
                entry.accessed = now;
                entry.clone()
            });
            // Exact LRU lives in memory; an approximate restart order needs at
            // most one durable touch per minute, not an fsync for every block.
            if entry.is_some()
                && now.saturating_sub(store.last_touch_flush.load(Ordering::Relaxed)) >= 60
            {
                store.save(&index)?;
                store.last_touch_flush.store(now, Ordering::Relaxed);
            }
            Ok(entry)
        })
        .await
        .map_err(error)?
    }

    pub async fn read(self: &Arc<Self>, blob: &Blob) -> Result<Option<Vec<u8>>, MediaCacheError> {
        let (store, blob) = (Arc::clone(self), blob.clone());
        tokio::task::spawn_blocking(move || {
            if !valid_blob(&blob.file) || blob.size > OBJECT_LIMIT as u64 {
                return Ok(None);
            }
            let path = store.root.join(&blob.file);
            if regular_file(&path)? != Some(blob.size) {
                return Ok(None);
            }
            let bytes = std::fs::read(&path).map_err(error)?;
            Ok((digest(&bytes) == blob.sha256).then_some(bytes))
        })
        .await
        .map_err(error)?
    }

    pub async fn put(
        self: &Arc<Self>,
        key: &str,
        mut entry: Entry,
        bodies: Vec<(Option<u64>, Vec<u8>)>,
    ) -> Result<Entry, MediaCacheError> {
        if entry.freshness.no_store {
            self.remove(key).await?;
            return Ok(entry);
        }
        let (store, key) = (Arc::clone(self), key.to_string());
        tokio::task::spawn_blocking(move || {
            let mut index = store.index.lock().map_err(error)?;
            let mut writes = Vec::new();
            for (offset, bytes) in bodies {
                if bytes.len() > OBJECT_LIMIT {
                    return Err(MediaCacheError::TooLarge);
                }
                let sha256 = digest(&bytes);
                let slot = offset
                    .map(|value| format!("{value:x}"))
                    .unwrap_or_else(|| "full".into());
                let file = format!("blob-{key}-{slot}-{sha256}.bin");
                let blob = Blob {
                    file: file.clone(),
                    sha256,
                    size: bytes.len() as u64,
                };
                if let Some(offset) = offset {
                    entry.blocks.insert(offset, blob);
                } else {
                    entry.full = Some(blob);
                }
                writes.push((file, bytes));
            }
            let mut planned = index.clone();
            let mut obsolete = planned
                .entries
                .insert(key.clone(), entry.clone())
                .into_iter()
                .flat_map(|entry| entry.blobs().cloned().collect::<Vec<_>>())
                .collect::<Vec<_>>();
            obsolete.extend(store.evict(&mut planned)?);
            // Publish a complete transaction; a failed write leaves the old
            // index usable, and any newly published orphan is removed here.
            let published = (|| {
                if planned.entries.contains_key(&key) {
                    for (file, bytes) in &writes {
                        atomic_write(&store.root, file, bytes)?;
                    }
                }
                store.save(&planned)
            })();
            if let Err(error) = published {
                store.remove_obsolete(&index, entry.blobs().cloned())?;
                return Err(error);
            }
            *index = planned;
            store
                .last_touch_flush
                .store(entry.accessed, Ordering::Relaxed);
            store.remove_obsolete(&index, obsolete)?;
            Ok(entry)
        })
        .await
        .map_err(error)?
    }

    pub async fn remove(self: &Arc<Self>, key: &str) -> Result<(), MediaCacheError> {
        let (store, key) = (Arc::clone(self), key.to_string());
        tokio::task::spawn_blocking(move || {
            let mut index = store.index.lock().map_err(error)?;
            let Some(entry) = index.entries.remove(&key) else {
                return Ok(());
            };
            store.save(&index)?;
            store.remove_obsolete(&index, entry.blobs().cloned())
        })
        .await
        .map_err(error)?
    }

    fn evict(&self, index: &mut Index) -> Result<Vec<Blob>, MediaCacheError> {
        let mut obsolete = Vec::new();
        let mut bytes = index
            .entries
            .values()
            .flat_map(Entry::blobs)
            .map(|blob| blob.size)
            .sum::<u64>();
        // Include JSON framing and keys; this conservative estimate prevents
        // oversized metadata from turning every later cache write into an error.
        let sizes = index
            .entries
            .iter()
            .map(|(key, entry)| {
                serde_json::to_vec(entry).map(|value| (key.clone(), value.len() as u64 + 68))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()
            .map_err(error)?;
        let mut metadata = 16 + sizes.values().sum::<u64>();
        while bytes > self.disk_limit
            || index.entries.len() > self.entry_limit
            || metadata > INDEX_LIMIT
        {
            let Some(key) = index
                .entries
                .iter()
                .min_by_key(|(key, entry)| (entry.accessed, *key))
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some(entry) = index.entries.remove(&key) {
                bytes = bytes.saturating_sub(entry.blobs().map(|blob| blob.size).sum());
                metadata = metadata.saturating_sub(sizes.get(&key).copied().unwrap_or(0));
                obsolete.extend(entry.blobs().cloned());
            }
        }
        Ok(obsolete)
    }

    fn save(&self, index: &Index) -> Result<(), MediaCacheError> {
        let bytes = serde_json::to_vec(index).map_err(error)?;
        if bytes.len() as u64 > INDEX_LIMIT {
            return Err(error("media cache index exceeds its bound"));
        }
        atomic_write(&self.root, "index.json", &bytes)
    }

    fn cleanup(&self, index: &Index) -> Result<(), MediaCacheError> {
        let live = index
            .entries
            .values()
            .flat_map(Entry::blobs)
            .map(|blob| blob.file.as_str())
            .collect::<HashSet<_>>();
        for (count, file) in std::fs::read_dir(&self.root).map_err(error)?.enumerate() {
            if count >= 20_000 {
                return Err(error("media cache directory exceeds its entry bound"));
            }
            let file = file.map_err(error)?;
            let name = file.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if (valid_blob(name) && !live.contains(name))
                || (name.starts_with(".lgsm-media-") && name.ends_with(".tmp"))
            {
                // Only owned regular files inside the canonical marked root are removed.
                if file.file_type().map_err(error)?.is_file() {
                    std::fs::remove_file(file.path()).map_err(error)?;
                }
            }
        }
        Ok(())
    }

    fn remove_obsolete(
        &self,
        index: &Index,
        blobs: impl IntoIterator<Item = Blob>,
    ) -> Result<(), MediaCacheError> {
        let live = index
            .entries
            .values()
            .flat_map(Entry::blobs)
            .map(|blob| blob.file.as_str())
            .collect::<HashSet<_>>();
        for blob in blobs {
            if valid_blob(&blob.file) && !live.contains(blob.file.as_str()) {
                let path = self.root.join(&blob.file);
                if regular_file(&path)?.is_some() {
                    std::fs::remove_file(path).map_err(error)?;
                }
            }
        }
        Ok(())
    }
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn valid_blob(value: &str) -> bool {
    let parts = value.split('-').collect::<Vec<_>>();
    parts.len() == 4
        && parts[0] == "blob"
        && valid_hash(parts[1])
        && (parts[2] == "full"
            || (!parts[2].is_empty()
                && parts[2].len() <= 16
                && parts[2].bytes().all(|byte| byte.is_ascii_hexdigit())))
        && parts[3].strip_suffix(".bin").is_some_and(valid_hash)
}
fn regular_file(path: &Path) -> Result<Option<u64>, MediaCacheError> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => Ok(Some(meta.len())),
        Ok(_) => Err(error("cache object is not a regular file")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(value) => Err(error(value)),
    }
}
fn atomic_write(root: &Path, name: &str, bytes: &[u8]) -> Result<(), MediaCacheError> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let temporary = root.join(format!(
        ".lgsm-media-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(error)?;
        file.write_all(bytes).map_err(error)?;
        file.sync_all().map_err(error)?;
        drop(file);
        std::fs::rename(&temporary, root.join(name)).map_err(error)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}
