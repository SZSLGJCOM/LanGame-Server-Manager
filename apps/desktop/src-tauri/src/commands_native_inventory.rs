use super::*;
use std::collections::BTreeMap;
use std::hash::Hasher;
use std::io::Read;

/// Fingerprint the declared program entry files, not mutable native data or
/// every game asset. A missing entry is a failure, never an empty inventory.
pub(super) fn program_files(
    descriptor: &ModuleDescriptor,
    root: &Path,
) -> Result<BTreeMap<PathBuf, (u64, u64)>, Box<dyn std::error::Error>> {
    let mut files = BTreeMap::new();
    let verification = descriptor
        .install
        .as_ref()
        .and_then(|install| install.verification_path.as_deref());
    let executable = descriptor
        .process
        .as_ref()
        .map(|process| process.executable.as_str())
        .filter(|value| !value.contains("{{"));
    for relative in verification.into_iter().chain(executable) {
        let relative = Path::new(relative);
        if relative
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err("native program entry must remain within its installation".into());
        }
        let path = root.join(relative);
        if !path.is_file() {
            return Err("native program entry is missing after creation or maintenance".into());
        }
        let entry = save_inventory(&path)?;
        let value = entry
            .get(Path::new(""))
            .ok_or("native program entry inventory is empty")?;
        files.insert(relative.to_path_buf(), *value);
    }
    if files.is_empty() {
        return Err("native module has no fingerprintable declared program entry".into());
    }
    Ok(files)
}

pub(super) fn save_inventory(
    root: &Path,
) -> Result<BTreeMap<PathBuf, (u64, u64)>, Box<dyn std::error::Error>> {
    let mut files = BTreeMap::new();
    if !root.exists() {
        return Ok(files);
    }
    let mut pending = vec![(root.to_path_buf(), 0_u16)];
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    while let Some((path, depth)) = pending.pop() {
        if depth > 64 {
            return Err("native save inventory depth limit exceeded".into());
        }
        let metadata = fs::symlink_metadata(&path)?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err("native save contains a reparse point".into());
            }
        }
        if metadata.file_type().is_symlink() {
            return Err("native save contains a link".into());
        }
        if metadata.is_dir() {
            for entry in fs::read_dir(&path)? {
                pending.push((entry?.path(), depth + 1));
                if pending.len() > 100_000 {
                    return Err("native save inventory entry limit exceeded".into());
                }
            }
            continue;
        }
        if !metadata.is_file() {
            return Err("native data is not a regular file".into());
        }
        bytes = bytes.saturating_add(metadata.len());
        if files.len() >= 100_000 || bytes > 16 * 1024 * 1024 * 1024 {
            return Err("native save inventory limit exceeded".into());
        }
        let mut file = fs::File::open(&path)?;
        let mut digest = std::collections::hash_map::DefaultHasher::new();
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            digest.write(&buffer[..read]);
        }
        files.insert(
            path.strip_prefix(root)?.to_path_buf(),
            (metadata.len(), digest.finish()),
        );
    }
    Ok(files)
}

#[test]
fn native_inventory_detects_changes_to_retained_files_and_directories() {
    let root = temp_test_dir("native-inventory");
    let config = root.join("Server.ini");
    fs::write(&config, b"ServerGuid=first").unwrap();
    let file_before = save_inventory(&config).unwrap();
    let tree_before = save_inventory(&root).unwrap();
    assert_eq!(file_before.len(), 1);
    assert_eq!(tree_before.len(), 1);
    fs::write(&config, b"ServerGuid=other").unwrap();
    assert_ne!(save_inventory(&config).unwrap(), file_before);
    assert_ne!(save_inventory(&root).unwrap(), tree_before);
    assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_program_inventory_rejects_missing_programs_and_detects_jar_changes() {
    let descriptors = discover_modules(workspace_root().join("modules")).unwrap();
    let descriptor = find_descriptor(&descriptors, "minecraft").unwrap();
    let root = temp_test_dir("native-program-inventory");
    assert!(program_files(descriptor, &root).is_err());
    fs::create_dir_all(root.join("jre/bin")).unwrap();
    fs::write(root.join("jre/bin/java.exe"), b"synthetic runtime").unwrap();
    fs::write(root.join("server.jar"), b"first program").unwrap();
    let before = program_files(descriptor, &root).unwrap();
    assert_eq!(before.len(), 2);
    fs::write(root.join("server.jar"), b"other program").unwrap();
    assert_ne!(program_files(descriptor, &root).unwrap(), before);
    fs::remove_file(root.join("server.jar")).unwrap();
    assert!(program_files(descriptor, &root).is_err());
    fs::remove_dir_all(root).unwrap();
}
