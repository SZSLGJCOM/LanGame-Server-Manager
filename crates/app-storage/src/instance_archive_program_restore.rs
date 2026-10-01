use std::fs;
use std::path::Path;

use sqlx::SqlitePool;

use super::{ProgramPlan, check_file, files, invalid, open_file, verify_library};
use crate::StorageError;
use crate::instance_archive_store::Archive;
use crate::instance_settings_lock::InstanceSettingsLock;

fn needs_copy(root: &Path, plan: &ProgramPlan) -> Result<bool, StorageError> {
    let mut missing = false;
    for (relative, expected) in &plan.files {
        let path = root.join(crate::private_runtime_refresh::validated_relative_path(
            relative,
        )?);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => missing = true,
            Err(source) => return Err(StorageError::ReadPath { path, source }),
            Ok(_) => check_file(&mut open_file(&path, false)?, &path, expected)?,
        }
    }
    Ok(missing)
}

/// Persist the staging identity before copying any payload. A crash in the
/// earlier mkdir/admission window preserves the unrecognized directory.
pub(crate) async fn prepare_staging(
    pool: &SqlitePool,
    archive: &Archive,
    root: &Path,
    plan: &ProgramPlan,
    lock: &InstanceSettingsLock,
) -> Result<Option<String>, StorageError> {
    let root = root.to_owned();
    let plan = plan.clone();
    let original = archive.identity.clone();
    let expected = archive.restore_staging_identity.clone();
    let identity = lock
        .spawn_blocking(move || {
            let _root = files::guard_identity(
                &root,
                original
                    .as_deref()
                    .ok_or_else(|| invalid(&root, "Archive has no directory identity."))?,
            )?;
            let stage = plan.staging(&root);
            if !needs_copy(&root, &plan)? {
                if let Some(expected) = expected.as_deref() {
                    if files::verify_identity(&stage, Some(expected))? {
                        files::purge_tree(&stage, expected)?;
                    }
                } else if fs::symlink_metadata(&stage).is_ok() {
                    return Err(invalid(
                        &stage,
                        "Unrecognized reconstruction staging was retained.",
                    ));
                }
                return Ok(None);
            }
            if let Some(expected) = expected {
                if !files::verify_identity(&stage, Some(&expected))? {
                    return Err(invalid(
                        &stage,
                        "Program reconstruction staging is missing.",
                    ));
                }
                return Ok(Some(expected));
            }
            fs::create_dir(&stage).map_err(|source| StorageError::CreatePath {
                path: stage.clone(),
                source,
            })?;
            files::identity(&stage)
        })
        .await
        .map_err(|error| worker_error("preparing archive reconstruction staging", error))??;
    if let Some(identity) = &identity {
        sqlx::query("UPDATE instance_archives SET restore_staging_identity_json=?2 WHERE archive_id=?1 AND state IN ('archiving','restoring')")
            .bind(&archive.id).bind(identity).execute(pool).await?;
    }
    Ok(identity)
}

pub(crate) fn restore_files(
    root: &Path,
    root_identity: &str,
    library: &Path,
    plan: &ProgramPlan,
    staging_identity: Option<&str>,
) -> Result<(), StorageError> {
    let _root = files::guard_identity(root, root_identity)?;
    let _library =
        super::native::open(library, true, false).map_err(|source| StorageError::ReadPath {
            path: library.to_owned(),
            source,
        })?;
    verify_library(library, plan)?;
    if !needs_copy(root, plan)? {
        return Ok(());
    }
    let staging = plan.staging(root);
    let identity = staging_identity.ok_or_else(|| {
        invalid(
            &staging,
            "Program reconstruction staging has no persisted identity.",
        )
    })?;
    let staging_guard = files::guard_identity(&staging, identity)?;
    files::preflight_tree(&staging, identity)?;
    for (index, (relative, expected)) in plan.files.iter().enumerate() {
        let destination = root.join(crate::private_runtime_refresh::validated_relative_path(
            relative,
        )?);
        if destination
            .try_exists()
            .map_err(|source| StorageError::ReadPath {
                path: destination.clone(),
                source,
            })?
        {
            check_file(&mut open_file(&destination, false)?, &destination, expected)?;
            continue;
        }
        if !files::verify_identity(&staging, Some(identity))? {
            return Err(invalid(
                &staging,
                "Program reconstruction staging disappeared.",
            ));
        }
        let temporary = staging.join(format!("{index:06}"));
        if fs::symlink_metadata(&temporary).is_ok() {
            // This exact temporary slot belongs to the persisted staging directory.
            // It may contain an incomplete stream from the previous attempt.
            open_file(&temporary, true)?
                .remove()
                .map_err(|source| StorageError::DeletePath {
                    path: temporary.clone(),
                    source,
                })?;
        }
        let source = library.join(
            relative
                .strip_prefix("runtime/")
                .ok_or_else(|| invalid(Path::new(relative), "Invalid reconstruction path."))?,
        );
        let mut source_guard = open_file(&source, false)?;
        check_file(&mut source_guard, &source, expected)?;
        let actual = crate::instance_creation_io::copy_creation_file(&source, &temporary, None)?;
        if actual != expected.sha256 {
            return Err(invalid(
                &temporary,
                "Copied program failed SHA-256 verification.",
            ));
        }
        let mut complete = open_file(&temporary, true)?;
        check_file(&mut complete, &temporary, expected)?;
        let parent = destination
            .parent()
            .ok_or_else(|| invalid(&destination, "Program target has no parent."))?;
        crate::instance_isolation::paths::normalize_path(parent)?;
        fs::create_dir_all(parent).map_err(|source| StorageError::CreatePath {
            path: parent.to_owned(),
            source,
        })?;
        complete
            .rename(&destination)
            .map_err(|source| StorageError::MovePath {
                from: temporary,
                to: destination,
                source,
            })?;
    }
    if needs_copy(root, plan)? {
        return Err(invalid(
            root,
            "Archive program reconstruction is incomplete.",
        ));
    }
    drop(staging_guard);
    files::purge_tree(&staging, identity)
}

fn worker_error(operation: &'static str, error: tokio::task::JoinError) -> StorageError {
    StorageError::BlockingTaskFailed {
        operation,
        message: error.to_string(),
    }
}
