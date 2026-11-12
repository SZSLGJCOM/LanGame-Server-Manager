//! Additional archive readers. Paths, output budgets and publication remain owned
//! by the existing Mod staging transaction; decoders never extract to a directory.
use super::*;
use std::io::{self, Read, Seek, SeekFrom, Write};

#[path = "commands_mod_archive_preflight.rs"]
mod preflight;

const DECODER_MEMORY_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::commands) enum AdditionalModArchiveFormat {
    SevenZ,
    Rar,
}

impl AdditionalModArchiveFormat {
    pub(in crate::commands) fn extension(self) -> &'static str {
        match self {
            Self::SevenZ => "7z",
            Self::Rar => "rar",
        }
    }
}

enum ArchiveReader {
    SevenZ {
        source: fs::File,
        archive: Box<sevenz_rust2::Archive>,
    },
    Rar(Box<unrar_rs::RarArchive>),
}

/// Recognize bytes, then validate the entire directory before assigning a format.
/// Payload checksums are verified by the extraction pass before commit.
pub(in crate::commands) fn inspect_additional_mod_archive(
    path: &Path,
) -> Result<Option<AdditionalModArchiveFormat>, String> {
    let Some((format, reader)) = open_archive(path)? else {
        return Ok(None);
    };
    let entries = reader.inspect()?;
    plan_manual_mod_archive_entries(
        entries,
        &mut ManualModStagingRegistry::default(),
        ManualModArchiveLayout::Preserve,
    )?;
    Ok(Some(format))
}

pub(super) fn extract_additional_mod_archive_to_directory(
    path: &Path,
    staging_root: &Path,
    partials_root: &Path,
    registry: &mut ManualModStagingRegistry,
    layout: ManualModArchiveLayout,
) -> Result<DirectoryCopyStats, String> {
    let (_, mut reader) = open_archive(path)?
        .ok_or("Mod archive does not contain a supported 7z or RAR signature")?;
    let entries = reader.inspect()?;
    let plan = plan_manual_mod_archive_entries(entries, registry, layout)?;
    require_mod_capacity(staging_root, plan.metrics.uncompressed_bytes)?;
    reader.extract(&plan, staging_root, partials_root)
}

fn open_archive(
    path: &Path,
) -> Result<Option<(AdditionalModArchiveFormat, ArchiveReader)>, String> {
    let metadata = validate_manual_mod_source_filesystem_entry(path)?;
    if !metadata.is_file() {
        return Err("Mod archive is not a regular file".into());
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Keep one source handle for directory inspection and payload decoding.
        // No concurrent writer/deleter may replace it while a transaction reads it.
        options.share_mode(0x1);
    }
    let mut source = options
        .open(path)
        .map_err(|error| format!("Cannot open Mod archive: {error}"))?;
    let size = source
        .metadata()
        .map_err(|error| format!("Cannot inspect Mod archive: {error}"))?
        .len();
    if size != metadata.len() {
        return Err("Mod archive changed while being opened".into());
    }
    let mut signature = [0; 8];
    let length = size.min(signature.len() as u64) as usize;
    source
        .read_exact(&mut signature[..length])
        .map_err(|error| format!("Cannot read Mod archive signature: {error}"))?;
    source.rewind().map_err(|error| error.to_string())?;
    if length >= 6 && signature[..6] == *b"7z\xbc\xaf\x27\x1c" {
        preflight::seven_z(&mut source, size)?;
        let archive = sevenz_rust2::Archive::read_with_memory_limit(
            &mut source,
            &sevenz_rust2::Password::empty(),
            (MANUAL_MOD_ARCHIVE_MAX_CENTRAL_DIRECTORY_BYTES / 1024) as usize,
        )
        .map_err(|error| format!("Cannot read 7z Mod archive: {error}"))?;
        Ok(Some((
            AdditionalModArchiveFormat::SevenZ,
            ArchiveReader::SevenZ {
                source,
                archive: Box::new(archive),
            },
        )))
    } else if length >= 7 && signature[..7] == *b"Rar!\x1a\x07\0"
        || length == 8 && signature == *b"Rar!\x1a\x07\x01\0"
    {
        preflight::rar(&mut source, size, signature[6] == 1)?;
        let mut archive = unrar_rs::RarArchive::open(source)
            .map_err(|error| format!("Cannot read RAR Mod archive: {error}"))?;
        archive.set_limits(unrar_rs::Limits {
            max_dict_size: DECODER_MEMORY_BYTES,
            ..Default::default()
        });
        archive.set_decode_mode(unrar_rs::DecodeMode::Serial);
        Ok(Some((
            AdditionalModArchiveFormat::Rar,
            ArchiveReader::Rar(Box::new(archive)),
        )))
    } else {
        Ok(None)
    }
}

impl ArchiveReader {
    fn inspect(&self) -> Result<Vec<ManualModArchiveEntryPlan>, String> {
        let mut entries = Vec::new();
        let mut paths = HashSet::new();
        let mut metadata_bytes = 0u64;
        let mut compression_budget = ManualModArchiveCompressionBudget::default();
        match self {
            Self::SevenZ { archive, .. } => {
                check_count(archive.files.len())?;
                let mut blocks = vec![ManualModArchiveMetrics::default(); archive.blocks.len()];
                for (index, entry) in archive.files.iter().enumerate() {
                    if entry.is_anti_item || archive.is_file_encrypted(index) {
                        return Err(
                            "Mod archive contains a deletion marker or encrypted entry".into()
                        );
                    }
                    let mode = if entry.has_windows_attributes {
                        if entry.windows_attributes & 0x400 != 0 {
                            return Err("Mod archive contains a reparse point".into());
                        }
                        Some(entry.windows_attributes >> 16)
                    } else {
                        None
                    };
                    let plan = inspect_entry(
                        index,
                        &entry.name,
                        entry.is_directory,
                        mode,
                        entry.compressed_size,
                        entry.size,
                        &mut paths,
                        &mut metadata_bytes,
                    )?;
                    if let Some(block) = archive.stream_map.file_block_index[index] {
                        add_metrics(&mut blocks[block], &plan)?;
                    } else if !entry.is_directory && entry.size != 0 {
                        return Err("7z entry declares bytes without a data stream".into());
                    }
                    entries.push(plan);
                }
                // A solid block owns its compressed bytes as a whole. Subsequent
                // files legitimately have compressed_size=0 and are not bombs.
                let pack_starts = archive.stream_map.block_first_pack_stream_index();
                for (index, block) in blocks.into_iter().enumerate() {
                    let start = *pack_starts
                        .get(index)
                        .ok_or("7z block has no packed stream")?;
                    let end = pack_starts
                        .get(index + 1)
                        .copied()
                        .unwrap_or(archive.pack_sizes().len());
                    let packed_bytes = archive
                        .pack_sizes()
                        .get(start..end)
                        .ok_or("7z packed stream range is invalid")?
                        .iter()
                        .try_fold(0u64, |sum, value| sum.checked_add(*value))
                        .ok_or("7z packed size overflow")?;
                    compression_budget.include(
                        block.file_count,
                        packed_bytes,
                        block.uncompressed_bytes,
                    )?;
                }
            }
            Self::Rar(archive) => {
                check_count(archive.len())?;
                let mut block = ManualModArchiveMetrics::default();
                for member in archive.indexed_member_infos() {
                    let index = member.index;
                    let entry = member.info;
                    if entry.is_encrypted {
                        return Err("Mod archive requires a password".into());
                    }
                    if entry.is_symlink || entry.is_hardlink || entry.is_file_copy {
                        return Err("Mod archive contains a link or redirected entry".into());
                    }
                    if entry.volumes.is_split() || !member.extractable {
                        return Err("Mod archive requires another RAR volume".into());
                    }
                    if entry.compression.dict_size > DECODER_MEMORY_BYTES {
                        return Err(
                            "RAR Mod archive dictionary exceeds the decoder memory budget".into(),
                        );
                    }
                    let unpacked_bytes = entry
                        .unpacked_size
                        .ok_or("RAR entry has no complete unpacked size")?;
                    if !entry.is_directory
                        && entry.compression.method == unrar_rs::CompressionMethod::Store
                        && entry.compressed_size != unpacked_bytes
                    {
                        return Err(format!(
                            "RAR Stored entry has inconsistent lengths (compressed={} bytes, uncompressed={} bytes)",
                            entry.compressed_size, unpacked_bytes,
                        ));
                    }
                    let mode = match entry.host_os {
                        unrar_rs::HostOs::Unix | unrar_rs::HostOs::Darwin => {
                            Some(entry.attributes.unix_mode())
                        }
                        _ => {
                            if entry.attributes.0 & 0x400 != 0 {
                                return Err("Mod archive contains a reparse point".into());
                            }
                            None
                        }
                    };
                    let plan = inspect_entry(
                        index,
                        &entry.raw_name,
                        entry.is_directory,
                        mode,
                        entry.compressed_size,
                        unpacked_bytes,
                        &mut paths,
                        &mut metadata_bytes,
                    )?;
                    if !entry.is_directory && !entry.compression.solid && block.file_count != 0 {
                        compression_budget.include(
                            block.file_count,
                            block.compressed_bytes,
                            block.uncompressed_bytes,
                        )?;
                        block = ManualModArchiveMetrics::default();
                    }
                    add_metrics(&mut block, &plan)?;
                    entries.push(plan);
                }
                compression_budget.include(
                    block.file_count,
                    block.compressed_bytes,
                    block.uncompressed_bytes,
                )?;
            }
        }
        Ok(entries)
    }

    fn extract(
        &mut self,
        plan: &ManualModArchivePlan,
        staging_root: &Path,
        partials_root: &Path,
    ) -> Result<DirectoryCopyStats, String> {
        let mut stats = DirectoryCopyStats::default();
        let mut completed = HashSet::new();
        let selected = plan
            .entries
            .iter()
            .map(|entry| (entry.index, entry))
            .collect::<HashMap<_, _>>();
        match self {
            Self::SevenZ { source, archive } => {
                let indices = archive
                    .files
                    .iter()
                    .enumerate()
                    .map(|(index, entry)| (entry.name.clone(), index))
                    .collect::<HashMap<_, _>>();
                let mut extract = |entry: &sevenz_rust2::ArchiveEntry, input: &mut dyn Read| {
                    let index = *indices.get(&entry.name).ok_or_else(|| {
                        io::Error::other("7z directory changed during extraction")
                    })?;
                    let result = if let Some(entry_plan) = selected.get(&index) {
                        extract_planned_entry(
                            entry_plan,
                            staging_root,
                            partials_root,
                            &mut stats,
                            &mut completed,
                            |output| {
                                io::copy(&mut input.take(entry.size.saturating_add(1)), output)
                                    .map_err(|error| error.to_string())?;
                                Ok(())
                            },
                        )
                    } else {
                        // Layout filtering must still validate/consume a solid predecessor.
                        let count = io::copy(
                            &mut input.take(entry.size.saturating_add(1)),
                            &mut io::sink(),
                        )?;
                        if count != entry.size {
                            Err("7z entry does not match its declared size".into())
                        } else {
                            Ok(())
                        }
                    };
                    result.map_err(io::Error::other)?;
                    Ok(true)
                };
                let password = sevenz_rust2::Password::empty();
                for index in 0..archive.blocks.len() {
                    sevenz_rust2::BlockDecoder::new_with_memory_limit(
                        1,
                        (DECODER_MEMORY_BYTES / 1024) as usize,
                        index,
                        archive,
                        &password,
                        source,
                    )
                    .for_each_entries(&mut extract)
                    .map_err(|error| format!("Cannot decode 7z Mod archive: {error}"))?;
                }
                for (index, entry) in archive.files.iter().enumerate() {
                    if archive.stream_map.file_block_index[index].is_none() {
                        extract(entry, &mut io::empty())
                            .map_err(|error: sevenz_rust2::Error| error.to_string())?;
                    }
                }
            }
            Self::Rar(archive) => {
                for index in 0..archive.len() {
                    let entry = archive.by_index(index).map_err(|error| error.to_string())?;
                    if let Some(entry_plan) = selected.get(&index) {
                        extract_planned_entry(
                            entry_plan,
                            staging_root,
                            partials_root,
                            &mut stats,
                            &mut completed,
                            |output| {
                                entry.copy_to(output).map_err(|error| error.to_string())?;
                                Ok(())
                            },
                        )?;
                    } else {
                        // copy_to streams and verifies; Entry:Read would spool the whole file.
                        let expected = entry.size().ok_or("RAR entry size disappeared")?;
                        let mut discard = LimitedWriter::new(io::sink(), expected);
                        entry
                            .copy_to(&mut discard)
                            .map_err(|error| error.to_string())?;
                        discard.verify()?;
                    }
                }
            }
        }
        if completed.len() != plan.entries.len()
            || stats.file_count != plan.metrics.file_count
            || stats.total_bytes != plan.metrics.uncompressed_bytes
        {
            return Err("Mod archive directory changed during extraction".into());
        }
        Ok(stats)
    }
}

fn check_count(count: usize) -> Result<(), String> {
    if count > MANUAL_MOD_ARCHIVE_MAX_ENTRY_COUNT {
        Err("Mod archive contains too many entries".into())
    } else {
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn inspect_entry(
    index: usize,
    name: &str,
    directory: bool,
    mode: Option<u32>,
    compressed_bytes: u64,
    uncompressed_bytes: u64,
    paths: &mut HashSet<String>,
    metadata_bytes: &mut u64,
) -> Result<ManualModArchiveEntryPlan, String> {
    *metadata_bytes = metadata_bytes
        .checked_add(name.len() as u64)
        .ok_or("Mod archive name accounting overflow")?;
    if *metadata_bytes > MANUAL_MOD_ARCHIVE_MAX_CENTRAL_DIRECTORY_BYTES {
        return Err("Mod archive names exceed the metadata budget".into());
    }
    if name.is_empty() || name.contains('\0') {
        return Err("Mod archive contains an empty or invalid name".into());
    }
    // Treat both archive separator spellings identically on every test host.
    let name = name.replace('\\', "/");
    if name.starts_with('/') || name.split('/').any(|part| part == "..") {
        return Err("Mod archive contains an unsafe archive entry".into());
    }
    let relative_path = normalize_manual_mod_relative_path(Path::new(&name))?;
    if relative_path.as_os_str().is_empty() {
        return Err("Mod archive contains an empty path".into());
    }
    if !paths.insert(manual_mod_relative_path_key(&relative_path)?) {
        return Err("Mod archive contains a duplicate or Windows-aliased path".into());
    }
    if directory {
        if uncompressed_bytes != 0
            || mode.is_some_and(|mode| {
                let kind = mode & 0o170_000;
                kind != 0 && kind != 0o040_000
            })
        {
            return Err("Mod archive directory contains data or a special filesystem type".into());
        }
    } else {
        validate_manual_mod_archive_regular_file_mode(mode, &relative_path)?;
    }
    uncompressed_bytes
        .checked_add(1)
        .ok_or("Mod archive size overflows extraction accounting")?;
    Ok(ManualModArchiveEntryPlan {
        index,
        relative_path,
        kind: if directory {
            ManualModArchiveEntryKind::Directory
        } else {
            ManualModArchiveEntryKind::File
        },
        compressed_bytes,
        uncompressed_bytes,
    })
}

fn add_metrics(
    metrics: &mut ManualModArchiveMetrics,
    entry: &ManualModArchiveEntryPlan,
) -> Result<(), String> {
    if entry.kind == ManualModArchiveEntryKind::File {
        metrics.file_count = metrics
            .file_count
            .checked_add(1)
            .ok_or("Mod archive count overflow")?;
        metrics.compressed_bytes = metrics
            .compressed_bytes
            .checked_add(entry.compressed_bytes)
            .ok_or("Mod archive packed size overflow")?;
        metrics.uncompressed_bytes = metrics
            .uncompressed_bytes
            .checked_add(entry.uncompressed_bytes)
            .ok_or("Mod archive unpacked size overflow")?;
    }
    Ok(())
}

fn extract_planned_entry(
    plan: &ManualModArchiveEntryPlan,
    staging_root: &Path,
    partials_root: &Path,
    stats: &mut DirectoryCopyStats,
    completed: &mut HashSet<usize>,
    decode: impl FnOnce(&mut dyn Write) -> Result<(), String>,
) -> Result<(), String> {
    if !completed.insert(plan.index) {
        return Err("Mod archive repeats an entry while decoding".into());
    }
    let output_path = staging_root.join(&plan.relative_path);
    if plan.kind == ManualModArchiveEntryKind::Directory {
        fs::create_dir_all(&output_path).map_err(|error| error.to_string())?;
        return Ok(());
    }
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut output = StagedArchiveWriter::new(partials_root, plan.uncompressed_bytes)?;
    decode(&mut output).map_err(|error| {
        format!(
            "Cannot decode Mod entry {}: {error}",
            plan.relative_path.display()
        )
    })?;
    output.finish(&output_path)?;
    stats.file_count = stats
        .file_count
        .checked_add(1)
        .ok_or("Mod archive count overflow")?;
    stats.total_bytes = stats
        .total_bytes
        .checked_add(plan.uncompressed_bytes)
        .ok_or("Mod archive extracted size overflow")?;
    Ok(())
}

struct LimitedWriter<W> {
    inner: W,
    expected: u64,
    written: u64,
}

impl<W: Write> LimitedWriter<W> {
    fn new(inner: W, expected: u64) -> Self {
        Self {
            inner,
            expected,
            written: 0,
        }
    }
    fn verify(&self) -> Result<(), String> {
        if self.written == self.expected {
            Ok(())
        } else {
            Err("Mod archive entry size differs from its directory".into())
        }
    }
}

impl<W: Write> Write for LimitedWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() as u64 > self.expected.saturating_sub(self.written) {
            return Err(io::Error::other(
                "Mod archive entry exceeded its declared extraction limit",
            ));
        }
        let count = self.inner.write(bytes)?;
        self.written += count as u64;
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

struct StagedArchiveWriter {
    output: Option<LimitedWriter<fs::File>>,
    partial: PathBuf,
    budget: ModSpaceBudget,
    published: bool,
}

impl StagedArchiveWriter {
    fn new(partials: &Path, expected: u64) -> Result<Self, String> {
        let partial = partials.join(uuid::Uuid::new_v4().simple().to_string());
        let budget = ModSpaceBudget::new(&partial)?;
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            output: Some(LimitedWriter::new(file, expected)),
            partial,
            budget,
            published: false,
        })
    }

    fn finish(mut self, destination: &Path) -> Result<(), String> {
        let output = self
            .output
            .as_mut()
            .ok_or("Mod archive output already closed")?;
        output.verify()?;
        output.inner.sync_all().map_err(|error| error.to_string())?;
        drop(self.output.take());
        fs::rename(&self.partial, destination).map_err(|error| error.to_string())?;
        self.published = true;
        Ok(())
    }
}

impl Write for StagedArchiveWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.budget
            .before_write(bytes.len())
            .map_err(io::Error::other)?;
        self.output
            .as_mut()
            .ok_or_else(|| io::Error::other("Mod archive output closed"))?
            .write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.output
            .as_mut()
            .ok_or_else(|| io::Error::other("Mod archive output closed"))?
            .flush()
    }
}

impl Drop for StagedArchiveWriter {
    fn drop(&mut self) {
        drop(self.output.take());
        if !self.published {
            let _ = fs::remove_file(&self.partial);
        }
    }
}

#[cfg(test)]
#[path = "commands_mod_archive_formats_tests.rs"]
pub(in crate::commands) mod tests;
