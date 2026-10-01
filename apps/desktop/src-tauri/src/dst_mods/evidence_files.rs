use super::*;

pub(super) fn safe_component(name: &str) -> bool {
    let device = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    !name.is_empty()
        && !matches!(name, "." | "..")
        && !name.ends_with(['.', ' '])
        && !name.chars().any(|ch| {
            ch.is_control() || matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        })
        && !matches!(
            device.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        )
        && !device
            .strip_prefix("COM")
            .or_else(|| device.strip_prefix("LPT"))
            .is_some_and(|suffix| {
                matches!(
                    suffix,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
}

pub(super) fn io_error(error: std::io::Error) -> String {
    format!("Cannot inspect Mod evidence: {error}")
}

pub(super) fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}

pub(super) fn guard_path(path: &Path) -> Result<Option<Vec<fs::File>>, String> {
    if !path.is_absolute()
        || path.components().any(|part| {
            matches!(
                part,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        return Err(String::from(
            "Mod evidence roots must be absolute paths without traversal.",
        ));
    }
    let mut guards = Vec::new();
    for ancestor in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        let metadata = match fs::symlink_metadata(ancestor) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io_error(error)),
        };
        if is_link(&metadata) {
            return Err(String::from(
                "Mod evidence paths cannot cross symlinks or reparse points.",
            ));
        }
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::{
                FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ, FILE_SHARE_WRITE,
            };
            if metadata.is_dir() {
                options
                    .access_mode(FILE_READ_ATTRIBUTES)
                    .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                    .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
            } else {
                options
                    .share_mode(FILE_SHARE_READ)
                    .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
            }
        }
        let file = options.open(ancestor).map_err(io_error)?;
        let opened = file.metadata().map_err(io_error)?;
        if is_link(&opened) || (ancestor != path && !opened.is_dir()) {
            return Err(String::from(
                "Mod evidence paths cannot cross symlinks or reparse points.",
            ));
        }
        guards.push(file);
    }
    Ok(Some(guards))
}

pub(super) fn read_evidence_source(path: &Path, budget: &Cell<usize>) -> Result<String, String> {
    let guards = guard_path(path)?.ok_or("Mod Lua file was not found.")?;
    let file = guards.last().ok_or("Mod Lua file could not be opened.")?;
    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.is_file() || metadata.len() > MAX_MODINFO_BYTES as u64 {
        return Err(String::from(
            "Mod Lua file exceeded the safe file size limit or is not a file.",
        ));
    }
    let mut bytes = Vec::new();
    file.take((MAX_MODINFO_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() > MAX_MODINFO_BYTES {
        return Err(String::from(
            "Mod Lua file exceeded the safe file size limit.",
        ));
    }
    budget.set(
        budget
            .get()
            .checked_sub(bytes.len())
            .ok_or("Mod imports exceeded the total source byte limit.")?,
    );
    Ok(decode_utf8_or_gb18030_text(&bytes)
        .trim_start_matches('\u{feff}')
        .to_owned())
}
