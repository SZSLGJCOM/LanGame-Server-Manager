use std::fs;
use std::io::Read;
use std::path::Path;

#[derive(Default)]
pub(super) enum PackageNames {
    #[default]
    Empty,
    Named(String),
    Ambiguous,
}

impl PackageNames {
    pub(super) fn inspect(&mut self, path: &Path) -> Result<(), String> {
        if matches!(self, Self::Ambiguous)
            || !path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("tmod"))
        {
            return Ok(());
        }
        let candidate = read_name(path)?;
        // ModOrganizer reads all generations recursively, then groups by the
        // internal Name. One Workshop row can only control one such group.
        *self = match (&*self, candidate) {
            (Self::Empty, Some(name)) => Self::Named(name),
            (Self::Named(previous), Some(name)) if previous == &name => return Ok(()),
            _ => Self::Ambiguous,
        };
        Ok(())
    }

    pub(super) fn unique(self) -> Option<String> {
        match self {
            Self::Named(name) => Some(name),
            _ => None,
        }
    }
}

fn read_name(path: &Path) -> Result<Option<String>, String> {
    let parent = path
        .parent()
        .ok_or("Workshop package has no parent directory")?;
    let ancestors: Vec<_> = parent
        .ancestors()
        .filter(|p| !p.as_os_str().is_empty())
        .collect();
    let _pins = ancestors
        .into_iter()
        .rev()
        .map(crate::commands::commands_managed_save_control::io::pin_directory)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            format!(
                "Cannot pin tModLoader package directory {}: {error}",
                parent.display()
            )
        })?;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        };
        options
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .share_mode(FILE_SHARE_READ);
    }
    let mut file = options
        .open(path)
        .map_err(|error| format!("Cannot open tModLoader package {}: {error}", path.display()))?;
    let metadata = file.metadata().map_err(|error| {
        format!(
            "Cannot inspect tModLoader package {}: {error}",
            path.display()
        )
    })?;
    super::reject_reparse(path, &metadata)?;
    if !metadata.is_file() {
        return Err(format!(
            "tModLoader package {} is not a regular file",
            path.display()
        ));
    }
    // Large Workshop packages need only their prefix for an inventory label.
    let mut prefix = [0u8; 4096];
    let length = metadata.len().min(prefix.len() as u64) as usize;
    file.read_exact(&mut prefix[..length]).map_err(|error| {
        format!(
            "Cannot read tModLoader package header {}: {error}",
            path.display()
        )
    })?;
    Ok(header_name(&prefix[..length], metadata.len()).map(str::to_owned))
}

// tModLoader v2026.09.3.1, Core/TmodFile.cs Read(): BinaryReader strings,
// SHA-1, 256-byte signature, signed little-endian remaining length, then Name.
// Pre-0.11 files use a compressed layout and cannot supply this modern header.
fn header_name(prefix: &[u8], file_size: u64) -> Option<&str> {
    let mut data = prefix.strip_prefix(b"TMOD")?;
    if version(read_string(&mut data)?)? < (0, 11) {
        return None;
    }
    take(&mut data, 20 + 256)?;
    let remaining = i32::from_le_bytes(take(&mut data, 4)?.try_into().ok()?);
    let payload_start = (prefix.len() - data.len()) as u64;
    if u64::try_from(remaining).ok()? != file_size.checked_sub(payload_start)? {
        return None;
    }
    let name = read_string(&mut data)?;
    version(read_string(&mut data)?)?;
    let count = i32::from_le_bytes(take(&mut data, 4)?.try_into().ok()?);
    let table_start = (prefix.len() - data.len()) as u64;
    if count <= 0 || u64::try_from(count).ok()? * 9 > file_size.checked_sub(table_start)? {
        return None;
    }
    // VerifyNames rejects empty, dotted and reserved Terraria names. Only
    // expose names the one-name-per-line enabled setting can round-trip.
    (!name.is_empty()
        && name.trim() == name
        && !name.eq_ignore_ascii_case("Terraria")
        && !name.contains(['.', '/', '\\'])
        && !name.chars().any(char::is_control)
        && !name.starts_with('#')
        && !name.starts_with("--"))
    .then_some(name)
}

fn take<'a>(data: &mut &'a [u8], length: usize) -> Option<&'a [u8]> {
    let (value, rest) = data.split_at_checked(length)?;
    *data = rest;
    Some(value)
}

fn read_string<'a>(data: &mut &'a [u8]) -> Option<&'a str> {
    let mut length = 0u32;
    for shift in (0..=28).step_by(7) {
        let (&byte, rest) = data.split_first()?;
        *data = rest;
        if shift == 28 && byte > 7 {
            return None;
        }
        length |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return std::str::from_utf8(take(data, length as usize)?).ok();
        }
    }
    None
}

fn version(value: &str) -> Option<(i32, i32)> {
    let mut components = [0; 4];
    let mut count = 0;
    for part in value.split('.') {
        if !part.bytes().all(|digit| digit.is_ascii_digit()) {
            return None;
        }
        *components.get_mut(count)? = part.parse::<i32>().ok()?;
        count += 1;
    }
    (count >= 2).then_some((components[0], components[1]))
}
