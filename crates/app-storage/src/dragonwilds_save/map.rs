use std::collections::BTreeMap;

use super::spud::{Reader, Result, encode_string, put_count};

pub(super) struct Entry<'a> {
    pub tag: String,
    pub value: f32,
    pub raw: &'a [u8],
}

/// Current native SPUD opaque map: removed-key count, entry count, then
/// tagged FGameplayTag structs and IEEE-754 values. No delta maps are accepted.
pub(super) fn decode(data: &[u8]) -> Result<Vec<Entry<'_>>> {
    let mut reader = Reader::new(data);
    if reader.u32()? != 0 {
        return Err("Dragonwilds delta difficulty maps are unsupported".into());
    }
    let count = reader.count(17)?;
    let mut entries = Vec::new();
    let mut seen = BTreeMap::new();
    for _ in 0..count {
        let start = reader.position;
        if reader.string()? != "TagName" || reader.string()? != "NameProperty" {
            return Err("Unsupported Dragonwilds gameplay tag serialization".into());
        }
        if reader.u32()? != 0 {
            return Err("Unexpected Dragonwilds gameplay tag array index".into());
        }
        let size = usize::try_from(reader.u32()?).map_err(|_| "Gameplay tag size overflow")?;
        if reader.u8()? != 0 {
            return Err("Unsupported Dragonwilds gameplay tag flags".into());
        }
        let mut payload = Reader::new(reader.take(size)?);
        let tag = payload.string()?;
        payload.finish()?;
        if tag.is_empty() || tag.chars().any(char::is_control) {
            return Err("Invalid Dragonwilds gameplay tag name".into());
        }
        if reader.string()? != "None" {
            return Err("Dragonwilds gameplay tag has unexpected additional properties".into());
        }
        let value = float(reader.take(4)?)?;
        if seen.insert(tag.clone(), ()).is_some() {
            return Err("Duplicate Dragonwilds difficulty tags".into());
        }
        entries.push(Entry {
            tag,
            value,
            raw: &data[start..reader.position],
        });
    }
    reader.finish()?;
    Ok(entries)
}

pub(super) fn float(bytes: &[u8]) -> Result<f32> {
    let value = f32::from_le_bytes(
        bytes
            .try_into()
            .map_err(|_| "Difficulty value must contain exactly four bytes")?,
    );
    if !value.is_finite() {
        return Err("Dragonwilds difficulty values must be finite".into());
    }
    Ok(value)
}

pub(super) fn valid_new_tag(tag: &str) -> bool {
    tag.strip_prefix("Difficulty.").is_some_and(|suffix| {
        !suffix.is_empty()
            && suffix.split('.').all(|part| {
                !part.is_empty() && part.bytes().all(|v| v.is_ascii_alphanumeric() || v == b'_')
            })
    })
}

fn encode_entry(tag: &str, value: f32) -> Result<Vec<u8>> {
    let payload = encode_string(tag)?;
    let mut result = encode_string("TagName")?;
    result.extend_from_slice(&encode_string("NameProperty")?);
    result.extend_from_slice(&0u32.to_le_bytes());
    put_count(&mut result, payload.len())?;
    result.push(0);
    result.extend_from_slice(&payload);
    result.extend_from_slice(&encode_string("None")?);
    result.extend_from_slice(&value.to_le_bytes());
    Ok(result)
}

pub(super) fn encode(entries: &[Entry<'_>], values: &BTreeMap<String, f32>) -> Result<Vec<u8>> {
    let mut remaining = values.clone();
    let mut output = 0u32.to_le_bytes().to_vec();
    put_count(&mut output, values.len())?;
    // Preserve original order and all key bytes, including unknown existing keys.
    for entry in entries {
        if let Some(value) = remaining.remove(&entry.tag) {
            if !value.is_finite() {
                return Err("Dragonwilds difficulty values must be finite".into());
            }
            if value.to_bits() == entry.value.to_bits() {
                output.extend_from_slice(entry.raw);
            } else {
                output.extend_from_slice(&entry.raw[..entry.raw.len() - 4]);
                output.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
    for (tag, value) in remaining {
        if !valid_new_tag(&tag) {
            return Err("New Dragonwilds overrides require a valid Difficulty.* tag".into());
        }
        if !value.is_finite() {
            return Err("Dragonwilds difficulty values must be finite".into());
        }
        output.extend_from_slice(&encode_entry(&tag, value)?);
    }
    Ok(output)
}
