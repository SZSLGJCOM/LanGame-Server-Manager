//! Names from native saves supplement an already verified live RCON roster.
//! Never enumerate saved players to infer presence, or modify a running save.
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use app_core::{InstanceDetails, ark_maps::ArkMapProcess};

use crate::commands::commands_program_storage::instance_root;
use crate::live_players::ark_profile::{self, Edition, MAX_PROFILE_BYTES};

const MAX_COLLECTION_BYTES: usize = 16 * 1024 * 1024;

#[derive(Default)]
pub(super) struct ProfileNames {
    pub(super) names: HashMap<String, String>,
    pub(super) issue: Option<String>,
}

pub(super) async fn collect(
    details: &InstanceDetails,
    process_key: Option<&str>,
    accounts: Vec<String>,
) -> Result<ProfileNames, String> {
    let edition = match details.summary.module_id.as_str() {
        "arksurvivalevolved" => Edition::Ase,
        "arksurvivalascended" => Edition::Asa,
        _ => return Err("Native ARK profiles require an ARK instance.".into()),
    };
    let map = app_core::ark_maps::processes(details)?
        .into_iter()
        .find(|map| map.process_key == process_key.unwrap_or("main"))
        .ok_or("The player-query map is no longer enabled.")?;
    let instance_path = instance_root(details)?.to_owned();
    tokio::task::spawn_blocking(move || {
        let runtime = app_storage::resolve_instance_runtime_root(&instance_path)
            .map_err(|error| error.to_string())?;
        Ok(read_names(
            &profile_directory(&runtime, &map, edition),
            edition,
            &accounts,
        ))
    })
    .await
    .map_err(|error| format!("ARK profile name collection failed: {error}"))?
}

fn profile_directory(runtime: &Path, map: &ArkMapProcess, edition: Edition) -> PathBuf {
    let directory = runtime.join("ShooterGame/Saved").join(&map.save_directory);
    match edition {
        Edition::Asa => directory.join(&map.map_name),
        Edition::Ase => directory,
    }
}

fn read_names(directory: &Path, edition: Edition, accounts: &[String]) -> ProfileNames {
    let mut result = ProfileNames::default();
    let directory = match fs::canonicalize(directory) {
        Ok(directory) => directory,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return result,
        Err(error) => {
            result.issue = Some(format!(
                "Cannot read the current map's player profiles: {}",
                error.kind()
            ));
            return result;
        }
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut budget = MAX_COLLECTION_BYTES;
    for account in accounts.iter().take(512) {
        if result.names.contains_key(account) {
            continue;
        }
        if budget == 0 || Instant::now() >= deadline {
            result.issue =
                Some("Native profile name collection reached its bounded read budget.".into());
            break;
        }
        match read_name(&directory, edition, account, &mut budget) {
            Ok(Some(name)) => {
                result.names.insert(account.clone(), name);
            }
            Ok(None) => {}
            Err(error) => {
                result.issue.get_or_insert(error);
            }
        }
    }
    result
}

fn regular_file(file: &File) -> Result<fs::Metadata, String> {
    let metadata = file
        .metadata()
        .map_err(|error| format!("Cannot inspect a native player profile: {}", error.kind()))?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Native player profiles must not be reparse points.".into());
        }
    }
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("Native player profiles must be ordinary files.".into());
    }
    Ok(metadata)
}

fn read_name(
    directory: &Path,
    edition: Edition,
    account: &str,
    budget: &mut usize,
) -> Result<Option<String>, String> {
    // Account IDs come from RCON, not from a file picker. Restrict them before
    // joining a path; the parser also verifies the ID stored inside the file.
    let safe = match edition {
        Edition::Asa => account.len() == 32 && account.bytes().all(|byte| byte.is_ascii_hexdigit()),
        Edition::Ase => {
            matches!(account.len(), 17 | 19) && account.bytes().all(|byte| byte.is_ascii_digit())
        }
    };
    if !safe {
        return Err("The live account cannot identify a native ARK profile.".into());
    }
    let path = directory.join(format!("{account}.arkprofile"));
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err("Native player profile links are not followed.".into());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "Cannot inspect a native player profile: {}",
                error.kind()
            ));
        }
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let mut file = options
        .open(&path)
        .map_err(|error| format!("Cannot open a native player profile: {}", error.kind()))?;
    let before = regular_file(&file)?;
    let limit = MAX_PROFILE_BYTES.min(*budget);
    if before.len() > limit as u64 {
        return Err("A native player profile exceeds the bounded read budget.".into());
    }
    let mut bytes = Vec::with_capacity(before.len() as usize);
    (&mut file)
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Cannot read a native player profile: {}", error.kind()))?;
    *budget = budget.saturating_sub(bytes.len());
    let after = regular_file(&file)?;
    if bytes.len() > limit
        || bytes.len() as u64 != before.len()
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err("A native player profile changed while its name was being read.".into());
    }
    ark_profile::read_account_name(&bytes, account, edition).map_err(|error| error.to_string())
}

#[cfg(test)]
#[path = "commands_live_player_ark_profiles_tests.rs"]
mod tests;
