//! Explicit role-file publication while general running configuration stays protected.
use std::fs;
use std::path::Path;

use app_core::InstanceDetails;
use serde_json::Value;

use crate::instance_file_patch::io::{guard_directories, read_bytes_with_limit};
use crate::instance_isolation::ensure_instance_paths_available;
use crate::instance_isolation::paths::{contains, normalize_path};
use crate::instance_settings_lock::acquire_instance_settings_mutation_lock;
use crate::instances::effective_instance_install_root;
use crate::player_access_normalization::normalize_valheim_platform_id;
use crate::storage_db::{connect_pool, fetch_instance_record};
use crate::templates::{
    ManagedConfigMergePlan, ManagedConfigMutation, render_valheim_platform_id_lines,
};
use crate::{StorageError, StoragePaths, read_instance_details};

const MAX_BYTES: usize = 512 * 1024;

fn invalid(path: &Path, message: impl Into<String>) -> StorageError {
    StorageError::InvalidInstancePath {
        path: path.to_owned(),
        message: message.into(),
    }
}

/// Publish only one saved Valheim role roster and its active native copy.
/// No settings, banned list, world file or startup override is rewritten here.
pub async fn materialize_valheim_player_roster(
    paths: &StoragePaths,
    instance_id: &str,
    field_key: &str,
) -> Result<InstanceDetails, StorageError> {
    let filename = match field_key {
        "admin_list" => "adminlist.txt",
        "permitted_list" => "permittedlist.txt",
        _ => {
            return Err(invalid(
                &paths.instances_root,
                "Only Valheim admin_list or permitted_list may be published live.",
            ));
        }
    };
    let lock = acquire_instance_settings_mutation_lock(paths, instance_id)?;
    let worker_lock = lock.clone();
    let paths = paths.clone();
    let instance_id = instance_id.to_owned();
    let field_key = field_key.to_owned();
    lock.complete_mutation("publishing a Valheim role roster", async move {
        let details = read_instance_details(&paths, &instance_id).await?;
        if details.summary.module_id != "valheim" {
            return Err(invalid(
                Path::new(&details.config_file_path),
                "Role roster publication requires a Valheim instance.",
            ));
        }
        let pool = connect_pool(&paths).await?;
        let validation = async {
            let record = fetch_instance_record(&pool, &instance_id).await?;
            let install = effective_instance_install_root(&record)?;
            let mut connection = pool.acquire().await?;
            ensure_instance_paths_available(
                &paths,
                &mut connection,
                &instance_id,
                "valheim",
                &install,
                &record.config_dir,
                Path::new(&details.saves_path),
            )
            .await
        }
        .await;
        pool.close().await;
        validation?;
        worker_lock
            .spawn_blocking(move || publish(details, &field_key, filename))
            .await
            .map_err(|error| StorageError::BlockingTaskFailed {
                operation: "publishing a Valheim role roster",
                message: error.to_string(),
            })?
    })
    .await
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, StorageError> {
    match fs::symlink_metadata(path) {
        Ok(_) => read_bytes_with_limit(path, MAX_BYTES).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(StorageError::ReadPath {
            path: path.to_owned(),
            source,
        }),
    }
}

fn publish(
    details: InstanceDetails,
    field_key: &str,
    filename: &str,
) -> Result<InstanceDetails, StorageError> {
    let config = Path::new(&details.config_file_path);
    let config_dir = config
        .parent()
        .ok_or_else(|| invalid(config, "Configuration directory is missing."))?;
    let instance_root = config_dir
        .parent()
        .ok_or_else(|| invalid(config, "Instance directory is missing."))?;
    let saves_dir = Path::new(&details.saves_path);
    let _config_guards = guard_directories(config_dir)?;
    let _save_guards = guard_directories(saves_dir)?;
    let root = normalize_path(instance_root)?;
    let saves = normalize_path(saves_dir)?;
    if saves == root || !contains(&root, &saves) {
        return Err(invalid(
            saves_dir,
            "Live Valheim role rosters must stay inside their instance directory.",
        ));
    }
    let original_config = read_bytes_with_limit(config, MAX_BYTES)?;
    let document: Value = serde_json::from_slice(
        original_config
            .strip_prefix(b"\xef\xbb\xbf")
            .unwrap_or(&original_config),
    )
    .map_err(|source| StorageError::InvalidConfigJson {
        path: config.to_owned(),
        source,
    })?;
    let settings = document
        .get("settings")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| invalid(config, "Committed instance settings must be an object."))?;
    let expected: Value = serde_json::from_str(&details.settings_json)?;
    if Value::Object(settings.clone()) != expected {
        return Err(invalid(
            config,
            "Valheim settings changed before role publication. Reload before trying again.",
        ));
    }
    let invalid_roster = || StorageError::InvalidModuleSetting {
        module_id: "valheim".to_owned(),
        field: field_key.to_owned(),
        message:
            "The selected platform roster contains an invalid value; existing files were preserved."
                .to_owned(),
    };
    let raw = match settings.get(field_key) {
        None => "",
        Some(Value::String(value)) => value,
        Some(_) => return Err(invalid_roster()),
    };
    // Match the authoritative renderer's blank/comment semantics, but reject
    // invalid entries instead of silently dropping operators' persisted data.
    if raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with("//"))
        .any(|line| normalize_valheim_platform_id(line).is_none())
    {
        return Err(invalid_roster());
    }
    let rendered = format!(
        "{}\n",
        render_valheim_platform_id_lines(&settings, field_key)
    )
    .into_bytes();
    let destinations = [config_dir.join(filename), saves_dir.join(filename)];
    let plans = destinations
        .iter()
        .map(|path| {
            Ok(ManagedConfigMergePlan {
                destination_path: path.clone(),
                replacement: rendered.clone(),
                original: read_optional(path)?,
            })
        })
        .collect::<Result<Vec<_>, StorageError>>()?;
    let mut files = ManagedConfigMutation::new("valheim");
    let result = (|| {
        if read_bytes_with_limit(config, MAX_BYTES)? != original_config {
            return Err(invalid(
                config,
                "Valheim settings changed during role preparation.",
            ));
        }
        files.apply(plans)?;
        for path in &destinations {
            if read_bytes_with_limit(path, MAX_BYTES)? != rendered {
                return Err(invalid(
                    path,
                    "Valheim role roster publication could not be verified.",
                ));
            }
        }
        if read_bytes_with_limit(config, MAX_BYTES)? != original_config {
            return Err(invalid(
                config,
                "Valheim settings changed during role publication.",
            ));
        }
        Ok(())
    })();
    match result {
        Ok(()) => {
            files.commit();
            Ok(details)
        }
        Err(error) => Err(files.rollback_after(error)),
    }
}
