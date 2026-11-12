//! Resolve unloaded official creature classes from this owned installation's
//! package manifest. No bundled path guesses, asset loading, or persistent cache.
use std::fs::{self, Metadata, OpenOptions};
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

const MANIFEST: &str = "Manifest_UFSFiles_Win64.txt";
// The verified ASA build 26564816 manifest is 25,690,021 bytes; the longest
// observed row is 220 bytes. Keep generous, explicit bounds for later builds.
const MAX_MANIFEST_BYTES: u64 = 64 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 4096;

pub(super) fn resolve(root: &Path, creature: &str) -> Result<String, String> {
    // The caller validates SpawnInput before resolution and validates the
    // resolved input again. Explicit paths must retain their existing behavior.
    if creature.starts_with("/Game/") {
        return Ok(creature.into());
    }
    let short = creature
        .strip_suffix("_C")
        .filter(|name| {
            !name.is_empty()
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        })
        .ok_or("生物类名无效，生成请求未发送。")?;
    if !root.is_absolute() {
        return Err("方舟程序目录必须是已确认的绝对路径。".into());
    }
    let path = root.join(MANIFEST);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(creature.into()),
        Err(_) => return Err("无法检查方舟资源清单，生成请求未发送。".into()),
    };
    if !plain_file(&metadata) {
        return Err("方舟资源清单必须是普通文件，不能是链接或重解析点。".into());
    }
    if metadata.len() == 0 || metadata.len() > MAX_MANIFEST_BYTES {
        return Err("方舟资源清单为空或超过 64 MiB，生成请求未发送。".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Inspect the opened object itself instead of following a link created
        // between the path check and open. The caller owns the resolved root.
        options.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let file = options
        .open(path)
        .map_err(|_| "无法读取方舟资源清单，生成请求未发送。")?;
    let before = file
        .metadata()
        .map_err(|_| "无法检查已打开的方舟资源清单。")?;
    if !plain_file(&before) || before.len() == 0 || before.len() > MAX_MANIFEST_BYTES {
        return Err("已打开的方舟资源清单不是受支持的普通文件。".into());
    }
    let (resolved, bytes) = scan(&file, short, MAX_MANIFEST_BYTES)?;
    let after = file
        .metadata()
        .map_err(|_| "无法确认方舟资源清单读取结果。")?;
    if bytes != before.len()
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err("读取期间方舟资源清单发生变化，生成请求未发送。".into());
    }
    // A mod or an already loaded class need not appear in the official manifest.
    Ok(resolved.unwrap_or_else(|| creature.into()))
}

fn plain_file(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.is_file() && metadata.file_attributes() & 0x400 == 0
    }
    #[cfg(not(windows))]
    {
        metadata.is_file() && !metadata.file_type().is_symlink()
    }
}

fn scan(reader: impl Read, short: &str, maximum: u64) -> Result<(Option<String>, u64), String> {
    let mut reader = BufReader::new(reader.take(maximum + 1));
    let mut line = Vec::with_capacity(512);
    let mut total = 0;
    let mut rows = 0;
    let mut resolved = None;
    loop {
        line.clear();
        // Taking each line separately bounds read_until's allocation even if a
        // corrupt file has no newline. The outer Take also bounds total reads.
        let bytes = reader
            .by_ref()
            .take((MAX_LINE_BYTES + 1) as u64)
            .read_until(b'\n', &mut line)
            .map_err(|_| "读取方舟资源清单失败，生成请求未发送。")?;
        if bytes == 0 {
            break;
        }
        total += bytes as u64;
        if total > maximum || bytes > MAX_LINE_BYTES {
            return Err("方舟资源清单或单行超过读取上限，生成请求未发送。".into());
        }
        let text = std::str::from_utf8(&line)
            .map_err(|_| "方舟资源清单不是有效 UTF-8，生成请求未发送。")?;
        let text = if rows == 0 {
            text.trim_start_matches('\u{feff}')
        } else {
            text
        };
        let text = text.trim_end_matches(['\r', '\n']);
        if text.is_empty() {
            continue;
        }
        rows += 1;
        let (path, metadata) = text
            .split_once('\t')
            .filter(|(_, metadata)| !metadata.trim().is_empty() && !metadata.contains('\t'))
            .ok_or("方舟资源清单行格式损坏，生成请求未发送。")?;
        if path.is_empty()
            || path.starts_with('/')
            || path.contains('\\')
            || path.contains(':')
            || path.chars().any(char::is_control)
            || metadata.chars().any(char::is_control)
            || path.split('/').any(|part| matches!(part, "" | "." | ".."))
        {
            return Err("方舟资源清单包含异常路径，生成请求未发送。".into());
        }
        let Some(package) = path
            .strip_prefix("ShooterGame/Content/")
            .and_then(|path| path.strip_suffix(".uasset"))
        else {
            continue;
        };
        let name = package.rsplit('/').next().unwrap_or_default();
        if !name.eq_ignore_ascii_case(short) {
            continue;
        }
        let candidate = format!("/Game/{package}.{name}_C");
        if candidate.len() > 300
            || candidate.contains("..")
            || !candidate
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_/.-".contains(&byte))
        {
            return Err("方舟资源清单中的生物蓝图路径无效，生成请求未发送。".into());
        }
        if resolved
            .as_ref()
            .is_some_and(|previous| previous != &candidate)
        {
            return Err("生物短类名对应多个资源，请填写完整 /Game/ 蓝图路径。".into());
        }
        resolved = Some(candidate);
    }
    if rows == 0 {
        return Err("方舟资源清单没有有效条目，生成请求未发送。".into());
    }
    Ok((resolved, total))
}

#[cfg(test)]
#[path = "ark_tools_blueprints_tests.rs"]
mod tests;
