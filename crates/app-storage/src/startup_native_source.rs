use crate::managed_console_log::owned_fs::{FileIdentity, identity};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NativeStartupSource {
    pub process_key: String,
    pub source_path: String,
    pub through_byte_offset: u64,
    pub process_creation_time: u64,
    identity: FileIdentity,
    prefix: Vec<u8>,
    anchor: Vec<u8>,
}

fn bytes_at(file: &mut File, offset: u64, count: usize) -> io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = vec![0; count];
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

impl NativeStartupSource {
    pub fn capture(
        process_key: String,
        source_path: &Path,
        process_creation_time: u64,
        file: &mut File,
        through_byte_offset: u64,
    ) -> io::Result<Self> {
        let position = file.stream_position()?;
        let result = (|| {
            if through_byte_offset > file.metadata()?.len() {
                return Err(io::Error::other(
                    "Native startup cursor exceeds its captured file",
                ));
            }
            let count = through_byte_offset.min(64) as usize;
            Ok(Self {
                process_key,
                source_path: source_path.to_string_lossy().into_owned(),
                through_byte_offset,
                process_creation_time,
                identity: identity(file)?,
                prefix: bytes_at(file, 0, count)?,
                anchor: bytes_at(file, through_byte_offset - count as u64, count)?,
            })
        })();
        file.seek(SeekFrom::Start(position))?;
        result
    }

    pub fn validated_offset(
        &self,
        path: &Path,
        file: &mut File,
        process_creation_time: u64,
    ) -> io::Result<Option<u64>> {
        let normalized = |path: &Path| {
            path.to_string_lossy()
                .replace('\\', "/")
                .trim_start_matches("//?/")
                .to_ascii_lowercase()
        };
        if process_creation_time != self.process_creation_time
            || normalized(path) != normalized(Path::new(&self.source_path))
            || identity(file)? != self.identity
            || file.metadata()?.len() < self.through_byte_offset
            || self.prefix.len() != self.through_byte_offset.min(64) as usize
            || self.anchor.len() != self.prefix.len()
        {
            return Ok(None);
        }
        let position = file.stream_position()?;
        let result =
            (|| {
                let prefix = bytes_at(file, 0, self.prefix.len())?;
                let anchor = bytes_at(
                    file,
                    self.through_byte_offset - self.anchor.len() as u64,
                    self.anchor.len(),
                )?;
                Ok((prefix == self.prefix && anchor == self.anchor)
                    .then_some(self.through_byte_offset))
            })();
        file.seek(SeekFrom::Start(position))?;
        result
    }
}
