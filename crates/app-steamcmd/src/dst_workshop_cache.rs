use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use thiserror::Error;

use crate::dst_workshop_cache_fs::{
    CopyStats, canonical_plain_directory, copy_tree, create_stage, ensure_plain_directory,
    inspect_tree, require_plain_file,
};
use crate::dst_workshop_cache_publish::PublishStage;
use crate::dst_workshop_cache_vdf::{KvEntry, KvValue, parse_document, render_document};

const DST_APP_ID: &str = "322330";
const MANIFEST_NAME: &str = "appworkshop_322330.acf";
const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;
static DEPLOY_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DstWorkshopCacheDeployment {
    pub item_ids: Vec<String>,
    pub file_count: u64,
    pub byte_count: u64,
    pub unchanged: bool,
}

#[derive(Debug, Error)]
pub enum DstWorkshopCacheError {
    #[error("invalid DST Workshop item id `{item_id}`")]
    InvalidItemId { item_id: String },
    #[error("unsafe Workshop cache path: {path}")]
    UnsafePath { path: PathBuf },
    #[error("invalid Workshop manifest {path}: {message}")]
    InvalidManifest { path: PathBuf, message: String },
    #[error("incomplete Workshop source item {item_id}: {message}")]
    IncompleteSource { item_id: String, message: String },
    #[error("failed to {operation} {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Workshop cache deployment lock is unavailable")]
    LockUnavailable,
    #[error("Workshop cache rollback failed; preserved recovery data at {stage}: {message}")]
    RollbackFailed { stage: PathBuf, message: String },
}

#[derive(Clone)]
struct SourceItem {
    id: String,
    manifest: String,
    installed: KvEntry,
    details: KvEntry,
    source_path: PathBuf,
    stats: CopyStats,
}

pub fn deploy_dst_workshop_cache(
    source_workshop_root: &Path,
    target_ugc_root: &Path,
    item_ids: &[String],
) -> Result<DstWorkshopCacheDeployment, DstWorkshopCacheError> {
    let _guard = DEPLOY_LOCK
        .lock()
        .map_err(|_| DstWorkshopCacheError::LockUnavailable)?;
    let item_ids = normalized_ids(item_ids)?;
    if item_ids.is_empty() {
        return Ok(DstWorkshopCacheDeployment {
            item_ids,
            file_count: 0,
            byte_count: 0,
            unchanged: true,
        });
    }

    let source_root = canonical_plain_directory(source_workshop_root)?;
    let source_manifest_path = source_root.join(MANIFEST_NAME);
    let source_document = read_manifest(&source_manifest_path)?;
    let source_app = app_workshop(&source_document, &source_manifest_path)?;
    require_app_id(source_app, &source_manifest_path)?;

    let mut source_items = Vec::with_capacity(item_ids.len());
    for id in &item_ids {
        source_items.push(load_source_item(&source_root, source_app, id)?);
    }
    let totals = source_items
        .iter()
        .fold(CopyStats::default(), |mut sum, item| {
            sum.files += item.stats.files;
            sum.bytes += item.stats.bytes;
            sum
        });

    if target_ugc_root.exists() && canonical_plain_directory(target_ugc_root)? == source_root {
        return Ok(deployment(item_ids, totals, true));
    }

    let target_manifest_path = target_ugc_root.join(MANIFEST_NAME);
    let target_exists = target_manifest_path.exists();
    let mut target_document = if target_exists {
        let document = read_manifest(&target_manifest_path)?;
        require_app_id(
            app_workshop(&document, &target_manifest_path)?,
            &target_manifest_path,
        )?;
        document
    } else {
        new_manifest_document()
    };

    let mut unchanged_items = HashSet::new();
    if target_exists {
        let target_app = app_workshop(&target_document, &target_manifest_path)?;
        for item in &source_items {
            if target_item_is_current(target_ugc_root, target_app, item)? {
                unchanged_items.insert(item.id.clone());
            }
        }
    }
    merge_items(&mut target_document, &target_manifest_path, &source_items)?;
    let rendered_manifest = render_document(&target_document);
    let manifest_unchanged =
        target_exists && read_file(&target_manifest_path)? == rendered_manifest.as_bytes();
    if unchanged_items.len() == source_items.len() && manifest_unchanged {
        return Ok(deployment(item_ids, totals, true));
    }

    ensure_plain_directory(target_ugc_root)?;
    let content_root = target_ugc_root.join("content").join(DST_APP_ID);
    ensure_plain_directory(&content_root)?;
    let stage = create_stage(target_ugc_root)?;
    let staged_content = stage.join("content").join(DST_APP_ID);
    fs::create_dir_all(&staged_content)
        .map_err(|source| io("create staging directory", &staged_content, source))?;

    let staged_manifest = stage.join(MANIFEST_NAME);
    let stage_result = (|| {
        for item in &source_items {
            if unchanged_items.contains(&item.id) {
                continue;
            }
            let destination = staged_content.join(&item.id);
            fs::create_dir(&destination)
                .map_err(|source| io("create staged item", &destination, source))?;
            let copied = copy_tree(&item.source_path, &destination, 0)?;
            if copied.files != item.stats.files || copied.bytes != item.stats.bytes {
                return Err(DstWorkshopCacheError::IncompleteSource {
                    item_id: item.id.clone(),
                    message: String::from("payload changed while it was copied"),
                });
            }
        }
        fs::write(&staged_manifest, rendered_manifest.as_bytes())
            .map_err(|source| io("write staged manifest", &staged_manifest, source))?;
        Ok(())
    })();
    if let Err(error) = stage_result {
        let _ = fs::remove_dir_all(&stage);
        return Err(error);
    }

    let publish_result = PublishStage {
        stage: &stage,
        staged_content: &staged_content,
        staged_manifest: &staged_manifest,
        content_root: &content_root,
        target_manifest: &target_manifest_path,
        item_ids: source_items
            .iter()
            .filter(|item| !unchanged_items.contains(&item.id))
            .map(|item| item.id.as_str())
            .collect(),
        manifest_unchanged,
    }
    .publish();
    if let Err(error) = publish_result {
        if !matches!(error, DstWorkshopCacheError::RollbackFailed { .. }) {
            let _ = fs::remove_dir_all(&stage);
        }
        return Err(error);
    }
    fs::remove_dir_all(&stage).map_err(|source| io("remove staging directory", &stage, source))?;
    Ok(deployment(item_ids, totals, false))
}

fn deployment(ids: Vec<String>, stats: CopyStats, unchanged: bool) -> DstWorkshopCacheDeployment {
    DstWorkshopCacheDeployment {
        item_ids: ids,
        file_count: stats.files,
        byte_count: stats.bytes,
        unchanged,
    }
}

fn normalized_ids(ids: &[String]) -> Result<Vec<String>, DstWorkshopCacheError> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    for raw in ids {
        let id = raw.trim();
        if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) || id == "0" {
            return Err(DstWorkshopCacheError::InvalidItemId {
                item_id: raw.clone(),
            });
        }
        if seen.insert(id.to_owned()) {
            result.push(id.to_owned());
        }
    }
    Ok(result)
}

fn load_source_item(
    source_root: &Path,
    source_app: &[KvEntry],
    id: &str,
) -> Result<SourceItem, DstWorkshopCacheError> {
    let installed = section_entry(source_app, "WorkshopItemsInstalled", id)
        .cloned()
        .ok_or_else(|| incomplete(id, "missing WorkshopItemsInstalled metadata"))?;
    let details = section_entry(source_app, "WorkshopItemDetails", id)
        .cloned()
        .ok_or_else(|| incomplete(id, "missing WorkshopItemDetails metadata"))?;
    let installed_object = object_value(&installed)
        .ok_or_else(|| incomplete(id, "installed metadata is not an object"))?;
    let details_object =
        object_value(&details).ok_or_else(|| incomplete(id, "detail metadata is not an object"))?;
    let manifest = text_value(installed_object, "manifest")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| incomplete(id, "missing installed manifest id"))?;
    if text_value(details_object, "manifest") != Some(manifest) {
        return Err(incomplete(id, "installed and detail manifest ids differ"));
    }
    let expected_size = text_value(installed_object, "size")
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| incomplete(id, "missing or invalid installed size"))?;
    let source_path = source_root.join("content").join(DST_APP_ID).join(id);
    let stats = inspect_tree(&source_path, 0)?;
    if stats.files == 0 || stats.bytes != expected_size {
        return Err(incomplete(
            id,
            &format!(
                "payload size {} does not match official metadata {expected_size}",
                stats.bytes
            ),
        ));
    }
    Ok(SourceItem {
        id: id.to_owned(),
        manifest: manifest.to_owned(),
        installed,
        details,
        source_path,
        stats,
    })
}

fn target_item_is_current(
    target_root: &Path,
    target_app: &[KvEntry],
    source: &SourceItem,
) -> Result<bool, DstWorkshopCacheError> {
    let Some(installed) =
        section_entry(target_app, "WorkshopItemsInstalled", &source.id).and_then(object_value)
    else {
        return Ok(false);
    };
    if text_value(installed, "manifest") != Some(source.manifest.as_str()) {
        return Ok(false);
    }
    let target_path = target_root
        .join("content")
        .join(DST_APP_ID)
        .join(&source.id);
    if !target_path.exists() {
        return Ok(false);
    }
    let stats = inspect_tree(&target_path, 0)?;
    Ok(stats.files == source.stats.files && stats.bytes == source.stats.bytes)
}

fn merge_items(
    document: &mut [KvEntry],
    path: &Path,
    source_items: &[SourceItem],
) -> Result<(), DstWorkshopCacheError> {
    let app = app_workshop_mut(document, path)?;
    for (section, select) in [
        ("WorkshopItemsInstalled", false),
        ("WorkshopItemDetails", true),
    ] {
        let entries = ensure_object(app, section)?;
        for item in source_items {
            let replacement = if select {
                &item.details
            } else {
                &item.installed
            };
            if let Some(existing) = entries.iter_mut().find(|entry| entry.key == item.id) {
                *existing = replacement.clone();
            } else {
                entries.push(replacement.clone());
            }
        }
    }
    Ok(())
}

fn read_manifest(path: &Path) -> Result<Vec<KvEntry>, DstWorkshopCacheError> {
    require_plain_file(path)?;
    let metadata = fs::metadata(path).map_err(|source| io("read metadata", path, source))?;
    if metadata.len() > MAX_MANIFEST_BYTES {
        return Err(invalid_manifest(path, "manifest exceeds 16 MiB"));
    }
    let bytes = read_file(path)?;
    let text =
        String::from_utf8(bytes).map_err(|_| invalid_manifest(path, "manifest is not UTF-8"))?;
    parse_document(&text).map_err(|message| invalid_manifest(path, &message))
}

fn read_file(path: &Path) -> Result<Vec<u8>, DstWorkshopCacheError> {
    fs::read(path).map_err(|source| io("read file", path, source))
}

fn app_workshop<'a>(
    document: &'a [KvEntry],
    path: &Path,
) -> Result<&'a [KvEntry], DstWorkshopCacheError> {
    document
        .iter()
        .find(|entry| entry.key == "AppWorkshop")
        .and_then(object_value)
        .ok_or_else(|| invalid_manifest(path, "missing AppWorkshop object"))
}

fn app_workshop_mut<'a>(
    document: &'a mut [KvEntry],
    path: &Path,
) -> Result<&'a mut Vec<KvEntry>, DstWorkshopCacheError> {
    document
        .iter_mut()
        .find(|entry| entry.key == "AppWorkshop")
        .and_then(|entry| match &mut entry.value {
            KvValue::Object(value) => Some(value),
            KvValue::Text(_) => None,
        })
        .ok_or_else(|| invalid_manifest(path, "missing AppWorkshop object"))
}

fn require_app_id(app: &[KvEntry], path: &Path) -> Result<(), DstWorkshopCacheError> {
    if text_value(app, "appid") != Some(DST_APP_ID) {
        return Err(invalid_manifest(path, "appid is not 322330"));
    }
    Ok(())
}

fn section_entry<'a>(app: &'a [KvEntry], section: &str, id: &str) -> Option<&'a KvEntry> {
    app.iter()
        .find(|entry| entry.key == section)
        .and_then(object_value)?
        .iter()
        .find(|entry| entry.key == id)
}

fn object_value(entry: &KvEntry) -> Option<&[KvEntry]> {
    match &entry.value {
        KvValue::Object(value) => Some(value),
        KvValue::Text(_) => None,
    }
}

fn text_value<'a>(entries: &'a [KvEntry], key: &str) -> Option<&'a str> {
    entries
        .iter()
        .find(|entry| entry.key == key)
        .and_then(|entry| match &entry.value {
            KvValue::Text(value) => Some(value.as_str()),
            KvValue::Object(_) => None,
        })
}

fn ensure_object<'a>(
    entries: &'a mut Vec<KvEntry>,
    key: &str,
) -> Result<&'a mut Vec<KvEntry>, DstWorkshopCacheError> {
    if !entries.iter().any(|entry| entry.key == key) {
        entries.push(KvEntry {
            key: key.to_owned(),
            value: KvValue::Object(Vec::new()),
        });
    }
    let Some(entry) = entries.iter_mut().find(|entry| entry.key == key) else {
        return Err(invalid_manifest(
            Path::new(MANIFEST_NAME),
            &format!("failed to create {key} object"),
        ));
    };
    match &mut entry.value {
        KvValue::Object(value) => Ok(value),
        KvValue::Text(_) => Err(invalid_manifest(
            Path::new(MANIFEST_NAME),
            &format!("{key} is not an object"),
        )),
    }
}

fn new_manifest_document() -> Vec<KvEntry> {
    vec![KvEntry {
        key: String::from("AppWorkshop"),
        value: KvValue::Object(vec![
            KvEntry {
                key: String::from("appid"),
                value: KvValue::Text(String::from(DST_APP_ID)),
            },
            KvEntry {
                key: String::from("SizeOnDisk"),
                value: KvValue::Text(String::from("0")),
            },
            KvEntry {
                key: String::from("NeedsUpdate"),
                value: KvValue::Text(String::from("0")),
            },
            KvEntry {
                key: String::from("NeedsDownload"),
                value: KvValue::Text(String::from("0")),
            },
            KvEntry {
                key: String::from("WorkshopItemsInstalled"),
                value: KvValue::Object(Vec::new()),
            },
            KvEntry {
                key: String::from("WorkshopItemDetails"),
                value: KvValue::Object(Vec::new()),
            },
        ]),
    }]
}

fn incomplete(id: &str, message: &str) -> DstWorkshopCacheError {
    DstWorkshopCacheError::IncompleteSource {
        item_id: id.to_owned(),
        message: message.to_owned(),
    }
}
fn invalid_manifest(path: &Path, message: &str) -> DstWorkshopCacheError {
    DstWorkshopCacheError::InvalidManifest {
        path: path.to_path_buf(),
        message: message.to_owned(),
    }
}
fn io(operation: &'static str, path: &Path, source: std::io::Error) -> DstWorkshopCacheError {
    DstWorkshopCacheError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}
