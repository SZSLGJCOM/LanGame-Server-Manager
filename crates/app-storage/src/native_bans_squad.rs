//! Official Squad Bans.cfg forms: SteamID:expiry, and the native administrative
//! prefix ending in ` [SteamID admin] Banned:SteamID:expiry`. Comments are not names.

use std::collections::BTreeMap;
use std::path::Path;

use app_core::{NativeBanEntry, RuntimePlayerIdentityKind};

use super::native_bans::{MAX_BYTES, MAX_ROWS};
use super::native_bans_io::{decode, invalid};
use super::native_bans_text::is_steam_id;
use crate::StorageError;

const MODULE: &str = "squad";

pub(super) fn parse(
    content: &str,
    path: &Path,
    observed_at_unix_ms: u64,
) -> Result<Vec<NativeBanEntry>, StorageError> {
    let mut accounts = BTreeMap::new();
    let mut rows = 0;
    for line in content.lines() {
        if let Some((account, expiry)) = record(line, path)? {
            rows += 1;
            if rows > MAX_ROWS {
                return Err(invalid(
                    MODULE,
                    path,
                    "Native ban list exceeds 4096 records.",
                ));
            }
            if expiry == 0 || expiry > observed_at_unix_ms / 1000 {
                accounts.insert(
                    account.to_owned(),
                    identity_kind(account).ok_or_else(|| {
                        invalid(
                            MODULE,
                            path,
                            "A native ban account identity is unsupported.",
                        )
                    })?,
                );
            }
        }
    }
    Ok(accounts
        .into_iter()
        .map(|(account_id, identity_kind)| NativeBanEntry {
            account_id,
            display_name: None,
            identity_kind,
        })
        .collect())
}

fn record<'a>(line: &'a str, path: &Path) -> Result<Option<(&'a str, u64)>, StorageError> {
    let core = line.split_once("//").map_or(line, |(core, _)| core).trim();
    if core.is_empty() {
        return Ok(None);
    }
    let account_expiry = if let Some((prefix, record)) = core.rsplit_once(" Banned:") {
        // The native writer's first %s is an opaque administrative label. It
        // may be Steam/EOS attribution, Console or RCON; it is never a target.
        if prefix.trim().is_empty() || prefix.chars().any(char::is_control) {
            return Err(invalid(
                MODULE,
                path,
                "A native administrative ban prefix is malformed.",
            ));
        }
        record
    } else {
        core
    };
    let (account, expiry) = account_expiry.split_once(':').ok_or_else(|| {
        invalid(
            MODULE,
            path,
            "A native ban record lacks its expiration timestamp.",
        )
    })?;
    let account = account.trim();
    let expiry = expiry.trim();
    if identity_kind(account).is_none()
        || expiry.is_empty()
        || !expiry.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(invalid(
            MODULE,
            path,
            "A native ban record has an unsupported account or expiration timestamp.",
        ));
    }
    let expiry = expiry.parse::<u64>().map_err(|_| {
        invalid(
            MODULE,
            path,
            "A native ban expiration timestamp is outside its supported range.",
        )
    })?;
    Ok(Some((account, expiry)))
}

fn is_eos_id(account: &str) -> bool {
    account.len() == 32 && account.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn identity_kind(account: &str) -> Option<RuntimePlayerIdentityKind> {
    if is_steam_id(account) {
        Some(RuntimePlayerIdentityKind::SteamId)
    } else if is_eos_id(account) {
        Some(RuntimePlayerIdentityKind::EosId)
    } else {
        None
    }
}

/// Pure edit plan used by the exclusive Squad writer. Decode validates the whole
/// input before removal. Retained lines keep their exact bytes, BOM and endings.
pub(crate) fn remove_squad_native_ban_lines(
    original: &[u8],
    account_id: &str,
    path: &Path,
) -> Result<Vec<u8>, StorageError> {
    if original.len() > MAX_BYTES || identity_kind(account_id).is_none() {
        return Err(invalid(
            MODULE,
            path,
            "Native ban data or the selected account identity is outside its supported range.",
        ));
    }
    let content = decode(original, MODULE, path)?;
    let mut retained = String::with_capacity(content.len());
    let mut removed = false;
    let mut rows = 0;
    for line in content.split_inclusive('\n') {
        let entry = record(line, path)?;
        if entry.is_some() {
            rows += 1;
            if rows > MAX_ROWS {
                return Err(invalid(
                    MODULE,
                    path,
                    "Native ban list exceeds 4096 records.",
                ));
            }
        }
        if entry.is_some_and(|(account, _)| account == account_id) {
            removed = true;
        } else {
            retained.push_str(line);
        }
    }
    if !removed {
        return Err(invalid(
            MODULE,
            path,
            "The selected account is no longer present in the native ban list; refresh it before retrying.",
        ));
    }
    if original.starts_with(&[0xff, 0xfe]) {
        let mut result = vec![0xff, 0xfe];
        result.extend(retained.encode_utf16().flat_map(u16::to_le_bytes));
        Ok(result)
    } else if original.starts_with(&[0xef, 0xbb, 0xbf]) {
        Ok([&[0xef, 0xbb, 0xbf][..], retained.as_bytes()].concat())
    } else {
        Ok(retained.into_bytes())
    }
}
