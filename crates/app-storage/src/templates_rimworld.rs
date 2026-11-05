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

pub(super) fn render_server_image_url_json(settings: &Map<String, Value>) -> String {
    Value::String(
        settings
            .get("server_image_url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    )
    .to_string()
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

/// The new server reads per-listing files with owner hashes and item def names.
/// The old ledger contains usernames and display labels, which cannot safely
/// identify the owner and item of a migrated listing.
fn old_market_has_listings(root: &Path) -> Result<bool, StorageError> {
    let path = root.join("Assets/Market.json");
    let Some((document, _)) = read_optional_json(&path)? else {
        return Ok(false);
    };
    match document.get("Entries").and_then(Value::as_array) {
        Some(entries) if entries.iter().all(Value::is_object) => Ok(!entries.is_empty()),
        Some(_) => Err(failure(
            &path,
            "The old RimWorld market ledger contains a malformed listing. Verify the original ledger before starting or updating.",
        )),
        None => Err(failure(
            &path,
            "The old RimWorld market ledger must contain an Entries array. Verify the original ledger before updating.",
        )),
    }
}

fn active_market_error(root: &Path) -> StorageError {
    failure(
        &root.join("Assets/Market.json"),
        "The old RimWorld local market has active listings. Use a verified original Together server (26.8.31.1, 26.10.15.1 or 26.10.20.1 (1)) with the matching client to let sellers withdraw their items, then retry. Restore an unmodified copy of the original package and ledger if the installed program cannot be verified.",
    )
}

pub(crate) fn validate_market_archive_update(root: &Path) -> Result<(), StorageError> {
    if old_market_has_listings(root)? {
        return Err(active_market_error(root));
    }
    Ok(())
}

// Each official Windows archive below contains only RTServer.exe. Its actual
// bytes identify a server that can read and withdraw the old local listings.
pub(super) fn old_market_archive(image: &str) -> Option<&'static str> {
    match image {
        "939fe9a83434d31c543b3c486ed3398c6498ae9a9a609f8296e73c84cd5b7cc6" => {
            Some("f16c703f1e3e4e6de0f4877d8ae502a817c85482681956aa01e8a4b619bab68c")
        }
        "ef8e9acaf32ce4af5863295300254c25e8dd5388c39f59ce2b4ffd80eff04ac6" => {
            Some("fdebc2ccd6789b4bd33fbd6941d2ea8b41f8a0d063c30db09abcfb851fec18a1")
        }
        "871abd0df922baddf26437cec48676c01bbc4ca7cac2c077a6c773662a25c7bc" => {
            Some("ebffcd587bac87405d8d5f2e671d8fce0d5db3a0872a3d14699395ece512790d")
        }
        _ => None,
    }
}

fn can_withdraw_old_market(root: &Path) -> Result<bool, StorageError> {
    let path = root.join("RTServer.exe");
    let mut image = match crate::instance_archive_files::native::open_verified_file(&path, false) {
        Ok(image) => image,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(failure(&path, error)),
    };
    let bytes = image
        .reader()
        .metadata()
        .map_err(|e| failure(&path, e))?
        .len();
    if !matches!(bytes, 69_131_303 | 69_142_747 | 75_269_969) {
        return Ok(false);
    }
    let digest = crate::private_runtime_refresh::hash_open_file(image.reader(), &path, None)?;
    let Some(archive) = old_market_archive(&digest) else {
        return Ok(false);
    };
    let inventory = crate::program_seed::read_clean_package_copy_inventory(root, None)?;
    Ok(inventory.is_none_or(|inventory| {
        inventory.module_id == "rimworld"
            && inventory
                .archive_sha256
                .as_deref()
                .is_none_or(|recorded| recorded == archive)
            && inventory
                .package
                .files
                .iter()
                .any(|(name, hash)| name.eq_ignore_ascii_case("RTServer.exe") && *hash == digest)
    }))
}

pub(super) fn validate_release_before_start(
    root: &Path,
    descriptor: Option<&app_modules::ModuleDescriptor>,
    requested_feature: Option<&str>,
) -> Result<(), StorageError> {
    if old_market_has_listings(root)? && !can_withdraw_old_market(root)? {
        return Err(active_market_error(root));
    }
    let Some(feature) = requested_feature else {
        return Ok(());
    };
    let current = match descriptor {
        Some(descriptor) => crate::archive_program_is_current(root, descriptor, None)?,
        None => false,
    };
    if !current {
        return Err(failure(
            root,
            format!(
                "{feature} requires the verified Together 26.11.2.1 server package. Restore the RimWorld module and update the instance program, or turn off this option for the installed version."
            ),
        ));
    }
    Ok(())
}

pub(super) fn materialize_server_config(
    context: &ModuleSupportMaterializationContext<'_>,
    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let source = context.config_dir.join("ServerConfig.json");
    let (rendered, _) = read_optional_json(&source)?
        .ok_or_else(|| failure(&source, "Rendered RimWorld server configuration is missing"))?;
    let mut rendered = rendered
        .as_object()
        .cloned()
        .ok_or_else(|| failure(&source, "Rendered RimWorld configuration must be an object"))?;
    // The complete template uses an empty string when this optional override is
    // absent. Do not let that rendering fallback clear a native operator choice.
    if context
        .settings
        .get("server_image_url")
        .is_none_or(Value::is_null)
    {
        rendered.remove("ServerImageURL");
    }
    let destination = context.install_root.join("Configs/ServerConfig.json");
    let (document, original) = read_optional_json(&destination)?
        .map(|(value, bytes)| (value, Some(bytes)))
        .unwrap_or_else(|| (Value::Object(Map::new()), None));
    let mut document = document
        .as_object()
        .cloned()
        .ok_or_else(|| failure(&destination, "RimWorld configuration must be an object"))?;
    document.extend(rendered);
    document
        .entry("ServerImageURL".to_owned())
        .or_insert_with(|| Value::String(String::new()));
    let mut replacement = serde_json::to_vec_pretty(&document)?;
    replacement.push(b'\n');
    files.apply(vec![super::templates_materialize::ManagedConfigMergePlan {
        destination_path: destination,
        original,
        replacement,
    }])
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
