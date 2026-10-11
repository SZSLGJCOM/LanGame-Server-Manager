use app_core::{ModulePortGroupSpec, PortBinding};
use app_modules::ModuleDescriptor;
use serde_json::{Map, Value};

use crate::StorageError;

pub(crate) const SERVER_NAMES_VERSION: u64 = 1;

/// Legacy documents stored a common prefix and per-map suffixes. Decode them
/// into today's complete names without changing archived or live source bytes.
/// A normal managed write persists the current document version with the names.
pub(crate) fn decode_document_settings(document: &Value) -> Result<Value, StorageError> {
    let document = document
        .as_object()
        .ok_or(StorageError::InvalidSettingsRoot)?;
    let mut settings = match document.get("settings") {
        Some(Value::Object(settings)) => settings.clone(),
        None => Map::new(),
        _ => return Err(StorageError::InvalidSettingsRoot),
    };
    let module_id = match document.get("module_id") {
        Some(Value::String(module_id)) => module_id.as_str(),
        None => return Ok(Value::Object(settings)),
        _ => return Err(StorageError::InvalidSettingsRoot),
    };
    if !app_core::ark_maps::is_ark(module_id) {
        return Ok(Value::Object(settings));
    }
    let invalid_field = |field: &str, message: &str| StorageError::InvalidModuleSetting {
        module_id: module_id.into(),
        field: field.into(),
        message: message.into(),
    };
    let version = match document.get("ark_server_names_version") {
        None => 0,
        Some(value) => value.as_u64().ok_or_else(|| {
            invalid_field(
                "ark_server_names_version",
                "ARK server-name document version must be 0 or 1",
            )
        })?,
    };
    if version > SERVER_NAMES_VERSION {
        return Err(invalid_field(
            "ark_server_names_version",
            "Unsupported ARK server-name document version; configuration was preserved",
        ));
    }
    let maps = app_core::ark_maps::parse_additional_maps(&Value::Object(settings.clone()))
        .map_err(|error| invalid(module_id, error))?;
    if version == SERVER_NAMES_VERSION || maps.is_empty() {
        return Ok(Value::Object(settings));
    }
    // Freeze the old base before updating the primary name. Whitespace and an
    // empty base were legal native values and must retain their published form.
    let base = match settings.get("server_name") {
        Some(Value::String(name)) => name.clone(),
        None => document
            .get("instance_name")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                invalid_field(
                    "server_name",
                    "Legacy ARK names require a string server_name or instance_name",
                )
            })?
            .to_owned(),
        _ => {
            return Err(invalid_field(
                "server_name",
                "ARK server_name must be a string",
            ));
        }
    };
    let primary_map = match settings.get("map_name") {
        Some(Value::String(name)) => name.as_str(),
        None if module_id == "arksurvivalascended" => "TheIsland_WP",
        None => "TheIsland",
        _ => return Err(invalid_field("map_name", "ARK map_name must be a string")),
    };
    let primary_name = format!("{base} | {primary_map}");
    settings.insert("server_name".into(), Value::String(primary_name));
    let additional = settings
        .get_mut("additional_maps")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| invalid_field("additional_maps", "ARK additional_maps must be an array"))?;
    for (value, map) in additional.iter_mut().zip(maps) {
        value["name"] = Value::String(format!("{base} | {}", map.name));
    }
    Ok(Value::Object(settings))
}

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

#[cfg(test)]
#[path = "ark_maps_document_tests.rs"]
mod document_tests;
