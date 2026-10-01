//! Instance-local collection provenance. These records never enable or remove Mods.
use std::collections::HashSet;

use app_modules::ModuleDescriptor;
use serde_json::{Map, Value};

use crate::StorageError;

const FIELD: &str = "steam_workshop_collections";
const MAX_COLLECTIONS: usize = 128;
const MAX_COLLECTION_MEMBERS: usize = 8192;
const MAX_TOTAL_MEMBERS: usize = 65_536;

pub(super) fn validate(
    descriptor: Option<&ModuleDescriptor>,
    settings: &Map<String, Value>,
) -> Result<(), StorageError> {
    for (field, modules) in [
        ("dst_removed_workshop_mod_ids", &["dontstarve"][..]),
        (
            "steam_workshop_disabled_mod_ids",
            &["barotrauma", "conanexiles", "soulmask"][..],
        ),
        ("steam_workshop_removed_mod_ids", &["palworld"][..]),
    ] {
        validate_member_markers(descriptor, settings, field, modules)?;
    }
    let Some(value) = settings.get(FIELD) else {
        return Ok(());
    };
    let invalid = |field: String, message: &str| StorageError::InvalidModuleSetting {
        module_id: descriptor
            .map_or("runtime", |descriptor| descriptor.summary.id.as_str())
            .into(),
        field,
        message: message.into(),
    };
    if !descriptor
        .and_then(|descriptor| descriptor.workshop.as_ref())
        .is_some_and(|workshop| {
            workshop.provider.eq_ignore_ascii_case("steam")
                && workshop.consumer_app_id.is_some_and(|app_id| app_id > 0)
        })
    {
        return Err(invalid(
            FIELD.into(),
            "collection records require a Steam Workshop module",
        ));
    }
    let records = value
        .as_array()
        .ok_or_else(|| invalid(FIELD.into(), "must be an array"))?;
    if records.len() > MAX_COLLECTIONS {
        return Err(invalid(
            FIELD.into(),
            "must contain at most 128 collections",
        ));
    }
    let mut ids = HashSet::new();
    let mut total_members = 0usize;
    for (index, value) in records.iter().enumerate() {
        let path = format!("{FIELD}[{index}]");
        let record = value
            .as_object()
            .ok_or_else(|| invalid(path.clone(), "must be an object"))?;
        if record
            .keys()
            .any(|key| !matches!(key.as_str(), "id" | "title" | "member_ids"))
        {
            return Err(invalid(path, "only id, title and member_ids are supported"));
        }
        let id = record
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| valid_id(id))
            .ok_or_else(|| {
                invalid(
                    format!("{path}.id"),
                    "must be a canonical 6–20 digit nonzero Workshop u64 ID",
                )
            })?;
        if !ids.insert(id) {
            return Err(invalid(
                format!("{path}.id"),
                "collection IDs must be unique",
            ));
        }
        if !record
            .get("title")
            .and_then(Value::as_str)
            .is_some_and(|title| title.chars().count() <= 512)
        {
            return Err(invalid(
                format!("{path}.title"),
                "must be a string of at most 512 characters",
            ));
        }
        let members = record
            .get("member_ids")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid(format!("{path}.member_ids"), "must be an array"))?;
        if members.len() > MAX_COLLECTION_MEMBERS {
            return Err(invalid(
                format!("{path}.member_ids"),
                "must contain at most 8192 members",
            ));
        }
        total_members += members.len();
        if total_members > MAX_TOTAL_MEMBERS {
            return Err(invalid(
                FIELD.into(),
                "must contain at most 65536 members in total",
            ));
        }
        let mut member_ids = HashSet::new();
        for member in members {
            let id = member.as_str().filter(|id| valid_id(id)).ok_or_else(|| {
                invalid(
                    format!("{path}.member_ids"),
                    "members must be canonical 6–20 digit nonzero Workshop u64 IDs",
                )
            })?;
            if !member_ids.insert(id) {
                return Err(invalid(
                    format!("{path}.member_ids"),
                    "member IDs must be unique within each collection",
                ));
            }
        }
    }
    Ok(())
}

fn valid_id(id: &str) -> bool {
    (6..=20).contains(&id.len())
        && id.bytes().all(|byte| byte.is_ascii_digit())
        && !id.starts_with('0')
        && id.parse::<u64>().is_ok()
}

// Retained options are not membership after an explicit instance removal.
// Keeping this separate from the native options preserves re-addition defaults.
fn validate_member_markers(
    descriptor: Option<&ModuleDescriptor>,
    settings: &Map<String, Value>,
    field: &str,
    allowed_modules: &[&str],
) -> Result<(), StorageError> {
    let Some(value) = settings.get(field) else {
        return Ok(());
    };
    let module_id = descriptor.map_or("runtime", |descriptor| descriptor.summary.id.as_str());
    let invalid = || StorageError::InvalidModuleSetting {
        module_id: module_id.into(),
        field: field.into(),
        message: format!(
            "Mod membership markers require module {} and at most 8192 unique canonical Workshop IDs",
            allowed_modules.join(", ")
        ),
    };
    let ids = value
        .as_array()
        .filter(|ids| allowed_modules.contains(&module_id) && ids.len() <= MAX_COLLECTION_MEMBERS)
        .ok_or_else(invalid)?;
    let mut seen = HashSet::new();
    for value in ids {
        let id = value
            .as_str()
            .filter(|id| valid_id(id))
            .ok_or_else(invalid)?;
        if !seen.insert(id) {
            return Err(invalid());
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "settings_validation_workshop_collections_tests.rs"]
mod tests;
