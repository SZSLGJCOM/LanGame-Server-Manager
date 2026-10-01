use std::path::PathBuf;

use sqlx::SqlitePool;

use crate::instance_archive_files;
use crate::instance_archive_store::{self as store, Archive};
use crate::storage_db::{connect_pool, fetch_instance_record};
use crate::{StorageError, StoragePaths};

/// Program ownership needed before an archive operation acquires its lifecycle
/// leases. This is metadata only: callers must reread it after taking the leases
/// and retain those leases until the filesystem worker finishes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstanceStorageResources {
    pub instance_id: Option<String>,
    pub module_id: Option<String>,
    pub program_roots: Vec<PathBuf>,
}

pub async fn read_instance_archive_resources(
    paths: &StoragePaths,
    archive_id: &str,
) -> Result<InstanceStorageResources, StorageError> {
    store::validate_id(archive_id)?;
    let pool = connect_pool(paths).await?;
    let result = async {
        let archive = store::load(&pool, archive_id).await?;
        archive_resources(&pool, paths, &archive).await
    }
    .await;
    pool.close().await;
    result
}

/// Read only the catalog identity used to choose lifecycle leases. This does not
/// authorize cleanup: the unchanged purge implementation still verifies directory
/// ownership, the full tree, and absent references. Recovery snapshot validity is
/// deliberately not a cleanup condition, matching the existing can_purge contract
/// and archive_corrupt_metadata_is_visible_but_never_inferred_for_restore test.
pub async fn read_instance_archive_cleanup_resources(
    paths: &StoragePaths,
    archive_id: &str,
) -> Result<InstanceStorageResources, StorageError> {
    store::validate_id(archive_id)?;
    let pool = connect_pool(paths).await?;
    let result = async {
        let archive = store::load(&pool, archive_id).await?;
        finish(InstanceStorageResources {
            instance_id: archive.instance_id,
            module_id: archive.module_id,
            program_roots: vec![instance_archive_files::archive_path(paths, &archive.leaf)?],
        })
    }
    .await;
    pool.close().await;
    result
}

pub async fn read_instance_retirement_resources(
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<InstanceStorageResources, StorageError> {
    let pool = connect_pool(paths).await?;
    let result = async {
        // Recovery must use the durable source claims even if the active record
        // still exists or a failed delete has already removed that record.
        let pending: Option<String> = sqlx::query_scalar(
            "SELECT archive_id FROM instance_archives WHERE instance_id=?1 AND \
             (state IN ('archiving','restoring') OR (purpose='delete' AND state='purging')) \
             ORDER BY archive_id LIMIT 1",
        )
        .bind(instance_id)
        .fetch_optional(&pool)
        .await?;
        if let Some(id) = pending {
            let archive = store::load(&pool, &id).await?;
            return archive_resources(&pool, paths, &archive).await;
        }
        let mut connection = pool.acquire().await?;
        let instance = fetch_instance_record(&mut *connection, instance_id).await?;
        drop(connection);
        // Resource admission uses durable registrations, not a runtime that may
        // already have been removed. Actual deletion revalidates ownership.
        let install = instance.program_install_root.clone().ok_or_else(|| {
            store::invalid(
                &instance.config_dir,
                "Instance program ownership is missing or inconsistent.",
            )
        })?;
        let descriptor =
            crate::save_paths::load_module_descriptor(paths, &instance.summary.module_id)?;
        let saves = match crate::save_paths::effective_instance_saves_dir(
            descriptor.as_ref(),
            &install,
            &instance,
        ) {
            Ok(saves) => saves,
            Err(StorageError::ConfigJson(_)) => instance.saves_dir.clone(),
            Err(StorageError::InvalidModuleSavePathTemplate { .. })
                if crate::instance_retirement_paths::configuration_is_missing(&instance)? =>
            {
                instance.saves_dir.clone()
            }
            Err(error) => return Err(error),
        };
        let mut resources = InstanceStorageResources {
            instance_id: Some(instance_id.to_owned()),
            module_id: Some(instance.summary.module_id.clone()),
            program_roots: vec![
                install,
                saves,
                instance.saves_dir.clone(),
                instance
                    .config_dir
                    .parent()
                    .unwrap_or(&instance.config_dir)
                    .to_owned(),
            ],
        };
        add_library_roots(&pool, &instance.summary.module_id, &mut resources).await?;
        finish(resources)
    }
    .await;
    pool.close().await;
    result
}

async fn archive_resources(
    pool: &SqlitePool,
    paths: &StoragePaths,
    archive: &Archive,
) -> Result<InstanceStorageResources, StorageError> {
    let mut resources = InstanceStorageResources {
        instance_id: archive.instance_id.clone(),
        module_id: archive.module_id.clone(),
        program_roots: vec![instance_archive_files::archive_path(paths, &archive.leaf)?],
    };
    if archive.snapshot.is_some() {
        let snapshot = store::snapshot(archive)?;
        let instance = store::instance(&snapshot);
        let config = PathBuf::from(store::string(instance, "config_path")?);
        if let Some(root) = config.parent() {
            resources.program_roots.push(root.to_owned());
        }
        resources.program_roots.push(PathBuf::from(
            snapshot
                .effective_saves_path
                .as_deref()
                .unwrap_or(store::string(instance, "saves_path")?),
        ));
        resources.instance_id = Some(store::string(instance, "id")?.to_owned());
        let module = store::string(instance, "module_id")?.to_owned();
        resources.module_id = Some(module.clone());
        if let Some(install) = snapshot.tables["game_installs"].first() {
            resources
                .program_roots
                .push(PathBuf::from(store::string(install, "install_root")?));
        }
        if let Some(plan) = snapshot.external_program {
            resources.program_roots.push(plan.root);
        }
        add_library_roots(pool, &module, &mut resources).await?;
        if let Some(plan) = snapshot.program
            && plan.module_id != module
        {
            add_library_roots(pool, &plan.module_id, &mut resources).await?;
        }
    }
    finish(resources)
}

async fn add_library_roots(
    pool: &SqlitePool,
    module_id: &str,
    resources: &mut InstanceStorageResources,
) -> Result<(), StorageError> {
    let roots: Vec<String> = sqlx::query_scalar(
        "SELECT install_root FROM game_installs WHERE module_id=?1 AND scope='library' LIMIT 4097",
    )
    .bind(module_id)
    .fetch_all(pool)
    .await?;
    if roots.len() > store::MAX_ARCHIVES {
        return Err(store::invalid(
            std::path::Path::new(module_id),
            "Program resource inventory exceeds its limit.",
        ));
    }
    resources
        .program_roots
        .extend(roots.into_iter().map(PathBuf::from));
    Ok(())
}

fn finish(
    mut resources: InstanceStorageResources,
) -> Result<InstanceStorageResources, StorageError> {
    for root in &mut resources.program_roots {
        *root = crate::instance_isolation::paths::normalize_resource_path(root)?;
    }
    resources.program_roots.sort();
    resources.program_roots.dedup();
    Ok(resources)
}
