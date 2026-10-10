/// Valheim World::SaveWorldFWLData uses a length-prefixed ZPackage with .NET
/// BinaryWriter strings. Version 32 adds starting global keys; 41 adds player
/// history after them. The full package is validated, including history records,
/// but names, seeds and player identifiers are never returned.
pub(super) fn decode_keys(bytes: &[u8]) -> Result<(i32, Vec<String>, bool), String> {
    let mut reader = Reader { bytes, offset: 0 };
    let length = reader.int()?;
    if length < 0 || length as usize != bytes.len().saturating_sub(4) {
        return Err("World metadata package length is invalid.".into());
    }
    let version = reader.int()?;
    if !(26..=41).contains(&version) {
        return Err(format!(
            "Unsupported Valheim world metadata version: {version}."
        ));
    }
    let _display_name = reader.string()?;
    let _seed_name = reader.string()?;
    reader.skip(4 + 8 + 4)?; // seed, world UID, world generation version
    let needs_db = if version >= 30 {
        let flag = reader.take(1)?[0];
        if flag > 1 {
            return Err("World metadata boolean is invalid.".into());
        }
        flag == 1
    } else {
        false
    };
    let mut keys = Vec::new();
    if version >= 32 {
        let count = reader.int()?;
        if !(0..=512).contains(&count) {
            return Err("World metadata key count exceeds 512.".into());
        }
        for _ in 0..count {
            let key = reader.string()?;
            if key.len() > 1024 || key.chars().any(char::is_control) {
                return Err("World metadata contains an invalid rule key.".into());
            }
            keys.push(key);
        }
    }
    if version >= 41 {
        let history_count = reader.int()?;
        // CrossNetworkUserInfo.Write/Read contains exactly four BinaryWriter
        // strings (platform ID, display name, assigned name, PlayFab ID). Even
        // empty strings need one length byte each; reject impossible counts
        // before iterating. The outer 64 KiB read budget bounds all records.
        if history_count < 0 || history_count as usize > reader.remaining() / 4 {
            return Err("World metadata player history count is invalid.".into());
        }
        for _ in 0..history_count {
            for _ in 0..4 {
                let _private_value = reader.string()?;
            }
        }
    }
    if reader.remaining() != 0 {
        return Err("World metadata contains unrecognized trailing data.".into());
    }
    Ok((version, keys, needs_db))
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.offset)
    }
    fn take(&mut self, size: usize) -> Result<&'a [u8], String> {
        let end = self
            .offset
            .checked_add(size)
            .ok_or("World metadata offset exceeds its numeric range.")?;
        let result = self
            .bytes
            .get(self.offset..end)
            .ok_or("World metadata is truncated.")?;
        self.offset = end;
        Ok(result)
    }
    fn skip(&mut self, size: usize) -> Result<(), String> {
        self.take(size).map(|_| ())
    }
    fn int(&mut self) -> Result<i32, String> {
        let mut bytes = [0; 4];
        bytes.copy_from_slice(self.take(4)?);
        Ok(i32::from_le_bytes(bytes))
    }
    fn string(&mut self) -> Result<String, String> {
        let mut length = 0_u32;
        for shift in (0..35).step_by(7) {
            let byte = self.take(1)?[0];
            if shift == 28 && byte > 15 {
                return Err("World metadata string length is invalid.".into());
            }
            length |= u32::from(byte & 127) << shift;
            if byte & 128 == 0 {
                if length > 4096 {
                    return Err("World metadata string exceeds 4096 bytes.".into());
                }
                return String::from_utf8(self.take(length as usize)?.to_vec())
                    .map_err(|_| "World metadata string is not UTF-8.".into());
            }
        }
        Err("World metadata string length is invalid.".into())
    }
}
