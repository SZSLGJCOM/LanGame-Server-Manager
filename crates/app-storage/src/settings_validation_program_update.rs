use app_core::InstanceProgramUpdatePolicy;
use serde_json::{Map, Value};

use crate::StorageError;

pub(super) fn validate(module_id: &str, settings: &Map<String, Value>) -> Result<(), StorageError> {
    read_policy(module_id, settings).map(|_| ())
}

fn read_policy(
    module_id: &str,
    settings: &Map<String, Value>,
) -> Result<InstanceProgramUpdatePolicy, StorageError> {
    InstanceProgramUpdatePolicy::from_settings(settings).map_err(|message| {
        StorageError::InvalidModuleSetting {
            module_id: module_id.to_owned(),
            field: "program_update".to_owned(),
            message,
        }
    })
}

pub(crate) fn validate_program_update_policy_change(
    module_id: &str,
    persisted: &Map<String, Value>,
    incoming: &Map<String, Value>,
    active: bool,
) -> Result<(), StorageError> {
    let previous = read_policy(module_id, persisted)?;
    let next = read_policy(module_id, incoming)?;
    if active && previous != next {
        return Err(StorageError::InvalidModuleSetting {
            module_id: module_id.to_owned(),
            field: "program_update.policy".to_owned(),
            message: "请先停止实例并取消待启动操作，再更改程序更新策略。".to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_validation::{SettingsValidationPhase, validate_settings_against_schema};
    use serde_json::json;

    #[test]
    fn program_update_validation_does_not_depend_on_a_module_schema() {
        for phase in [
            SettingsValidationPhase::Creation,
            SettingsValidationPhase::Complete,
        ] {
            validate_settings_against_schema(None, &Map::new(), phase).unwrap();
            for policy in ["automatic", "pinned"] {
                let settings =
                    Map::from_iter([("program_update".into(), json!({"policy": policy}))]);
                validate_settings_against_schema(None, &settings, phase).unwrap();
            }
            for value in [
                json!(null),
                json!("pinned"),
                json!({"policy": false}),
                json!({"policy": "latest"}),
                json!({"polciy": "pinned"}),
            ] {
                let settings = Map::from_iter([("program_update".into(), value)]);
                let error = validate_settings_against_schema(None, &settings, phase).unwrap_err();
                assert!(
                    matches!(error, StorageError::InvalidModuleSetting { field, .. }
                    if field == "program_update")
                );
            }
        }
    }
}
