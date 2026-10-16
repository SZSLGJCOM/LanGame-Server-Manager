//! Check physical, caller-available space on the volume that receives Mod bytes.
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const CAPACITY_RESERVE: u64 = 512 * 1024 * 1024;
const RECHECK_BYTES: u64 = 16 * 1024 * 1024;
const COPY_BUFFER_BYTES: usize = 64 * 1024;

fn check_capacity(available: u64, additional_bytes: u64) -> Result<(), String> {
    let required = additional_bytes.checked_add(CAPACITY_RESERVE).ok_or_else(|| {
        format!(
            "Mod write requires {additional_bytes} additional bytes plus 512 MiB headroom, exceeding capacity accounting; {available} bytes are available. Use a smaller package or another output volume."
        )
    })?;
    if available < required {
        return Err(format!(
            "Mod write requires {required} free bytes including 512 MiB headroom; {available} bytes are available. Free space or choose another output volume."
        ));
    }
    Ok(())
}

pub(in crate::commands) fn require_mod_capacity(
    path: &Path,
    additional_bytes: u64,
) -> Result<(), String> {
    check_capacity(available_bytes(path)?, additional_bytes)
}

pub(in crate::commands) struct ModSpaceBudget {
    path: PathBuf,
    available: u64,
    written_since_probe: u64,
}

impl ModSpaceBudget {
    pub(in crate::commands) fn new(path: &Path) -> Result<Self, String> {
        let available = available_bytes(path)?;
        check_capacity(available, 0)?;
        Ok(Self {
            path: path.to_path_buf(),
            available,
            written_since_probe: 0,
        })
    }

    /// Call immediately before each write. A write crossing the 16 MiB probe
    /// interval gets a fresh reading before its entire size is reserved.
    pub(in crate::commands) fn before_write(&mut self, bytes: usize) -> Result<(), String> {
        self.reserve_with(bytes, available_bytes)
    }

    fn reserve_with(
        &mut self,
        bytes: usize,
        probe: impl FnOnce(&Path) -> Result<u64, String>,
    ) -> Result<(), String> {
        let bytes =
            u64::try_from(bytes).map_err(|_| "Mod write size exceeds capacity accounting")?;
        if bytes == 0 {
            return Ok(());
        }
        if bytes >= RECHECK_BYTES || self.written_since_probe >= RECHECK_BYTES.saturating_sub(bytes)
        {
            self.available = probe(&self.path)?;
            self.written_since_probe = 0;
        }
        check_capacity(self.available, bytes)?;
        self.available -= bytes;
        self.written_since_probe = self
            .written_since_probe
            .checked_add(bytes)
            .ok_or("Mod write size exceeds capacity accounting")?;
        Ok(())
    }
}

#[cfg(windows)]
fn nearest_plain_directory(path: &Path) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty() {
        return Err("Mod output path is empty".into());
    }
    let absolute = std::path::absolute(path)
        .map_err(|error| format!("Cannot resolve Mod output location: {error}"))?;
    if absolute
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err("Mod output path contains parent traversal".into());
    }
    for ancestor in absolute.ancestors() {
        let metadata = match fs::symlink_metadata(ancestor) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("Cannot inspect Mod output location: {error}")),
        };
        ensure_plain_entry(&metadata)?;
        if !metadata.is_dir() {
            if ancestor == absolute && metadata.is_file() {
                continue;
            }
            return Err("Mod output parent is not a regular directory".into());
        }
        for parent in ancestor.ancestors() {
            let metadata = fs::symlink_metadata(parent)
                .map_err(|error| format!("Cannot inspect Mod output ancestor: {error}"))?;
            ensure_plain_entry(&metadata)?;
            if !metadata.is_dir() {
                return Err("Mod output ancestor is not a regular directory".into());
            }
        }
        return fs::canonicalize(ancestor)
            .map_err(|error| format!("Cannot resolve Mod output directory: {error}"));
    }
    Err("Mod output location has no existing regular directory ancestor".into())
}

#[cfg(windows)]
fn ensure_plain_entry(metadata: &fs::Metadata) -> Result<(), String> {
    use std::os::windows::fs::MetadataExt;
    if metadata.file_type().is_symlink() || metadata.file_attributes() & 0x0000_0400 != 0 {
        return Err("Mod output location contains a symbolic link or reparse point".into());
    }
    Ok(())
}

#[cfg(windows)]
fn available_bytes(path: &Path) -> Result<u64, String> {
    use std::os::windows::ffi::OsStrExt;
    let canonical = nearest_plain_directory(path)?;
    let mut path = canonical.as_os_str().encode_wide().collect::<Vec<_>>();
    if path.contains(&0) {
        return Err("Mod output directory contains an invalid NUL character".into());
    }
    // Canonical paths preserve extended-length UTF-16 and UNC prefixes. UNC
    // shares require a trailing separator for GetDiskFreeSpaceExW.
    if path.last() != Some(&(b'\\' as u16)) {
        path.push(b'\\' as u16);
    }
    path.push(0);
    let mut available = 0;
    // The UTF-16 input and output remain valid for the duration of this call.
    // The first output respects the current user's quota on this volume.
    let success = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            path.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if success == 0 {
        return Err(format!(
            "Cannot query available space for Mod output: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(available)
}

#[cfg(not(windows))]
fn available_bytes(_path: &Path) -> Result<u64, String> {
    Err("Mod disk capacity verification requires the supported Windows host".into())
}

pub(in crate::commands) fn copy_mod_payload(
    source: &Path,
    destination: &Path,
) -> Result<u64, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Also prevents truncating the source through a same-path or hard-link
        // destination, without relying on path spelling to establish identity.
        options.share_mode(0x1);
    }
    let mut input = options
        .open(source)
        .map_err(|error| format!("Cannot open Mod payload source: {error}"))?;
    let metadata = input
        .metadata()
        .map_err(|error| format!("Cannot inspect Mod payload source: {error}"))?;
    if !metadata.is_file() {
        return Err("Mod payload source is not a regular file".into());
    }
    let mut budget = ModSpaceBudget::new(destination)?;
    check_capacity(budget.available, metadata.len())?;
    let mut output = File::options()
        .write(true)
        .create(true)
        .truncate(true)
        .open(destination)
        .map_err(|error| format!("Cannot open Mod payload destination: {error}"))?;
    let mut buffer = [0; COPY_BUFFER_BYTES];
    let mut copied = 0u64;
    loop {
        let read = input
            .read(&mut buffer)
            .map_err(|error| format!("Cannot read Mod payload: {error}"))?;
        if read == 0 {
            break;
        }
        budget.before_write(read)?;
        output
            .write_all(&buffer[..read])
            .map_err(|error| format!("Cannot write Mod payload: {error}"))?;
        copied = copied
            .checked_add(read as u64)
            .ok_or("Mod payload copy size exceeds capacity accounting")?;
    }
    output
        .set_permissions(metadata.permissions())
        .map_err(|error| format!("Cannot preserve Mod payload permissions: {error}"))?;
    Ok(copied)
}

#[cfg(test)]
#[path = "commands_mod_capacity_tests.rs"]
mod tests;
