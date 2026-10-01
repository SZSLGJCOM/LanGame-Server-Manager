use crate::StorageError;
use serde_json::{Map, Value};

pub(super) fn validate(module_id: &str, settings: &Map<String, Value>) -> Result<(), StorageError> {
    let Some(performance) = settings.get("runtime_performance") else {
        return Ok(());
    };
    let invalid = |message: String| StorageError::InvalidModuleSetting {
        module_id: module_id.to_owned(),
        field: "runtime_performance.resource_limits".into(),
        message,
    };
    let performance = performance
        .as_object()
        .ok_or_else(|| invalid("runtime_performance must be an object".into()))?;
    let Some(value) = performance.get("resource_limits") else {
        return Ok(());
    };
    let limits: app_core::RuntimeResourceLimits =
        serde_json::from_value(value.clone()).map_err(|error| invalid(error.to_string()))?;
    limits.validate().map_err(invalid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resource_limits_validate_types_units_and_ranges_at_storage_boundary() {
        for limits in [
            json!(null),
            json!({"cpu_percent": 0}),
            json!({"cpu_percent": 101}),
            json!({"cpu_percent": "25"}),
            json!({"memory_limit_mib": 63}),
            json!({"memory_limit_mib": 1.5}),
            json!({"host_memory_reserve_mib": -1}),
            json!({"cpu_percentage": 50}),
        ] {
            let settings = Map::from_iter([(
                "runtime_performance".into(),
                json!({"resource_limits":limits}),
            )]);
            assert!(validate("test", &settings).is_err());
        }
        for limits in [
            json!({}),
            json!({"cpu_percent":null,"memory_limit_mib":null}),
            json!({"cpu_percent":25,"memory_limit_mib":4096,"host_memory_reserve_mib":2048}),
        ] {
            let settings = Map::from_iter([(
                "runtime_performance".into(),
                json!({"resource_limits":limits}),
            )]);
            validate("test", &settings).unwrap();
        }
    }
}
