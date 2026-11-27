use super::*;
use std::collections::BTreeSet;

pub(super) fn prepare(
    paths: &StoragePaths,
    moves: &[RootMove],
    database: &db::Database,
    copied: bool,
    plan: &mut MetadataPlan,
) -> Result<(), StorageError> {
    let mut libraries = BTreeSet::new();
    for row in &database.rows {
        match row.table.as_str() {
            "instances" => {
                let id = row
                    .key_value
                    .as_str()
                    .ok_or_else(|| invalid(&paths.database_path, "Instance ID is not a string."))?;
                let module = row_text(&row.before, "module_id", &paths.database_path)?;
                let config = Path::new(row_text(&row.before, "config_path", &paths.database_path)?);
                let root = crate::instance_reconciliation::checked_instance_root(paths, config)?;
                instance(
                    InstanceContext {
                        root: &root,
                        logical_root: &root,
                        id,
                        module,
                        allow_missing_program: false,
                    },
                    moves,
                    copied,
                    plan,
                )?;
            }
            "game_installs"
                if row_text(&row.before, "scope", &paths.database_path)? == "library" =>
            {
                let root = Path::new(row_text(&row.before, "install_root", &paths.database_path)?);
                let module = row_text(&row.before, "module_id", &paths.database_path)?;
                if libraries.insert(normalize_resource_path(root)?) {
                    program(root, module, moves, copied, plan)?;
                }
            }
            _ => {}
        }
    }
    steamcmd(&paths.steamcmd_root, moves, copied, plan)
}

pub(super) struct InstanceContext<'a> {
    pub root: &'a Path,
    pub logical_root: &'a Path,
    pub id: &'a str,
    pub module: &'a str,
    pub allow_missing_program: bool,
}

pub(super) fn instance(
    instance: InstanceContext<'_>,
    moves: &[RootMove],
    copied: bool,
    plan: &mut MetadataPlan,
) -> Result<(), StorageError> {
    let InstanceContext {
        root,
        logical_root,
        id,
        module,
        allow_missing_program,
    } = instance;
    let config = root.join("config/instance.json");
    let bytes = read_bytes(&config, 4 * 1024 * 1024)?;
    let value: Value = serde_json::from_slice(&bytes)?;
    if text(&value, "instance_id", &config)? != id || text(&value, "module_id", &config)? != module
    {
        return Err(invalid(
            &config,
            "Instance configuration does not match its registered owner.",
        ));
    }
    let settings = value
        .get("settings")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid(&config, "Instance settings must be an object."))?;
    preflight::reject_references(&Value::Object(settings.clone()), &config, "settings", moves)?;
    let runtime = root.join("runtime");
    for name in [
        ".langame-shared-program.json",
        ".langame-exclusive-program.json",
    ] {
        binding(
            &runtime.join(name),
            module,
            allow_missing_program,
            moves,
            copied,
            plan,
        )?;
    }
    projection(&runtime, logical_root, moves, copied, plan)?;
    program(&runtime, module, moves, copied, plan)?;
    backups(root, logical_root, id, moves, copied, plan)?;
    startup::prepare(root, logical_root, id, moves, copied, plan)?;
    console::prepare(&root.join("logs/managed-console"), moves, copied, plan)
}

fn version(value: &Value, expected: u64, path: &Path) -> Result<(), StorageError> {
    if value.get("version").and_then(Value::as_u64) != Some(expected) {
        return Err(invalid(path, "Unsupported management metadata version."));
    }
    Ok(())
}

fn binding(
    path: &Path,
    module: &str,
    allow_missing_program: bool,
    moves: &[RootMove],
    copied: bool,
    plan: &mut MetadataPlan,
) -> Result<(), StorageError> {
    if !exists(path)? {
        return Ok(());
    }
    let (before, mut value) = read_json(path)?;
    version(&value, 1, path)?;
    if text(&value, "module_id", path)? != module || text(&value, "program_id", path)?.is_empty() {
        return Err(invalid(
            path,
            "Program binding does not match the instance module.",
        ));
    }
    let root = PathBuf::from(text(&value, "program_root", path)?);
    let identity_path = root.join(".langame-program-identity.json");
    if !allow_missing_program || exists(&identity_path)? {
        let (_, identity) = read_json(&identity_path)?;
        version(&identity, 1, &identity_path)?;
        if text(&identity, "module_id", &identity_path)? != module
            || text(&identity, "id", &identity_path)? != text(&value, "program_id", path)?
        {
            return Err(invalid(
                path,
                "Program reference no longer matches its source identity.",
            ));
        }
    }
    if copied && exists(&identity_path)? {
        let target_identity = remap_path(&identity_path, moves)?;
        if read_bytes(&target_identity, MAX_JSON_BYTES)?
            != read_bytes(&identity_path, MAX_JSON_BYTES)?
        {
            return Err(invalid(
                &target_identity,
                "Copied program identity differs from the registered source.",
            ));
        }
    }
    rewrite_field(&mut value, "program_root", moves, path)?;
    add_json_patch(path, &before, &value, moves, copied, plan)
}

fn program(
    root: &Path,
    module: &str,
    moves: &[RootMove],
    copied: bool,
    plan: &mut MetadataPlan,
) -> Result<(), StorageError> {
    if !exists(root)? {
        return Ok(());
    }
    if !owned::plain_directory(root)? {
        return Err(invalid(root, "Program storage is not a plain directory."));
    }
    let identity_path = root.join(".langame-program-identity.json");
    if exists(&identity_path)? {
        let (_, value) = read_json(&identity_path)?;
        version(&value, 1, &identity_path)?;
        if text(&value, "id", &identity_path)?.is_empty()
            || text(&value, "module_id", &identity_path)? != module
        {
            return Err(invalid(
                &identity_path,
                "Program identity does not match its registered module.",
            ));
        }
    }
    for (name, field) in [
        (".langame-program-acquisition.json", "target"),
        (".langame-retained-library.json", "program_root"),
    ] {
        let path = root.join(name);
        if !exists(&path)? {
            continue;
        }
        let (before, mut value) = read_json(&path)?;
        version(&value, 1, &path)?;
        if text(&value, "module_id", &path)? != module
            || !same_path(Path::new(text(&value, field, &path)?), root)?
        {
            return Err(invalid(
                &path,
                "Program ownership metadata does not belong to this directory and module.",
            ));
        }
        if field == "target" && uuid::Uuid::parse_str(text(&value, "id", &path)?).is_err() {
            return Err(invalid(&path, "Program acquisition identity is invalid."));
        }
        rewrite_field(&mut value, field, moves, &path)?;
        add_json_patch(&path, &before, &value, moves, copied, plan)?;
    }
    for name in [
        ".langame-clean-package.json",
        ".langame-initial-package.json",
    ] {
        let path = root.join(name);
        if !exists(&path)? || !moved(&path, moves)? {
            continue;
        }
        let (before, mut value) = read_json(&path)?;
        version(&value, 1, &path)?;
        if text(&value, "module_id", &path)? != module
            || text(&value, "source", &path)? != "official_clean_install"
        {
            return Err(invalid(
                &path,
                "Program verification metadata is not a supported clean package.",
            ));
        }
        let object = value
            .as_object_mut()
            .ok_or_else(|| invalid(&path, "Program manifest is not an object."))?;
        // FileStamp is an optional cache. Keep official content hashes and force
        // the existing verifier to establish fresh evidence after a copy.
        object.remove("verified_files");
        add_json_patch(&path, &before, &value, moves, copied, plan)?;
    }
    Ok(())
}

fn projection(
    runtime: &Path,
    logical_root: &Path,
    moves: &[RootMove],
    copied: bool,
    plan: &mut MetadataPlan,
) -> Result<(), StorageError> {
    let path = runtime.join(".langame-runtime-projection.json");
    if exists(&path)? {
        let (before, mut value) = read_json(&path)?;
        version(&value, 1, &path)?;
        if !same_path(
            Path::new(text(&value, "instance_root", &path)?),
            logical_root,
        )? {
            return Err(invalid(
                &path,
                "Runtime projection belongs to a different instance.",
            ));
        }
        rewrite_field(&mut value, "instance_root", moves, &path)?;
        rewrite_field(&mut value, "source_root", moves, &path)?;
        add_json_patch(&path, &before, &value, moves, copied, plan)?;
    }
    let path = runtime.join(".langame-package-baseline.json");
    if exists(&path)? {
        let (before, mut value) = read_json(&path)?;
        version(&value, 2, &path)?;
        let source = PathBuf::from(text(&value, "source_root", &path)?);
        if moved(&source, moves)? && !exists(&source)? {
            return Err(invalid(
                &path,
                "The runtime package baseline refers to a missing source directory; repair that source before moving its path.",
            ));
        }
        if moved(&source, moves)? && copied {
            let target = remap_path(&source, moves)?;
            let canonical = fs::canonicalize(&target).map_err(|source| StorageError::ReadPath {
                path: target,
                source,
            })?;
            value["source_root"] = Value::String(canonical.to_string_lossy().into_owned());
        }
        add_json_patch(&path, &before, &value, moves, copied, plan)?;
    }
    Ok(())
}

fn backups(
    root: &Path,
    logical_root: &Path,
    instance_id: &str,
    moves: &[RootMove],
    copied: bool,
    plan: &mut MetadataPlan,
) -> Result<(), StorageError> {
    let directory = root.join("backups");
    if !exists(&directory)? {
        return Ok(());
    }
    if !owned::plain_directory(&directory)? {
        return Err(invalid(&directory, "Backups are not in a plain directory."));
    }
    for (index, entry) in fs::read_dir(&directory)
        .map_err(|source| StorageError::ReadPath {
            path: directory.clone(),
            source,
        })?
        .enumerate()
    {
        if index >= 100_000 {
            return Err(invalid(
                &directory,
                "Too many backup entries to migrate safely.",
            ));
        }
        let entry = entry.map_err(|source| StorageError::ReadPath {
            path: directory.clone(),
            source,
        })?;
        let path = entry.path().join("backup.json");
        if !exists(&path)? {
            continue;
        }
        let (before, mut value) = read_json(&path)?;
        let backup_id = text(&value, "backup_id", &path)?;
        let expected = logical_root.join("backups").join(backup_id);
        if text(&value, "instance_id", &path)? != instance_id
            || entry.file_name().to_str() != Some(backup_id)
            || !same_path(Path::new(text(&value, "backup_path", &path)?), &expected)?
        {
            return Err(invalid(
                &path,
                "Backup manifest does not match its registered instance and directory.",
            ));
        }
        rewrite_field(&mut value, "backup_path", moves, &path)?;
        rewrite_field(&mut value, "saves_path", moves, &path)?;
        add_json_patch(&path, &before, &value, moves, copied, plan)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SteamCmdReadyFile {
    name: String,
    length: u64,
    modified_ns: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SteamCmdReadyRecord {
    root: String,
    files: Vec<SteamCmdReadyFile>,
}

fn steamcmd_fingerprint(root: &Path) -> Option<SteamCmdReadyRecord> {
    let canonical = fs::canonicalize(root).ok()?;
    let mut files = Vec::new();
    for name in ["steamcmd.exe", "steamclient.dll", "steamclient64.dll"] {
        let metadata = match fs::metadata(canonical.join(name)) {
            Ok(metadata) if metadata.is_file() => metadata,
            _ if name != "steamcmd.exe" => continue,
            _ => return None,
        };
        let modified_ns = metadata
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_nanos();
        if metadata.len() == 0 || modified_ns == 0 {
            return None;
        }
        files.push(SteamCmdReadyFile {
            name: name.to_owned(),
            length: metadata.len(),
            modified_ns,
        });
    }
    Some(SteamCmdReadyRecord {
        root: canonical.to_string_lossy().to_ascii_lowercase(),
        files,
    })
}

fn steamcmd_ready(
    root: &Path,
    moves: &[RootMove],
    copied: bool,
    plan: &mut MetadataPlan,
) -> Result<(), StorageError> {
    let path = root.join(".langame-steamcmd-ready.json");
    if !exists(&path)? {
        return Ok(());
    }
    let before = read_bytes(&path, 16 * 1024)?;
    let Ok(record) = serde_json::from_slice::<SteamCmdReadyRecord>(&before) else {
        return Ok(());
    };
    let Some(source_current) = steamcmd_fingerprint(root) else {
        return Ok(());
    };
    // Rebind readiness only when the source runtime was genuinely verified
    // immediately before migration; never promote an unverified or altered runtime.
    if record != source_current || !copied || !moved(root, moves)? {
        return Ok(());
    }
    let target_root = remap_path(root, moves)?;
    let target_file = remap_path(&path, moves)?;
    if read_bytes(&target_file, 16 * 1024)? != before {
        return Err(invalid(
            &target_file,
            "Copied SteamCMD readiness metadata differs from its validated source.",
        ));
    }
    let Some(target_current) = steamcmd_fingerprint(&target_root) else {
        return Err(invalid(
            &target_root,
            "Copied SteamCMD runtime is incomplete after file verification.",
        ));
    };
    if target_current.files.len() != source_current.files.len()
        || target_current.files.iter().zip(&source_current.files).any(
            |(after_file, before_file)| {
                after_file.name != before_file.name || after_file.length != before_file.length
            },
        )
    {
        return Err(invalid(
            &target_root,
            "Copied SteamCMD runtime files do not match the verified source lengths.",
        ));
    }
    let after = serde_json::to_vec(&target_current)?;
    if after == before {
        return Ok(());
    }
    let total: usize = plan
        .files
        .iter()
        .map(|patch| patch.before.len().saturating_add(patch.after.len()))
        .sum();
    if total
        .saturating_add(before.len())
        .saturating_add(after.len())
        > super::MAX_PLAN_BYTES
    {
        return Err(invalid(
            &target_file,
            "Migration metadata plan exceeds its byte limit.",
        ));
    }
    plan.files.push(FilePatch {
        path: target_file,
        before_sha256: digest(&before),
        before,
        after,
    });
    Ok(())
}

fn steamcmd(
    root: &Path,
    moves: &[RootMove],
    copied: bool,
    plan: &mut MetadataPlan,
) -> Result<(), StorageError> {
    let path = root.join(".langame-steamcmd-owner.json");
    if !exists(&path)? {
        return Ok(());
    }
    let (before, mut value) = read_json(&path)?;
    let canonical = fs::canonicalize(root).map_err(|source| StorageError::ReadPath {
        path: root.to_owned(),
        source,
    })?;
    if text(&value, "product", &path)? != "langame-server-manager"
        || !text(&value, "canonical_root", &path)?
            .eq_ignore_ascii_case(&canonical.to_string_lossy())
    {
        return Err(invalid(
            &path,
            "SteamCMD ownership does not match its current canonical directory.",
        ));
    }
    if copied && moved(root, moves)? {
        let target = remap_path(root, moves)?;
        let canonical = fs::canonicalize(&target).map_err(|source| StorageError::ReadPath {
            path: target,
            source,
        })?;
        value["canonical_root"] = Value::String(canonical.to_string_lossy().into_owned());
    }
    add_json_patch(&path, &before, &value, moves, copied, plan)?;
    steamcmd_ready(root, moves, copied, plan)
}
