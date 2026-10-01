//! Installation of the pinned ARK tool dependency into an already stopped,
//! instance-owned private runtime. The command layer owns lifecycle admission.
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[path = "ark_tools_install_files.rs"]
mod files;
#[path = "ark_tools_install_transaction.rs"]
mod transaction;

const OWNER: &str = ".langame-ark-tools.json";
const PLUGIN: &str = "ArkApi/Plugins/LgsmArkTools/LgsmArkTools.dll";
const INFO: &str = "ArkApi/Plugins/LgsmArkTools/PluginInfo.json";
const NOTICES: &str = "ArkApi/Plugins/LgsmArkTools/THIRD_PARTY_NOTICES.txt";
const MAX_FILE: usize = 16 * 1024 * 1024;
const MAX_ARCHIVE: usize = 32 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(super) struct EmbeddedArkTools {
    pub(super) plugin: &'static [u8],
    pub(super) proxy: Option<&'static [u8]>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct InstallStatus {
    pub(super) ready: bool,
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
    url: &'static str,
    archive_hash: &'static str,
    archive_size: usize,
    payload: &'static [(&'static str, &'static str)],
}

const ASE: Profile = Profile {
    module_id: "arksurvivalevolved",
    executable: "ShooterGameServer.exe",
    version: "ArkApi 3.56",
    url: "https://github.com/ArkServerApi/AseApi/releases/download/3.56/ArkApi_3.56.zip",
    archive_hash: "169533afd6529dcdb6c58e5b0bfd13f5d90f593f0e1188e4c156104db53e84f6",
    archive_size: 11_003_472,
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
    version: "AsaApi 2.03",
    url: "https://github.com/ArkServerApi/AsaApi/releases/download/2.03/AsaApi_2.03.zip",
    archive_hash: "ac72fb29436198ac062cd273e1c496b1ef4e6ffddeec08243d11d9b35e8b8ae3",
    archive_size: 28_800_286,
    payload: &[
        (
            "ArkApi/AsaApi.dll",
            "7d12ff5fd238c4094da863d99651f3b54d7ffde1752eaa7cc263222cb7cd09d5",
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
    // The tool page discloses and obtains consent for ASA's executable-hash
    // request to these pinned upstream cache providers before preparation.
    let mut settings = serde_json::json!({
        "AutomaticPluginReloading": false,
        "AutomaticPluginReloadSeconds": 5,
        "SaveWorldBeforePluginReload": true
    });
    if profile.module_id == ASA.module_id {
        let proxy = embedded
            .proxy
            .ok_or("The embedded ASA loader is unavailable")?;
        dll(proxy)?;
        result.insert("version.dll".into(), proxy.to_vec());
        settings["AttachToParent"] = false.into();
        settings["AutomaticCacheDownload"] = serde_json::json!({
            "Enable": true,
            "DownloadCacheURL": "https://cdn.pelayori.com/cache/",
            "DownloadCacheURLs": ["https://cdn.pelayori.com/cache/", "https://cdn.shadowhunter.co.za/cache/", "https://cdn.shadowhunter-systems.co.za/cache/"]
        });
    }
    result.insert(
        "config.json".into(),
        serde_json::to_vec_pretty(&serde_json::json!({"settings":settings}))
            .map_err(|e| e.to_string())?,
    );
    Ok(result)
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

fn status(profile: &Profile, issue: Option<String>) -> InstallStatus {
    InstallStatus {
        ready: issue.is_none(),
        issue,
        framework_version: profile.version,
    }
}

pub(super) fn inspect(
    root: &Path,
    module_id: &str,
    embedded: EmbeddedArkTools,
) -> Result<InstallStatus, String> {
    let profile = profile(module_id)?;
    let wanted = expected(profile, embedded)?;
    let (bin, _pins) = files::runtime(root, profile.executable)?;
    if transaction::pending(&bin)? {
        return Ok(status(profile, Some("ARK preparation was interrupted; stop this instance and use the tool page to recover preparation".into())));
    }
    let owned = files::ownership(&bin, profile, &wanted)?;
    let Some(owned) = owned else {
        return Ok(status(
            profile,
            Some("Prepare the ARK server extension while this instance is stopped".into()),
        ));
    };
    for (path, hash) in &wanted {
        let actual = files::read(&bin, path)?;
        if actual.as_ref().map(|b| digest(b)).as_ref() != Some(hash) {
            return Ok(status(
                profile,
                Some(format!("ARK extension preparation is required: {path}")),
            ));
        }
    }
    if owned.files != wanted {
        return Ok(status(
            profile,
            Some("Update the ARK server extension while this instance is stopped".into()),
        ));
    }
    Ok(status(profile, None))
}

pub(super) async fn install(
    root: PathBuf,
    module_id: String,
    embedded: EmbeddedArkTools,
) -> Result<InstallStatus, String> {
    let profile = profile(&module_id)?;
    // Fail before networking if an unowned loader or modified configuration
    // would be overwritten. The command keeps lifecycle admission throughout.
    let wanted = expected(profile, embedded)?;
    let check_root = root.clone();
    let check_wanted = wanted.clone();
    let already_ready = tokio::task::spawn_blocking(move || {
        let (bin, _pins) = files::runtime(&check_root, profile.executable)?;
        transaction::recover(&bin, profile, &check_wanted)?;
        let owned = files::ownership(&bin, profile, &check_wanted)?;
        files::preflight(&bin, profile, &check_wanted, owned.as_ref())?;
        inspect(&check_root, profile.module_id, embedded).map(|s| s.ready)
    })
    .await
    .map_err(|e| format!("ARK extension inspection task failed: {e}"))??;
    if already_ready {
        return Ok(status(profile, None));
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let request = client.get(profile.url).build().map_err(|e| e.to_string())?;
    let response = app_network::read_public_bytes(
        &client,
        request,
        Duration::from_secs(90),
        MAX_ARCHIVE,
        app_network::SourcePreference::InternationalFirst,
    )
    .await
    .map_err(|e| format!("ARK dependency download failed: {e}"))?;
    let bytes = response.bytes;
    tokio::task::spawn_blocking(move || {
        let mut payload = unpack(profile, &bytes)?;
        payload.extend(embedded_files(profile, embedded)?);
        publish(&root, profile, payload, &wanted)?;
        inspect(&root, &module_id, embedded)
    })
    .await
    .map_err(|e| format!("ARK extension preparation task failed: {e}"))?
}

fn unpack(profile: &Profile, bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, String> {
    if bytes.len() != profile.archive_size || digest(bytes) != profile.archive_hash {
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
        let name = entry.name().to_string();
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
    transaction::publish(root, profile, payload, wanted, before_publish)
}

pub(super) fn ensure_can_start(root: &Path) -> Result<(), String> {
    let (bin, _pins) = files::runtime_directory(root)?;
    if transaction::pending(&bin)? {
        return Err("ARK preparation was interrupted; keep this instance stopped and use the ARK tool page to recover preparation before starting".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "ark_tools_install_tests.rs"]
mod tests;
