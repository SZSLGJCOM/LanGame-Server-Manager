//! Installation of the pinned ARK tool dependency into an already stopped,
//! instance-owned private runtime. The command layer owns lifecycle admission.
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};

use super::progress::{InstallPhase, InstallReporter, InstallUpdate};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[path = "ark_tools_download.rs"]
mod download;
#[path = "ark_tools_install_files.rs"]
mod files;
#[path = "ark_tools_uninstall.rs"]
mod removal;
#[path = "ark_tools_startup.rs"]
mod startup;
#[path = "ark_tools_symbols.rs"]
mod symbols;
#[path = "ark_tools_install_transaction.rs"]
mod transaction;

const OWNER: &str = ".langame-ark-tools.json";
const PLUGIN: &str = "ArkApi/Plugins/LgsmArkTools/LgsmArkTools.dll";
const INFO: &str = "ArkApi/Plugins/LgsmArkTools/PluginInfo.json";
const NOTICES: &str = "ArkApi/Plugins/LgsmArkTools/THIRD_PARTY_NOTICES.txt";
const MAX_FILE: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(super) struct EmbeddedArkTools {
    pub(super) plugin: &'static [u8],
    pub(super) proxy: Option<&'static [u8]>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct InstallStatus {
    // Installed files are intact; runtime protocol compatibility is checked by RCON.
    pub(super) ready: bool,
    pub(super) matches_bundle: bool,
    pub(super) can_uninstall: bool,
    pub(super) issue: Option<String>,
    pub(super) framework_version: &'static str,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Ownership {
    schema: u32,
    module_id: String,
    files: BTreeMap<String, String>,
}

struct Profile {
    module_id: &'static str,
    executable: &'static str,
    version: &'static str,
    payload: &'static [(&'static str, &'static str)],
}

const ASE: Profile = Profile {
    module_id: "arksurvivalevolved",
    executable: "ShooterGameServer.exe",
    version: "ArkApi 3.56",
    payload: &[
        (
            "version.dll",
            "08e639665a26496dc237266330af58a567098c29e01208641696766b46da5626",
        ),
        (
            "msdia140.dll",
            "26295cdc39ec335323a74ee2c5d3df238926cd4e4a53aa39cfc15232165262f2",
        ),
    ],
};
const ASA: Profile = Profile {
    module_id: "arksurvivalascended",
    executable: "ArkAscendedServer.exe",
    version: "AsaApi 2.11",
    payload: &[
        (
            "ArkApi/AsaApi.dll",
            "f7fed1b8e6921a5f63eda1f83369fc147c56c5bdd3c74b122242bc636a3e75a4",
        ),
        (
            "ArkApi/pdbignores.txt",
            "205fa9887e0f55d66e90e37dff82cd924b2019b9fce07e353260998a088352cc",
        ),
        (
            "msdia140.dll",
            "60a613fa7cbd96cac5312d07fbb97492a7a36763bf84a5f41e077faf479de807",
        ),
        (
            "libcrypto-3-x64.dll",
            "f77f1976a89a56511f90cb24e00e52a4c44d06529de72c09252dce09ccdc9833",
        ),
        (
            "libssl-3-x64.dll",
            "e506c6335ba2770fafe15a0f17f711a9d076cf14b89ef8a69dba6a1f43ab1822",
        ),
    ],
};

fn profile(module_id: &str) -> Result<&'static Profile, String> {
    match module_id {
        "arksurvivalevolved" => Ok(&ASE),
        "arksurvivalascended" => Ok(&ASA),
        _ => Err("ARK tools only support ASE and ASA private runtimes".into()),
    }
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn embedded_files(
    profile: &Profile,
    embedded: EmbeddedArkTools,
) -> Result<BTreeMap<String, Vec<u8>>, String> {
    fn dll(bytes: &[u8]) -> Result<(), String> {
        if bytes.len() > MAX_FILE || bytes.len() < 64 || &bytes[..2] != b"MZ" {
            return Err("The embedded ARK extension is missing or invalid".into());
        }
        let offset = u32::from_le_bytes(bytes[60..64].try_into().unwrap()) as usize;
        if bytes.get(offset..offset.saturating_add(6)) != Some(b"PE\0\0\x64\x86") {
            return Err("The embedded ARK extension must be a Windows x64 image".into());
        }
        Ok(())
    }
    dll(embedded.plugin)?;
    let mut result = BTreeMap::from([
        (PLUGIN.into(), embedded.plugin.to_vec()),
        (
            INFO.into(),
            include_bytes!("../../../../modules/ark-tools/PluginInfo.json").to_vec(),
        ),
        (
            NOTICES.into(),
            include_bytes!("../../../../modules/ark-tools/THIRD_PARTY_NOTICES.txt").to_vec(),
        ),
    ]);
    if profile.module_id == ASA.module_id {
        let proxy = embedded
            .proxy
            .ok_or("The embedded ASA loader is unavailable")?;
        dll(proxy)?;
        result.insert("version.dll".into(), proxy.to_vec());
    }
    result.insert("config.json".into(), managed_configuration(profile, false)?);
    Ok(result)
}

fn managed_configuration(profile: &Profile, automatic_download: bool) -> Result<Vec<u8>, String> {
    let mut settings = serde_json::json!({
        "AutomaticPluginReloading": false,
        "AutomaticPluginReloadSeconds": 5,
        "SaveWorldBeforePluginReload": true
    });
    if profile.module_id == ASA.module_id {
        settings["AttachToParent"] = false.into();
        settings["AutomaticCacheDownload"] = serde_json::json!({
            "Enable": automatic_download,
            "DownloadCacheURL": "https://cdn.pelayori.com/cache/",
            "DownloadCacheURLs": ["https://cdn.pelayori.com/cache/", "https://cdn.shadowhunter.co.za/cache/", "https://cdn.shadowhunter-systems.co.za/cache/"]
        });
    }
    serde_json::to_vec_pretty(&serde_json::json!({"settings":settings})).map_err(|e| e.to_string())
}

fn expected(
    profile: &Profile,
    embedded: EmbeddedArkTools,
) -> Result<BTreeMap<String, String>, String> {
    let mut result: BTreeMap<String, String> = profile
        .payload
        .iter()
        .map(|(p, h)| ((*p).into(), (*h).into()))
        .collect();
    result.extend(
        embedded_files(profile, embedded)?
            .into_iter()
            .map(|(p, b)| (p, digest(&b))),
    );
    Ok(result)
}

fn status(profile: &Profile, issue: Option<String>, can_uninstall: bool) -> InstallStatus {
    InstallStatus {
        ready: issue.is_none(),
        matches_bundle: false,
        can_uninstall,
        issue,
        framework_version: profile.version,
    }
}

pub(super) fn inspect(
    root: &Path,
    module_id: &str,
    embedded: EmbeddedArkTools,
) -> Result<InstallStatus, String> {
    inspect_profile(root, profile(module_id)?, embedded)
}

fn inspect_profile(
    root: &Path,
    profile: &Profile,
    embedded: EmbeddedArkTools,
) -> Result<InstallStatus, String> {
    let wanted = expected(profile, embedded)?;
    let (bin, _pins) = files::runtime(root, profile.executable)?;
    let removing = removal::pending(&bin)?;
    if removing || transaction::pending(&bin)? {
        return Ok(status(profile, Some("ARK preparation or removal was interrupted; stop this instance and use the tool page to recover".into()), removing));
    }
    let owned = files::ownership(&bin, profile, &wanted)?;
    let Some(owned) = owned else {
        return Ok(status(
            profile,
            Some("Prepare the ARK server extension while this instance is stopped".into()),
            false,
        ));
    };
    let can_uninstall = removal::available(&bin, &owned)?;
    if !can_uninstall {
        return Ok(status(
            profile,
            Some("Prepare the ARK server extension while this instance is stopped".into()),
            false,
        ));
    }
    // A different manager build must not erase a valid installation. Ownership
    // still pins dependencies/configuration; verify installed plugin bytes against
    // their recorded hashes and leave compatibility to the runtime handshake.
    for (path, hash) in &owned.files {
        let actual = files::read(&bin, path)?;
        if actual.as_ref().map(|b| digest(b)).as_ref() != Some(hash) {
            return Ok(status(
                profile,
                Some(format!("ARK extension preparation is required: {path}")),
                can_uninstall,
            ));
        }
    }
    Ok(InstallStatus {
        matches_bundle: owned.files == wanted,
        ..status(profile, None, can_uninstall)
    })
}

pub(super) fn uninstall(
    root: &Path,
    module_id: &str,
    embedded: EmbeddedArkTools,
) -> Result<bool, String> {
    let profile = profile(module_id)?;
    let wanted = expected(profile, embedded)?;
    removal::remove(root, profile, &wanted)
}

pub(super) async fn install(
    root: PathBuf,
    module_id: String,
    embedded: EmbeddedArkTools,
    preference: app_network::SourcePreference,
    report: InstallReporter,
) -> Result<InstallStatus, String> {
    let profile = profile(&module_id)?;
    let package = download::package(&module_id)?;
    report(InstallUpdate {
        phase: InstallPhase::Checking,
        source: None,
        downloaded_bytes: None,
        total_bytes: None,
    });
    // Fail before networking if an unowned loader or modified configuration
    // would be overwritten. The command keeps lifecycle admission throughout.
    let wanted = expected(profile, embedded)?;
    let check_root = root.clone();
    let check_wanted = wanted.clone();
    let inspected = tokio::task::spawn_blocking(move || {
        let (bin, _pins) = files::runtime(&check_root, profile.executable)?;
        removal::recover_all(&bin, profile, &check_wanted)?;
        let owned = files::ownership(&bin, profile, &check_wanted)?;
        files::preflight(&bin, profile, &check_wanted, owned.as_ref())?;
        inspect(&check_root, profile.module_id, embedded)
    })
    .await
    .map_err(|e| format!("ARK extension inspection task failed: {e}"))??;
    // Symbol acquisition is an explicit stopped-instance operation, including
    // re-entry after the game executable changes with an intact plugin bundle.
    let symbols = if profile.module_id == ASA.module_id {
        Some(symbols::prepare(root.clone(), report.clone()).await?)
    } else {
        None
    };
    // Explicit preparation can still replace an intact previous build. Only an
    // intact installation of this exact bundle can skip publication.
    if inspected.matches_bundle {
        if let Some(symbols) = symbols {
            tokio::task::spawn_blocking(move || symbols.publish())
                .await
                .map_err(|error| format!("ASA symbol publication task failed: {error}"))??;
        }
        return Ok(inspected);
    }
    let bytes = download::fetch(package, preference, &report).await?;
    tokio::task::spawn_blocking(move || {
        let mut payload = unpack(profile, package, &bytes)?;
        payload.extend(embedded_files(profile, embedded)?);
        report(InstallUpdate {
            phase: InstallPhase::Installing,
            source: None,
            downloaded_bytes: None,
            total_bytes: None,
        });
        publish(&root, profile, payload, &wanted)?;
        if let Some(symbols) = symbols {
            symbols.publish()?;
        }
        inspect(&root, &module_id, embedded)
    })
    .await
    .map_err(|e| format!("ARK extension preparation task failed: {e}"))?
}

fn unpack(
    profile: &Profile,
    package: &download::Package,
    bytes: &[u8],
) -> Result<BTreeMap<String, Vec<u8>>, String> {
    if bytes.len() != package.size || digest(bytes) != package.sha256 {
        return Err("The ARK dependency archive does not match the pinned release checksum".into());
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    if zip.len() > 128 {
        return Err("ARK dependency archive has too many entries".into());
    }
    let mut result = BTreeMap::new();
    let mut names = std::collections::HashSet::new();
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|e| e.to_string())?;
        let name = crate::archive_entry::name(&entry)
            .map_err(|error| {
                format!("Invalid ARK dependency entry name at index {index}: {error}")
            })?
            .into_owned();
        // Validate even ignored entries; never follow archive-supplied paths.
        if !files::safe_relative(name.trim_end_matches('/'))
            || entry.is_symlink()
            || !names.insert(name.to_ascii_lowercase())
        {
            return Err("ARK dependency archive contains an unsafe or duplicate path".into());
        }
        let Some((_, hash)) = profile.payload.iter().find(|(p, _)| *p == name) else {
            continue;
        };
        if entry.is_dir()
            || entry.size() > MAX_FILE as u64
            || entry
                .unix_mode()
                .is_some_and(|m| m & 0o170000 != 0 && m & 0o170000 != 0o100000)
        {
            return Err(format!("Invalid ARK dependency entry: {name}"));
        }
        let mut content = Vec::new();
        entry
            .by_ref()
            .take((MAX_FILE + 1) as u64)
            .read_to_end(&mut content)
            .map_err(|e| e.to_string())?;
        if content.len() > MAX_FILE || digest(&content) != *hash {
            return Err(format!("ARK dependency entry checksum failed: {name}"));
        }
        result.insert(name, content);
    }
    if result.len() != profile.payload.len() {
        return Err("ARK dependency archive is incomplete".into());
    }
    Ok(result)
}

fn publish(
    root: &Path,
    profile: &Profile,
    mut payload: BTreeMap<String, Vec<u8>>,
    wanted: &BTreeMap<String, String>,
) -> Result<(), String> {
    publish_with(root, profile, &mut payload, wanted, |_| Ok(()))
}

fn publish_with(
    root: &Path,
    profile: &Profile,
    payload: &mut BTreeMap<String, Vec<u8>>,
    wanted: &BTreeMap<String, String>,
    before_publish: impl Fn(usize) -> Result<(), String>,
) -> Result<(), String> {
    let (bin, _pins) = files::runtime(root, profile.executable)?;
    removal::recover_all(&bin, profile, wanted)?;
    transaction::publish(root, profile, payload, wanted, before_publish)
}

pub(super) fn ensure_can_start(root: &Path) -> Result<(), String> {
    let (bin, _pins) = files::runtime_directory(root)?;
    if transaction::pending(&bin)? || removal::pending(&bin)? {
        return Err("ARK preparation or removal was interrupted; keep this instance stopped and use the ARK tool page to recover before starting".into());
    }
    Ok(())
}

pub(super) fn refresh_owned_asa_proxy(
    root: &Path,
    embedded: EmbeddedArkTools,
) -> Result<(), String> {
    startup::refresh(root, embedded)
}

#[cfg(test)]
#[path = "ark_tools_install_tests.rs"]
mod tests;
