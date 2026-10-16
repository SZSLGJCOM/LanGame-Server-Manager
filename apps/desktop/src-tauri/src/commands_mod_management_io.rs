//! Durable intent precedes renames. Recovery completes an interrupted operation;
//! reported failures roll back while retaining the journal if recovery is needed.
use super::*;

#[cfg(test)]
thread_local! {
    static FAIL_NEXT_PUBLICATION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(in crate::commands::commands_mod_staging::online_packages) fn fail_next_publication() {
    FAIL_NEXT_PUBLICATION.with(|failure| failure.set(true));
}

pub(super) struct ManagementPaths {
    pub(super) target: PathBuf,
    pub(super) root: PathBuf,
    pub(super) disabled: PathBuf,
    pub(super) journal: PathBuf,
    identity: String,
}

impl ManagementPaths {
    pub(super) fn new(target: &Path) -> Result<Self, String> {
        validate_manual_mod_target_path(target)?;
        let resolved = resolve_manual_mod_path_with_missing_components(target)?;
        let identity = resolved.to_string_lossy().replace('/', "\\").to_lowercase();
        let digest = sha2::Sha256::digest(identity.as_bytes());
        let suffix = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let root = management_storage_parent(target)?.join(format!(".langame-mod-state-{suffix}"));
        validate_manual_mod_existing_path_components(&root, false)?;
        if let Ok(metadata) = fs::symlink_metadata(&root)
            && (!metadata.is_dir() || manual_mod_metadata_is_reparse(&metadata))
        {
            return Err("Mod management storage is not a plain directory".into());
        }
        let paths = Self {
            target: target.into(),
            disabled: root.join("disabled"),
            journal: root.join("pending.json"),
            root,
            identity,
        };
        paths.validate_owner()?;
        Ok(paths)
    }

    fn validate_owner(&self) -> Result<(), String> {
        if !entry_exists(&self.root)? {
            return Ok(());
        }
        let owner = self.root.join("owner.json");
        let metadata = validate_manual_mod_source_filesystem_entry(&owner)?;
        if !metadata.is_file() || metadata.len() > 32768 {
            return Err("Invalid Mod management storage identity".into());
        }
        let identity: String =
            serde_json::from_slice(&fs::read(owner).map_err(|error| error.to_string())?)
                .map_err(|_| "Invalid Mod management storage identity")?;
        if identity != self.identity {
            return Err("Mod management storage belongs to another target".into());
        }
        Ok(())
    }

    fn prepare(&self) -> Result<(), String> {
        if !entry_exists(&self.root)? {
            fs::create_dir(&self.root)
                .map_err(|error| format!("Cannot create Mod management storage: {error}"))?;
            write_new_synced(
                &self.root.join("owner.json"),
                &serde_json::to_vec(&self.identity).map_err(|error| error.to_string())?,
            )?;
        }
        self.validate_owner()?;
        for path in [&self.disabled, &self.root.join("preserved")] {
            validate_manual_mod_existing_path_components(path, false)?;
            if !entry_exists(path)? {
                fs::create_dir(path).map_err(|error| error.to_string())?;
            }
            if !validate_manual_mod_source_filesystem_entry(path)?.is_dir() {
                return Err("Mod management storage contains an invalid directory".into());
            }
        }
        Ok(())
    }
}

/// A sibling of `~mods` is still inside the recursive Pak loading tree. For
/// nested plugin containers, keep disabled payloads outside the outermost one.
pub(super) fn management_storage_parent(target: &Path) -> Result<&Path, String> {
    let mut storage_parent = target.parent().ok_or("Mod target has no parent")?;
    for (index, ancestor) in target.ancestors().enumerate() {
        let name = ancestor
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if ["paks", "bepinex", "plugins", "ue4ss"]
            .iter()
            .any(|container| name.eq_ignore_ascii_case(container))
        {
            storage_parent = ancestor
                .parent()
                .ok_or("Mod loading container has no parent")?;
        } else if index > 0 {
            break;
        }
    }
    Ok(storage_parent)
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ManagementTransaction {
    schema: u8,
    key: String,
    action: String,
    retained_name: String,
    original: InstalledPackages,
    adopted: Option<InstalledPackage>,
    had_payload: bool,
    payload_files: BTreeMap<String, String>,
}

impl ManagementTransaction {
    pub(super) fn new(
        key: String,
        action: &str,
        manifest: &mut InstalledPackages,
        adopted: Option<InstalledPackage>,
    ) -> Result<Self, String> {
        let transaction = Self {
            schema: 1,
            key,
            action: action.into(),
            retained_name: uuid::Uuid::new_v4().simple().to_string(),
            original: std::mem::take(manifest),
            adopted,
            had_payload: true,
            payload_files: BTreeMap::new(),
        };
        transaction.validate()?;
        Ok(transaction)
    }

    fn package(&self) -> Result<&InstalledPackage, String> {
        self.adopted
            .as_ref()
            .or_else(|| self.original.packages.get(&self.key))
            .ok_or_else(|| String::from("Journal package is missing"))
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema != 1
            || !matches!(
                self.action.as_str(),
                "adopt" | "enable" | "disable" | "delete"
            )
        {
            return Err("Invalid Mod management journal".into());
        }
        validate_component(&self.key)?;
        validate_component(&self.retained_name)?;
        validate_management_manifest(&self.original)?;
        if self.action == "adopt" {
            let adopted = self
                .adopted
                .as_ref()
                .ok_or("Adoption journal has no ownership evidence")?;
            if self.key != format!("local-{}", adopted.target_name)
                || self.original.packages.contains_key(&self.key)
                || !adopted.enabled
            {
                return Err("Invalid adoption journal".into());
            }
        } else if self.adopted.is_some() || !self.original.packages.contains_key(&self.key) {
            return Err("Invalid Mod management journal ownership".into());
        }
        validate_management_manifest(&self.desired())?;
        let mut payload_record = copy_package(self.package()?);
        payload_record.files = self.payload_files.clone();
        validate_management_manifest(&InstalledPackages {
            packages: [(self.key.clone(), payload_record)].into(),
        })?;
        Ok(())
    }

    fn desired(&self) -> InstalledPackages {
        let mut manifest = InstalledPackages {
            packages: self
                .original
                .packages
                .iter()
                .map(|(key, package)| (key.clone(), copy_package(package)))
                .collect(),
        };
        if let Some(package) = &self.adopted {
            manifest
                .packages
                .insert(self.key.clone(), copy_package(package));
        }
        match self.action.as_str() {
            "delete" => {
                manifest.packages.remove(&self.key);
            }
            "enable" | "disable" => {
                if let Some(package) = manifest.packages.get_mut(&self.key) {
                    package.enabled = self.action == "enable";
                }
            }
            _ => {}
        }
        manifest
    }

    fn locations(&self, paths: &ManagementPaths) -> Result<(PathBuf, PathBuf), String> {
        let package = self.package()?;
        let source = if package.enabled {
            paths.target.join(&package.target_name)
        } else {
            paths.disabled.join(&package.target_name)
        };
        let destination = match self.action.as_str() {
            "enable" => paths.target.join(&package.target_name),
            "disable" => paths.disabled.join(&package.target_name),
            "delete" => paths
                .root
                .join("preserved")
                .join(&self.retained_name)
                .join(&package.target_name),
            _ => source.clone(),
        };
        Ok((source, destination))
    }
}

#[derive(Default)]
pub(super) struct ManagementOutcome {
    pub(super) preserved_path: Option<String>,
    pub(super) removed_file_count: usize,
    pub(super) warnings: Vec<String>,
}

fn write_new_synced(path: &Path, bytes: &[u8]) -> Result<(), String> {
    validate_manual_mod_existing_path_components(path, false)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| {
            format!(
                "Cannot create Mod operation record {}: {error}",
                path.display()
            )
        })?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| error.to_string())
}

fn entry_exists(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!(
            "Cannot inspect Mod operation path {}: {error}",
            path.display()
        )),
    }
}

fn manifest_equal(left: &InstalledPackages, right: &InstalledPackages) -> Result<bool, String> {
    Ok(
        serde_json::to_value(left).map_err(|error| error.to_string())?
            == serde_json::to_value(right).map_err(|error| error.to_string())?,
    )
}

fn validate_manifest_transition(
    paths: &ManagementPaths,
    transaction: &ManagementTransaction,
) -> Result<(), String> {
    let file = paths.target.join(ONLINE_MOD_MANIFEST);
    if !entry_exists(&file)? {
        return Ok(());
    }
    let current = read_manifest(&file)?;
    if !manifest_equal(&current, &transaction.original)?
        && !manifest_equal(&current, &transaction.desired())?
    {
        return Err(
            "Mod ownership changed during an interrupted operation; recovery data was retained"
                .into(),
        );
    }
    Ok(())
}

fn publish_manifest(paths: &ManagementPaths, manifest: &InstalledPackages) -> Result<(), String> {
    #[cfg(test)]
    if FAIL_NEXT_PUBLICATION.with(|failure| failure.replace(false)) {
        return Err("Injected Mod ownership publication failure".into());
    }
    validate_management_manifest(manifest)?;
    validate_manual_mod_target_path(&paths.target)?;
    fs::create_dir_all(&paths.target).map_err(|error| error.to_string())?;
    let bytes = serde_json::to_vec(manifest).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err("Mod ownership record exceeds its size limit".into());
    }
    let next = paths
        .root
        .join(format!("manifest-{}.json", uuid::Uuid::new_v4().simple()));
    write_new_synced(&next, &bytes)?;
    let destination = paths.target.join(ONLINE_MOD_MANIFEST);
    // The durable journal contains the complete previous record before this gap.
    if entry_exists(&destination)? {
        read_manifest(&destination)?;
        fs::remove_file(&destination).map_err(|error| error.to_string())?;
    }
    fs::rename(&next, destination).map_err(|error| {
        format!("Cannot publish Mod ownership record; recovery is available: {error}")
    })
}

fn move_payload(
    paths: &ManagementPaths,
    transaction: &ManagementTransaction,
    recovering: bool,
) -> Result<(), String> {
    if transaction.action == "adopt" {
        return Ok(());
    }
    move_routed_patchers(paths, transaction, recovering)?;
    let (source, destination) = transaction.locations(paths)?;
    validate_manual_mod_existing_path_components(&source, false)?;
    validate_manual_mod_existing_path_components(&destination, false)?;
    let source_exists = entry_exists(&source)?;
    let destination_exists = entry_exists(&destination)?;
    if destination_exists {
        fingerprint_tree(&destination)?;
        if recovering
            && !source_exists
            && fingerprint_tree(&destination)? == transaction.payload_files
        {
            return Ok(());
        }
        return Err("Mod destination already exists; no files were overwritten".into());
    }
    if !source_exists {
        if transaction.action == "delete" && !transaction.had_payload {
            return Ok(());
        }
        return Err(
            "Managed Mod payload is missing; ownership and recovery data were preserved".into(),
        );
    }
    if fingerprint_tree(&source)? != transaction.payload_files {
        return Err("Mod payload changed during the operation; files were preserved".into());
    }
    if let Some(parent) = destination.parent() {
        validate_manual_mod_existing_path_components(parent, false)?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    validate_manual_mod_existing_path_components(&source, false)?;
    validate_manual_mod_existing_path_components(&destination, false)?;
    fs::rename(source, destination).map_err(|error| format!("Cannot move Mod payload: {error}"))
}

fn roll_back(paths: &ManagementPaths, transaction: &ManagementTransaction) -> Result<(), String> {
    if transaction.action != "adopt" {
        let (source, destination) = transaction.locations(paths)?;
        if entry_exists(&destination)? {
            if entry_exists(&source)? {
                return Err("Rollback source conflicts with another file".into());
            }
            fingerprint_tree(&destination)?;
            validate_manual_mod_existing_path_components(&source, false)?;
            fs::rename(destination, source).map_err(|error| error.to_string())?;
        }
    }
    rollback_routed_patchers(paths, transaction)?;
    validate_manifest_transition(paths, transaction)?;
    publish_manifest(paths, &transaction.original)?;
    fs::remove_file(&paths.journal).map_err(|error| error.to_string())
}

pub(super) fn execute(
    paths: &ManagementPaths,
    transaction: &ManagementTransaction,
) -> Result<ManagementOutcome, String> {
    paths.prepare()?;
    let mut serialized = serde_json::to_value(transaction).map_err(|error| error.to_string())?;
    let (source, destination) = transaction.locations(paths)?;
    if transaction.action != "adopt" && entry_exists(&destination)? {
        return Err("Mod destination already exists; no files were overwritten".into());
    }
    let had_payload = entry_exists(&source)?;
    if transaction.action != "delete" && !had_payload {
        return Err("Managed Mod payload is missing".into());
    }
    let payload_files = if had_payload {
        fingerprint_tree(&source)?
    } else {
        BTreeMap::new()
    };
    serialized["had_payload"] = had_payload.into();
    serialized["payload_files"] =
        serde_json::to_value(payload_files).map_err(|error| error.to_string())?;
    let transaction: ManagementTransaction =
        serde_json::from_value(serialized).map_err(|error| error.to_string())?;
    write_new_synced(
        &paths.journal,
        &serde_json::to_vec(&transaction).map_err(|error| error.to_string())?,
    )?;
    if let Err(error) = move_payload(paths, &transaction, false) {
        return match roll_back(paths, &transaction) {
            Ok(()) => Err(format!("{error}; the operation was rolled back")),
            Err(cleanup) => Err(format!(
                "{error}; recovery record could not be cleared: {cleanup}"
            )),
        };
    }
    let applied = (|| {
        validate_manifest_transition(paths, &transaction)?;
        publish_manifest(paths, &transaction.desired())
    })();
    if let Err(error) = applied {
        return match roll_back(paths, &transaction) {
            Ok(()) => Err(format!("{error}; the operation was rolled back")),
            Err(rollback) => Err(format!(
                "{error}; rollback needs recovery: {rollback}; recovery record: {}",
                paths.journal.display()
            )),
        };
    }
    finish(paths, &transaction)
}

pub(super) fn recover(paths: &ManagementPaths) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(&paths.journal) {
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
        Ok(metadata) => metadata,
    };
    if !metadata.is_file()
        || manual_mod_metadata_is_reparse(&metadata)
        || metadata.len() > MAX_MANIFEST_BYTES * 2 + 65536
    {
        return Err(
            "Unsafe or oversized Mod recovery record; existing files were preserved".into(),
        );
    }
    let mut bytes = Vec::new();
    fs::File::open(&paths.journal)
        .and_then(|file| {
            file.take(MAX_MANIFEST_BYTES * 2 + 65537)
                .read_to_end(&mut bytes)
        })
        .map_err(|error| error.to_string())?;
    let transaction: ManagementTransaction = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Invalid Mod recovery record: {error}"))?;
    transaction.validate()?;
    validate_manifest_transition(paths, &transaction)?;
    let current = read_manifest(&paths.target.join(ONLINE_MOD_MANIFEST))?;
    let committed = manifest_equal(&current, &transaction.desired())?;
    let (source, destination) = transaction.locations(paths)?;
    if !(committed
        && (transaction.action == "adopt"
            || (!entry_exists(&source)?
                && (entry_exists(&destination)? || transaction.action == "delete"))))
    {
        move_payload(paths, &transaction, true)?;
    }
    publish_manifest(paths, &transaction.desired())?;
    finish(paths, &transaction)?;
    Ok(())
}

fn finish(
    paths: &ManagementPaths,
    transaction: &ManagementTransaction,
) -> Result<ManagementOutcome, String> {
    let mut outcome = ManagementOutcome::default();
    if transaction.action == "delete" {
        for (relative, hash) in &transaction.package()?.routed_files {
            if !routed::is_patcher(relative) {
                continue;
            }
            let (_, retained) = routed_locations(paths, transaction, relative)?;
            if entry_exists(&retained)? && fingerprint_tree(&retained)?.get("") == Some(hash) {
                fs::remove_file(&retained).map_err(|error| error.to_string())?;
                outcome.removed_file_count += 1;
            }
        }
        let (_, retained) = transaction.locations(paths)?;
        let current = fingerprint_tree(&retained)?;
        for (relative, expected) in &transaction.package()?.files {
            let candidate = if relative.is_empty() {
                transaction.package()?.target_name.as_str()
            } else {
                relative.as_str()
            };
            if protected_file(candidate) || current.get(relative) != Some(expected) {
                continue;
            }
            let file = if relative.is_empty() {
                retained.clone()
            } else {
                retained.join(relative)
            };
            validate_manual_mod_existing_path_components(&file, false)?;
            // Rehash immediately before removal, including on resumed cleanup.
            if fingerprint_tree(&file)?.get("") != Some(expected) {
                continue;
            }
            match fs::remove_file(&file) {
                Ok(()) => {
                    outcome.removed_file_count += 1;
                }
                Err(error) => outcome.warnings.push(format!(
                    "Preserved {} because it could not be removed: {error}",
                    file.display()
                )),
            }
        }
        remove_empty_directories(&retained)?;
        if entry_exists(&retained)? {
            outcome.preserved_path = Some(retained.to_string_lossy().into_owned());
            outcome.warnings.push("Configuration, modified, or unowned files were preserved outside the Mod loading directory".into());
        }
    }
    fs::remove_file(&paths.journal).map_err(|error| {
        format!("Mod operation completed but its recovery record could not be cleared: {error}")
    })?;
    Ok(outcome)
}

fn routed_locations(
    paths: &ManagementPaths,
    transaction: &ManagementTransaction,
    relative: &str,
) -> Result<(PathBuf, PathBuf), String> {
    let package = transaction.package()?;
    let active = routed::runtime_root(&paths.target)?.join(relative);
    let disabled = paths
        .root
        .join("disabled-routes")
        .join(&transaction.key)
        .join(relative);
    let source = if package.enabled {
        active.clone()
    } else {
        disabled.clone()
    };
    let destination = match transaction.action.as_str() {
        "enable" => active,
        "disable" => disabled,
        "delete" => paths
            .root
            .join("preserved")
            .join(&transaction.retained_name)
            .join("runtime")
            .join(relative),
        _ => source.clone(),
    };
    Ok((source, destination))
}

fn move_routed_patchers(
    paths: &ManagementPaths,
    transaction: &ManagementTransaction,
    recovering: bool,
) -> Result<(), String> {
    for (relative, hash) in &transaction.package()?.routed_files {
        if !routed::is_patcher(relative) {
            continue;
        }
        let (source, destination) = routed_locations(paths, transaction, relative)?;
        validate_manual_mod_existing_path_components(&source, false)?;
        validate_manual_mod_existing_path_components(&destination, false)?;
        let source_exists = entry_exists(&source)?;
        let destination_exists = entry_exists(&destination)?;
        if recovering
            && !source_exists
            && destination_exists
            && fingerprint_tree(&destination)?.get("") == Some(hash)
        {
            continue;
        }
        if recovering && transaction.action == "delete" && !source_exists && !destination_exists {
            continue;
        }
        if destination_exists || fingerprint_tree(&source)?.get("") != Some(hash) {
            return Err(format!(
                "Patcher {relative} is missing, modified, or conflicts with another file; files were preserved"
            ));
        }
        fs::create_dir_all(
            destination
                .parent()
                .ok_or("Patcher destination has no parent")?,
        )
        .map_err(|error| error.to_string())?;
        validate_manual_mod_existing_path_components(&destination, false)?;
        fs::rename(&source, &destination).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn rollback_routed_patchers(
    paths: &ManagementPaths,
    transaction: &ManagementTransaction,
) -> Result<(), String> {
    if transaction.action == "adopt" {
        return Ok(());
    }
    for (relative, hash) in transaction.package()?.routed_files.iter().rev() {
        if !routed::is_patcher(relative) {
            continue;
        }
        let (source, destination) = routed_locations(paths, transaction, relative)?;
        // An untouched source proves this route was never moved. A conflicting
        // destination belongs to someone else and must not block other restores.
        if entry_exists(&source)? || !entry_exists(&destination)? {
            continue;
        }
        if fingerprint_tree(&destination)?.get("") != Some(hash) {
            return Err(format!(
                "Patcher rollback conflicts at {relative}; recovery data was retained"
            ));
        }
        validate_manual_mod_existing_path_components(&source, false)?;
        validate_manual_mod_existing_path_components(&destination, false)?;
        fs::create_dir_all(source.parent().ok_or("Patcher source has no parent")?)
            .map_err(|error| error.to_string())?;
        fs::rename(destination, source).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn remove_empty_directories(root: &Path) -> Result<(), String> {
    if !entry_exists(root)? || !validate_manual_mod_source_filesystem_entry(root)?.is_dir() {
        return Ok(());
    }
    let mut pending = vec![root.to_path_buf()];
    let mut directories = Vec::new();
    while let Some(path) = pending.pop() {
        if directories.len() > MAX_PACKAGE_FILES {
            return Err("Mod cleanup exceeds its entry limit".into());
        }
        directories.push(path.clone());
        for entry in fs::read_dir(&path).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            if validate_manual_mod_source_filesystem_entry(&entry.path())?.is_dir() {
                pending.push(entry.path());
            }
        }
    }
    for directory in directories.iter().rev() {
        match fs::remove_dir(directory) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::DirectoryNotEmpty => {}
            Err(error) => {
                return Err(format!(
                    "Cannot clean empty Mod directory {}: {error}",
                    directory.display()
                ));
            }
        }
    }
    Ok(())
}
