use serde_json::{Map, Value};

use crate::StorageError;

const REVIEW_KEY: &str = "projectzomboid_b42_policy_reviewed";
const RETIRED_OPTIONS: &str =
    include_str!("../../../modules/projectzomboid/retired-server-options.json");

fn retired_options() -> Result<Map<String, Value>, StorageError> {
    Ok(serde_json::from_str(RETIRED_OPTIONS)?)
}

pub(crate) fn preserve_retired_settings(
    persisted: &Map<String, Value>,
    incoming: &mut Map<String, Value>,
) -> Result<(), StorageError> {
    // These values are historical user data, not live native options. Partial
    // Mods/maintenance updates must neither erase them nor acknowledge a review.
    for key in retired_options()?
        .keys()
        .map(String::as_str)
        .chain([REVIEW_KEY])
    {
        if let Some(value) = persisted.get(key) {
            incoming
                .entry(key.to_owned())
                .or_insert_with(|| value.clone());
        }
    }
    Ok(())
}

pub(crate) fn validate_before_start(settings: &Map<String, Value>) -> Result<(), StorageError> {
    if settings.get(REVIEW_KEY).and_then(Value::as_bool) == Some(true) {
        return Ok(());
    }
    for (key, retired) in retired_options()? {
        let Some(value) = settings.get(&key) else {
            continue;
        };
        let default = &retired["default"];
        let matches_default = match (value.as_f64(), default.as_f64()) {
            (Some(value), Some(default)) => value == default,
            _ => value == default,
        };
        if !matches_default {
            return Err(StorageError::InvalidModuleSetting {
                module_id: String::from("projectzomboid"),
                field: String::from(REVIEW_KEY),
                message: String::from(
                    "Project Zomboid Build 42 no longer reads some customized settings. Review the current Configuration and native Sandbox Lua, then confirm the Build 42 review in Anti-cheat and save before starting. Previous values remain preserved.",
                ),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn new_and_unchanged_old_instances_do_not_require_review() {
        validate_before_start(&Map::new()).unwrap();
        let defaults = retired_options()
            .unwrap()
            .into_iter()
            .map(|(key, value)| (key, value["default"].clone()))
            .collect();
        validate_before_start(&defaults).unwrap();
    }

    #[test]
    fn changed_old_policies_require_explicit_review_without_exposing_values() {
        let mut settings = json!({"anti_cheat_protection_type_1": false,
            "discord_channel": "private-channel", "anti_cheat_speed": 2})
        .as_object()
        .unwrap()
        .clone();
        let error = validate_before_start(&settings).unwrap_err().to_string();
        assert!(error.contains("Build 42"));
        assert!(!error.contains("private-channel"));
        settings.insert(REVIEW_KEY.into(), json!(false));
        assert!(validate_before_start(&settings).is_err());
        settings.insert(REVIEW_KEY.into(), json!(true));
        validate_before_start(&settings).unwrap();
    }

    #[test]
    fn partial_updates_preserve_old_values_without_confirming_review() {
        let persisted = json!({"anti_cheat_protection_type_1": false,
            "discord_channel_id": "123456", "mods": "A"})
        .as_object()
        .unwrap()
        .clone();
        let mut incoming = json!({"mods": "B"}).as_object().unwrap().clone();
        preserve_retired_settings(&persisted, &mut incoming).unwrap();
        assert_eq!(incoming["anti_cheat_protection_type_1"], false);
        assert_eq!(incoming["discord_channel_id"], "123456");
        assert_eq!(incoming["mods"], "B");
        assert!(!incoming.contains_key(REVIEW_KEY));
        assert!(validate_before_start(&incoming).is_err());
    }

    #[test]
    fn explicit_unchecking_is_preserved_by_later_partial_updates() {
        let persisted = json!({"anti_cheat_type_2_threshold_multiplier": 9, "projectzomboid_b42_policy_reviewed": true}).as_object().unwrap().clone();
        let mut incoming = json!({"projectzomboid_b42_policy_reviewed": false})
            .as_object()
            .unwrap()
            .clone();
        preserve_retired_settings(&persisted, &mut incoming).unwrap();
        assert_eq!(incoming[REVIEW_KEY], false);
        assert!(validate_before_start(&incoming).is_err());
    }
}
