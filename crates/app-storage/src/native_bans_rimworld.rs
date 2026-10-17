//! RimWorld Together 26.8.31.1 persists FL_Player JSON directly in Assets/Users.
//! Only Username and IsBanned are deserialized; account credentials and IP data
//! are never included in the returned roster or diagnostic messages.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use app_core::{NativeBanEntry, RuntimePlayerIdentityKind};
use serde::Deserialize;

use super::native_bans::MAX_ROWS;
use super::native_bans_io::{decode, guard_optional_directory, invalid, read_optional_native_file};
use crate::StorageError;
use crate::instance_file_patch::io::is_link;

const MODULE: &str = "rimworld";
const MAX_PLAYER_BYTES: usize = 1024 * 1024;
const MAX_ROSTER_BYTES: usize = 32 * 1024 * 1024;

#[derive(Deserialize)]
struct Player {
    #[serde(rename = "Username")]
    username: String,
    #[serde(rename = "IsBanned")]
    is_banned: bool,
}

pub(super) fn read(runtime_root: &Path) -> Result<Vec<NativeBanEntry>, StorageError> {
    let directory = runtime_root.join("Assets/Users");
    let Some(_guards) = guard_optional_directory(&directory, MODULE)? else {
        return Ok(Vec::new());
    };
    let before =
        fs::metadata(&directory).map_err(|error| invalid(MODULE, &directory, error.to_string()))?;
    let listing =
        fs::read_dir(&directory).map_err(|error| invalid(MODULE, &directory, error.to_string()))?;
    let mut accounts = BTreeMap::new();
    let mut total_bytes = 0;
    for (index, child) in listing.enumerate() {
        if index >= MAX_ROWS {
            return Err(invalid(
                MODULE,
                &directory,
                "Native player directory exceeds 4096 entries.",
            ));
        }
        let path = child
            .map_err(|error| invalid(MODULE, &directory, error.to_string()))?
            .path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| invalid(MODULE, &path, error.to_string()))?;
        if is_link(&metadata) {
            return Err(invalid(
                MODULE,
                &path,
                "Native player records cannot be links or reparse points.",
            ));
        }
        if metadata.is_dir() {
            continue;
        }
        // GetAllUserFiles uses Directory.GetFiles without an extension filter.
        // Read every direct regular file as the native server does.
        let bytes =
            read_optional_native_file(&path, MODULE, MAX_PLAYER_BYTES)?.ok_or_else(|| {
                invalid(
                    MODULE,
                    &path,
                    "A native player record disappeared during observation.",
                )
            })?;
        total_bytes += bytes.len();
        if total_bytes > MAX_ROSTER_BYTES {
            return Err(invalid(
                MODULE,
                &directory,
                "Native player records exceed the 32 MiB total read limit.",
            ));
        }
        let content = decode(&bytes, MODULE, &path)?;
        let player: Player = serde_json::from_str(&content).map_err(|_| {
            invalid(
                MODULE,
                &path,
                "A native player record has malformed Username or IsBanned JSON fields.",
            )
        })?;
        if player.username.trim().is_empty()
            || player.username != player.username.trim()
            || player.username.chars().count() > 256
            || player.username.chars().any(char::is_control)
        {
            return Err(invalid(
                MODULE,
                &path,
                "A native player record has an unsupported account name.",
            ));
        }
        if accounts.insert(player.username, player.is_banned).is_some() {
            return Err(invalid(
                MODULE,
                &directory,
                "Native player records contain ambiguous duplicate account names.",
            ));
        }
    }
    let after =
        fs::metadata(&directory).map_err(|error| invalid(MODULE, &directory, error.to_string()))?;
    if before.modified().ok() != after.modified().ok() {
        return Err(invalid(
            MODULE,
            &directory,
            "The native player directory changed during observation.",
        ));
    }
    Ok(accounts
        .into_iter()
        .filter_map(|(account_id, banned)| {
            banned.then(|| NativeBanEntry {
                display_name: Some(account_id.clone()),
                account_id,
                identity_kind: RuntimePlayerIdentityKind::PlayerName,
            })
        })
        .collect())
}
