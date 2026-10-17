//! Read-only projections of game-owned ban persistence. No application setting
//! or online-player snapshot is used to reconstruct or rewrite these files.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use app_core::{NativeBanEntry, NativeBanRoster, RuntimePlayerIdentityKind};

use crate::StorageError;
use crate::instance_file_patch::io::guard_directories;
use crate::native_bans_io::{decode, invalid, read_optional_native_file};
pub(crate) use crate::native_bans_squad::remove_squad_native_ban_lines;

pub(super) const MAX_BYTES: usize = 8 * 1024 * 1024;
pub(super) const MAX_ROWS: usize = 4096;

/// Callers resolve and authorize both persisted instance roots. The instance
/// root is distinct from the actual program runtime for games such as Necesse.
pub fn read_native_ban_roster(
    runtime_root: &Path,
    instance_root: &Path,
    module_id: &str,
    instance_id: &str,
) -> Result<NativeBanRoster, StorageError> {
    if instance_id.trim().is_empty() || instance_id.chars().any(char::is_control) {
        return Err(invalid(
            module_id,
            runtime_root,
            "An instance identity is required.",
        ));
    }
    let _runtime_guards = guard_directories(runtime_root)?;
    let _instance_guards = if module_id == "necesse" {
        Some(guard_directories(instance_root)?)
    } else {
        None
    };
    let observed_at_unix_ms: u64 = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| {
            invalid(
                module_id,
                runtime_root,
                "The observation clock is unavailable.",
            )
        })?
        .as_millis()
        .try_into()
        .map_err(|_| {
            invalid(
                module_id,
                runtime_root,
                "The observation timestamp is outside its supported range.",
            )
        })?;
    let entries = read_entries(
        runtime_root,
        instance_root,
        module_id,
        instance_id,
        observed_at_unix_ms,
    )?;
    Ok(NativeBanRoster {
        instance_id: instance_id.to_owned(),
        module_id: module_id.to_owned(),
        observed_at_unix_ms,
        entries,
    })
}

#[cfg(test)]
#[path = "native_bans_tests.rs"]
mod tests;

fn read_entries(
    runtime_root: &Path,
    instance_root: &Path,
    module_id: &str,
    instance_id: &str,
    observed_at_unix_ms: u64,
) -> Result<Vec<NativeBanEntry>, StorageError> {
    match module_id {
        "soulmask"
        | "necesse"
        | "squad"
        | "conanexiles"
        | "palworld"
        | "arksurvivalascended"
        | "arksurvivalevolved" => {
            let path = match module_id {
                "soulmask" => runtime_root.join("WS/Saved/BlackAccountList.txt"),
                "necesse" => instance_root.join("data/cfg/banned.cfg"),
                "conanexiles" => runtime_root.join("ConanSandbox/Saved/blacklist.txt"),
                "palworld" => runtime_root.join("Pal/Saved/SaveGames/banlist.txt"),
                // Native Windows defaults to BanList.txt beside the server.
                // ASA and ASE have different persisted CSV record formats.
                "arksurvivalascended" | "arksurvivalevolved" => {
                    runtime_root.join("ShooterGame/Binaries/Win64/BanList.txt")
                }
                _ => runtime_root.join("SquadGame/ServerConfig/Bans.cfg"),
            };
            let Some(bytes) = read_optional_native_file(&path, module_id, MAX_BYTES)? else {
                return Ok(Vec::new());
            };
            let content = if module_id == "necesse" {
                // The current JVM launch fixes UTF-8; native FileReader does not
                // remove a BOM and the native writer never emits one.
                if bytes.starts_with(&[0xef, 0xbb, 0xbf]) || bytes.starts_with(&[0xff, 0xfe]) {
                    return Err(invalid(
                        module_id,
                        &path,
                        "Native Necesse ban data must be UTF-8 without a byte-order mark.",
                    ));
                }
                String::from_utf8(bytes).map_err(|_| {
                    invalid(
                        module_id,
                        &path,
                        "Native Necesse ban data is not valid UTF-8.",
                    )
                })?
            } else {
                decode(&bytes, module_id, &path)?
            };
            if module_id == "squad" {
                crate::native_bans_squad::parse(&content, &path, observed_at_unix_ms)
            } else {
                crate::native_bans_text::parse(&content, module_id, &path, observed_at_unix_ms)
            }
        }
        "rimworld" => crate::native_bans_rimworld::read(runtime_root),
        "unturned" => {
            if instance_id.contains(['/', '\\', ':']) || instance_id == "." || instance_id == ".." {
                return Err(invalid(
                    module_id,
                    runtime_root,
                    "The native server identity must be a single path component.",
                ));
            }
            let path = runtime_root
                .join("Servers")
                .join(instance_id)
                .join("Server/Blacklist.dat");
            let Some(bytes) = read_optional_native_file(&path, module_id, MAX_BYTES)? else {
                return Ok(Vec::new());
            };
            crate::native_bans_unturned::parse(&bytes, &path, observed_at_unix_ms)
        }
        "runescapedragonwilds" => Ok(crate::read_dragonwilds_ban_roster(
            runtime_root,
            instance_id,
        )?
        .entries
        .into_iter()
        .map(|entry| NativeBanEntry {
            account_id: entry.account_id,
            display_name: entry.display_name,
            identity_kind: RuntimePlayerIdentityKind::EosId,
        })
        .collect()),
        _ => Err(invalid(
            module_id,
            runtime_root,
            "No verified native ban reader is available for this module.",
        )),
    }
}
