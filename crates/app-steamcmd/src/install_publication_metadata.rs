use std::fs;
use std::io::{self, Read};
use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::SteamCmdError;

pub(super) const RETAINED_LIBRARY_MARKER: &str = ".langame-retained-library.json";
pub(super) const MAX_RETAINED_LIBRARY_BYTES: u64 = 16_384;

#[derive(Default)]
pub(super) struct InstallPublicationMetadata {
    pub(super) preserve_retained_data: bool,
    pub(super) retained_library_sha256: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetainedLibrary {
    version: u32,
    module_id: String,
    program_root: String,
}

impl InstallPublicationMetadata {
    pub(super) fn read(
        root: &Path,
        module_id: &str,
        preserve_retained_data: bool,
    ) -> Result<Self, SteamCmdError> {
        let path = root.join(RETAINED_LIBRARY_MARKER);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Self {
                    preserve_retained_data,
                    retained_library_sha256: None,
                });
            }
            Err(source) => return Err(metadata_error(&path, source)),
        };
        if !metadata.is_file() || metadata.len() > MAX_RETAINED_LIBRARY_BYTES {
            return Err(invalid(
                &path,
                "retained library marker is not a bounded file",
            ));
        }
        for ancestor in path.ancestors() {
            let metadata = fs::symlink_metadata(ancestor)
                .map_err(|source| metadata_error(ancestor, source))?;
            #[cfg(windows)]
            let reparse = {
                use std::os::windows::fs::MetadataExt;
                metadata.file_attributes() & 0x400 != 0
            };
            #[cfg(not(windows))]
            let reparse = false;
            if metadata.file_type().is_symlink() || reparse {
                return Err(invalid(ancestor, "retained library path contains a link"));
            }
        }
        let root = fs::canonicalize(root).map_err(|source| metadata_error(root, source))?;
        let mut bytes = Vec::new();
        fs::File::open(&path)
            .and_then(|file| {
                file.take(MAX_RETAINED_LIBRARY_BYTES + 1)
                    .read_to_end(&mut bytes)
            })
            .map_err(|source| metadata_error(&path, source))?;
        if bytes.len() as u64 > MAX_RETAINED_LIBRARY_BYTES {
            return Err(invalid(
                &path,
                "retained library marker exceeds its size limit",
            ));
        }
        let marker: RetainedLibrary = serde_json::from_slice(&bytes)
            .map_err(|_| invalid(&path, "invalid retained library marker"))?;
        if marker.version != 1
            || marker.module_id != module_id
            || marker.program_root != root.to_string_lossy()
        {
            return Err(invalid(
                &path,
                "retained library marker belongs to another installation",
            ));
        }
        Ok(Self {
            preserve_retained_data,
            retained_library_sha256: Some(
                Sha256::digest(&bytes)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect(),
            ),
        })
    }
}

fn invalid(path: &Path, message: &'static str) -> SteamCmdError {
    metadata_error(path, io::Error::new(io::ErrorKind::InvalidData, message))
}

fn metadata_error(path: &Path, source: io::Error) -> SteamCmdError {
    SteamCmdError::InstallOperationLock {
        action: "preserve installation metadata",
        path: path.to_owned(),
        source,
    }
}

#[cfg(test)]
#[path = "install_publication_metadata_tests.rs"]
mod tests;
