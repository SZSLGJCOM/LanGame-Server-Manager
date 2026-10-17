//! Layout verified from the installed Assembly-CSharp SteamBlacklist/River IL.
//! IP, judge, reason and hardware data are consumed only to locate the next entry;
//! the public projection exposes the Steam account identity alone.

use std::collections::BTreeSet;
use std::path::Path;

use app_core::{NativeBanEntry, RuntimePlayerIdentityKind};

use super::native_bans::MAX_ROWS;
use super::native_bans_io::invalid;
use crate::StorageError;

pub(super) fn parse(
    bytes: &[u8],
    path: &Path,
    observed_at_unix_ms: u64,
) -> Result<Vec<NativeBanEntry>, StorageError> {
    let mut cursor = Cursor {
        bytes,
        position: 0,
        path,
    };
    let version = cursor.byte()?;
    if !matches!(version, 2..=4) {
        return Err(invalid(
            "unturned",
            path,
            "Unsupported native Blacklist.dat version.",
        ));
    }
    let count = usize::from(cursor.u16()?);
    if count > MAX_ROWS {
        return Err(invalid(
            "unturned",
            path,
            "Native blacklist exceeds 4096 records.",
        ));
    }
    let now: u32 = (observed_at_unix_ms / 1000).try_into().map_err(|_| {
        invalid(
            "unturned",
            path,
            "The native blacklist clock exceeds its supported range.",
        )
    })?;
    let mut accounts = BTreeSet::new();
    for _ in 0..count {
        let player_id = cursor.u64()?;
        if version > 2 {
            cursor.take(4)?; // IP address.
        }
        cursor.take(8)?; // Judge account.
        let reason_length = usize::from(cursor.byte()?);
        // River.writeString clips UTF-8 bytes to 255 and can split a multibyte
        // character. The game accepts that payload with replacement decoding;
        // this projection consumes it without exposing or decoding the reason.
        cursor.take(reason_length)?;
        let duration = cursor.u32()?;
        let banned = cursor.u32()?;
        if version >= 4 {
            let hardware_count = i32::from_le_bytes(cursor.array()?);
            if hardware_count > MAX_ROWS as i32 {
                return Err(invalid(
                    "unturned",
                    path,
                    "A native blacklist hardware array exceeds its read limit.",
                ));
            }
            for _ in 0..hardware_count.max(0) {
                let length = usize::from(cursor.u16()?);
                cursor.take(length)?;
            }
        }
        // SteamBlacklistEntry.isExpired uses unsigned native u32 addition and
        // strict >. Duration zero is not a permanent-ban marker in this format.
        if now <= banned.wrapping_add(duration) {
            if player_id == 0 {
                return Err(invalid(
                    "unturned",
                    path,
                    "An active native blacklist record has no Steam account identity.",
                ));
            }
            accounts.insert(player_id.to_string());
        }
    }
    if cursor.position != bytes.len() {
        return Err(invalid(
            "unturned",
            path,
            "Native blacklist contains unsupported trailing data.",
        ));
    }
    Ok(accounts
        .into_iter()
        .map(|account_id| NativeBanEntry {
            account_id,
            display_name: None,
            identity_kind: RuntimePlayerIdentityKind::SteamId,
        })
        .collect())
}

#[cfg(test)]
#[path = "native_bans_unturned_tests.rs"]
mod tests;

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
    path: &'a Path,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], StorageError> {
        let end = self
            .position
            .checked_add(length)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| {
                invalid(
                    "unturned",
                    self.path,
                    "Native blacklist is truncated or malformed.",
                )
            })?;
        let bytes = &self.bytes[self.position..end];
        self.position = end;
        Ok(bytes)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], StorageError> {
        self.take(N)?.try_into().map_err(|_| {
            invalid(
                "unturned",
                self.path,
                "Native blacklist has an incomplete numeric field.",
            )
        })
    }

    fn byte(&mut self) -> Result<u8, StorageError> {
        Ok(self.array::<1>()?[0])
    }

    fn u16(&mut self) -> Result<u16, StorageError> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    fn u32(&mut self) -> Result<u32, StorageError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, StorageError> {
        Ok(u64::from_le_bytes(self.array()?))
    }
}
