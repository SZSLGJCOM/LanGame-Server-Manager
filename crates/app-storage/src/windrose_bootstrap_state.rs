use std::fs::{self, File, Metadata};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use super::{StorageError, WindroseBootstrapObservation, invalid};

pub(super) const SERVER: &str = "R5/ServerDescription.json";
pub(super) const MAX_DOCUMENT: u64 = 64 * 1024;
const DATABASE: &str = "R5/Saved/SaveProfiles/Default/RocksDB_v2";

pub(super) struct NativeState {
    pub observation: WindroseBootstrapObservation,
    pub world_id: Option<String>,
    pub has_worlds: bool,
    pub uninitialized: bool,
}

pub(super) fn inspect(root: &Path) -> Result<NativeState, StorageError> {
    let server = read_bytes(root, Path::new(SERVER), MAX_DOCUMENT)?;
    let document = server.as_deref().and_then(parse);
    let persistent = document.as_ref().and_then(|value| {
        value
            .get("ServerDescription_Persistent")
            .and_then(Value::as_object)
    });
    let text = |key: &str| {
        persistent
            .and_then(|value| value.get(key))
            .and_then(Value::as_str)
    };
    let selected = text("WorldIslandId").filter(|value| !value.is_empty());
    if let Some(selected) = selected
        && (selected.trim() != selected
            || selected.contains(['/', '\\', '\0'])
            || Path::new(selected).components().count() != 1
            || !matches!(
                Path::new(selected).components().next(),
                Some(Component::Normal(_))
            ))
    {
        return Err(invalid(
            root,
            "native world identity is not a safe directory name",
        ));
    }
    let mut observation = WindroseBootstrapObservation {
        world_ready: false,
        use_direct_connection: persistent
            .and_then(|value| value.get("UseDirectConnection"))
            .and_then(Value::as_bool),
        direct_connection_server_port: persistent
            .and_then(|value| value.get("DirectConnectionServerPort"))
            .and_then(Value::as_u64)
            .and_then(|value| u16::try_from(value).ok()),
    };
    let mut has_worlds = false;
    let mut matches = 0;
    let mut matching_document = false;
    let mut world_count = 0;
    if let Some(database) = checked_path(root, Path::new(DATABASE), true)? {
        for (index, version) in fs::read_dir(&database)
            .map_err(|_| invalid(root, "cannot enumerate native version directories"))?
            .enumerate()
        {
            if index >= 64 {
                return Err(invalid(root, "native version directory limit exceeded"));
            }
            let version =
                version.map_err(|_| invalid(root, "cannot inspect native version entry"))?;
            let metadata = fs::symlink_metadata(version.path())
                .map_err(|_| invalid(root, "cannot inspect native version metadata"))?;
            plain(root, &metadata)?;
            if !metadata.is_dir() {
                continue;
            }
            let relative = Path::new(DATABASE).join(version.file_name()).join("Worlds");
            let Some(worlds) = checked_path(root, &relative, true)? else {
                continue;
            };
            for world in fs::read_dir(worlds)
                .map_err(|_| invalid(root, "cannot enumerate native world directories"))?
            {
                world_count += 1;
                if world_count > 256 {
                    return Err(invalid(root, "native world directory limit exceeded"));
                }
                let world =
                    world.map_err(|_| invalid(root, "cannot inspect native world entry"))?;
                let metadata = fs::symlink_metadata(world.path())
                    .map_err(|_| invalid(root, "cannot inspect native world metadata"))?;
                plain(root, &metadata)?;
                // Even an incomplete world directory belongs to the native server.
                has_worlds = true;
                if !metadata.is_dir()
                    || selected.is_none_or(|id| world.file_name().to_str() != Some(id))
                {
                    continue;
                }
                matches += 1;
                let bytes = read_bytes(
                    root,
                    &relative
                        .join(world.file_name())
                        .join("WorldDescription.json"),
                    MAX_DOCUMENT,
                )?;
                matching_document = bytes
                    .as_deref()
                    .and_then(parse)
                    .as_ref()
                    .and_then(|value| value.pointer("/WorldDescription/islandId"))
                    .and_then(Value::as_str)
                    == selected;
            }
        }
    }
    observation.world_ready = selected.is_some()
        && matches == 1
        && matching_document
        && text("PersistentServerId").is_some_and(|value| !value.is_empty());
    let empty_identity = |key| {
        persistent
            .and_then(|value| value.get(key))
            .is_none_or(|value| value.as_str().is_some_and(str::is_empty))
    };
    let uninitialized = server.is_none()
        || (persistent.is_some()
            && empty_identity("PersistentServerId")
            && empty_identity("WorldIslandId"));
    Ok(NativeState {
        world_id: selected
            .filter(|_| observation.world_ready)
            .map(str::to_owned),
        observation,
        has_worlds,
        uninitialized,
    })
}

fn parse(bytes: &[u8]) -> Option<Value> {
    serde_json::from_slice(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes)).ok()
}

pub(super) fn checked_path(
    root: &Path,
    relative: &Path,
    directory: bool,
) -> Result<Option<PathBuf>, StorageError> {
    for ancestor in root.ancestors().filter(|path| !path.as_os_str().is_empty()) {
        let metadata = fs::symlink_metadata(ancestor)
            .map_err(|_| invalid(root, "cannot inspect managed runtime ancestors"))?;
        plain(root, &metadata)?;
        if !metadata.is_dir() {
            return Err(invalid(root, "managed runtime ancestor is not a directory"));
        }
    }
    let components = relative.components().collect::<Vec<_>>();
    if components.is_empty()
        || components
            .iter()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(invalid(
            root,
            "bootstrap path must remain below its managed root",
        ));
    }
    let mut path = root.to_owned();
    for (index, component) in components.iter().enumerate() {
        path.push(component.as_os_str());
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(invalid(root, "cannot inspect bootstrap path")),
        };
        plain(root, &metadata)?;
        if metadata.is_dir() != (directory || index + 1 < components.len())
            || (!metadata.is_dir() && !metadata.is_file())
        {
            return Err(invalid(root, "bootstrap path has an unexpected type"));
        }
    }
    Ok(Some(path))
}

fn plain(root: &Path, metadata: &Metadata) -> Result<(), StorageError> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(invalid(root, "bootstrap refuses reparse points"));
        }
    }
    if metadata.file_type().is_symlink() {
        return Err(invalid(root, "bootstrap refuses symbolic links"));
    }
    Ok(())
}

pub(super) fn read_bytes(
    root: &Path,
    relative: &Path,
    limit: u64,
) -> Result<Option<Vec<u8>>, StorageError> {
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
    let mut file = options
        .open(&path)
        .map_err(|_| invalid(root, "cannot open bootstrap document"))?;
    let metadata = file
        .metadata()
        .map_err(|_| invalid(root, "cannot inspect bootstrap document"))?;
    plain(root, &metadata)?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(invalid(
            root,
            "bootstrap document exceeds its regular-file size limit",
        ));
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid(root, "cannot read bootstrap document"))?;
    if bytes.len() as u64 > limit {
        return Err(invalid(root, "bootstrap document exceeds its size limit"));
    }
    Ok(Some(bytes))
}
