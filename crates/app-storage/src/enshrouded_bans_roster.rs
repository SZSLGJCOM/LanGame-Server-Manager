use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use crate::StorageError;

pub(super) const SETTINGS_KEY: &str = "banned_player_ids";
pub(super) type Records = BTreeMap<u64, Value>;

pub(super) fn settings_ids(settings: &Map<String, Value>) -> Result<BTreeSet<u64>, StorageError> {
    crate::player_access_normalization::parse_enshrouded_banned_account_ids(settings)
        .map(|ids| ids.into_iter().collect())
        .map_err(|message| StorageError::InvalidModuleSetting {
            module_id: "enshrouded".to_owned(),
            field: SETTINGS_KEY.to_owned(),
            message,
        })
}

pub(super) fn set_ids(settings: &mut Map<String, Value>, ids: &BTreeSet<u64>) {
    settings.insert(
        SETTINGS_KEY.to_owned(),
        Value::String(
            ids.iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    );
}

/// Missing IDs in an ordinary settings save are never interpreted as unbans.
/// Only the player-access operation may supply a single explicit removal.
pub(super) fn reconcile_ids(
    instance_id: &str,
    native_ids: impl Iterator<Item = u64>,
    persisted: &Map<String, Value>,
    incoming: &mut Map<String, Value>,
    expected: Option<&str>,
    removed: Option<u64>,
) -> Result<(), StorageError> {
    let mut current = settings_ids(persisted)?;
    current.extend(native_ids);
    if let Some(expected) = expected {
        let expected: Value = serde_json::from_str(expected)?;
        let expected = expected
            .as_object()
            .ok_or(StorageError::InvalidSettingsRoot)?;
        if settings_ids(expected)? != current {
            return Err(StorageError::InstanceSettingsPreconditionFailed {
                id: instance_id.to_owned(),
            });
        }
    }
    current.extend(settings_ids(incoming)?);
    if let Some(removed) = removed {
        current.remove(&removed);
    }
    set_ids(incoming, &current);
    Ok(())
}

/// Retain all fields from both copies. Conflicting metadata cannot be resolved
/// without losing one value, so the caller must preserve the originals and fail.
pub(super) fn merge_record(existing: &mut Value, other: Value) -> Result<(), String> {
    let mut merged = existing.clone();
    merge_metadata(&mut merged, other, &[])?;
    *existing = merged;
    Ok(())
}

fn merge_metadata(existing: &mut Value, other: Value, path: &[&str]) -> Result<(), String> {
    if *existing == other {
        return Ok(());
    }
    // Older managed output supplied these exact placeholders. They must not
    // erase names or dates subsequently enriched by the native server.
    if placeholder(existing, path) {
        *existing = other;
        return Ok(());
    }
    if placeholder(&other, path) {
        return Ok(());
    }
    match (existing, other) {
        (Value::Object(existing), Value::Object(other)) => {
            for (key, value) in other {
                match existing.get_mut(&key) {
                    Some(current) => {
                        let mut nested = path.to_vec();
                        nested.push(&key);
                        merge_metadata(current, value, &nested)?;
                    }
                    None => {
                        existing.insert(key, value);
                    }
                }
            }
            Ok(())
        }
        _ => Err(
            "Conflicting native ban metadata; resolve the two preserved records before saving"
                .to_owned(),
        ),
    }
}

fn placeholder(value: &Value, path: &[&str]) -> bool {
    match path {
        ["displayName" | "characterName"] => value.as_str() == Some(""),
        ["banDate", "value"] => value.as_i64() == Some(0),
        _ => false,
    }
}

#[cfg(test)]
#[path = "enshrouded_bans_roster_tests.rs"]
mod tests;
