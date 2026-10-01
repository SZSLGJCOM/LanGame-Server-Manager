use app_core::{ModulePortGroupSpec, PortBinding};
use app_modules::ModuleDescriptor;
use serde_json::{Map, Value};

use crate::StorageError;

fn invalid(module_id: &str, message: String) -> StorageError {
    StorageError::InvalidModuleSetting {
        module_id: module_id.into(),
        field: "additional_maps".into(),
        message,
    }
}

pub(crate) fn normalize_settings(
    module_id: &str,
    instance_id: &str,
    settings: &mut Map<String, Value>,
) -> Result<(), StorageError> {
    if !app_core::ark_maps::is_ark(module_id) {
        return Ok(());
    }
    // These values only exist in a trusted process projection, never persisted input.
    settings.remove("_managed_ark_save_directory");
    settings.remove("_managed_ark_native_log");
    let maps = app_core::ark_maps::parse_additional_maps(&Value::Object(settings.clone()))
        .map_err(|error| invalid(module_id, error))?;
    if !maps.is_empty()
        && settings
            .get("cluster_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .is_empty()
    {
        settings.insert(
            "cluster_id".into(),
            Value::String(format!("lgsm-{instance_id}")),
        );
    }
    Ok(())
}

pub(crate) fn validate_changes(
    module_id: &str,
    previous: &Map<String, Value>,
    incoming: &Map<String, Value>,
    running: bool,
) -> Result<(), StorageError> {
    if !app_core::ark_maps::is_ark(module_id) {
        return Ok(());
    }
    app_core::ark_maps::validate_map_changes(
        &Value::Object(previous.clone()),
        &Value::Object(incoming.clone()),
        running,
    )
    .map_err(|error| invalid(module_id, error))
}

pub(crate) fn requested_ports(
    descriptor: Option<&ModuleDescriptor>,
    settings: &Map<String, Value>,
    incoming: &[PortBinding],
    current: &[PortBinding],
) -> Result<Vec<PortBinding>, StorageError> {
    let Some(descriptor) = descriptor else {
        return Ok(incoming.to_vec());
    };
    app_core::ark_maps::requested_ports(
        &descriptor.summary.id,
        &Value::Object(settings.clone()),
        &descriptor.default_ports,
        incoming,
        current,
    )
    .map_err(|error| invalid(&descriptor.summary.id, error))
}

pub(crate) fn port_groups(
    descriptor: Option<&ModuleDescriptor>,
    settings: &Map<String, Value>,
) -> Result<Vec<ModulePortGroupSpec>, StorageError> {
    let Some(descriptor) = descriptor else {
        return Ok(Vec::new());
    };
    app_core::ark_maps::port_groups(
        &descriptor.summary.id,
        &Value::Object(settings.clone()),
        &descriptor.runtime.port_groups,
    )
    .map_err(|error| invalid(&descriptor.summary.id, error))
}
