use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::atomic_file::compare_and_swap_optional_file_atomically;

use super::windrose_document::{
    merge_json, optional_setting_string, parse_json, render_world_description, require_identity,
    required_string_at, serialize_json,
};
use super::{
    WINDROSE_SERVER_DESCRIPTION_FILE, WindroseWorldTargetError, read_bytes,
    resolve_world_description, validate_destination,
};

pub(super) const WINDROSE_WORLD_UPDATE_PLAN_FILE: &str = "windrose-world-update-plan.json";

pub(super) struct PreparedWindroseDocuments {
    pub(super) selected: String,
    pub(super) server_path: PathBuf,
    pub(super) server_original: Option<Vec<u8>>,
    pub(super) server_replacement: Vec<u8>,
    pub(super) world: Option<PreparedWindroseWorld>,
}

pub(super) struct PreparedWindroseWorld {
    pub(super) path: PathBuf,
    pub(super) original: Vec<u8>,
    pub(super) replacement: Vec<u8>,
}

#[cfg(test)]
pub(super) fn materialize_windrose_documents(
    install_root: &Path,
    config_dir: &Path,
    settings: &Map<String, Value>,
    instance_running: bool,
) -> Result<(), WindroseWorldTargetError> {
    let mut files = super::super::ManagedConfigMutation::new("windrose");
    match materialize_windrose_documents_pending(
        install_root,
        config_dir,
        settings,
        instance_running,
        &mut files,
    ) {
        Ok(()) => {
            files.commit();
            Ok(())
        }
        Err(error) => {
            files
                .rollback()
                .map_err(|rollback| configuration_error(config_dir, rollback))?;
            Err(error)
        }
    }
}

pub(super) fn materialize_windrose_documents_pending(
    install_root: &Path,
    config_dir: &Path,
    settings: &Map<String, Value>,
    instance_running: bool,
    files: &mut super::super::ManagedConfigMutation,
) -> Result<(), WindroseWorldTargetError> {
    let prepared = prepare_windrose_documents(install_root, config_dir, settings)?;
    let Some(world) = prepared.world.as_ref() else {
        files
            .remove(&pending_plan_path(config_dir))
            .map_err(|error| configuration_error(config_dir, error))?;
        return files
            .apply(vec![
                super::super::managed_config_merge::ManagedConfigMergePlan {
                    destination_path: prepared.server_path.clone(),
                    original: prepared.server_original,
                    replacement: prepared.server_replacement,
                },
            ])
            .map_err(|error| configuration_error(&prepared.server_path, error));
    };
    if world.original == world.replacement {
        let plan_path = pending_plan_path(config_dir);
        let keep_pending = if plan_path.is_file() {
            let persisted = parse_json(&plan_path, &read_bytes(&plan_path)?)?;
            persisted == pending_plan_document(install_root, config_dir, &prepared, settings)?
        } else {
            false
        };
        if !keep_pending {
            files
                .remove(&pending_plan_path(config_dir))
                .map_err(|error| configuration_error(config_dir, error))?;
        }
        return files
            .apply(vec![
                super::super::managed_config_merge::ManagedConfigMergePlan {
                    destination_path: prepared.server_path.clone(),
                    original: prepared.server_original,
                    replacement: prepared.server_replacement,
                },
            ])
            .map_err(|error| configuration_error(&prepared.server_path, error));
    }
    if instance_running {
        return Err(WindroseWorldTargetError::RunningWorldMutation);
    }

    let plan_path = pending_plan_path(config_dir);
    let plan = pending_plan_document(install_root, config_dir, &prepared, settings)?;
    let bytes = serialize_json(&plan_path, &plan)?;
    files
        .write(&plan_path, &bytes)
        .map_err(|error| configuration_error(&plan_path, error))
}

fn configuration_error(path: &Path, error: crate::StorageError) -> WindroseWorldTargetError {
    WindroseWorldTargetError::Replacement {
        path: path.to_path_buf(),
        source: std::io::Error::other(error),
    }
}

pub(super) fn prepare_windrose_documents(
    install_root: &Path,
    config_dir: &Path,
    settings: &Map<String, Value>,
) -> Result<PreparedWindroseDocuments, WindroseWorldTargetError> {
    let source_path = config_dir.join(WINDROSE_SERVER_DESCRIPTION_FILE);
    let mut rendered_server = parse_json(&source_path, &read_bytes(&source_path)?)?;
    let selected = optional_setting_string(settings, "world_island_id")?;
    let invite_code = optional_setting_string(settings, "invite_code")?;
    let persistent = rendered_server
        .get_mut("ServerDescription_Persistent")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| WindroseWorldTargetError::InvalidShape {
            path: source_path.clone(),
            field: String::from("ServerDescription_Persistent"),
        })?;
    // These identities belong to the native bootstrap. Empty form defaults
    // must not become invalid native identities or erase an existing world.
    persistent.remove("PersistentServerId");
    if selected.is_empty() {
        persistent.remove("WorldIslandId");
    }
    if invite_code.is_empty() {
        persistent.remove("InviteCode");
    }
    let server_path = install_root
        .join("R5")
        .join(WINDROSE_SERVER_DESCRIPTION_FILE);
    validate_destination(install_root, &server_path)?;
    let existing_server = match fs::read(&server_path) {
        Ok(original) => {
            let document = parse_json(&server_path, &original)?;
            Some((original, document))
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(WindroseWorldTargetError::Io {
                path: server_path.clone(),
                source,
            });
        }
    };
    let mut merged_server = existing_server
        .as_ref()
        .map(|(_, document)| document.clone())
        .unwrap_or_else(|| Value::Object(Map::new()));
    merge_json(&mut merged_server, &rendered_server, &server_path)?;
    if !selected.is_empty() {
        let proposed_id = required_string_at(
            &merged_server,
            &["ServerDescription_Persistent", "WorldIslandId"],
            &server_path,
        )?;
        require_identity(
            "rendered ServerDescription WorldIslandId",
            &selected,
            proposed_id,
        )?;
    }
    let server_replacement = serialize_json(&server_path, &merged_server)?;

    let world = if selected.is_empty() {
        None
    } else {
        let resolved = resolve_world_description(install_root, &selected)?;
        let replacement = render_world_description(&resolved, settings)?;
        Some(PreparedWindroseWorld {
            path: resolved.path,
            original: resolved.original,
            replacement,
        })
    };
    Ok(PreparedWindroseDocuments {
        selected,
        server_path,
        server_original: existing_server.map(|(original, _)| original),
        server_replacement,
        world,
    })
}

pub(super) fn pending_plan_document(
    install_root: &Path,
    config_dir: &Path,
    prepared: &PreparedWindroseDocuments,
    settings: &Map<String, Value>,
) -> Result<Value, WindroseWorldTargetError> {
    let world =
        prepared
            .world
            .as_ref()
            .ok_or_else(|| WindroseWorldTargetError::InvalidPendingPlan {
                path: pending_plan_path(config_dir),
                message: String::from("world target is absent"),
            })?;
    let canonical_install = super::canonicalize(install_root)?;
    let relative = world.path.strip_prefix(&canonical_install).map_err(|_| {
        WindroseWorldTargetError::OutsideInstallRoot {
            path: world.path.clone(),
            root: canonical_install.clone(),
        }
    })?;
    let mut parameters = Map::new();
    for key in [
        "world_name",
        "world_preset_type",
        "coop_quests",
        "easy_explore",
        "mob_health_multiplier",
        "mob_damage_multiplier",
        "ship_health_multiplier",
        "ship_damage_multiplier",
        "boarding_difficulty_multiplier",
        "coop_stats_correction_modifier",
        "coop_ship_stats_correction_modifier",
        "combat_difficulty",
    ] {
        parameters.insert(
            String::from(key),
            settings
                .get(key)
                .cloned()
                .ok_or(WindroseWorldTargetError::InvalidSetting { key })?,
        );
    }
    Ok(json!({
        "version": 1,
        "pending": true,
        "world_island_id": prepared.selected,
        "world_description_relative_path": relative.to_string_lossy().replace('\\', "/"),
        "world_parameters": parameters,
    }))
}

pub(super) fn pending_plan_path(config_dir: &Path) -> PathBuf {
    config_dir.join(WINDROSE_WORLD_UPDATE_PLAN_FILE)
}

pub(super) fn clear_pending_plan(config_dir: &Path) -> Result<(), WindroseWorldTargetError> {
    let path = pending_plan_path(config_dir);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(WindroseWorldTargetError::Io { path, source }),
    }
}

pub(super) fn replace_server(
    path: &Path,
    existing: Option<&[u8]>,
    replacement: &[u8],
) -> Result<(), WindroseWorldTargetError> {
    if existing.is_some_and(|original| original == replacement) {
        return Ok(());
    }
    let parent = path
        .parent()
        .ok_or_else(|| WindroseWorldTargetError::InvalidShape {
            path: path.to_path_buf(),
            field: String::from("destination parent"),
        })?;
    fs::create_dir_all(parent).map_err(|source| WindroseWorldTargetError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    if !compare_and_swap_optional_file_atomically(path, existing, Some(replacement)).map_err(
        |source| WindroseWorldTargetError::Replacement {
            path: path.to_path_buf(),
            source,
        },
    )? {
        return Err(WindroseWorldTargetError::ConcurrentModification {
            path: path.to_path_buf(),
        });
    }
    Ok(())
}
