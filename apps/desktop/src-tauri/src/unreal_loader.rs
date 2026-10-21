//! Pinned UE4SS runtime preparation for the reviewed native world-save adapters.
//! Callers retain the stopped instance and independent-program permits through
//! `install`; this module never obtains or releases those application leases.
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

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

const ARCHIVE_URL: &str = "https://github.com/UE4SS-RE/RE-UE4SS/releases/download/experimental/UE4SS_v3.0.1-1125-g527a483b.zip";
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

pub(crate) fn supported(module_id: &str) -> bool {
    !matches!(module_id, "palworld" | "astroneer" | "humanitz") && Profile::get(module_id).is_ok()
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

/// Download only the fixed official asset using the application's bounded,
/// credential-free public-transfer policy. ZIP parsing stays off the UI thread.
pub(crate) async fn prepare(module_id: &str) -> Result<PreparedLoader, String> {
    let profile = Profile::get(module_id)?;
    let client = reqwest::Client::builder()
        .user_agent(concat!("LanGameServerManager/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| format!("Cannot prepare the UE4SS downloader: {error}"))?;
    let request = client
        .get(ARCHIVE_URL)
        .build()
        .map_err(|error| error.to_string())?;
    let response = app_network::read_public_bytes(
        &client,
        request,
        Duration::from_secs(180),
        ARCHIVE_BYTES,
        app_network::SourcePreference::InternationalFirst,
    )
    .await
    .map_err(|error| format!("Cannot download the fixed official UE4SS 1125 runtime: {error}"))?;
    tokio::task::spawn_blocking(move || archive::prepare(profile, &response.bytes))
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
