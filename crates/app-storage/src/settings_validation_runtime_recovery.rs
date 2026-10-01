use serde_json::{Map, Value};

use crate::StorageError;

pub(super) fn validate_runtime_recovery_settings(
    module_id: &str,
    settings: &Map<String, Value>,
) -> Result<(), StorageError> {
    let Some(value) = settings.get("runtime_restart") else {
        return Ok(());
    };
    let invalid = |field: &str, message: &str| StorageError::InvalidModuleSetting {
        module_id: module_id.to_owned(),
        field: field.to_owned(),
        message: message.to_owned(),
    };
    let policy = value
        .as_object()
        .ok_or_else(|| invalid("runtime_restart", "must be a JSON object"))?;
    for field in ["enabled", "only_nonzero_exit"] {
        if policy.get(field).is_some_and(|value| !value.is_boolean()) {
            return Err(invalid(
                &format!("runtime_restart.{field}"),
                "must be a boolean",
            ));
        }
    }
    for (field, minimum, maximum) in [("max_restarts", 1, 10), ("backoff_ms", 0, 300_000)] {
        if let Some(value) = policy.get(field)
            && !value
                .as_u64()
                .is_some_and(|number| (minimum..=maximum).contains(&number))
        {
            return Err(invalid(
                &format!("runtime_restart.{field}"),
                &format!("must be an integer from {minimum} to {maximum}"),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_validation::{SettingsValidationPhase, validate_settings_against_schema};
    use serde_json::json;

    fn check(value: Value) -> Result<(), StorageError> {
        validate_settings_against_schema(
            None,
            &Map::from_iter([("runtime_restart".to_owned(), value)]),
            SettingsValidationPhase::Complete,
        )
    }

    fn assert_invalid(value: Value, expected_field: &str) {
        let error = check(value).expect_err("invalid recovery settings must not be persisted");
        assert!(
            matches!(error, StorageError::InvalidModuleSetting { field, .. }
            if field == expected_field)
        );
    }

    #[test]
    fn runtime_recovery_rejects_nonobject_policy_and_nonboolean_switches() {
        for value in [
            json!(null),
            json!(false),
            json!("true"),
            json!([]),
            json!(3),
        ] {
            assert_invalid(value, "runtime_restart");
        }
        for field in ["enabled", "only_nonzero_exit"] {
            for value in [json!(null), json!("true"), json!(0), json!([]), json!({})] {
                assert_invalid(json!({field: value}), &format!("runtime_restart.{field}"));
            }
        }
    }

    #[test]
    fn runtime_recovery_rejects_fractional_string_and_out_of_range_limits() {
        for value in [
            json!(null),
            json!("3"),
            json!(true),
            json!(-1),
            json!(0),
            json!(11),
            json!(1.5),
        ] {
            assert_invalid(
                json!({"max_restarts": value}),
                "runtime_restart.max_restarts",
            );
        }
        for value in [
            json!(null),
            json!("5000"),
            json!(false),
            json!(-1),
            json!(300001),
            json!(0.5),
        ] {
            assert_invalid(json!({"backoff_ms": value}), "runtime_restart.backoff_ms");
        }
    }

    #[test]
    fn runtime_recovery_accepts_absent_partial_and_bounded_policies() {
        validate_settings_against_schema(None, &Map::new(), SettingsValidationPhase::Creation)
            .unwrap();
        check(json!({})).unwrap();
        check(json!({"enabled": false})).unwrap();
        for max_restarts in [1, 10] {
            for backoff_ms in [0, 300000] {
                check(json!({"enabled": true, "only_nonzero_exit": false,
                    "max_restarts": max_restarts, "backoff_ms": backoff_ms}))
                .unwrap();
            }
        }
    }

    #[test]
    fn runtime_recovery_validation_does_not_depend_on_a_module_schema() {
        let mut descriptor = app_modules::discover_modules(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules"),
        )
        .unwrap()
        .remove(0);
        descriptor.schema_json = None;
        let settings = Map::from_iter([("runtime_restart".to_owned(), json!({"max_restarts": 0}))]);
        for phase in [
            SettingsValidationPhase::Creation,
            SettingsValidationPhase::Complete,
        ] {
            let error = validate_settings_against_schema(Some(&descriptor), &settings, phase)
                .expect_err("recovery is a manager policy independent of game schemas");
            assert!(
                matches!(error, StorageError::InvalidModuleSetting { module_id, field, .. }
                if module_id == descriptor.summary.id && field == "runtime_restart.max_restarts")
            );
        }
    }
}
