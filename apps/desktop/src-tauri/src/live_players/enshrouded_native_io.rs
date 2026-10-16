//! Bounded local exchange with the instance-owned Enshrouded extension.
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};

pub(crate) const EXE_SHA256: &str =
    "4de2acdd3b18e562f129bc488a2daea2da16b00341497304423e1e18c49a83ba";
pub(crate) const DIRECTORY: &str = ".langame-enshrouded-control";
pub(crate) const OWNER: &str = ".langame-enshrouded-control-owner.json";
pub(crate) const ENABLE: &[u8] = b"LGSM_ENSHROUDED_CONTROL_1\n";
pub(crate) const MAX_BYTES: usize = 64 * 1024;

pub(crate) fn bundled_payload() -> &'static [u8] {
    #[cfg(windows)]
    {
        include_bytes!(concat!(env!("OUT_DIR"), "/enshrouded-control/dbghelp.dll"))
    }
    #[cfg(not(windows))]
    {
        &[]
    }
}

pub(crate) fn reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}

pub(crate) fn pin_directory(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
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
        .map_err(|_| "Cannot pin the native control directory.")?;
    let metadata = file
        .metadata()
        .map_err(|_| "Cannot inspect the native control directory.")?;
    if !metadata.is_dir() || reparse(&metadata) {
        return Err("Native control requires an ordinary directory.".into());
    }
    Ok(file)
}

pub(crate) fn read_regular(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000);
    }
    let file = options
        .open(path)
        .map_err(|_| "The native control file is unavailable.")?;
    let metadata = file
        .metadata()
        .map_err(|_| "Cannot inspect the native control file.")?;
    if !metadata.is_file() || reparse(&metadata) || metadata.len() > limit as u64 {
        return Err("Native control requires a bounded ordinary file.".into());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read the native control file.")?;
    if bytes.len() > limit {
        return Err("The native control file exceeded its read budget.".into());
    }
    Ok(bytes)
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) fn publish(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Ok(metadata) = fs::symlink_metadata(path)
        && (!metadata.is_file() || reparse(&metadata))
    {
        return Err("Refusing to replace a nonordinary native control file.".into());
    }
    let parent = path
        .parent()
        .ok_or("Native control destination has no parent.")?;
    let temporary = parent.join(format!(".{}.pending", uuid::Uuid::new_v4().simple()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|_| "Cannot create the native control temporary file.")?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|_| "Cannot atomically publish the native control file.".into())
}
