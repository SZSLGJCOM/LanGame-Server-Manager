use super::*;

const MAX_PACKAGE_ENTRIES: usize = 50_000;
const MAX_PACKAGE_DEPTH: usize = 24;
const MAX_CONFIG_BYTES: u64 = 16 * 1024 * 1024;
const CONAN_NATIVE_OWNERSHIP_FILE: &str = ".lgsm-conan-mod-payloads.json";

#[derive(Clone, Copy)]
enum PayloadKind {
    Conan,
    Barotrauma,
}

impl PayloadKind {
    fn module_id(self) -> &'static str {
        match self {
            Self::Conan => "conanexiles",
            Self::Barotrauma => "barotrauma",
        }
    }

    fn matches(self, path: &Path, standalone: bool) -> bool {
        match self {
            Self::Conan => path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pak")),
            Self::Barotrauma if standalone => path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("xml")),
            Self::Barotrauma => path
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("filelist.xml")),
        }
    }
}

struct Package {
    root: PathBuf,
    payload: PathBuf,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct NativePakOwnership {
    item_id: String,
    sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous_sha256: Option<String>,
}

type NativePakOwners = std::collections::BTreeMap<String, NativePakOwnership>;

pub(super) fn materialize_barotrauma_workshop_mods(
    context: &ModuleSupportMaterializationContext<'_>,
    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    if let Some(plan) = prepare_barotrauma_workshop_mods(context)? {
        files.apply(vec![plan])?;
    }
    Ok(())
}

pub(super) fn prepare_barotrauma_workshop_mods(
    context: &ModuleSupportMaterializationContext<'_>,
) -> Result<Option<ManagedConfigMergePlan>, StorageError> {
    #[cfg(test)]
    crate::instance_archive::test_gate::pause(
        &context.storage_paths.database_path,
        crate::instance_archive::test_gate::Point::Packages,
    );
    if !context.install_root.exists() {
        return Ok(None);
    }
    let kind = PayloadKind::Barotrauma;
    let local_root = context.config_dir.join(BAROTRAUMA_LOCAL_MODS_DIR);
    let entries = local_entries(&local_root, kind)?;
    let packages = resolve_packages(
        context,
        &local_root,
        &entries,
        kind,
        BAROTRAUMA_WORKSHOP_APP_ID,
        &std::collections::BTreeMap::new(),
    )?;
    let config_path = context.config_dir.join(BAROTRAUMA_CONFIG_PLAYER_FILE);
    // The running game reads and updates this instance file. The package copy
    // is only a seed for an instance that has never had its own configuration.
    let original = read_optional_config(&config_path, kind)?;
    let original_bytes = original.as_ref().map(|value| value.as_bytes().to_vec());
    let baseline = match original {
        Some(existing) => Some(existing),
        None => read_optional_config(
            &context.install_root.join(BAROTRAUMA_CONFIG_PLAYER_FILE),
            kind,
        )?,
    };
    let mut paths = Vec::new();
    for (id, package) in packages {
        let payload = if package.root.starts_with(&local_root) {
            package.payload
        } else {
            let destination = local_root.join(id);
            package_staging::replace_package_directory(
                kind.module_id(),
                &package.root,
                &destination,
            )?;
            destination.join(
                package
                    .payload
                    .strip_prefix(&package.root)
                    .map_err(|error| failure(kind, &package.payload, error))?,
            )
        };
        let relative = payload
            .strip_prefix(context.config_dir)
            .map_err(|error| failure(kind, &payload, error))?;
        paths.push(relative.to_string_lossy().replace('\\', "/"));
    }
    let rendered = render_barotrauma_config_player_xml(
        baseline.as_deref().unwrap_or_default(),
        &paths,
        context.config_dir,
    )
    .map_err(|error| failure(kind, &config_path, error))?;
    Ok(Some(ManagedConfigMergePlan {
        original: original_bytes,
        destination_path: config_path,
        replacement: rendered.into_bytes(),
    }))
}

pub(super) fn materialize_conan_modlist(
    context: &ModuleSupportMaterializationContext<'_>,
    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    files.apply(vec![prepare_conan_modlist(context)?])
}

pub(super) fn prepare_conan_modlist(
    context: &ModuleSupportMaterializationContext<'_>,
) -> Result<ManagedConfigMergePlan, StorageError> {
    #[cfg(test)]
    crate::instance_archive::test_gate::pause(
        &context.storage_paths.database_path,
        crate::instance_archive::test_gate::Point::Packages,
    );
    let kind = PayloadKind::Conan;
    let local_root = context.install_root.join("ConanSandbox/Mods");
    let path = local_root.join(CONAN_MODLIST_FILE);
    let original = managed_config_merge::read_optional_bytes(&path)?;
    let entries = local_entries(&local_root, kind)?;
    let ownership_path = context.config_dir.join(CONAN_NATIVE_OWNERSHIP_FILE);
    let mut previous_record = read_optional_config(&ownership_path, kind)?.map(String::into_bytes);
    let mut native_owners: NativePakOwners = previous_record
        .as_deref()
        .map(serde_json::from_slice)
        .transpose()
        .map_err(|error| failure(kind, &ownership_path, error))?
        .unwrap_or_default();
    for (name, owner) in &native_owners {
        let valid_hash = |value: &str| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        };
        if !name.ends_with(".pak")
            || name.contains(['/', '\\', ':'])
            || name.chars().any(char::is_control)
            || name != &name.to_ascii_lowercase()
            || owner.item_id.len() < 5
            || !owner.item_id.bytes().all(|byte| byte.is_ascii_digit())
            || !valid_hash(&owner.sha256)
            || owner
                .previous_sha256
                .as_deref()
                .is_some_and(|hash| !valid_hash(hash))
        {
            return Err(failure(
                kind,
                &ownership_path,
                "The native PAK ownership record has invalid identifiers or SHA-256 values.",
            ));
        }
    }
    let packages = resolve_packages(
        context,
        &local_root,
        &entries,
        kind,
        CONAN_WORKSHOP_APP_ID,
        &native_owners,
    )?;
    let mut names = Vec::new();
    let mut owners = std::collections::HashMap::new();
    // Validate the complete native namespace before publishing any payload.
    // Windows and Conan treat differently-cased PAK names as the same target.
    for (id, package) in &packages {
        let name = package
            .payload
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                failure(
                    kind,
                    &package.payload,
                    "The PAK filename must be valid Unicode.",
                )
            })?;
        if let Some(previous) = owners.insert(name.to_ascii_lowercase(), id) {
            return Err(failure(
                kind,
                &package.payload,
                format!(
                    "Mods {previous} and {id} use the same native PAK filename {name}; rename or remove the conflicting package before enabling both."
                ),
            ));
        }
        names.push(name.to_owned());
    }
    // Preflight every target before the first copy. Installed packages own only
    // the exact bytes recorded during their own publication, never arbitrary
    // same-named user PAKs in the native directory.
    for ((id, package), name) in packages.iter().zip(&names) {
        let destination = local_root.join(name);
        if package.payload != destination {
            let hashes = owned_hashes(&native_owners, name, id, &destination)?;
            package_staging::validate_owned_target(&destination, &hashes)
                .map_err(|error| failure(kind, &destination, error))?;
        }
    }
    for ((id, package), name) in packages.iter().zip(&names) {
        let destination = local_root.join(name);
        // A manually imported PAK already lives at its native destination.
        if package.payload != destination {
            let hashes = owned_hashes(&native_owners, name, id, &destination)?;
            let mut next_owners = native_owners.clone();
            let bytes = package_staging::copy_package_file_with_record(
                kind.module_id(),
                &package.payload,
                &destination,
                &ownership_path,
                previous_record.as_deref(),
                &hashes,
                |digest, prior_digest| {
                    let key = name.to_ascii_lowercase();
                    let owner = NativePakOwnership {
                        item_id: id.clone(),
                        sha256: digest.to_owned(),
                        previous_sha256: None,
                    };
                    next_owners.insert(key.clone(), owner.clone());
                    let finalized =
                        serde_json::to_vec_pretty(&next_owners).map_err(std::io::Error::other)?;
                    let mut pending = next_owners.clone();
                    pending.insert(
                        key,
                        NativePakOwnership {
                            previous_sha256: prior_digest.map(str::to_owned),
                            ..owner
                        },
                    );
                    Ok((
                        serde_json::to_vec_pretty(&pending).map_err(std::io::Error::other)?,
                        finalized,
                    ))
                },
            )?;
            native_owners = next_owners;
            previous_record = Some(bytes);
        }
    }
    Ok(ManagedConfigMergePlan {
        original,
        destination_path: path,
        replacement: render_conan_modlist_lines(&names).into_bytes(),
    })
}

fn owned_hashes(
    owners: &NativePakOwners,
    name: &str,
    id: &str,
    target: &Path,
) -> Result<Vec<String>, StorageError> {
    let Some(owner) = owners.get(&name.to_ascii_lowercase()) else {
        return Ok(Vec::new());
    };
    if owner.item_id != id {
        return Err(failure(
            PayloadKind::Conan,
            target,
            "The native PAK is owned by another Mod; it will not be overwritten.",
        ));
    }
    Ok(std::iter::once(owner.sha256.clone())
        .chain(owner.previous_sha256.clone())
        .collect())
}

fn resolve_packages(
    context: &ModuleSupportMaterializationContext<'_>,
    local_root: &Path,
    entries: &[(PathBuf, bool)],
    kind: PayloadKind,
    app_id: &str,
    native_owners: &NativePakOwners,
) -> Result<Vec<(String, Package)>, StorageError> {
    let ids = parse_workshop_id_list(context.settings, "mod_workshop_ids");
    let enabled: HashSet<_> = ids.iter().map(String::as_str).collect();
    let mut directories = std::collections::HashMap::new();
    for (path, directory) in entries {
        let Some(id) = local_package_id(path).filter(|id| enabled.contains(id.as_str())) else {
            continue;
        };
        if *directory {
            let package = inspect_package(path, kind)?;
            if directories.insert(id.clone(), package).is_some() {
                return Err(ambiguous(kind, local_root, &id));
            }
        }
    }
    let mut packages = Vec::new();
    for id in ids {
        let matching = entries
            .iter()
            .filter(|(path, directory)| {
                // Only recorded LGSM output is a native mirror. A same-named
                // operator file remains a separate source and must be disambiguated.
                let mirror = !directory
                    && path.file_name().is_some_and(|name| {
                        native_owners
                            .get(&name.to_string_lossy().to_ascii_lowercase())
                            .is_some_and(|owner| {
                                owner.item_id != id || directories.contains_key(&owner.item_id)
                            })
                    });
                !mirror && local_package_id(path).as_deref() == Some(id.as_str())
            })
            .collect::<Vec<_>>();
        let local = if matching.len() == 1 {
            Some(match directories.remove(&id) {
                Some(package) => package,
                None => inspect_package(&matching[0].0, kind)?,
            })
        } else if matching.is_empty() {
            None
        } else {
            return Err(ambiguous(kind, local_root, &id));
        };
        let mut selected = local;
        if selected.is_none() {
            for cache in [
                context.shared_install_root,
                context.storage_paths.steamcmd_root.as_path(),
            ] {
                let root = cache
                    .join("steamapps/workshop/content")
                    .join(app_id)
                    .join(&id);
                if optional_metadata(&root, kind)?.is_some() {
                    selected = Some(inspect_package(&root, kind)?);
                    break;
                }
            }
        }
        let package = selected.ok_or_else(|| failure(kind, local_root, format!(
            "Enabled Mod {id} has no installed payload; install it before enabling or starting the server."
        )))?;
        packages.push((id, package));
    }
    Ok(packages)
}

fn local_package_id(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    // Keep this aligned with the desktop numeric_prefix inventory contract.
    let prefix: String = name.chars().take_while(char::is_ascii_digit).collect();
    (prefix.len() >= 5).then_some(prefix)
}

fn local_entries(root: &Path, kind: PayloadKind) -> Result<Vec<(PathBuf, bool)>, StorageError> {
    reject_link_ancestors(root, kind)?;
    fs::create_dir_all(root).map_err(|error| failure(kind, root, error))?;
    let mut entries = Vec::new();
    for (count, entry) in fs::read_dir(root)
        .map_err(|error| failure(kind, root, error))?
        .enumerate()
    {
        if count >= MAX_PACKAGE_ENTRIES {
            return Err(failure(kind, root, "Mod entry count exceeds the limit."));
        }
        let path = entry.map_err(|error| failure(kind, root, error))?.path();
        let metadata = optional_metadata(&path, kind)?
            .ok_or_else(|| failure(kind, &path, "Mod entry disappeared during discovery."))?;
        if metadata.is_dir() || (metadata.is_file() && kind.matches(&path, true)) {
            entries.push((path, metadata.is_dir()));
        }
    }
    Ok(entries)
}

fn inspect_package(root: &Path, kind: PayloadKind) -> Result<Package, StorageError> {
    reject_link_ancestors(root, kind)?;
    let mut found = None;
    let mut visited = 0;
    inspect_entry(root, kind, true, 0, &mut visited, &mut found)?;
    let payload = found.ok_or_else(|| {
        failure(
            kind,
            root,
            "The installed Mod has no native package payload.",
        )
    })?;
    Ok(Package {
        root: root.to_owned(),
        payload,
    })
}

fn inspect_entry(
    path: &Path,
    kind: PayloadKind,
    standalone: bool,
    depth: usize,
    visited: &mut usize,
    found: &mut Option<PathBuf>,
) -> Result<(), StorageError> {
    *visited += 1;
    if depth >= MAX_PACKAGE_DEPTH || *visited > MAX_PACKAGE_ENTRIES {
        return Err(failure(
            kind,
            path,
            "Mod package size or directory depth exceeds the limit.",
        ));
    }
    let metadata = optional_metadata(path, kind)?
        .ok_or_else(|| failure(kind, path, "Mod entry disappeared during discovery."))?;
    if metadata.is_dir() {
        for entry in fs::read_dir(path).map_err(|error| failure(kind, path, error))? {
            let entry = entry.map_err(|error| failure(kind, path, error))?;
            inspect_entry(&entry.path(), kind, false, depth + 1, visited, found)?;
        }
    } else if metadata.is_file() && kind.matches(path, standalone) {
        if found.is_some() {
            return Err(failure(
                kind,
                path,
                "The Mod contains multiple native package payloads; select an unambiguous package.",
            ));
        }
        *found = Some(path.to_owned());
    } else if !metadata.is_file() {
        return Err(failure(
            kind,
            path,
            "Mod entries must be regular files or directories.",
        ));
    }
    Ok(())
}

fn read_optional_config(path: &Path, kind: PayloadKind) -> Result<Option<String>, StorageError> {
    use std::io::Read;
    reject_link_ancestors(path, kind)?;
    match fs::File::open(path) {
        Ok(file) => {
            let metadata = file
                .metadata()
                .map_err(|error| failure(kind, path, error))?;
            if !metadata.is_file() || metadata.len() > MAX_CONFIG_BYTES {
                return Err(failure(
                    kind,
                    path,
                    "Configuration must be a regular file no larger than 16 MiB.",
                ));
            }
            let mut text = String::new();
            file.take(MAX_CONFIG_BYTES + 1)
                .read_to_string(&mut text)
                .map_err(|error| failure(kind, path, error))?;
            if text.len() as u64 > MAX_CONFIG_BYTES {
                return Err(failure(
                    kind,
                    path,
                    "Configuration exceeds the 16 MiB limit.",
                ));
            }
            Ok(Some(text))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(failure(kind, path, error)),
    }
}

fn reject_link_ancestors(path: &Path, kind: PayloadKind) -> Result<(), StorageError> {
    for ancestor in path.ancestors().filter(|path| !path.as_os_str().is_empty()) {
        optional_metadata(ancestor, kind)?;
    }
    Ok(())
}

fn optional_metadata(path: &Path, kind: PayloadKind) -> Result<Option<fs::Metadata>, StorageError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            let linked = metadata.file_type().is_symlink();
            #[cfg(windows)]
            let linked = {
                use std::os::windows::fs::MetadataExt;
                linked || metadata.file_attributes() & 0x400 != 0
            };
            if linked {
                Err(failure(
                    kind,
                    path,
                    "Linked Mod or configuration entries cannot be materialized.",
                ))
            } else {
                Ok(Some(metadata))
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(failure(kind, path, error)),
    }
}

fn ambiguous(kind: PayloadKind, root: &Path, id: &str) -> StorageError {
    failure(
        kind,
        root,
        format!(
            "Multiple local packages match Mod ID {id}; keep one unambiguous source before enabling it."
        ),
    )
}

fn failure(kind: PayloadKind, path: &Path, message: impl std::fmt::Display) -> StorageError {
    StorageError::ModuleSupportMaterialization {
        module_id: kind.module_id().to_owned(),
        path: path.to_owned(),
        message: message.to_string(),
    }
}
