//! Download Valve's signed update manifest and verified packages before starting
//! its bootstrapper. Native SteamCMD remains responsible for applying the update.
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

use super::http_download::{DownloadIntegrity, download_file_with_progress, fetch_text_validated};
use super::steamcmd_prepare::{PrepareReporter, SteamCmdPreparePhase, SteamCmdPrepareProgress};
use super::steamcmd_update_manifest::{UpdateManifest, UpdatePackage, parse_manifest};
use super::{InstallDeadline, SteamCmdError, http_client, ps_literal, run_powershell};

const UPDATE_BASE: &str = "https://client-update.steamstatic.com";
const PLATFORM: &str = "win64";
const MANIFEST_LIMIT: usize = 64 * 1024;

pub(super) struct PreparedUpdate {
    pub manifest: String,
}

pub(super) async fn prepare_update<F: FnMut(SteamCmdPrepareProgress)>(
    root: &Path,
    deadline: InstallDeadline,
    reporter: &mut PrepareReporter<F>,
    repair_bootstrapper: bool,
) -> Result<PreparedUpdate, SteamCmdError> {
    reporter.stage(
        SteamCmdPreparePhase::Inspecting,
        "Checking SteamCMD updates...",
    );
    let client = http_client()?;
    let (text, manifest) = fetch_text_validated(
        &client,
        &format!("{UPDATE_BASE}/steam_cmd_{PLATFORM}"),
        deadline,
        MANIFEST_LIMIT,
        |text| parse_manifest(text, PLATFORM),
    )
    .await?;
    let cache = root.join("package");
    ensure_cache_directory(&cache).await?;
    let bootstrapper = manifest
        .packages
        .iter()
        .find(|package| package.bootstrapper)
        .ok_or_else(|| update_error("The update manifest has no bootstrapper package"))?;
    // Only seed missing/old bootstrapper executables. A modern installation must
    // update itself transactionally; replacing its EXE first could leave mixed
    // versions when the user stops before native installation starts.
    let needs_bootstrapper = repair_bootstrapper
        || !supports_update_override(&root.join("steamcmd.exe"), deadline).await?;
    let bootstrap_archive = UpdatePackage {
        name: bootstrapper.name.clone(),
        file: bootstrapper.archive_file.clone(),
        size: bootstrapper.archive_size,
        sha256: bootstrapper.archive_sha256.clone(),
        ..bootstrapper.clone()
    };
    let missing = missing_packages(
        &cache,
        &manifest,
        needs_bootstrapper.then_some(&bootstrap_archive),
        deadline,
    )
    .await?;
    let total = missing.iter().map(|package| package.size).sum::<u64>();
    if total > 0 {
        reporter.stage(
            SteamCmdPreparePhase::Updating,
            "Downloading SteamCMD updates...",
        );
        reporter.download(0, Some(total));
    }
    let mut completed = 0;
    for package in missing {
        deadline.check_cancelled()?;
        let downloaded = download_file_with_progress(
            &client,
            &format!("{UPDATE_BASE}/{}", package.file),
            &cache.join(&package.file),
            DownloadIntegrity {
                sha256: Some(&package.sha256),
                size: Some(package.size),
                ..Default::default()
            },
            deadline,
            |bytes, _| reporter.download(completed + bytes, Some(total)),
        )
        .await?;
        deadline.check_cancelled()?;
        downloaded.persist(&cache.join(&package.file))?;
        completed += package.size;
    }
    if needs_bootstrapper {
        reporter.stage(
            SteamCmdPreparePhase::Extracting,
            "Preparing the SteamCMD bootstrapper...",
        );
        install_bootstrapper(root, &cache.join(&bootstrap_archive.file), deadline).await?;
    }
    Ok(PreparedUpdate { manifest: text })
}

async fn missing_packages<'a>(
    cache: &Path,
    manifest: &'a UpdateManifest,
    bootstrap: Option<&'a UpdatePackage>,
    deadline: InstallDeadline,
) -> Result<Vec<&'a UpdatePackage>, SteamCmdError> {
    let mut missing = Vec::new();
    for package in manifest.packages.iter().chain(bootstrap) {
        if missing
            .iter()
            .any(|item: &&UpdatePackage| item.file == package.file)
        {
            continue;
        }
        if !cache_matches(&cache.join(&package.file), package, deadline).await? {
            missing.push(package);
        }
    }
    Ok(missing)
}

async fn supports_update_override(
    path: &Path,
    deadline: InstallDeadline,
) -> Result<bool, SteamCmdError> {
    deadline.check_cancelled()?;
    let metadata = match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(update_error(format!(
                "Cannot inspect SteamCMD bootstrapper: {error}"
            )));
        }
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(update_error("SteamCMD bootstrapper is not a regular file"));
    }
    if metadata.len() < 64 || metadata.len() > 64 * 1024 * 1024 {
        return Ok(false);
    }
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|error| update_error(format!("Cannot read SteamCMD bootstrapper: {error}")))?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    let result = async {
        let mut buffer = vec![0_u8; 64 * 1024];
        loop {
            deadline.check_cancelled()?;
            let count = file.read(&mut buffer).await.map_err(|error| {
                update_error(format!("Cannot inspect SteamCMD bootstrapper: {error}"))
            })?;
            if count == 0 {
                break;
            }
            if bytes.len() + count > 64 * 1024 * 1024 {
                return Ok(false);
            }
            bytes.extend_from_slice(&buffer[..count]);
        }
        if bytes.get(..2) != Some(b"MZ") {
            return Ok(false);
        }
        let Some(offset) = bytes.get(60..64) else {
            return Ok(false);
        };
        let offset = u32::from_le_bytes([offset[0], offset[1], offset[2], offset[3]]) as usize;
        Ok(
            bytes.get(offset..offset.saturating_add(6)) == Some(b"PE\0\0\x64\x86")
                && bytes
                    .windows(b"-overridepackageurl".len())
                    .any(|window| window == b"-overridepackageurl"),
        )
    }
    .await;
    drop(file.into_std().await);
    result
}

async fn ensure_cache_directory(path: &Path) -> Result<(), SteamCmdError> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
            return Err(update_error(
                "SteamCMD package cache is not a regular directory",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tokio::fs::create_dir(path)
                .await
                .map_err(|source| SteamCmdError::CreatePath {
                    path: path.to_owned(),
                    source,
                })?;
        }
        Err(error) => {
            return Err(update_error(format!(
                "Cannot inspect package cache: {error}"
            )));
        }
    }
    Ok(())
}

async fn cache_matches(
    path: &Path,
    package: &UpdatePackage,
    deadline: InstallDeadline,
) -> Result<bool, SteamCmdError> {
    deadline.check_cancelled()?;
    let metadata = match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(update_error(format!(
                "Cannot inspect cached package: {error}"
            )));
        }
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(update_error(
            "SteamCMD cached package is not a regular file",
        ));
    }
    if metadata.len() != package.size {
        return Ok(false);
    }
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|error| update_error(format!("Cannot read cached package: {error}")))?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut size = 0;
    let result = async {
        loop {
            deadline.check_cancelled()?;
            let count = file
                .read(&mut buffer)
                .await
                .map_err(|error| update_error(format!("Cannot hash cached package: {error}")))?;
            if count == 0 {
                break;
            }
            size += count as u64;
            if size > package.size {
                return Ok(false);
            }
            hash.update(&buffer[..count]);
        }
        Ok(size == package.size
            && hash
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
                .eq_ignore_ascii_case(&package.sha256))
    }
    .await;
    // Settle tokio's blocking file worker before releasing operation ownership.
    drop(file.into_std().await);
    result
}

async fn install_bootstrapper(
    root: &Path,
    archive: &Path,
    deadline: InstallDeadline,
) -> Result<(), SteamCmdError> {
    let executable = root.join("steamcmd.exe");
    let (staging, file) = super::create_minecraft_staging_file(&executable)?;
    drop(file);
    let temporary = BootstrapStaging(staging);
    let script = format!(
        concat!(
            "$ErrorActionPreference='Stop'\n",
            "Add-Type -AssemblyName System.IO.Compression.FileSystem\n",
            "$zip=[IO.Compression.ZipFile]::OpenRead({archive})\n",
            "try {{\n",
            " $entry=$zip.GetEntry('steamcmd.exe')\n",
            " if ($null -eq $entry -or $entry.Length -lt 1 -or $entry.Length -gt 64MB) {{ throw 'Invalid SteamCMD bootstrapper ZIP' }}\n",
            " [IO.Compression.ZipFileExtensions]::ExtractToFile($entry,{staging},$true)\n",
            "}} finally {{ $zip.Dispose() }}\n"
        ),
        archive = ps_literal(archive),
        staging = ps_literal(&temporary.0),
    );
    let output = run_powershell(&script, Some(root), deadline).await?;
    if !output.status.success() {
        return Err(update_error(super::output_excerpt(
            &output.stdout,
            &output.stderr,
        )));
    }
    deadline.check_cancelled()?;
    super::publish_minecraft_staging_file(&executable, &temporary.0)
}

struct BootstrapStaging(PathBuf);
impl Drop for BootstrapStaging {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub(super) fn update_error(detail: impl Into<String>) -> SteamCmdError {
    SteamCmdError::PrepareSteamCmd {
        output_excerpt: detail.into(),
    }
}

#[cfg(test)]
#[path = "steamcmd_update_cache_tests.rs"]
mod tests;
