use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use app_modules::ModuleDescriptor;
use serde::{Deserialize, Serialize};

use super::invalid;
use crate::StorageError;
use crate::atomic_file::write_file_atomically;
use crate::instance_creation_io::check_creation_cancelled;
use crate::instance_isolation::paths::{normalize_path, normalize_resource_path};
use crate::private_runtime_refresh::{PackageTree, validated_relative_path};

#[path = "program_file_stamp.rs"]
mod file_stamp;
#[path = "program_package_verification.rs"]
mod verification;
use verification::{CachedVerification, PackageVerifier};
#[path = "program_steam_metadata.rs"]
mod steam_metadata;

#[path = "program_package_reuse.rs"]
mod reuse;
pub(crate) use reuse::retired_library_is_clean;

pub(super) const CLEAN_PACKAGE: &str = ".langame-clean-package.json";
const INITIAL_PACKAGE: &str = ".langame-initial-package.json";
pub(super) const MAX_MANIFEST_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CleanPackage {
    pub(super) version: u32,
    pub(super) module_id: String,
    pub(super) source: String,
    pub(super) files: BTreeMap<String, String>,
    pub(super) directories: BTreeSet<String>,
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "verification::read_cached_verifications"
    )]
    pub(super) verified_files: BTreeMap<String, CachedVerification>,
}

#[path = "program_baseline.rs"]
mod baseline;
pub use baseline::{record_library_program_baseline, record_verified_library_program_baseline};

/// Destructive and archive operations always verify the current file bytes.
pub(crate) fn read_clean_package_tree(
    root: &Path,
    cancellation: Option<&AtomicBool>,
) -> Result<Option<PackageTree>, StorageError> {
    read_verified_clean_package(root, None, cancellation)
}

/// Read the installer allowlist for a preview. This does not verify file bytes.
pub(crate) fn read_package_inventory(
    root: &Path,
    module_id: &str,
) -> Result<Option<PackageTree>, StorageError> {
    Ok(read_initial_or_clean_manifest(root)?
        .filter(|manifest| manifest.module_id == module_id)
        .map(|manifest| PackageTree {
            files: manifest.files,
            directories: manifest.directories,
        }))
}

/// Metadata-only copy preview; never use this as proof that payloads are clean.
pub(crate) fn read_clean_package_inventory(
    root: &Path,
    descriptor: &ModuleDescriptor,
) -> Result<Option<PackageTree>, StorageError> {
    let Some(mut manifest) =
        read_manifest(root)?.filter(|manifest| manifest.module_id == descriptor.summary.id)
    else {
        return Ok(None);
    };
    let exclusions = package_exclusions(descriptor)?;
    let omit = |key: &str| excluded(key, &exclusions, descriptor.summary.id == "dontstarve");
    manifest.files.retain(|key, _| !omit(key));
    manifest.directories.retain(|key| !omit(key));
    Ok((!manifest.files.is_empty()).then_some(PackageTree {
        files: manifest.files,
        directories: manifest.directories,
    }))
}

pub(crate) fn require_initial_package_tree(
    root: &Path,
    module_id: &str,
    cancellation: Option<&AtomicBool>,
) -> Result<PackageTree, StorageError> {
    let (name, manifest) = match read_named_manifest(root, INITIAL_PACKAGE)? {
        Some(manifest) => (INITIAL_PACKAGE, Some(manifest)),
        None => (CLEAN_PACKAGE, read_manifest(root)?),
    };
    let mut verifier = PackageVerifier::new(root, true, cancellation);
    let package = verifier.verify(Some(module_id), manifest)?;
    if let Some(manifest) = &package {
        verifier.persist_verification(manifest, name)?;
    }
    package
        .map(into_package_tree)
        .ok_or_else(|| StorageError::CleanLibraryProgramRequired {
            path: root.to_owned(),
        })
}

#[derive(Clone)]
pub(crate) struct CleanCopyInventory {
    pub(crate) module_id: String,
    pub(crate) package: PackageTree,
}

pub(crate) fn read_clean_package_copy_inventory(
    root: &Path,
    module_id: Option<&str>,
) -> Result<Option<CleanCopyInventory>, StorageError> {
    Ok(read_manifest(root)?
        .filter(|manifest| module_id.is_none_or(|id| id == manifest.module_id))
        .map(|manifest| CleanCopyInventory {
            module_id: manifest.module_id.clone(),
            package: into_package_tree(manifest),
        }))
}

pub(crate) fn write_clean_package_copy_inventory(
    root: &Path,
    inventory: &CleanCopyInventory,
) -> Result<(), StorageError> {
    let manifest = CleanPackage {
        version: 1,
        module_id: inventory.module_id.clone(),
        source: "official_clean_install".into(),
        files: inventory.package.files.clone(),
        directories: inventory.package.directories.clone(),
        verified_files: BTreeMap::new(),
    };
    validate_manifest(&manifest, root)?;
    write_manifest(root, &manifest)
}

fn into_package_tree(manifest: CleanPackage) -> PackageTree {
    PackageTree {
        files: manifest.files,
        directories: manifest.directories,
    }
}

fn read_initial_or_clean_manifest(root: &Path) -> Result<Option<CleanPackage>, StorageError> {
    match read_named_manifest(root, INITIAL_PACKAGE)? {
        Some(manifest) => Ok(Some(manifest)),
        None => read_manifest(root),
    }
}

pub(crate) fn require_clean_package_tree(
    root: &Path,
    module_id: &str,
    cancellation: Option<&AtomicBool>,
) -> Result<PackageTree, StorageError> {
    read_verified_clean_package(root, Some(module_id), cancellation)?.ok_or_else(|| {
        StorageError::CleanLibraryProgramRequired {
            path: root.to_owned(),
        }
    })
}

/// Prove the official allowlist still matches this module. Additional files
/// remain untrusted and must be excluded or retained separately during adoption.
pub fn library_program_is_pristine(
    root: &Path,
    descriptor: &ModuleDescriptor,
    cancellation: Option<&AtomicBool>,
) -> Result<bool, StorageError> {
    Ok(read_verified_clean_package(root, Some(&descriptor.summary.id), cancellation)?.is_some())
}

/// After official validation of the same package version, retain only an
/// existing allowlist whose program bytes still match, refreshing only the
/// validated Steam installation metadata already listed for this module. The
/// caller must hold the installation lifecycle lease and invalidate the
/// baseline instead when the version changed: new required files can leave all
/// old hashes unchanged. Unknown files are never added to this allowlist.
pub fn retain_verified_library_program_baseline(
    root: &Path,
    descriptor: &ModuleDescriptor,
    cancellation: Option<&AtomicBool>,
) -> Result<bool, StorageError> {
    retain_baseline(root, descriptor, false, cancellation)
}

/// Finalize a just-published isolated payload. Retained data can change shipped
/// defaults; only files with matching installer identities reuse their hashes.
pub fn retain_published_library_program_baseline(
    root: &Path,
    descriptor: &ModuleDescriptor,
    cancellation: Option<&AtomicBool>,
) -> Result<bool, StorageError> {
    retain_baseline(root, descriptor, true, cancellation)
}

fn retain_baseline(
    root: &Path,
    descriptor: &ModuleDescriptor,
    allow_cached: bool,
    cancellation: Option<&AtomicBool>,
) -> Result<bool, StorageError> {
    let mut verifier = PackageVerifier::new(root, allow_cached, cancellation);
    let module_id = Some(descriptor.summary.id.as_str());
    let original_clean = read_manifest(root)?;
    let clean = prepare_retained_manifest(
        root,
        descriptor,
        original_clean.as_ref(),
        allow_cached,
        cancellation,
    )?;
    let Some(clean) = verifier.verify(module_id, clean)? else {
        record_library_program_baseline(root, descriptor, false, cancellation)?;
        return Ok(false);
    };
    persist_retained_manifest(
        &verifier,
        root,
        original_clean.as_ref(),
        &clean,
        CLEAN_PACKAGE,
        cancellation,
    )?;
    let original_initial = read_named_manifest(root, INITIAL_PACKAGE)?;
    if original_initial.is_some() {
        let initial = prepare_retained_manifest(
            root,
            descriptor,
            original_initial.as_ref(),
            allow_cached,
            cancellation,
        )?;
        if let Some(initial) = verifier.verify(module_id, initial)? {
            persist_retained_manifest(
                &verifier,
                root,
                original_initial.as_ref(),
                &initial,
                INITIAL_PACKAGE,
                cancellation,
            )?;
        } else {
            // Shipped defaults may have become instance data. Keep the filtered
            // program allowlist, but never use that stale full tree for first use.
            check_creation_cancelled(cancellation)?;
            let path = normalize_resource_path(&root.join(INITIAL_PACKAGE))?;
            fs::remove_file(&path).map_err(|source| StorageError::DeletePath { path, source })?;
        }
    }
    Ok(true)
}

fn prepare_retained_manifest(
    root: &Path,
    descriptor: &ModuleDescriptor,
    original: Option<&CleanPackage>,
    allow_cached: bool,
    cancellation: Option<&AtomicBool>,
) -> Result<Option<CleanPackage>, StorageError> {
    let Some(original) = original else {
        return Ok(None);
    };
    let mut manifest = original.clone();
    if !allow_cached
        && !steam_metadata::refresh_existing_entry(root, descriptor, &mut manifest, cancellation)?
    {
        return Ok(None);
    }
    Ok(Some(manifest))
}

fn persist_retained_manifest(
    verifier: &PackageVerifier<'_>,
    root: &Path,
    original: Option<&CleanPackage>,
    manifest: &CleanPackage,
    name: &str,
    cancellation: Option<&AtomicBool>,
) -> Result<(), StorageError> {
    let original = original
        .ok_or_else(|| invalid(root, "program inventory disappeared during verification"))?;
    if original.files == manifest.files {
        return verifier.persist_verification(manifest, name);
    }
    check_creation_cancelled(cancellation)?;
    let current = read_named_manifest(root, name)?
        .ok_or_else(|| invalid(root, "program inventory disappeared during verification"))?;
    if current.module_id != original.module_id
        || current.files != original.files
        || current.directories != original.directories
    {
        return Err(invalid(
            root,
            "program inventory changed during verification",
        ));
    }
    write_named_manifest(root, manifest, name)
}

fn read_verified_clean_package(
    root: &Path,
    module_id: Option<&str>,
    cancellation: Option<&AtomicBool>,
) -> Result<Option<PackageTree>, StorageError> {
    verify_package(root, module_id, read_manifest(root)?, cancellation)
}

fn verify_package(
    root: &Path,
    module_id: Option<&str>,
    manifest: Option<CleanPackage>,
    cancellation: Option<&AtomicBool>,
) -> Result<Option<PackageTree>, StorageError> {
    Ok(PackageVerifier::new(root, false, cancellation)
        .verify(module_id, manifest)?
        .map(into_package_tree))
}

pub(super) fn package_exclusions(
    descriptor: &ModuleDescriptor,
) -> Result<BTreeSet<String>, StorageError> {
    let mut excluded = BTreeSet::from(["steamapps/workshop".into()]);
    for value in descriptor
        .storage
        .runtime_copy_exclusions
        .iter()
        .chain(&descriptor.storage.retained_paths)
    {
        let value = value.replace('\\', "/");
        checked_relative(&value)?;
        excluded.insert(value);
    }
    if let Some(template) = &descriptor.storage.saves_path_template {
        let normalized = template.replace('\\', "/");
        if let Some(relative) = normalized.strip_prefix("{{paths.install_root}}/") {
            let prefix = relative
                .split("{{")
                .next()
                .unwrap_or("")
                .trim_end_matches('/');
            checked_relative(prefix)?;
            excluded.insert(prefix.into());
        }
    }
    Ok(excluded)
}

pub(super) fn excluded(key: &str, exclusions: &BTreeSet<String>, dst: bool) -> bool {
    let key = key.to_ascii_lowercase();
    metadata_path(&key)
        || exclusions.iter().any(|root| {
            let root = root.to_ascii_lowercase();
            key == root || key.starts_with(&(root + "/"))
        })
        || (dst
            && key
                .strip_prefix("mods/")
                .is_some_and(|tail| tail.starts_with("workshop-")))
}

fn metadata_path(key: &str) -> bool {
    key.split('/')
        .any(|part| part.to_ascii_lowercase().starts_with(".langame-"))
}

pub(super) fn read_manifest(root: &Path) -> Result<Option<CleanPackage>, StorageError> {
    read_named_manifest(root, CLEAN_PACKAGE)
}

fn read_named_manifest(root: &Path, name: &str) -> Result<Option<CleanPackage>, StorageError> {
    normalize_path(root)?;
    let path = root.join(name);
    normalize_resource_path(&path)?;
    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(StorageError::ReadPath { path, source }),
    };
    let mut bytes = Vec::new();
    if file
        .metadata()
        .map_err(|source| StorageError::ReadPath {
            path: path.clone(),
            source,
        })?
        .len()
        > MAX_MANIFEST_BYTES
    {
        return Err(invalid(&path, "clean package manifest is too large"));
    }
    file.take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| StorageError::ReadPath {
            path: path.clone(),
            source,
        })?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(invalid(&path, "clean package manifest is too large"));
    }
    let manifest: CleanPackage = serde_json::from_slice(&bytes)
        .map_err(|error| invalid(&path, format!("invalid clean package manifest: {error}")))?;
    validate_manifest(&manifest, root)?;
    Ok(Some(manifest))
}

pub(super) fn validate_manifest(manifest: &CleanPackage, root: &Path) -> Result<(), StorageError> {
    if manifest.version != 1
        || manifest.source != "official_clean_install"
        || manifest.module_id.is_empty()
        || manifest.files.is_empty()
    {
        return Err(invalid(
            root,
            "unsupported or untrusted clean package manifest",
        ));
    }
    let mut folded = BTreeSet::new();
    let mut prefixes = BTreeMap::new();
    let file_keys = manifest
        .files
        .keys()
        .map(|key| key.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    for key in manifest.files.keys().chain(&manifest.directories) {
        checked_relative(key)?;
        if metadata_path(key) || !folded.insert(key.to_ascii_lowercase()) {
            return Err(invalid(root, "unsafe or conflicting clean package paths"));
        }
        let mut prefix = String::new();
        for component in key.split('/') {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(component);
            if let Some(previous) = prefixes.insert(prefix.to_ascii_lowercase(), prefix.clone())
                && previous != prefix
            {
                return Err(invalid(root, "clean package path spellings conflict"));
            }
        }
        let mut ancestor = Path::new(key).parent();
        while let Some(parent) = ancestor {
            if file_keys.contains(
                &parent
                    .to_string_lossy()
                    .replace('\\', "/")
                    .to_ascii_lowercase(),
            ) {
                return Err(invalid(
                    root,
                    "clean package file is also a directory ancestor",
                ));
            }
            ancestor = parent.parent();
        }
    }
    if manifest.files.values().any(|hash| {
        hash.len() != 64
            || !hash
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    }) {
        return Err(invalid(root, "invalid clean package SHA-256"));
    }
    Ok(())
}

pub(super) fn checked_relative(value: &str) -> Result<PathBuf, StorageError> {
    if value.split('/').any(|part| {
        part.is_empty()
            || part == "."
            || part == ".."
            || part.ends_with(['.', ' '])
            || part
                .chars()
                .any(|ch| ch.is_control() || matches!(ch, '<' | '>' | '|' | '?' | '*' | '"'))
    }) {
        return Err(invalid(Path::new(value), "unsafe clean package path"));
    }
    validated_relative_path(value)
}

pub(super) fn write_manifest(root: &Path, manifest: &CleanPackage) -> Result<(), StorageError> {
    write_named_manifest(root, manifest, CLEAN_PACKAGE)
}

fn write_named_manifest(
    root: &Path,
    manifest: &CleanPackage,
    name: &str,
) -> Result<(), StorageError> {
    let path = root.join(name);
    normalize_resource_path(&path)?;
    let bytes = encode_manifest(&path, manifest, MAX_MANIFEST_BYTES)?;
    write_file_atomically(&path, &bytes)
        .map_err(|source| StorageError::WriteConfig { path, source })
}

pub(super) fn encode_manifest(
    path: &Path,
    manifest: &CleanPackage,
    max_bytes: u64,
) -> Result<Vec<u8>, StorageError> {
    let mut bytes = serde_json::to_vec(manifest)?;
    if bytes.len() as u64 > max_bytes && !manifest.verified_files.is_empty() {
        #[derive(Serialize)]
        struct WithoutCache<'a> {
            version: u32,
            module_id: &'a str,
            source: &'a str,
            files: &'a BTreeMap<String, String>,
            directories: &'a BTreeSet<String>,
        }
        // The optional speedup must not reduce the supported package size.
        bytes = serde_json::to_vec(&WithoutCache {
            version: manifest.version,
            module_id: &manifest.module_id,
            source: &manifest.source,
            files: &manifest.files,
            directories: &manifest.directories,
        })?;
    }
    if bytes.len() as u64 > max_bytes {
        return Err(invalid(path, "clean package manifest is too large"));
    }
    Ok(bytes)
}
