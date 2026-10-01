use super::*;

#[path = "commands_mod_packages.rs"]
mod online_packages;
pub(super) use online_packages::{
    ONLINE_MOD_MANIFEST, stage_online_mod_sources, verify_online_mod_dependencies,
};

const MANUAL_MOD_ARCHIVE_MAX_ENTRY_COUNT: usize = 20_000;
const MANUAL_MOD_ARCHIVE_MAX_UNCOMPRESSED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MANUAL_MOD_ARCHIVE_MAX_CENTRAL_DIRECTORY_BYTES: u64 = 64 * 1024 * 1024;
const MANUAL_MOD_ARCHIVE_CENTRAL_DIRECTORY_ALLOWANCE_BYTES: u64 =
    MANUAL_MOD_ARCHIVE_MAX_CENTRAL_DIRECTORY_BYTES;
const MANUAL_MOD_ARCHIVE_MAX_SOURCE_BYTES: u64 = MANUAL_MOD_ARCHIVE_MAX_UNCOMPRESSED_BYTES
    + MANUAL_MOD_ARCHIVE_CENTRAL_DIRECTORY_ALLOWANCE_BYTES;
const MANUAL_MOD_ARCHIVE_MAX_COMPRESSION_RATIO: u64 = 200;
#[cfg(windows)]
const WINDOWS_FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

static MANUAL_MOD_TARGET_LOCKS: OnceLock<StdMutex<HashMap<String, Arc<StdMutex<()>>>>> =
    OnceLock::new();

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct ManualModArchiveMetrics {
    file_count: usize,
    compressed_bytes: u64,
    uncompressed_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManualModArchiveEntryKind {
    Directory,
    File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManualModArchiveLayout {
    Preserve,
    ThunderstorePayload,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ManualModArchiveEntryPlan {
    index: usize,
    relative_path: PathBuf,
    kind: ManualModArchiveEntryKind,
    compressed_bytes: u64,
    uncompressed_bytes: u64,
}

#[derive(Debug)]
struct ManualModArchivePlan {
    entries: Vec<ManualModArchiveEntryPlan>,
    metrics: ManualModArchiveMetrics,
}

#[derive(Debug, Default, Clone)]
struct ManualModStagingRegistry {
    path_kinds: HashMap<String, ManualModArchiveEntryKind>,
}

#[derive(Debug)]
struct ManualModTransactionPaths {
    root: PathBuf,
    staging: PathBuf,
    partials: PathBuf,
    backup: PathBuf,
}

#[derive(Debug)]
struct ManualModCommitAction {
    destination: PathBuf,
    backup: Option<PathBuf>,
    installed: bool,
}

#[derive(Debug)]
struct ManualModCommitFailure {
    message: String,
    rollback_complete: bool,
}

#[derive(Debug)]
struct StagedManualModSource {
    source_path: PathBuf,
    target_relative_path: Option<PathBuf>,
    stats: DirectoryCopyStats,
}

pub(super) fn stage_manual_mod_sources(
    target: ResolvedManualModTarget,
    source_paths: Vec<PathBuf>,
) -> Result<ManualModStageResult, String> {
    stage_manual_mod_source_batch(
        &target,
        source_paths,
        true,
        ManualModArchiveLayout::Preserve,
        false,
    )
}

pub(super) fn stage_thunderstore_archive_to_directory(
    archive_path: PathBuf,
    target_path: PathBuf,
) -> Result<(), String> {
    let target = ResolvedManualModTarget {
        instance_id: String::from("thunderstore-download"),
        module_id: String::from("thunderstore"),
        source_label: String::from("Thunderstore"),
        target_label: String::from("downloaded package"),
        target_path,
        accepts: vec![String::from("zip")],
        id_strategy: None,
    };
    stage_manual_mod_source_batch(
        &target,
        vec![archive_path],
        true,
        ManualModArchiveLayout::ThunderstorePayload,
        false,
    )?;
    Ok(())
}

pub(super) fn stage_downloaded_workshop_items_into_manual_target(
    result: &SteamWorkshopDownloadResult,
    target: &ResolvedManualModTarget,
) -> Result<ManualModStageResult, String> {
    let mut source_paths = Vec::with_capacity(result.items.len());
    for item in &result.items {
        if !item.expected_path_exists {
            return Err(format!(
                "Steam Workshop item `{}` was not found at `{}` after download",
                item.item_id, item.expected_path
            ));
        }
        let source_path = PathBuf::from(&item.expected_path);
        if !item.item_id.bytes().all(|byte| byte.is_ascii_digit())
            || item.item_id.is_empty()
            || source_path.file_name().and_then(|name| name.to_str()) != Some(item.item_id.as_str())
        {
            return Err(format!(
                "Workshop item `{}` does not identify its own package directory",
                item.item_id
            ));
        }
        if !source_path.exists() {
            return Err(format!(
                "Steam Workshop item `{}` was not found at `{}` after download",
                item.item_id,
                source_path.display()
            ));
        }
        source_paths.push(source_path);
    }
    stage_manual_mod_source_batch(
        target,
        source_paths,
        false,
        ManualModArchiveLayout::Preserve,
        true,
    )
}

fn stage_manual_mod_source_batch(
    target: &ResolvedManualModTarget,
    source_paths: Vec<PathBuf>,
    enforce_declared_accepts: bool,
    archive_layout: ManualModArchiveLayout,
    replace_source_roots: bool,
) -> Result<ManualModStageResult, String> {
    if source_paths.is_empty() {
        return Err(String::from("no mod sources were provided"));
    }
    if enforce_declared_accepts {
        validate_manual_mod_source_inputs(&source_paths, &target.accepts)?;
    } else {
        validate_manual_mod_source_filesystem_entries(&source_paths)?;
    }

    let target_lock = manual_mod_target_lock(&target.target_path)?;
    let _target_guard = target_lock
        .lock()
        .map_err(|_| String::from("manual mod target lock poisoned"))?;

    validate_manual_mod_target_path(&target.target_path)?;
    validate_manual_mod_target_tree(&target.target_path)?;
    for source_path in &source_paths {
        validate_manual_mod_source_destination(source_path, &target.target_path)?;
    }

    let transaction = create_manual_mod_transaction(&target.target_path)?;
    let mut registry = ManualModStagingRegistry::default();
    let mut staged_sources = Vec::with_capacity(source_paths.len());
    let mut copied_file_count = 0usize;
    let mut copied_total_bytes = 0u64;
    let mut replace_roots = HashSet::new();
    for source_path in source_paths {
        let staged = match stage_manual_mod_source_to_transaction(
            &source_path,
            &transaction,
            &mut registry,
            archive_layout,
            None,
        ) {
            Ok(staged) => staged,
            Err(error) => {
                return Err(manual_mod_staging_failure(error, &transaction, true));
            }
        };
        if registry.path_kinds.contains_key(ONLINE_MOD_MANIFEST) {
            return Err(manual_mod_staging_failure(
                format!(
                    "manual mod sources may not replace the reserved ownership record {ONLINE_MOD_MANIFEST}"
                ),
                &transaction,
                true,
            ));
        }
        if replace_source_roots && let Some(relative) = &staged.target_relative_path {
            replace_roots.insert(relative.clone());
        }
        copied_file_count = copied_file_count
            .checked_add(staged.stats.file_count)
            .ok_or_else(|| {
                manual_mod_staging_failure(
                    String::from("manual mod staged file count overflowed"),
                    &transaction,
                    true,
                )
            })?;
        copied_total_bytes = copied_total_bytes
            .checked_add(staged.stats.total_bytes)
            .ok_or_else(|| {
                manual_mod_staging_failure(
                    String::from("manual mod staged byte count overflowed"),
                    &transaction,
                    true,
                )
            })?;
        staged_sources.push(staged);
    }

    if copied_file_count == 0 {
        return Err(manual_mod_staging_failure(
            String::from("no mod files were staged"),
            &transaction,
            true,
        ));
    }

    validate_manual_mod_target_path(&target.target_path)
        .map_err(|error| manual_mod_staging_failure(error, &transaction, true))?;
    validate_manual_mod_target_tree(&target.target_path)
        .map_err(|error| manual_mod_staging_failure(error, &transaction, true))?;
    let affected_root_names = staged_manual_mod_root_names(&transaction)
        .map_err(|error| manual_mod_staging_failure(error, &transaction, true))?;
    if let Err(failure) =
        commit_manual_mod_transaction(&transaction, &target.target_path, &replace_roots)
    {
        let retained = !failure.rollback_complete;
        return Err(manual_mod_staging_failure(
            failure.message,
            &transaction,
            !retained,
        ));
    }

    let target_path =
        fs::canonicalize(&target.target_path).unwrap_or_else(|_| target.target_path.clone());
    let cleanup_warning = cleanup_manual_mod_transaction(&transaction).err();
    let items = staged_sources
        .into_iter()
        .map(|staged| {
            let installed_path = staged
                .target_relative_path
                .as_ref()
                .map(|relative| target_path.join(relative))
                .unwrap_or_else(|| target_path.clone());
            ManualModStageItem {
                source_path: staged.source_path.to_string_lossy().into_owned(),
                target_path: installed_path.to_string_lossy().into_owned(),
                status: String::from("installed"),
                message: cleanup_warning.clone(),
                file_count: staged.stats.file_count,
                total_bytes: staged.stats.total_bytes,
            }
        })
        .collect();

    Ok(ManualModStageResult {
        instance_id: target.instance_id.clone(),
        module_id: target.module_id.clone(),
        source_label: target.source_label.clone(),
        target_label: target.target_label.clone(),
        target_path: target_path.to_string_lossy().into_owned(),
        affected_root_names,
        items,
        copied_file_count,
        copied_total_bytes,
    })
}

pub(super) fn validate_manual_mod_source_inputs(
    source_paths: &[PathBuf],
    declared_accepts: &[String],
) -> Result<(), String> {
    let accepted_types = declared_accepts
        .iter()
        .filter_map(|accepted| {
            let normalized = accepted.trim().trim_start_matches('.').to_lowercase();
            (!normalized.is_empty()).then_some(normalized)
        })
        .collect::<HashSet<_>>();
    if accepted_types.is_empty() {
        return Err(String::from(
            "module does not declare any accepted manual mod input types",
        ));
    }

    let mut accepted_labels = accepted_types.iter().cloned().collect::<Vec<_>>();
    accepted_labels.sort_unstable();
    for source_path in source_paths {
        let metadata = validate_manual_mod_source_filesystem_entry(source_path)?;
        let source_type = if metadata.is_dir() {
            String::from("folder")
        } else if metadata.is_file() {
            source_path
                .extension()
                .and_then(|extension| extension.to_str())
                .map(str::trim)
                .filter(|extension| !extension.is_empty())
                .map(str::to_lowercase)
                .ok_or_else(|| {
                    format!(
                        "manual mod file {} has no extension; accepted types: {}",
                        source_path.display(),
                        accepted_labels.join(", ")
                    )
                })?
        } else {
            return Err(format!(
                "manual mod source {} is not a regular file or folder",
                source_path.display()
            ));
        };

        if !accepted_types.contains(&source_type) {
            return Err(format!(
                "manual mod source {} has unsupported type `{}`; accepted types: {}",
                source_path.display(),
                source_type,
                accepted_labels.join(", ")
            ));
        }
    }
    Ok(())
}

fn validate_manual_mod_source_filesystem_entries(source_paths: &[PathBuf]) -> Result<(), String> {
    for source_path in source_paths {
        validate_manual_mod_source_filesystem_entry(source_path)?;
    }
    Ok(())
}

fn validate_manual_mod_source_filesystem_entry(source_path: &Path) -> Result<fs::Metadata, String> {
    validate_manual_mod_path_syntax(source_path)?;
    validate_manual_mod_existing_path_components(source_path, false)?;
    let metadata = fs::symlink_metadata(source_path).map_err(|error| {
        format!(
            "failed to inspect manual mod source {}: {}",
            source_path.display(),
            error
        )
    })?;
    if manual_mod_metadata_is_reparse(&metadata) {
        return Err(format!(
            "manual mod source {} is a symbolic link or reparse point",
            source_path.display()
        ));
    }
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(format!(
            "manual mod source {} is not a regular file or folder",
            source_path.display()
        ));
    }
    Ok(metadata)
}

fn stage_manual_mod_source_to_transaction(
    source_path: &Path,
    transaction: &ManualModTransactionPaths,
    registry: &mut ManualModStagingRegistry,
    archive_layout: ManualModArchiveLayout,
    target_name: Option<&std::ffi::OsStr>,
) -> Result<StagedManualModSource, String> {
    let metadata = validate_manual_mod_source_filesystem_entry(source_path)?;
    if metadata.is_file()
        && source_path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
    {
        let stats = extract_zip_archive_to_directory(
            source_path,
            &transaction.staging,
            &transaction.partials,
            registry,
            archive_layout,
        )?;
        return Ok(StagedManualModSource {
            source_path: source_path.to_path_buf(),
            target_relative_path: None,
            stats,
        });
    }

    let file_name = target_name
        .or_else(|| source_path.file_name())
        .ok_or_else(|| format!("path has no file name: {}", source_path.display()))?;
    validate_manual_mod_windows_component(file_name)?;
    let relative_path = PathBuf::from(file_name);
    let target_path = transaction.staging.join(&relative_path);
    let stats = if metadata.is_dir() {
        registry.register_directory(&relative_path)?;
        fs::create_dir(&target_path).map_err(|error| {
            format!(
                "failed to prepare staged mod folder {}: {}",
                target_path.display(),
                error
            )
        })?;
        copy_manual_mod_directory_secure(
            source_path,
            &target_path,
            &relative_path,
            &transaction.partials,
            registry,
        )?
    } else {
        registry.register_file(&relative_path)?;
        let copied_bytes =
            copy_manual_mod_file_to_staging(source_path, &target_path, &transaction.partials)?;
        DirectoryCopyStats {
            file_count: 1,
            total_bytes: copied_bytes,
        }
    };

    Ok(StagedManualModSource {
        source_path: source_path.to_path_buf(),
        target_relative_path: Some(relative_path),
        stats,
    })
}

fn copy_manual_mod_directory_secure(
    source_dir: &Path,
    target_dir: &Path,
    target_relative_dir: &Path,
    partials_root: &Path,
    registry: &mut ManualModStagingRegistry,
) -> Result<DirectoryCopyStats, String> {
    let mut entries = fs::read_dir(source_dir)
        .map_err(|error| format!("failed to read {}: {}", source_dir.display(), error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to inspect {}: {}", source_dir.display(), error))?;
    entries.sort_by_key(|entry| entry.file_name().to_string_lossy().to_lowercase());

    let mut stats = DirectoryCopyStats::default();
    for entry in entries {
        let source_path = entry.path();
        let file_name = entry.file_name();
        validate_manual_mod_windows_component(&file_name)?;
        let metadata = fs::symlink_metadata(&source_path).map_err(|error| {
            format!(
                "failed to inspect manual mod source {}: {}",
                source_path.display(),
                error
            )
        })?;
        if manual_mod_metadata_is_reparse(&metadata) {
            return Err(format!(
                "manual mod source tree contains a symbolic link or reparse point: {}",
                source_path.display()
            ));
        }

        let relative_path = target_relative_dir.join(&file_name);
        let target_path = target_dir.join(&file_name);
        if metadata.is_dir() {
            registry.register_directory(&relative_path)?;
            fs::create_dir(&target_path).map_err(|error| {
                format!(
                    "failed to prepare staged directory {}: {}",
                    target_path.display(),
                    error
                )
            })?;
            let nested = copy_manual_mod_directory_secure(
                &source_path,
                &target_path,
                &relative_path,
                partials_root,
                registry,
            )?;
            stats.file_count = stats
                .file_count
                .checked_add(nested.file_count)
                .ok_or_else(|| String::from("manual mod folder file count overflowed"))?;
            stats.total_bytes = stats
                .total_bytes
                .checked_add(nested.total_bytes)
                .ok_or_else(|| String::from("manual mod folder byte count overflowed"))?;
            continue;
        }
        if !metadata.is_file() {
            return Err(format!(
                "manual mod source tree contains a special filesystem entry: {}",
                source_path.display()
            ));
        }

        registry.register_file(&relative_path)?;
        let copied_bytes =
            copy_manual_mod_file_to_staging(&source_path, &target_path, partials_root)?;
        stats.file_count = stats
            .file_count
            .checked_add(1)
            .ok_or_else(|| String::from("manual mod folder file count overflowed"))?;
        stats.total_bytes = stats
            .total_bytes
            .checked_add(copied_bytes)
            .ok_or_else(|| String::from("manual mod folder byte count overflowed"))?;
    }
    Ok(stats)
}

fn copy_manual_mod_file_to_staging(
    source_path: &Path,
    target_path: &Path,
    partials_root: &Path,
) -> Result<u64, String> {
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "failed to prepare staged mod target {}: {}",
                parent.display(),
                error
            )
        })?;
    }
    let partial_path = partials_root.join(uuid::Uuid::new_v4().simple().to_string());
    let copied_bytes = match fs::copy(source_path, &partial_path) {
        Ok(copied_bytes) => copied_bytes,
        Err(error) => {
            let _ = fs::remove_file(&partial_path);
            return Err(format!(
                "failed to copy {} into staging: {}",
                source_path.display(),
                error
            ));
        }
    };
    if let Err(error) = fs::rename(&partial_path, target_path) {
        let _ = fs::remove_file(&partial_path);
        return Err(format!(
            "failed to finalize staged mod file {}: {}",
            target_path.display(),
            error
        ));
    }
    Ok(copied_bytes)
}

fn extract_zip_archive_to_directory(
    archive_path: &Path,
    staging_root: &Path,
    partials_root: &Path,
    registry: &mut ManualModStagingRegistry,
    archive_layout: ManualModArchiveLayout,
) -> Result<DirectoryCopyStats, String> {
    let source_metadata = fs::symlink_metadata(archive_path).map_err(|error| {
        format!(
            "failed to inspect mod archive {}: {}",
            archive_path.display(),
            error
        )
    })?;
    validate_manual_mod_archive_source_size(archive_path, source_metadata.len())?;
    let mut file = fs::File::open(archive_path).map_err(|error| {
        format!(
            "failed to open mod archive {}: {}",
            archive_path.display(),
            error
        )
    })?;
    let opened_size = file
        .metadata()
        .map_err(|error| {
            format!(
                "failed to inspect opened mod archive {}: {}",
                archive_path.display(),
                error
            )
        })?
        .len();
    validate_manual_mod_archive_source_size(archive_path, opened_size)?;
    if opened_size != source_metadata.len() {
        return Err(format!(
            "manual mod archive {} changed while it was being opened",
            archive_path.display()
        ));
    }
    preflight_manual_mod_archive_container(&mut file, archive_path, opened_size)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| {
        format!(
            "failed to read mod archive {}: {}",
            archive_path.display(),
            error
        )
    })?;
    let plan = inspect_manual_mod_archive(&mut archive, archive_path, registry, archive_layout)?;
    let mut stats = DirectoryCopyStats::default();
    for entry_plan in &plan.entries {
        let output_path = staging_root.join(&entry_plan.relative_path);
        if entry_plan.kind == ManualModArchiveEntryKind::Directory {
            fs::create_dir_all(&output_path).map_err(|error| {
                format!(
                    "failed to prepare staged archive directory {}: {}",
                    output_path.display(),
                    error
                )
            })?;
            continue;
        }

        let mut entry = archive.by_index(entry_plan.index).map_err(|error| {
            format!(
                "failed to read entry {} from {}: {}",
                entry_plan.index,
                archive_path.display(),
                error
            )
        })?;
        if entry.size() != entry_plan.uncompressed_bytes
            || entry.compressed_size() != entry_plan.compressed_bytes
        {
            return Err(format!(
                "manual mod archive {} changed while it was being staged",
                archive_path.display()
            ));
        }
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "failed to prepare staged archive target {}: {}",
                    parent.display(),
                    error
                )
            })?;
        }
        let remaining_bytes = MANUAL_MOD_ARCHIVE_MAX_UNCOMPRESSED_BYTES
            .checked_sub(stats.total_bytes)
            .ok_or_else(|| String::from("manual mod archive extracted size exceeded its limit"))?;
        let copied_bytes = extract_manual_mod_archive_file(
            &mut entry,
            entry_plan,
            &output_path,
            partials_root,
            remaining_bytes,
        )?;
        stats.file_count = stats
            .file_count
            .checked_add(1)
            .ok_or_else(|| String::from("manual mod archive file count overflowed"))?;
        stats.total_bytes = stats
            .total_bytes
            .checked_add(copied_bytes)
            .ok_or_else(|| String::from("manual mod archive extracted size overflowed"))?;
        if stats.total_bytes > MANUAL_MOD_ARCHIVE_MAX_UNCOMPRESSED_BYTES {
            return Err(String::from(
                "manual mod archive extracted size exceeded its limit",
            ));
        }
    }
    if stats.file_count != plan.metrics.file_count
        || stats.total_bytes != plan.metrics.uncompressed_bytes
    {
        return Err(format!(
            "manual mod archive changed during extraction: expected {} files and {} bytes, extracted {} files and {} bytes",
            plan.metrics.file_count,
            plan.metrics.uncompressed_bytes,
            stats.file_count,
            stats.total_bytes
        ));
    }
    Ok(stats)
}

fn validate_manual_mod_archive_source_size(
    archive_path: &Path,
    source_bytes: u64,
) -> Result<(), String> {
    if source_bytes > MANUAL_MOD_ARCHIVE_MAX_SOURCE_BYTES {
        return Err(format!(
            "manual mod archive {} is too large: {} bytes exceeds the source limit of {} bytes",
            archive_path.display(),
            source_bytes,
            MANUAL_MOD_ARCHIVE_MAX_SOURCE_BYTES
        ));
    }
    Ok(())
}

fn preflight_manual_mod_archive_container(
    file: &mut fs::File,
    archive_path: &Path,
    source_bytes: u64,
) -> Result<(), String> {
    const EOCD_SIGNATURE: &[u8; 4] = b"PK\x05\x06";
    const EOCD_FIXED_BYTES: usize = 22;
    const ZIP_MAX_COMMENT_BYTES: usize = u16::MAX as usize;
    const ZIP64_TRAILER_BYTES: usize = 20 + 56;

    let tail_bytes = source_bytes
        .min((EOCD_FIXED_BYTES + ZIP_MAX_COMMENT_BYTES + ZIP64_TRAILER_BYTES) as u64)
        as usize;
    if tail_bytes < EOCD_FIXED_BYTES {
        return Err(format!(
            "manual mod archive {} is too short to contain a ZIP directory",
            archive_path.display()
        ));
    }
    let tail_start = source_bytes - tail_bytes as u64;
    file.seek(SeekFrom::Start(tail_start)).map_err(|error| {
        format!(
            "failed to inspect ZIP directory for {}: {}",
            archive_path.display(),
            error
        )
    })?;
    let mut tail = vec![0u8; tail_bytes];
    file.read_exact(&mut tail).map_err(|error| {
        format!(
            "failed to read ZIP directory trailer for {}: {}",
            archive_path.display(),
            error
        )
    })?;

    let mut found_eocd = false;
    for offset in (0..=tail.len() - EOCD_FIXED_BYTES).rev() {
        if &tail[offset..offset + 4] != EOCD_SIGNATURE {
            continue;
        }
        let Some(comment_bytes) = read_manual_mod_zip_u16(&tail, offset + 20) else {
            continue;
        };
        let Some(record_end) = offset
            .checked_add(EOCD_FIXED_BYTES)
            .and_then(|value| value.checked_add(comment_bytes as usize))
        else {
            continue;
        };
        if record_end > tail.len() {
            continue;
        }
        let disk_number = read_manual_mod_zip_u16(&tail, offset + 4).unwrap_or(u16::MAX);
        let directory_disk = read_manual_mod_zip_u16(&tail, offset + 6).unwrap_or(u16::MAX);
        let disk_entries = read_manual_mod_zip_u16(&tail, offset + 8).unwrap_or(u16::MAX);
        let entry_count = read_manual_mod_zip_u16(&tail, offset + 10).unwrap_or(u16::MAX);
        let directory_bytes = read_manual_mod_zip_u32(&tail, offset + 12).unwrap_or(u32::MAX);
        let directory_offset = read_manual_mod_zip_u32(&tail, offset + 16).unwrap_or(u32::MAX);
        let uses_zip64 = disk_number == u16::MAX
            || directory_disk == u16::MAX
            || disk_entries == u16::MAX
            || entry_count == u16::MAX
            || directory_bytes == u32::MAX
            || directory_offset == u32::MAX;
        let (
            actual_entry_count,
            actual_directory_bytes,
            directory_record_end,
            relative_directory_offset,
        ) = if uses_zip64 {
            read_manual_mod_zip64_directory_metrics(&tail, tail_start, offset, archive_path)?
        } else {
            if disk_number != directory_disk || disk_entries != entry_count {
                return Err(format!(
                    "manual mod archive {} uses an unsupported multi-disk ZIP directory",
                    archive_path.display()
                ));
            }
            (
                u64::from(entry_count),
                u64::from(directory_bytes),
                tail_start + offset as u64,
                u64::from(directory_offset),
            )
        };
        if !manual_mod_zip_directory_layout_is_plausible(
            file,
            actual_entry_count,
            actual_directory_bytes,
            relative_directory_offset,
            directory_record_end,
        )? {
            continue;
        }
        found_eocd = true;
        validate_manual_mod_archive_directory_metrics(
            archive_path,
            actual_entry_count,
            actual_directory_bytes,
        )?;
    }

    if !found_eocd {
        return Err(format!(
            "manual mod archive {} has no valid ZIP directory trailer",
            archive_path.display()
        ));
    }
    Ok(())
}

fn read_manual_mod_zip64_directory_metrics(
    tail: &[u8],
    tail_start: u64,
    eocd_offset: usize,
    archive_path: &Path,
) -> Result<(u64, u64, u64, u64), String> {
    const LOCATOR_SIGNATURE: &[u8; 4] = b"PK\x06\x07";
    const ZIP64_SIGNATURE: &[u8; 4] = b"PK\x06\x06";
    const LOCATOR_BYTES: usize = 20;
    const ZIP64_FIXED_BYTES: usize = 56;

    let locator_offset = eocd_offset.checked_sub(LOCATOR_BYTES).ok_or_else(|| {
        format!(
            "manual mod archive {} has an incomplete ZIP64 directory locator",
            archive_path.display()
        )
    })?;
    if &tail[locator_offset..locator_offset + 4] != LOCATOR_SIGNATURE {
        return Err(format!(
            "manual mod archive {} has no valid ZIP64 directory locator",
            archive_path.display()
        ));
    }
    let disk_count = read_manual_mod_zip_u32(tail, locator_offset + 16).unwrap_or(u32::MAX);
    if disk_count != 1 {
        return Err(format!(
            "manual mod archive {} uses an unsupported multi-disk ZIP64 directory",
            archive_path.display()
        ));
    }

    let locator_absolute = tail_start + locator_offset as u64;
    let mut zip64_offset = None;
    for candidate in (0..locator_offset).rev() {
        if candidate + ZIP64_FIXED_BYTES > tail.len()
            || &tail[candidate..candidate + 4] != ZIP64_SIGNATURE
        {
            continue;
        }
        let Some(record_bytes) = read_manual_mod_zip_u64(tail, candidate + 4) else {
            continue;
        };
        let Some(record_end) = (tail_start + candidate as u64)
            .checked_add(12)
            .and_then(|value| value.checked_add(record_bytes))
        else {
            continue;
        };
        if record_bytes >= 44 && record_end == locator_absolute {
            zip64_offset = Some(candidate);
            break;
        }
    }
    let zip64_offset = zip64_offset.ok_or_else(|| {
        format!(
            "manual mod archive {} has no bounded ZIP64 directory record",
            archive_path.display()
        )
    })?;
    let disk_number = read_manual_mod_zip_u32(tail, zip64_offset + 16).unwrap_or(u32::MAX);
    let directory_disk = read_manual_mod_zip_u32(tail, zip64_offset + 20).unwrap_or(u32::MAX);
    let disk_entries = read_manual_mod_zip_u64(tail, zip64_offset + 24).unwrap_or(u64::MAX);
    let entry_count = read_manual_mod_zip_u64(tail, zip64_offset + 32).unwrap_or(u64::MAX);
    let directory_bytes = read_manual_mod_zip_u64(tail, zip64_offset + 40).unwrap_or(u64::MAX);
    let directory_offset = read_manual_mod_zip_u64(tail, zip64_offset + 48).unwrap_or(u64::MAX);
    if disk_number != directory_disk || disk_entries != entry_count {
        return Err(format!(
            "manual mod archive {} uses an unsupported multi-disk ZIP64 directory",
            archive_path.display()
        ));
    }
    Ok((
        entry_count,
        directory_bytes,
        tail_start + zip64_offset as u64,
        directory_offset,
    ))
}

fn manual_mod_zip_directory_layout_is_plausible(
    file: &mut fs::File,
    entry_count: u64,
    directory_bytes: u64,
    relative_directory_offset: u64,
    directory_record_end: u64,
) -> Result<bool, String> {
    if entry_count == 0 {
        return Ok(true);
    }
    let Some(minimum_directory_bytes) = entry_count.checked_mul(46) else {
        return Ok(false);
    };
    if directory_bytes < minimum_directory_bytes {
        return Ok(false);
    }
    let Some(directory_start) = directory_record_end.checked_sub(directory_bytes) else {
        return Ok(false);
    };
    if directory_start < relative_directory_offset {
        return Ok(false);
    }
    file.seek(SeekFrom::Start(directory_start))
        .map_err(|error| format!("failed to seek to ZIP central directory: {error}"))?;
    let mut signature = [0u8; 4];
    match file.read_exact(&mut signature) {
        Ok(()) => Ok(signature == *b"PK\x01\x02"),
        Err(error) if error.kind() == ErrorKind::UnexpectedEof => Ok(false),
        Err(error) => Err(format!("failed to read ZIP central directory: {error}")),
    }
}

fn validate_manual_mod_archive_directory_metrics(
    archive_path: &Path,
    entry_count: u64,
    directory_bytes: u64,
) -> Result<(), String> {
    if entry_count > MANUAL_MOD_ARCHIVE_MAX_ENTRY_COUNT as u64 {
        return Err(format!(
            "manual mod archive {} contains too many directory entries: {} exceeds {}",
            archive_path.display(),
            entry_count,
            MANUAL_MOD_ARCHIVE_MAX_ENTRY_COUNT
        ));
    }
    if directory_bytes > MANUAL_MOD_ARCHIVE_MAX_CENTRAL_DIRECTORY_BYTES {
        return Err(format!(
            "manual mod archive {} has an oversized central directory: {} bytes exceeds {} bytes",
            archive_path.display(),
            directory_bytes,
            MANUAL_MOD_ARCHIVE_MAX_CENTRAL_DIRECTORY_BYTES
        ));
    }
    let minimum_directory_bytes = entry_count.saturating_mul(46);
    if directory_bytes < minimum_directory_bytes {
        return Err(format!(
            "manual mod archive {} has inconsistent ZIP directory metrics",
            archive_path.display()
        ));
    }
    Ok(())
}

fn read_manual_mod_zip_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_manual_mod_zip_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn read_manual_mod_zip_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        bytes.get(offset..offset + 8)?.try_into().ok()?,
    ))
}

fn extract_manual_mod_archive_file(
    entry: &mut zip::read::ZipFile<'_, fs::File>,
    plan: &ManualModArchiveEntryPlan,
    output_path: &Path,
    partials_root: &Path,
    remaining_bytes: u64,
) -> Result<u64, String> {
    let declared_limit = plan.uncompressed_bytes.min(remaining_bytes);
    let read_limit = declared_limit
        .checked_add(1)
        .ok_or_else(|| String::from("manual mod archive entry limit overflowed"))?;
    let partial_path = partials_root.join(uuid::Uuid::new_v4().simple().to_string());
    let mut output = fs::File::create(&partial_path).map_err(|error| {
        format!(
            "failed to create temporary archive output {}: {}",
            partial_path.display(),
            error
        )
    })?;
    let copy_result = {
        let mut bounded_entry = entry.take(read_limit);
        std::io::copy(&mut bounded_entry, &mut output)
    };
    let copied_bytes = match copy_result {
        Ok(copied_bytes) => copied_bytes,
        Err(error) => {
            drop(output);
            let _ = fs::remove_file(&partial_path);
            return Err(format!(
                "failed to extract archive entry {}: {}",
                plan.relative_path.display(),
                error
            ));
        }
    };
    if copied_bytes > declared_limit {
        drop(output);
        let _ = fs::remove_file(&partial_path);
        return Err(format!(
            "archive entry {} exceeded its declared or cumulative extraction limit",
            plan.relative_path.display()
        ));
    }
    if copied_bytes != plan.uncompressed_bytes {
        drop(output);
        let _ = fs::remove_file(&partial_path);
        return Err(format!(
            "archive entry {} declared {} bytes but extracted {} bytes",
            plan.relative_path.display(),
            plan.uncompressed_bytes,
            copied_bytes
        ));
    }
    if let Err(error) = output.sync_all() {
        drop(output);
        let _ = fs::remove_file(&partial_path);
        return Err(format!(
            "failed to flush staged archive entry {}: {}",
            plan.relative_path.display(),
            error
        ));
    }
    drop(output);
    if let Err(error) = fs::rename(&partial_path, output_path) {
        let _ = fs::remove_file(&partial_path);
        return Err(format!(
            "failed to finalize staged archive entry {}: {}",
            output_path.display(),
            error
        ));
    }
    Ok(copied_bytes)
}

fn inspect_manual_mod_archive(
    archive: &mut zip::ZipArchive<fs::File>,
    archive_path: &Path,
    registry: &mut ManualModStagingRegistry,
    archive_layout: ManualModArchiveLayout,
) -> Result<ManualModArchivePlan, String> {
    if archive.len() > MANUAL_MOD_ARCHIVE_MAX_ENTRY_COUNT {
        return Err(format!(
            "manual mod archive {} contains too many entries: {} exceeds {}",
            archive_path.display(),
            archive.len(),
            MANUAL_MOD_ARCHIVE_MAX_ENTRY_COUNT
        ));
    }

    let mut archive_metrics = ManualModArchiveMetrics::default();
    let mut archive_entries = Vec::with_capacity(archive.len());
    let mut archive_paths = HashSet::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| {
            format!(
                "failed to inspect entry {} from {}: {}",
                index,
                archive_path.display(),
                error
            )
        })?;
        let entry_name = entry.name().to_string();
        let Some(enclosed_name) = entry.enclosed_name() else {
            return Err(format!(
                "refusing to extract unsafe archive entry `{}`",
                entry_name
            ));
        };
        let relative_path = normalize_manual_mod_relative_path(&enclosed_name)?;
        if relative_path.as_os_str().is_empty() {
            continue;
        }
        let path_key = manual_mod_relative_path_key(&relative_path)?;
        if !archive_paths.insert(path_key) {
            return Err(format!(
                "manual mod archive contains a duplicate or Windows-aliased path: {}",
                relative_path.display()
            ));
        }
        if entry.is_symlink() {
            return Err(format!(
                "manual mod archive contains a symbolic link entry: {}",
                relative_path.display()
            ));
        }

        let kind = if entry.is_dir() {
            ManualModArchiveEntryKind::Directory
        } else {
            validate_manual_mod_archive_regular_file_mode(entry.unix_mode(), &relative_path)?;
            ManualModArchiveEntryKind::File
        };
        archive_entries.push(ManualModArchiveEntryPlan {
            index,
            relative_path,
            kind,
            compressed_bytes: entry.compressed_size(),
            uncompressed_bytes: entry.size(),
        });
        if kind == ManualModArchiveEntryKind::Directory {
            continue;
        }

        validate_manual_mod_archive_metrics(1, entry.compressed_size(), entry.size())
            .map_err(|error| format!("unsafe archive entry `{}`: {}", entry_name, error))?;
        archive_metrics.file_count = archive_metrics
            .file_count
            .checked_add(1)
            .ok_or_else(|| String::from("manual mod archive file count overflowed"))?;
        archive_metrics.compressed_bytes = archive_metrics
            .compressed_bytes
            .checked_add(entry.compressed_size())
            .ok_or_else(|| String::from("manual mod archive compressed size overflowed"))?;
        archive_metrics.uncompressed_bytes = archive_metrics
            .uncompressed_bytes
            .checked_add(entry.size())
            .ok_or_else(|| String::from("manual mod archive uncompressed size overflowed"))?;
        validate_manual_mod_archive_metrics(
            archive_metrics.file_count,
            archive_metrics.compressed_bytes,
            archive_metrics.uncompressed_bytes,
        )?;
    }

    let payload_prefix = match archive_layout {
        ManualModArchiveLayout::Preserve => None,
        ManualModArchiveLayout::ThunderstorePayload => {
            let entry_names = archive_entries
                .iter()
                .map(|entry| entry.relative_path.to_string_lossy().replace('\\', "/"))
                .collect::<Vec<_>>();
            select_thunderstore_payload_prefix(&entry_names).map(PathBuf::from)
        }
    };
    let mut metrics = ManualModArchiveMetrics::default();
    let mut entries = Vec::with_capacity(archive_entries.len());
    let mut planned_registry = registry.clone();
    for mut entry in archive_entries {
        if let Some(prefix) = payload_prefix.as_deref() {
            let Ok(relative_path) = entry.relative_path.strip_prefix(prefix) else {
                continue;
            };
            if relative_path.as_os_str().is_empty() {
                continue;
            }
            entry.relative_path = relative_path.to_path_buf();
        }
        match entry.kind {
            ManualModArchiveEntryKind::Directory => {
                planned_registry.register_directory(&entry.relative_path)?;
            }
            ManualModArchiveEntryKind::File => {
                planned_registry.register_file(&entry.relative_path)?;
                metrics.file_count = metrics
                    .file_count
                    .checked_add(1)
                    .ok_or_else(|| String::from("manual mod archive file count overflowed"))?;
                metrics.compressed_bytes = metrics
                    .compressed_bytes
                    .checked_add(entry.compressed_bytes)
                    .ok_or_else(|| String::from("manual mod archive compressed size overflowed"))?;
                metrics.uncompressed_bytes = metrics
                    .uncompressed_bytes
                    .checked_add(entry.uncompressed_bytes)
                    .ok_or_else(|| {
                        String::from("manual mod archive uncompressed size overflowed")
                    })?;
                validate_manual_mod_archive_metrics(
                    metrics.file_count,
                    metrics.compressed_bytes,
                    metrics.uncompressed_bytes,
                )?;
            }
        }
        entries.push(entry);
    }
    *registry = planned_registry;
    Ok(ManualModArchivePlan { entries, metrics })
}

pub(super) fn select_thunderstore_payload_prefix(entry_names: &[String]) -> Option<&'static str> {
    if entry_names
        .iter()
        .any(|name| name.starts_with("BepInEx/plugins/"))
    {
        return Some("BepInEx/plugins/");
    }
    if entry_names.iter().any(|name| name.starts_with("plugins/")) {
        return Some("plugins/");
    }
    None
}

pub(super) fn validate_manual_mod_archive_metrics(
    file_count: usize,
    compressed_bytes: u64,
    uncompressed_bytes: u64,
) -> Result<(), String> {
    if file_count > MANUAL_MOD_ARCHIVE_MAX_ENTRY_COUNT {
        return Err(format!(
            "archive file count {} exceeds the limit of {}",
            file_count, MANUAL_MOD_ARCHIVE_MAX_ENTRY_COUNT
        ));
    }
    if uncompressed_bytes > MANUAL_MOD_ARCHIVE_MAX_UNCOMPRESSED_BYTES {
        return Err(format!(
            "archive uncompressed size {} bytes exceeds the limit of {} bytes",
            uncompressed_bytes, MANUAL_MOD_ARCHIVE_MAX_UNCOMPRESSED_BYTES
        ));
    }
    if uncompressed_bytes > 0
        && (compressed_bytes == 0
            || uncompressed_bytes
                > compressed_bytes.saturating_mul(MANUAL_MOD_ARCHIVE_MAX_COMPRESSION_RATIO))
    {
        return Err(format!(
            "archive compression ratio exceeds the limit of {}:1",
            MANUAL_MOD_ARCHIVE_MAX_COMPRESSION_RATIO
        ));
    }
    Ok(())
}

impl ManualModStagingRegistry {
    fn register_directory(&mut self, relative_path: &Path) -> Result<(), String> {
        let components = manual_mod_relative_path_components(relative_path)?;
        if components.is_empty() {
            return Ok(());
        }
        for index in 1..=components.len() {
            let key = components[..index].join("/");
            if self.path_kinds.get(&key) == Some(&ManualModArchiveEntryKind::File) {
                return Err(format!(
                    "staged mod paths conflict at {}",
                    relative_path.display()
                ));
            }
            self.path_kinds
                .entry(key)
                .or_insert(ManualModArchiveEntryKind::Directory);
        }
        Ok(())
    }

    fn register_file(&mut self, relative_path: &Path) -> Result<(), String> {
        let components = manual_mod_relative_path_components(relative_path)?;
        let Some((file_name, parent_components)) = components.split_last() else {
            return Err(String::from("staged mod file path is empty"));
        };
        for index in 1..=parent_components.len() {
            let key = parent_components[..index].join("/");
            if self.path_kinds.get(&key) == Some(&ManualModArchiveEntryKind::File) {
                return Err(format!(
                    "staged mod paths conflict at {}",
                    relative_path.display()
                ));
            }
            self.path_kinds
                .entry(key)
                .or_insert(ManualModArchiveEntryKind::Directory);
        }
        let key = if parent_components.is_empty() {
            file_name.clone()
        } else {
            format!("{}/{}", parent_components.join("/"), file_name)
        };
        if self.path_kinds.contains_key(&key) {
            return Err(format!(
                "staged mod paths contain a duplicate or Windows alias: {}",
                relative_path.display()
            ));
        }
        self.path_kinds.insert(key, ManualModArchiveEntryKind::File);
        Ok(())
    }
}

fn normalize_manual_mod_relative_path(path: &Path) -> Result<PathBuf, String> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(value) => {
                validate_manual_mod_windows_component(value)?;
                normalized.push(value);
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    return Err(format!(
                        "manual mod path escapes its target: {}",
                        path.display()
                    ));
                }
            }
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                return Err(format!(
                    "manual mod path must be relative: {}",
                    path.display()
                ));
            }
        }
    }
    Ok(normalized)
}

fn manual_mod_relative_path_components(path: &Path) -> Result<Vec<String>, String> {
    let normalized = normalize_manual_mod_relative_path(path)?;
    normalized
        .components()
        .map(|component| match component {
            std::path::Component::Normal(value) => value
                .to_str()
                .map(str::to_lowercase)
                .ok_or_else(|| format!("manual mod path is not valid Unicode: {}", path.display())),
            _ => Err(format!("manual mod path is invalid: {}", path.display())),
        })
        .collect()
}

fn manual_mod_relative_path_key(path: &Path) -> Result<String, String> {
    Ok(manual_mod_relative_path_components(path)?.join("/"))
}

fn validate_manual_mod_windows_component(component: &std::ffi::OsStr) -> Result<(), String> {
    let text = component
        .to_str()
        .ok_or_else(|| String::from("manual mod path component is not valid Unicode"))?;
    if text.contains(':') {
        return Err(format!(
            "manual mod path component `{}` contains a Windows alternate data stream separator",
            text
        ));
    }
    if text.ends_with('.') || text.ends_with(' ') {
        return Err(format!(
            "manual mod path component `{}` has a Windows-ambiguous trailing character",
            text
        ));
    }
    let device_stem = text
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(['.', ' '])
        .to_ascii_uppercase();
    let numbered_device = device_stem.len() == 4
        && (device_stem.as_bytes().starts_with(b"COM")
            || device_stem.as_bytes().starts_with(b"LPT"))
        && matches!(device_stem.as_bytes()[3], b'1'..=b'9');
    if matches!(device_stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") || numbered_device {
        return Err(format!(
            "manual mod path component `{}` is a reserved Windows device name",
            text
        ));
    }
    Ok(())
}

fn validate_manual_mod_path_syntax(path: &Path) -> Result<(), String> {
    for component in path.components() {
        match component {
            std::path::Component::Normal(value) => {
                validate_manual_mod_windows_component(value)?;
            }
            std::path::Component::ParentDir | std::path::Component::CurDir => {
                return Err(format!(
                    "manual mod path contains a relative traversal component: {}",
                    path.display()
                ));
            }
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {}
        }
    }
    Ok(())
}

fn validate_manual_mod_archive_regular_file_mode(
    unix_mode: Option<u32>,
    relative_path: &Path,
) -> Result<(), String> {
    const UNIX_FILE_TYPE_MASK: u32 = 0o170_000;
    const UNIX_REGULAR_FILE: u32 = 0o100_000;
    if let Some(mode) = unix_mode {
        let file_type = mode & UNIX_FILE_TYPE_MASK;
        if file_type != 0 && file_type != UNIX_REGULAR_FILE {
            return Err(format!(
                "manual mod archive contains a special filesystem entry: {}",
                relative_path.display()
            ));
        }
    }
    Ok(())
}

fn manual_mod_metadata_is_reparse(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & WINDOWS_FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn validate_manual_mod_target_path(target_path: &Path) -> Result<(), String> {
    validate_manual_mod_path_syntax(target_path)?;
    if target_path.file_name().is_none() {
        return Err(format!(
            "manual mod target cannot be a filesystem root: {}",
            target_path.display()
        ));
    }
    validate_manual_mod_existing_path_components(target_path, true)
}

fn validate_manual_mod_existing_path_components(
    path: &Path,
    final_must_be_directory: bool,
) -> Result<(), String> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if matches!(
            component,
            std::path::Component::Prefix(_) | std::path::Component::RootDir
        ) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if manual_mod_metadata_is_reparse(&metadata) {
                    return Err(format!(
                        "manual mod path contains a symbolic link or reparse point: {}",
                        current.display()
                    ));
                }
                if current != path && !metadata.is_dir() {
                    return Err(format!(
                        "manual mod path parent is not a directory: {}",
                        current.display()
                    ));
                }
                if current == path && final_must_be_directory && !metadata.is_dir() {
                    return Err(format!(
                        "manual mod target is not a directory: {}",
                        current.display()
                    ));
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "failed to inspect manual mod path {}: {}",
                    current.display(),
                    error
                ));
            }
        }
    }
    Ok(())
}

fn validate_manual_mod_target_tree(target_path: &Path) -> Result<(), String> {
    let root_metadata = match fs::symlink_metadata(target_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "failed to inspect manual mod target {}: {}",
                target_path.display(),
                error
            ));
        }
    };
    if manual_mod_metadata_is_reparse(&root_metadata) || !root_metadata.is_dir() {
        return Err(format!(
            "manual mod target is not a safe directory: {}",
            target_path.display()
        ));
    }

    let mut pending = vec![target_path.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory).map_err(|error| {
            format!(
                "failed to inspect manual mod target tree {}: {}",
                directory.display(),
                error
            )
        })?;
        for entry in entries {
            let path = entry
                .map_err(|error| {
                    format!(
                        "failed to inspect manual mod target tree {}: {}",
                        directory.display(),
                        error
                    )
                })?
                .path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                format!(
                    "failed to inspect manual mod target entry {}: {}",
                    path.display(),
                    error
                )
            })?;
            if manual_mod_metadata_is_reparse(&metadata) {
                return Err(format!(
                    "manual mod target tree contains a symbolic link or reparse point: {}",
                    path.display()
                ));
            }
            if metadata.is_dir() {
                pending.push(path);
            } else if !metadata.is_file() {
                return Err(format!(
                    "manual mod target tree contains a special filesystem entry: {}",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

fn validate_manual_mod_source_destination(
    source_path: &Path,
    target_path: &Path,
) -> Result<(), String> {
    let source_canonical = fs::canonicalize(source_path).map_err(|error| {
        format!(
            "failed to resolve manual mod source {}: {}",
            source_path.display(),
            error
        )
    })?;
    let target_resolved = resolve_manual_mod_path_with_missing_components(target_path)?;
    if target_resolved.starts_with(&source_canonical) {
        return Err(format!(
            "refusing to stage {} into one of its own child directories",
            source_path.display()
        ));
    }
    Ok(())
}

fn resolve_manual_mod_path_with_missing_components(path: &Path) -> Result<PathBuf, String> {
    let mut existing = path;
    let mut missing = Vec::new();
    loop {
        match fs::canonicalize(existing) {
            Ok(mut canonical) => {
                for component in missing.iter().rev() {
                    canonical.push(component);
                }
                return Ok(canonical);
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let file_name = existing.file_name().ok_or_else(|| {
                    format!("failed to resolve manual mod path {}", path.display())
                })?;
                missing.push(file_name.to_os_string());
                existing = existing.parent().ok_or_else(|| {
                    format!("failed to resolve manual mod path {}", path.display())
                })?;
            }
            Err(error) => {
                return Err(format!(
                    "failed to resolve manual mod path {}: {}",
                    path.display(),
                    error
                ));
            }
        }
    }
}

fn manual_mod_target_lock(target_path: &Path) -> Result<Arc<StdMutex<()>>, String> {
    let key = target_path
        .to_string_lossy()
        .replace('/', "\\")
        .to_lowercase();
    let locks = MANUAL_MOD_TARGET_LOCKS.get_or_init(|| StdMutex::new(HashMap::new()));
    let mut locks = locks
        .lock()
        .map_err(|_| String::from("manual mod target lock registry poisoned"))?;
    locks.retain(|_, lock| Arc::strong_count(lock) > 1);
    Ok(locks
        .entry(key)
        .or_insert_with(|| Arc::new(StdMutex::new(())))
        .clone())
}

fn create_manual_mod_transaction(target_path: &Path) -> Result<ManualModTransactionPaths, String> {
    let parent = target_path.parent().ok_or_else(|| {
        format!(
            "manual mod target has no parent directory: {}",
            target_path.display()
        )
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        format!(
            "failed to prepare manual mod transaction parent {}: {}",
            parent.display(),
            error
        )
    })?;
    validate_manual_mod_target_path(target_path)?;

    let root = parent.join(format!(
        ".langame-mod-transaction-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir(&root).map_err(|error| {
        format!(
            "failed to create manual mod transaction {}: {}",
            root.display(),
            error
        )
    })?;
    let transaction = ManualModTransactionPaths {
        staging: root.join("staging"),
        partials: root.join("partials"),
        backup: root.join("backup"),
        root,
    };
    for directory in [
        &transaction.staging,
        &transaction.partials,
        &transaction.backup,
    ] {
        if let Err(error) = fs::create_dir(directory) {
            let _ = fs::remove_dir_all(&transaction.root);
            return Err(format!(
                "failed to prepare manual mod transaction directory {}: {}",
                directory.display(),
                error
            ));
        }
    }
    Ok(transaction)
}

fn cleanup_manual_mod_transaction(transaction: &ManualModTransactionPaths) -> Result<(), String> {
    if !transaction.root.exists() {
        return Ok(());
    }
    fs::remove_dir_all(&transaction.root).map_err(|error| {
        format!(
            "failed to remove manual mod transaction {}: {}",
            transaction.root.display(),
            error
        )
    })
}

fn manual_mod_staging_failure(
    error: String,
    transaction: &ManualModTransactionPaths,
    cleanup: bool,
) -> String {
    if !cleanup {
        return format!(
            "{}; rollback data was retained at {}",
            error,
            transaction.root.display()
        );
    }
    match cleanup_manual_mod_transaction(transaction) {
        Ok(()) => error,
        Err(cleanup_error) => format!("{}; {}", error, cleanup_error),
    }
}

// Read the actual transaction payload before commit moves it away. Inventory
// includes unrelated imports, while archive source names do not identify roots.
fn staged_manual_mod_root_names(
    transaction: &ManualModTransactionPaths,
) -> Result<Vec<String>, String> {
    let entries = fs::read_dir(&transaction.staging)
        .map_err(|error| format!("failed to read staged mod roots: {error}"))?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("failed to inspect staged mod root: {error}"))?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| String::from("staged mod root name is not valid Unicode"))?;
        if !name.eq_ignore_ascii_case(ONLINE_MOD_MANIFEST) {
            names.push(name);
        }
    }
    names.sort();
    names.dedup();
    Ok(names)
}

fn commit_manual_mod_transaction(
    transaction: &ManualModTransactionPaths,
    target_path: &Path,
    replace_roots: &HashSet<PathBuf>,
) -> Result<(), ManualModCommitFailure> {
    if !target_path.exists() {
        return fs::rename(&transaction.staging, target_path).map_err(|error| {
            ManualModCommitFailure {
                message: format!(
                    "failed to commit manual mod target {}: {}",
                    target_path.display(),
                    error
                ),
                rollback_complete: true,
            }
        });
    }

    {
        let mut actions = Vec::new();
        let result = commit_manual_mod_directory_entries(
            &transaction.staging,
            target_path,
            target_path,
            Path::new(""),
            &transaction.backup,
            &mut actions,
            replace_roots,
        );
        match result {
            Ok(()) => Ok(()),
            Err(error) => {
                let rollback_errors = rollback_manual_mod_commit(target_path, &actions);
                Err(ManualModCommitFailure {
                    message: if rollback_errors.is_empty() {
                        format!("manual mod commit failed and was rolled back: {}", error)
                    } else {
                        format!(
                            "manual mod commit failed: {}; rollback errors: {}",
                            error,
                            rollback_errors.join("; ")
                        )
                    },
                    rollback_complete: rollback_errors.is_empty(),
                })
            }
        }
    }
}

fn commit_manual_mod_directory_entries(
    staging_dir: &Path,
    target_dir: &Path,
    target_root: &Path,
    relative_dir: &Path,
    backup_root: &Path,
    actions: &mut Vec<ManualModCommitAction>,
    replace_roots: &HashSet<PathBuf>,
) -> Result<(), String> {
    let mut entries = fs::read_dir(staging_dir)
        .map_err(|error| {
            format!(
                "failed to read staging {}: {}",
                staging_dir.display(),
                error
            )
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            format!(
                "failed to inspect staging {}: {}",
                staging_dir.display(),
                error
            )
        })?;
    entries.sort_by_key(|entry| entry.file_name().to_string_lossy().to_lowercase());
    for entry in entries {
        let file_name = entry.file_name();
        let source_path = entry.path();
        let destination_path = target_dir.join(&file_name);
        let relative_path = relative_dir.join(&file_name);
        commit_manual_mod_entry(
            &source_path,
            &destination_path,
            target_root,
            &relative_path,
            backup_root,
            actions,
            replace_roots,
        )?;
    }
    Ok(())
}

fn commit_manual_mod_entry(
    source_path: &Path,
    destination_path: &Path,
    target_root: &Path,
    relative_path: &Path,
    backup_root: &Path,
    actions: &mut Vec<ManualModCommitAction>,
    replace_roots: &HashSet<PathBuf>,
) -> Result<(), String> {
    let source_metadata = fs::symlink_metadata(source_path).map_err(|error| {
        format!(
            "failed to inspect staged mod entry {}: {}",
            source_path.display(),
            error
        )
    })?;
    if manual_mod_metadata_is_reparse(&source_metadata) {
        return Err(format!(
            "staged mod entry became a symbolic link or reparse point: {}",
            source_path.display()
        ));
    }
    validate_manual_mod_target_operation_parent(target_root, destination_path)?;
    let destination_metadata = match fs::symlink_metadata(destination_path) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => {
            return Err(format!(
                "failed to inspect manual mod destination {}: {}",
                destination_path.display(),
                error
            ));
        }
    };
    if destination_metadata
        .as_ref()
        .is_some_and(manual_mod_metadata_is_reparse)
    {
        return Err(format!(
            "manual mod destination is a symbolic link or reparse point: {}",
            destination_path.display()
        ));
    }

    if source_metadata.is_dir()
        && !replace_roots.contains(relative_path)
        && destination_metadata
            .as_ref()
            .is_some_and(fs::Metadata::is_dir)
    {
        commit_manual_mod_directory_entries(
            source_path,
            destination_path,
            target_root,
            relative_path,
            backup_root,
            actions,
            replace_roots,
        )?;
        fs::remove_dir(source_path).map_err(|error| {
            format!(
                "failed to finalize staged directory {}: {}",
                source_path.display(),
                error
            )
        })?;
        return Ok(());
    }

    let backup_path = destination_metadata
        .as_ref()
        .map(|_| backup_root.join(relative_path));
    if let Some(backup_path) = &backup_path {
        if let Some(parent) = backup_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "failed to prepare manual mod rollback directory {}: {}",
                    parent.display(),
                    error
                )
            })?;
        }
        validate_manual_mod_target_operation_parent(target_root, destination_path)?;
        validate_manual_mod_destination_for_backup(destination_path)?;
        fs::rename(destination_path, backup_path).map_err(|error| {
            format!(
                "failed to preserve existing mod path {}: {}",
                destination_path.display(),
                error
            )
        })?;
        actions.push(ManualModCommitAction {
            destination: destination_path.to_path_buf(),
            backup: Some(backup_path.clone()),
            installed: false,
        });
    }

    validate_manual_mod_target_operation_parent(target_root, destination_path)?;
    validate_manual_mod_destination_absent(destination_path)?;
    if let Err(error) = fs::rename(source_path, destination_path) {
        return Err(format!(
            "failed to install staged mod path {}: {}",
            destination_path.display(),
            error
        ));
    }
    if let Some(action) = actions.last_mut()
        && action.destination == destination_path
        && !action.installed
    {
        action.installed = true;
    } else {
        actions.push(ManualModCommitAction {
            destination: destination_path.to_path_buf(),
            backup: None,
            installed: true,
        });
    }
    Ok(())
}

fn validate_manual_mod_target_operation_parent(
    target_root: &Path,
    destination_path: &Path,
) -> Result<(), String> {
    let parent = destination_path.parent().ok_or_else(|| {
        format!(
            "manual mod destination has no parent directory: {}",
            destination_path.display()
        )
    })?;
    parent.strip_prefix(target_root).map_err(|_| {
        format!(
            "manual mod destination {} is outside target {}",
            destination_path.display(),
            target_root.display()
        )
    })?;
    validate_manual_mod_existing_path_components(target_root, true)?;
    validate_manual_mod_existing_path_components(parent, true)?;
    let parent_metadata = fs::symlink_metadata(parent).map_err(|error| {
        format!(
            "failed to inspect manual mod destination parent {}: {}",
            parent.display(),
            error
        )
    })?;
    if manual_mod_metadata_is_reparse(&parent_metadata) || !parent_metadata.is_dir() {
        return Err(format!(
            "manual mod destination parent is not a safe directory: {}",
            parent.display()
        ));
    }
    Ok(())
}

fn validate_manual_mod_destination_for_backup(destination_path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(destination_path).map_err(|error| {
        format!(
            "failed to revalidate manual mod destination {} before backup: {}",
            destination_path.display(),
            error
        )
    })?;
    if manual_mod_metadata_is_reparse(&metadata) {
        return Err(format!(
            "manual mod destination became a symbolic link or reparse point: {}",
            destination_path.display()
        ));
    }
    Ok(())
}

fn validate_manual_mod_destination_absent(destination_path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(destination_path) {
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "failed to revalidate manual mod destination {} before install: {}",
            destination_path.display(),
            error
        )),
        Ok(_) => Err(format!(
            "manual mod destination changed before install: {}",
            destination_path.display()
        )),
    }
}

fn validate_manual_mod_backup_for_restore(backup_path: &Path) -> Result<(), String> {
    validate_manual_mod_existing_path_components(backup_path, false)?;
    let metadata = fs::symlink_metadata(backup_path).map_err(|error| {
        format!(
            "failed to inspect manual mod rollback backup {}: {}",
            backup_path.display(),
            error
        )
    })?;
    if manual_mod_metadata_is_reparse(&metadata) {
        return Err(format!(
            "manual mod rollback backup is a symbolic link or reparse point: {}",
            backup_path.display()
        ));
    }
    if metadata.is_dir() {
        validate_manual_mod_target_tree(backup_path)?;
    } else if !metadata.is_file() {
        return Err(format!(
            "manual mod rollback backup is not a regular file or directory: {}",
            backup_path.display()
        ));
    }
    Ok(())
}

fn rollback_manual_mod_commit(
    target_root: &Path,
    actions: &[ManualModCommitAction],
) -> Vec<String> {
    let mut errors = Vec::new();
    for action in actions.iter().rev() {
        if action.installed
            && let Err(error) = remove_manual_mod_installed_path(target_root, &action.destination)
        {
            errors.push(error);
            continue;
        }
        if let Some(backup_path) = &action.backup {
            if let Err(error) =
                validate_manual_mod_target_operation_parent(target_root, &action.destination)
            {
                errors.push(error);
                continue;
            }
            match fs::symlink_metadata(&action.destination) {
                Ok(_) => {
                    errors.push(format!(
                        "rollback destination still exists: {}",
                        action.destination.display()
                    ));
                    continue;
                }
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => {
                    errors.push(format!(
                        "failed to inspect rollback destination {}: {}",
                        action.destination.display(),
                        error
                    ));
                    continue;
                }
            }
            if let Err(error) = validate_manual_mod_backup_for_restore(backup_path) {
                errors.push(error);
                continue;
            }
            if let Err(error) =
                validate_manual_mod_target_operation_parent(target_root, &action.destination)
            {
                errors.push(error);
                continue;
            }
            if let Err(error) = fs::rename(backup_path, &action.destination) {
                errors.push(format!(
                    "failed to restore {}: {}",
                    action.destination.display(),
                    error
                ));
            }
        }
    }
    errors
}

fn remove_manual_mod_installed_path(target_root: &Path, path: &Path) -> Result<(), String> {
    validate_manual_mod_target_operation_parent(target_root, path)?;
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "failed to inspect rollback path {}: {}",
                path.display(),
                error
            ));
        }
    };
    if manual_mod_metadata_is_reparse(&metadata) {
        return Err(format!(
            "refusing to remove reparse point during rollback: {}",
            path.display()
        ));
    }
    if metadata.is_dir() {
        validate_manual_mod_target_tree(path)?;
        fs::remove_dir_all(path)
            .map_err(|error| format!("failed to remove {}: {}", path.display(), error))
    } else {
        fs::remove_file(path)
            .map_err(|error| format!("failed to remove {}: {}", path.display(), error))
    }
}

#[cfg(test)]
#[path = "commands_mod_affected_roots_tests.rs"]
mod affected_roots_tests;

#[cfg(test)]
mod staging_tests {
    use super::*;

    #[test]
    fn manual_mod_archive_cannot_replace_online_package_ownership() {
        let root = staging_test_root("reserved-manifest");
        let archive = root.join("package.zip");
        write_staging_test_zip(
            &archive,
            &[
                (".LANGAME-ONLINE-MODS.JSON", b"forged"),
                ("plugin.jar", b"payload"),
            ],
        );
        let target_path = root.join("mods");
        fs::create_dir_all(&target_path).unwrap();
        fs::write(target_path.join(ONLINE_MOD_MANIFEST), "original").unwrap();
        let target = ResolvedManualModTarget {
            instance_id: "server".into(),
            module_id: "minecraft".into(),
            source_label: "local".into(),
            target_label: "mods".into(),
            target_path: target_path.clone(),
            accepts: vec!["zip".into()],
            id_strategy: None,
        };
        let result = stage_manual_mod_sources(target, vec![archive]);
        let record = fs::read_to_string(target_path.join(ONLINE_MOD_MANIFEST)).unwrap();
        let unexpected_file = target_path.join("plugin.jar").exists();
        fs::remove_dir_all(root).unwrap();
        assert!(result.unwrap_err().contains("reserved ownership record"));
        assert_eq!(record, "original");
        assert!(!unexpected_file);
    }

    #[test]
    fn manual_mod_unicode_names_are_valid_windows_components() {
        for name in ["a中.zip", "😀.jar", "插件.dll"] {
            validate_manual_mod_windows_component(std::ffi::OsStr::new(name))
                .expect("ordinary Unicode filenames must not panic or be rejected");
        }
    }

    #[test]
    fn workshop_update_replaces_only_the_requested_item_tree() {
        let root = staging_test_root("workshop-replace");
        let source = root.join("cache").join("123456");
        let target_path = root.join("mods");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(target_path.join("123456")).unwrap();
        fs::write(source.join("current.dll"), "new").unwrap();
        fs::write(target_path.join("123456").join("removed.dll"), "old").unwrap();
        fs::write(target_path.join("operator.cfg"), "keep").unwrap();
        let target = ResolvedManualModTarget {
            instance_id: "instance".into(),
            module_id: "squad".into(),
            source_label: "Workshop".into(),
            target_label: "Mods".into(),
            target_path: target_path.clone(),
            accepts: vec!["folder".into()],
            id_strategy: None,
        };
        let downloaded = SteamWorkshopDownloadResult {
            consumer_app_id: 393380,
            install_root: root.to_string_lossy().into_owned(),
            workshop_root: root.join("cache").to_string_lossy().into_owned(),
            items: vec![app_steamcmd::SteamWorkshopDownloadItemResult {
                item_id: "123456".into(),
                expected_path: source.to_string_lossy().into_owned(),
                expected_path_exists: true,
            }],
            output_excerpt: String::new(),
        };
        stage_downloaded_workshop_items_into_manual_target(&downloaded, &target).unwrap();
        assert!(!target_path.join("123456").join("removed.dll").exists());
        assert_eq!(
            fs::read_to_string(target_path.join("123456").join("current.dll")).unwrap(),
            "new"
        );
        assert_eq!(
            fs::read_to_string(target_path.join("operator.cfg")).unwrap(),
            "keep"
        );
        assert_no_staging_transaction(&root);
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn aborted_manual_staging_caller_keeps_storage_lease_until_writer_finishes() {
        let root = staging_test_root("abort-lease");
        let source_path = root.join("source.jar");
        let target_path = root.join("target");
        fs::write(&source_path, "mod payload").unwrap();
        let target = ResolvedManualModTarget {
            instance_id: String::from("abortable-instance"),
            module_id: String::from("minecraft"),
            source_label: String::from("local files"),
            target_label: String::from("mods"),
            target_path: target_path.clone(),
            accepts: vec![String::from("jar")],
            id_strategy: None,
        };
        let state = Arc::new(DesktopState::default());
        let worker_state = Arc::clone(&state);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let (completed_tx, completed_rx) = tokio::sync::oneshot::channel();
        let caller = tokio::spawn(async move {
            let operation = worker_state
                .begin_storage_context_operation("manual staging abort test")
                .expect("storage operation");
            spawn_blocking_storage_context_task(&operation, move || {
                started_tx.send(()).expect("started signal");
                release_rx.recv().expect("release signal");
                let result = stage_manual_mod_sources(target, vec![source_path]);
                completed_tx.send(()).expect("completed signal");
                result
            })
            .await
        });

        started_rx.await.expect("started signal");
        caller.abort();
        let _ = caller.await;
        assert!(state.begin_storage_context_transition().is_err());

        release_tx.send(()).expect("release writer");
        completed_rx.await.expect("completed signal");
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match state.begin_storage_context_transition() {
                Ok(transition) => {
                    drop(transition);
                    break;
                }
                Err(_) if Instant::now() < deadline => tokio::task::yield_now().await,
                Err(error) => {
                    panic!("blocking writer completion should release the storage lease: {error}")
                }
            }
        }
        assert_eq!(
            fs::read_to_string(target_path.join("source.jar")).unwrap(),
            "mod payload"
        );
        let _ = fs::remove_dir_all(root);
    }

    pub(super) fn staging_test_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "langame-mod-staging-{}-{}",
            label,
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    pub(super) fn write_staging_test_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, contents) in entries {
            archive.start_file(*name, options).unwrap();
            archive.write_all(contents).unwrap();
        }
        archive.finish().unwrap();
    }

    pub(super) fn assert_no_staging_transaction(root: &Path) {
        assert!(
            !fs::read_dir(root)
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| {
                    entry
                        .file_name()
                        .to_str()
                        .is_some_and(|name| name.starts_with(".langame-mod-transaction-"))
                })
        );
    }

    #[test]
    fn manual_mod_target_lock_is_shared_by_normalized_target() {
        let first = manual_mod_target_lock(Path::new("D:/LanGame/Mods")).unwrap();
        let second = manual_mod_target_lock(Path::new("d:\\langame\\mods")).unwrap();

        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn manual_mod_archive_directory_limits_are_checked_before_parser() {
        let archive_path = Path::new("oversized.zip");
        let entry_error = validate_manual_mod_archive_directory_metrics(
            archive_path,
            MANUAL_MOD_ARCHIVE_MAX_ENTRY_COUNT as u64 + 1,
            (MANUAL_MOD_ARCHIVE_MAX_ENTRY_COUNT as u64 + 1) * 46,
        )
        .unwrap_err();
        let directory_error = validate_manual_mod_archive_directory_metrics(
            archive_path,
            1,
            MANUAL_MOD_ARCHIVE_MAX_CENTRAL_DIRECTORY_BYTES + 1,
        )
        .unwrap_err();

        assert!(entry_error.contains("too many directory entries"));
        assert!(directory_error.contains("oversized central directory"));
    }

    #[test]
    fn thunderstore_archive_uses_secure_payload_planning_and_transaction_publish() {
        let root = staging_test_root("thunderstore-payload");
        let archive_path = root.join("package.zip");
        let target = root.join("payload");
        write_staging_test_zip(
            &archive_path,
            &[
                ("manifest.json", br#"{"name":"Example"}"#),
                ("BepInEx/plugins/Example.dll", b"plugin"),
            ],
        );

        stage_thunderstore_archive_to_directory(archive_path, target.clone()).unwrap();

        assert_eq!(fs::read(target.join("Example.dll")).unwrap(), b"plugin");
        assert!(!target.join("manifest.json").exists());
        assert!(!target.join("BepInEx").exists());
        assert_no_staging_transaction(&root);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn thunderstore_archive_rejects_windows_aliases_without_partial_publish() {
        let root = staging_test_root("thunderstore-alias");
        let archive_path = root.join("package.zip");
        let target = root.join("payload");
        write_staging_test_zip(
            &archive_path,
            &[
                ("plugins/Example.dll", b"first"),
                ("plugins/example.dll", b"second"),
            ],
        );

        let error = stage_thunderstore_archive_to_directory(archive_path, target.clone())
            .expect_err("Windows aliases must fail before publication");

        assert!(error.contains("duplicate or Windows-aliased path"));
        assert!(!target.exists());
        assert_no_staging_transaction(&root);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn thunderstore_archive_rejects_zip_bombs_without_partial_publish() {
        let root = staging_test_root("thunderstore-ratio");
        let archive_path = root.join("package.zip");
        let target = root.join("payload");
        let compressed = vec![0u8; 1024 * 1024];
        write_staging_test_zip(&archive_path, &[("plugins/compressed.dll", &compressed)]);

        let error = stage_thunderstore_archive_to_directory(archive_path, target.clone())
            .expect_err("high-ratio archives must fail before publication");

        assert!(error.contains("compression ratio"));
        assert!(!target.exists());
        assert_no_staging_transaction(&root);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn thunderstore_archive_rejects_symbolic_links_without_partial_publish() {
        let root = staging_test_root("thunderstore-link");
        let archive_path = root.join("package.zip");
        let target = root.join("payload");
        let file = fs::File::create(&archive_path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        archive
            .add_symlink(
                "plugins/linked.dll",
                "../../outside.dll",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive.finish().unwrap();

        let error = stage_thunderstore_archive_to_directory(archive_path, target.clone())
            .expect_err("archive links must fail before publication");

        assert!(error.contains("symbolic link"));
        assert!(!target.exists());
        assert_no_staging_transaction(&root);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn manual_mod_commit_rollback_restores_replaced_file() {
        let root = staging_test_root("rollback");
        let destination = root.join("target").join("mod.jar");
        let backup = root.join("transaction").join("backup").join("mod.jar");
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::create_dir_all(backup.parent().unwrap()).unwrap();
        fs::write(&destination, "new").unwrap();
        fs::write(&backup, "old").unwrap();
        let actions = vec![ManualModCommitAction {
            destination: destination.clone(),
            backup: Some(backup.clone()),
            installed: true,
        }];

        let errors = rollback_manual_mod_commit(&root.join("target"), &actions);

        assert!(errors.is_empty());
        assert_eq!(fs::read_to_string(&destination).unwrap(), "old");
        assert!(!backup.exists());
        let _ = fs::remove_dir_all(&root);
    }
}
