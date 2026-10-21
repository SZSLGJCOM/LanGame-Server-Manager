use super::*;
use serde::{Deserialize, Serialize};

const OWNER: &str = ".langame-ue4ss-loader.json";
const SETTINGS: &str = "ue4ss/UE4SS-settings.ini";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema: u32,
    module_id: String,
    archive_sha256: String,
    files: BTreeMap<String, String>,
    pending_files: Option<BTreeMap<String, String>>,
}

fn decode(
    bytes: &[u8],
    profile: Profile,
    allowed: &BTreeMap<String, String>,
) -> Result<Receipt, String> {
    let receipt: Receipt = serde_json::from_slice(bytes)
        .map_err(|_| "The UE4SS ownership receipt is invalid; existing files were preserved.")?;
    let valid = |files: &BTreeMap<String, String>| {
        files.len() <= allowed.len()
            && files.iter().all(|(name, hash)| {
                allowed.contains_key(name)
                    && hash.len() == 64
                    && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
    };
    if receipt.schema != 1
        || receipt.module_id != profile.id
        || receipt.archive_sha256 != ARCHIVE_HASH
        || !valid(&receipt.files)
        || receipt
            .pending_files
            .as_ref()
            .is_some_and(|files| files.len() != allowed.len() || !valid(files))
        || (receipt.pending_files.is_none() && receipt.files.len() != allowed.len())
    {
        return Err(
            "The UE4SS ownership receipt does not match this game; existing files were preserved."
                .into(),
        );
    }
    Ok(receipt)
}

fn original(path: &Path, limit: usize) -> Result<Option<Vec<u8>>, String> {
    if !exists(path)? {
        return Ok(None);
    }
    let _parents = pin(path.parent().ok_or("The UE4SS file has no parent.")?, false)?;
    read(path, limit).map(Some)
}

fn verified_root(root: &Path, profile: Profile) -> Result<Vec<std::fs::File>, String> {
    let mut pins = pin(root, false)?;
    pins.extend(pin(&profile.win64(root), false)?);
    let mut binaries = vec![(profile.executable, profile.game_hash)];
    if profile.id == "conanexiles" {
        binaries.push((
            "Dreamworld.dll",
            "d076d5d223360f133e365ef37901ff9789eb68186a8f6d66494038f4864684b2",
        ));
    }
    for (name, expected) in binaries {
        let path = profile.win64(root).join(name);
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        let mut unsafe_file = !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > 320 * 1024 * 1024;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            unsafe_file |= metadata.file_attributes() & 0x400 != 0;
        }
        if unsafe_file {
            return Err("The native game image must be a bounded ordinary file.".into());
        }
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1).custom_flags(0x0020_0000);
        }
        let mut file = options.open(path).map_err(|error| error.to_string())?;
        use sha2::Digest;
        use std::io::Read;
        let mut digest = sha2::Sha256::new();
        let mut buffer = [0u8; 65536];
        let mut count = 0u64;
        loop {
            let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
            if read == 0 {
                break;
            }
            count += read as u64;
            if count > 320 * 1024 * 1024 {
                return Err("The native image exceeded its size limit.".into());
            }
            digest.update(&buffer[..read]);
        }
        let actual = digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        if actual != expected {
            return Err("This game build is outside the verified automatic UE4SS save contract; no loader files were changed.".into());
        }
        pins.push(file);
    }
    Ok(pins)
}

fn check_other_proxies(win64: &Path, profile: Profile) -> Result<(), String> {
    // UE4SS setup_paths switches its working/config directory to a child named
    // after the executable directory's third parent. In this layout that is
    // the private runtime root's basename, not the Unreal project name.
    if let Some(name) = win64.ancestors().nth(3).and_then(Path::file_name) {
        let override_path = win64.join("ue4ss").join(name);
        if exists(&override_path)? {
            let metadata =
                std::fs::symlink_metadata(&override_path).map_err(|error| error.to_string())?;
            let mut redirects = metadata.is_dir() || metadata.file_type().is_symlink();
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                redirects |= metadata.file_attributes() & 0x400 != 0;
            }
            if redirects {
                return Err("A game-specific UE4SS configuration directory redirects the owned profile and Mods directory; it was preserved.".into());
            }
        }
    }
    for proxy in ["dwmapi.dll", "version.dll", "dsound.dll", "winmm.dll"] {
        if proxy != profile.proxy && exists(&win64.join(proxy))? {
            return Err(format!(
                "Another loader entry exists at {proxy}; it was preserved. Remove the conflicting loader through its original manager before preparing this runtime."
            ));
        }
    }
    Ok(())
}

fn compatible_proxy(profile: Profile, name: &str, value: &str) -> bool {
    name == profile.proxy
        && matches!(
            (profile.proxy, value),
            (
                "version.dll",
                "60c95a24f43760c778847d4b68927c6ed325b109368f7ea9e69f1821228a583c"
            ) | (
                "dsound.dll",
                "e0066aeb6abad295ee5f27a5da06254c00859058f8babfdaf2dfc78e922312d0"
            )
        )
}

/// Inspect every destination before preparing an intent. Existing supported
/// manually installed runtimes may be adopted, but an unknown byte is not owned.
fn inspect(
    win64: &Path,
    profile: Profile,
    expected: &BTreeMap<String, String>,
) -> Result<(Option<Vec<u8>>, Receipt, BTreeMap<String, Vec<u8>>), String> {
    check_other_proxies(win64, profile)?;
    let prior = original(&win64.join(OWNER), 16 * 1024)?;
    let mut receipt = prior
        .as_ref()
        .map(|bytes| decode(bytes, profile, expected))
        .transpose()?
        .unwrap_or(Receipt {
            schema: 1,
            module_id: profile.id.into(),
            archive_sha256: ARCHIVE_HASH.into(),
            files: BTreeMap::new(),
            pending_files: None,
        });
    let mut current = BTreeMap::new();
    for (name, wanted) in expected {
        if let Some(bytes) = original(&win64.join(name), FILE_LIMIT)? {
            let actual = hash(&bytes);
            let owned = receipt.files.get(name) == Some(&actual)
                || receipt
                    .pending_files
                    .as_ref()
                    .and_then(|pending| pending.get(name))
                    == Some(&actual);
            if prior.is_some() && !owned
                || prior.is_none() && &actual != wanted && !compatible_proxy(profile, name, &actual)
            {
                return Err(format!(
                    "The existing UE4SS runtime conflicts at {name}; it was preserved. A different or modified loader cannot be replaced automatically."
                ));
            }
            receipt.files.insert(name.clone(), actual);
            current.insert(name.clone(), bytes);
        }
    }
    Ok((prior, receipt, current))
}

pub(super) fn installed(root: &Path, profile: Profile) -> Result<bool, String> {
    let _pins = verified_root(root, profile)?;
    let win64 = profile.win64(root);
    let expected = archive::expected(profile);
    let (_, receipt, current) = inspect(&win64, profile, &expected)?;
    if receipt.pending_files.is_some() || current.len() != expected.len() {
        return Ok(false);
    }
    if current.iter().any(|(name, bytes)| {
        expected.get(name) != Some(&hash(bytes)) && !compatible_proxy(profile, name, &hash(bytes))
    }) {
        return Ok(false);
    }
    let Some(settings) = original(&win64.join(SETTINGS), 64 * 1024)? else {
        return Ok(false);
    };
    Ok(profile::merge_settings(&settings, profile.settings())? == settings)
}

pub(super) fn install(
    root: &Path,
    profile: Profile,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<InstallOutcome, String> {
    let _pins = verified_root(root, profile)?;
    publish_runtime(&profile.win64(root), profile, files, publish)
}

fn publish_runtime(
    win64: &Path,
    profile: Profile,
    files: &BTreeMap<String, Vec<u8>>,
    mut commit: impl FnMut(&Path, Option<&[u8]>, &[u8]) -> Result<(), String>,
) -> Result<InstallOutcome, String> {
    let _pins = pin(win64, false)?;
    let expected = files
        .iter()
        .map(|(name, bytes)| (name.clone(), hash(bytes)))
        .collect::<BTreeMap<_, _>>();
    let (prior, mut receipt, current) = inspect(win64, profile, &expected)?;
    let settings = original(&win64.join(SETTINGS), 64 * 1024)?;
    let merged = profile::merge_settings(settings.as_deref().unwrap_or(&[]), profile.settings())?;
    let mut expected = expected;
    // The exact genuine proxies used in the native trials remain supported.
    // A compatible operator installation is reused instead of recompiling it.
    if let Some(bytes) = current.get(profile.proxy)
        && compatible_proxy(profile, profile.proxy, &hash(bytes))
    {
        expected.insert(profile.proxy.into(), hash(bytes));
    }
    if receipt.pending_files.is_none()
        && current.len() == files.len()
        && current
            .iter()
            .all(|(name, bytes)| expected.get(name) == Some(&hash(bytes)))
        && settings.as_deref() == Some(merged.as_slice())
    {
        return Ok(InstallOutcome::Reused);
    }
    receipt.pending_files = Some(expected.clone());
    let intent = serde_json::to_vec(&receipt).map_err(|error| error.to_string())?;
    commit(&win64.join(OWNER), prior.as_deref(), &intent)?;
    // Keep the activation proxy last, after core, SDK backend, licenses and the
    // merged EngineTick profile. A crash is recoverable through this intent.
    for (name, bytes) in files
        .iter()
        .filter(|(name, _)| name.as_str() != profile.proxy)
    {
        let target = win64.join(name);
        let _parents = pin(target.parent().ok_or("Invalid UE4SS destination.")?, true)?;
        if current.get(name) != Some(bytes) {
            commit(&target, current.get(name).map(Vec::as_slice), bytes)?;
        }
    }
    let settings_path = win64.join(SETTINGS);
    let _parents = pin(
        settings_path
            .parent()
            .ok_or("Invalid UE4SS settings path.")?,
        true,
    )?;
    if settings.as_deref() != Some(merged.as_slice()) {
        commit(&settings_path, settings.as_deref(), &merged)?;
    }
    if current.get(profile.proxy).map(|bytes| hash(bytes)).as_ref() != expected.get(profile.proxy) {
        let bytes = files
            .get(profile.proxy)
            .ok_or("The verified proxy payload is missing.")?;
        commit(
            &win64.join(profile.proxy),
            current.get(profile.proxy).map(Vec::as_slice),
            bytes,
        )?;
    }
    receipt.files = expected;
    receipt.pending_files = None;
    commit(
        &win64.join(OWNER),
        Some(&intent),
        &serde_json::to_vec(&receipt).map_err(|error| error.to_string())?,
    )?;
    Ok(InstallOutcome::Installed)
}

#[cfg(test)]
pub(super) fn publish_fixture(
    win64: &Path,
    profile: Profile,
    files: &BTreeMap<String, Vec<u8>>,
    commit: impl FnMut(&Path, Option<&[u8]>, &[u8]) -> Result<(), String>,
) -> Result<InstallOutcome, String> {
    publish_runtime(win64, profile, files, commit)
}
