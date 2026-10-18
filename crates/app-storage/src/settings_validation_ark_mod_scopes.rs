use super::*;

pub(super) fn validate(module_id: &str, settings: &Map<String, Value>) -> Result<(), StorageError> {
    use app_core::ark_mod_scopes::{OVERRIDES_FIELD, effective_settings, validate_overrides};
    if !settings.contains_key(OVERRIDES_FIELD)
        && !settings.contains_key(app_core::ark_mod_scopes::SNAPSHOTS_FIELD)
    {
        return Ok(());
    }
    let value = Value::Object(settings.clone());
    let invalid = |message| StorageError::InvalidModuleSetting {
        module_id: module_id.into(),
        field: OVERRIDES_FIELD.into(),
        message,
    };
    validate_overrides(module_id, &value).map_err(invalid)?;
    let Some(overrides) = value.get(OVERRIDES_FIELD).and_then(Value::as_object) else {
        return Ok(());
    };
    if overrides.is_empty() {
        return Ok(());
    }
    for key in app_core::ark_mod_scopes::configured_process_keys(&value).map_err(invalid)? {
        let effective = effective_settings(module_id, &value, &key).map_err(invalid)?;
        let effective = effective
            .as_object()
            .ok_or(StorageError::InvalidSettingsRoot)?;
        curseforge_membership::validate_for_module(module_id, effective)
            .map_err(|error| invalid(format!("Map `{key}`: {error}")))?;
        if module_id == "arksurvivalascended" {
            crate::templates::validate_asa_mod_configurations(effective)
                .map_err(|error| invalid(format!("Map `{key}`: {error}")))?;
        }
    }
    Ok(())
}
