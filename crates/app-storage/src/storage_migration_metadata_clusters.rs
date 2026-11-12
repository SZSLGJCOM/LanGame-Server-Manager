//! Rebind snapshot member destinations through the existing migration journal.
//! The snapshot store and external transfer directory do not move, so payload
//! entries, checksums, membership and the cluster storage key stay unchanged.
use super::*;

pub(super) fn prepare(
    paths: &StoragePaths,
    moves: &[RootMove],
    copied: bool,
    plan: &mut MetadataPlan,
) -> Result<(), StorageError> {
    let root = paths.app_data_root.join("cluster-backups");
    if !exists(&root)? {
        return Ok(());
    }
    let mut pending = vec![(root.clone(), 0usize)];
    let mut count = 0;
    while let Some((directory, depth)) = pending.pop() {
        if !owned::plain_directory(&directory)? {
            continue;
        }
        for entry in fs::read_dir(&directory).map_err(|source| StorageError::ReadPath {
            path: directory.clone(),
            source,
        })? {
            let path = entry
                .map_err(|source| StorageError::ReadPath {
                    path: directory.clone(),
                    source,
                })?
                .path();
            count += 1;
            if count > 100_000 {
                return Err(invalid(
                    &root,
                    "Too many ARK cluster snapshot entries to validate migration.",
                ));
            }
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if name == "restore-pending.json" {
                return Err(invalid(
                    &path,
                    "Complete the pending ARK cluster restore before moving storage.",
                ));
            }
            if name == "manifest.json" {
                prepare_snapshot(paths, &path, moves, copied, plan)?;
            } else if depth < 2
                && fs::symlink_metadata(&path)
                    .map_err(|source| StorageError::ReadPath {
                        path: path.clone(),
                        source,
                    })?
                    .is_dir()
            {
                pending.push((path, depth + 1));
            }
        }
    }
    Ok(())
}

fn prepare_snapshot(
    paths: &StoragePaths,
    path: &Path,
    moves: &[RootMove],
    copied: bool,
    plan: &mut MetadataPlan,
) -> Result<(), StorageError> {
    let (before, mut value) = read_json(path)?;
    let scopes = value
        .get("scopes")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid(path, "ARK cluster snapshot scopes are invalid."))?;
    let mut affected = false;
    for scope in scopes {
        affected |= moved(Path::new(text(scope, "target", path)?), moves)?;
    }
    if !affected {
        return Ok(());
    }
    crate::ark_cluster_backups::validate_snapshot_for_storage_migration(paths, path, &value)?;
    let directory = path.parent().expect("validated snapshot directory");
    for scope in value["scopes"]
        .as_array_mut()
        .expect("validated snapshot scopes")
    {
        let key = text(scope, "key", path)?;
        if key == "transfer" {
            if moved(Path::new(text(scope, "target", path)?), moves)? {
                return Err(invalid(
                    path,
                    "ARK transfer directories must remain outside managed storage roots.",
                ));
            }
            continue;
        }
        if key.ends_with("-config") {
            let config = directory.join(key).join("instance.json");
            let (_, settings) = read_json(&config)?;
            preflight::reject_references(&settings, &config, "snapshot configuration", moves)?;
        }
        rewrite_field(scope, "target", moves, path)?;
    }
    add_json_patch(path, &before, &value, moves, copied, plan)
}
