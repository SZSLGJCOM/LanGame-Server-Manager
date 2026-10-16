use super::server_plan::{self, Projection};
use super::*;

struct PreparedScope {
    target: PathBuf,
    original: Option<io::Inventory>,
    projection: Projection,
}

struct Prepared {
    scopes: Vec<PreparedScope>,
    guards: Vec<String>,
    fingerprint: String,
}

fn prepare(
    context: &Context,
    source: &Path,
    tree: &io::Inventory,
    plan: &ServerPlan,
) -> Result<Prepared, StorageError> {
    server_plan::validate(context, plan)?;
    let guards = server_plan::guards(plan)?;
    let mut scopes = Vec::new();
    for scope in &plan.scopes {
        let original = io::current_inventory(&scope.target)?;
        let projection = server_plan::project(scope, source, tree, original.as_ref())?;
        scopes.push(PreparedScope {
            target: scope.target.clone(),
            original,
            projection,
        });
    }
    if scopes
        .iter()
        .map(|scope| scope.projection.files)
        .sum::<u64>()
        == 0
    {
        return Err(invalid(source, "No native save files were selected."));
    }
    let targets: Vec<_> = scopes
        .iter()
        .map(|scope| scope.original.as_ref().map(|tree| &tree.digest))
        .collect();
    let fingerprint = io::digest(serde_json::to_vec(&(
        &context.record.summary.id,
        &context.record.summary.module_id,
        source,
        &context.settings_json,
        &context.settings,
        plan,
        &tree.digest,
        targets,
        &guards,
    ))?);
    Ok(Prepared {
        scopes,
        guards,
        fingerprint,
    })
}

pub(super) fn preview(
    context: &Context,
    source: &Path,
    tree: &io::Inventory,
    plan: ServerPlan,
) -> Result<InstanceSaveImportPreview, StorageError> {
    let prepared = prepare(context, source, tree, &plan)?;
    let mut changes = Vec::new();
    for scope in &prepared.scopes {
        changes.push(InstanceSaveImportChange {
            path: scope.target.to_string_lossy().into_owned(),
            file_count: scope.projection.files,
            total_bytes: scope.projection.bytes,
        });
    }
    Ok(InstanceSaveImportPreview {
        instance_id: context.record.summary.id.clone(),
        module_id: context.record.summary.module_id.clone(),
        source_path: source.to_string_lossy().into_owned(),
        target_path: context.target.to_string_lossy().into_owned(),
        file_count: prepared
            .scopes
            .iter()
            .map(|scope| scope.projection.files)
            .sum(),
        total_bytes: prepared
            .scopes
            .iter()
            .map(|scope| scope.projection.bytes)
            .sum(),
        existing_file_count: prepared
            .scopes
            .iter()
            .filter_map(|scope| scope.original.as_ref())
            .map(|tree| tree.files)
            .sum(),
        existing_total_bytes: prepared
            .scopes
            .iter()
            .filter_map(|scope| scope.original.as_ref())
            .map(|tree| tree.bytes)
            .sum(),
        fingerprint: prepared.fingerprint,
        warnings: plan.warnings,
        changes,
    })
}

struct Stage {
    target: PathBuf,
    staging: PathBuf,
    rollback: PathBuf,
    expected: io::Inventory,
    original: io::Inventory,
    recovery: RestoreRollback,
    cleanup: DirectoryCleanup,
    moved: bool,
    published: bool,
}

fn compensate(stages: &mut [Stage]) -> Vec<String> {
    let mut failures = Vec::new();
    for stage in stages.iter_mut().rev().filter(|stage| stage.moved) {
        let restored = (|| {
            if stage.published {
                move_directory(&stage.target, &stage.staging)?;
            }
            stage.recovery.restore()
        })();
        if let Err(error) = restored {
            // Retain any staged bytes if recovery cannot complete.
            stage.cleanup.disarm();
            failures.push(format!(
                "{}: {error}; recovery {}",
                stage.target.display(),
                stage.rollback.display()
            ));
        }
    }
    failures
}

pub(super) fn import(
    context: &Context,
    source: &Path,
    tree: &io::Inventory,
    plan: ServerPlan,
    expected: &str,
    publication: &publication::Publication,
    mut publish: impl FnMut(&Path, &Path) -> Result<(), StorageError>,
) -> Result<InstanceSaveImportResult, StorageError> {
    let prepared = prepare(context, source, tree, &plan)?;
    if expected.is_empty() || prepared.fingerprint != expected {
        return Err(invalid(
            source,
            "The source, current saves or Configuration changed after preview. Inspect the import again; no saves were replaced.",
        ));
    }
    let mut stages = Vec::new();
    let mut backups = Vec::new();
    let files = prepared
        .scopes
        .iter()
        .map(|scope| scope.projection.files)
        .sum();
    let bytes = prepared
        .scopes
        .iter()
        .map(|scope| scope.projection.bytes)
        .sum();
    for scope in prepared.scopes {
        let parent = scope
            .target
            .parent()
            .ok_or_else(|| invalid(&scope.target, "Save target has no parent."))?;
        ensure_plain_directory(parent)?;
        let staging = create_unique_directory(parent, ".save-import-publish")?;
        let cleanup = DirectoryCleanup::new(staging.clone(), parent.to_owned());
        server_plan::write(&scope.projection, &staging)?;
        if io::current_inventory(&scope.target)? != scope.original {
            return Err(invalid(
                &scope.target,
                "Current saves changed while preparing the import.",
            ));
        }
        ensure_plain_directory(&scope.target)?;
        let original = io::inventory(&scope.target, None)?;
        let expected_original = scope.original.unwrap_or_else(|| io::Inventory {
            digest: io::digest(b"{}"),
            files: 0,
            bytes: 0,
            entries: Default::default(),
        });
        if original != expected_original {
            return Err(invalid(
                &scope.target,
                "Current saves changed after preview; inspect the import again.",
            ));
        }
        let backup = create_instance_backup_with_prefix(
            &context.record,
            &scope.target,
            &context.record.summary.id,
            if scope.target == context.target {
                "pre-import"
            } else {
                "pre-import-map"
            },
            InstanceBackupKind::PreRestore,
        )?;
        if io::inventory(&Path::new(&backup.backup_path).join("saves"), None)? != original
            || io::inventory(&scope.target, None)? != original
        {
            return Err(invalid(
                &scope.target,
                "Current saves changed while creating the safeguard backup.",
            ));
        }
        backups.push(backup.backup_path);
        let rollback = unique_available_path(parent, ".save-import-rollback");
        let recovery = RestoreRollback::new(scope.target.clone(), rollback.clone());
        stages.push(Stage {
            target: scope.target,
            staging,
            rollback,
            expected: scope.projection.inventory,
            original,
            recovery,
            cleanup,
            moved: false,
            published: false,
        });
    }
    let roots: Vec<_> = stages
        .iter()
        .flat_map(|stage| [stage.target.clone(), stage.staging.clone()])
        .collect();
    let stamp = publication::Stamp::capture(&roots)?;
    if io::inventory(source, None)? != *tree || server_plan::guards(&plan)? != prepared.guards {
        return Err(invalid(
            source,
            "The source or native world selector changed while preparing the import.",
        ));
    }
    for stage in &stages {
        if io::inventory(&stage.target, None)? != stage.original
            || io::inventory(&stage.staging, None)? != stage.expected
        {
            return Err(invalid(
                &stage.target,
                "Save data changed before publication.",
            ));
        }
    }
    let result = publication
        .publish(&stamp, || {
            if server_plan::guards(&plan)? != prepared.guards {
                return Err(invalid(
                    source,
                    "The native world selector changed before publication.",
                ));
            }
            for stage in &mut stages {
                move_directory(&stage.target, &stage.rollback)?;
                stage.moved = true;
                publish(&stage.staging, &stage.target)?;
                stage.published = true;
            }
            Ok(())
        })
        .and_then(|()| {
            for stage in &stages {
                if io::inventory(&stage.target, None)? != stage.expected {
                    return Err(invalid(&stage.target, "Published saves failed readback."));
                }
            }
            Ok(())
        });
    if let Err(error) = result {
        let recovery = compensate(&mut stages);
        return Err(invalid(
            &context.target,
            format!(
                "Import failed: {error}. Recovery errors: {recovery:?}. Safeguard backups: {}",
                backups.join("; ")
            ),
        ));
    }
    let mut warnings = Vec::new();
    for stage in &mut stages {
        stage.cleanup.disarm();
        stage.recovery.commit();
        if let Err(error) = remove_managed_directory(
            &stage.rollback,
            stage.target.parent().unwrap_or(&context.target),
        ) {
            warnings.push(format!(
                "Import succeeded; recovery copy retained at {}: {error}",
                stage.rollback.display()
            ));
        }
    }
    let safeguard = backups
        .first()
        .cloned()
        .ok_or_else(|| invalid(source, "Missing safeguard backup."))?;
    Ok(InstanceSaveImportResult {
        instance_id: context.record.summary.id.clone(),
        saves_path: context.target.to_string_lossy().into_owned(),
        safeguard_backup_path: safeguard,
        copied_file_count: files,
        copied_total_bytes: bytes,
        warnings,
        safeguard_backup_paths: backups,
    })
}
