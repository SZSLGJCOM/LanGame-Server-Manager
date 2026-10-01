use std::path::PathBuf;

use crate::instance_archive_files::plain_directory;
use crate::instance_isolation::paths::{contains, normalize_path};
use crate::{StorageError, StoragePaths, StoredInstanceRecord};

pub(crate) struct RetirementPaths {
    pub program: PathBuf,
    pub saves: PathBuf,
    pub library: bool,
    pub exclusive: bool,
}

/// Deletion does not require a runnable program. The only relaxed case is a
/// damaged internal runtime whose ownership is still unambiguously registered.
/// Archive, startup and configuration continue to use the strict resolver.
pub(crate) async fn resolve(
    paths: &StoragePaths,
    record: &StoredInstanceRecord,
) -> Result<RetirementPaths, StorageError> {
    let root = crate::instance_reconciliation::checked_instance_root(paths, &record.config_dir)?;
    if !plain_directory(&root)? {
        let program = record.program_install_root.clone().ok_or_else(|| {
            crate::instance_archive_store::invalid(&root, "Instance program ownership is missing.")
        })?;
        return Ok(RetirementPaths {
            library: !contains(&root, &normalize_path(&program)?),
            program,
            saves: record.saves_dir.clone(),
            exclusive: false,
        });
    }
    let (program, library, exclusive) = match crate::instances::effective_instance_install_root(
        record,
    ) {
        Ok(program) => (
            program,
            crate::program_runtime::instance_uses_library_program(&root)?,
            crate::program_runtime::instance_uses_exclusive_program(&root)?,
        ),
        Err(error) => {
            let runtime = root.join("runtime");
            plain_directory(&runtime)?;
            crate::workshop_collection_removal::ensure_no_pending(&root)?;
            if record.runtime_mode != "independent" {
                return Err(error);
            }
            let Some(registered) = &record.program_install_root else {
                return Err(error);
            };
            if normalize_path(registered)? != normalize_path(&runtime)? {
                return Err(error);
            }
            let pool = crate::storage_db::connect_pool(paths).await?;
            let owned = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM instances i JOIN game_installs g ON g.id=i.install_id \
                 WHERE i.id=?1 AND g.scope='instance' AND g.owner_instance_id=i.id AND g.module_id=i.module_id)",
            ).bind(&record.summary.id).fetch_one(&pool).await;
            pool.close().await;
            if !owned? {
                return Err(error);
            }
            (registered.clone(), false, false)
        }
    };
    let descriptor = crate::save_paths::load_module_descriptor(paths, &record.summary.module_id)?;
    let saves = match crate::save_paths::effective_instance_saves_dir(
        descriptor.as_ref(),
        &program,
        record,
    ) {
        Ok(saves) => saves,
        Err(StorageError::ConfigJson(_)) if !exclusive => record.saves_dir.clone(),
        Err(StorageError::InvalidModuleSavePathTemplate { .. })
            if !exclusive && configuration_is_missing(record)? =>
        {
            record.saves_dir.clone()
        }
        Err(error) => return Err(error),
    };
    Ok(RetirementPaths {
        program,
        saves,
        library,
        exclusive,
    })
}

pub(crate) fn configuration_is_missing(
    record: &StoredInstanceRecord,
) -> Result<bool, StorageError> {
    let config = record.config_dir.join("instance.json");
    crate::instance_isolation::paths::normalize_resource_path(&config)?;
    config
        .try_exists()
        .map(|exists| !exists)
        .map_err(|source| StorageError::ReadPath {
            path: config,
            source,
        })
}
