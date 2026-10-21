//! Desktop-only ingestion of prepared native saves. The configured world name
//! remains authoritative; imports never copy server configuration or guess player
//! identity conversions. Source, target, settings and ownership bind the preview.
use super::*;
use crate::instance_isolation::paths::normalize_path;
use crate::instance_settings_lock::acquire_instance_settings_read_lock;
use crate::storage_db::load_instance_isolation_records;
use crate::templates::{SchemaDefaultContext, collect_schema_inspection_defaults};
use serde::Serialize;

#[path = "save_import_io.rs"]
mod io;
#[path = "save_import_profiles.rs"]
mod profiles;
#[path = "save_import_server_plan.rs"]
mod server_plan;
#[path = "save_import_server_transaction.rs"]
mod server_transaction;
#[path = "save_import_server_unreal.rs"]
mod server_unreal;
#[path = "save_import_server_worlds.rs"]
mod server_worlds;
use server_plan::{ServerMapping, ServerPlan, ServerScope};

#[derive(Debug, Clone, Serialize)]
pub struct InstanceSaveImportSupport {
    pub module_id: String,
    pub supported: bool,
    pub mode: String,
    pub source_hint: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstanceSaveImportPreview {
    pub instance_id: String,
    pub module_id: String,
    pub source_path: String,
    pub target_path: String,
    pub file_count: u64,
    pub total_bytes: u64,
    pub existing_file_count: u64,
    pub existing_total_bytes: u64,
    pub fingerprint: String,
    pub warnings: Vec<String>,
    pub changes: Vec<InstanceSaveImportChange>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstanceSaveImportChange {
    pub path: String,
    pub file_count: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstanceSaveImportResult {
    pub instance_id: String,
    pub saves_path: String,
    pub safeguard_backup_path: String,
    pub copied_file_count: u64,
    pub copied_total_bytes: u64,
    pub warnings: Vec<String>,
    pub safeguard_backup_paths: Vec<String>,
}

#[derive(Clone)]
struct Context {
    record: StoredInstanceRecord,
    install: PathBuf,
    target: PathBuf,
    settings: Map<String, Value>,
    settings_json: String,
    protected: Vec<PathBuf>,
}

fn invalid(path: &Path, message: impl Into<String>) -> StorageError {
    StorageError::ReadPath {
        path: path.to_owned(),
        source: std::io::Error::other(message.into()),
    }
}

async fn context(
    paths: &StoragePaths,
    connection: &mut sqlx::SqliteConnection,
    instance_id: &str,
) -> Result<Context, StorageError> {
    let record = fetch_instance_record(&mut *connection, instance_id).await?;
    let descriptor = load_module_descriptor(paths, &record.summary.module_id)?;
    let install = effective_instance_install_root(&record)?;
    let target = effective_instance_saves_dir(descriptor.as_ref(), &install, &record)?;
    let (settings, settings_json) = effective_instance_settings(descriptor.as_ref(), &record)?;
    let mut protected = vec![
        paths.app_data_root.clone(),
        paths.instances_root.clone(),
        paths.games_root.clone(),
        paths.archives_root.clone(),
        paths.steamcmd_root.clone(),
        paths.modules_root.clone(),
    ];
    for other in load_instance_isolation_records(connection).await? {
        protected.push(instance_root(&other));
        protected.push(other.saves_dir);
        if let Some(program) = other.program_install_root {
            protected.push(program);
        }
    }
    Ok(Context {
        record,
        install,
        target,
        settings,
        settings_json,
        protected,
    })
}

/// Schema defaults overlaid with the instance's stored settings, plus the stored
/// JSON. Shared by import planning and the read-only save slot listing.
pub(crate) fn effective_instance_settings(
    descriptor: Option<&app_modules::ModuleDescriptor>,
    record: &StoredInstanceRecord,
) -> Result<(Map<String, Value>, String), StorageError> {
    let settings_json =
        crate::instances::read_instance_settings_json(&record.config_dir.join("instance.json"))?;
    let mut settings = collect_schema_inspection_defaults(
        descriptor.and_then(|module| module.schema_json.as_deref()),
        SchemaDefaultContext {
            instance_id: Some(&record.summary.id),
            instance_name: Some(&record.summary.name),
        },
    )?;
    settings.extend(serde_json::from_str::<Map<String, Value>>(&settings_json)?);
    Ok((settings, settings_json))
}

fn stopped(context: &Context) -> Result<(), StorageError> {
    if context.record.summary.active_process_count != 0
        || !matches!(
            context.record.summary.status,
            InstanceStatus::Stopped | InstanceStatus::Error
        )
    {
        return Err(invalid(
            &context.target,
            "Stop the server before previewing or importing saves.",
        ));
    }
    Ok(())
}

async fn validate_ownership(
    paths: &StoragePaths,
    connection: &mut sqlx::SqliteConnection,
    context: &Context,
) -> Result<(), StorageError> {
    stopped(context)?;
    ensure_instance_paths_available(
        paths,
        connection,
        &context.record.summary.id,
        &context.record.summary.module_id,
        &context.install,
        &context.record.config_dir,
        &context.target,
    )
    .await
}

async fn read_context(
    paths: &StoragePaths,
    instance_id: &str,
    validate: bool,
) -> Result<Context, StorageError> {
    let pool = connect_pool(paths).await?;
    let result = async {
        let mut connection = pool.acquire().await?;
        let context = context(paths, &mut connection, instance_id).await?;
        if validate {
            validate_ownership(paths, &mut connection, &context).await?;
        }
        Ok(context)
    }
    .await;
    pool.close().await;
    result
}

pub async fn read_instance_save_import_support(
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<InstanceSaveImportSupport, StorageError> {
    let _read = acquire_instance_settings_read_lock(paths, instance_id)?;
    let context = read_context(paths, instance_id, false).await?;
    if let Some(source_hint) =
        server_unreal::hint(&context).or_else(|| server_worlds::hint(&context))
    {
        return Ok(InstanceSaveImportSupport {
            module_id: context.record.summary.module_id,
            supported: true,
            mode: "native_directory".into(),
            source_hint,
        });
    }
    Ok(profiles::support(&context))
}

fn plan_server_import(
    context: &Context,
    source: &Path,
    tree: &io::Inventory,
) -> Result<Option<ServerPlan>, StorageError> {
    match server_unreal::plan(context, source, tree)? {
        Some(plan) => Ok(Some(plan)),
        None => server_worlds::plan(context, source, tree),
    }
}

fn fingerprint(
    context: &Context,
    source: &Path,
    tree: &io::Inventory,
    target: &Option<io::Inventory>,
) -> Result<String, StorageError> {
    Ok(io::digest(serde_json::to_vec(&(
        &context.record.summary.id,
        &context.record.summary.module_id,
        source,
        normalize_path(&context.target)?,
        &context.settings_json,
        &context.settings,
        &tree.digest,
        target.as_ref().map(|tree| &tree.digest),
    ))?))
}

fn preview(context: &Context, source: &Path) -> Result<InstanceSaveImportPreview, StorageError> {
    stopped(context)?;
    let source = io::validate_source_path(source, &context.target, &context.protected)?;
    let tree = io::inventory(&source, None)?;
    if let Some(plan) = plan_server_import(context, &source, &tree)? {
        return server_transaction::preview(context, &source, &tree, plan);
    }
    let warnings = profiles::validate(context, &source, &tree)?;
    let current = io::current_inventory(&context.target)?;
    let payload = tree.with_preserved_files(None, profiles::preserved_paths(context))?;
    Ok(InstanceSaveImportPreview {
        instance_id: context.record.summary.id.clone(),
        module_id: context.record.summary.module_id.clone(),
        source_path: source.to_string_lossy().into_owned(),
        target_path: context.target.to_string_lossy().into_owned(),
        file_count: payload.files,
        total_bytes: payload.bytes,
        existing_file_count: current.as_ref().map_or(0, |tree| tree.files),
        existing_total_bytes: current.as_ref().map_or(0, |tree| tree.bytes),
        fingerprint: fingerprint(context, &source, &tree, &current)?,
        warnings,
        changes: vec![InstanceSaveImportChange {
            path: context.target.to_string_lossy().into_owned(),
            file_count: payload.files,
            total_bytes: payload.bytes,
        }],
    })
}

pub async fn preview_instance_save_import(
    paths: &StoragePaths,
    instance_id: &str,
    source_path: &Path,
) -> Result<InstanceSaveImportPreview, StorageError> {
    let lease = acquire_instance_settings_read_lock(paths, instance_id)?;
    let context = read_context(paths, instance_id, true).await?;
    let source = source_path.to_owned();
    lease
        .spawn_blocking(move || preview(&context, &source))
        .await
        .map_err(|error| invalid(source_path, format!("Save preview worker failed: {error}")))?
}

pub async fn import_instance_save(
    paths: &StoragePaths,
    instance_id: &str,
    source_path: &Path,
    expected_fingerprint: &str,
) -> Result<InstanceSaveImportResult, StorageError> {
    let lease = acquire_instance_settings_mutation_lock(paths, instance_id)?;
    let paths = paths.clone();
    let instance_id = instance_id.to_owned();
    let source = source_path.to_owned();
    let expected = expected_fingerprint.to_owned();
    lease.complete_mutation("importing native saves", async move {
        let pool = connect_pool(&paths).await?;
        let result = async {
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
            let original = context(&paths, &mut tx, &instance_id).await?;
            validate_ownership(&paths, &mut tx, &original).await?;
            tx.rollback().await?;
            let checked = original.clone();
            let checked_paths = paths.clone();
            let source_checked = source.clone();
            publication::run(&pool, move |connection| {
                let checked = checked.clone();
                let paths = checked_paths.clone();
                let source = source_checked.clone();
                Box::pin(async move {
                    let current = context(&paths, connection, &checked.record.summary.id).await?;
                    if current.target != checked.target || current.install != checked.install
                        || current.record.config_dir != checked.record.config_dir
                        || current.settings_json != checked.settings_json || current.settings != checked.settings
                        || current.record.summary.module_id != checked.record.summary.module_id {
                        return Err(invalid(&current.target, "Instance configuration or save destination changed after preview; inspect again."));
                    }
                    io::validate_source_path(&source, &current.target, &current.protected)?;
                    validate_ownership(&paths, connection, &current).await
                })
            }, move |publication| {
                let transaction = instance_transaction_lock(&instance_root(&original.record));
                let _transaction = transaction.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                import_transaction(&original, &source, &expected, &publication, publish_new_directory)
            }).await
        }.await;
        pool.close().await;
        result
    }).await
}

fn import_transaction(
    context: &Context,
    source: &Path,
    expected: &str,
    publication: &publication::Publication,
    publish: impl FnOnce(&Path, &Path) -> Result<(), StorageError>,
) -> Result<InstanceSaveImportResult, StorageError> {
    stopped(context)?;
    let source = io::validate_source_path(source, &context.target, &context.protected)?;
    let original_source = io::inventory(&source, None)?;
    if let Some(plan) = plan_server_import(context, &source, &original_source)? {
        let mut first_publish = Some(publish);
        return server_transaction::import(
            context,
            &source,
            &original_source,
            plan,
            expected,
            publication,
            |source, target| match first_publish.take() {
                Some(first) => first(source, target),
                None => publish_new_directory(source, target),
            },
        );
    }
    profiles::validate(context, &source, &original_source)?;
    let original_target = io::current_inventory(&context.target)?;
    if expected.is_empty()
        || fingerprint(context, &source, &original_source, &original_target)? != expected
    {
        return Err(invalid(
            &source,
            "The source, current saves or Configuration changed after preview. Inspect the import again; no saves were replaced.",
        ));
    }
    let parent = context
        .target
        .parent()
        .ok_or_else(|| invalid(&context.target, "Save destination has no parent."))?;
    ensure_plain_directory(parent)?;
    let staging = create_unique_directory(parent, ".save-import-publish")?;
    let mut cleanup = DirectoryCleanup::new(staging.clone(), parent.to_owned());
    let preserved = profiles::preserved_paths(context);
    let staged_expected =
        original_source.with_preserved_files(original_target.as_ref(), preserved)?;
    let copied = io::inventory_with_preserved(&source, Some(&staging), preserved)?;
    io::preserve_target_files(
        &context.target,
        &staging,
        original_target.as_ref(),
        preserved,
    )?;
    if copied != original_source
        || io::inventory(&staging, None)? != staged_expected
        || io::inventory(&source, None)? != original_source
    {
        return Err(invalid(
            &source,
            "Source saves changed while staging. Close the source game and inspect the import again.",
        ));
    }
    // Back up the complete effective scope so the existing Restore action can
    // recover it without special knowledge of this import implementation.
    ensure_plain_directory(&context.target)?;
    let safeguard = create_instance_backup_with_prefix(
        &context.record,
        &context.target,
        &context.record.summary.id,
        "pre-import",
        InstanceBackupKind::PreRestore,
    )?;
    let backed_up = io::inventory(&Path::new(&safeguard.backup_path).join("saves"), None)?;
    let current = io::inventory(&context.target, None)?;
    let expected_target = original_target.clone().unwrap_or_else(|| io::Inventory {
        digest: io::digest(b"{}"),
        files: 0,
        bytes: 0,
        entries: Default::default(),
    });
    if backed_up != expected_target || current != expected_target {
        return Err(invalid(
            &context.target,
            "Current saves changed while creating the safeguard backup; no saves were replaced.",
        ));
    }
    let stamp = publication::Stamp::capture(&[context.target.clone(), staging.clone()])?;
    if io::inventory(&source, None)? != original_source
        || io::inventory(&staging, None)? != staged_expected
        || io::inventory(&context.target, None)? != expected_target
    {
        return Err(invalid(
            &source,
            "Source or destination changed before publication; no saves were replaced.",
        ));
    }
    let rollback = unique_available_path(parent, ".save-import-rollback");
    // Keep compensation armed until the coordinator has also released its DB
    // fence. A successful directory move alone is not transaction completion.
    let mut recovery = RestoreRollback::new(context.target.clone(), rollback.clone());
    let mut original_moved = false;
    let mut published = false;
    let publication_result = publication
        .publish(&stamp, || {
            move_directory(&context.target, &rollback)?;
            original_moved = true;
            publish(&staging, &context.target)?;
            published = true;
            Ok(())
        })
        .and_then(|()| {
            #[cfg(test)]
            crate::instance_archive::test_gate::pause(
                &context.target,
                crate::instance_archive::test_gate::Point::SaveImportReadback,
            );
            // The instance lease still owns these bytes. Release the database
            // writer before hashing, and retain rollback until readback succeeds.
            match io::inventory(&context.target, None) {
                Ok(actual) if actual == staged_expected => Ok(()),
                result => {
                    let reason = match result {
                        Err(error) => error.to_string(),
                        Ok(_) => "published save content differs from its verified source".into(),
                    };
                    Err(invalid(
                        &context.target,
                        format!("Published save verification failed: {reason}"),
                    ))
                }
            }
        });
    if let Err(error) = publication_result {
        if original_moved {
            let restored = (|| {
                if published {
                    move_directory(&context.target, &staging)?;
                }
                recovery.restore()
            })();
            if let Err(restore) = restored {
                return Err(invalid(
                    &context.target,
                    format!(
                        "Import failed ({error}); restoring the original directory also failed ({restore}). Recovery directory: {}. Safeguard backup: {}",
                        rollback.display(),
                        safeguard.backup_path
                    ),
                ));
            }
        }
        return Err(invalid(
            &context.target,
            format!(
                "Import failed; original saves were preserved or restored: {error}. Safeguard backup: {}",
                safeguard.backup_path
            ),
        ));
    }
    cleanup.disarm();
    recovery.commit();
    let mut warnings = Vec::new();
    if let Err(error) = remove_managed_directory(&rollback, parent) {
        warnings.push(format!("Import succeeded, but removing the redundant recovery directory failed: {} ({error}). The standard safeguard backup is retained.", rollback.display()));
    }
    let payload = original_source.with_preserved_files(None, preserved)?;
    let safeguard_backup_paths = vec![safeguard.backup_path.clone()];
    Ok(InstanceSaveImportResult {
        instance_id: context.record.summary.id.clone(),
        saves_path: context.target.to_string_lossy().into_owned(),
        safeguard_backup_path: safeguard.backup_path,
        copied_file_count: payload.files,
        copied_total_bytes: payload.bytes,
        warnings,
        safeguard_backup_paths,
    })
}

#[cfg(test)]
#[path = "save_import_tests.rs"]
mod tests;
