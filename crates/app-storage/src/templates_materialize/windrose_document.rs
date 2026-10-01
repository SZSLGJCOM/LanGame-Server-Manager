use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use super::{ResolvedWindroseWorld, WindroseWorldTargetError};

pub(super) const COOP_QUESTS_TAG: &str = r#"{"TagName": "WDS.Parameter.Coop.SharedQuests"}"#;
pub(super) const EASY_EXPLORE_TAG: &str = r#"{"TagName": "WDS.Parameter.EasyExplore"}"#;
pub(super) const MOB_HEALTH_TAG: &str = r#"{"TagName": "WDS.Parameter.MobHealthMultiplier"}"#;
pub(super) const MOB_DAMAGE_TAG: &str = r#"{"TagName": "WDS.Parameter.MobDamageMultiplier"}"#;
pub(super) const SHIP_HEALTH_TAG: &str = r#"{"TagName": "WDS.Parameter.ShipsHealthMultiplier"}"#;
pub(super) const SHIP_DAMAGE_TAG: &str = r#"{"TagName": "WDS.Parameter.ShipsDamageMultiplier"}"#;
pub(super) const BOARDING_DIFFICULTY_TAG: &str =
    r#"{"TagName": "WDS.Parameter.BoardingDifficultyMultiplier"}"#;
pub(super) const COOP_STATS_TAG: &str =
    r#"{"TagName": "WDS.Parameter.Coop.StatsCorrectionModifier"}"#;
pub(super) const COOP_SHIP_STATS_TAG: &str =
    r#"{"TagName": "WDS.Parameter.Coop.ShipStatsCorrectionModifier"}"#;
pub(super) const COMBAT_DIFFICULTY_TAG: &str = r#"{"TagName": "WDS.Parameter.CombatDifficulty"}"#;

pub(super) fn render_world_description(
    resolved: &ResolvedWindroseWorld,
    settings: &Map<String, Value>,
) -> Result<Vec<u8>, WindroseWorldTargetError> {
    let mut document = resolved.document.clone();
    let description = object_at_mut(&mut document, &["WorldDescription"], &resolved.path)?;
    description.insert(
        String::from("WorldName"),
        Value::String(required_setting_string(settings, "world_name")?),
    );
    description.insert(
        String::from("WorldPresetType"),
        Value::String(required_setting_string(settings, "world_preset_type")?),
    );
    let world_settings = child_object_at_or_insert_mut(
        description,
        "WorldSettings",
        "WorldDescription.WorldSettings",
        &resolved.path,
    )?;
    {
        let bools = child_object_at_or_insert_mut(
            world_settings,
            "BoolParameters",
            "WorldDescription.WorldSettings.BoolParameters",
            &resolved.path,
        )?;
        bools.insert(
            String::from(COOP_QUESTS_TAG),
            Value::Bool(required_setting_bool(settings, "coop_quests")?),
        );
        bools.insert(
            String::from(EASY_EXPLORE_TAG),
            Value::Bool(required_setting_bool(settings, "easy_explore")?),
        );
    }
    {
        let floats = child_object_at_or_insert_mut(
            world_settings,
            "FloatParameters",
            "WorldDescription.WorldSettings.FloatParameters",
            &resolved.path,
        )?;
        for (tag, key) in [
            (MOB_HEALTH_TAG, "mob_health_multiplier"),
            (MOB_DAMAGE_TAG, "mob_damage_multiplier"),
            (SHIP_HEALTH_TAG, "ship_health_multiplier"),
            (SHIP_DAMAGE_TAG, "ship_damage_multiplier"),
            (BOARDING_DIFFICULTY_TAG, "boarding_difficulty_multiplier"),
            (COOP_STATS_TAG, "coop_stats_correction_modifier"),
            (COOP_SHIP_STATS_TAG, "coop_ship_stats_correction_modifier"),
        ] {
            let number = serde_json::Number::from_f64(required_setting_number(settings, key)?)
                .ok_or(WindroseWorldTargetError::InvalidSetting { key })?;
            floats.insert(String::from(tag), Value::Number(number));
        }
    }
    let tags = child_object_at_or_insert_mut(
        world_settings,
        "TagParameters",
        "WorldDescription.WorldSettings.TagParameters",
        &resolved.path,
    )?;
    let combat = required_setting_string(settings, "combat_difficulty")?;
    let combat_value = tags
        .entry(String::from(COMBAT_DIFFICULTY_TAG))
        .or_insert_with(|| json!({}));
    let combat_object =
        combat_value
            .as_object_mut()
            .ok_or_else(|| WindroseWorldTargetError::InvalidShape {
                path: resolved.path.clone(),
                field: String::from(
                    "WorldDescription.WorldSettings.TagParameters.CombatDifficulty",
                ),
            })?;
    combat_object.insert(
        String::from("TagName"),
        Value::String(format!("WDS.Parameter.CombatDifficulty.{combat}")),
    );
    if document == resolved.document {
        return Ok(resolved.original.clone());
    }
    serialize_json(&resolved.path, &document)
}

pub(super) fn parse_json(path: &Path, bytes: &[u8]) -> Result<Value, WindroseWorldTargetError> {
    serde_json::from_slice(bytes).map_err(|error| WindroseWorldTargetError::MalformedJson {
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}

pub(super) fn serialize_json(
    path: &Path,
    document: &Value,
) -> Result<Vec<u8>, WindroseWorldTargetError> {
    let mut bytes = serde_json::to_vec_pretty(document).map_err(|error| {
        WindroseWorldTargetError::MalformedJson {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub(super) fn required_string_at<'a>(
    document: &'a Value,
    path: &[&str],
    file: &Path,
) -> Result<&'a str, WindroseWorldTargetError> {
    let mut value = document;
    for segment in path {
        value = value
            .get(*segment)
            .ok_or_else(|| WindroseWorldTargetError::InvalidShape {
                path: file.to_path_buf(),
                field: path.join("."),
            })?;
    }
    value
        .as_str()
        .ok_or_else(|| WindroseWorldTargetError::InvalidShape {
            path: file.to_path_buf(),
            field: path.join("."),
        })
}

pub(super) fn require_identity(
    evidence: &'static str,
    expected: &str,
    actual: &str,
) -> Result<(), WindroseWorldTargetError> {
    if expected == actual {
        Ok(())
    } else {
        Err(WindroseWorldTargetError::IdentityMismatch {
            evidence,
            expected: String::from(expected),
            actual: String::from(actual),
        })
    }
}

pub(super) fn optional_setting_string(
    settings: &Map<String, Value>,
    key: &'static str,
) -> Result<String, WindroseWorldTargetError> {
    match settings.get(key) {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(value)) => Ok(value.clone()),
        _ => Err(WindroseWorldTargetError::InvalidSetting { key }),
    }
}

pub(super) fn merge_json(
    target: &mut Value,
    source: &Value,
    path: &Path,
) -> Result<(), WindroseWorldTargetError> {
    let target_object =
        target
            .as_object_mut()
            .ok_or_else(|| WindroseWorldTargetError::InvalidShape {
                path: path.to_path_buf(),
                field: String::from("root object"),
            })?;
    let source_object =
        source
            .as_object()
            .ok_or_else(|| WindroseWorldTargetError::InvalidShape {
                path: path.to_path_buf(),
                field: String::from("rendered root object"),
            })?;
    for (key, value) in source_object {
        if let (Some(existing), Value::Object(_)) = (target_object.get_mut(key), value)
            && existing.is_object()
        {
            merge_json(existing, value, path)?;
            continue;
        }
        target_object.insert(key.clone(), value.clone());
    }
    Ok(())
}

fn object_at_mut<'a>(
    document: &'a mut Value,
    path: &[&str],
    file: &Path,
) -> Result<&'a mut Map<String, Value>, WindroseWorldTargetError> {
    let mut value = document;
    for segment in path {
        value = value
            .get_mut(*segment)
            .ok_or_else(|| WindroseWorldTargetError::InvalidShape {
                path: file.to_path_buf(),
                field: path.join("."),
            })?;
    }
    value
        .as_object_mut()
        .ok_or_else(|| WindroseWorldTargetError::InvalidShape {
            path: file.to_path_buf(),
            field: path.join("."),
        })
}

fn child_object_at_or_insert_mut<'a>(
    parent: &'a mut Map<String, Value>,
    key: &str,
    field: &str,
    file: &Path,
) -> Result<&'a mut Map<String, Value>, WindroseWorldTargetError> {
    parent
        .entry(String::from(key))
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| WindroseWorldTargetError::InvalidShape {
            path: file.to_path_buf(),
            field: String::from(field),
        })
}

fn required_setting_string(
    settings: &Map<String, Value>,
    key: &'static str,
) -> Result<String, WindroseWorldTargetError> {
    settings
        .get(key)
        .and_then(Value::as_str)
        .map(String::from)
        .ok_or(WindroseWorldTargetError::InvalidSetting { key })
}

fn required_setting_bool(
    settings: &Map<String, Value>,
    key: &'static str,
) -> Result<bool, WindroseWorldTargetError> {
    settings
        .get(key)
        .and_then(Value::as_bool)
        .ok_or(WindroseWorldTargetError::InvalidSetting { key })
}

fn required_setting_number(
    settings: &Map<String, Value>,
    key: &'static str,
) -> Result<f64, WindroseWorldTargetError> {
    settings
        .get(key)
        .and_then(Value::as_f64)
        .ok_or(WindroseWorldTargetError::InvalidSetting { key })
}

pub(super) fn error_path(error: &WindroseWorldTargetError, fallback: &Path) -> PathBuf {
    match error {
        WindroseWorldTargetError::Io { path, .. }
        | WindroseWorldTargetError::MalformedJson { path, .. }
        | WindroseWorldTargetError::InvalidShape { path, .. }
        | WindroseWorldTargetError::OutsideInstallRoot { path, .. }
        | WindroseWorldTargetError::ConcurrentModification { path }
        | WindroseWorldTargetError::Replacement { path, .. }
        | WindroseWorldTargetError::RollbackFailed { path }
        | WindroseWorldTargetError::InvalidPendingPlan { path, .. }
        | WindroseWorldTargetError::MissingUpdater { path }
        | WindroseWorldTargetError::UpdaterLaunch { path, .. }
        | WindroseWorldTargetError::UpdaterTermination { path, .. } => path.clone(),
        _ => fallback.to_path_buf(),
    }
}
