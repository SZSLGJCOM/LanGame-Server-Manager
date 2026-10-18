//! Recovery evidence must come from complete lines appended by this invocation.
//! Historical diagnostic tails intentionally have a different, looser contract.
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

use super::steamcmd_manifest_recovery::{ManifestDenial, manifest_denial};

const CHECKPOINT_BYTES: u64 = 64;
const MAX_DENIALS: usize = 8192;

#[path = "steamcmd_invocation_log.rs"]
mod invocation_log;
pub(super) use invocation_log::InvocationLog;
pub(super) use invocation_log::LogUpdate;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Identity {
    #[cfg(windows)]
    volume_and_index: (u32, u64),
    #[cfg(unix)]
    device_and_inode: (u64, u64),
    created: Option<std::time::SystemTime>,
}

pub(super) struct ContentLogEvidence {
    log: InvocationLog,
    denials: Vec<ManifestDenial>,
    diagnostics: Vec<String>,
}

#[derive(Clone)]
struct Snapshot {
    identity: Identity,
    length: u64,
    checkpoint: Vec<u8>,
}

impl ContentLogEvidence {
    pub(super) async fn before_spawn(root: &Path) -> Self {
        Self {
            log: InvocationLog::before_spawn(root.join("logs/content_log.txt")).await,
            denials: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    pub(super) async fn poll(&mut self) -> LogUpdate {
        let update = self.log.poll().await;
        for line in &update.lines {
            let Some(denial) = manifest_denial(line) else {
                continue;
            };
            if self.denials.len() < MAX_DENIALS && !self.denials.contains(&denial) {
                self.denials.push(denial);
                if self.diagnostics.len() < 16 {
                    self.diagnostics.push(line.clone());
                }
            }
        }
        update
    }

    #[cfg(test)]
    pub(super) async fn finish(self) -> Vec<ManifestDenial> {
        self.finish_with_excerpt().await.0
    }

    pub(super) async fn finish_with_excerpt(mut self) -> (Vec<ManifestDenial>, Option<String>) {
        while self.poll().await.activity {}
        (
            self.denials,
            (!self.diagnostics.is_empty()).then(|| self.diagnostics.join("\n")),
        )
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
