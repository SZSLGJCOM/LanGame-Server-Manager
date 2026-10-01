use std::fs::{self, File, Metadata};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use serde_json::Value;

const SERVER: &str = "R5/ServerDescription.json";
const DATABASE: &str = "R5/Saved/SaveProfiles/Default/RocksDB_v2";
const MAX_DOCUMENT_BYTES: u64 = 64 * 1024;
const MAX_VERSION_ENTRIES: usize = 64;
const MAX_WORLD_ENTRIES: usize = 256;

pub(super) fn ready(install_root: &Path, fixture_root: &Path) -> Result<bool, String> {
    let root = checked_root(install_root, fixture_root)?;
    ready_in_root(&root)
}

/// Read-only inspection of the selected existing instance's effective program
/// root, supplied by its pinned backend launch preview. No fresh-world gate,
/// bootstrap, configuration changes or fixture cleanup is performed here.
pub(super) fn ready_existing(install_root: &Path) -> Result<bool, String> {
    for ancestor in install_root.ancestors() {
        let metadata = fs::symlink_metadata(ancestor)
            .map_err(|_| "Windrose existing runtime cannot be inspected")?;
        require_plain(&metadata)?;
        if !metadata.is_dir() {
            return Err("Windrose runtime requires plain directories".into());
        }
    }
    let root = install_root
        .canonicalize()
        .map_err(|_| "Windrose runtime cannot be resolved")?;
    ready_in_root(&root)
}

fn ready_in_root(root: &Path) -> Result<bool, String> {
    let Some(server) = read_document(root, Path::new(SERVER))? else {
        return Ok(false);
    };
    let Some(selected) = server
        .pointer("/ServerDescription_Persistent/WorldIslandId")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    else {
        return Ok(false);
    };
    validate_world_name(selected)?;
    let mut matches = 0;
    for version in version_directories(root)? {
        let relative = version
            .join("Worlds")
            .join(selected)
            .join("WorldDescription.json");
        if checked_path(root, &relative, false)?.is_none() {
            continue;
        }
        matches += 1;
        let Some(world) = read_document(root, &relative)? else {
            return Ok(false);
        };
        if world
            .pointer("/WorldDescription/islandId")
            .and_then(Value::as_str)
            != Some(selected)
        {
            return Ok(false);
        }
    }
    Ok(matches == 1)
}

pub(super) fn require_fresh(install_root: &Path, fixture_root: &Path) -> Result<(), String> {
    let root = checked_root(install_root, fixture_root)?;
    if read_document(&root, Path::new(SERVER))?
        .and_then(|server| {
            server
                .pointer("/ServerDescription_Persistent/WorldIslandId")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .is_some_and(|id| !id.is_empty())
    {
        return Err("fresh Windrose fixture already selects a world before startup".into());
    }
    let mut world_entries = 0;
    for version in version_directories(&root)? {
        let worlds = version.join("Worlds");
        let Some(directory) = checked_path(&root, &worlds, true)? else {
            continue;
        };
        for entry in fs::read_dir(directory).map_err(|_| "cannot inspect fresh Windrose worlds")? {
            world_entries += 1;
            if world_entries > MAX_WORLD_ENTRIES {
                return Err("fresh Windrose world inventory exceeds its entry limit".into());
            }
            let entry = entry.map_err(|_| "cannot inspect fresh Windrose world entry")?;
            let relative = worlds.join(entry.file_name());
            let metadata = fs::symlink_metadata(entry.path())
                .map_err(|_| "cannot inspect fresh Windrose world metadata")?;
            require_plain(&metadata)?;
            if metadata.is_dir()
                && checked_path(&root, &relative.join("WorldDescription.json"), false)?.is_some()
            {
                return Err("fresh Windrose fixture already contains a native world".into());
            }
        }
    }
    Ok(())
}

fn validate_world_name(value: &str) -> Result<(), String> {
    // Match the storage resolver's single-folder identity contract. The native
    // examples use GUIDs, but their spelling is not a declared schema constraint.
    if value.trim().is_empty()
        || value.trim() != value
        || value.contains(['/', '\\', '\0'])
        || Path::new(value).components().count() != 1
        || !matches!(
            Path::new(value).components().next(),
            Some(Component::Normal(_))
        )
    {
        return Err("Windrose world identity is not a safe folder name".into());
    }
    Ok(())
}

fn checked_root(install_root: &Path, fixture_root: &Path) -> Result<PathBuf, String> {
    for path in [fixture_root, install_root] {
        for ancestor in path.ancestors() {
            let metadata = fs::symlink_metadata(ancestor)
                .map_err(|_| "Windrose disposable root cannot be inspected")?;
            require_plain(&metadata)?;
            if !metadata.is_dir() {
                return Err("Windrose disposable root requires plain directories".into());
            }
        }
    }
    let fixture = fixture_root
        .canonicalize()
        .map_err(|_| "cannot resolve Windrose fixture")?;
    let install = install_root
        .canonicalize()
        .map_err(|_| "cannot resolve Windrose runtime")?;
    if install == fixture || !install.starts_with(&fixture) {
        return Err("Windrose probe refuses a runtime outside its disposable fixture".into());
    }
    Ok(install)
}

fn require_plain(metadata: &Metadata) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Windrose probe refuses reparse points".into());
        }
    }
    if metadata.file_type().is_symlink() {
        return Err("Windrose probe refuses symbolic links".into());
    }
    Ok(())
}

fn checked_path(root: &Path, relative: &Path, directory: bool) -> Result<Option<PathBuf>, String> {
    let components = relative.components().collect::<Vec<_>>();
    if components.is_empty()
        || components
            .iter()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("Windrose probe path must remain inside its runtime".into());
    }
    let mut path = root.to_path_buf();
    for (index, component) in components.iter().enumerate() {
        path.push(component.as_os_str());
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("Windrose native path cannot be inspected".into()),
        };
        require_plain(&metadata)?;
        let wants_directory = directory || index + 1 < components.len();
        if (wants_directory && !metadata.is_dir()) || (!wants_directory && !metadata.is_file()) {
            return Err("Windrose native path has an unexpected file type".into());
        }
    }
    Ok(Some(path))
}

fn version_directories(root: &Path) -> Result<Vec<PathBuf>, String> {
    let Some(database) = checked_path(root, Path::new(DATABASE), true)? else {
        return Ok(Vec::new());
    };
    let mut versions = Vec::new();
    for (index, entry) in fs::read_dir(database)
        .map_err(|_| "cannot enumerate Windrose version folders")?
        .enumerate()
    {
        if index >= MAX_VERSION_ENTRIES {
            return Err("Windrose version inventory exceeds its entry limit".into());
        }
        let entry = entry.map_err(|_| "cannot inspect Windrose version folder")?;
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|_| "cannot inspect Windrose version metadata")?;
        require_plain(&metadata)?;
        if metadata.is_dir() {
            versions.push(Path::new(DATABASE).join(entry.file_name()));
        }
    }
    Ok(versions)
}

fn read_document(root: &Path, relative: &Path) -> Result<Option<Value>, String> {
    let Some(path) = checked_path(root, relative, false)? else {
        return Ok(None);
    };
    let mut options = File::options();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("cannot read Windrose native document".into()),
    };
    let before = file
        .metadata()
        .map_err(|_| "cannot inspect Windrose document")?;
    require_plain(&before)?;
    if !before.is_file() || before.len() > MAX_DOCUMENT_BYTES {
        return Err("Windrose native document exceeds its regular-file size limit".into());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_DOCUMENT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read Windrose native document")?;
    let after = file
        .metadata()
        .map_err(|_| "cannot inspect Windrose document after reading")?;
    if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err("Windrose native document exceeds its size limit".into());
    }
    if before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
        || bytes.len() as u64 != after.len()
    {
        return Ok(None);
    }
    // Partial native writes remain pending; neither JSON errors nor identity
    // values may enter diagnostics, since the server document contains secrets.
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    Ok(serde_json::from_slice(bytes).ok())
}

#[cfg(test)]
#[path = "commands_native_windrose_readiness_tests.rs"]
mod tests;
