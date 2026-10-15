//! Bounded, read-only evidence from one owned ASA process and its executable.
use app_core::ProcessIdentity;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(super) const MAX_LOG_BYTES: u64 = 256 * 1024;
const MAX_LOG_FILES: usize = 4096;
const WINDOWS_EPOCH_TICKS: u64 = 116_444_736_000_000_000;
type ImageEvidence = Option<(String, Arc<File>)>;
#[derive(Default)]
pub(super) struct ImageCache {
    images: BTreeMap<PathBuf, ImageEvidence>,
}
#[derive(Clone)]
pub(super) struct Evidence {
    pub(super) pid: u32,
    pub(super) identity: ProcessIdentity,
    pub(super) executable_hash: String,
    pub(super) log_directory: PathBuf,
    pub(super) created_at: SystemTime,
    pub(super) _executable: Arc<File>,
}
#[derive(Default, Debug, PartialEq, Eq)]
pub(super) struct Observation {
    pub(super) cold_download: bool,
    pub(super) initialized: bool,
}
pub(super) fn capture(
    root: &str,
    executable: &str,
    pid: u32,
    identity: ProcessIdentity,
    images: &mut ImageCache,
) -> Result<Option<Evidence>, String> {
    if !app_runtime::process_matches_identity(pid, &identity).map_err(|error| error.to_string())? {
        return Ok(None);
    }
    let executable_path = fs::canonicalize(executable).map_err(|error| error.to_string())?;
    let identity_path =
        fs::canonicalize(&identity.image_path).map_err(|error| error.to_string())?;
    if !executable_path
        .to_string_lossy()
        .eq_ignore_ascii_case(&identity_path.to_string_lossy())
    {
        return Ok(None);
    }
    let created_at = process_creation_time(&identity).ok_or("Invalid ASA process creation time")?;
    let image = cached_image(images, &executable_path, || {
        super::super::commands_ark_tools::has_owned_asa_framework_for_start(Path::new(root))
    })?;
    let Some((executable_hash, file)) = image else {
        return Ok(None);
    };
    Ok(Some(Evidence {
        pid,
        identity,
        executable_hash,
        log_directory: executable_path
            .parent()
            .ok_or("ASA executable has no parent")?
            .join("logs"),
        created_at,
        _executable: file,
    }))
}

pub(super) fn cached_image(
    images: &mut ImageCache,
    executable_path: &Path,
    owned_framework: impl FnOnce() -> Result<bool, String>,
) -> Result<ImageEvidence, String> {
    if !images.images.contains_key(executable_path) {
        let image = if owned_framework()? {
            let mut file =
                open_regular(executable_path, false).map_err(|error| error.to_string())?;
            if file.metadata().map_err(|error| error.to_string())?.len() > 1024 * 1024 * 1024 {
                return Err("ASA executable exceeds the startup hash limit".into());
            }
            let mut hasher = Sha256::new();
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let length = file.read(&mut buffer).map_err(|error| error.to_string())?;
                if length == 0 {
                    break;
                }
                hasher.update(&buffer[..length]);
            }
            let hash = hasher
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            Some((hash, Arc::new(file)))
        } else {
            None
        };
        images.images.insert(executable_path.to_path_buf(), image);
    }
    Ok(images.images.get(executable_path).cloned().flatten())
}

pub(super) fn process_creation_time(identity: &ProcessIdentity) -> Option<SystemTime> {
    let ticks = identity.creation_time.checked_sub(WINDOWS_EPOCH_TICKS)?;
    UNIX_EPOCH.checked_add(Duration::from_nanos(ticks.checked_mul(100)?))
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

fn open_regular(path: &Path, allow_writes: bool) -> std::io::Result<File> {
    #[cfg(not(windows))]
    let _ = allow_writes;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options
            .share_mode(if allow_writes { 0x1 | 0x2 } else { 0x1 })
            .custom_flags(0x0020_0000);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || reparse(&metadata) {
        return Err(std::io::Error::other(
            "ASA startup evidence must be a regular file",
        ));
    }
    Ok(file)
}

pub(super) fn observe(evidence: &Evidence) -> Result<Observation, String> {
    if !app_runtime::process_matches_identity(evidence.pid, &evidence.identity)
        .map_err(|error| error.to_string())?
    {
        return Err(format!(
            "ASA process {} exited or changed identity during offset initialization",
            evidence.pid
        ));
    }
    read_observation(
        &evidence.log_directory,
        evidence.pid,
        evidence.created_at,
        &evidence.executable_hash,
    )
    .map_err(|error| format!("Cannot inspect ASA offset startup evidence: {error}"))
}

pub(super) fn read_observation(
    directory: &Path,
    pid: u32,
    created: SystemTime,
    hash: &str,
) -> std::io::Result<Observation> {
    let metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Observation::default());
        }
        Err(error) => return Err(error),
    };
    if !metadata.is_dir() || reparse(&metadata) {
        return Err(std::io::Error::other(
            "ASA log directory is not an ordinary directory",
        ));
    }
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Observation::default());
        }
        Err(error) => return Err(error),
    };
    let prefix = format!("ArkApi_{pid}_");
    let mut result = Observation::default();
    for (index, entry) in entries.enumerate() {
        if index >= MAX_LOG_FILES {
            return Err(std::io::Error::other(
                "ASA log directory exceeds the inspection limit",
            ));
        }
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with(&prefix) || !name.ends_with(".log") {
            continue;
        }
        let mut file = match open_regular(&entry.path(), true) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        let metadata = file.metadata()?;
        // A PID may be reused. A file from before this exact process was created
        // is never evidence, even when its filename or contents contain its PID.
        if metadata.created()? < created || metadata.modified()? < created {
            continue;
        }
        let offset = metadata.len().saturating_sub(MAX_LOG_BYTES);
        file.seek(SeekFrom::Start(offset))?;
        let mut bytes = Vec::new();
        file.take(MAX_LOG_BYTES).read_to_end(&mut bytes)?;
        let text = String::from_utf8_lossy(&bytes);
        let text = if offset > 0 {
            text.split_once('\n').map_or("", |(_, rest)| rest)
        } else {
            &text
        };
        let observation = parse_observation(text, hash);
        result.cold_download |= observation.cold_download;
        result.initialized |= observation.initialized;
    }
    Ok(result)
}

pub(super) fn parse_observation(text: &str, hash: &str) -> Observation {
    let mut result = Observation::default();
    let mut cold_cache = false;
    let expected = format!("Downloading cache archive {hash}.zip");
    for line in text
        .split_inclusive('\n')
        .filter(|line| line.ends_with('\n'))
    {
        let Some((_, message)) = line
            .trim_end_matches(['\r', '\n'])
            .split_once("[API][info] ")
        else {
            continue;
        };
        if message == "No verified local cache matches the current executable" {
            cold_cache = true;
        }
        if cold_cache && message == expected {
            result.cold_download = true;
        }
        if message == "API was successfully loaded" {
            result.initialized = true;
        }
    }
    result
}
