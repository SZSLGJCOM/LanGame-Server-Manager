use std::io::Read;
use std::time::Duration;

use app_core::{
    InstanceBackupResult, InstanceDetails, InstanceStatus, InstanceSummary, PortBinding,
};
use serde::Serialize;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

use crate::instance_archive_files::{self as files, native};
use crate::instance_archive_store::{self as store, Archive, Snapshot, invalid};
use crate::{StorageError, StoragePaths};

const MAX_CONFIGURATION_BYTES: u64 = 4 * 1024 * 1024;
const MAX_RUN_ENTRIES: usize = 100;

#[path = "instance_archive_details_files.rs"]
mod retained;

#[derive(Clone, Debug, Serialize)]
pub struct InstanceArchiveDetails {
    pub archive_id: String,
    pub instance: InstanceDetails,
    pub maintenance: InstanceArchiveMaintenance,
    pub runs: InstanceArchiveRuns,
    pub log: InstanceArchiveLog,
    pub backups: InstanceArchiveBackups,
}

#[derive(Clone, Debug, Serialize)]
pub struct InstanceArchiveMaintenance {
    pub autostart: bool,
    pub auto_backup_on_stop: bool,
    pub backup_retention_count: u32,
    pub crash_restart_limit: u32,
    pub runtime_mode: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct InstanceArchiveRun {
    pub id: i64,
    pub status: String,
    pub started_at: Option<String>,
    pub stopped_at: Option<String>,
    pub exit_code: Option<i32>,
    pub crash_flag: bool,
    pub display_name: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct InstanceArchiveRuns {
    pub entries: Vec<InstanceArchiveRun>,
    pub total: usize,
    pub truncated: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct InstanceArchiveLog {
    pub relative_path: Option<String>,
    pub text: String,
    pub truncated: bool,
    pub issues: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct InstanceArchiveBackups {
    pub entries: Vec<InstanceBackupResult>,
    pub issues: Vec<String>,
    pub truncated: bool,
}

pub async fn read_instance_archive_details(
    paths: &StoragePaths,
    archive_id: &str,
) -> Result<InstanceArchiveDetails, StorageError> {
    store::validate_id(archive_id)?;
    let lock = super::inventory_lock(paths)?;
    // Preview must not initialize a database, register unknown directories, or
    // advance archive recovery. The journal is opened without write access.
    let options = SqliteConnectOptions::new()
        .filename(&paths.database_path)
        .read_only(true)
        .create_if_missing(false)
        .busy_timeout(Duration::from_secs(30));
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    let result = async {
        let archive = store::load(&pool, archive_id).await?;
        if archive.purpose != "archive" || archive.state != "archived" {
            return Err(invalid(
                std::path::Path::new(&archive.leaf),
                "Only completed instance archives have a details preview.",
            ));
        }
        let snapshot = store::snapshot(&archive)?;
        let paths = paths.clone();
        lock.spawn_blocking(move || read_details(&paths, &archive, &snapshot))
            .await
            .map_err(|error| StorageError::BlockingTaskFailed {
                operation: "reading archived instance details",
                message: error.to_string(),
            })?
    }
    .await;
    pool.close().await;
    result
}

fn read_details(
    paths: &StoragePaths,
    archive: &Archive,
    snapshot: &Snapshot,
) -> Result<InstanceArchiveDetails, StorageError> {
    let _parents = files::guard_parents(
        paths,
        archive.instances_identity.as_deref(),
        archive.parent_identity.as_deref(),
    )?;
    // Validate the historical binding, but do not require free ports or the
    // reconstruction library: those affect restoration, not reading settings.
    let original = super::transactions::original_root(paths, archive, snapshot)?;
    let root = files::archive_path(paths, &archive.leaf)?;
    let identity = archive
        .identity
        .as_deref()
        .ok_or_else(|| invalid(&root, "Archive directory identity is missing."))?;
    let _root = files::guard_identity(&root, identity)?;
    let config_path = root.join("config/instance.json");
    let mut config = native::open_verified_file(&config_path, false).map_err(|source| {
        StorageError::ReadPath {
            path: config_path.clone(),
            source,
        }
    })?;
    let mut bytes = Vec::new();
    config
        .reader()
        .take(MAX_CONFIGURATION_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| StorageError::ReadPath {
            path: config_path.clone(),
            source,
        })?;
    if bytes.len() as u64 > MAX_CONFIGURATION_BYTES {
        return Err(invalid(
            &config_path,
            "Archived instance configuration exceeds 4 MiB.",
        ));
    }
    if store::digest(&bytes) != snapshot.config_sha256 {
        return Err(invalid(
            &config_path,
            "Archived instance configuration differs from its recovery snapshot.",
        ));
    }
    let document: serde_json::Value = serde_json::from_slice(&bytes)?;
    let mut settings = document
        .as_object()
        .and_then(|object| object.get("settings"))
        .and_then(serde_json::Value::as_object)
        .cloned()
        .ok_or_else(|| {
            invalid(
                &config_path,
                "Archived instance settings must be an object.",
            )
        })?;
    let instance = store::instance(snapshot);
    let ports: Vec<PortBinding> = snapshot.tables["instance_ports"]
        .iter()
        .map(|row| {
            let port = row["port"]
                .as_u64()
                .and_then(|value| u16::try_from(value).ok())
                .ok_or_else(|| invalid(&root, "Archived port is outside 0..65535."))?;
            Ok(PortBinding {
                name: store::string(row, "name")?.to_owned(),
                protocol: store::string(row, "protocol")?.to_owned(),
                port,
            })
        })
        .collect::<Result<_, StorageError>>()?;
    let maintenance = InstanceArchiveMaintenance {
        autostart: saved_bool(instance, "autostart")?,
        auto_backup_on_stop: saved_bool(instance, "auto_backup_on_stop")?,
        backup_retention_count: saved_u32(instance, "backup_retention_count")?,
        crash_restart_limit: saved_u32(instance, "crash_restart_limit")?,
        runtime_mode: store::string(instance, "runtime_mode")?.to_owned(),
    };
    let mut rows = snapshot.tables["instance_runs"]
        .iter()
        .map(|row| {
            let id = row["id"]
                .as_i64()
                .ok_or_else(|| invalid(&root, "Archived run ID is not an integer."))?;
            Ok((id, row))
        })
        .collect::<Result<Vec<_>, StorageError>>()?;
    rows.sort_by_key(|(id, _)| std::cmp::Reverse(*id));
    let runs = InstanceArchiveRuns {
        entries: rows
            .iter()
            .take(MAX_RUN_ENTRIES)
            .map(|(id, row)| {
                let exit_code = match &row["exit_code"] {
                    serde_json::Value::Null => None,
                    value => Some(
                        value
                            .as_i64()
                            .and_then(|number| i32::try_from(number).ok())
                            .ok_or_else(|| {
                                invalid(&root, "Archived exit code is not a 32-bit integer.")
                            })?,
                    ),
                };
                Ok(InstanceArchiveRun {
                    id: *id,
                    status: store::string(row, "status")?.to_owned(),
                    started_at: saved_optional_string(row, "started_at")?,
                    stopped_at: saved_optional_string(row, "stopped_at")?,
                    exit_code,
                    crash_flag: saved_bool(row, "crash_flag")?,
                    display_name: saved_optional_string(row, "display_name")?,
                })
            })
            .collect::<Result<_, StorageError>>()?,
        total: rows.len(),
        truncated: rows.len() > MAX_RUN_ENTRIES,
    };
    let log = retained::read_log(&root, &original, &rows);
    let instance_id = store::string(instance, "id")?;
    let backups = retained::read_backups(&root, instance_id);
    let module_id = store::string(instance, "module_id")?;
    if module_id == "returntomoria" {
        project_moria_permissions(&root, snapshot, &mut settings)?;
    }
    let descriptor = crate::save_paths::load_module_descriptor(paths, module_id)?;
    let status = match store::string(instance, "status")? {
        "stopped" => InstanceStatus::Stopped,
        "error" => InstanceStatus::Error,
        _ => {
            return Err(invalid(
                &root,
                "Archived instance status is not stopped or error.",
            ));
        }
    };
    let details = InstanceDetails {
        summary: InstanceSummary {
            id: instance_id.to_owned(),
            name: store::string(instance, "name")?.to_owned(),
            module_id: module_id.to_owned(),
            status,
            // Completed archives no longer have registered processes or runs.
            active_process_count: 0,
            bind_ip: store::string(instance, "bind_ip")?.to_owned(),
            port_count: ports.len(),
            autostart: maintenance.autostart,
        },
        config_file_path: config_path.to_string_lossy().into_owned(),
        // Preserve the recorded effective location for display. Reading archive
        // details must never open the original or an external save location.
        saves_path: snapshot
            .effective_saves_path
            .clone()
            .unwrap_or(store::string(instance, "saves_path")?.to_owned()),
        backup_uses_declared_saves_path: crate::save_paths::module_declares_saves_path(
            descriptor.as_ref(),
        ),
        auto_backup_on_stop: maintenance.auto_backup_on_stop,
        backup_retention_count: maintenance.backup_retention_count,
        settings_json: serde_json::to_string_pretty(&settings)?,
        ports,
        active_run: None,
    };
    Ok(InstanceArchiveDetails {
        archive_id: archive.id.clone(),
        instance: details,
        maintenance,
        runs,
        log,
        backups,
    })
}

fn project_moria_permissions(
    root: &std::path::Path,
    snapshot: &Snapshot,
    settings: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), StorageError> {
    use crate::instance_native_settings::{MORIA_PERMISSIONS_FILE, MoriaPermissionsSnapshot};

    let Some(plan) = &snapshot.external_program else {
        if let Some(permissions) =
            MoriaPermissionsSnapshot::read("returntomoria", &root.join("runtime"))?
        {
            permissions.project(settings);
        }
        return Ok(());
    };
    let Some((index, (_, file))) = plan
        .files
        .iter()
        .enumerate()
        .find(|(_, (relative, _))| relative.eq_ignore_ascii_case(MORIA_PERMISSIONS_FILE))
    else {
        return Ok(());
    };
    // External files use inventory indexes, not their historical relative paths.
    let payload = root.join(super::external::PAYLOAD);
    let identity = plan
        .payload_identity
        .as_deref()
        .ok_or_else(|| invalid(&payload, "Archive payload has no committed identity."))?;
    let _payload = files::guard_identity(&payload, identity)?;
    if !file.stored {
        return Err(invalid(
            &payload,
            "Moria permissions were not retained in the archive.",
        ));
    }
    let path = payload.join(format!("{index:06}"));
    let bytes = crate::instance_file_patch::io::read_bytes(&path)?;
    if bytes.len() as u64 != file.bytes || store::digest(&bytes) != file.sha256 {
        return Err(invalid(
            &path,
            "Archived Moria permissions differ from their recovery snapshot.",
        ));
    }
    let permissions = String::from_utf8(bytes)
        .map_err(|_| invalid(&path, "Archived Moria permissions must contain UTF-8 text."))?;
    settings.insert("permissions_lines".to_owned(), permissions.into());
    Ok(())
}

fn saved_bool(row: &store::SavedRow, field: &str) -> Result<bool, StorageError> {
    match row[field].as_i64() {
        Some(0) => Ok(false),
        Some(1) => Ok(true),
        _ => Err(invalid(
            std::path::Path::new(".trash"),
            format!("Archived field {field} is not a stored boolean."),
        )),
    }
}

fn saved_u32(row: &store::SavedRow, field: &str) -> Result<u32, StorageError> {
    row[field]
        .as_u64()
        .and_then(|number| u32::try_from(number).ok())
        .ok_or_else(|| {
            invalid(
                std::path::Path::new(".trash"),
                format!("Archived field {field} is not an unsigned 32-bit integer."),
            )
        })
}

fn saved_optional_string(
    row: &store::SavedRow,
    field: &str,
) -> Result<Option<String>, StorageError> {
    match &row[field] {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(value) => Ok(Some(value.clone())),
        _ => Err(invalid(
            std::path::Path::new(".trash"),
            format!("Archived field {field} is not optional text."),
        )),
    }
}
