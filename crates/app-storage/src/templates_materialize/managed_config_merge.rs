use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::StorageError;
#[path = "managed_ini_merge.rs"]
mod managed_ini_merge;
pub(super) use managed_ini_merge::{merge_ini_documents, parse_ini_document};
#[path = "managed_config_merge_plan.rs"]
mod managed_config_merge_plan;
pub(crate) use managed_config_merge_plan::ManagedConfigMergePlan;
pub(crate) use managed_config_merge_plan::ManagedConfigMutation;
#[cfg(test)]
pub(super) use managed_config_merge_plan::{
    apply_managed_config_plans_with_writer, apply_pending_managed_config_plans,
    write_managed_config_plan,
};

pub(super) struct ManagedIniFile<'a> {
    pub source_path: &'a Path,
    pub destination_path: &'a Path,
    pub removed_sections: &'a [&'a str],
}

pub(super) enum ManagedConfigFile<'a> {
    Ini {
        source_path: &'a Path,
        destination_path: &'a Path,
        removed_sections: &'a [&'a str],
    },
    JsonObject {
        source_path: &'a Path,
        destination_path: &'a Path,
    },
    Text {
        source_path: &'a Path,
        destination_path: &'a Path,
    },
}

pub(super) fn merge_rendered_ini_file(
    source_path: &Path,
    destination_path: &Path,
    module_id: &str,
    removed_sections: &[&str],

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let plan = plan_rendered_ini_file(source_path, destination_path, module_id, removed_sections)?;
    files.apply(vec![plan])
}

pub(super) fn merge_rendered_ini_files(
    configs: &[ManagedIniFile<'_>],
    module_id: &str,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let configs = configs
        .iter()
        .map(|file| ManagedConfigFile::Ini {
            source_path: file.source_path,
            destination_path: file.destination_path,
            removed_sections: file.removed_sections,
        })
        .collect::<Vec<_>>();
    merge_rendered_config_files(&configs, module_id, files)
}

pub(super) fn merge_rendered_config_files(
    configs: &[ManagedConfigFile<'_>],
    module_id: &str,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let mut destinations = HashSet::new();
    let mut plans = Vec::with_capacity(configs.len());
    for file in configs {
        let destination_path = match file {
            ManagedConfigFile::Ini {
                destination_path, ..
            }
            | ManagedConfigFile::JsonObject {
                destination_path, ..
            }
            | ManagedConfigFile::Text {
                destination_path, ..
            } => *destination_path,
        };
        if !destinations.insert(destination_path.to_path_buf()) {
            return Err(materialization_error(
                module_id,
                destination_path,
                String::from("managed configuration destination is declared more than once"),
            ));
        }
        plans.push(match file {
            ManagedConfigFile::Ini {
                source_path,
                destination_path,
                removed_sections,
            } => {
                plan_rendered_ini_file(source_path, destination_path, module_id, removed_sections)?
            }
            ManagedConfigFile::JsonObject {
                source_path,
                destination_path,
            } => plan_rendered_json_object_file(source_path, destination_path, module_id)?,
            ManagedConfigFile::Text {
                source_path,
                destination_path,
            } => plan_rendered_text_file(source_path, destination_path, module_id)?,
        });
    }
    files.apply(plans)
}

fn plan_rendered_ini_file(
    source_path: &Path,
    destination_path: &Path,
    module_id: &str,
    removed_sections: &[&str],
) -> Result<ManagedConfigMergePlan, StorageError> {
    let rendered = read_required_rendered_file(source_path, module_id)?;
    let rendered_document = parse_ini_document(&rendered).map_err(|message| {
        materialization_error(
            module_id,
            source_path,
            format!("invalid rendered INI: {message}"),
        )
    })?;
    let original = read_optional_bytes(destination_path)?;

    let merged = match original.as_deref() {
        None => rendered,
        Some(existing) => {
            let existing = std::str::from_utf8(existing).map_err(|source| {
                materialization_error(
                    module_id,
                    destination_path,
                    format!("refusing to replace non-UTF-8 existing INI: {source}"),
                )
            })?;
            let existing_document = parse_ini_document(existing).map_err(|message| {
                materialization_error(
                    module_id,
                    destination_path,
                    format!("refusing to replace malformed existing INI: {message}"),
                )
            })?;
            merge_ini_documents(existing_document, rendered_document, removed_sections)
        }
    };

    Ok(ManagedConfigMergePlan {
        destination_path: destination_path.to_path_buf(),
        replacement: merged.into_bytes(),
        original,
    })
}

pub(super) fn merge_rendered_json_object_file(
    source_path: &Path,
    destination_path: &Path,
    module_id: &str,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let plan = plan_rendered_json_object_file(source_path, destination_path, module_id)?;
    files.apply(vec![plan])
}

fn plan_rendered_json_object_file(
    source_path: &Path,
    destination_path: &Path,
    module_id: &str,
) -> Result<ManagedConfigMergePlan, StorageError> {
    let rendered = read_required_rendered_file(source_path, module_id)?;
    plan_rendered_json_object_contents(source_path, &rendered, destination_path, module_id)
}

pub(super) fn plan_rendered_json_object_contents(
    source_path: &Path,
    rendered: &str,
    destination_path: &Path,
    module_id: &str,
) -> Result<ManagedConfigMergePlan, StorageError> {
    let rendered_object = parse_json_object(rendered).map_err(|message| {
        materialization_error(
            module_id,
            source_path,
            format!("invalid rendered JSON object: {message}"),
        )
    })?;
    let original = read_optional_bytes(destination_path)?;
    let mut merged = match original.as_deref() {
        Some(existing) => {
            let existing = std::str::from_utf8(existing).map_err(|source| {
                materialization_error(
                    module_id,
                    destination_path,
                    format!("refusing to replace non-UTF-8 existing JSON: {source}"),
                )
            })?;
            parse_json_object(existing).map_err(|message| {
                materialization_error(
                    module_id,
                    destination_path,
                    format!("refusing to replace malformed existing JSON object: {message}"),
                )
            })?
        }
        None => Map::new(),
    };

    merge_json_objects(&mut merged, rendered_object);
    let mut bytes = serde_json::to_vec_pretty(&Value::Object(merged)).map_err(|source| {
        materialization_error(
            module_id,
            destination_path,
            format!("failed to serialize merged JSON object: {source}"),
        )
    })?;
    bytes.push(b'\n');
    Ok(ManagedConfigMergePlan {
        destination_path: destination_path.to_path_buf(),
        replacement: bytes,
        original,
    })
}

fn plan_rendered_text_file(
    source_path: &Path,
    destination_path: &Path,
    module_id: &str,
) -> Result<ManagedConfigMergePlan, StorageError> {
    if !source_path.is_file() {
        return Err(materialization_error(
            module_id,
            source_path,
            String::from("rendered support file is missing"),
        ));
    }
    let replacement = fs::read(source_path).map_err(|source| StorageError::ReadConfig {
        path: source_path.to_path_buf(),
        source,
    })?;
    Ok(ManagedConfigMergePlan {
        destination_path: destination_path.to_path_buf(),
        replacement,
        original: read_optional_bytes(destination_path)?,
    })
}

fn parse_json_object(content: &str) -> Result<Map<String, Value>, String> {
    match serde_json::from_str::<Value>(content).map_err(|source| source.to_string())? {
        Value::Object(object) => Ok(object),
        _ => Err(String::from("top-level value must be an object")),
    }
}

fn merge_json_objects(existing: &mut Map<String, Value>, rendered: Map<String, Value>) {
    for (key, rendered_value) in rendered {
        match rendered_value {
            Value::Object(rendered_object) => match existing.get_mut(&key) {
                Some(Value::Object(existing_object)) => {
                    merge_json_objects(existing_object, rendered_object);
                }
                _ => {
                    existing.insert(key, Value::Object(rendered_object));
                }
            },
            value => {
                existing.insert(key, value);
            }
        }
    }
}

pub(super) fn read_required_rendered_file(
    path: &Path,
    module_id: &str,
) -> Result<String, StorageError> {
    if !path.is_file() {
        return Err(materialization_error(
            module_id,
            path,
            String::from("rendered support file is missing"),
        ));
    }
    fs::read_to_string(path).map_err(|source| StorageError::ReadConfig {
        path: path.to_path_buf(),
        source,
    })
}

pub(super) fn read_optional_bytes(path: &Path) -> Result<Option<Vec<u8>>, StorageError> {
    match fs::read(path) {
        Ok(content) => Ok(Some(content)),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(StorageError::ReadConfig {
            path: path.to_path_buf(),
            source,
        }),
    }
}

pub(super) fn materialization_error(module_id: &str, path: &Path, message: String) -> StorageError {
    StorageError::ModuleSupportMaterialization {
        module_id: module_id.to_string(),
        path: PathBuf::from(path),
        message,
    }
}
