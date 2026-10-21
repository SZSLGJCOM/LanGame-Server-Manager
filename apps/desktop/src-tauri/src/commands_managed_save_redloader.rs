//! Fixed official RedLoader release. Downloads data only; no installer is run.
use super::{io, payload};
use std::{collections::BTreeMap, io::Read, path::Path};

const OWNER: &str = ".langame-sons-save-loader.json";
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Release {
    version: String,
    archive_bytes: usize,
    archive_sha256: String,
    files: BTreeMap<String, String>,
}
fn release() -> Result<Release, String> {
    serde_json::from_str(include_str!(
        "../../../../modules/sonsoftheforest/control/redloader.json"
    ))
    .map_err(|_| "The fixed RedLoader release manifest is invalid.".into())
}
pub(super) fn files() -> Result<BTreeMap<String, String>, String> {
    Ok(release()?.files)
}

fn receipt(release: &Release) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&serde_json::json!({"protocol":1,"module_id":"sonsoftheforest","archive_sha256":release.archive_sha256})).map_err(|e|e.to_string())
}

/// Missing reviewed files can resume an interrupted install. Any different
/// existing bytes, including proxy/configuration files, remain an error.
fn inspect_release(root: &Path, release: &Release) -> Result<bool, String> {
    let _root = io::pin_directories(root)?;
    let expected_receipt = receipt(release)?;
    if let Some(old) = io::read_optional(&root.join(OWNER), 4096)?
        && old != expected_receipt
    {
        return Err("The Sons save loader ownership record is unknown; it was preserved.".into());
    }
    let mut missing = false;
    for (relative, expected) in &release.files {
        let path = root.join(relative);
        let _pins = payload::inspect_ancestors(root, path.parent().ok_or("Invalid loader path")?)?;
        match io::read_optional(&path, 64 * 1024 * 1024)? {
            Some(current) if io::digest(&current) != *expected => {
                return Err("An existing RedLoader file, proxy or configuration differs from the fixed release; it was preserved. Resolve that loader conflict before starting.".into());
            }
            None => missing = true,
            _ => {}
        }
    }
    Ok(missing)
}

pub(super) fn requires_archive(root: &Path) -> Result<bool, String> {
    inspect_release(root, &release()?)
}

pub(super) fn adopt_complete_release(root: &Path) -> Result<(), String> {
    let release = release()?;
    adopt_release(root, &release)
}

fn adopt_release(root: &Path, release: &Release) -> Result<(), String> {
    if inspect_release(root, release)? {
        return Err("The fixed RedLoader release is incomplete; its archive is required.".into());
    }
    let previous = io::read_optional(&root.join(OWNER), 4096)?;
    if previous.is_none() {
        io::replace_owned(&root.join(OWNER), None, &receipt(release)?)?;
    }
    verify_release(root, release)
}

pub(super) async fn download() -> Result<Vec<u8>, String> {
    let release = release()?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|_| "Cannot initialize the official RedLoader download.")?;
    let mut response = client
        .get("https://github.com/ToniMacaroni/RedLoader/releases/download/0.8.6/Redloader.zip")
        .send()
        .await
        .map_err(|_| "The official RedLoader download failed; no loader files were installed.")?
        .error_for_status()
        .map_err(|_| "The official RedLoader release was unavailable.")?;
    if response
        .content_length()
        .is_some_and(|length| length != release.archive_bytes as u64)
    {
        return Err("The official RedLoader archive size changed.".into());
    }
    let mut bytes = Vec::with_capacity(release.archive_bytes);
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "The official RedLoader download was interrupted.")?
    {
        if bytes.len().saturating_add(chunk.len()) > release.archive_bytes {
            return Err("The RedLoader archive exceeds its fixed size.".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.len() != release.archive_bytes || io::digest(&bytes) != release.archive_sha256 {
        return Err("The official RedLoader archive does not match the reviewed release.".into());
    }
    Ok(bytes)
}

pub(super) fn verify(root: &Path) -> Result<(), String> {
    let release = release()?;
    verify_release(root, &release)
}

fn verify_release(root: &Path, release: &Release) -> Result<(), String> {
    // Loader-owned generated interop/cache files are outside this immutable
    // release list. Never delete or overwrite them during extension refresh.
    for (relative, expected) in &release.files {
        let path = root.join(relative);
        let _pins = io::pin_directories(path.parent().ok_or("Invalid loader path")?)?;
        if io::digest(&io::read_regular(&path, 64 * 1024 * 1024)?) != *expected {
            return Err("The prepared RedLoader release was changed. Preserve custom loader files and prepare again while stopped.".into());
        }
    }
    Ok(())
}

pub(super) fn install(root: &Path, bytes: &[u8]) -> Result<(), String> {
    let release = release()?;
    if release.version != "0.8.6"
        || bytes.len() != release.archive_bytes
        || io::digest(bytes) != release.archive_sha256
    {
        return Err("The RedLoader archive identity is invalid.".into());
    }
    let _root = io::pin_directories(root)?;
    // The pinned archive hash bounds the central directory before ZIP parsing.
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|_| "Cannot read the fixed RedLoader archive.")?;
    if archive.len() > 1024 {
        return Err("The RedLoader archive has too many entries.".into());
    }
    let mut contents: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|_| "Cannot inspect the RedLoader archive.")?;
        if entry.is_dir() {
            continue;
        }
        let name = entry
            .name()
            .map_err(|_| "The RedLoader archive entry name is invalid.")?
            .into_owned();
        if name.contains('\\')
            || entry.enclosed_name().is_none()
            || entry.size() > 64 * 1024 * 1024
            || contents.contains_key(&name)
        {
            return Err("Unsafe RedLoader archive entry.".into());
        }
        let expected = release
            .files
            .get(&name)
            .ok_or("Unexpected RedLoader archive file.")?;
        let mut data = Vec::new();
        entry
            .take(64 * 1024 * 1024 + 1)
            .read_to_end(&mut data)
            .map_err(|_| "Cannot read the RedLoader archive.")?;
        if io::digest(&data) != *expected {
            return Err("The RedLoader archive file identity changed.".into());
        }
        contents.insert(name, data);
    }
    if contents.len() != release.files.len() {
        return Err("The RedLoader release is incomplete.".into());
    }
    // Inspect every destination before the first loader write, including a
    // recovered partial installation from a previous cancelled start.
    inspect_release(root, &release)?;
    let receipt = receipt(&release)?;
    let previous = io::read_optional(&root.join(OWNER), 4096)?;
    io::replace_owned(&root.join(OWNER), previous.as_deref(), &receipt)?;
    for (relative, data) in contents {
        let path = root.join(relative);
        let _pins = payload::create_directories(root, path.parent().ok_or("Invalid loader path")?)?;
        if io::read_optional(&path, 64 * 1024 * 1024)?.is_none() {
            io::replace_owned(&path, None, &data)?;
        }
    }
    verify(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::commands_managed_save_control::tests::Fixture;

    fn fixed_release() -> Release {
        Release {
            version: "0.8.6".into(),
            archive_bytes: 8,
            archive_sha256: io::digest(b"test archive"),
            files: BTreeMap::from([
                ("winhttp.dll".into(), io::digest(b"fixed proxy")),
                ("_Redloader/loader.dll".into(), io::digest(b"fixed loader")),
            ]),
        }
    }

    #[test]
    fn startup_detects_first_partial_and_complete_loader_without_downloading() {
        let root = Fixture::new();
        let release = fixed_release();
        assert!(inspect_release(&root.0, &release).unwrap());
        assert!(adopt_release(&root.0, &release).is_err());
        assert!(!root.0.join(OWNER).exists());
        std::fs::write(root.0.join("winhttp.dll"), b"fixed proxy").unwrap();
        assert!(inspect_release(&root.0, &release).unwrap());
        std::fs::create_dir(root.0.join("_Redloader")).unwrap();
        std::fs::write(root.0.join("_Redloader/loader.dll"), b"fixed loader").unwrap();
        assert!(!inspect_release(&root.0, &release).unwrap());
        adopt_release(&root.0, &release).unwrap();
        let original = std::fs::read(root.0.join(OWNER)).unwrap();
        adopt_release(&root.0, &release).unwrap();
        assert_eq!(std::fs::read(root.0.join(OWNER)).unwrap(), original);
        std::fs::remove_file(root.0.join("_Redloader/loader.dll")).unwrap();
        assert!(inspect_release(&root.0, &release).unwrap());
        assert_eq!(std::fs::read(root.0.join(OWNER)).unwrap(), original);
    }

    #[test]
    fn startup_preserves_operator_loader_and_unknown_receipt_before_any_write() {
        for conflict in ["winhttp.dll", "_Redloader/loader.dll", OWNER] {
            let root = Fixture::new();
            let release = fixed_release();
            std::fs::create_dir(root.0.join("_Redloader")).unwrap();
            std::fs::write(root.0.join("winhttp.dll"), b"fixed proxy").unwrap();
            std::fs::write(root.0.join("_Redloader/loader.dll"), b"fixed loader").unwrap();
            std::fs::write(root.0.join(conflict), b"operator content").unwrap();
            assert!(inspect_release(&root.0, &release).is_err(), "{conflict}");
            assert!(adopt_release(&root.0, &release).is_err(), "{conflict}");
            assert_eq!(
                std::fs::read(root.0.join(conflict)).unwrap(),
                b"operator content"
            );
            if conflict != OWNER {
                assert!(!root.0.join(OWNER).exists());
            }
        }
    }
}
