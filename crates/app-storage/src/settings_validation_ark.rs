use crate::StorageError;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashSet};

#[path = "settings_validation_ark_syntax.rs"]
mod syntax;
use syntax::{
    NativeValue, native_index, nonnegative_number, parse_tuple, validate_numeric_properties,
};

pub(super) fn validate_ark_settings(
    module_id: &str,
    settings: &Map<String, Value>,
    schema: &Value,
) -> Result<(), StorageError> {
    let invalid = |field: &str, message: String| StorageError::InvalidModuleSetting {
        module_id: module_id.to_owned(),
        field: field.to_owned(),
        message,
    };
    app_core::ark_maps::parse_additional_maps(&Value::Object(settings.clone()))
        .map_err(|error| invalid("additional_maps", error))?;
    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        for (field, definition) in properties {
            let Some(raw) = settings.get(field).and_then(Value::as_str) else {
                continue;
            };
            let Some(source) = definition.get("x-lsgm-source-key").and_then(Value::as_str) else {
                continue;
            };
            let Some(native) = source.strip_prefix("/script/shootergame.shootergamemode.") else {
                continue;
            };
            let native = native.split(['[', '<']).next().unwrap_or(native);
            let indexed = source.contains('[');
            let tuple = native.starts_with("Config")
                || native.ends_with("Multipliers")
                || matches!(
                    native,
                    "LevelExperienceRampOverrides"
                        | "OverrideNamedEngramEntries"
                        | "OverrideEngramEntries"
                        | "EngramEntryAutoUnlocks"
                        | "NPCReplacements"
                );
            if !indexed && !tuple && native != "OverridePlayerLevelEngramPoints" {
                continue;
            }
            let mut indexes = HashSet::new();
            for (line_index, line) in raw.lines().enumerate() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let result = if indexed {
                    validate_indexed(line, native, &mut indexes)
                } else {
                    let value = line
                        .strip_prefix(native)
                        .and_then(|rest| rest.trim_start().strip_prefix('='))
                        .unwrap_or(line)
                        .trim();
                    if native == "OverridePlayerLevelEngramPoints" {
                        nonnegative_number(value, true).map(|_| ())
                    } else {
                        parse_tuple(value).and_then(|rule| {
                            validate_numeric_properties(&rule)?;
                            if native == "LevelExperienceRampOverrides" {
                                validate_experience(&rule)?;
                            }
                            Ok(())
                        })
                    }
                };
                result
                    .map_err(|error| invalid(field, format!("line {}: {error}", line_index + 1)))?;
            }
        }
    }
    for field in ["game_ini_extra", "game_user_settings_extra"] {
        if let Some(raw) = settings.get(field).and_then(Value::as_str) {
            validate_extra(raw).map_err(|error| invalid(field, error))?;
        }
    }
    app_core::ark_cluster::resolve_cluster_directory(
        settings
            .get("cluster_id")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        settings
            .get("cluster_directory")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        std::path::Path::new(""),
    )
    .map_err(|error| invalid(error.field, error.message.to_owned()))?;
    Ok(())
}

fn validate_indexed(
    line: &str,
    native: &str,
    indexes: &mut HashSet<(String, u64)>,
) -> Result<(), String> {
    let (key, raw) = line
        .split_once('=')
        .ok_or_else(|| String::from("indexed setting requires index=value"))?;
    let mut key = key.trim().strip_prefix(native).unwrap_or(key.trim());
    let mut kind = "";
    if native == "PerLevelStatsMultiplier_DinoTamed" {
        for suffix in ["_Add", "_Affinity"] {
            if let Some(index) = key.strip_prefix(suffix) {
                key = index;
                kind = suffix;
                break;
            }
        }
    }
    let key = if key.starts_with('[') {
        key.strip_prefix('[')
            .and_then(|key| key.strip_suffix(']'))
            .ok_or_else(|| String::from("indexed setting needs matching brackets"))?
    } else {
        key
    };
    let index = native_index(key)?;
    if !indexes.insert((kind.to_owned(), index)) {
        return Err(String::from("duplicate native index"));
    }
    nonnegative_number(raw.trim(), false)?;
    Ok(())
}

fn validate_experience(rule: &NativeValue<'_>) -> Result<(), String> {
    let NativeValue::Group(entries) = rule else {
        return Err(String::from("experience ramp must be a native tuple"));
    };
    let mut levels = BTreeMap::new();
    for (name, value) in entries {
        let Some(name) = name else {
            return Err(String::from(
                "experience entries require named level indexes",
            ));
        };
        let Some(index) = name
            .strip_prefix("ExperiencePointsForLevel[")
            .and_then(|index| index.strip_suffix(']'))
        else {
            // Preserve valid extension properties; only the native indexed family is owned here.
            if name.starts_with("ExperiencePointsForLevel") {
                return Err(String::from(
                    "experience entry requires a bracketed level index",
                ));
            }
            continue;
        };
        let index = native_index(index)?;
        let NativeValue::Scalar(raw) = value else {
            return Err(String::from("experience points must be numeric"));
        };
        let points = nonnegative_number(raw, false)?;
        if levels.insert(index, points).is_some() {
            return Err(String::from("duplicate experience level index"));
        }
    }
    if levels.is_empty() {
        return Err(String::from("experience ramp needs at least one level"));
    }
    let mut previous = None;
    for points in levels.values() {
        if previous.is_some_and(|previous| points <= previous) {
            return Err(String::from(
                "experience points must increase with level index",
            ));
        }
        previous = Some(points);
    }
    Ok(())
}

fn validate_extra(raw: &str) -> Result<(), String> {
    for (index, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with(';')
            || line.starts_with('#')
            || line.starts_with("//")
        {
            continue;
        }
        let valid = if line.starts_with('[') {
            line.find(']').is_some_and(|end| {
                end > 1 && !line[1..end].contains('[') && {
                    let trailing = line[end + 1..].trim();
                    trailing.is_empty()
                        || trailing.starts_with(';')
                        || trailing.starts_with('#')
                        || trailing.starts_with("//")
                }
            })
        } else {
            line.split_once('=')
                .is_some_and(|(key, _)| !key.trim().is_empty())
        };
        if !valid {
            return Err(format!(
                "line {} must be an INI section or key=value assignment",
                index + 1
            ));
        }
    }
    Ok(())
}
