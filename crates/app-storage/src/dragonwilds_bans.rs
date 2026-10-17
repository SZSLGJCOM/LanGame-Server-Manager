//! Read-only projection of the game's persisted KnownPlayerList. No setting or
//! online snapshot is authoritative for this roster, and no INI text is exposed.

use std::collections::HashSet;
#[cfg(test)]
use std::fs::{self, OpenOptions};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use app_core::{DragonwildsBanEntry, DragonwildsBanRoster};

use crate::StorageError;
use crate::native_bans_io::{decode as decode_native, read_optional_native_file};

const FILE: &str = "RSDragonwilds/Saved/Config/WindowsServer/DedicatedServer.ini";
const SECTION: &str = "[/Script/Dominion.DedicatedServerSettings]";
const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_ROWS: usize = 4096;

/// The caller resolves and authorizes the instance's actual runtime root. This
/// works before or after a restart and never loads or modifies the game runtime.
pub fn read_dragonwilds_ban_roster(
    runtime_root: &Path,
    instance_id: &str,
) -> Result<DragonwildsBanRoster, StorageError> {
    let path = runtime_root.join(FILE);
    if instance_id.trim().is_empty() {
        return Err(invalid(&path, "An instance identity is required."));
    }
    let content = read_optional_native_file(&path, "runescapedragonwilds", MAX_BYTES)?
        .ok_or_else(|| invalid(&path, "The native configuration is missing."))?;
    let entries = parse(
        &decode_native(&content, "runescapedragonwilds", &path)?,
        &path,
    )?;
    let observed_at_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid(&path, "The observation clock is unavailable."))?
        .as_millis()
        .try_into()
        .map_err(|_| {
            invalid(
                &path,
                "The observation timestamp is outside its supported range.",
            )
        })?;
    Ok(DragonwildsBanRoster {
        instance_id: instance_id.to_owned(),
        observed_at_unix_ms,
        entries,
    })
}

fn invalid(path: &Path, message: impl Into<String>) -> StorageError {
    StorageError::ModuleSupportMaterialization {
        module_id: "runescapedragonwilds".into(),
        path: path.to_path_buf(),
        message: message.into(),
    }
}

fn parse(content: &str, path: &Path) -> Result<Vec<DragonwildsBanEntry>, StorageError> {
    let mut sections = 0;
    let mut in_section = false;
    let mut accounts = HashSet::new();
    let mut entries = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_section = line == SECTION;
            sections += usize::from(in_section);
            continue;
        }
        if !in_section || line.is_empty() || line.starts_with([';', '#']) {
            continue;
        }
        let (key, value) = line.split_once('=').unwrap_or((line, ""));
        let key = key.trim();
        if !matches!(
            key,
            "KnownPlayerList" | "+KnownPlayerList" | "-KnownPlayerList" | "!KnownPlayerList"
        ) {
            continue;
        }
        if key != "KnownPlayerList" || accounts.len() >= MAX_ROWS {
            return Err(invalid(
                path,
                "The native KnownPlayerList has unsupported array syntax or exceeds 4096 records.",
            ));
        }
        let (account, body) = value
            .trim()
            .strip_prefix("(UserId=")
            .and_then(|value| value.split_once(','))
            .ok_or_else(|| invalid(path, "A native KnownPlayerList record is malformed."))?;
        if account.len() != 32
            || !account
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || !accounts.insert(account.to_owned())
        {
            return Err(invalid(
                path,
                "Native KnownPlayerList accounts must be unique 32-digit lowercase hexadecimal identities.",
            ));
        }
        let (middle, banned) = if let Some(middle) = body.strip_suffix(",bIsBanned=True)") {
            (middle, true)
        } else if let Some(middle) = body.strip_suffix(",bIsBanned=False)") {
            (middle, false)
        } else {
            return Err(invalid(
                path,
                "A native KnownPlayerList record lacks its final boolean ban flag.",
            ));
        };
        if banned {
            entries.push(DragonwildsBanEntry {
                account_id: account.to_owned(),
                display_name: display_name(middle),
            });
        }
    }
    if sections != 1 {
        return Err(invalid(
            path,
            "Exactly one native DedicatedServerSettings section is required.",
        ));
    }
    entries.sort_unstable_by(|left, right| left.account_id.cmp(&right.account_id));
    Ok(entries)
}

fn display_name(middle: &str) -> Option<String> {
    // Only the observed native shape is eligible for a name. Future fields,
    // assignments embedded in names, and credentials must never become labels.
    let (name, privileges) = middle
        .strip_prefix("UserName=")?
        .rsplit_once(",Privileges=(PrivilegeMask=")?;
    let mask = privileges.strip_suffix(')')?;
    if mask.is_empty() || !mask.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let name = if name.starts_with('"') {
        name.strip_prefix('"')?.strip_suffix('"')?
    } else {
        name
    };
    let lowercase = middle.to_ascii_lowercase();
    if name.trim().is_empty()
        || name.chars().count() > 256
        || name.contains(['=', '"', '\\'])
        || name.chars().any(char::is_control)
        || ["password", "credential", "secret", "token"]
            .iter()
            .any(|field| lowercase.contains(field))
    {
        return None;
    }
    Some(name.to_owned())
}

#[cfg(test)]
#[path = "dragonwilds_bans_tests.rs"]
mod tests;
