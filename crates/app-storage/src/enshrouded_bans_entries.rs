use std::path::Path;

use serde_json::{Map, Value};

use super::{NATIVE_KEY, RETIRED_KEY, invalid, roster};
use crate::StorageError;

// Native 1.0.0.1 parsing and reserialization independently confirm an object
// with an exact uint64 accountId. Unknown fields are retained, never reconstructed from
// the editor's ID-only projection. Other shapes fail without modifying files.
pub(super) fn parse(
    document: &Map<String, Value>,
    path: &Path,
) -> Result<roster::Records, StorageError> {
    let mut records = roster::Records::new();
    for key in [NATIVE_KEY, RETIRED_KEY] {
        let Some(value) = document.get(key) else {
            continue;
        };
        let entries = value
            .as_array()
            .ok_or_else(|| invalid(path, format!("Native {key} must contain an array")))?;
        if entries.len() > 4096 {
            return Err(invalid(path, format!("Native {key} exceeds 4096 entries")));
        }
        for (index, entry) in entries.iter().enumerate() {
            let object = entry.as_object().ok_or_else(|| {
                invalid(
                    path,
                    format!("Native {key} entry {index} must be an account record"),
                )
            })?;
            let id = object
                .get("accountId")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    invalid(
                        path,
                        format!("Native {key} entry {index} requires an exact uint64 accountId"),
                    )
                })?;
            for field in ["displayName", "characterName"] {
                if object.get(field).is_some_and(|value| !value.is_string()) {
                    return Err(invalid(
                        path,
                        format!("Native {key} entry {index} has a non-string {field}"),
                    ));
                }
            }
            if let Some(date) = object.get("banDate") {
                let date = date.as_object().ok_or_else(|| {
                    invalid(
                        path,
                        format!("Native {key} entry {index} requires an object banDate"),
                    )
                })?;
                if date
                    .get("value")
                    .is_some_and(|value| value.as_i64().is_none())
                {
                    return Err(invalid(
                        path,
                        format!(
                            "Native {key} entry {index} requires a signed integer banDate.value"
                        ),
                    ));
                }
            }
            if let Some(current) = records.get_mut(&id) {
                roster::merge_record(current, entry.clone())
                    .map_err(|message| invalid(path, message))?;
            } else {
                records.insert(id, entry.clone());
            }
        }
    }
    Ok(records)
}
