use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use super::{ManagedConfigMergePlan, ManagedConfigMutation, StorageError};
use crate::managed_console_log::owned_fs::reject_links;

const DIRECTORY: &str = "lgsm_world_map_observer";
const MARKER: &[u8] = b"-- LGSM managed: world map observer";
const BLOCK_START: &[u8] = b"-- LGSM managed: world map observer begin";
const BLOCK_END: &[u8] = b"-- LGSM managed: world map observer end";
const MAX_FILE_BYTES: usize = 1024 * 1024;
const RESOURCES: [(&str, &[u8]); 3] = [
    (
        "modinfo.lua",
        include_bytes!("../../../../modules/dontstarve/world_map_observer/modinfo.lua"),
    ),
    (
        "modmain.lua",
        include_bytes!("../../../../modules/dontstarve/world_map_observer/modmain.lua"),
    ),
    (
        "render_observer.lua",
        include_bytes!("../../../../modules/dontstarve/world_map_shared/render_observer.lua"),
    ),
];

/// Recognize our complete internal helper without hiding a user's same-named
/// directory. This shares the materializer's exact file and ownership contract.
pub fn is_managed_dst_map_observer(directory: &Path) -> Result<bool, StorageError> {
    if directory.file_name() != Some(std::ffi::OsStr::new(DIRECTORY)) {
        return Ok(false);
    }
    reject_links(directory).map_err(|error| failure(directory, error))?;
    let metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(failure(directory, error)),
    };
    if !metadata.is_dir() {
        return Ok(false);
    }
    let mut count = 0;
    for entry in fs::read_dir(directory).map_err(|error| failure(directory, error))? {
        let entry = entry.map_err(|error| failure(directory, error))?;
        if !RESOURCES.iter().any(|(name, _)| entry.file_name() == *name) {
            return Ok(false);
        }
        let path = entry.path();
        let Some(bytes) = read_optional_bounded(&path)? else {
            return Ok(false);
        };
        if !managed_first_line(&bytes) {
            return Ok(false);
        }
        count += 1;
    }
    Ok(count == RESOURCES.len())
}

/// The caller has verified the instance's independently owned runtime and that
/// it is stopped. Every transformed file retains its original bytes for CAS and
/// the surrounding instance-configuration transaction's rollback.
pub(super) fn materialize(
    runtime: &Path,
    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let plans = prepare(runtime)?;
    validate_directory(&runtime.join("mods").join(DIRECTORY))?;
    for plan in plans {
        reject_links(&plan.destination_path)
            .map_err(|error| failure(&plan.destination_path, error))?;
        files.apply(vec![plan])?;
    }
    Ok(())
}

fn prepare(runtime: &Path) -> Result<Vec<ManagedConfigMergePlan>, StorageError> {
    reject_links(runtime).map_err(|error| failure(runtime, error))?;
    let mods = runtime.join("mods");
    let directory = mods.join(DIRECTORY);
    validate_directory(&directory)?;
    let mut plans = Vec::with_capacity(RESOURCES.len() + 1);
    for (name, replacement) in RESOURCES {
        let path = directory.join(name);
        if !managed_first_line(replacement) {
            return Err(failure(
                &path,
                "bundled observer resource is missing its ownership marker",
            ));
        }
        let original = read_optional_bounded(&path)?;
        if original
            .as_deref()
            .is_some_and(|bytes| !managed_first_line(bytes))
        {
            return Err(failure(
                &path,
                "refusing to overwrite an unmanaged observer file",
            ));
        }
        plans.push(ManagedConfigMergePlan {
            destination_path: path,
            replacement: replacement.to_vec(),
            original,
        });
    }
    let settings = mods.join("modsettings.lua");
    let original = read_optional_bounded(&settings)?;
    let replacement = merge_settings(&settings, original.as_deref().unwrap_or_default())?;
    plans.push(ManagedConfigMergePlan {
        destination_path: settings,
        replacement,
        original,
    });
    Ok(plans)
}

fn validate_directory(directory: &Path) -> Result<(), StorageError> {
    reject_links(directory).map_err(|error| failure(directory, error))?;
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(failure(directory, error)),
    };
    // Empty directories are safe to adopt, including those left after the
    // existing file transaction rolls back a first installation. A nonempty
    // directory must contain only our explicitly marked, expected files.
    for entry in entries {
        let entry = entry.map_err(|error| failure(directory, error))?;
        let path = entry.path();
        if !RESOURCES.iter().any(|(name, _)| entry.file_name() == *name) {
            return Err(failure(
                &path,
                "observer directory contains an unowned entry",
            ));
        }
        let bytes = read_optional_bounded(&path)?
            .ok_or_else(|| failure(&path, "observer file disappeared during validation"))?;
        if !managed_first_line(&bytes) {
            return Err(failure(
                &path,
                "refusing to adopt an unmarked observer directory",
            ));
        }
    }
    Ok(())
}

fn managed_first_line(bytes: &[u8]) -> bool {
    bytes
        .split(|byte| *byte == b'\n')
        .next()
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
        == Some(MARKER)
}

fn read_optional_bounded(path: &Path) -> Result<Option<Vec<u8>>, StorageError> {
    reject_links(path).map_err(|error| failure(path, error))?;
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(failure(path, error)),
    };
    let metadata = file.metadata().map_err(|error| failure(path, error))?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES as u64 {
        return Err(failure(
            path,
            "observer configuration is not a bounded regular file",
        ));
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| failure(path, error))?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(failure(
            path,
            "observer configuration grew beyond its read limit",
        ));
    }
    Ok(Some(bytes))
}

fn enable_block(newline: &[u8]) -> Vec<u8> {
    let mut block = Vec::new();
    for line in [
        BLOCK_START,
        b"ForceEnableMod('lgsm_world_map_observer')",
        BLOCK_END,
    ] {
        block.extend_from_slice(line);
        block.extend_from_slice(newline);
    }
    block
}

fn merge_settings(path: &Path, original: &[u8]) -> Result<Vec<u8>, StorageError> {
    let (bom, mut body) = if let Some(body) = original.strip_prefix(b"\xef\xbb\xbf") {
        (b"\xef\xbb\xbf".as_slice(), body)
    } else {
        (b"".as_slice(), original)
    };
    for newline in [b"\r\n".as_slice(), b"\n".as_slice()] {
        if let Some(rest) = body.strip_prefix(enable_block(newline).as_slice()) {
            body = rest;
            break;
        }
    }
    if [BLOCK_START, BLOCK_END]
        .iter()
        .any(|marker| body.windows(marker.len()).any(|bytes| bytes == *marker))
    {
        return Err(failure(
            path,
            "observer enable block is duplicated, misplaced, or edited; existing settings were preserved",
        ));
    }
    let newline = match body.iter().position(|byte| *byte == b'\n') {
        Some(index) if index > 0 && body[index - 1] == b'\r' => b"\r\n".as_slice(),
        _ => b"\n".as_slice(),
    };
    let mut merged = bom.to_vec();
    merged.extend_from_slice(&enable_block(newline));
    merged.extend_from_slice(body);
    Ok(merged)
}

fn failure(path: &Path, message: impl std::fmt::Display) -> StorageError {
    StorageError::ModuleSupportMaterialization {
        module_id: "dontstarve".to_owned(),
        path: path.to_owned(),
        message: message.to_string(),
    }
}

#[cfg(test)]
#[path = "dst_map_observer_tests.rs"]
mod tests;
