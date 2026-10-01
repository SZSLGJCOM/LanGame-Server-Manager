use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use app_modules::ModuleDescriptor;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;

use crate::StorageError;
use crate::instance_archive_files::{self as files, native};
use crate::instance_archive_store::{digest, invalid};
use crate::instance_isolation::paths::{contains, normalize_path, normalize_resource_path};
use crate::private_runtime_refresh::{PackageTree, validated_relative_path};

#[path = "instance_archive_program_restore.rs"]
mod reconstruction;
pub(crate) use reconstruction::{prepare_staging, restore_files};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProgramFile {
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProgramPlan {
    pub version: u32,
    pub module_id: String,
    pub package_fingerprint: String,
    pub current_version: Option<String>,
    pub files: BTreeMap<String, ProgramFile>,
    pub restore_token: String,
}

impl ProgramPlan {
    pub(crate) fn validate(&self, module: &str) -> Result<(), StorageError> {
        if self.version != 1
            || self.module_id != module
            || self.files.is_empty()
            || self.files.len() > 200_000
            || !valid_hash(&self.package_fingerprint)
            || uuid::Uuid::parse_str(&self.restore_token).is_err()
        {
            return Err(invalid(
                Path::new(module),
                "Archive program reconstruction metadata is invalid.",
            ));
        }
        let mut total = 0_u64;
        let mut folded = std::collections::BTreeSet::new();
        for (key, entry) in &self.files {
            validated_relative_path(key)?;
            if !key.starts_with("runtime/")
                || protected_name(key)
                || !valid_hash(&entry.sha256)
                || !folded.insert(key.to_ascii_lowercase())
            {
                return Err(invalid(
                    Path::new(key),
                    "Archive program path or hash is invalid.",
                ));
            }
            total = total
                .checked_add(entry.bytes)
                .ok_or_else(|| invalid(Path::new(module), "Archive program sizes overflow."))?;
        }
        Ok(())
    }
    pub(crate) fn bytes(&self) -> u64 {
        self.files.values().map(|entry| entry.bytes).sum()
    }
    pub(crate) fn staging(&self, root: &Path) -> PathBuf {
        root.join(format!(".langame-archive-rebuild-{}", self.restore_token))
    }
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
fn fingerprint(module: &str, tree: &PackageTree) -> Result<String, StorageError> {
    Ok(digest(&serde_json::to_vec(&(module, tree))?))
}

/// Advisory inventory only. Restore separately verifies the allowlisted payloads
/// while holding the installation lifecycle lock; a summary never authorizes copying.
pub(crate) fn manifest_fingerprint(root: &Path, module: &str) -> Result<String, StorageError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Manifest {
        version: u32,
        module_id: String,
        source: String,
        files: BTreeMap<String, String>,
        directories: std::collections::BTreeSet<String>,
        // Optional first-use acceleration does not change the program identity.
        #[serde(default, rename = "verified_files")]
        _verified_files: Option<serde::de::IgnoredAny>,
    }
    normalize_path(root)?;
    let path = root.join(".langame-clean-package.json");
    let mut file = open_file(&path, false)?;
    let mut bytes = Vec::new();
    file.reader()
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| StorageError::ReadPath {
            path: path.clone(),
            source,
        })?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err(invalid(
            &path,
            "Official program manifest exceeds its supported size.",
        ));
    }
    let manifest: Manifest = serde_json::from_slice(&bytes)?;
    if manifest.version != 1
        || manifest.module_id != module
        || manifest.source != "official_clean_install"
        || manifest.files.len() + manifest.directories.len() > 200_000
    {
        return Err(invalid(
            &path,
            "The installed official program manifest is unavailable or invalid.",
        ));
    }
    for (relative, sha) in &manifest.files {
        validated_relative_path(relative)?;
        if !valid_hash(sha) {
            return Err(invalid(
                &path,
                "Official program manifest contains an invalid digest.",
            ));
        }
    }
    for relative in &manifest.directories {
        validated_relative_path(relative)?;
    }
    fingerprint(
        module,
        &PackageTree {
            files: manifest.files,
            directories: manifest.directories,
        },
    )
}

pub(crate) async fn library(
    connection: &mut SqliteConnection,
    module: &str,
) -> Result<Option<(PathBuf, Option<String>)>, StorageError> {
    let row: Option<(String, Option<String>)> = sqlx::query_as("SELECT install_root,current_version FROM game_installs WHERE module_id=?1 AND scope='library' AND owner_instance_id IS NULL AND install_state='installed' ORDER BY id DESC LIMIT 1")
        .bind(module).fetch_optional(connection).await?;
    Ok(row.map(|(path, version)| (PathBuf::from(path), version)))
}

pub(crate) async fn source_is_idle(
    connection: &mut SqliteConnection,
    root: &Path,
) -> Result<bool, StorageError> {
    let active: Vec<String> = sqlx::query_scalar("SELECT g.install_root FROM game_installs g JOIN instances i ON i.install_id=g.id WHERE i.status NOT IN ('stopped','error') OR EXISTS(SELECT 1 FROM instance_runs r WHERE r.instance_id=i.id AND r.status='running') LIMIT 4097")
        .fetch_all(connection).await?;
    if active.len() > 4096 {
        return Ok(false);
    }
    let expected = normalize_path(root)?;
    for path in active {
        if normalize_path(Path::new(&path))? == expected {
            return Ok(false);
        }
    }
    Ok(true)
}

/// This is a content identity, not a claim that an operator's whole runtime is
/// pristine. Only individually identical, non-personal files may be omitted.
pub(crate) fn plan(
    root: &Path,
    saves: &Path,
    descriptor: Option<&ModuleDescriptor>,
    library: Option<(PathBuf, Option<String>)>,
) -> Result<(Option<ProgramPlan>, Option<String>), StorageError> {
    let Some(descriptor) = descriptor else {
        return Ok((
            None,
            Some("Module metadata is unavailable; all program files were retained.".into()),
        ));
    };
    let Some((library, version)) = library else {
        return Ok((
            None,
            Some("No installed library copy is available; all program files were retained.".into()),
        ));
    };
    let library_root = normalize_path(&library)?;
    let instance_root = normalize_path(root)?;
    if contains(&library_root, &instance_root) || contains(&instance_root, &library_root) {
        return Err(invalid(
            &library,
            "Archive reconstruction source overlaps the instance.",
        ));
    }
    let package = match crate::program_seed::require_clean_package_tree(
        &library,
        &descriptor.summary.id,
        None,
    ) {
        Ok(package) => package,
        Err(error) => {
            return Ok((
                None,
                Some(format!(
                    "The library has no matching verified official package; all program files were retained: {error}"
                )),
            ));
        }
    };
    let runtime = root.join("runtime");
    let mut protected = descriptor
        .storage
        .runtime_copy_exclusions
        .iter()
        .chain(&descriptor.storage.retained_paths)
        .map(|path| path.replace('\\', "/").to_ascii_lowercase())
        .collect::<Vec<_>>();
    if let Ok(relative) = saves.strip_prefix(&runtime) {
        protected.push(
            relative
                .to_string_lossy()
                .replace('\\', "/")
                .to_ascii_lowercase(),
        );
    }
    let mut omitted = BTreeMap::new();
    for (relative, expected) in &package.files {
        let key = format!("runtime/{relative}");
        let lower = relative.to_ascii_lowercase();
        if protected_name(&key)
            || protected.iter().any(|path| {
                path.is_empty() || lower == *path || lower.starts_with(&format!("{path}/"))
            })
        {
            continue;
        }
        let path = runtime.join(validated_relative_path(relative)?);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => return Err(StorageError::ReadPath { path, source }),
            Ok(metadata) if !metadata.is_file() => continue,
            Ok(_) => {}
        }
        let mut file = open_file(&path, false)?;
        let (actual, bytes) = hash(&mut file, &path)?;
        if actual == *expected {
            omitted.insert(
                key,
                ProgramFile {
                    sha256: actual,
                    bytes,
                },
            );
        }
    }
    if omitted.is_empty() {
        return Ok((None, Some("No unchanged official program files were eligible; personal, modified and unknown files were retained.".into())));
    }
    let plan = ProgramPlan {
        version: 1,
        module_id: descriptor.summary.id.clone(),
        package_fingerprint: fingerprint(&descriptor.summary.id, &package)?,
        current_version: version,
        files: omitted,
        restore_token: uuid::Uuid::new_v4().to_string(),
    };
    plan.validate(&descriptor.summary.id)?;
    Ok((Some(plan), None))
}

fn protected_name(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    if lower.split('/').any(|part| {
        part.starts_with(".langame-")
            || matches!(
                part,
                "config"
                    | "configs"
                    | "configuration"
                    | "saves"
                    | "saved"
                    | "worlds"
                    | "backups"
                    | "mods"
                    | "plugins"
                    | "patchers"
                    | "userdata"
            )
    }) {
        return true;
    }
    Path::new(&lower)
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            matches!(
                extension,
                "ini"
                    | "cfg"
                    | "conf"
                    | "config"
                    | "json"
                    | "toml"
                    | "yaml"
                    | "yml"
                    | "xml"
                    | "properties"
                    | "txt"
                    | "csv"
                    | "log"
                    | "sav"
                    | "save"
                    | "db"
                    | "sqlite"
                    | "sqlite3"
                    | "zip"
                    | "tar"
                    | "gz"
                    | "bak"
                    | "acf"
                    | "vdf"
                    | "bat"
                    | "cmd"
                    | "ps1"
                    | "sh"
            )
        })
}

pub(crate) fn verify_library(root: &Path, plan: &ProgramPlan) -> Result<(), StorageError> {
    let package = crate::program_seed::require_clean_package_tree(root, &plan.module_id, None)
        .map_err(|error| invalid(root, format!("Install or repair the exact archived program version before restoring; no download was started: {error}")))?;
    verify_package_identity(root, plan, &package)
}

fn verify_package_identity(
    root: &Path,
    plan: &ProgramPlan,
    package: &PackageTree,
) -> Result<(), StorageError> {
    if fingerprint(&plan.module_id, package)? != plan.package_fingerprint {
        return Err(invalid(
            root,
            "The installed library package does not match this archive's exact program fingerprint. Install or repair that version first; no download was started.",
        ));
    }
    for (key, expected) in &plan.files {
        if package
            .files
            .get(key.strip_prefix("runtime/").unwrap_or_default())
            != Some(&expected.sha256)
        {
            return Err(invalid(
                root,
                "Archive reconstruction files do not match the verified library manifest.",
            ));
        }
    }
    Ok(())
}

pub(crate) fn omit_files(
    root: &Path,
    identity: &str,
    library: &Path,
    plan: &ProgramPlan,
) -> Result<(), StorageError> {
    let _root = files::guard_identity(root, identity)?;
    let _library = native::open(library, true, false).map_err(|source| StorageError::ReadPath {
        path: library.to_owned(),
        source,
    })?;
    // finish_archiving verifies the complete library before moving the instance.
    // Recheck the manifest here without rereading the whole payload; each source
    // is still hashed through its protected handle immediately before unlinking.
    let inventory =
        crate::program_seed::read_clean_package_copy_inventory(library, Some(&plan.module_id))?
            .ok_or_else(|| invalid(library, "The archived program manifest is unavailable."))?;
    verify_package_identity(library, plan, &inventory.package)?;
    files::preflight_tree(root, identity)?;
    // Validate every listed file before the first removal. Already absent entries
    // are the only allowed completed steps when resuming this durable plan.
    for (key, expected) in &plan.files {
        let path = root.join(validated_relative_path(key)?);
        if path.try_exists().map_err(|source| StorageError::ReadPath {
            path: path.clone(),
            source,
        })? {
            let mut file = open_file(&path, false)?;
            check_file(&mut file, &path, expected)?;
        }
    }
    for (key, expected) in &plan.files {
        // A resumed archive may already have omitted this target. Its source is
        // still required to reconstruct it, so verify before skipping absence.
        let source = library.join(key.strip_prefix("runtime/").unwrap());
        let mut available = open_file(&source, false)?;
        check_file(&mut available, &source, expected)?;
        let path = root.join(validated_relative_path(key)?);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => return Err(StorageError::ReadPath { path, source }),
            Ok(_) => {}
        }
        let mut file = open_file(&path, true)?;
        check_file(&mut file, &path, expected)?;
        file.remove()
            .map_err(|source| StorageError::DeletePath { path, source })?;
    }
    Ok(())
}

fn open_file(path: &Path, mutate: bool) -> Result<native::OwnedNode, StorageError> {
    normalize_resource_path(path)?;
    native::open_verified_file(path, mutate).map_err(|source| StorageError::ReadPath {
        path: path.to_owned(),
        source,
    })
}
fn hash(file: &mut native::OwnedNode, path: &Path) -> Result<(String, u64), StorageError> {
    let mut digest = Sha256::new();
    let mut buffer = vec![0; 256 * 1024];
    let mut bytes = 0_u64;
    loop {
        let count = file
            .reader()
            .read(&mut buffer)
            .map_err(|source| StorageError::ReadPath {
                path: path.to_owned(),
                source,
            })?;
        if count == 0 {
            break;
        }
        #[cfg(test)]
        crate::instance_archive::read_probe::record(path, count as u64);
        digest.update(&buffer[..count]);
        bytes += count as u64;
    }
    Ok((
        digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        bytes,
    ))
}
fn check_file(
    file: &mut native::OwnedNode,
    path: &Path,
    expected: &ProgramFile,
) -> Result<(), StorageError> {
    let (sha, bytes) = hash(file, path)?;
    if sha != expected.sha256 || bytes != expected.bytes {
        return Err(invalid(
            path,
            "Program file changed after archive planning; existing bytes were preserved.",
        ));
    }
    Ok(())
}
