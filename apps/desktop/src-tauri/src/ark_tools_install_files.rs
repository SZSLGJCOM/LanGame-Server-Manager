use super::*;

fn reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}

pub(super) fn safe_relative(value: &str) -> bool {
    !value.is_empty()
        && value.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.ends_with(['.', ' '])
                && !part
                    .chars()
                    .any(|c| c.is_control() || "\\:*?\"<>|".contains(c))
                && !matches!(
                    part.split('.')
                        .next()
                        .unwrap_or("")
                        .to_ascii_uppercase()
                        .as_str(),
                    "CON"
                        | "PRN"
                        | "AUX"
                        | "NUL"
                        | "COM1"
                        | "COM2"
                        | "COM3"
                        | "COM4"
                        | "COM5"
                        | "COM6"
                        | "COM7"
                        | "COM8"
                        | "COM9"
                        | "LPT1"
                        | "LPT2"
                        | "LPT3"
                        | "LPT4"
                        | "LPT5"
                        | "LPT6"
                        | "LPT7"
                        | "LPT8"
                        | "LPT9"
                )
        })
}

pub(super) fn pin(path: &Path) -> Result<File, String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| {
        format!(
            "Cannot inspect ARK extension directory {}: {e}",
            path.display()
        )
    })?;
    if !metadata.is_dir() || reparse(&metadata) {
        return Err(format!(
            "ARK extension directories must be ordinary directories: {}",
            path.display()
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // List access enforces the no-delete sharing guard. Metadata-only
        // handles would not stop an ancestor being exchanged for a junction.
        options
            .access_mode(0x1 | 0x80)
            .share_mode(0x1 | 0x2)
            .custom_flags(0x0200_0000 | 0x0020_0000);
    }
    let file = options
        .open(path)
        .map_err(|e| format!("Cannot pin ARK extension directory {}: {e}", path.display()))?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_dir() || reparse(&metadata) {
        return Err("ARK extension directory changed during inspection".into());
    }
    Ok(file)
}

pub(super) fn runtime(root: &Path, executable: &str) -> Result<(PathBuf, Vec<File>), String> {
    let (bin, pins) = runtime_directory(root)?;
    let executable = fs::symlink_metadata(bin.join(executable))
        .map_err(|e| format!("ARK server executable is unavailable: {e}"))?;
    if !executable.is_file() || reparse(&executable) {
        return Err("ARK server executable must be a regular file".into());
    }
    Ok((bin, pins))
}

pub(super) fn runtime_directory(root: &Path) -> Result<(PathBuf, Vec<File>), String> {
    if !root.is_absolute()
        || root
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err("ARK tools require an absolute private runtime path".into());
    }
    let bin = root.join("ShooterGame/Binaries/Win64");
    let mut pins = Vec::new();
    let mut current = PathBuf::new();
    for component in bin.components() {
        current.push(component);
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        pins.push(pin(&current)?);
    }
    Ok((bin, pins))
}

pub(super) fn prepare_parent(
    bin: &Path,
    relative: &str,
    pins: &mut Vec<File>,
    created: &mut Vec<PathBuf>,
) -> Result<(), String> {
    if !safe_relative(relative) {
        return Err("Invalid managed ARK extension path".into());
    }
    let mut current = bin.to_path_buf();
    let parts: Vec<_> = relative.split('/').collect();
    for part in &parts[..parts.len() - 1] {
        current.push(part);
        match fs::create_dir(&current) {
            Ok(()) => created.push(current.clone()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.to_string()),
        }
        pins.push(pin(&current)?);
    }
    Ok(())
}

pub(super) fn remove_created_directories(paths: &[PathBuf]) -> Result<(), String> {
    for path in paths.iter().rev() {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.to_string()),
        };
        if reparse(&metadata) || !metadata.is_dir() {
            return Err("ARK preparation left a changed directory for inspection".into());
        }
        fs::remove_dir(path).map_err(|e| format!("ARK preparation directory was retained: {e}"))?;
    }
    Ok(())
}

pub(super) fn read(bin: &Path, relative: &str) -> Result<Option<Vec<u8>>, String> {
    if !safe_relative(relative) {
        return Err("Invalid ARK extension file path".into());
    }
    let mut pins = Vec::new();
    let mut current = bin.to_path_buf();
    let parts: Vec<_> = relative.split('/').collect();
    for part in &parts[..parts.len() - 1] {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.to_string()),
            Ok(_) => pins.push(pin(&current)?),
        }
    }
    read_regular(&bin.join(relative))
}

pub(super) struct LockedFile {
    file: File,
    pub(super) bytes: Vec<u8>,
    #[cfg(not(windows))]
    path: PathBuf,
}

impl LockedFile {
    pub(super) fn remove(self) -> Result<(), String> {
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Storage::FileSystem::*;
            // The very handle used to verify bytes owns deletion. Denying
            // write/delete sharing closes the path-swap window after hashing.
            let disposition = FILE_DISPOSITION_INFO_EX {
                Flags: FILE_DISPOSITION_FLAG_DELETE | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
            };
            if unsafe {
                SetFileInformationByHandle(
                    self.file.as_raw_handle(),
                    FileDispositionInfoEx,
                    (&disposition as *const FILE_DISPOSITION_INFO_EX).cast(),
                    std::mem::size_of_val(&disposition) as u32,
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error().to_string());
            }
            Ok(())
        }
        #[cfg(not(windows))]
        {
            // The shipped ARK installer is Windows-only. Keep portable fixture
            // builds available without pretending Unix has Windows share locks.
            drop(self.file);
            fs::remove_file(self.path).map_err(|e| e.to_string())
        }
    }
}

pub(super) fn lock_regular(path: &Path, mutate: bool) -> Result<Option<LockedFile>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
        Ok(m) => m,
    };
    if !metadata.is_file() || reparse(&metadata) || metadata.len() > MAX_FILE as u64 {
        return Err(format!(
            "Unsafe or oversized ARK extension file: {}",
            path.display()
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options
            .access_mode(0x8000_0000 | if mutate { 0x0001_0000 } else { 0 })
            .share_mode(0x1)
            .custom_flags(0x0020_0000);
    }
    #[cfg(not(windows))]
    let _ = mutate;
    let mut file = options.open(path).map_err(|e| e.to_string())?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || reparse(&metadata) || metadata.len() > MAX_FILE as u64 {
        return Err("ARK extension file changed during inspection".into());
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take((MAX_FILE + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_FILE {
        return Err("ARK extension file exceeds its read limit".into());
    }
    Ok(Some(LockedFile {
        file,
        bytes,
        #[cfg(not(windows))]
        path: path.to_path_buf(),
    }))
}

fn read_regular(path: &Path) -> Result<Option<Vec<u8>>, String> {
    Ok(lock_regular(path, false)?.map(|file| file.bytes))
}

pub(super) fn remove_hash(path: &Path, hash: &str) -> Result<(), String> {
    if let Some(file) = lock_regular(path, true)? {
        if digest(&file.bytes) != hash {
            return Err(format!(
                "Preserved a concurrently changed ARK file: {}",
                path.display()
            ));
        }
        file.remove()?;
    }
    Ok(())
}

pub(super) fn ownership(
    bin: &Path,
    profile: &Profile,
    wanted: &BTreeMap<String, String>,
) -> Result<Option<Ownership>, String> {
    let Some(bytes) = read(bin, OWNER)? else {
        return Ok(None);
    };
    if bytes.len() > 16 * 1024 {
        return Err("ARK extension ownership record exceeds its limit".into());
    }
    let owned = decode_ownership(&bytes, profile, wanted)?;
    Ok(Some(owned))
}

pub(super) fn decode_ownership(
    bytes: &[u8],
    profile: &Profile,
    wanted: &BTreeMap<String, String>,
) -> Result<Ownership, String> {
    if bytes.len() > 16 * 1024 {
        return Err("ARK extension ownership record exceeds its limit".into());
    }
    let owned: Ownership =
        serde_json::from_slice(bytes).map_err(|_| "ARK extension ownership record is invalid")?;
    if owned.schema != 1
        || owned.module_id != profile.module_id
        || owned.files.keys().ne(wanted.keys())
        || owned
            .files
            .values()
            .any(|h| h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err("ARK extension ownership does not match this module and pinned profile".into());
    }
    for (path, hash) in &owned.files {
        let embedded = matches!(path.as_str(), PLUGIN | INFO | NOTICES)
            || (profile.module_id == ASA.module_id && path == "version.dll");
        if !embedded && wanted.get(path) != Some(hash) {
            return Err(format!(
                "ARK extension dependency or configuration has unknown ownership: {path}"
            ));
        }
    }
    Ok(owned)
}

pub(super) fn preflight(
    bin: &Path,
    profile: &Profile,
    wanted: &BTreeMap<String, String>,
    owned: Option<&Ownership>,
) -> Result<(), String> {
    if owned.is_none() {
        for name in [
            "ArkApi",
            "AsaApi",
            "AsaApiLoader.exe",
            "dwmapi.dll",
            "winmm.dll",
            "UE4SS.dll",
            "ue4ss",
        ] {
            match fs::symlink_metadata(bin.join(name)) {
                Ok(_) => {
                    return Err(format!(
                        "An unmanaged ARK loader already exists: {name}. Its files were preserved."
                    ));
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.to_string()),
            }
        }
    }
    for path in wanted.keys() {
        if let Some(bytes) = read(bin, path)?
            && owned.and_then(|o| o.files.get(path)) != Some(&digest(&bytes))
        {
            return Err(format!(
                "ARK preparation will not overwrite an unknown or modified file: {path}"
            ));
        }
    }
    if profile.module_id == ASA.module_id && read(bin, "msvcp140.dll")?.is_none() {
        return Err("The ASA package is missing its native msvcp140.dll dependency; repair the server installation first".into());
    }
    Ok(())
}

pub(super) fn rollback_file(
    path: &Path,
    backup: &Path,
    before: Option<&[u8]>,
    replacement: &[u8],
) -> Result<(), String> {
    let current = lock_regular(path, true)?;
    if current.as_ref().map(|file| file.bytes.as_slice()) == before {
        return Ok(()); // Also accepts a previous interrupted rollback.
    }
    if current
        .as_ref()
        .is_some_and(|file| file.bytes != replacement)
    {
        return Err(format!(
            "Rollback preserved a concurrently changed file: {}",
            path.display()
        ));
    }
    let original = if let Some(before) = before {
        let original = lock_regular(backup, false)?.ok_or("Rollback backup is missing")?;
        if original.bytes != before {
            return Err("Rollback backup does not match the original ARK extension file".into());
        }
        Some(original)
    } else {
        None
    };
    if let Some(current) = current {
        current.remove()?;
    }
    if original.is_some() {
        fs::hard_link(backup, path).map_err(|e| e.to_string())?;
    }
    Ok(())
}
