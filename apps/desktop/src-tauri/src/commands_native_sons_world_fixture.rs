//! Explicit task-checkpoint seed for the disposable native acceptance only.
//! The game loads this unmodified archive normally; no native flag is changed.
use super::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use std::path::Component;

const MAX_ARCHIVE: usize = 64 * 1024 * 1024;
const MAX_UNPACKED: u64 = 128 * 1024 * 1024;
const SLOT: &str = "DedicatedServer/Multiplayer/0000000001/SaveData.zip";

pub(super) async fn prepare<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    details: &InstanceDetails,
) -> Result<(), Box<dyn std::error::Error>> {
    if details.summary.module_id != "sonsoftheforest"
        || std::env::var("LANGAME_NATIVE_MANUAL_SAVE").as_deref() != Ok("true")
    {
        return Ok(());
    }
    let source = std::env::var_os("LANGAME_NATIVE_SONS_WORLD_ZIP");
    let expected = std::env::var_os("LANGAME_NATIVE_SONS_WORLD_ZIP_SHA256");
    let (source, expected) = match (source, expected) {
        (None, None) => return Ok(()),
        (Some(source), Some(expected)) => (PathBuf::from(source), expected),
        _ => return Err("Sons task checkpoint requires both archive and SHA256 opt-ins".into()),
    };
    let expected = expected
        .to_str()
        .filter(|value| value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("Sons task checkpoint SHA256 is invalid")?
        .to_ascii_lowercase();
    if details.active_run.is_some() {
        return Err("Sons task checkpoint requires a fresh stopped fixture".into());
    }
    let _operation = runtime
        .state
        .begin_storage_context_operation("isolated Sons task checkpoint seed")?;
    let fixture = runtime.package.root.clone();
    let config = PathBuf::from(&details.config_file_path);
    let saves = PathBuf::from(&details.saves_path);
    tokio::task::spawn_blocking(move || seed(&fixture, &config, &saves, &source, &expected))
        .await
        .map_err(|_| "Sons task checkpoint worker failed")??;
    println!(
        "NATIVE_SONS_WORLD source=task_checkpoint slot=1 archive_verified=true fresh_world_generation=not_claimed"
    );
    Ok(())
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn ordinary(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    !metadata.file_type().is_symlink()
}

fn local_absolute(path: &Path) -> bool {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return false;
    }
    #[cfg(windows)]
    {
        use std::path::Prefix;
        let Some(Component::Prefix(prefix)) = path.components().next() else {
            return false;
        };
        if !matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)) {
            return false;
        }
        let Some(text) = path.to_str() else {
            return false;
        };
        let offset = if text.starts_with(r"\\?\") { 6 } else { 2 };
        if text.get(offset..).is_none_or(|tail| tail.contains(':')) {
            return false;
        }
    }
    true
}

fn pin_directory(path: &Path) -> Result<fs::File, String> {
    let before = fs::symlink_metadata(path).map_err(|_| "Cannot inspect checkpoint directory")?;
    if !before.is_dir() || !ordinary(&before) {
        return Err("Checkpoint directory is redirected".into());
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options
            .access_mode(0x1)
            .share_mode(0x1 | 0x2)
            .custom_flags(0x0200_0000 | 0x0020_0000);
    }
    let file = options
        .open(path)
        .map_err(|_| "Cannot pin checkpoint directory")?;
    let metadata = file
        .metadata()
        .map_err(|_| "Cannot inspect pinned checkpoint directory")?;
    if !metadata.is_dir() || !ordinary(&metadata) {
        return Err("Checkpoint directory changed".into());
    }
    Ok(file)
}

fn pin_ancestors(path: &Path) -> Result<Vec<fs::File>, String> {
    if !local_absolute(path) {
        return Err("Checkpoint requires local absolute paths".into());
    }
    path.ancestors()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(pin_directory)
        .collect()
}

fn read_regular(path: &Path, limit: usize) -> Result<(fs::File, Vec<u8>), String> {
    let before = fs::symlink_metadata(path).map_err(|_| "Checkpoint file is unavailable")?;
    if !before.is_file() || !ordinary(&before) || before.len() == 0 || before.len() > limit as u64 {
        return Err("Checkpoint file must be ordinary and bounded".into());
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000).share_mode(0x1);
    }
    let mut file = options
        .open(path)
        .map_err(|_| "Cannot pin checkpoint file")?;
    let metadata = file
        .metadata()
        .map_err(|_| "Cannot inspect pinned checkpoint file")?;
    if !metadata.is_file() || !ordinary(&metadata) || metadata.len() > limit as u64 {
        return Err("Checkpoint file changed".into());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read checkpoint file")?;
    if bytes.is_empty() || bytes.len() > limit {
        return Err("Checkpoint file exceeds its bound".into());
    }
    Ok((file, bytes))
}

pub(super) fn validate_archive(bytes: &[u8], expected: &str) -> Result<(), String> {
    if bytes.len() > MAX_ARCHIVE || digest(bytes) != expected {
        return Err("Sons task checkpoint archive identity changed".into());
    }
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| "Invalid checkpoint ZIP")?;
    if archive.is_empty() || archive.len() > 256 {
        return Err("Checkpoint ZIP entry count is invalid".into());
    }
    let mut names = BTreeSet::new();
    let mut total = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| "Cannot inspect checkpoint ZIP entry")?;
        let name = entry
            .name()
            .map_err(|_| "Invalid checkpoint ZIP name")?
            .into_owned();
        if name.contains(['\\', ':'])
            || name.chars().any(char::is_control)
            || entry.enclosed_name().is_none()
            || !names.insert(name.to_ascii_lowercase())
            || entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("Unsafe checkpoint ZIP entry".into());
        }
        total = total
            .checked_add(entry.size())
            .ok_or("Checkpoint ZIP expanded size overflow")?;
        if entry.size() > MAX_ARCHIVE as u64 || total > MAX_UNPACKED {
            return Err("Checkpoint ZIP exceeds its expanded bound".into());
        }
        let size = entry.size();
        let actual = std::io::copy(&mut (&mut entry).take(size + 1), &mut std::io::sink())
            .map_err(|_| "Checkpoint ZIP failed decompression or CRC validation")?;
        if actual != size {
            return Err("Checkpoint ZIP entry size changed".into());
        }
    }
    Ok(())
}

fn seed(
    fixture: &Path,
    config: &Path,
    saves: &Path,
    source: &Path,
    expected: &str,
) -> Result<(), String> {
    let _fixture_pins = pin_ancestors(fixture)?;
    let fixture = dunce::canonicalize(fixture).map_err(|_| "Cannot resolve checkpoint fixture")?;
    // InstanceDetails points at config/instance.json, whereas native settings
    // are materialized under the sibling data root used by -userdatapath.
    let config_parent = config.parent().ok_or("Checkpoint has no config parent")?;
    let _config_pins = pin_ancestors(config_parent)?;
    let metadata_root = dunce::canonicalize(config_parent)
        .map_err(|_| "Cannot resolve checkpoint metadata root")?;
    let instance_root = metadata_root
        .parent()
        .ok_or("Checkpoint has no instance root")?;
    if config.file_name().and_then(std::ffi::OsStr::to_str) != Some("instance.json")
        || metadata_root.file_name().and_then(std::ffi::OsStr::to_str) != Some("config")
        || instance_root == fixture
        || !instance_root.starts_with(&fixture)
    {
        return Err("Checkpoint metadata is not the fixture instance configuration".into());
    }
    let (_metadata_file, metadata_bytes) = read_regular(config, 1024 * 1024)?;
    if !serde_json::from_slice::<Value>(&metadata_bytes)
        .map_err(|_| "Invalid fixture instance metadata")?
        .is_object()
    {
        return Err("Invalid fixture instance metadata".into());
    }
    let data_path = saves.parent().ok_or("Checkpoint has no native data root")?;
    let _data_pins = pin_ancestors(data_path)?;
    let data = dunce::canonicalize(data_path).map_err(|_| "Cannot resolve checkpoint data root")?;
    let expected_saves = data.join("Saves");
    if data == fixture
        || !data.starts_with(&fixture)
        || data != instance_root.join("data")
        || !local_absolute(saves)
        || dunce::simplified(saves) != expected_saves
    {
        return Err("Sons checkpoint destination escaped the fresh fixture".into());
    }
    let (_config_file, bytes) = read_regular(&data.join("dedicatedserver.cfg"), 1024 * 1024)?;
    let settings: Value =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid Sons fixture configuration")?;
    if settings.get("SaveMode").and_then(Value::as_str) != Some("Continue")
        || settings.get("SaveSlot").and_then(Value::as_u64) != Some(1)
    {
        return Err("Sons task checkpoint requires Continue and SaveSlot 1".into());
    }
    let source_parent = source.parent().ok_or("Checkpoint source has no parent")?;
    let _source_pins = pin_ancestors(source_parent)?;
    if dunce::canonicalize(source)
        .map_err(|_| "Cannot resolve checkpoint source")?
        .starts_with(&fixture)
    {
        return Err("Checkpoint source must be outside its fresh destination fixture".into());
    }
    let (_source_file, bytes) = read_regular(source, MAX_ARCHIVE)?;
    validate_archive(&bytes, expected)?;
    let mut pins = Vec::new();
    let mut directory = data.clone();
    for component in Path::new("Saves").components() {
        directory.push(component);
        match fs::create_dir(&directory) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err("Cannot create checkpoint save directory".into()),
        }
        pins.push(pin_directory(&directory)?);
    }
    if fs::read_dir(&directory)
        .map_err(|_| "Cannot inspect fresh checkpoint saves")?
        .next()
        .is_some()
    {
        return Err("Sons task checkpoint refuses existing fixture world data".into());
    }
    let destination = saves.join(SLOT);
    for component in Path::new(SLOT)
        .parent()
        .ok_or("Invalid checkpoint slot")?
        .components()
    {
        if !matches!(component, Component::Normal(_)) {
            return Err("Invalid checkpoint slot component".into());
        }
        directory.push(component);
        fs::create_dir(&directory)
            .map_err(|_| "Checkpoint slot already exists or cannot be created")?;
        pins.push(pin_directory(&directory)?);
    }
    let mut options = fs::OpenOptions::new();
    options.create_new(true).read(true).write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000).share_mode(0x1);
    }
    let mut file = options
        .open(destination)
        .map_err(|_| "Checkpoint destination already exists or cannot be created")?;
    file.write_all(&bytes)
        .map_err(|_| "Cannot copy task checkpoint")?;
    file.sync_all()
        .map_err(|_| "Cannot flush task checkpoint")?;
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "Cannot reread task checkpoint")?;
    let mut copied = Vec::new();
    file.take(MAX_ARCHIVE as u64 + 1)
        .read_to_end(&mut copied)
        .map_err(|_| "Cannot verify copied task checkpoint")?;
    if copied.len() != bytes.len() || digest(&copied) != expected {
        return Err("Copied task checkpoint identity changed".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zip(name: &str) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(b"synthetic boundary fixture, not a game world")
            .unwrap();
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn sons_task_checkpoint_rejects_changed_or_unsafe_archive() {
        let bytes = zip("World.json");
        assert!(validate_archive(&bytes, &digest(&bytes)).is_ok());
        assert!(validate_archive(&bytes, &"0".repeat(64)).is_err());
        let data_start = zip::ZipArchive::new(Cursor::new(&bytes))
            .unwrap()
            .by_index(0)
            .unwrap()
            .data_start()
            .expect("the generated fixture has an ordinary file entry")
            as usize;
        let mut corrupt = bytes.clone();
        corrupt[data_start] ^= 0x80;
        assert!(validate_archive(&corrupt, &digest(&corrupt)).is_err());
        for name in ["../foreign.json", "folder\\foreign.json", "file:stream"] {
            let bytes = zip(name);
            assert!(validate_archive(&bytes, &digest(&bytes)).is_err());
        }
    }

    #[test]
    fn sons_task_checkpoint_copies_once_and_preserves_existing_world() {
        let root =
            std::env::temp_dir().join(format!("sons_checkpoint_{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir(&root).unwrap();
        let fixture = root.join("fixture");
        let instance = fixture.join("instance");
        let config_root = instance.join("config");
        let data = instance.join("data");
        fs::create_dir(&fixture).unwrap();
        fs::create_dir(&instance).unwrap();
        fs::create_dir(&config_root).unwrap();
        fs::create_dir(&data).unwrap();
        let metadata = config_root.join("instance.json");
        fs::write(&metadata, b"{}").unwrap();
        let config = data.join("dedicatedserver.cfg");
        fs::write(&config, br#"{"SaveMode":"Continue","SaveSlot":1}"#).unwrap();
        let source = root.join("task.zip");
        let bytes = zip("World.json");
        fs::write(&source, &bytes).unwrap();
        let saves = data.join("Saves");
        assert!(seed(&fixture, &config, &saves, &source, &digest(&bytes)).is_err());
        for invalid in [
            br#"{"SaveMode":"New","SaveSlot":1}"#.as_slice(),
            br#"{"SaveMode":"Continue","SaveSlot":2}"#.as_slice(),
        ] {
            fs::write(&config, invalid).unwrap();
            assert!(seed(&fixture, &metadata, &saves, &source, &digest(&bytes)).is_err());
            assert!(!saves.exists());
        }
        fs::write(&config, br#"{"SaveMode":"Continue","SaveSlot":1}"#).unwrap();
        assert!(seed(&fixture, &metadata, &saves, &source, &digest(&bytes)).is_ok());
        let destination = saves.join(SLOT);
        assert_eq!(fs::read(&destination).unwrap(), bytes);
        assert!(seed(&fixture, &metadata, &saves, &source, &digest(&bytes)).is_err());
        assert_eq!(fs::read(&destination).unwrap(), bytes);
        fs::remove_file(destination).unwrap();
        for part in [
            "Saves/DedicatedServer/Multiplayer/0000000001",
            "Saves/DedicatedServer/Multiplayer",
            "Saves/DedicatedServer",
            "Saves",
        ] {
            fs::remove_dir(data.join(part)).unwrap();
        }
        fs::remove_file(config).unwrap();
        fs::remove_file(metadata).unwrap();
        fs::remove_file(source).unwrap();
        fs::remove_dir(data).unwrap();
        fs::remove_dir(config_root).unwrap();
        fs::remove_dir(instance).unwrap();
        fs::remove_dir(fixture).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
