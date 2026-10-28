use std::path::PathBuf;

use app_core::scum_custom_zones::{
    ScumCustomZonesSnapshot, ScumCustomZonesStatus, WriteScumCustomZoneDamageInput,
};
use sqlx::Connection;

use crate::instance_isolation::ensure_instance_paths_available;
use crate::instance_settings_lock::{
    acquire_instance_settings_mutation_lock, acquire_instance_settings_read_lock,
};
use crate::instances::effective_instance_install_root;
use crate::runtime::load_active_instance_run;
use crate::save_paths::{effective_instance_saves_dir, load_module_descriptor};
use crate::storage_db::{connect_pool, fetch_instance_record};
use crate::{InstanceStatus, StorageError, StoragePaths};

#[path = "scum_custom_zones_db.rs"]
mod db;
#[path = "scum_custom_zones_io.rs"]
mod io;
use io::{DatabaseAnchor, invalid};

#[cfg(test)]
#[path = "scum_custom_zones_tests.rs"]
mod tests;

struct ZoneContext {
    database: PathBuf,
    install: PathBuf,
    stopped: bool,
}

async fn load_context(
    paths: &StoragePaths,
    instance_id: &str,
    write: bool,
) -> Result<ZoneContext, StorageError> {
    let pool = connect_pool(paths).await?;
    let result = async {
        let record = fetch_instance_record(&pool, instance_id).await?;
        if record.summary.module_id != "scum" {
            return Err(invalid(
                &record.config_dir,
                "Custom-zone data belongs only to a SCUM instance.",
            ));
        }
        let stopped = matches!(record.summary.status, InstanceStatus::Stopped)
            && record.summary.active_process_count == 0
            && load_active_instance_run(&pool, instance_id)
                .await?
                .is_none();
        if write && !stopped {
            return Err(invalid(
                &record.config_dir,
                "Stop the server before changing custom zones.",
            ));
        }
        let install = effective_instance_install_root(&record)?;
        let descriptor = load_module_descriptor(paths, "scum")?;
        let saves = effective_instance_saves_dir(descriptor.as_ref(), &install, &record)?;
        let mut connection = pool.acquire().await?;
        ensure_instance_paths_available(
            paths,
            &mut connection,
            instance_id,
            "scum",
            &install,
            &record.config_dir,
            &saves,
        )
        .await?;
        Ok(ZoneContext {
            database: saves.join("SaveFiles/SCUM.db"),
            install,
            stopped,
        })
    }
    .await;
    pool.close().await;
    result
}

fn unavailable(
    instance_id: &str,
    status: ScumCustomZonesStatus,
    message: Option<&str>,
) -> ScumCustomZonesSnapshot {
    ScumCustomZonesSnapshot {
        instance_id: instance_id.to_owned(),
        status,
        database_identity: None,
        revision: None,
        configurations: Vec::new(),
        writable: false,
        message: message.map(str::to_owned),
        backup_id: None,
    }
}

async fn check_layout(context: &ZoneContext) -> Result<bool, StorageError> {
    let install = context.install.clone();
    tokio::task::spawn_blocking(move || io::verified_layout(&install))
        .await
        .map_err(|error| StorageError::BlockingTaskFailed {
            operation: "verifying SCUM zone layout",
            message: error.to_string(),
        })?
}

pub async fn read_scum_custom_zones(
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<ScumCustomZonesSnapshot, StorageError> {
    let _lease = acquire_instance_settings_read_lock(paths, instance_id)?;
    let context = load_context(paths, instance_id, false).await?;
    if !context.stopped {
        return Ok(unavailable(
            instance_id,
            ScumCustomZonesStatus::Running,
            None,
        ));
    }
    if !context
        .database
        .try_exists()
        .map_err(|error| invalid(&context.database, error.to_string()))?
    {
        return Ok(unavailable(instance_id, ScumCustomZonesStatus::Empty, None));
    }
    if !check_layout(&context).await? {
        return Ok(unavailable(
            instance_id,
            ScumCustomZonesStatus::Unsupported,
            Some("The installed server layout has not been verified for custom-zone edits."),
        ));
    }
    read_snapshot(instance_id, &context).await
}

async fn read_snapshot(
    instance_id: &str,
    context: &ZoneContext,
) -> Result<ScumCustomZonesSnapshot, StorageError> {
    let anchor = DatabaseAnchor::open(&context.database, false)?;
    let mut connection = db::connect(&anchor, false).await?;
    let result = async {
        let mut transaction = connection.begin().await?;
        let mut result = db::read(&mut transaction, &anchor)
            .await?
            .snapshot(instance_id, &anchor)?;
        if std::fs::metadata(&context.database)
            .map_err(|error| invalid(&context.database, error.to_string()))?
            .permissions()
            .readonly()
        {
            result.writable = false;
            result.message = Some("The SCUM database is read-only.".into());
        }
        transaction.commit().await?;
        Ok(result)
    }
    .await;
    connection.close().await?;
    result
}

pub async fn write_scum_custom_zone_damage(
    paths: &StoragePaths,
    input: WriteScumCustomZoneDamageInput,
) -> Result<ScumCustomZonesSnapshot, StorageError> {
    let lease = acquire_instance_settings_mutation_lock(paths, &input.instance_id)?;
    let paths = paths.clone();
    lease
        .complete_mutation("saving SCUM custom-zone damage", async move {
            let context = load_context(&paths, &input.instance_id, true).await?;
            if !check_layout(&context).await? {
                return Err(invalid(
                    &context.database,
                    "The installed server layout has not been verified for custom-zone edits.",
                ));
            }
            write_stopped(&paths, &context, input).await
        })
        .await
}

async fn write_stopped(
    paths: &StoragePaths,
    context: &ZoneContext,
    input: WriteScumCustomZoneDamageInput,
) -> Result<ScumCustomZonesSnapshot, StorageError> {
    let anchor = DatabaseAnchor::open(&context.database, true)?;
    let mut connection = db::connect(&anchor, true).await?;
    let result = async {
        // A writer reservation and read snapshot protect the complete WAL-backed
        // database while the existing backup facility captures the pre-change save.
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let prepared = db::prepare(&mut transaction, &anchor, &input).await?;
        let backup_id = if prepared.changed() {
            let backup = crate::backups::create_instance_backup(paths, &input.instance_id).await?;
            let current = load_context(paths, &input.instance_id, true).await?;
            if current.database != context.database {
                return Err(invalid(&context.database, "The instance database location changed; reload before saving."));
            }
            Some(backup.backup_id)
        } else { None };
        anchor.check_auxiliary_files()?;
        let expected = db::apply(&mut transaction, &anchor, &input, prepared).await?;
        transaction.commit().await?;
        // Observe the committed state, rather than reporting an uncommitted readback.
        let mut actual = db::read(&mut connection, &anchor).await?.snapshot(&input.instance_id, &anchor)?;
        if actual.revision != expected.revision {
            return Err(invalid(&context.database, "The committed custom-zone result changed. A pre-change backup is available in Maintenance; reload before retrying."));
        }
        actual.backup_id = backup_id;
        Ok(actual)
    }.await;
    connection.close().await?;
    result
}
