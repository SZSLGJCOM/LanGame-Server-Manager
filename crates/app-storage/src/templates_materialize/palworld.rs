use std::collections::HashSet;
use std::io::{ErrorKind, Read};

use super::managed_config_merge::{materialization_error, merge_ini_documents, parse_ini_document};
use super::*;

const SECTION: &str = "/Script/Pal.PalGameWorldSettings";
const MAX_CONFIG_BYTES: usize = 256 * 1024;
const MAX_OPTIONS: usize = 4096;
const MAX_NESTING: usize = 64;

pub(super) fn plan_world_settings(
    source: &Path,
    destination: &Path,
) -> Result<ManagedConfigMergePlan, StorageError> {
    let rendered = read_bounded(source)?.ok_or_else(|| {
        materialization_error(
            "palworld",
            source,
            "rendered world settings are missing".to_owned(),
        )
    })?;
    let original = read_bounded(destination)?;
    let replacement = merge_world_settings(&rendered, original.as_deref())
        .map_err(|message| materialization_error("palworld", destination, message))?;
    Ok(ManagedConfigMergePlan {
        destination_path: destination.to_path_buf(),
        replacement,
        original,
    })
}

fn read_bounded(path: &Path) -> Result<Option<Vec<u8>>, StorageError> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(StorageError::ReadConfig {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let mut bytes = Vec::new();
    file.take(MAX_CONFIG_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| StorageError::ReadConfig {
            path: path.to_path_buf(),
            source,
        })?;
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err(materialization_error(
            "palworld",
            path,
            "world settings exceed the size limit".to_owned(),
        ));
    }
    Ok(Some(bytes))
}

fn merge_world_settings(rendered: &[u8], existing: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let rendered =
        std::str::from_utf8(rendered).map_err(|_| "rendered world settings are not UTF-8")?;
    parse_ini_document(rendered)?;
    let (line_index, rendered_map) =
        option_line(rendered)?.ok_or("rendered world settings must contain OptionSettings")?;
    let managed = parse_options(rendered_map)?;
    let Some(existing) = existing else {
        return Ok(rendered.as_bytes().to_vec());
    };
    let existing = std::str::from_utf8(existing)
        .map_err(|_| "refusing to replace non-UTF-8 world settings")?;
    let existing_document = parse_ini_document(existing)?;
    let prior = option_line(existing)?
        .map(|(_, value)| parse_options(value))
        .transpose()?
        .unwrap_or_default();
    let managed_keys = managed
        .iter()
        .map(|(key, _)| key.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    // OptionSettings is one INI value containing a nested Unreal struct. Only
    // current rendered members are managed; unknown and retired members survive.
    let entries = prior
        .iter()
        .filter(|(key, _)| !managed_keys.contains(&key.to_ascii_lowercase()))
        .chain(managed.iter())
        .map(|(_, entry)| *entry)
        .collect::<Vec<_>>();
    if entries.len() > MAX_OPTIONS {
        return Err("merged world settings contain too many options".to_owned());
    }
    let mut document = rendered
        .lines()
        .enumerate()
        .map(|(index, line)| {
            if index == line_index {
                format!("OptionSettings=({})", entries.join(","))
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    document.push('\n');
    let merged = merge_ini_documents(existing_document, parse_ini_document(&document)?, &[]);
    if merged.len() > MAX_CONFIG_BYTES {
        return Err("merged world settings exceed the size limit".to_owned());
    }
    Ok(merged.into_bytes())
}

fn option_line(document: &str) -> Result<Option<(usize, &str)>, String> {
    let document = document.strip_prefix('\u{feff}').unwrap_or(document);
    if document.contains('\u{feff}') {
        return Err("unexpected byte-order mark inside world settings".to_owned());
    }
    let mut in_section = false;
    let mut section_seen = false;
    let mut result = None;
    for (index, line) in document.lines().enumerate() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_section = line[1..line.len() - 1].trim().eq_ignore_ascii_case(SECTION);
            if in_section && std::mem::replace(&mut section_seen, true) {
                return Err("duplicate PalGameWorldSettings sections are ambiguous".to_owned());
            }
            continue;
        }
        if !in_section || line.starts_with([';', '#']) || line.starts_with("//") {
            continue;
        }
        if let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case("OptionSettings")
            && result.replace((index, value.trim())).is_some()
        {
            return Err("duplicate OptionSettings values are ambiguous".to_owned());
        }
    }
    Ok(result)
}

fn parse_options(value: &str) -> Result<Vec<(&str, &str)>, String> {
    let body = value
        .strip_prefix('(')
        .and_then(|value| value.strip_suffix(')'))
        .ok_or("OptionSettings must be a parenthesized struct")?;
    if body.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut quoted = false;
    let mut escaped = false;
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut entries = Vec::new();
    let mut keys = HashSet::new();
    for (index, character) in body.char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                quoted = false;
            }
            continue;
        }
        match character {
            '"' => quoted = true,
            '(' => {
                depth += 1;
                if depth > MAX_NESTING {
                    return Err("OptionSettings nesting exceeds the limit".to_owned());
                }
            }
            ')' => {
                depth = depth
                    .checked_sub(1)
                    .ok_or("unbalanced OptionSettings parentheses")?;
            }
            ',' if depth == 0 => {
                push_entry(&body[start..index], &mut entries, &mut keys)?;
                start = index + 1;
            }
            _ => {}
        }
    }
    if quoted || depth != 0 {
        return Err("unterminated OptionSettings value".to_owned());
    }
    push_entry(&body[start..], &mut entries, &mut keys)?;
    Ok(entries)
}

fn push_entry<'a>(
    entry: &'a str,
    entries: &mut Vec<(&'a str, &'a str)>,
    keys: &mut HashSet<String>,
) -> Result<(), String> {
    if entries.len() >= MAX_OPTIONS {
        return Err("too many OptionSettings members".to_owned());
    }
    let entry = entry.trim();
    let (key, _) = entry
        .split_once('=')
        .ok_or("OptionSettings member is missing an assignment")?;
    let key = key.trim();
    if key.is_empty()
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err("invalid OptionSettings member name".to_owned());
    }
    if !keys.insert(key.to_ascii_lowercase()) {
        return Err("duplicate OptionSettings member is ambiguous".to_owned());
    }
    entries.push((key, entry));
    Ok(())
}
