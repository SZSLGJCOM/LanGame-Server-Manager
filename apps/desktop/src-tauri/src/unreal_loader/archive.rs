use super::*;
use std::collections::BTreeSet;
use std::io::{Cursor, Read};

pub(super) const RUNTIME_FILES: [(&str, &str); 4] = [
    ("ue4ss/UE4SS.dll", CORE_HASH),
    (
        "ue4ss/LICENSE",
        "99e1d8f8b7cb68e8b2313380ae3b0c8115e39dfbc443fcf21fd67ab74ce1d57e",
    ),
    (
        "ue4ss/UE4SS_SDK_Backends/UE4SS.json",
        "6b8c1725ff59a318b812d5952f63107778b4ceb22b882b297a3666ee9e06c8ff",
    ),
    (
        "dwmapi.dll",
        "f2f9e57f7707c8456b117391b1ab45fd7149332f89a3a94726eda0cc2300e834",
    ),
];

pub(super) fn prepare(profile: Profile, bytes: &[u8]) -> Result<PreparedLoader, String> {
    if bytes.len() != ARCHIVE_BYTES || hash(bytes) != ARCHIVE_HASH {
        return Err("The download is not the fixed official UE4SS 1125 archive; no runtime files were changed.".into());
    }
    let mut files = extract(bytes)?;
    for (name, expected) in RUNTIME_FILES {
        if files.get(name).map(|bytes| hash(bytes)).as_deref() != Some(expected) {
            return Err(format!(
                "The fixed UE4SS archive is missing its verified runtime file: {name}"
            ));
        }
    }
    if profile.proxy != "dwmapi.dll" {
        files.remove("dwmapi.dll");
        let proxy = profile
            .proxy_bytes()
            .ok_or("This platform has no bundled official proxy.")?;
        if !proxy.starts_with(b"MZ") || proxy.len() > 2 * 1024 * 1024 {
            return Err("The bundled official UE4SS proxy is invalid.".into());
        }
        files.insert(profile.proxy.into(), proxy.to_vec());
    }
    for (path, license) in profile::licenses() {
        files.insert(path.into(), license.to_vec());
    }
    if let Some(layout) = profile.vtable_layout() {
        files.insert(profile::VTABLE_LAYOUT.into(), layout.to_vec());
    }
    Ok(PreparedLoader {
        module_id: profile.id.into(),
        files,
    })
}

fn extract(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| format!("Cannot inspect the UE4SS archive: {error}"))?;
    if archive.len() > 128 {
        return Err("The UE4SS archive contains too many entries.".into());
    }
    let mut names = BTreeSet::new();
    let mut expanded = 0u64;
    let mut files = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let name = entry
            .name()
            .map_err(|error| error.to_string())?
            .into_owned();
        let path = name.trim_end_matches('/');
        if path.is_empty()
            || path.contains(['\\', ':', '\0'])
            || path.starts_with('/')
            || path.split('/').any(|part| {
                part.is_empty() || part == "." || part == ".." || part.ends_with(['.', ' '])
            })
            || entry.enclosed_name().is_none()
            || !names.insert(path.to_ascii_lowercase())
            || entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("The UE4SS archive contains an unsafe or duplicate path.".into());
        }
        expanded = expanded
            .checked_add(entry.size())
            .ok_or("UE4SS archive size overflowed.")?;
        if entry.size() > FILE_LIMIT as u64 || expanded > 40 * 1024 * 1024 {
            return Err("The UE4SS archive exceeds its extraction limits.".into());
        }
        if !entry.is_dir() && RUNTIME_FILES.iter().any(|(wanted, _)| *wanted == name) {
            let mut value = Vec::with_capacity(entry.size() as usize);
            entry
                .by_ref()
                .take(FILE_LIMIT as u64 + 1)
                .read_to_end(&mut value)
                .map_err(|error| error.to_string())?;
            if value.len() > FILE_LIMIT || value.len() as u64 != entry.size() {
                return Err("A UE4SS archive member did not match its declared size.".into());
            }
            files.insert(name, value);
        }
    }
    Ok(files)
}

pub(super) fn expected(profile: Profile) -> BTreeMap<String, String> {
    let mut expected = RUNTIME_FILES
        .into_iter()
        .filter(|(name, _)| *name != "dwmapi.dll" || profile.proxy == "dwmapi.dll")
        .map(|(name, hash)| (name.into(), hash.into()))
        .collect::<BTreeMap<_, _>>();
    if let Some(proxy) = profile.proxy_bytes() {
        expected.insert(profile.proxy.into(), hash(proxy));
    }
    for (name, bytes) in profile::licenses() {
        expected.insert(name.into(), hash(bytes));
    }
    if let Some(layout) = profile.vtable_layout() {
        expected.insert(profile::VTABLE_LAYOUT.into(), hash(layout));
    }
    expected
}

#[cfg(test)]
pub(super) fn extract_fixture(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, String> {
    extract(bytes)
}
