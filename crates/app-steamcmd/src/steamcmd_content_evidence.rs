//! Recovery evidence must come from complete lines appended by this invocation.
//! Historical diagnostic tails intentionally have a different, looser contract.
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use super::steamcmd_manifest_recovery::{ManifestDenial, manifest_denial};

const MAX_NEW_BYTES: u64 = 4 * 1024 * 1024;
const CHECKPOINT_BYTES: u64 = 64;

#[derive(Debug, PartialEq, Eq)]
struct Identity {
    #[cfg(windows)]
    volume_and_index: (u32, u64),
    #[cfg(unix)]
    device_and_inode: (u64, u64),
    created: Option<std::time::SystemTime>,
}

pub(super) struct ContentLogEvidence {
    path: PathBuf,
    before: Option<Snapshot>,
    usable: bool,
}

struct Snapshot {
    identity: Identity,
    length: u64,
    checkpoint: Vec<u8>,
}

impl ContentLogEvidence {
    pub(super) async fn before_spawn(root: &Path) -> Self {
        let path = root.join("logs/content_log.txt");
        let fallback = path.clone();
        tokio::task::spawn_blocking(move || {
            let snapshot = read_snapshot(&path);
            Self {
                path,
                usable: snapshot.is_ok(),
                before: snapshot.ok().flatten(),
            }
        })
        .await
        .unwrap_or(Self {
            path: fallback,
            before: None,
            usable: false,
        })
    }

    pub(super) async fn finish(self) -> Vec<ManifestDenial> {
        // Losing evidence only disables recovery; ordinary failure diagnostics
        // and SteamCMD's exit status remain available to the caller.
        tokio::task::spawn_blocking(move || self.read_new_denials())
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default()
    }

    fn read_new_denials(self) -> io::Result<Vec<ManifestDenial>> {
        if !self.usable {
            return Ok(Vec::new());
        }
        let mut file = open_log(&self.path)?;
        let length = file.metadata()?.len();
        let (offset, skip_partial) = if let Some(before) = self.before {
            if identity(&file)? != before.identity
                || length < before.length
                || checkpoint(&mut file, before.length)? != before.checkpoint
            {
                // Rotation, replacement and truncate-and-regrow are ambiguous.
                // Never reinterpret an entire replacement log as new evidence.
                return Ok(Vec::new());
            }
            (
                before.length,
                before.length > 0 && before.checkpoint.last() != Some(&b'\n'),
            )
        } else {
            (0, false)
        };
        let appended = length - offset;
        if appended == 0 || appended > MAX_NEW_BYTES {
            return Ok(Vec::new());
        }
        file.seek(SeekFrom::Start(offset))?;
        let mut bytes = Vec::with_capacity(appended as usize);
        file.take(appended).read_to_end(&mut bytes)?;
        if bytes.len() as u64 != appended {
            return Ok(Vec::new());
        }
        let start = if skip_partial {
            match bytes.iter().position(|byte| *byte == b'\n') {
                Some(index) => index + 1,
                None => return Ok(Vec::new()),
            }
        } else {
            0
        };
        let Some(end) = bytes.iter().rposition(|byte| *byte == b'\n') else {
            return Ok(Vec::new());
        };
        if start >= end {
            return Ok(Vec::new());
        }
        let Ok(text) = std::str::from_utf8(&bytes[start..end]) else {
            return Ok(Vec::new());
        };
        Ok(text.lines().filter_map(manifest_denial).collect())
    }
}

fn read_snapshot(path: &Path) -> io::Result<Option<Snapshot>> {
    let mut file = match open_log(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let length = file.metadata()?.len();
    Ok(Some(Snapshot {
        identity: identity(&file)?,
        length,
        checkpoint: checkpoint(&mut file, length)?,
    }))
}

fn open_log(path: &Path) -> io::Result<File> {
    super::steam_depot::check_plain(path, false)?;
    File::open(path)
}

fn identity(file: &File) -> io::Result<Identity> {
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::other("Steam content log is not a regular file"));
    }
    Ok(Identity {
        #[cfg(windows)]
        volume_and_index: {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Storage::FileSystem::{
                BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
            };
            let mut information = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
            // SAFETY: the file owns a live handle and the API initializes the
            // supplied structure on success. No handle ownership is transferred.
            if unsafe {
                GetFileInformationByHandle(file.as_raw_handle().cast(), information.as_mut_ptr())
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: success above initialized every field.
            let information = unsafe { information.assume_init() };
            (
                information.dwVolumeSerialNumber,
                (u64::from(information.nFileIndexHigh) << 32)
                    | u64::from(information.nFileIndexLow),
            )
        },
        #[cfg(unix)]
        device_and_inode: {
            use std::os::unix::fs::MetadataExt;
            (metadata.dev(), metadata.ino())
        },
        created: metadata.created().ok(),
    })
}

fn checkpoint(file: &mut File, offset: u64) -> io::Result<Vec<u8>> {
    let count = offset.min(CHECKPOINT_BYTES);
    file.seek(SeekFrom::Start(offset - count))?;
    let mut bytes = vec![0; count as usize];
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
#[path = "steamcmd_content_evidence_tests.rs"]
mod tests;
