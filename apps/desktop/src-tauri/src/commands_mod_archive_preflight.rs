//! Bound container metadata before the archive libraries retain directory tables.
//! These checks walk framing only; the libraries still verify headers and decode.
use super::*;

pub(super) fn seven_z(file: &mut fs::File, size: u64) -> Result<(), String> {
    let mut header = [0; 32];
    file.read_exact(&mut header)
        .map_err(|error| error.to_string())?;
    let offset = u64::from_le_bytes(header[12..20].try_into().unwrap());
    let length = u64::from_le_bytes(header[20..28].try_into().unwrap());
    if length > MANUAL_MOD_ARCHIVE_MAX_CENTRAL_DIRECTORY_BYTES {
        return Err("7z directory exceeds the metadata budget".into());
    }
    let end = 32u64
        .checked_add(offset)
        .and_then(|start| start.checked_add(length));
    if end.is_none_or(|end| end > size) {
        return Err("7z directory exceeds the archive boundary".into());
    }
    // Recovery mode searches for a header when the normal trailer is absent;
    // a downloaded package must have a complete, bounded directory instead.
    if length == 0 && size != 32 {
        return Err("7z archive has no complete directory".into());
    }
    file.rewind().map_err(|error| error.to_string())
}

pub(super) fn rar(file: &mut fs::File, size: u64, rar5: bool) -> Result<(), String> {
    let mut offset: u64 = if rar5 { 8 } else { 7 };
    let mut headers = 0usize;
    let mut entries = 0usize;
    let mut header_bytes = 0u64;
    while offset < size {
        headers = headers.checked_add(1).ok_or("RAR header count overflow")?;
        // Service records also consume memory even though they are not files.
        if headers > MANUAL_MOD_ARCHIVE_MAX_ENTRY_COUNT * 2 + 64 {
            return Err("RAR archive contains too many headers".into());
        }
        file.seek(SeekFrom::Start(offset))
            .map_err(|error| error.to_string())?;
        let (header_size, data_size, is_file, end) = if rar5 {
            let mut crc = [0; 4];
            file.read_exact(&mut crc)
                .map_err(|error| error.to_string())?;
            let (length, size_bytes) = vint(file)?;
            if length > unrar_rs::limits::RAR5_MAX_HEADER_BODY {
                return Err("RAR header exceeds the format metadata limit".into());
            }
            let header_size = length
                .checked_add(4 + size_bytes)
                .ok_or("RAR header size overflow")?;
            let mut consumed = 0;
            let kind = header_vint(file, &mut consumed, length)?;
            let flags = header_vint(file, &mut consumed, length)?;
            if kind == 4 {
                return Err("Mod archive requires a password".into());
            }
            if flags & 1 != 0 {
                let extra = header_vint(file, &mut consumed, length)?;
                if extra > length.saturating_sub(consumed) {
                    return Err("RAR extra header exceeds its block".into());
                }
            }
            let data = if flags & 2 != 0 {
                header_vint(file, &mut consumed, length)?
            } else {
                0
            };
            (header_size, data, kind == 2, kind == 5)
        } else {
            let mut base = [0; 7];
            file.read_exact(&mut base)
                .map_err(|error| error.to_string())?;
            let kind = base[2];
            let flags = u16::from_le_bytes([base[3], base[4]]);
            let length = u16::from_le_bytes([base[5], base[6]]) as u64;
            if length < 7 {
                return Err("RAR header is shorter than its framing".into());
            }
            if kind == 0x73 && flags & 0x80 != 0 {
                return Err("Mod archive requires a password".into());
            }
            let mut data_size = 0;
            if flags & 0x8000 != 0 {
                if length < 11 {
                    return Err("RAR data header is incomplete".into());
                }
                let mut low = [0; 4];
                file.read_exact(&mut low)
                    .map_err(|error| error.to_string())?;
                data_size = u32::from_le_bytes(low) as u64;
            }
            if matches!(kind, 0x74 | 0x7a) && flags & 0x100 != 0 {
                if length < 40 {
                    return Err("RAR large file header is incomplete".into());
                }
                file.seek(SeekFrom::Start(offset + 32))
                    .map_err(|error| error.to_string())?;
                let mut high = [0; 4];
                file.read_exact(&mut high)
                    .map_err(|error| error.to_string())?;
                data_size |= u64::from(u32::from_le_bytes(high)) << 32;
            }
            (length, data_size, kind == 0x74, kind == 0x7b)
        };
        header_bytes = header_bytes
            .checked_add(header_size)
            .ok_or("RAR metadata size overflow")?;
        if header_bytes > MANUAL_MOD_ARCHIVE_MAX_CENTRAL_DIRECTORY_BYTES {
            return Err("RAR directory exceeds the metadata budget".into());
        }
        if is_file {
            entries += 1;
            check_count(entries)?;
        }
        let next = offset
            .checked_add(header_size)
            .and_then(|value| value.checked_add(data_size))
            .ok_or("RAR block size overflow")?;
        if next <= offset || next > size {
            return Err("RAR block exceeds the archive boundary".into());
        }
        offset = next;
        if end {
            break;
        }
    }
    if headers == 0 {
        return Err("RAR archive has no headers".into());
    }
    file.rewind().map_err(|error| error.to_string())
}

fn header_vint(file: &mut fs::File, consumed: &mut u64, limit: u64) -> Result<u64, String> {
    let (value, length) = vint(file)?;
    *consumed = consumed
        .checked_add(length)
        .ok_or("RAR header size overflow")?;
    if *consumed > limit {
        return Err("RAR value exceeds its header".into());
    }
    Ok(value)
}

fn vint(file: &mut fs::File) -> Result<(u64, u64), String> {
    let mut value = 0u64;
    for index in 0..10 {
        let mut byte = [0];
        file.read_exact(&mut byte)
            .map_err(|error| error.to_string())?;
        if index == 9 && byte[0] > 1 {
            return Err("RAR integer overflow".into());
        }
        value |= u64::from(byte[0] & 0x7f) << (index * 7);
        if byte[0] & 0x80 == 0 {
            return Ok((value, index + 1));
        }
    }
    Err("RAR integer is unterminated".into())
}
