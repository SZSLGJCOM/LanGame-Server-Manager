//! Install the pinned read-only bridge before the dedicated server can load it.
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const WIN64: &str = "RSDragonwilds/Binaries/Win64";
const LOADER: &str = "ue4ss";
const STAGE: &str = ".langame-player-query-staging";
const INTENT: &str = ".langame-dragonwilds-player-query-intent.json";
const RECEIPT: &str = ".langame-dragonwilds-player-query.json";
const PROXY_COPY: &str = "LgsmPlayerQueryProxy.bin";
// The reviewed source manifest identifies compatibility. PE build timestamps
// and toolchain output hashes may differ between application builds.
const PROXY_SOURCE_REVISION: &str =
    "7d2a03749c570614b0fb88705c0d89b4134ff608e6affb6be7d639fa307b2f35";
pub(super) const ARCHIVE_BYTES: u64 = 8_718_500;
const ARCHIVE_SHA: &str = "4f9762f812329a640c8cfa14444c2bb97ecc213b8320bd4c5433383c3eef48f7";
const SERVER_SHAS: &[&str] = &[
    "89ed65335a46db067a361d35ae4e6be80857a4c1f2453510600d632879dfbeed",
    "da1761a644a9d9b19da0c2da86ad98c7c4ce8ff265f710bf1897e3b007d2dcbe",
    "ecaa5ebc6e0b08108856703a4aceed7b5cf0ea04d708b82dfc66ef74709a4269",
];
const MAX_FILE: usize = 24 * 1024 * 1024;
const MAX_PROXY: usize = 4 * 1024 * 1024;
const MAX_ENCODED_PROXY: usize = MAX_PROXY.div_ceil(3) * 4;
const PROFILE: &[u8] =
    include_bytes!("../../../../modules/runescapedragonwilds/extensions/UE4SS-settings.ini");
const SCRIPT: &[u8] = include_bytes!(
    "../../../../modules/runescapedragonwilds/extensions/LgsmPlayerQuery/Scripts/main.lua"
);
const RELEASE: &[(&str, usize, &str)] = &[
    (
        "UE4SS.dll",
        20_567_040,
        "91d5444f41d19d0bb502f0fbdc15d76421e20359e898caa39e41e134c8f87958",
    ),
    (
        "LICENSE",
        1085,
        "99e1d8f8b7cb68e8b2313380ae3b0c8115e39dfbc443fcf21fd67ab74ce1d57e",
    ),
    (
        "UE4SS_SDK_Backends/UE4SS.json",
        8032,
        "6b8c1725ff59a318b812d5952f63107778b4ceb22b882b297a3666ee9e06c8ff",
    ),
];

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Preparation {
    Ready,
    NeedsInstall,
    ExternalLoader,
    UnsupportedBuild,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ownership {
    schema: u32,
    archive_sha256: String,
    proxy_source_revision: String,
    proxy_sha256: String,
    files: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pending_proxy_base64: Option<String>,
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}

fn exists(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("Cannot inspect {}: {error}", path.display())),
    }
}

fn open_regular(path: &Path, maximum: u64) -> Result<File, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("Cannot inspect {}: {error}", path.display()))?;
    if !metadata.is_file() || reparse(&metadata) || metadata.len() > maximum {
        return Err(format!(
            "Expected a bounded ordinary file: {}",
            path.display()
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1).custom_flags(0x0020_0000);
    }
    let file = options
        .open(path)
        .map_err(|error| format!("Cannot read {}: {error}", path.display()))?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || reparse(&metadata) || metadata.len() > maximum {
        return Err(format!(
            "Expected a bounded ordinary file: {}",
            path.display()
        ));
    }
    Ok(file)
}

fn read(path: &Path, maximum: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    open_regular(path, maximum as u64)?
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > maximum {
        return Err("Player-query file exceeded its read limit".into());
    }
    Ok(bytes)
}

fn fingerprint(path: &Path, maximum: u64, executable: bool) -> Result<String, String> {
    let mut file = open_regular(path, maximum)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    let mut total = 0u64;
    loop {
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        if total == 0 && executable && (count < 2 || &buffer[..2] != b"MZ") {
            return Err(format!(
                "Dedicated server program is not a PE image: {}",
                path.display()
            ));
        }
        total += count as u64;
        if total > maximum {
            return Err("Player-query file grew beyond its read limit".into());
        }
        hash.update(&buffer[..count]);
    }
    if executable && total < 2 {
        return Err("Dedicated server program is empty".into());
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

// Pin every existing ancestor without delete sharing while using path-based I/O.
fn pin_chain(path: &Path) -> Result<Vec<File>, String> {
    if !path.is_absolute() {
        return Err("Player-query root must be absolute".into());
    }
    let mut pins = Vec::new();
    for parent in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        let metadata = fs::symlink_metadata(parent).map_err(|error| error.to_string())?;
        if !metadata.is_dir() || reparse(&metadata) {
            return Err("Player-query paths cannot use reparse points".into());
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options
                .access_mode(1)
                .share_mode(3)
                .custom_flags(0x0220_0000);
        }
        let pin = options.open(parent).map_err(|error| error.to_string())?;
        let metadata = pin.metadata().map_err(|error| error.to_string())?;
        if !metadata.is_dir() || reparse(&metadata) {
            return Err("Player-query directory changed during preparation".into());
        }
        pins.push(pin);
    }
    Ok(pins)
}

fn program_supported(root: &Path, server_shas: &[&str]) -> Result<bool, String> {
    fingerprint(
        &root.join("RSDragonwildsServer.exe"),
        32 * 1024 * 1024,
        true,
    )?;
    Ok(server_shas.contains(
        &fingerprint(
            &root
                .join(WIN64)
                .join("RSDragonwildsServer-Win64-Shipping.exe"),
            512 * 1024 * 1024,
            true,
        )?
        .as_str(),
    ))
}

fn ownership(root: &Path, name: &str) -> Result<Option<Ownership>, String> {
    if !exists(&root.join(name))? {
        return Ok(None);
    }
    let limit = if name == INTENT {
        MAX_ENCODED_PROXY + 16 * 1024
    } else {
        16 * 1024
    };
    serde_json::from_slice(&read(&root.join(name), limit)?)
        .map(Some)
        .map_err(|_| {
            "Invalid Dragonwilds player-query ownership record; files were preserved".into()
        })
}

fn expected_files(
    proxy: &[u8],
    ue4ss_license: &[u8],
    fmt_license: &[u8],
    payload: &BTreeMap<String, Vec<u8>>,
) -> Ownership {
    let mut files = payload
        .iter()
        .map(|(name, bytes)| (name.clone(), digest(bytes)))
        .collect::<BTreeMap<_, _>>();
    for (name, bytes) in [
        ("UE4SS-settings.ini", PROFILE),
        ("Mods/LgsmPlayerQuery/Scripts/main.lua", SCRIPT),
        ("Mods/LgsmPlayerQuery/enabled.txt", &[]),
        ("LICENSE-UE4SS.txt", ue4ss_license),
        ("LICENSE-fmt.txt", fmt_license),
        (PROXY_COPY, proxy),
    ] {
        files.insert(name.into(), digest(bytes));
    }
    Ownership {
        schema: 1,
        archive_sha256: ARCHIVE_SHA.into(),
        proxy_source_revision: PROXY_SOURCE_REVISION.into(),
        proxy_sha256: digest(proxy),
        files,
        pending_proxy_base64: None,
    }
}

fn validate_owner(owner: &Ownership, release: &[(&str, usize, &str)]) -> Result<(), String> {
    let mut expected = release
        .iter()
        .map(|(name, _, hash)| (name.to_string(), hash.to_string()))
        .collect::<BTreeMap<_, _>>();
    expected.extend([
        ("UE4SS-settings.ini".into(), digest(PROFILE)),
        (
            "Mods/LgsmPlayerQuery/Scripts/main.lua".into(),
            digest(SCRIPT),
        ),
        ("Mods/LgsmPlayerQuery/enabled.txt".into(), digest(&[])),
    ]);
    if owner.schema != 1
        || owner.archive_sha256 != ARCHIVE_SHA
        || owner.proxy_source_revision != PROXY_SOURCE_REVISION
        || owner.proxy_sha256.len() != 64
        || !owner
            .proxy_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || owner.files.get(PROXY_COPY) != Some(&owner.proxy_sha256)
        || owner.files.len() != expected.len() + 3
        || expected
            .iter()
            .any(|(name, hash)| owner.files.get(name) != Some(hash))
        || ["LICENSE-UE4SS.txt", "LICENSE-fmt.txt"].iter().any(|name| {
            owner.files.get(*name).is_none_or(|hash| {
                hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        })
    {
        return Err(
            "Dragonwilds player-query ownership does not match this release; files were preserved"
                .into(),
        );
    }
    pending_proxy(owner)?;
    Ok(())
}

fn pending_proxy(owner: &Ownership) -> Result<Option<Vec<u8>>, String> {
    let Some(encoded) = &owner.pending_proxy_base64 else {
        return Ok(None);
    };
    if encoded.len() > MAX_ENCODED_PROXY {
        return Err("Player-query intent proxy exceeds its recovery limit".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| "Player-query intent contains invalid recovery bytes")?;
    if bytes.len() > MAX_PROXY || !bytes.starts_with(b"MZ") || digest(&bytes) != owner.proxy_sha256
    {
        return Err(
            "Player-query intent recovery bytes do not match their recorded proxy hash".into(),
        );
    }
    Ok(Some(bytes))
}

fn encode_intent(owner: &Ownership, proxy: &[u8]) -> Result<Vec<u8>, String> {
    let mut intent = owner.clone();
    intent.pending_proxy_base64 = Some(base64::engine::general_purpose::STANDARD.encode(proxy));
    pending_proxy(&intent)?;
    // Commit the bounded recovery bytes with ownership before creating any DLL
    // or staging path, including when the next application build changes PE bytes.
    serde_json::to_vec(&intent).map_err(|error| error.to_string())
}

fn temporary_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.strip_prefix('.')
                .and_then(|name| name.strip_suffix(".pending"))
                .is_some_and(|token| {
                    token.len() == 32 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
        })
}

fn validate_tree(
    directory: &Path,
    files: &BTreeMap<String, String>,
    complete: bool,
    staged: bool,
) -> Result<(), String> {
    if !exists(directory)? {
        return if complete {
            Err("Owned Dragonwilds player-query files are missing".into())
        } else {
            Ok(())
        };
    }
    let _pins = pin_chain(directory)?;
    // UE4SS writes logs and caches after startup. Only the immutable files we
    // own authorize recovery; unrelated runtime output and Mods stay untouched.
    if !staged {
        for (name, expected) in files {
            let path = directory.join(name);
            if !exists(&path)? {
                if complete {
                    return Err("Owned Dragonwilds player-query files are missing".into());
                }
                continue;
            }
            let _parents = pin_chain(path.parent().ok_or("Player-query file has no parent")?)?;
            if fingerprint(&path, MAX_FILE as u64, false)? != *expected {
                return Err(format!(
                    "Owned player-query file was modified; preserved {}",
                    path.display()
                ));
            }
        }
        return Ok(());
    }
    let mut allowed_dirs = BTreeSet::new();
    for name in files.keys() {
        for parent in Path::new(name).ancestors().skip(1) {
            allowed_dirs.insert(parent.to_path_buf());
        }
    }
    let mut pending = vec![directory.to_path_buf()];
    let mut seen = 0;
    while let Some(current) = pending.pop() {
        for entry in fs::read_dir(&current).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            seen += 1;
            if seen > 64 {
                return Err(
                    "Unexpected player-query directory contents; files were preserved".into(),
                );
            }
            let path = entry.path();
            let relative = path
                .strip_prefix(directory)
                .map_err(|error| error.to_string())?;
            let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
            if reparse(&metadata) {
                return Err("Player-query files cannot use reparse points".into());
            }
            if metadata.is_dir() && allowed_dirs.contains(relative) {
                pending.push(path);
                continue;
            }
            if temporary_name(relative) && metadata.is_file() && metadata.len() <= MAX_FILE as u64 {
                // Intent owns this unpublished staging tree. A process crash
                // can leave an incomplete temporary copy, never a live DLL.
                if complete {
                    fs::remove_file(&path).map_err(|error| error.to_string())?;
                }
                continue;
            }
            let name = relative.to_string_lossy().replace('\\', "/");
            let expected = files
                .get(&name)
                .ok_or("Unexpected player-query file; existing files were preserved")?;
            if !metadata.is_file() || fingerprint(&path, MAX_FILE as u64, false)? != *expected {
                return Err(format!(
                    "Owned player-query file was modified; preserved {}",
                    path.display()
                ));
            }
        }
    }
    if complete && files.keys().any(|name| !directory.join(name).is_file()) {
        return Err("Owned player-query installation is incomplete".into());
    }
    Ok(())
}

pub(super) fn inspect(root: &Path, proxy: &[u8]) -> Result<Preparation, String> {
    inspect_release(root, proxy, SERVER_SHAS, RELEASE)
}

/// A retired owned loader leaves its scripts behind. They cannot answer IPC
/// without a proxy; external query-only loaders keep their own admission.
pub(super) fn owned_loader_disabled(root: &Path) -> Result<bool, String> {
    let win64 = root.join(WIN64);
    let _pins = pin_chain(&win64)?;
    if exists(&win64.join("version.dll"))? || exists(&win64.join("dwmapi.dll"))? {
        return Ok(false);
    }
    let owner = match ownership(root, RECEIPT)? {
        Some(owner) => Some(owner),
        None => ownership(root, INTENT)?,
    };
    let Some(owner) = owner else {
        return Ok(false);
    };
    validate_owner(&owner, RELEASE)?;
    Ok(true)
}

fn verified_proxy(win64: &Path, owner: &Ownership) -> Result<bool, String> {
    let path = win64.join("version.dll");
    if !exists(&path)? {
        return Ok(false);
    }
    if fingerprint(&path, 4 * 1024 * 1024, false)? != owner.proxy_sha256 {
        return Err("Owned Dragonwilds proxy was modified; existing files were preserved".into());
    }
    Ok(true)
}

fn retire_proxy(win64: &Path, owner: &Ownership) -> Result<(), String> {
    if verified_proxy(win64, owner)? {
        fs::remove_file(win64.join("version.dll")).map_err(|error| {
            format!("Cannot disable the owned loader for this unsupported game build: {error}")
        })?;
    }
    Ok(())
}

fn inspect_release(
    root: &Path,
    proxy: &[u8],
    server_shas: &[&str],
    release: &[(&str, usize, &str)],
) -> Result<Preparation, String> {
    if !proxy.starts_with(b"MZ") || proxy.len() > 4 * 1024 * 1024 {
        return Err("Bundled Dragonwilds proxy is invalid".into());
    }
    let _pins = pin_chain(&root.join(WIN64))?;
    super::ensure_program_stopped(root)?;
    let supported = program_supported(root, server_shas)?;
    let win64 = root.join(WIN64);
    if let Some(owner) = ownership(root, RECEIPT)? {
        validate_owner(&owner, release)?;
        if !supported {
            retire_proxy(&win64, &owner)?;
            return Ok(Preparation::UnsupportedBuild);
        }
        validate_tree(&win64.join(LOADER), &owner.files, true, false)?;
        return Ok(
            if verified_proxy(&win64, &owner)? && exists(&root.join("langame_player_query"))? {
                let _exchange = pin_chain(&root.join("langame_player_query"))?;
                Preparation::Ready
            } else {
                Preparation::NeedsInstall
            },
        );
    }
    if let Some(owner) = ownership(root, INTENT)? {
        validate_owner(&owner, release)?;
        if !supported {
            retire_proxy(&win64, &owner)?;
            return Ok(Preparation::UnsupportedBuild);
        }
        validate_tree(&win64.join(LOADER), &owner.files, false, false)?;
        validate_tree(&win64.join(STAGE), &owner.files, false, true)?;
        if exists(&win64.join("version.dll"))? {
            validate_tree(&win64.join(LOADER), &owner.files, true, false)?;
            if fingerprint(&win64.join("version.dll"), 4 * 1024 * 1024, false)?
                != owner.proxy_sha256
            {
                return Err(
                    "Interrupted Dragonwilds proxy was modified; files were preserved".into(),
                );
            }
        }
        return Ok(Preparation::NeedsInstall);
    }
    for name in ["version.dll", "dwmapi.dll", LOADER, STAGE] {
        if exists(&win64.join(name))? {
            return Ok(Preparation::ExternalLoader);
        }
    }
    Ok(if supported {
        Preparation::NeedsInstall
    } else {
        Preparation::UnsupportedBuild
    })
}

fn read_archive(
    archive: &[u8],
    size: u64,
    hash: &str,
    release: &[(&str, usize, &str)],
) -> Result<BTreeMap<String, Vec<u8>>, String> {
    if archive.len() as u64 != size || digest(archive) != hash {
        return Err(
            "Dragonwilds UE4SS archive size or SHA-256 does not match the pinned official release"
                .into(),
        );
    }
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive))
        .map_err(|error| format!("Invalid UE4SS archive: {error}"))?;
    if zip.len() > 512 {
        return Err("UE4SS archive has too many entries".into());
    }
    let mut payload = BTreeMap::new();
    for (name, expected_size, expected_hash) in release {
        let archive_name = format!("ue4ss/{name}");
        let mut entry = zip
            .by_name(&archive_name)
            .map_err(|_| format!("UE4SS archive lacks {archive_name}"))?;
        if !entry.is_file()
            || entry.size() != *expected_size as u64
            || *expected_size > MAX_FILE
            || entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("UE4SS payload is not an ordinary bounded file".into());
        }
        let mut bytes = Vec::new();
        entry
            .by_ref()
            .take(*expected_size as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() != *expected_size || digest(&bytes) != *expected_hash {
            return Err("UE4SS payload checksum mismatch".into());
        }
        payload.insert(name.to_string(), bytes);
    }
    Ok(payload)
}

#[cfg(test)]
pub(super) fn verify_archive(archive: &[u8]) -> Result<(), String> {
    read_archive(archive, ARCHIVE_BYTES, ARCHIVE_SHA, RELEASE).map(|_| ())
}

// Atomic publication never replaces a file. Persistent intent grants ownership
// of incomplete staging, while published files must retain their recorded hash.
fn publish_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if exists(path)? {
        return if read(path, MAX_FILE)? == bytes {
            Ok(())
        } else {
            Err(format!(
                "Player-query destination changed; preserved {}",
                path.display()
            ))
        };
    }
    let parent = path.parent().ok_or("Player-query file has no parent")?;
    let _pins = pin_chain(parent)?;
    let temporary = parent.join(format!(".{}.pending", uuid::Uuid::new_v4().simple()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        // The caller holds the program lease; re-check external modifications.
        if path.try_exists()? {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "destination appeared",
            ));
        }
        // A hard link publishes without ever replacing a concurrently created
        // user file. Both paths are on the same installation volume.
        fs::hard_link(&temporary, path)?;
        fs::remove_file(&temporary)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|error| {
        format!(
            "Cannot publish player-query file {}: {error}",
            path.display()
        )
    })
}

pub(super) fn install(
    root: &Path,
    archive: &[u8],
    proxy: &[u8],
    ue4ss_license: &[u8],
    fmt_license: &[u8],
) -> Result<(), String> {
    let _pins = pin_chain(&root.join(WIN64))?;
    if inspect(root, proxy)? != Preparation::NeedsInstall {
        return Ok(());
    }
    let payload = read_archive(archive, ARCHIVE_BYTES, ARCHIVE_SHA, RELEASE)?;
    install_payload(root, proxy, ue4ss_license, fmt_license, payload, RELEASE)
}

fn install_payload(
    root: &Path,
    proxy: &[u8],
    ue4ss_license: &[u8],
    fmt_license: &[u8],
    mut payload: BTreeMap<String, Vec<u8>>,
    release: &[(&str, usize, &str)],
) -> Result<(), String> {
    if ue4ss_license.is_empty()
        || fmt_license.is_empty()
        || ue4ss_license.len() > 64 * 1024
        || fmt_license.len() > 64 * 1024
    {
        return Err("Bundled Dragonwilds proxy licenses are invalid".into());
    }
    let previous = ownership(root, INTENT)?.or(ownership(root, RECEIPT)?);
    let proxy = if let Some(previous) = &previous {
        validate_owner(previous, release)?;
        retained_proxy(root, previous, proxy)?
    } else {
        proxy.to_vec()
    };
    let owner = expected_files(&proxy, ue4ss_license, fmt_license, &payload);
    validate_owner(&owner, release)?;
    if let Some(previous) = &previous
        && (previous.files != owner.files || previous.proxy_sha256 != owner.proxy_sha256)
    {
        return Err(
            "Interrupted player-query intent differs from this payload; files were preserved"
                .into(),
        );
    }
    if !exists(&root.join(INTENT))? {
        publish_new(&root.join(INTENT), &encode_intent(&owner, &proxy)?)?;
    }
    let win64 = root.join(WIN64);
    let stage = win64.join(STAGE);
    let loader = win64.join(LOADER);
    payload.extend([
        ("UE4SS-settings.ini".into(), PROFILE.to_vec()),
        (
            "Mods/LgsmPlayerQuery/Scripts/main.lua".into(),
            SCRIPT.to_vec(),
        ),
        ("Mods/LgsmPlayerQuery/enabled.txt".into(), Vec::new()),
        ("LICENSE-UE4SS.txt".into(), ue4ss_license.to_vec()),
        ("LICENSE-fmt.txt".into(), fmt_license.to_vec()),
        (PROXY_COPY.into(), proxy.clone()),
    ]);
    if !exists(&loader)? {
        fs::create_dir_all(&stage).map_err(|error| error.to_string())?;
        validate_tree(&stage, &owner.files, false, true)?;
        for (name, bytes) in payload {
            let path = stage.join(name);
            fs::create_dir_all(path.parent().ok_or("Player-query payload has no parent")?)
                .map_err(|error| error.to_string())?;
            publish_new(&path, &bytes)?;
        }
        validate_tree(&stage, &owner.files, true, true)?;
        fs::rename(&stage, &loader)
            .map_err(|error| format!("Cannot publish the verified player-query loader: {error}"))?;
    }
    validate_tree(&loader, &owner.files, true, false)?;
    let exchange = root.join("langame_player_query");
    fs::create_dir_all(&exchange).map_err(|error| error.to_string())?;
    let _exchange_pin = pin_chain(&exchange)?;
    super::ensure_program_stopped(root)?;
    publish_new(&win64.join("version.dll"), &proxy)?;
    publish_new(
        &root.join(RECEIPT),
        &serde_json::to_vec(&owner).map_err(|error| error.to_string())?,
    )?;
    fs::remove_file(root.join(INTENT))
        .map_err(|error| format!("Cannot finalize player-query installation: {error}"))
}

fn retained_proxy(root: &Path, owner: &Ownership, bundled: &[u8]) -> Result<Vec<u8>, String> {
    for directory in [LOADER, STAGE] {
        let path = root.join(WIN64).join(directory).join(PROXY_COPY);
        if exists(&path)? {
            let _pins = pin_chain(path.parent().ok_or("Proxy copy has no parent")?)?;
            let bytes = read(&path, 4 * 1024 * 1024)?;
            if digest(&bytes) != owner.proxy_sha256 {
                return Err(
                    "Retained Dragonwilds proxy was modified; existing files were preserved".into(),
                );
            }
            return Ok(bytes);
        }
    }
    if let Some(bytes) = pending_proxy(owner)? {
        return Ok(bytes);
    }
    if digest(bundled) == owner.proxy_sha256 {
        return Ok(bundled.to_vec());
    }
    Err("The interrupted installation lacks its recorded proxy bytes; existing files were preserved".into())
}

#[cfg(test)]
#[path = "commands_dragonwilds_player_query_files_tests.rs"]
mod tests;
