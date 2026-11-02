use super::*;
use crate::instance_archive_store::{self as store, Archive, Snapshot};

pub(super) fn prepare(
    paths: &StoragePaths,
    moves: &[RootMove],
    database: &db::Database,
    copied: bool,
    plan: &mut MetadataPlan,
) -> Result<(), StorageError> {
    for archive in &database.archives {
        let before = row(archive);
        let mut after = before.clone();
        for field in ["original_root", "preserved_external_saves_path"] {
            let value = after
                .get_mut(field)
                .ok_or_else(|| invalid(&paths.database_path, "Missing archive path field."))?;
            rewrite_path(value, moves, &paths.database_path)?;
        }
        if archive.state == "archived" {
            let mut snapshot = store::snapshot(archive)?;
            let physical = owned::archive_path(paths, &archive.leaf)?;
            let original = PathBuf::from(
                archive
                    .original_root
                    .as_deref()
                    .ok_or_else(|| invalid(&physical, "Missing original archive root."))?,
            );
            let id = archive
                .instance_id
                .as_deref()
                .ok_or_else(|| invalid(&physical, "Archive instance identity is missing."))?;
            let module = archive
                .module_id
                .as_deref()
                .ok_or_else(|| invalid(&physical, "Archive module identity is missing."))?;
            let allow_missing = snapshot
                .external_program
                .as_ref()
                .is_some_and(|program| !program.requires_source());
            documents::instance(
                documents::InstanceContext {
                    root: &physical,
                    logical_root: &original,
                    id,
                    module,
                    allow_missing_program: allow_missing,
                },
                moves,
                copied,
                plan,
            )?;
            for record in snapshot.tables.get("instances").into_iter().flatten() {
                for field in ["env_json", "args_json"] {
                    let value: Value = serde_json::from_str(row_text(record, field, &physical)?)?;
                    preflight::reject_references(&value, &physical, field, moves)?;
                }
            }
            rebind_snapshot_cursors(&mut snapshot, &original, &physical, moves, copied)?;
            db::rewrite_saved_rows(&mut snapshot.tables, moves, &physical)?;
            if let Some(path) = &mut snapshot.effective_saves_path {
                let mut value = Value::String(path.clone());
                rewrite_path(&mut value, moves, &physical)?;
                *path = value
                    .as_str()
                    .ok_or_else(|| invalid(&physical, "Invalid archive save path."))?
                    .to_owned();
            }
            if snapshot.external_program.is_some() {
                external_program(&mut snapshot, &physical, module, moves, copied, plan)?;
            }
            for (field, source, expected) in [
                (
                    "directory_identity_json",
                    &physical,
                    archive.identity.as_deref(),
                ),
                (
                    "instances_root_identity_json",
                    &paths.instances_root,
                    archive.instances_identity.as_deref(),
                ),
                (
                    "archive_parent_identity_json",
                    &paths.archives_root,
                    archive.parent_identity.as_deref(),
                ),
            ] {
                let observed = identity(source, true)?;
                let next =
                    rebind_identity(source, expected.unwrap_or(&observed), true, moves, copied)?;
                after.insert(field.into(), Value::String(next));
            }
            let serialized = serde_json::to_string(&snapshot)?;
            if serialized.len() > store::MAX_SNAPSHOT_BYTES {
                return Err(invalid(
                    &physical,
                    "Migrated archive snapshot exceeds its metadata limit.",
                ));
            }
            after.insert(
                "snapshot_sha256".into(),
                Value::String(digest(serialized.as_bytes())),
            );
            after.insert("snapshot_json".into(), Value::String(serialized));
        }
        if copied && before != after {
            plan.rows.push(SqlPatch {
                table: "instance_archives".into(),
                key_column: "archive_id".into(),
                key_value: Value::String(archive.id.clone()),
                before,
                after,
            });
        }
    }
    Ok(())
}

fn row(archive: &Archive) -> BTreeMap<String, Value> {
    BTreeMap::from([
        (
            "instance_id".into(),
            Value::from(archive.instance_id.clone()),
        ),
        ("module_id".into(), Value::from(archive.module_id.clone())),
        (
            "original_root".into(),
            Value::from(archive.original_root.clone()),
        ),
        ("archive_leaf".into(), Value::String(archive.leaf.clone())),
        (
            "directory_identity_json".into(),
            Value::from(archive.identity.clone()),
        ),
        (
            "snapshot_json".into(),
            Value::from(archive.snapshot.clone()),
        ),
        (
            "snapshot_sha256".into(),
            Value::from(archive.snapshot_hash.clone()),
        ),
        (
            "preserved_external_saves_path".into(),
            Value::from(archive.external_saves.clone()),
        ),
        ("state".into(), Value::String(archive.state.clone())),
        ("purpose".into(), Value::String(archive.purpose.clone())),
        (
            "instances_root_identity_json".into(),
            Value::from(archive.instances_identity.clone()),
        ),
        (
            "archive_parent_identity_json".into(),
            Value::from(archive.parent_identity.clone()),
        ),
        (
            "restore_staging_identity_json".into(),
            Value::from(archive.restore_staging_identity.clone()),
        ),
    ])
}

fn external_program(
    snapshot: &mut Snapshot,
    physical: &Path,
    module: &str,
    moves: &[RootMove],
    copied: bool,
    plan: &mut MetadataPlan,
) -> Result<(), StorageError> {
    let external = snapshot
        .external_program
        .as_mut()
        .ok_or_else(|| invalid(physical, "Archive external program is missing."))?;
    if external.restore_identity.is_some() {
        return Err(invalid(
            physical,
            "Finish the archive program restoration before migrating storage.",
        ));
    }
    let source_root = external.root.clone();
    if exists(&source_root)? {
        external.identity = rebind_identity(&source_root, &external.identity, true, moves, copied)?;
    } else if external.requires_source() {
        return Err(invalid(
            &source_root,
            "The archive's required exact program source is missing.",
        ));
    }
    let parent = source_root
        .parent()
        .ok_or_else(|| invalid(&source_root, "Archive program has no parent."))?;
    external.parent_identity =
        rebind_identity(parent, &external.parent_identity, true, moves, copied)?;
    if let Some(expected) = &external.payload_identity {
        external.payload_identity = Some(rebind_identity(
            &physical.join(".langame-archived-program"),
            expected,
            true,
            moves,
            copied,
        )?);
    }
    let binding = physical.join("runtime").join(if external.exclusive {
        ".langame-exclusive-program.json"
    } else {
        ".langame-shared-program.json"
    });
    if copied {
        let target = remap_path(&binding, moves)?;
        if let Some(patch) = plan.files.iter().find(|patch| patch.path == target) {
            external.binding_sha256 = digest(&patch.after);
            external.binding_bytes = patch.after.len() as u64;
        }
    }
    for (index, (relative, file)) in external.files.iter_mut().enumerate() {
        if !is_path_marker(relative) {
            continue;
        }
        if !file.stored {
            return Err(invalid(
                &source_root.join(relative),
                "An archive omits management metadata required for path rebinding.",
            ));
        }
        let slot = physical
            .join(".langame-archived-program")
            .join(format!("{index:06}"));
        let (before, mut value) = read_json(&slot)?;
        if digest(&before) != file.sha256 || before.len() as u64 != file.bytes {
            return Err(invalid(
                &slot,
                "Archived management metadata differs from its protected snapshot.",
            ));
        }
        rewrite_captured_marker(
            &mut value,
            relative,
            &source_root,
            module,
            &slot,
            moves,
            copied,
        )?;
        add_json_patch(&slot, &before, &value, moves, copied, plan)?;
        if copied {
            let target = remap_path(&slot, moves)?;
            if let Some(patch) = plan.files.iter().find(|patch| patch.path == target) {
                file.sha256 = digest(&patch.after);
                file.bytes = patch.after.len() as u64;
            }
        }
    }
    external.fingerprint = digest(&serde_json::to_vec(&(
        &external.files,
        &external.directories,
    ))?);
    if moved(&source_root, moves)? {
        external.root = remap_stored_path(&source_root, moves)?;
    }
    external.validate()?;
    Ok(())
}

fn is_path_marker(relative: &str) -> bool {
    matches!(
        relative,
        ".langame-program-acquisition.json"
            | ".langame-retained-library.json"
            | ".langame-shared-program.json"
            | ".langame-exclusive-program.json"
            | ".langame-package-baseline.json"
            | ".langame-runtime-projection.json"
    )
}

fn rewrite_captured_marker(
    value: &mut Value,
    name: &str,
    root: &Path,
    module: &str,
    slot: &Path,
    moves: &[RootMove],
    copied: bool,
) -> Result<(), StorageError> {
    let version = if name == ".langame-package-baseline.json" {
        2
    } else {
        1
    };
    if value.get("version").and_then(Value::as_u64) != Some(version) {
        return Err(invalid(
            slot,
            "Unsupported archived program metadata version.",
        ));
    }
    match name {
        ".langame-program-acquisition.json" | ".langame-retained-library.json" => {
            let field = if name == ".langame-program-acquisition.json" {
                "target"
            } else {
                "program_root"
            };
            if text(value, "module_id", slot)? != module
                || !same_path(Path::new(text(value, field, slot)?), root)?
            {
                return Err(invalid(
                    slot,
                    "Archived program ownership marker does not match its original installation.",
                ));
            }
            if field == "target" && uuid::Uuid::parse_str(text(value, "id", slot)?).is_err() {
                return Err(invalid(
                    slot,
                    "Archived program acquisition identity is invalid.",
                ));
            }
            rewrite_field(value, field, moves, slot)?;
        }
        ".langame-shared-program.json" | ".langame-exclusive-program.json" => {
            if text(value, "module_id", slot)? != module
                || text(value, "program_id", slot)?.is_empty()
            {
                return Err(invalid(
                    slot,
                    "Archived program reference identity is invalid.",
                ));
            }
            rewrite_field(value, "program_root", moves, slot)?;
        }
        ".langame-runtime-projection.json" => {
            rewrite_field(value, "instance_root", moves, slot)?;
            rewrite_field(value, "source_root", moves, slot)?;
        }
        ".langame-package-baseline.json" => {
            let old = PathBuf::from(text(value, "source_root", slot)?);
            if moved(&old, moves)? {
                if !exists(&old)? {
                    return Err(invalid(
                        slot,
                        "An archived package baseline source is missing; it cannot be rebound safely.",
                    ));
                }
                if copied {
                    let target = remap_path(&old, moves)?;
                    let canonical =
                        fs::canonicalize(&target).map_err(|source| StorageError::ReadPath {
                            path: target,
                            source,
                        })?;
                    value["source_root"] = Value::String(canonical.to_string_lossy().into_owned());
                }
            }
        }
        _ => {
            return Err(invalid(
                slot,
                "Unsupported archived management path marker.",
            ));
        }
    }
    Ok(())
}

fn rebind_snapshot_cursors(
    snapshot: &mut Snapshot,
    original: &Path,
    physical: &Path,
    moves: &[RootMove],
    copied: bool,
) -> Result<(), StorageError> {
    let mut logs = BTreeMap::new();
    for run in snapshot.tables.get("instance_runs").into_iter().flatten() {
        if let (Some(id), Some(path)) = (
            run.get("id").and_then(Value::as_i64),
            run.get("log_path").and_then(Value::as_str),
        ) {
            let physical_log = remap_path(
                Path::new(path),
                &[RootMove {
                    source: original.to_owned(),
                    target: physical.to_owned(),
                }],
            )?;
            logs.insert(id, physical_log);
        }
    }
    for cursor in snapshot
        .tables
        .get_mut("player_history_cursors")
        .into_iter()
        .flatten()
    {
        let Some(expected) = cursor
            .get("source_identity")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            continue;
        };
        let id = cursor
            .get("run_id")
            .and_then(Value::as_i64)
            .ok_or_else(|| invalid(physical, "Archive history cursor has no run ID."))?;
        let Some(source) = logs.get(&id) else {
            continue;
        };
        if !moved(source, moves)? {
            continue;
        }
        if !exists(source)? {
            if cursor.get("completed").and_then(Value::as_i64) == Some(1) {
                continue;
            }
            return Err(invalid(
                source,
                "An archived unfinished history cursor refers to a missing log.",
            ));
        }
        let next = rebind_identity(source, &expected, false, moves, copied)?;
        if copied {
            console::verify_copy(source, &remap_path(source, moves)?)?;
            cursor.insert("source_identity".into(), Value::String(next));
        }
    }
    Ok(())
}
