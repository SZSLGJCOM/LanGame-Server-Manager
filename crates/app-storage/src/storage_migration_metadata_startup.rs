use super::*;
use crate::startup_sessions::{self, NativeStartupSource};

pub(super) fn prepare(
    root: &Path,
    logical_root: &Path,
    instance_id: &str,
    moves: &[RootMove],
    copied: bool,
    plan: &mut MetadataPlan,
) -> Result<(), StorageError> {
    let path = root.join("logs/startup-session.json");
    if !exists(&path)? {
        return Ok(());
    }
    let before = read_bytes(&path, startup_sessions::MAX_RECEIPT_BYTES)?;
    let logical_root = normalize_resource_path(logical_root)?;
    let receipt = startup_sessions::parse_for_relocation(&before, &logical_root, instance_id)
        .map_err(|error| invalid(&path, error.to_string()))?;
    let mut value: Value = serde_json::from_slice(&before)?;
    // Startup reads compare this owned path against the canonical instance root.
    // A copied archive's physical directory is not its future restored root.
    value["receipt"]["source_path"] = Value::String(
        remap_path(Path::new(&receipt.source_path), moves)?
            .to_string_lossy()
            .into_owned(),
    );
    for (index, source) in receipt.native_sources.iter().enumerate() {
        value["receipt"]["native_sources"][index] = serde_json::to_value(rebind_native_source(
            source,
            root,
            &logical_root,
            moves,
            copied,
        )?)?;
    }
    let after = serde_json::to_vec_pretty(&value)?;
    startup_sessions::parse_for_relocation(&after, &remap_path(&logical_root, moves)?, instance_id)
        .map_err(|error| invalid(&path, error.to_string()))?;
    add_json_patch(&path, &before, &value, moves, copied, plan)
}

fn rebind_native_source(
    source: &NativeStartupSource,
    physical_root: &Path,
    logical_root: &Path,
    moves: &[RootMove],
    copied: bool,
) -> Result<NativeStartupSource, StorageError> {
    let logical_path = Path::new(&source.source_path);
    let mapped = remap_stored_path(logical_path, moves)?;
    // Native readers use persisted instance paths, including configured aliases.
    // Keep the same spelling as those rows, and retain unmoved paths unchanged.
    let target = if same_path(logical_path, &mapped)? {
        logical_path.to_owned()
    } else {
        mapped
    };
    let mut rebound = source.clone();
    rebound.source_path = target.to_string_lossy().into_owned();
    if !copied {
        return Ok(rebound);
    }
    let physical = remap_path(
        logical_path,
        &[RootMove {
            source: logical_root.to_owned(),
            target: physical_root.to_owned(),
        }],
    )?;
    let copied_path = remap_path(&physical, moves)?;
    if same_path(&physical, &copied_path)? {
        return Ok(rebound);
    }
    // Historical native logs may have rotated or disappeared. Keep their old
    // identity invalid instead of certifying a replacement as the earlier run.
    if !exists(&physical)? {
        return Ok(rebound);
    }
    let mut file =
        native::open_verified_file(&physical, false).map_err(|source| StorageError::ReadPath {
            path: physical.clone(),
            source,
        })?;
    let offset = source
        .validated_offset(logical_path, file.reader(), source.process_creation_time)
        .map_err(|error| invalid(&physical, error.to_string()))?;
    let Some(offset) = offset else {
        return Ok(rebound);
    };
    console::verify_copy(&physical, &copied_path)?;
    let mut copied_file = native::open_verified_file(&copied_path, false).map_err(|source| {
        StorageError::ReadPath {
            path: copied_path.clone(),
            source,
        }
    })?;
    NativeStartupSource::capture(
        source.process_key.clone(),
        &target,
        source.process_creation_time,
        copied_file.reader(),
        offset,
    )
    .map_err(|error| invalid(&copied_path, error.to_string()))
}
