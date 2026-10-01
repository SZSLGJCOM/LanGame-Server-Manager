use std::collections::{BTreeMap, BTreeSet};

const MAX_MANIFEST_BYTES: usize = 64 * 1024;
const MAX_TOKENS: usize = 4096;
const MAX_DEPTH: usize = 4;
const MAX_STRING_BYTES: usize = 4096;
const MAX_PACKAGES: usize = 32;
const MAX_PACKAGE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct UpdateManifest {
    pub(super) version: String,
    pub(super) packages: Vec<UpdatePackage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct UpdatePackage {
    pub(super) name: String,
    pub(super) file: String,
    pub(super) size: u64,
    pub(super) sha256: String,
    pub(super) bootstrapper: bool,
    // Native SteamCMD can consume VZ cache entries, but the bootstrap executable
    // must be extracted from the independently verified official ZIP variant.
    pub(super) archive_file: String,
    pub(super) archive_size: u64,
    pub(super) archive_sha256: String,
}

type Object = BTreeMap<String, Value>;

enum Value {
    Text(String),
    Object(Object),
}

enum Token {
    Text(String),
    Open,
    Close,
}

struct Parser<'a> {
    bytes: &'a [u8],
    cursor: usize,
    tokens: usize,
}

impl Parser<'_> {
    fn next_token(&mut self) -> Result<Option<Token>, String> {
        loop {
            while self
                .bytes
                .get(self.cursor)
                .is_some_and(u8::is_ascii_whitespace)
            {
                self.cursor += 1;
            }
            if self.bytes.get(self.cursor..self.cursor + 2) != Some(b"//") {
                break;
            }
            while self.bytes.get(self.cursor).is_some_and(|b| *b != b'\n') {
                self.cursor += 1;
            }
        }
        let Some(&first) = self.bytes.get(self.cursor) else {
            return Ok(None);
        };
        self.tokens += 1;
        if self.tokens > MAX_TOKENS {
            return Err("SteamCMD manifest has too many tokens".into());
        }
        self.cursor += 1;
        match first {
            b'{' => Ok(Some(Token::Open)),
            b'}' => Ok(Some(Token::Close)),
            b'"' => self.quoted().map(|text| Some(Token::Text(text))),
            _ => {
                let start = self.cursor - 1;
                while self
                    .bytes
                    .get(self.cursor)
                    .is_some_and(|byte| !byte.is_ascii_whitespace() && !matches!(byte, b'{' | b'}'))
                {
                    self.cursor += 1;
                }
                let word = &self.bytes[start..self.cursor];
                if word.len() > MAX_STRING_BYTES
                    || !word.iter().all(|byte| (b'!'..=b'~').contains(byte))
                    || word.contains(&b'"')
                {
                    return Err("SteamCMD manifest contains an invalid token".into());
                }
                Ok(Some(Token::Text(
                    String::from_utf8(word.to_vec())
                        .map_err(|_| "SteamCMD manifest contains non-ASCII text".to_owned())?,
                )))
            }
        }
    }

    fn quoted(&mut self) -> Result<String, String> {
        let mut text = String::new();
        while let Some(&byte) = self.bytes.get(self.cursor) {
            self.cursor += 1;
            match byte {
                b'"' => return Ok(text),
                b'\\' => {
                    let escaped = *self
                        .bytes
                        .get(self.cursor)
                        .ok_or("SteamCMD manifest has an unfinished escape")?;
                    if !matches!(escaped, b'"' | b'\\') {
                        return Err("SteamCMD manifest has an unsupported escape".into());
                    }
                    self.cursor += 1;
                    text.push(char::from(escaped));
                }
                b' '..=b'~' => text.push(char::from(byte)),
                _ => return Err("SteamCMD manifest contains non-ASCII or control text".into()),
            }
            if text.len() > MAX_STRING_BYTES {
                return Err("SteamCMD manifest string is too long".into());
            }
        }
        Err("SteamCMD manifest has an unterminated string".into())
    }

    fn object(&mut self, depth: usize, nested: bool) -> Result<Object, String> {
        if depth > MAX_DEPTH {
            return Err("SteamCMD manifest nesting is too deep".into());
        }
        let mut object = Object::new();
        loop {
            let key = match self.next_token()? {
                Some(Token::Text(key)) if !key.is_empty() && key.len() <= 128 => {
                    key.to_ascii_lowercase()
                }
                Some(Token::Close) if nested => return Ok(object),
                None if !nested => return Ok(object),
                _ => return Err("SteamCMD manifest has an invalid object boundary or key".into()),
            };
            let value = match self.next_token()? {
                Some(Token::Text(text)) => Value::Text(text),
                Some(Token::Open) => Value::Object(self.object(depth + 1, true)?),
                _ => return Err("SteamCMD manifest key has no value".into()),
            };
            if object.insert(key, value).is_some() {
                return Err("SteamCMD manifest contains a duplicate field".into());
            }
        }
    }
}

fn required_text<'a>(object: &'a Object, key: &str) -> Result<&'a str, String> {
    match object.get(key) {
        Some(Value::Text(text)) => Ok(text),
        _ => Err(format!("SteamCMD manifest is missing a text field: {key}")),
    }
}

fn decimal(text: &str) -> Result<u64, String> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("SteamCMD manifest contains an invalid decimal value".into());
    }
    text.parse::<u64>()
        .map_err(|_| "SteamCMD manifest decimal value is too large".into())
}

fn package_size(text: &str) -> Result<u64, String> {
    let value = decimal(text)?;
    if value == 0 || value > MAX_PACKAGE_BYTES {
        return Err("SteamCMD manifest package size is outside the allowed range".into());
    }
    Ok(value)
}

fn filename(text: &str) -> Result<String, String> {
    let stem = text
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    if text.len() > 255
        || !text
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        || !text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        || text.contains("..")
        || text.ends_with('.')
        || reserved
    {
        return Err("SteamCMD manifest contains an unsafe package filename".into());
    }
    Ok(text.to_owned())
}

fn sha256(text: &str) -> Result<String, String> {
    if text.len() != 64 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("SteamCMD manifest contains an invalid SHA256 digest".into());
    }
    Ok(text.to_ascii_lowercase())
}

fn parse_package(name: &str, object: &Object) -> Result<UpdatePackage, String> {
    if name.is_empty()
        || name.len() > 96
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        || object
            .values()
            .any(|value| matches!(value, Value::Object(_)))
    {
        return Err("SteamCMD manifest contains an invalid package record".into());
    }
    let archive_file = filename(required_text(object, "file")?)?;
    let archive_size = package_size(required_text(object, "size")?)?;
    let archive_sha256 = sha256(required_text(object, "sha2")?)?;
    let (file, size, digest) = match (object.get("zipvz"), object.get("sha2vz")) {
        (None, None) => (archive_file.clone(), archive_size, archive_sha256.clone()),
        (Some(Value::Text(file)), Some(Value::Text(digest))) => {
            let file = filename(file)?;
            let (_, size) = file
                .rsplit_once('_')
                .ok_or("SteamCMD VZ package filename has no size suffix")?;
            let size = package_size(size)?;
            (file, size, sha256(digest)?)
        }
        _ => return Err("SteamCMD manifest has incomplete VZ package metadata".into()),
    };
    let bootstrapper = match object.get("isbootstrapperpackage") {
        None => false,
        Some(Value::Text(value)) if value == "0" => false,
        Some(Value::Text(value)) if value == "1" => true,
        _ => return Err("SteamCMD manifest has an invalid bootstrapper flag".into()),
    };
    Ok(UpdatePackage {
        name: name.to_owned(),
        file,
        size,
        sha256: digest,
        bootstrapper,
        archive_file,
        archive_size,
        archive_sha256,
    })
}

pub(super) fn parse_manifest(text: &str, platform: &str) -> Result<UpdateManifest, String> {
    if text.len() > MAX_MANIFEST_BYTES {
        return Err("SteamCMD manifest is too large".into());
    }
    if !matches!(platform, "win32" | "win64") {
        return Err("Unsupported SteamCMD update platform".into());
    }
    let root = Parser {
        bytes: text.as_bytes(),
        cursor: 0,
        tokens: 0,
    }
    .object(0, false)?;
    let Some(Value::Object(platform)) = root.get(platform) else {
        return Err("SteamCMD manifest has no matching platform object".into());
    };
    let version = required_text(platform, "version")?;
    if decimal(version)? == 0 {
        return Err("SteamCMD manifest version cannot be zero".into());
    }
    let mut packages = Vec::new();
    let mut archive_total = 0_u64;
    let mut download_total = 0_u64;
    let mut files = BTreeSet::new();
    for (name, value) in platform {
        if matches!(name.as_str(), "version" | "ostype") {
            required_text(platform, name)?;
            continue;
        }
        let Value::Object(object) = value else {
            return Err("SteamCMD manifest package must be an object".into());
        };
        let package = parse_package(name, object)?;
        archive_total += package.archive_size;
        download_total += package.size;
        if packages.len() >= MAX_PACKAGES
            || archive_total > MAX_PACKAGE_BYTES
            || download_total > MAX_PACKAGE_BYTES
        {
            return Err("SteamCMD manifest package set exceeds the allowed limits".into());
        }
        if !files.insert(package.archive_file.to_ascii_lowercase())
            || (package.file != package.archive_file
                && !files.insert(package.file.to_ascii_lowercase()))
        {
            return Err("SteamCMD manifest reuses a package filename".into());
        }
        packages.push(package);
    }
    if packages.is_empty() {
        return Err("SteamCMD manifest has no packages".into());
    }
    Ok(UpdateManifest {
        version: version.to_owned(),
        packages,
    })
}

#[cfg(test)]
#[path = "steamcmd_update_manifest_tests.rs"]
mod tests;
