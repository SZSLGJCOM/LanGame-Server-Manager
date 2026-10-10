use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use app_modules::ModuleDescriptor;
use sha2::{Digest, Sha256};

use super::file_stamp::FileStamp;
use super::{CleanPackage, checked_relative, invalid};
use crate::StorageError;
use crate::instance_creation_io::check_creation_cancelled;
use crate::instance_isolation::paths::normalize_resource_path;

const MAX_STEAM_MANIFEST_BYTES: u64 = 1024 * 1024;

/// Steam validation rewrites timestamps and progress in this one installation
/// record. Its current digest remains in the copy allowlist, but those fields
/// must not invalidate otherwise unchanged program bytes. The caller has
/// already established that official validation kept the package version.
pub(super) fn refresh_existing_entry(
    root: &Path,
    descriptor: &ModuleDescriptor,
    manifest: &mut CleanPackage,
    cancellation: Option<&AtomicBool>,
) -> Result<bool, StorageError> {
    let Some(app_id) = descriptor.summary.steam_app_id.filter(|id| *id > 0) else {
        return Ok(true);
    };
    let key = format!("steamapps/appmanifest_{app_id}.acf");
    if !manifest.files.contains_key(&key) {
        // Never import a newly discovered file into an existing official list.
        return Ok(true);
    }
    check_creation_cancelled(cancellation)?;
    let path = root.join(checked_relative(&key)?);
    normalize_resource_path(&path)?;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        };
        options
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = match options.open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(source) => return Err(StorageError::ReadPath { path, source }),
    };
    let metadata = file.metadata().map_err(|source| StorageError::ReadPath {
        path: path.clone(),
        source,
    })?;
    if !metadata.is_file() || metadata.len() > MAX_STEAM_MANIFEST_BYTES {
        return Ok(false);
    }
    let before = FileStamp::read(&file).map_err(|source| StorageError::ReadPath {
        path: path.clone(),
        source,
    })?;
    let mut bytes = Vec::new();
    (&file)
        .take(MAX_STEAM_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| StorageError::ReadPath {
            path: path.clone(),
            source,
        })?;
    let after = FileStamp::read(&file).map_err(|source| StorageError::ReadPath {
        path: path.clone(),
        source,
    })?;
    if before != after || bytes.len() as u64 > MAX_STEAM_MANIFEST_BYTES {
        return Err(invalid(
            &path,
            "Steam installation metadata changed during verification",
        ));
    }
    if !std::str::from_utf8(&bytes)
        .ok()
        .is_some_and(|text| installed_app_state(text, app_id))
    {
        return Ok(false);
    }
    let downloading = root.join("steamapps/downloading").join(app_id.to_string());
    normalize_resource_path(&downloading)?;
    match fs::read_dir(&downloading) {
        Ok(mut entries) => match entries.next() {
            None => {}
            Some(Ok(_)) => return Ok(false),
            Some(Err(source)) => {
                return Err(StorageError::ReadDirectory {
                    path: downloading,
                    source,
                });
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(StorageError::ReadDirectory {
                path: downloading,
                source,
            });
        }
    }
    check_creation_cancelled(cancellation)?;
    let digest = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    manifest.files.insert(key.clone(), digest);
    manifest.verified_files.remove(&key);
    Ok(true)
}

fn installed_app_state(text: &str, app_id: u32) -> bool {
    let Some(fields) = app_state_fields(text) else {
        return false;
    };
    let number = |key: &str| fields.get(key).and_then(|value| value.parse::<u64>().ok());
    if number("appid") != Some(u64::from(app_id)) || number("stateflags") != Some(4) {
        return false;
    }
    let Some(build) = number("buildid").filter(|build| *build > 0) else {
        return false;
    };
    if fields.contains_key("targetbuildid")
        && !number("targetbuildid").is_some_and(|target| target == 0 || target == build)
    {
        return false;
    }
    for (total, done) in [
        ("bytestodownload", "bytesdownloaded"),
        ("bytestostage", "bytesstaged"),
    ] {
        if fields.contains_key(total) && number(total).is_none()
            || fields.contains_key(done) && number(done).is_none()
            || number(total)
                .is_some_and(|total| total > 0 && number(done).is_none_or(|done| done < total))
        {
            return false;
        }
    }
    true
}

#[derive(PartialEq)]
enum Token {
    Text(String),
    Open,
    Close,
}

/// Parse only top-level scalar fields from one AppState object. Nested depot
/// and UserConfig fields cannot shadow app identity, build or installation state.
fn app_state_fields(text: &str) -> Option<BTreeMap<String, String>> {
    let tokens = tokenize(text)?;
    let mut cursor = 0;
    if tokens.first()? != &Token::Text("AppState".into()) || tokens.get(1)? != &Token::Open {
        return None;
    }
    cursor += 2;
    let fields = read_block(&tokens, &mut cursor, 0, true)?;
    (cursor == tokens.len()).then_some(fields)
}

fn read_block(
    tokens: &[Token],
    cursor: &mut usize,
    depth: u8,
    capture: bool,
) -> Option<BTreeMap<String, String>> {
    if depth > 32 {
        return None;
    }
    let mut fields = BTreeMap::new();
    let mut keys = BTreeSet::new();
    loop {
        let key = match tokens.get(*cursor)? {
            Token::Close => {
                *cursor += 1;
                return Some(fields);
            }
            Token::Text(key) => key.to_ascii_lowercase(),
            Token::Open => return None,
        };
        if !keys.insert(key.clone()) {
            return None;
        }
        *cursor += 1;
        match tokens.get(*cursor)? {
            Token::Text(value) => {
                if capture {
                    fields.insert(key, value.clone());
                }
                *cursor += 1;
            }
            Token::Open => {
                *cursor += 1;
                read_block(tokens, cursor, depth + 1, false)?;
            }
            Token::Close => return None,
        }
    }
}

fn tokenize(text: &str) -> Option<Vec<Token>> {
    let mut chars = text.chars().peekable();
    let mut tokens = Vec::new();
    while let Some(ch) = chars.next() {
        match ch {
            ch if ch.is_whitespace() => {}
            '/' if chars.next() == Some('/') => {
                for ch in chars.by_ref() {
                    if ch == '\n' {
                        break;
                    }
                }
            }
            '{' => tokens.push(Token::Open),
            '}' => tokens.push(Token::Close),
            '"' => {
                let mut value = String::new();
                loop {
                    match chars.next()? {
                        '"' => break,
                        '\\' => value.push(match chars.next()? {
                            '\\' => '\\',
                            '"' => '"',
                            'n' => '\n',
                            'r' => '\r',
                            't' => '\t',
                            _ => return None,
                        }),
                        ch => value.push(ch),
                    }
                }
                tokens.push(Token::Text(value));
            }
            _ => return None,
        }
    }
    Some(tokens)
}

#[cfg(test)]
#[path = "program_steam_metadata_tests.rs"]
mod tests;
