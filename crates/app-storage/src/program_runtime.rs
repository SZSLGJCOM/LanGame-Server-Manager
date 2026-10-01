use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::StorageError;
use crate::atomic_file::write_file_atomically;
use crate::instance_isolation::paths::{contains, normalize_path, normalize_resource_path};
use crate::private_runtime::{PRIVATE_RUNTIME_MARKER, resolve_instance_private_runtime_root};

pub(crate) const SHARED_RUNTIME_BINDING: &str = ".langame-shared-program.json";
pub(crate) const EXCLUSIVE_RUNTIME_BINDING: &str = ".langame-exclusive-program.json";
const PROGRAM_IDENTITY: &str = ".langame-program-identity.json";

pub use app_core::InstanceProgramMode;

#[derive(Clone, Copy, Default)]
pub(crate) enum ProgramFileSelection<'a> {
    #[default]
    Automatic,
    Verified(&'a str),
    Local,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProgramIdentity {
    version: u32,
    id: String,
    module_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProgramUse {
    version: u32,
    program_id: String,
    module_id: String,
    instance_id: String,
}

pub(crate) fn previous_exclusive_instance(
    program_root: &Path,
    module_id: &str,
) -> Result<String, StorageError> {
    let identity: ProgramIdentity = read_record(&program_root.join(PROGRAM_IDENTITY))?;
    let usage: ProgramUse =
        read_record(&program_root.join(crate::program_exclusive::PROGRAM_USAGE))?;
    if identity.version != 1
        || usage.version != 1
        || identity.id.is_empty()
        || identity.module_id != module_id
        || usage.module_id != module_id
        || usage.program_id != identity.id
        || usage.instance_id.is_empty()
        || usage.instance_id.len() > 255
        || !usage
            .instance_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(invalid(
            program_root,
            "previous program usage does not match its identity",
        ));
    }
    Ok(usage.instance_id)
}

#[derive(Debug, Serialize, Deserialize)]
struct SharedProgramBinding {
    version: u32,
    program_root: PathBuf,
    program_id: String,
    module_id: String,
}

pub fn instance_program_mode(instance_root: &Path) -> Result<InstanceProgramMode, StorageError> {
    if instance_uses_exclusive_program(instance_root)? {
        return Ok(InstanceProgramMode::Independent);
    }
    match fs::symlink_metadata(instance_root.join("runtime").join(SHARED_RUNTIME_BINDING)) {
        Ok(_) => {
            resolve_shared_program(instance_root)?;
            Ok(InstanceProgramMode::Shared)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            resolve_instance_private_runtime_root(instance_root)?;
            Ok(InstanceProgramMode::Independent)
        }
        Err(source) => Err(StorageError::ReadPath {
            path: instance_root.join("runtime").join(SHARED_RUNTIME_BINDING),
            source,
        }),
    }
}

/// An instance owns its directory even when its executable comes from a shared
/// library. A damaged private runtime never falls back to that library.
pub fn resolve_instance_runtime_root(instance_root: &Path) -> Result<PathBuf, StorageError> {
    if instance_uses_exclusive_program(instance_root)? {
        return resolve_exclusive_program_reference(&instance_root.join("runtime"));
    }
    match instance_program_mode(instance_root)? {
        InstanceProgramMode::Shared => resolve_shared_program(instance_root),
        InstanceProgramMode::Independent => resolve_instance_private_runtime_root(instance_root),
    }
}

pub(crate) fn prepare_shared_program_reference(
    program_root: &Path,
    instance_root: &Path,
    module_id: &str,
) -> Result<PathBuf, StorageError> {
    prepare_program_reference(
        program_root,
        instance_root,
        module_id,
        SHARED_RUNTIME_BINDING,
    )
}

pub(crate) fn prepare_exclusive_program_reference(
    program_root: &Path,
    instance_root: &Path,
    module_id: &str,
) -> Result<PathBuf, StorageError> {
    prepare_program_reference(
        program_root,
        instance_root,
        module_id,
        EXCLUSIVE_RUNTIME_BINDING,
    )
}

pub(crate) fn record_exclusive_program_use(
    program_root: &Path,
    instance_root: &Path,
) -> Result<(), StorageError> {
    let binding: SharedProgramBinding = read_record(
        &instance_root
            .join("runtime")
            .join(EXCLUSIVE_RUNTIME_BINDING),
    )?;
    // Persist before configuration can write into the installation. A failed or
    // interrupted creation may leave data behind. Keep this history: reuse
    // requires absent instance/archive claims and a freshly checked clean tree.
    write_record(
        &program_root.join(crate::program_exclusive::PROGRAM_USAGE),
        &serde_json::json!({
            "version": 1,
            "program_id": binding.program_id,
            "module_id": binding.module_id,
            "instance_id": instance_root.file_name().and_then(|name| name.to_str()).unwrap_or_default(),
        }),
    )
}

pub fn instance_uses_exclusive_program(instance_root: &Path) -> Result<bool, StorageError> {
    let runtime = instance_root.join("runtime");
    if !binding_exists(&runtime.join(EXCLUSIVE_RUNTIME_BINDING))? {
        return Ok(false);
    }
    resolve_exclusive_program_reference(&runtime)?;
    Ok(true)
}

pub fn instance_uses_library_program(instance_root: &Path) -> Result<bool, StorageError> {
    if instance_uses_exclusive_program(instance_root)? {
        return Ok(true);
    }
    Ok(instance_program_mode(instance_root)? == InstanceProgramMode::Shared)
}

fn binding_exists(path: &Path) -> Result<bool, StorageError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(StorageError::ReadPath {
            path: path.to_owned(),
            source,
        }),
    }
}

fn prepare_program_reference(
    program_root: &Path,
    instance_root: &Path,
    module_id: &str,
    marker: &str,
) -> Result<PathBuf, StorageError> {
    let canonical = normalize_path(program_root)?;
    if !canonical.is_dir() {
        return Err(invalid(program_root, "shared program directory is missing"));
    }
    let instance = normalize_path(instance_root)?;
    if contains(&canonical, &instance) || contains(&instance, &canonical) {
        return Err(invalid(
            program_root,
            "program and instance directories overlap",
        ));
    }
    let identity_path = program_root.join(PROGRAM_IDENTITY);
    let identity = match fs::symlink_metadata(&identity_path) {
        Ok(_) => read_record::<ProgramIdentity>(&identity_path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let identity = ProgramIdentity {
                version: 1,
                id: uuid::Uuid::new_v4().to_string(),
                module_id: module_id.to_owned(),
            };
            write_record(&identity_path, &identity)?;
            identity
        }
        Err(source) => {
            return Err(StorageError::ReadPath {
                path: identity_path,
                source,
            });
        }
    };
    if identity.version != 1 || identity.module_id != module_id || identity.id.is_empty() {
        return Err(invalid(
            program_root,
            "shared program identity does not match the module",
        ));
    }
    let runtime = instance_root.join("runtime");
    fs::create_dir(&runtime).map_err(|source| StorageError::CreatePath {
        path: runtime.clone(),
        source,
    })?;
    write_record(
        &runtime.join(marker),
        &SharedProgramBinding {
            version: 1,
            program_root: canonical.clone(),
            program_id: identity.id,
            module_id: module_id.to_owned(),
        },
    )?;
    Ok(canonical)
}

fn resolve_shared_program(instance_root: &Path) -> Result<PathBuf, StorageError> {
    let runtime = instance_root.join("runtime");
    resolve_shared_program_reference(&runtime)
}

pub(crate) fn resolve_shared_program_reference(runtime: &Path) -> Result<PathBuf, StorageError> {
    resolve_program_reference(runtime, SHARED_RUNTIME_BINDING)
}

pub(crate) fn resolve_exclusive_program_reference(runtime: &Path) -> Result<PathBuf, StorageError> {
    resolve_program_reference(runtime, EXCLUSIVE_RUNTIME_BINDING)
}

fn resolve_program_reference(runtime: &Path, marker: &str) -> Result<PathBuf, StorageError> {
    let instance_root = runtime
        .parent()
        .ok_or_else(|| invalid(runtime, "shared runtime has no instance root"))?;
    normalize_path(runtime)?;
    match fs::symlink_metadata(runtime.join(PRIVATE_RUNTIME_MARKER)) {
        Ok(_) => {
            return Err(invalid(
                runtime,
                "conflicting shared and independent runtime ownership",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(StorageError::ReadPath {
                path: runtime.to_owned(),
                source,
            });
        }
    }
    let other_marker = if marker == SHARED_RUNTIME_BINDING {
        EXCLUSIVE_RUNTIME_BINDING
    } else {
        SHARED_RUNTIME_BINDING
    };
    if binding_exists(&runtime.join(other_marker))? {
        return Err(invalid(runtime, "conflicting library program bindings"));
    }
    let binding: SharedProgramBinding = read_record(&runtime.join(marker))?;
    let program = normalize_path(&binding.program_root)?;
    let instance = normalize_path(instance_root)?;
    if contains(&program, &instance) || contains(&instance, &program) {
        return Err(invalid(&program, "shared program overlaps instance data"));
    }
    let identity: ProgramIdentity = read_record(&program.join(PROGRAM_IDENTITY))?;
    if binding.version != 1
        || identity.version != 1
        || binding.program_id.is_empty()
        || binding.program_id != identity.id
        || binding.module_id != identity.module_id
    {
        return Err(invalid(
            &program,
            "shared program identity changed; repair its binding before use",
        ));
    }
    Ok(program)
}

pub(crate) fn ensure_private_data_path(
    instance_root: &Path,
    path: &Path,
) -> Result<(), StorageError> {
    let root = normalize_path(instance_root)?;
    let resource = normalize_resource_path(path)?;
    if !contains(&root, &resource) {
        return Err(invalid(
            path,
            "shared-program instance data must remain inside its instance directory",
        ));
    }
    Ok(())
}

fn read_record<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, StorageError> {
    normalize_resource_path(path)?;
    let mut bytes = Vec::new();
    fs::File::open(path)
        .and_then(|file| file.take(16_385).read_to_end(&mut bytes))
        .map_err(|source| StorageError::ReadPath {
            path: path.to_owned(),
            source,
        })?;
    if bytes.len() > 16_384 {
        return Err(invalid(path, "program ownership record is too large"));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| invalid(path, format!("invalid program ownership record: {error}")))
}

fn write_record(path: &Path, value: &impl Serialize) -> Result<(), StorageError> {
    let bytes = serde_json::to_vec(value)?;
    write_file_atomically(path, &bytes).map_err(|source| StorageError::WriteConfig {
        path: path.to_owned(),
        source,
    })
}

pub(crate) fn invalid(path: &Path, message: impl Into<String>) -> StorageError {
    StorageError::PrivateRuntimeRefresh {
        path: path.to_owned(),
        message: message.into(),
    }
}

#[cfg(test)]
#[path = "program_runtime_tests.rs"]
mod tests;
