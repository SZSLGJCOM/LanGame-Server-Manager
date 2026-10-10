use std::collections::BTreeMap;
use std::path::PathBuf;

use app_core::dragonwilds_world::{
    DragonwildsWorldMode, DragonwildsWorldSettingDefinition, DragonwildsWorldSettingsSnapshot,
    WriteDragonwildsWorldSettingsInput,
};
use serde::Deserialize;

use crate::atomic_file::compare_and_swap_file_atomically;
use crate::dragonwilds_save::{NativeWorldSettings, decode_world_settings, patch_world_settings};
use crate::instance_file_patch::io::guard_directories;
use crate::instance_file_patch::sha256;
use crate::instance_isolation::ensure_instance_paths_available;
use crate::instance_settings_lock::acquire_instance_settings_mutation_lock;
use crate::instances::effective_instance_install_root;
use crate::runtime::load_active_instance_run;
use crate::save_paths::{effective_instance_saves_dir, load_module_descriptor};
use crate::storage_db::{connect_pool, fetch_instance_record};
use crate::{InstanceStatus, StorageError, StoragePaths};

#[path = "dragonwilds_world_io.rs"]
mod io;
use io::{invalid, latest_world, read_world, world_file_name};

#[derive(Deserialize)]
struct NativeCatalog {
    settings: Vec<DragonwildsWorldSettingDefinition>,
}

fn definitions() -> Result<Vec<DragonwildsWorldSettingDefinition>, StorageError> {
    let catalog: NativeCatalog = serde_json::from_str(include_str!(
        "../../../modules/runescapedragonwilds/world-settings.json"
    ))?;
    if catalog.settings.len() != 66 {
        return Err(invalid(
            std::path::Path::new("world-settings.json"),
            "Unexpected Dragonwilds native catalog.",
        ));
    }
    Ok(catalog.settings)
}

struct WorldContext {
    saves: PathBuf,
    writable: bool,
    default_world_name: String,
}

async fn load_context(
    paths: &StoragePaths,
    instance_id: &str,
    write: bool,
) -> Result<WorldContext, StorageError> {
    let pool = connect_pool(paths).await?;
    let result = async {
        let record = fetch_instance_record(&pool, instance_id).await?;
        if record.summary.module_id != "runescapedragonwilds" {
            return Err(invalid(
                &record.config_dir,
                "World settings belong only to a Dragonwilds instance.",
            ));
        }
        let writable = matches!(record.summary.status, InstanceStatus::Stopped)
            && record.summary.active_process_count == 0
            && load_active_instance_run(&pool, instance_id)
                .await?
                .is_none();
        if write && !writable {
            return Err(invalid(
                &record.config_dir,
                "Stop the server before changing world settings.",
            ));
        }
        let install = effective_instance_install_root(&record)?;
        let descriptor = load_module_descriptor(paths, &record.summary.module_id)?;
        let saves = effective_instance_saves_dir(descriptor.as_ref(), &install, &record)?;
        let settings_path = record.config_dir.join("instance.json");
        let settings: serde_json::Value = serde_json::from_str(
            &crate::instances::read_instance_settings_json(&settings_path)?,
        )?;
        let default_world_name = settings
            .get("default_world_name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Ashenfall")
            .to_owned();
        let mut connection = pool.acquire().await?;
        ensure_instance_paths_available(
            paths,
            &mut connection,
            instance_id,
            &record.summary.module_id,
            &install,
            &record.config_dir,
            &saves,
        )
        .await?;
        Ok(WorldContext {
            saves,
            writable,
            default_world_name,
        })
    }
    .await;
    pool.close().await;
    result
}

#[cfg(test)]
#[path = "dragonwilds_world_settings_tests.rs"]
mod tests;

fn effective_values(
    native: &NativeWorldSettings,
    mode: DragonwildsWorldMode,
    definitions: &[DragonwildsWorldSettingDefinition],
) -> Result<BTreeMap<String, f64>, StorageError> {
    let mut values = BTreeMap::new();
    for definition in definitions {
        let preset = if mode == DragonwildsWorldMode::Custom {
            DragonwildsWorldMode::Normal
        } else {
            mode
        };
        let value = native
            .overrides
            .get(&definition.tag)
            .copied()
            .map(f64::from)
            .or_else(|| definition.preset_defaults.get(&preset).copied())
            .ok_or_else(|| {
                invalid(
                    std::path::Path::new("world-settings.json"),
                    format!("Missing native default for {}.", definition.tag),
                )
            })?;
        if !value.is_finite() {
            return Err(invalid(
                std::path::Path::new("world-settings.json"),
                "Native world contains a non-finite rule value.",
            ));
        }
        values.insert(definition.tag.clone(), value);
    }
    Ok(values)
}

fn snapshot(
    instance_id: &str,
    context: &WorldContext,
) -> Result<DragonwildsWorldSettingsSnapshot, StorageError> {
    let definitions = definitions()?;
    let Some(path) = latest_world(&context.saves, &context.default_world_name)? else {
        return Ok(DragonwildsWorldSettingsSnapshot {
            instance_id: instance_id.to_owned(),
            status: "empty".into(),
            world_file: None,
            world_name: Some(context.default_world_name.clone()),
            world_mode: None,
            revision: None,
            values: BTreeMap::new(),
            overrides: BTreeMap::new(),
            definitions,
            writable: false,
            message: None,
            backup_id: None,
        });
    };
    let bytes = read_world(&path)?;
    let native = decode_world_settings(&bytes).map_err(|message| invalid(&path, message))?;
    let mode = DragonwildsWorldMode::from_native(native.world_mode)
        .ok_or_else(|| invalid(&path, "Unknown native world mode; the save was preserved."))?;
    let values = effective_values(&native, mode, &definitions)?;
    let overrides = native
        .overrides
        .iter()
        .filter(|(tag, _)| definitions.iter().any(|definition| &definition.tag == *tag))
        .map(|(tag, value)| (tag.clone(), f64::from(*value)))
        .collect();
    Ok(DragonwildsWorldSettingsSnapshot {
        instance_id: instance_id.to_owned(),
        status: "ready".into(),
        world_file: Some(world_file_name(&path)?),
        world_name: Some(native.world_name),
        world_mode: Some(mode),
        revision: Some(sha256(&bytes)),
        values,
        overrides,
        definitions,
        writable: context.writable,
        message: None,
        backup_id: None,
    })
}

pub async fn read_dragonwilds_world_settings(
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<DragonwildsWorldSettingsSnapshot, StorageError> {
    let context = load_context(paths, instance_id, false).await?;
    let id = instance_id.to_owned();
    tokio::task::spawn_blocking(move || snapshot(&id, &context))
        .await
        .map_err(|error| StorageError::BlockingTaskFailed {
            operation: "reading Dragonwilds world settings",
            message: error.to_string(),
        })?
}

struct PreparedUpdate {
    path: PathBuf,
    original: Vec<u8>,
    replacement: Vec<u8>,
}

fn prepare(
    context: &WorldContext,
    input: &WriteDragonwildsWorldSettingsInput,
) -> Result<PreparedUpdate, StorageError> {
    let path = latest_world(&context.saves, &context.default_world_name)?.ok_or_else(|| {
        invalid(
            &context.saves,
            "No generated world save matches the configured world name.",
        )
    })?;
    if world_file_name(&path)? != input.world_file {
        return Err(invalid(
            &path,
            "The server's selected world changed. Reload world settings before saving.",
        ));
    }
    let bytes = read_world(&path)?;
    if sha256(&bytes) != input.expected_revision {
        return Err(invalid(
            &path,
            "World settings changed outside this editor. Your draft is retained; reload before saving.",
        ));
    }
    let native = decode_world_settings(&bytes).map_err(|message| invalid(&path, message))?;
    let previous_mode = DragonwildsWorldMode::from_native(native.world_mode)
        .ok_or_else(|| invalid(&path, "Unknown native world mode; the save was preserved."))?;
    let definitions = definitions()?;
    if input.values.len() > definitions.len() {
        return Err(invalid(&path, "Too many world rule changes."));
    }
    let mut overrides = native.overrides.clone();
    // Entering Custom retains the current preset's effective gameplay values.
    if previous_mode != DragonwildsWorldMode::Custom
        && input.world_mode == DragonwildsWorldMode::Custom
    {
        let current = effective_values(&native, previous_mode, &definitions)?;
        // Locked/internal values are carried forward unchanged as part of the
        // preset conversion. This does not allow incoming patches to edit them.
        for definition in &definitions {
            if let Some(value) = current.get(&definition.tag) {
                overrides
                    .entry(definition.tag.clone())
                    .or_insert(*value as f32);
            }
        }
    }
    for (tag, value) in &input.values {
        let definition = definitions
            .iter()
            .find(|definition| &definition.tag == tag)
            .ok_or_else(|| invalid(&path, "Unknown world rule; the save was preserved."))?;
        if !definition.editable_in(input.world_mode) {
            return Err(invalid(
                &path,
                format!("{} cannot be changed for this existing world mode.", tag),
            ));
        }
        let step = 10_f64.powi(-(definition.decimal_places as i32));
        let scaled = value / step;
        if !value.is_finite()
            || *value < definition.minimum
            || *value > definition.maximum
            || (scaled - scaled.round()).abs() > 0.00001
            || (definition.kind == "boolean" && *value != 0.0 && *value != 1.0)
        {
            return Err(invalid(
                &path,
                format!("{} must match its native range and precision.", tag),
            ));
        }
        overrides.insert(tag.clone(), *value as f32);
    }
    let replacement = patch_world_settings(&bytes, input.world_mode.native_value(), &overrides)
        .map_err(|message| invalid(&path, message))?;
    if replacement.len() > io::MAX_WORLD_BYTES {
        return Err(invalid(
            &path,
            "Updated world exceeds the 128 MiB safety limit.",
        ));
    }
    Ok(PreparedUpdate {
        path,
        original: bytes,
        replacement,
    })
}

pub async fn write_dragonwilds_world_settings(
    paths: &StoragePaths,
    input: WriteDragonwildsWorldSettingsInput,
) -> Result<DragonwildsWorldSettingsSnapshot, StorageError> {
    let lease = acquire_instance_settings_mutation_lock(paths, &input.instance_id)?;
    let paths = paths.clone();
    lease.complete_mutation("saving Dragonwilds world settings", async move {
        let context = load_context(&paths, &input.instance_id, true).await?;
        let prepared_context = WorldContext { saves: context.saves.clone(), writable: true, default_world_name: context.default_world_name.clone() };
        let prepare_input = input.clone();
        let prepared = tokio::task::spawn_blocking(move || prepare(&prepared_context, &prepare_input)).await
            .map_err(|error| StorageError::BlockingTaskFailed { operation: "preparing Dragonwilds world settings", message: error.to_string() })??;
        if prepared.original == prepared.replacement {
            let id = input.instance_id.clone();
            return tokio::task::spawn_blocking(move || snapshot(&id, &context)).await
                .map_err(|error| StorageError::BlockingTaskFailed { operation: "reading unchanged Dragonwilds world", message: error.to_string() })?;
        }
        let backup = crate::backups::create_instance_backup(&paths, &input.instance_id).await?;
        let context = load_context(&paths, &input.instance_id, true).await?;
        let id = input.instance_id;
        tokio::task::spawn_blocking(move || {
            let _guards = guard_directories(&context.saves)?;
            if latest_world(&context.saves, &context.default_world_name)?.as_ref() != Some(&prepared.path) {
                return Err(invalid(&prepared.path, "The selected world changed while its backup was created."));
            }
            let accepted = compare_and_swap_file_atomically(&prepared.path, &prepared.original, &prepared.replacement)
                .map_err(|error| invalid(&prepared.path, error.to_string()))?;
            if !accepted {
                return Err(invalid(&prepared.path, "World changed while saving; the original and safety backup were preserved. Reload before retrying."));
            }
            if read_world(&prepared.path)? != prepared.replacement {
                return Err(invalid(&prepared.path, "World write could not be confirmed. A pre-change backup is available in Maintenance."));
            }
            let mut result = snapshot(&id, &context)?;
            result.backup_id = Some(backup.backup_id);
            Ok(result)
        }).await.map_err(|error| StorageError::BlockingTaskFailed { operation: "committing Dragonwilds world settings", message: error.to_string() })?
    }).await
}
