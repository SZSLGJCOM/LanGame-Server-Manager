use std::fs;
use std::io::Read;
use std::path::Path;

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use super::{ManagedConfigMutation, ModuleSupportMaterializationContext, StorageError};

#[path = "templates_rimworld_native.rs"]
pub(super) mod native;

pub(crate) fn preserve_retired_settings(
    persisted: &Map<String, Value>,
    incoming: &mut Map<String, Value>,
) {
    // These are retained historical data, never current native whitelist controls.
    for key in ["use_whitelist", "whitelisted_users"] {
        if let Some(value) = persisted.get(key) {
            incoming.insert(key.to_owned(), value.clone());
        }
    }
    if let Some(value) = persisted.get("server_password") {
        incoming
            .entry("server_password".to_owned())
            .or_insert_with(|| value.clone());
    }
}

pub(super) fn render_rimworld_password_json(settings: &Map<String, Value>) -> String {
    let password = settings
        .get("server_password")
        .and_then(Value::as_str)
        .unwrap_or_default();
    // Official RTShared Hasher uses ASCII, SHA256 and uppercase hex without dashes.
    // The schema accepts printable ASCII only, avoiding .NET's lossy replacement.
    let hash = if password.is_empty() {
        String::new()
    } else {
        Sha256::digest(password.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<String>()
    };
    format!("\"{hash}\"")
}

fn failure(path: &Path, message: impl ToString) -> StorageError {
    StorageError::ModuleSupportMaterialization {
        module_id: String::from("rimworld"),
        path: path.to_owned(),
        message: message.to_string(),
    }
}

fn read_optional_json(path: &Path) -> Result<Option<(Value, Vec<u8>)>, StorageError> {
    for candidate in [path.parent().unwrap_or(path), path] {
        match fs::symlink_metadata(candidate) {
            Ok(metadata)
                if metadata.file_type().is_symlink()
                    || crate::private_runtime::is_reparse_point(candidate)? =>
            {
                return Err(failure(
                    candidate,
                    "RimWorld configuration must remain in regular instance-owned files",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(failure(candidate, error)),
        }
    }
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(failure(path, error)),
    };
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| failure(path, error))?;
    if bytes.len() > 1024 * 1024 {
        return Err(failure(path, "RimWorld configuration exceeds 1 MiB"));
    }
    let json_bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
    let value: Value = serde_json::from_slice(json_bytes)
        .map_err(|_| failure(path, "RimWorld configuration is not valid JSON"))?;
    if !value.is_object() {
        return Err(failure(
            path,
            "RimWorld configuration must be a JSON object",
        ));
    }
    Ok(Some((value, bytes)))
}

pub(super) fn materialize_configuration(
    context: &ModuleSupportMaterializationContext<'_>,
    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let path = context.install_root.join("Configs/PasswordConfig.json");
    let existing = read_optional_json(&path)?;
    // An omitted field keeps a password set by the native server. Explicit empty
    // input clears it; a retired enabled whitelist still blocks startup below.
    if !context.settings.contains_key("server_password") && existing.is_some() {
        return native::materialize(context, files);
    }
    let (mut document, original) = existing
        .map(|(value, bytes)| (value, Some(bytes)))
        .unwrap_or_else(|| (Value::Object(Map::new()), None));
    document["Password"] = serde_json::from_str(&render_rimworld_password_json(context.settings))?;
    files.apply(vec![super::templates_materialize::ManagedConfigMergePlan {
        destination_path: path,
        original,
        replacement: serde_json::to_vec_pretty(&document)?,
    }])?;
    native::materialize(context, files)
}

pub(super) fn validate_before_start(
    context: &ModuleSupportMaterializationContext<'_>,
) -> Result<(), StorageError> {
    let mut was_restricted = context
        .settings
        .get("use_whitelist")
        .and_then(Value::as_bool)
        == Some(true);
    // Old native files are retained, and can carry the only surviving restriction.
    for path in [
        context.config_dir.join("WhitelistConfig.json"),
        context.install_root.join("Configs/WhitelistConfig.json"),
    ] {
        if let Some((value, _)) = read_optional_json(&path)? {
            was_restricted |= value.get("UseWhitelist").and_then(Value::as_bool) == Some(true);
        }
    }
    if !was_restricted {
        return Ok(());
    }
    let password = read_optional_json(&context.install_root.join("Configs/PasswordConfig.json"))?;
    let has_password = password
        .as_ref()
        .and_then(|(value, _)| value.get("Password"))
        .and_then(Value::as_str)
        .is_some_and(|value| {
            value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        });
    if has_password {
        return Ok(());
    }
    Err(StorageError::InvalidModuleSetting {
        module_id: String::from("rimworld"),
        field: String::from("server_password"),
        message: String::from(
            "RimWorld Together 26.8.31.1 no longer supports whitelists. This instance previously required one. Set and save a server password in Room Settings before starting. Original whitelist files and settings remain preserved.",
        ),
    })
}
