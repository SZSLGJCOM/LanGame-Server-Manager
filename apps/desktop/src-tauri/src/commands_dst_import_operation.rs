use super::commands_dst_import_validation::ValidatedDstImportSource;
use super::commands_dst_import_validation::find_case_insensitive_child_dir;
use super::*;
use serde_json::Map;

pub(super) struct DstWorldReplacement {
    pub(super) result: DstWorldImportResult,
    pub(super) safeguard: InstanceBackupResult,
}

type ModPreparation = for<'a> fn(
    &'a StorageBootstrap,
    &'a StorageContextOperationGuard,
    &'a InstanceDetails,
    &'a [String],
    app_network::SourcePreference,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>,
>;

fn prepare_required_mods<'a>(
    storage: &'a StorageBootstrap,
    operation: &'a StorageContextOperationGuard,
    details: &'a InstanceDetails,
    ids: &'a [String],
    preference: app_network::SourcePreference,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
    Box::pin(prepare_dst_import_workshop_items(
        storage, operation, details, ids, preference,
    ))
}

/// The caller owns the stopped instance mutation for the entire future. Both
/// external imports and confirmed backup restores publish world + settings.
pub(super) async fn replace_dst_world_locked(
    storage: &StorageBootstrap,
    operation: &StorageContextOperationGuard,
    instance_id: &str,
    source_path: PathBuf,
    prepared: Option<app_storage::PreparedInstanceBackupRestore>,
    preference: app_network::SourcePreference,
) -> Result<DstWorldReplacement, String> {
    replace_dst_world_with_mod_preparation(
        storage,
        operation,
        instance_id,
        source_path,
        prepared,
        preference,
        prepare_required_mods,
    )
    .await
}

async fn replace_dst_world_with_mod_preparation(
    storage: &StorageBootstrap,
    operation: &StorageContextOperationGuard,
    instance_id: &str,
    source_path: PathBuf,
    prepared: Option<app_storage::PreparedInstanceBackupRestore>,
    preference: app_network::SourcePreference,
    prepare_mods: ModPreparation,
) -> Result<DstWorldReplacement, String> {
    if read_active_instance_run(&storage.paths, instance_id)
        .await
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Err("Stop the server before replacing DST world data.".into());
    }
    let details = read_instance_details(&storage.paths, instance_id)
        .await
        .map_err(|error| error.to_string())?;
    if details.summary.module_id != "dontstarve" {
        return Err("World replacement requires a DST instance.".into());
    }
    if let Some(prepared) = &prepared {
        app_storage::revalidate_prepared_instance_backup_restore(
            &storage.paths,
            instance_id,
            prepared,
        )
        .await
        .map_err(|error| error.to_string())?;
    }
    let restore = prepared.is_some();
    let target = dontstarve_cluster_root_from_config_file_path(&details.config_file_path);
    let caves_enabled = dst_caves_enabled(&details.settings_json)?;
    let validation_target = target.clone();
    let initial_settings: Value =
        serde_json::from_str(&details.settings_json).map_err(|error| error.to_string())?;
    let settings_snapshot = prepared
        .as_ref()
        .map(|value| value.dst_settings_snapshot())
        .transpose()
        .map_err(|error| error.to_string())?
        .flatten();
    let (source, mod_plan, settings) = tokio::task::spawn_blocking(move || {
        let source = if restore {
            validate_dontstarve_restore_source(&source_path)?
        } else {
            validate_dontstarve_import_source(&source_path)?
        };
        validate_dontstarve_import_paths(&source.cluster_root, &validation_target)?;
        if !restore {
            validate_dontstarve_import_policy(&source, caves_enabled)?;
        }
        prepare_dontstarve_target_cluster(&validation_target)?;
        let (plan, settings) =
            replacement_settings(&source, initial_settings, settings_snapshot, restore)?;
        Ok::<_, String>((source, plan, settings))
    })
    .await
    .map_err(|error| format!("DST source validation task failed: {error}"))??;
    let source_root = source.cluster_root.clone();
    let settings_json = serde_json::to_string(&settings).map_err(|error| error.to_string())?;
    let descriptors =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    let descriptor = find_descriptor(&descriptors, "dontstarve")?;
    let mut ports = details.ports.clone();
    for port in &descriptor.default_ports {
        if !ports.iter().any(|existing| existing.name == port.name) {
            ports.push(port.clone());
        }
    }
    prepare_mods(storage, operation, &details, &mod_plan.workshop_ids, preference).await
        .map_err(|error| format!("Cannot prepare the saved world's required Mods; world and settings were not replaced: {error}"))?;
    if let Some(prepared) = &prepared {
        app_storage::revalidate_prepared_instance_backup_restore(
            &storage.paths,
            instance_id,
            prepared,
        )
        .await
        .map_err(|error| error.to_string())?;
    }
    let (source, mod_plan) = tokio::task::spawn_blocking(move || {
        source.verify_unchanged()?;
        mod_plan.verify_unchanged()?;
        Ok::<_, String>((source, mod_plan))
    })
    .await
    .map_err(|error| format!("DST source revalidation task failed: {error}"))??;
    let imported_workshop_mod_ids = mod_plan.workshop_ids.clone();
    let safeguard = if let Some(prepared) = &prepared {
        app_storage::create_instance_pre_restore_backup(
            &storage.paths,
            instance_id,
            &prepared.backup.backup_id,
        )
        .await
    } else {
        create_instance_backup_snapshot(&storage.paths, instance_id).await
    }
    .map_err(|error| format!("Failed to create the pre-replacement backup: {error}"))?;
    let import_target = target.clone();
    let published = tokio::task::spawn_blocking(move || {
        if let Some(prepared) = prepared {
            restore_dontstarve_world_transaction(&source, &import_target, &prepared)
        } else {
            import_dontstarve_world_transaction(&source, &import_target, caves_enabled)
        }
    })
    .await
    .map_err(|error| format!("DST world publication task failed: {error}"))?
    .map_err(|error| format!("{error} Pre-replacement backup: {}.", safeguard.backup_path))?;
    let update = UpdateInstanceInput {
        id: instance_id.into(),
        bind_ip: details.summary.bind_ip.clone(),
        auto_backup_on_stop: details.auto_backup_on_stop,
        backup_retention_count: details.backup_retention_count,
        settings_json,
        ports,
    };
    if let Err(error) =
        app_storage::update_instance_if_current(&storage.paths, update, &details.settings_json)
            .await
    {
        let rollback = tokio::task::spawn_blocking(move || published.rollback())
            .await
            .map_err(|failure| format!("DST world rollback task failed: {failure}"))?;
        return match rollback {
            Ok(()) => Err(format!(
                "Failed to save restored DST configuration; original world and configuration were restored: {error}. Safeguard: {}.",
                safeguard.backup_path
            )),
            Err(rollback) => Err(format!(
                "Failed to save restored DST configuration: {error}; world rollback failed: {rollback}. Restore safeguard at {}.",
                safeguard.backup_path
            )),
        };
    }
    let transaction = tokio::task::spawn_blocking(move || published.commit())
        .await
        .map_err(|error| format!("DST world commit task failed: {error}"))?;
    Ok(DstWorldReplacement {
        result: DstWorldImportResult {
            instance_id: instance_id.into(),
            source_cluster_path: source_root.to_string_lossy().into_owned(),
            target_cluster_path: target.to_string_lossy().into_owned(),
            safeguard_path: safeguard.backup_path.clone(),
            imported_master: transaction.imported_master,
            imported_caves: transaction.imported_caves,
            imported_shards: transaction.imported_shards,
            imported_workshop_mod_ids,
            copied_file_count: transaction.stats.file_count,
            copied_total_bytes: transaction.stats.total_bytes,
        },
        safeguard,
    })
}

pub(super) async fn restore_dst_backup_locked(
    storage: &StorageBootstrap,
    operation: &StorageContextOperationGuard,
    instance_id: &str,
    prepared: app_storage::PreparedInstanceBackupRestore,
    preference: app_network::SourcePreference,
) -> Result<InstanceBackupRestoreResult, String> {
    let backup_id = prepared.backup.backup_id.clone();
    let source = Path::new(&prepared.backup.backup_path).join("saves");
    let replacement = replace_dst_world_locked(
        storage,
        operation,
        instance_id,
        source,
        Some(prepared),
        preference,
    )
    .await?;
    Ok(InstanceBackupRestoreResult {
        instance_id: instance_id.into(),
        backup_id,
        restored_at_unix_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        saves_path: replacement.result.target_cluster_path,
        restored_file_count: replacement.result.copied_file_count,
        restored_total_bytes: replacement.result.copied_total_bytes,
        safeguard_backup_id: replacement.safeguard.backup_id,
        safeguard_backup_path: replacement.safeguard.backup_path,
    })
}

pub(super) async fn prepare_dst_backup_restore_state(
    storage: &StorageBootstrap,
    current: &InstanceDetails,
    prepared: &app_storage::PreparedInstanceBackupRestore,
) -> Result<InstanceDetails, String> {
    app_storage::revalidate_prepared_instance_backup_restore(
        &storage.paths,
        &current.summary.id,
        prepared,
    )
    .await
    .map_err(|error| error.to_string())?;
    let descriptors =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    let descriptor = find_descriptor(&descriptors, "dontstarve")?.clone();
    if descriptor.default_ports.iter().any(|port| {
        !current
            .ports
            .iter()
            .any(|existing| existing.name == port.name)
    }) {
        return Err("The restored world requires missing port bindings. Update the server configuration separately before restoring.".into());
    }
    let mut expected = current.clone();
    let source_path = Path::new(&prepared.backup.backup_path).join("saves");
    let snapshot = prepared
        .dst_settings_snapshot()
        .map_err(|error| error.to_string())?;
    let expected = tokio::task::spawn_blocking(move || {
        let source = validate_dontstarve_restore_source(&source_path)?;
        let initial =
            serde_json::from_str(&expected.settings_json).map_err(|error| error.to_string())?;
        let (_, settings) = replacement_settings(&source, initial, snapshot, true)?;
        let normalized = app_storage::normalize_complete_instance_settings(
            Some(&descriptor),
            settings,
            &expected.summary.id,
            &expected.summary.name,
            &expected.summary.bind_ip,
        )
        .map_err(|error| error.to_string())?;
        expected.settings_json = serde_json::to_string_pretty(&Value::Object(normalized))
            .map_err(|error| error.to_string())?;
        Ok::<_, String>(expected)
    })
    .await
    .map_err(|error| format!("DST restore state preparation failed: {error}"))??;
    app_storage::revalidate_prepared_instance_backup_restore(
        &storage.paths,
        &current.summary.id,
        prepared,
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(expected)
}

fn replacement_settings(
    source: &ValidatedDstImportSource,
    initial_settings: Value,
    settings_snapshot: Option<Value>,
    restore: bool,
) -> Result<
    (
        super::dst_import_mods::DstImportedModPlan,
        Map<String, Value>,
    ),
    String,
> {
    let mut settings = initial_settings
        .as_object()
        .cloned()
        .ok_or("DST settings must be an object.")?;
    let mut owned_roots = Vec::new();
    if restore {
        // Inactive shard settings belong to the backup too; generated saves
        // determine topology, while an empty snapshot has explicit topology.
        for spec in app_core::dst_shards::DST_SHARDS {
            if let Some(root) =
                find_case_insensitive_child_dir(&source.cluster_root, spec.directory)
            {
                owned_roots.push((spec.process_key, root));
            }
        }
    } else {
        owned_roots.extend(
            source
                .shards()
                .into_iter()
                .map(|(key, root)| (key, root.to_owned())),
        );
    }
    let shards = owned_roots
        .iter()
        .map(|(key, root)| (*key, root.as_path()))
        .collect::<Vec<_>>();
    let plan = super::dst_import_mods::read_import_mods(&shards)?;
    plan.apply(&mut settings)?;
    if source.master_saved {
        settings.insert(
            "shard_layout".into(),
            json!(if source.is_island_adventures() {
                "island_adventures"
            } else {
                "standard"
            }),
        );
        if restore || source.is_island_adventures() {
            settings.insert("enable_caves".into(), json!(source.caves_root.is_some()));
        }
    } else if let Some(snapshot) = settings_snapshot {
        let saved = snapshot
            .get("settings")
            .and_then(Value::as_object)
            .ok_or("DST backup canonical settings must be an object.")?;
        for key in ["shard_layout", "enable_caves"] {
            if let Some(value) = saved.get(key) {
                settings.insert(key.into(), value.clone());
            }
        }
    }
    if restore {
        restore_generation_settings(&owned_roots, &mut settings)?;
    }
    Ok((plan, settings))
}

fn restore_generation_settings(
    roots: &[(&str, PathBuf)],
    settings: &mut Map<String, Value>,
) -> Result<(), String> {
    for spec in app_core::dst_shards::DST_SHARDS {
        let raw = roots
            .iter()
            .find(|(key, _)| *key == spec.process_key)
            .map(|(_, root)| read_static_world_override(&root.join("worldgenoverride.lua")))
            .transpose()?
            .flatten()
            .unwrap_or_default();
        settings.insert(
            format!("{}_worldgenoverride_lua", spec.process_key),
            Value::String(raw),
        );
        if let Some((_, root)) = roots.iter().find(|(key, _)| *key == spec.process_key) {
            read_static_world_override(&root.join("leveldataoverride.lua"))?;
        }
    }
    Ok(())
}

fn read_static_world_override(path: &Path) -> Result<Option<String>, String> {
    use std::io::Read;
    let metadata = match fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "Cannot inspect saved world options {}: {error}",
                path.display()
            ));
        }
    };
    if super::commands_dst_import_validation::is_link_or_reparse(&metadata)
        || !metadata.is_file()
        || metadata.len() > 120 * 1024
    {
        return Err(format!(
            "Saved world options {} must be a bounded plain Lua file.",
            path.display()
        ));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .and_then(|file| file.take(120 * 1024 + 1).read_to_end(&mut bytes))
        .map_err(|error| error.to_string())?;
    let raw = String::from_utf8(bytes).map_err(|error| error.to_string())?;
    let raw = raw.trim_start_matches('\u{feff}').to_owned();
    app_storage::extract_dst_static_lua_table(&raw, "overrides").map_err(|error| {
        format!(
            "Cannot restore saved world options {}: {error}",
            path.display()
        )
    })?;
    Ok(Some(raw))
}

#[cfg(test)]
#[path = "dst_import_roundtrip_tests.rs"]
mod tests;
