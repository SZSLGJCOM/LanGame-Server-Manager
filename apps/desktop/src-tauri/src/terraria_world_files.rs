use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;

use super::native_file::reparse as is_link_or_reparse;

pub(super) fn world_stem(name: &str) -> Result<&str, String> {
    let invalid = || "世界存档须为保存目录内的一个 .wld 文件名。".to_owned();
    let (stem, extension) = name.rsplit_once('.').ok_or_else(invalid)?;
    if stem.is_empty()
        || !extension.eq_ignore_ascii_case("wld")
        || name.encode_utf16().count() > 255
        || name.chars().any(|ch| {
            ch.is_control() || matches!(ch, '/' | '\\' | ':' | '?' | '*' | '"' | '<' | '>' | '|')
        })
    {
        return Err(invalid());
    }
    let device = stem
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_uppercase();
    if matches!(
        device.as_str(),
        "CON" | "NUL" | "AUX" | "PRN" | "CONIN$" | "CONOUT$"
    ) || device
        .strip_prefix("COM")
        .or_else(|| device.strip_prefix("LPT"))
        .is_some_and(|digit| {
            matches!(
                digit,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    {
        return Err(invalid());
    }
    Ok(stem)
}

pub(super) fn metadata(path: &Path) -> Result<Option<fs::Metadata>, String> {
    match fs::symlink_metadata(path) {
        Ok(value) => Ok(Some(value)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("无法检查 {}：{err}", path.display())),
    }
}

pub(super) fn check_saves_directory(root: &Path, saves: &Path) -> Result<(), String> {
    if saves != root.join("saves") || !root.is_absolute() {
        return Err("无法确认泰拉瑞亚实例的存档目录。".into());
    }
    for path in [root, saves] {
        match metadata(path)? {
            Some(value) if value.is_dir() && !is_link_or_reparse(&value) => {}
            None if path == saves => return Ok(()),
            _ => {
                return Err(format!(
                    "世界存档路径不是可用的普通目录：{}",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn read_world_version(path: &Path) -> Result<Option<i32>, String> {
    let Some(value) = metadata(path)? else {
        return Ok(None);
    };
    if !value.is_file() || is_link_or_reparse(&value) {
        return Err(format!("世界存档不是普通文件：{}", path.display()));
    }
    let mut options = File::options();
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
        .map_err(|err| format!("无法读取世界 {}：{err}", path.display()))?;
    let meta = file
        .metadata()
        .map_err(|err| format!("无法读取世界属性：{err}"))?;
    if !meta.is_file() || is_link_or_reparse(&meta) {
        return Err(format!("世界存档不是普通文件：{}", path.display()));
    }
    // Older desktop worlds predate FileMetadata. Never identify a corrupt byte
    // sequence as a newer world merely because its first four bytes are large.
    // Native validation of the full world remains the game's responsibility.
    let mut header = [0u8; 30];
    file.read_exact(&mut header)
        .map_err(|err| format!("无法读取世界 {} 的文件头：{err}", path.display()))?;
    let version = i32::from_le_bytes([header[0], header[1], header[2], header[3]]);
    if version <= 0 {
        return Err(format!("世界存档 {} 的格式编号无效。", path.display()));
    }
    if version >= 140 {
        let sections = i16::from_le_bytes([header[24], header[25]]);
        let first_section = i32::from_le_bytes([header[26], header[27], header[28], header[29]]);
        if &header[4..12] != b"relogic\x02"
            || sections < 1
            || first_section < 26 + i32::from(sections) * 4
            || first_section as u64 >= meta.len()
        {
            return Err(format!(
                "世界存档 {} 的文件头损坏或不是 Terraria PC 世界。",
                path.display()
            ));
        }
    }
    Ok(Some(version))
}

fn slot_names(saves: &Path, prefix: &str) -> Result<BTreeSet<String>, String> {
    if metadata(saves)?.is_none() {
        return Ok(BTreeSet::new());
    }
    let entries = fs::read_dir(saves).map_err(|err| format!("无法列出世界存档：{err}"))?;
    let mut slots = BTreeSet::new();
    for entry in entries {
        let entry = entry.map_err(|err| format!("无法检查世界存档名称：{err}"))?;
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if !name.starts_with(prefix) {
            continue;
        }
        // Include native .wld, .twld and their entire backup families (.bak,
        // .bak2, etc.). A world name may itself contain dots.
        for (end, _) in name.match_indices('.') {
            let extension = name[end + 1..].split('.').next().unwrap_or_default();
            if matches!(extension, "wld" | "twld") {
                slots.insert(name[..end].to_owned());
            }
        }
    }
    Ok(slots)
}

// BackupIO archives by *file stem* under -tmlsavedirectory/Worlds/Backups.
// Its retention glob matches any dated ZIP containing that stem. A reused
// stem can therefore delete an archive even after its .wld/.twld were removed.
// https://github.com/tModLoader/tModLoader/blob/1.4.4/patches/tModLoader/Terraria/ModLoader/IO/BackupIO.cs
fn matching_archives(instance_root: &Path, stem: &str) -> Result<Vec<String>, String> {
    let mut folder = instance_root.to_path_buf();
    for part in ["tmodloader", "Worlds", "Backups"] {
        folder.push(part);
        let Some(info) = metadata(&folder)? else {
            return Ok(Vec::new());
        };
        if !info.is_dir() || is_link_or_reparse(&info) {
            return Err(format!(
                "tModLoader 的世界备份目录无效：{}",
                folder.display()
            ));
        }
    }
    let mut archived = Vec::new();
    for entry in
        fs::read_dir(folder).map_err(|err| format!("无法检查 tModLoader 的世界备份：{err}"))?
    {
        let entry = entry.map_err(|err| format!("无法读取世界备份名称：{err}"))?;
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if let Some(archive) = name.strip_suffix(".zip")
            && archive.contains(stem)
        {
            archived.push(archive.to_owned());
        }
    }
    Ok(archived)
}

pub(super) fn ensure_slot_empty(
    saves: &Path,
    stem: &str,
    mod_root: Option<&Path>,
) -> Result<(), String> {
    let folded = stem.to_lowercase();
    if metadata(&saves.join(format!("{stem}.wld")))?.is_some()
        || metadata(&saves.join(format!("{stem}.twld")))?.is_some()
        || slot_names(saves, &folded)?.contains(&folded)
        || mod_root
            .map(|root| matching_archives(root, &folded))
            .transpose()?
            .is_some_and(|archives| !archives.is_empty())
    {
        return Err(format!(
            "世界「{stem}」已有存档、模组数据或备份。请选择其他文件名，或先恢复该世界。"
        ));
    }
    Ok(())
}

pub(super) fn suggest_world(
    saves: &Path,
    stem: &str,
    instance_root: &Path,
) -> Result<String, String> {
    // Leave room for the suffix and native sidecar/rollback extensions within
    // one Windows filename, even if the old world had a very long name.
    let mut length = 0;
    let prefix = stem
        .chars()
        .take_while(|ch| {
            length += ch.len_utf16();
            length <= 220
        })
        .collect::<String>();
    let base = format!("{prefix}-mods");
    let folded = base.to_lowercase();
    let slots = slot_names(saves, &folded)?;
    let mut taken = BTreeSet::new();
    for archive in matching_archives(instance_root, &folded)? {
        // Record *all* matching prefixes (e.g. "-2" and "-20" in "-200"),
        // since Terraria's retention glob may match either proposed name.
        for (at, _) in archive.match_indices(&folded) {
            taken.insert(1usize);
            if let Some(rest) = archive[at + folded.len()..].strip_prefix('-') {
                let mut number = 0usize;
                for digit in rest.bytes().take_while(u8::is_ascii_digit) {
                    let Some(next) = number
                        .checked_mul(10)
                        .and_then(|n| n.checked_add(usize::from(digit - b'0')))
                    else {
                        break;
                    };
                    number = next;
                    taken.insert(number);
                }
            }
        }
    }
    for i in 1..=slots.len() + taken.len() + 1 {
        if taken.contains(&i) {
            continue;
        }
        let proposed = if i == 1 {
            base.clone()
        } else {
            format!("{base}-{i}")
        };
        if !slots.contains(&proposed.to_lowercase()) {
            return Ok(format!("{proposed}.wld"));
        }
    }
    Err("未找到可用于新世界的空闲文件名。".into())
}
