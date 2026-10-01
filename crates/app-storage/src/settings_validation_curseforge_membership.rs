//! Instance membership is retained independently of ASA's active/passive launch lists.
use std::collections::HashSet;

use app_modules::ModuleDescriptor;
use serde_json::{Map, Value};

use crate::StorageError;

const FIELDS: [&str; 3] = [
    "curseforge_disabled_mod_ids",
    "curseforge_disabled_passive_mod_ids",
    "curseforge_removed_mod_ids",
];
const MAX_IDS: usize = 8192;

pub(super) fn validate(
    descriptor: Option<&ModuleDescriptor>,
    settings: &Map<String, Value>,
) -> Result<(), StorageError> {
    let module_id = descriptor.map_or("runtime", |descriptor| descriptor.summary.id.as_str());
    for field in FIELDS {
        let Some(value) = settings.get(field) else {
            continue;
        };
        let invalid = || StorageError::InvalidModuleSetting {
            module_id: module_id.into(),
            field: field.into(),
            message: concat!(
                "CurseForge membership requires ARK Survival Ascended and at most 8192 ",
                "unique canonical nonzero u64 project IDs"
            )
            .into(),
        };
        let ids = value
            .as_array()
            .filter(|ids| module_id == "arksurvivalascended" && ids.len() <= MAX_IDS)
            .ok_or_else(invalid)?;
        let mut seen = HashSet::new();
        for value in ids {
            let id = value
                .as_str()
                .filter(|id| valid_project_id(id))
                .ok_or_else(invalid)?;
            if !seen.insert(id) {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

fn valid_project_id(id: &str) -> bool {
    (1..=20).contains(&id.len())
        && !id.starts_with('0')
        && id.bytes().all(|byte| byte.is_ascii_digit())
        && id.parse::<u64>().is_ok()
}

#[cfg(test)]
#[path = "settings_validation_curseforge_membership_tests.rs"]
mod tests;
