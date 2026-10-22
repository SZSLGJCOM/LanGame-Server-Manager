//! Pinned UE4SS runtime preparation for the reviewed native world-save adapters.
//! Callers retain the stopped instance and independent-program permits through
//! `install`; this module never obtains or releases those application leases.
use std::collections::BTreeMap;
use std::path::Path;

#[path = "unreal_loader/archive.rs"]
mod archive;
#[path = "unreal_loader/installation.rs"]
mod installation;
#[path = "unreal_loader/profile.rs"]
mod profile;

use crate::live_players::file_ipc::unreal_world_save::installation::{
    exists, hash, pin, publish, read,
};
use profile::Profile;
pub(crate) use profile::compatible_settings;

const ARCHIVE_HASH: &str = "4f9762f812329a640c8cfa14444c2bb97ecc213b8320bd4c5433383c3eef48f7";
const ARCHIVE_BYTES: usize = 8_718_500;
const CORE_HASH: &str = "91d5444f41d19d0bb502f0fbdc15d76421e20359e898caa39e41e134c8f87958";
const FILE_LIMIT: usize = 32 * 1024 * 1024;

/// Immutable, validated runtime bytes. No arbitrary URL, destination or package
/// may enter the publication API through this value.
pub(crate) struct PreparedLoader {
    module_id: String,
    files: BTreeMap<String, Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstallOutcome {
    Installed,
    Reused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartupState {
    Ready,
    NeedsInstall,
    Unavailable,
}

pub(crate) fn supported(module_id: &str) -> bool {
    Profile::get(module_id).is_ok()
}

/// Validate an executable and return the paired native provider hash, if any.
pub(crate) fn verify_game_image(
    module_id: &str,
    actual: &str,
) -> Result<Option<&'static str>, String> {
    Profile::get(module_id)?.verify_game_image(actual)
}

#[cfg(test)]
pub(crate) fn diagnostic_log_path(root: &Path, module_id: &str) -> Option<std::path::PathBuf> {
    Profile::get(module_id)
        .ok()
        .map(|profile| profile.win64(root).join("ue4ss/UE4SS.log"))
}

/// Read-only admission distinguishes missing preparation from incompatible
/// files. A conflict is an error and must not be treated as permission to replace.
pub(crate) fn installed(root: &Path, module_id: &str) -> Result<bool, String> {
    installation::installed(root, Profile::get(module_id)?)
}

/// Inspect a stopped private program. An unsupported save extension may be
/// disabled without disabling the native server or an operator's other mods.
pub(crate) fn prepare_startup(root: &Path, module_id: &str) -> Result<StartupState, String> {
    installation::prepare_startup(root, Profile::get(module_id)?)
}

pub(crate) fn bundled_archive() -> Result<&'static [u8], String> {
    #[cfg(windows)]
    {
        Ok(include_bytes!(concat!(
            env!("OUT_DIR"),
            "/ue4ss-runtime/UE4SS.zip"
        )))
    }
    #[cfg(not(windows))]
    {
        Err("The bundled UE4SS runtime requires Windows.".into())
    }
}

/// The Windows build embeds the hash-pinned official release. First startup
/// requires no download; ZIP validation remains off the UI thread.
pub(crate) async fn prepare(module_id: &str) -> Result<PreparedLoader, String> {
    let profile = Profile::get(module_id)?;
    let bytes = bundled_archive()?;
    tokio::task::spawn_blocking(move || archive::prepare(profile, bytes))
        .await
        .map_err(|error| format!("UE4SS package preparation did not finish: {error}"))?
}

pub(crate) fn install(
    root: &Path,
    module_id: &str,
    prepared: &PreparedLoader,
) -> Result<InstallOutcome, String> {
    let profile = Profile::get(module_id)?;
    if prepared.module_id != module_id {
        return Err("The prepared UE4SS runtime belongs to another game.".into());
    }
    installation::install(root, profile, &prepared.files)
}

#[cfg(test)]
#[path = "unreal_loader/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "unreal_loader/startup_tests.rs"]
mod startup_tests;
