//! Read-only inspection of the exact depots recorded by an existing installation.
//! Missing depot metadata leaves other paths unclassified, never safe to delete.
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use app_core::AppSettings;
use app_modules::ModuleDescriptor;
use serde::Serialize;
use serde_json::{Value, json};

use super::Inspection;
use super::steam_seed::{
    DepotCandidate, Entry, Inventory, Progress, acf_app_id, bounded, check_plain, checked_root,
    folded, hash_reader, invalid, inventory_for_inspection, open_read,
};

const MAX_ENTRIES: usize = 1_000_000;
const BASELINE_METADATA: [&str; 2] = [
    ".langame-initial-package.json",
    ".langame-clean-package.json",
];

#[derive(Default, Serialize)]
struct Summary {
    manifest_complete: bool,
    expected_files: usize,
    expected_bytes: Option<u64>,
    overlapping_files: usize,
    matched_bytes: u64,
    matched_files: usize,
    missing_files: usize,
    missing_directories: usize,
    mismatched_files: usize,
    unreadable_files: usize,
    extras: usize,
    unclassified: usize,
    linked_paths: usize,
    payload_verified: bool,
    clean_tree: bool,
}

#[derive(Serialize)]
struct Observation {
    status: &'static str,
    actual_size: Option<u64>,
    actual_sha1: Option<String>,
    error: Option<String>,
}

pub(super) fn run(
    settings: &AppSettings,
    descriptor: &ModuleDescriptor,
    mode: Inspection,
    mut emit: impl FnMut(Value),
) -> io::Result<()> {
    let app_id = descriptor
        .summary
        .steam_app_id
        .filter(|id| *id != 0)
        .ok_or_else(|| invalid("inventory/verify requires a Steam module"))?;
    let install = descriptor
        .install
        .as_ref()
        .ok_or_else(|| invalid("module installation specification is missing"))?;
    if install.download_url_windows.is_some() {
        return Err(invalid(
            "inventory/verify does not inspect direct-download packages",
        ));
    }
    let games = checked_root(Path::new(&settings.games_root))?;
    let candidate = games.join(&install.shared_game_dir);
    let root = checked_root(&candidate)?;
    if root == games || !root.starts_with(&games) {
        return Err(invalid(
            "inspection target must be inside the selected game library",
        ));
    }
    let module_id = &descriptor.summary.id;
    emit(json!({"event":"inspection_started", "module_id":module_id,
        "root":root, "hash_payload":mode == Inspection::Verify}));
    let expected =
        inventory_for_inspection(&root, Path::new(&settings.steamcmd_root), app_id, false)?;
    inspect(&root, app_id, &expected, mode, false, |mut event| {
        event["module_id"] = json!(module_id);
        event["root"] = json!(root);
        emit(event);
    })
}

/// Retry may replace only these two manager inventory files. Their contents
/// never provide evidence: every official byte is verified against exact depots.
pub(super) fn verify_for_certification(
    root: &Path,
    steamcmd: &Path,
    app_id: u32,
    emit: impl FnMut(Value),
) -> io::Result<()> {
    // Only the certify caller, after its own successful official validation,
    // may use this observation as part of establishing a clean baseline.
    let expected = inventory_for_inspection(root, steamcmd, app_id, true)?;
    inspect(root, app_id, &expected, Inspection::Verify, true, emit)
}

/// Inspect unknown directories as well before permitting the native installer.
/// This metadata-only check follows no reparse points and changes no files.
pub(super) fn ensure_plain_tree(root: &Path) -> io::Result<()> {
    checked_root(root)?;
    let mut summary = Summary::default();
    inspect_tree(
        root,
        &Inventory::default(),
        None,
        false,
        &mut summary,
        &mut |_| {},
    )?;
    if summary.linked_paths != 0 {
        return Err(invalid(
            "installation contains reparse points; certification will not run",
        ));
    }
    Ok(())
}

fn inspect(
    root: &Path,
    app_id: u32,
    expected: &Inventory,
    mode: Inspection,
    allow_baseline_metadata: bool,
    mut emit: impl FnMut(Value),
) -> io::Result<()> {
    let mut summary = Summary {
        manifest_complete: expected.missing_manifests.is_empty() && !expected.files.is_empty(),
        expected_files: expected.files.len(),
        expected_bytes: expected
            .overlapping_files
            .is_empty()
            .then(|| expected.files.values().map(|entry| entry.size).sum()),
        overlapping_files: expected.overlapping_files.len(),
        ..Default::default()
    };
    emit(
        json!({"event":"inspection_inventory", "expected_files":summary.expected_files,
        "expected_bytes":summary.expected_bytes, "overlapping_files":summary.overlapping_files,
        "manifest_complete":summary.manifest_complete,
        "missing_manifests":expected.missing_manifests}),
    );
    let mut progress = Progress::new("steam_existing_verify", app_id);
    for (relative, entry) in &expected.files {
        let candidates = expected
            .overlapping_files
            .get(relative)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let observation =
            match inspect_file(&root.join(relative), entry, candidates, mode, &mut progress) {
                Ok(value) => value,
                Err(error) => Observation {
                    status: "unreadable_or_unsafe",
                    actual_size: None,
                    actual_sha1: None,
                    error: Some(error.to_string()),
                },
            };
        match observation.status {
            "matched" => {
                summary.matched_files += 1;
                summary.matched_bytes += observation.actual_size.unwrap_or(0);
            }
            "missing" => summary.missing_files += 1,
            "size_mismatch" | "hash_mismatch" => summary.mismatched_files += 1,
            "unreadable_or_unsafe" => summary.unreadable_files += 1,
            _ => (),
        }
        let matched_candidates: Vec<_> = candidates
            .iter()
            .filter(|candidate| {
                observation.status == "matched"
                    && observation.actual_size == Some(candidate.entry.size)
                    && observation.actual_sha1.as_deref() == Some(candidate.entry.sha1.as_str())
            })
            .collect();
        let identified = if candidates.is_empty() {
            Some(entry)
        } else {
            matched_candidates.first().map(|candidate| &candidate.entry)
        };
        emit(json!({"event":"inspection_file", "relative_path":relative,
            "expected_size":identified.map(|entry| entry.size),
            "expected_sha1":identified.map(|entry| &entry.sha1),
            "official_candidates":candidates, "matched_depots":matched_candidates,
            "actual":observation}));
    }
    for relative in &expected.directories {
        let path = root.join(relative);
        let status = match check_plain(&path, true) {
            Ok(()) => "present",
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                summary.missing_directories += 1;
                "missing"
            }
            Err(_) => {
                summary.unreadable_files += 1;
                "unreadable_or_unsafe"
            }
        };
        emit(json!({"event":"inspection_directory", "relative_path":relative, "status":status}));
    }
    if mode == Inspection::Verify {
        progress.emit()?;
    }
    // A concurrent installer changing its control files invalidates this read.
    // Do not emit authoritative extras against a stale manifest selection.
    for (relative, bytes) in &expected.acfs {
        if bounded(&root.join(relative), 4 * 1024 * 1024)? != bytes.as_bytes() {
            return Err(invalid(
                "Steam ACF changed during inspection; repeat the read",
            ));
        }
    }
    inspect_tree(
        root,
        expected,
        Some(app_id),
        allow_baseline_metadata,
        &mut summary,
        &mut emit,
    )?;
    summary.payload_verified = mode == Inspection::Verify
        && summary.manifest_complete
        && summary.missing_directories == 0
        && summary.unreadable_files == 0
        && summary.matched_files == summary.expected_files;
    if summary.payload_verified {
        summary.expected_bytes = Some(summary.matched_bytes);
    }
    summary.clean_tree =
        summary.payload_verified && summary.extras == 0 && summary.linked_paths == 0;
    emit(json!({"event":"inspection_summary", "summary":summary}));
    if !summary.manifest_complete
        || summary.missing_files != 0
        || summary.missing_directories != 0
        || summary.mismatched_files != 0
        || summary.unreadable_files != 0
        || summary.extras != 0
        || summary.linked_paths != 0
    {
        return Err(invalid(
            "installation inspection is incomplete or has discrepancies; no files were changed",
        ));
    }
    Ok(())
}

fn inspect_file(
    path: &Path,
    expected: &Entry,
    candidates: &[DepotCandidate],
    mode: Inspection,
    progress: &mut Progress,
) -> io::Result<Observation> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(Observation {
                status: "missing",
                actual_size: None,
                actual_sha1: None,
                error: None,
            });
        }
        Err(error) => return Err(error),
        Ok(_) => (),
    }
    let input = open_read(path)?;
    let size = input.metadata()?.len();
    let mut observation = Observation {
        status: "size_matches_not_hashed",
        actual_size: Some(size),
        actual_sha1: None,
        error: None,
    };
    let matches_size = if candidates.is_empty() {
        size == expected.size
    } else {
        candidates
            .iter()
            .any(|candidate| candidate.entry.size == size)
    };
    if !matches_size {
        observation.status = "size_mismatch";
    } else if mode == Inspection::Verify {
        let (actual_size, hash) = hash_reader(input, None, size, progress)?;
        let matches_hash = if candidates.is_empty() {
            actual_size == expected.size && hash == expected.sha1
        } else {
            candidates.iter().any(|candidate| {
                actual_size == candidate.entry.size && hash == candidate.entry.sha1
            })
        };
        observation.status = if matches_hash {
            "matched"
        } else {
            "hash_mismatch"
        };
        observation.actual_size = Some(actual_size);
        observation.actual_sha1 = Some(hash);
    }
    Ok(observation)
}

fn linked(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn inspect_tree(
    root: &Path,
    expected: &Inventory,
    steam_app_id: Option<u32>,
    allow_baseline_metadata: bool,
    summary: &mut Summary,
    emit: &mut impl FnMut(Value),
) -> io::Result<()> {
    let allowed: BTreeSet<_> = expected
        .files
        .keys()
        .chain(expected.acfs.keys())
        .map(|path| folded(path))
        .collect();
    let mut allowed_dirs: BTreeSet<_> = expected
        .directories
        .iter()
        .map(|path| folded(path))
        .collect();
    for relative in expected
        .files
        .keys()
        .chain(expected.acfs.keys())
        .chain(expected.directories.iter())
    {
        for parent in Path::new(relative).ancestors().skip(1) {
            if !parent.as_os_str().is_empty() {
                allowed_dirs.insert(folded(&parent.to_string_lossy().replace('\\', "/")));
            }
        }
    }
    // Successful native Steam validation can retain empty control directories
    // for the app and its referenced shared-depot owners. Only the ACFs selected
    // by the official inventory add owners; arbitrary on-disk ACFs add nothing.
    // Traversal below still rejects every unknown child in these directories.
    let mut control_app_ids: BTreeSet<_> = steam_app_id.into_iter().collect();
    for relative in expected.acfs.keys() {
        control_app_ids.insert(acf_app_id(relative)?);
    }
    for app_id in control_app_ids {
        for directory in ["steamapps/temp", "steamapps/downloading"] {
            allowed_dirs.insert(folded(directory));
            allowed_dirs.insert(folded(&format!("{directory}/{app_id}")));
        }
    }
    let mut pending = vec![PathBuf::new()];
    let mut entries = 0;
    while let Some(relative) = pending.pop() {
        let directory = root.join(&relative);
        check_plain(&directory, true)?;
        for entry in fs::read_dir(directory)? {
            entries += 1;
            if entries > MAX_ENTRIES {
                return Err(invalid("inspection exceeds entry limit"));
            }
            let entry = entry?;
            let relative = relative.join(entry.file_name());
            if relative.components().count() > 64 {
                return Err(invalid("inspection exceeds directory depth limit"));
            }
            let name = relative
                .to_str()
                .ok_or_else(|| invalid("inspection path is not UTF-8"))?
                .replace('\\', "/");
            let metadata = fs::symlink_metadata(entry.path())?;
            let is_link = linked(&metadata);
            let directory = metadata.is_dir() && !is_link;
            if !is_link && !directory {
                check_plain(&entry.path(), false)?;
            }
            if allow_baseline_metadata && BASELINE_METADATA.contains(&name.as_str()) {
                check_plain(&entry.path(), false)?;
                emit(
                    json!({"event":"inspection_manager_metadata", "relative_path":name,
                    "used_as_evidence":false}),
                );
                continue;
            }
            let known = if directory {
                allowed_dirs.contains(&folded(&name))
            } else {
                allowed.contains(&folded(&name))
            };
            if is_link {
                summary.linked_paths += 1;
            }
            if !known || is_link {
                let classification = if !summary.manifest_complete {
                    "unclassified"
                } else if is_link {
                    "unsafe_link"
                } else {
                    "extra"
                };
                if summary.manifest_complete {
                    summary.extras += 1;
                } else {
                    summary.unclassified += 1;
                }
                emit(json!({"event":"inspection_path", "relative_path":name,
                    "classification":classification, "kind":if is_link { "link" }
                    else if directory { "directory" } else { "file" },
                    "bytes":if directory { None } else { Some(metadata.len()) }}));
            }
            if directory {
                pending.push(relative);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "steam_inspect_tests.rs"]
mod tests;
