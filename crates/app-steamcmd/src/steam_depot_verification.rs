use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use app_core::{InstallVerificationProgress, InstallVerificationStage};
use sha1::{Digest, Sha1};
use sha2::Sha256;

use super::manifest::{decimal, parse_acf, text};
use super::{Entry, MAX_ACF, check_plain, checked_root, invalid, open_read, read_inventory};

/// An exact official file allowlist, including the app's selected ACFs.
/// Unrecognized files are preserved on disk and never enter this inventory.
#[derive(Debug)]
pub struct VerifiedSteamPackage {
    pub files: BTreeMap<String, String>,
    pub directories: BTreeSet<String>,
}

#[derive(Default)]
pub struct SteamPackageVerificationOptions<'a> {
    pub cancellation: Option<&'a AtomicBool>,
    /// Select depot paths using the consumer's program-package ownership rules.
    /// Excluded files remain untouched and never enter the verified inventory.
    /// App manifests and their build/depot metadata are always verified.
    pub include_path: Option<&'a dyn Fn(&str) -> bool>,
    /// Selected defaults are admitted only when their present bytes match an
    /// official depot entry. Missing or modified defaults remain untouched.
    /// Optional directories are recorded only as parents of admitted files.
    pub optional_path: Option<&'a dyn Fn(&str) -> bool>,
    /// Allow missing cached depot manifests only for incremental evidence of
    /// original defaults. The result then proves only files in available declared
    /// depots, never a complete installation. Normal acquisition keeps this false.
    /// Corrupt manifests, ACF/build validation and other I/O errors remain strict.
    pub allow_missing_depots: bool,
}

/// Call after successful native installation/validation, while retaining the
/// installation lifecycle lease through baseline publication and persistence.
/// Cached depot CRC/SHA1 checks are integrity checks, not signature verification.
/// Consumers must recheck these SHA256 values, or capture reliable file identity
/// from the still-protected verified handle and recheck it before publication.
pub fn verify_installed_steam_package(
    root: &Path,
    steamcmd: &Path,
    app_id: u32,
    expected_version: &str,
    cancellation: Option<&AtomicBool>,
) -> io::Result<VerifiedSteamPackage> {
    verify_installed_steam_package_with_progress(
        root,
        steamcmd,
        app_id,
        expected_version,
        SteamPackageVerificationOptions {
            cancellation,
            ..Default::default()
        },
        |_| {},
        |_, _, _| Ok(()),
    )
}

/// Reports local work and exposes each verified file while its read handle still
/// excludes writers and replacements. Evidence must be captured inside the callback.
pub fn verify_installed_steam_package_with_progress(
    root: &Path,
    steamcmd: &Path,
    app_id: u32,
    expected_version: &str,
    options: SteamPackageVerificationOptions<'_>,
    mut on_progress: impl FnMut(InstallVerificationProgress),
    mut on_verified_file: impl FnMut(&str, &File, &str) -> io::Result<()>,
) -> io::Result<VerifiedSteamPackage> {
    let cancellation = options.cancellation;
    cancelled(cancellation)?;
    let canonical_root = checked_root(root)?;
    let root = canonical_root.as_path();
    let expected_build = decimal(expected_version)?;
    if app_id == 0 || expected_build == 0 {
        return Err(invalid(
            "Steam certification requires an app ID and build ID",
        ));
    }
    let mut expected = read_inventory(
        root,
        steamcmd,
        app_id,
        !options.allow_missing_depots,
        true,
        cancellation,
    )?;
    if let Some(include) = options.include_path {
        expected.files.retain(|path, _| include(path));
        expected.directories.retain(|path| include(path));
    }
    if let Some(optional) = options.optional_path {
        expected.directories.retain(|path| !optional(path));
    }
    let main = format!("steamapps/appmanifest_{app_id}.acf");
    let app = parse_acf(
        expected
            .acfs
            .get(&main)
            .ok_or_else(|| invalid("app ACF missing"))?
            .as_bytes(),
        app_id,
    )?;
    let version = text(
        app.get("buildid")
            .ok_or_else(|| invalid("Steam build ID missing"))?,
    )?;
    if decimal(version)? != expected_build {
        return Err(invalid(
            "installed Steam build differs from completed installation",
        ));
    }
    if let Some(target) = app.get("targetbuildid") {
        let target = decimal(text(target)?)?;
        if target != 0 && target != expected_build {
            return Err(invalid("Steam target build differs from installed build"));
        }
    }
    let mut verified = VerifiedSteamPackage {
        files: BTreeMap::new(),
        directories: expected.directories.clone(),
    };
    // Overlapping depots can declare different official sizes for one path.
    // Measure the selected installed files rather than inventing a transfer size.
    let mut sizes = BTreeMap::new();
    let mut total_bytes = 0_u64;
    for (relative, entry) in &expected.files {
        cancelled(cancellation)?;
        let path = root.join(relative);
        let required = options
            .optional_path
            .is_none_or(|optional| !optional(relative));
        let size = if required {
            check_plain(&path, false)?;
            std::fs::metadata(path)?.len()
        } else {
            let Some(size) = optional_file_size(&path)? else {
                continue;
            };
            let matches = expected.overlapping_files.get(relative).map_or_else(
                || entry.size == size,
                |items| items.iter().any(|item| item.entry.size == size),
            );
            if !matches {
                continue;
            }
            size
        };
        total_bytes = total_bytes
            .checked_add(size)
            .ok_or_else(|| invalid("Steam package size overflow"))?;
        sizes.insert(relative.clone(), (size, required));
    }
    for original in expected.acfs.values() {
        total_bytes = total_bytes
            .checked_add(original.len() as u64)
            .ok_or_else(|| invalid("Steam package size overflow"))?;
    }
    let mut reporter = VerificationReporter::new(
        total_bytes,
        (sizes.len() + expected.acfs.len()) as u64,
        &mut on_progress,
    );
    for (relative, entry) in &expected.files {
        cancelled(cancellation)?;
        let Some(&(size, required)) = sizes.get(relative) else {
            continue;
        };
        let alternatives = expected.overlapping_files.get(relative);
        let candidates = alternatives
            .map(|items| items.iter().map(|item| &item.entry).collect::<Vec<_>>())
            .unwrap_or_else(|| vec![entry]);
        let mut file = open_read(&root.join(relative))?;
        if file.metadata()?.len() != size {
            return Err(invalid("Steam file size changed before verification"));
        }
        let digest = verify_file(
            &mut file,
            &root.join(relative),
            &candidates,
            required,
            cancellation,
            &mut reporter,
        )
        .map_err(|error| io::Error::new(error.kind(), format!("{relative}: {error}")))?;
        if let Some(digest) = digest {
            on_verified_file(relative, &file, &digest)?;
            verified.files.insert(relative.clone(), digest);
            add_parents(&mut verified.directories, relative);
        }
        reporter.file_completed();
    }
    for relative in &expected.directories {
        cancelled(cancellation)?;
        check_plain(&root.join(relative), true)?;
        add_parents(&mut verified.directories, relative);
    }
    for (relative, original) in &expected.acfs {
        cancelled(cancellation)?;
        let path = root.join(relative);
        let mut file = open_read(&path)?;
        let before = stamp(&file)?;
        let mut current = Vec::new();
        (&mut file)
            .take(MAX_ACF as u64 + 1)
            .read_to_end(&mut current)?;
        if current.len() > MAX_ACF || current != original.as_bytes() || stamp(&file)? != before {
            return Err(invalid("Steam ACF changed during package verification"));
        }
        let digest = hex(&Sha256::digest(&current));
        on_verified_file(relative, &file, &digest)?;
        verified.files.insert(relative.clone(), digest);
        reporter.bytes_processed(current.len() as u64);
        reporter.file_completed();
        add_parents(&mut verified.directories, relative);
    }
    cancelled(cancellation)?;
    reporter.finish();
    Ok(verified)
}

pub(super) fn cancelled(cancellation: Option<&AtomicBool>) -> io::Result<()> {
    if cancellation.is_some_and(|signal| signal.load(Ordering::Acquire)) {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Steam package verification cancelled",
        ))
    } else {
        Ok(())
    }
}

fn add_parents(directories: &mut BTreeSet<String>, relative: &str) {
    for parent in Path::new(relative).ancestors().skip(1) {
        if !parent.as_os_str().is_empty() {
            directories.insert(parent.to_string_lossy().replace('\\', "/"));
        }
    }
}

fn optional_file_size(path: &Path) -> io::Result<Option<u64>> {
    match check_plain(path, false) {
        Ok(()) => std::fs::metadata(path).map(|metadata| Some(metadata.len())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            // An absent leaf must not hide an existing linked or invalid parent.
            for ancestor in path.ancestors().skip(1) {
                match check_plain(ancestor, true) {
                    Ok(()) => return Ok(None),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

#[derive(PartialEq, Eq)]
struct Stamp {
    length: u64,
    modified: SystemTime,
    created: Option<SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64),
}

fn stamp(file: &File) -> io::Result<Stamp> {
    let metadata = file.metadata()?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(invalid("Steam package handle refers to a reparse point"));
        }
    }
    if !metadata.is_file() {
        return Err(invalid("Steam package handle is not a plain file"));
    }
    Ok(Stamp {
        length: metadata.len(),
        modified: metadata.modified()?,
        created: metadata.created().ok(),
        #[cfg(unix)]
        identity: {
            use std::os::unix::fs::MetadataExt;
            (metadata.dev(), metadata.ino())
        },
    })
}

fn verify_file(
    file: &mut File,
    path: &Path,
    candidates: &[&Entry],
    required: bool,
    cancellation: Option<&AtomicBool>,
    reporter: &mut VerificationReporter<'_>,
) -> io::Result<Option<String>> {
    let before = stamp(file)?;
    if !candidates.iter().any(|entry| entry.size == before.length) {
        return if required {
            Err(invalid("official Steam file size mismatch"))
        } else {
            Ok(None)
        };
    }
    let mut sha1 = Sha1::new();
    let mut sha256 = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut size = 0_u64;
    loop {
        cancelled(cancellation)?;
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        size = size
            .checked_add(count as u64)
            .ok_or_else(|| invalid("Steam file size overflow"))?;
        if size > before.length {
            return Err(invalid("Steam file grew during verification"));
        }
        sha1.update(&buffer[..count]);
        sha256.update(&buffer[..count]);
        reporter.bytes_processed(count as u64);
    }
    if stamp(file)? != before {
        return Err(invalid("Steam file metadata changed during verification"));
    }
    check_plain(path, false)?;
    let current = open_read(path)?;
    if stamp(&current)? != before {
        return Err(invalid("Steam file path changed during verification"));
    }
    let digest = hex(&sha1.finalize());
    if !candidates
        .iter()
        .any(|entry| entry.size == size && entry.sha1 == digest)
    {
        return if required {
            Err(invalid("official Steam file SHA1 mismatch"))
        } else {
            Ok(None)
        };
    }
    Ok(Some(hex(&sha256.finalize())))
}

struct VerificationReporter<'a> {
    progress: InstallVerificationProgress,
    last_report: Instant,
    on_progress: &'a mut dyn FnMut(InstallVerificationProgress),
}

impl<'a> VerificationReporter<'a> {
    fn new(
        total_bytes: u64,
        total_files: u64,
        on_progress: &'a mut dyn FnMut(InstallVerificationProgress),
    ) -> Self {
        let progress = InstallVerificationProgress {
            stage: InstallVerificationStage::SteamFiles,
            completed_bytes: 0,
            total_bytes,
            completed_files: 0,
            total_files,
        };
        on_progress(progress.clone());
        Self {
            progress,
            last_report: Instant::now(),
            on_progress,
        }
    }

    fn bytes_processed(&mut self, bytes: u64) {
        self.progress.completed_bytes += bytes;
        self.report_if_due();
    }

    fn file_completed(&mut self) {
        self.progress.completed_files += 1;
        self.report_if_due();
    }

    fn report_if_due(&mut self) {
        if self.last_report.elapsed() >= Duration::from_millis(100) {
            (self.on_progress)(self.progress.clone());
            self.last_report = Instant::now();
        }
    }

    fn finish(&mut self) {
        (self.on_progress)(self.progress.clone());
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
#[path = "steam_depot_verification_tests.rs"]
mod tests;
