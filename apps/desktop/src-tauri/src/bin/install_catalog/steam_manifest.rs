//! Bounded readers for Steam ACF and cached protobuf depot manifests.
use super::{Entry, MAX_FILES, Result, invalid, relative, validate_entries};
use std::collections::BTreeMap;

pub(super) type Object = BTreeMap<String, Value>;
#[derive(Debug)]
pub(super) enum Value {
    Text(String),
    Object(Object),
}
pub(super) fn text(value: &Value) -> Result<&str> {
    match value {
        Value::Text(s) => Ok(s),
        _ => Err(invalid("expected ACF text")),
    }
}
pub(super) fn object(value: &Value) -> Result<&Object> {
    match value {
        Value::Object(o) => Ok(o),
        _ => Err(invalid("expected ACF object")),
    }
}
pub(super) fn decimal(value: &str) -> Result<u64> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid("invalid Steam identifier"));
    }
    value.parse().map_err(invalid)
}
#[derive(Debug)]
enum Token {
    Text(String),
    Open,
    Close,
}
pub(super) fn parse_acf(bytes: &[u8], app_id: u32) -> Result<Object> {
    let source = std::str::from_utf8(bytes)
        .map_err(invalid)?
        .trim_start_matches('\u{feff}')
        .as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < source.len() {
        if source[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if source.get(i..i + 2) == Some(b"//") {
            while i < source.len() && source[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        match source[i] {
            b'{' => {
                tokens.push(Token::Open);
                i += 1;
            }
            b'}' => {
                tokens.push(Token::Close);
                i += 1;
            }
            b'"' => {
                i += 1;
                let mut value = Vec::new();
                while i < source.len() && source[i] != b'"' {
                    if source[i] == b'\\' && matches!(source.get(i + 1), Some(b'\\' | b'"')) {
                        i += 1;
                    }
                    value.push(source[i]);
                    i += 1;
                }
                if i == source.len() {
                    return Err(invalid("unterminated ACF string"));
                }
                i += 1;
                tokens.push(Token::Text(String::from_utf8(value).map_err(invalid)?));
            }
            _ => return Err(invalid("unquoted ACF token")),
        }
        if tokens.len() > 200_000 {
            return Err(invalid("ACF token limit exceeded"));
        }
    }
    let mut cursor = 0;
    let mut root = acf_object(&tokens, &mut cursor, 0)?;
    let app = match root.remove("appstate") {
        Some(Value::Object(app)) => app,
        _ => return Err(invalid("AppState missing")),
    };
    if decimal(text(
        app.get("appid")
            .ok_or_else(|| invalid("ACF appid missing"))?,
    )?)? != u64::from(app_id)
        || decimal(text(
            app.get("stateflags")
                .ok_or_else(|| invalid("ACF state missing"))?,
        )?)? != 4
    {
        return Err(invalid("ACF appid mismatch or installation incomplete"));
    }
    for (total, done) in [
        ("bytestodownload", "bytesdownloaded"),
        ("bytestostage", "bytesstaged"),
    ] {
        if let Some(total) = app.get(total) {
            let total = decimal(text(total)?)?;
            let done = app
                .get(done)
                .map(text)
                .transpose()?
                .map(decimal)
                .transpose()?
                .unwrap_or(0);
            if done < total {
                return Err(invalid("ACF byte progress incomplete"));
            }
        }
    }
    Ok(app)
}
fn acf_object(tokens: &[Token], cursor: &mut usize, depth: usize) -> Result<Object> {
    if depth > 16 {
        return Err(invalid("ACF nesting limit exceeded"));
    }
    let mut values = BTreeMap::new();
    while let Some(token) = tokens.get(*cursor) {
        *cursor += 1;
        let key = match token {
            Token::Text(key) => key.to_ascii_lowercase(),
            Token::Close if depth > 0 => return Ok(values),
            _ => return Err(invalid("ACF key expected")),
        };
        let token = tokens
            .get(*cursor)
            .ok_or_else(|| invalid("ACF value missing"))?;
        *cursor += 1;
        let value = match token {
            Token::Text(value) => Value::Text(value.clone()),
            Token::Open => Value::Object(acf_object(tokens, cursor, depth + 1)?),
            _ => return Err(invalid("ACF value expected")),
        };
        if values.insert(key, value).is_some() {
            return Err(invalid("duplicate ACF key"));
        }
    }
    if depth != 0 {
        return Err(invalid("unterminated ACF object"));
    }
    Ok(values)
}

// Wire layout: SteamKit commit 84c990c3982eedb5abd733116b987c9870a0dccb,
// ContentManifest.cs and DepotManifest.cs. This is an independent bounded reader.
#[cfg(test)]
pub(super) fn manifest(data: &[u8], depot: u64, gid: u64) -> Result<BTreeMap<String, Entry>> {
    manifest_inventory(data, depot, gid).map(|(files, _)| files)
}

pub(super) fn manifest_inventory(
    data: &[u8],
    depot: u64,
    gid: u64,
) -> Result<(BTreeMap<String, Entry>, std::collections::BTreeSet<String>)> {
    let mut rest = data;
    let mut sections = BTreeMap::new();
    loop {
        let magic = little(&mut rest)?;
        if magic == 0x32c415ab {
            if !rest.is_empty() {
                return Err(invalid("trailing manifest data"));
            }
            break;
        }
        if !matches!(magic, 0x71f617d0 | 0x1f4812be | 0x1b81b817) {
            return Err(invalid("unknown manifest section"));
        }
        let length = little(&mut rest)? as usize;
        let bytes = take(&mut rest, length)?;
        if sections.insert(magic, bytes).is_some() {
            return Err(invalid("duplicate manifest section"));
        }
    }
    if sections.len() != 3 {
        return Err(invalid("missing manifest section"));
    }
    let metadata = fields(sections[&0x1f4812be])?;
    if number(&metadata, 1)? != depot || number(&metadata, 2)? != gid || number(&metadata, 4)? != 0
    {
        return Err(invalid("manifest depot/GID mismatch or encrypted names"));
    }
    let payload = sections[&0x71f617d0];
    let crc = number(&metadata, 9)?;
    if crc == 0 || crc != u64::from(crc32(payload)) {
        return Err(invalid("manifest payload CRC mismatch or missing"));
    }
    let mut files = BTreeMap::new();
    let mut directories = std::collections::BTreeSet::new();
    for (id, wire, mapping) in fields(payload)? {
        if id != 1 {
            continue;
        }
        if wire != 2 || files.len() >= MAX_FILES {
            return Err(invalid("invalid or excessive file mappings"));
        }
        let mapping = fields(mapping)?;
        let name = relative(std::str::from_utf8(blob(&mapping, 1)?).map_err(invalid)?)?;
        let flags = number(&mapping, 3)?;
        if flags & !1023 != 0 || flags & (4 | 512) != 0 || !blob(&mapping, 7)?.is_empty() {
            return Err(invalid("unsupported encrypted/linked Steam entry"));
        }
        if flags & 64 != 0 {
            if !directories.insert(name) || directories.len() > MAX_FILES {
                return Err(invalid("duplicate or excessive manifest directories"));
            }
            continue;
        }
        let hash = blob(&mapping, 5)?;
        let size = number(&mapping, 2)?;
        if hash.len() != 20 {
            return Err(invalid("missing Steam content SHA1"));
        }
        let zero_hash = hash.iter().all(|byte| *byte == 0);
        let sha1 = if zero_hash {
            // Steam's empty files can have no chunks and an all-zero digest.
            // DepotDownloader likewise completes a zero-length, zero-chunk file.
            // Do not apply that representation to nonempty or chunked files.
            if size != 0 || mapping.iter().any(|field| field.0 == 6) {
                return Err(invalid(
                    "zero SHA1 is only valid for zero-byte, zero-chunk files",
                ));
            }
            "da39a3ee5e6b4b0d3255bfef95601890afd80709".into()
        } else {
            hash.iter().map(|byte| format!("{byte:02x}")).collect()
        };
        let entry = Entry { size, sha1 };
        if files.insert(name, entry).is_some() {
            return Err(invalid("duplicate manifest path"));
        }
    }
    validate_entries(&files)?;
    Ok((files, directories))
}
fn take<'a>(bytes: &mut &'a [u8], count: usize) -> Result<&'a [u8]> {
    if count > bytes.len() {
        return Err(invalid("truncated manifest"));
    }
    let (value, rest) = bytes.split_at(count);
    *bytes = rest;
    Ok(value)
}
fn little(bytes: &mut &[u8]) -> Result<u32> {
    Ok(u32::from_le_bytes(
        take(bytes, 4)?.try_into().map_err(invalid)?,
    ))
}
fn varint(bytes: &mut &[u8]) -> Result<u64> {
    let mut value = 0;
    for index in 0..10 {
        let byte = take(bytes, 1)?[0];
        if index == 9 && byte > 1 {
            return Err(invalid("protobuf integer overflow"));
        }
        value |= u64::from(byte & 127) << (index * 7);
        if byte < 128 {
            return Ok(value);
        }
    }
    Err(invalid("unterminated protobuf integer"))
}
type Field<'a> = (u64, u8, &'a [u8]);
pub(super) fn fields(mut bytes: &[u8]) -> Result<Vec<Field<'_>>> {
    let mut result = Vec::new();
    while !bytes.is_empty() {
        let tag = varint(&mut bytes)?;
        let id = tag >> 3;
        let wire = (tag & 7) as u8;
        if id == 0 || id > 536_870_911 || result.len() >= 2_000_000 {
            return Err(invalid("invalid protobuf field"));
        }
        let value = match wire {
            0 => {
                let original = bytes;
                varint(&mut bytes)?;
                &original[..original.len() - bytes.len()]
            }
            1 => take(&mut bytes, 8)?,
            5 => take(&mut bytes, 4)?,
            2 => {
                let size = usize::try_from(varint(&mut bytes)?).map_err(invalid)?;
                take(&mut bytes, size)?
            }
            _ => return Err(invalid("unsupported protobuf wire type")),
        };
        result.push((id, wire, value));
    }
    Ok(result)
}
fn field<'a>(fields: &[Field<'a>], id: u64, wire: u8) -> Result<Option<&'a [u8]>> {
    let mut matches = fields.iter().filter(|f| f.0 == id);
    let value = matches.next();
    if matches.next().is_some() || value.is_some_and(|f| f.1 != wire) {
        return Err(invalid("duplicate or mistyped protobuf field"));
    }
    Ok(value.map(|f| f.2))
}
pub(super) fn number(fields: &[Field<'_>], id: u64) -> Result<u64> {
    field(fields, id, 0)?
        .map(|mut bytes| varint(&mut bytes))
        .transpose()
        .map(|v| v.unwrap_or(0))
}
fn blob<'a>(fields: &[Field<'a>], id: u64) -> Result<&'a [u8]> {
    Ok(field(fields, id, 2)?.unwrap_or(&[]))
}
fn crc32(payload: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in (payload.len() as u32).to_le_bytes().iter().chain(payload) {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320_u32 & (0_u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

pub(super) fn depot_gid(value: &Value) -> Result<u64> {
    decimal(text(
        object(value)?
            .get("manifest")
            .ok_or_else(|| invalid("depot GID missing"))?,
    )?)
}
pub(super) fn insert_depot(depots: &mut BTreeMap<u64, u64>, id: u64, gid: u64) -> Result<()> {
    if id == 0 || gid == 0 || depots.insert(id, gid).is_some_and(|old| old != gid) {
        return Err(invalid("invalid or conflicting depot registration"));
    }
    Ok(())
}
pub(super) fn collect_depots(app: &Object, depots: &mut BTreeMap<u64, u64>) -> Result<()> {
    if let Some(installed) = app.get("installeddepots") {
        for (id, value) in object(installed)? {
            insert_depot(depots, decimal(id)?, depot_gid(value)?)?;
        }
    }
    Ok(())
}
