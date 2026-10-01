use crate::runtime_log_stream::file_identity::{LogFileIdentity, log_identity};
use serde::Serialize;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const SCAN_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Default, Serialize)]
pub(super) struct LogEvidence {
    pub source: String,
    pub path: PathBuf,
    pub bytes: u64,
    pub baseline_bytes: u64,
    pub scan_start_offset: u64,
    pub generation: String,
    pub end_offset: u64,
    pub transcript: super::transcript::Signature,
    pub scanned_bytes: u64,
    pub game_lines: u64,
    pub segments: usize,
    pub missing_prefix_or_gap: bool,
    pub capture_incomplete: bool,
    pub retention_marker: bool,
    pub read_failed: bool,
    pub scan_limit_reached: bool,
}

pub(super) enum NativeBaseline {
    Missing,
    Unreadable,
    Existing {
        identity: LogFileIdentity,
        length: u64,
        prefix: Vec<u8>,
        boundary: Vec<u8>,
    },
}

impl NativeBaseline {
    pub(super) fn capture(path: &Path) -> Self {
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Self::Missing,
            Err(_) => return Self::Unreadable,
        };
        let capture = (|| -> std::io::Result<Self> {
            let identity = log_identity(path, &file)?;
            let length = file.metadata()?.len();
            let prefix = read_range(&mut file, 0, length.min(4096))?;
            let boundary = read_range(&mut file, length.saturating_sub(4096), length.min(4096))?;
            Ok(Self::Existing {
                identity,
                length,
                prefix,
                boundary,
            })
        })();
        capture.unwrap_or(Self::Unreadable)
    }
}

fn read_range(file: &mut File, offset: u64, length: u64) -> std::io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = vec![0; length as usize];
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// A shared native file is evidence only after its pre-start boundary. Reused
/// old readiness/error lines never count as output from this acceptance run.
pub(super) fn inspect_since(path: &Path, baseline: NativeBaseline) -> LogEvidence {
    let mut evidence = LogEvidence {
        source: "native_increment".into(),
        path: path.into(),
        ..Default::default()
    };
    let result = (|| -> std::io::Result<()> {
        let mut file = File::open(path)?;
        evidence.bytes = file.metadata()?.len();
        evidence.end_offset = evidence.bytes;
        evidence.segments = 1;
        let (start, generation) = match baseline {
            NativeBaseline::Unreadable => {
                return Err(std::io::Error::other("native baseline unavailable"));
            }
            NativeBaseline::Missing => (0, "created"),
            NativeBaseline::Existing {
                identity,
                length,
                prefix,
                boundary,
            } => {
                evidence.baseline_bytes = length;
                if log_identity(path, &file)? != identity {
                    (0, "replaced")
                } else if evidence.bytes < length
                    || read_range(&mut file, 0, prefix.len() as u64)? != prefix
                    || read_range(
                        &mut file,
                        length.saturating_sub(4096),
                        boundary.len() as u64,
                    )? != boundary
                {
                    (0, "truncated_or_rewritten")
                } else {
                    (length, "appended")
                }
            }
        };
        evidence.scan_start_offset = start;
        evidence.generation = generation.into();
        file.seek(SeekFrom::Start(start))?;
        let length = evidence.bytes.saturating_sub(start);
        let mut bounded = file.take(length.min(SCAN_BYTES));
        let mut scan = Scan::default();
        let mut transcript = super::transcript::Accumulator::default();
        scan_file(&mut bounded, &mut scan, &mut evidence, &mut transcript)?;
        scan.finish(&mut evidence);
        evidence.transcript = transcript.finish();
        evidence.game_lines = evidence.transcript.game_lines;
        evidence.scan_limit_reached = evidence.scanned_bytes < length;
        Ok(())
    })();
    evidence.read_failed = result.is_err();
    evidence
}

fn scan_file(
    reader: &mut impl Read,
    scan: &mut Scan,
    evidence: &mut LogEvidence,
    transcript: &mut super::transcript::Accumulator,
) -> std::io::Result<()> {
    let mut buffer = [0u8; 65536];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            return Ok(());
        }
        evidence.scanned_bytes += count as u64;
        scan.push(&buffer[..count], evidence);
        transcript.bytes(&buffer[..count]);
    }
}

impl LogEvidence {
    pub(super) fn sound(&self) -> bool {
        !self.read_failed
            && !self.missing_prefix_or_gap
            && !self.capture_incomplete
            && !self.retention_marker
            && !self.scan_limit_reached
            && !self.transcript.line_limit_exceeded
    }
}

pub(super) fn inspect(path: &Path, source: &str) -> LogEvidence {
    let mut evidence = LogEvidence {
        source: source.into(),
        path: path.into(),
        generation: "run_owned".into(),
        ..Default::default()
    };
    let result = (|| -> std::io::Result<()> {
        let segments = app_storage::managed_console_log::open_log_segments(path)?;
        let mut files: Vec<(u64, File)> = match segments {
            Some(segments) => segments
                .into_iter()
                .map(|part| (part.start_offset, part.file))
                .collect(),
            None => vec![(0, File::open(path)?)],
        };
        evidence.segments = files.len();
        evidence.missing_prefix_or_gap = files.first().is_none_or(|(offset, _)| *offset != 0);
        let mut expected = 0;
        let mut scan = Scan::default();
        let mut transcript = super::transcript::Accumulator::default();
        for (offset, file) in &mut files {
            let length = file.metadata()?.len();
            evidence.missing_prefix_or_gap |= *offset != expected;
            expected = offset.saturating_add(length);
            evidence.end_offset = expected;
            evidence.bytes = evidence.bytes.saturating_add(length);
            let available = SCAN_BYTES.saturating_sub(evidence.scanned_bytes);
            let mut bounded = file.take(length.min(available));
            scan_file(&mut bounded, &mut scan, &mut evidence, &mut transcript)?;
        }
        scan.finish(&mut evidence);
        evidence.transcript = transcript.finish();
        evidence.game_lines = evidence.transcript.game_lines;
        evidence.scan_limit_reached = evidence.scanned_bytes < evidence.bytes;
        Ok(())
    })();
    evidence.read_failed = result.is_err();
    evidence
}

#[derive(Default)]
struct Scan {
    prefix: Vec<u8>,
    length: usize,
}

impl Scan {
    fn push(&mut self, bytes: &[u8], evidence: &mut LogEvidence) {
        for fragment in bytes.split_inclusive(|byte| *byte == b'\n') {
            self.length = self.length.saturating_add(fragment.len());
            let prefix_bytes = 512usize
                .saturating_sub(self.prefix.len())
                .min(fragment.len());
            self.prefix.extend_from_slice(&fragment[..prefix_bytes]);
            let (incomplete, retention) =
                diagnostic_markers(&String::from_utf8_lossy(&self.prefix));
            evidence.capture_incomplete |= incomplete;
            evidence.retention_marker |= retention;
            if fragment.last() == Some(&b'\n') {
                self.finish(evidence);
            }
        }
    }

    fn finish(&mut self, evidence: &mut LogEvidence) {
        if self.length > 0 && game_line(&String::from_utf8_lossy(&self.prefix)) {
            evidence.game_lines += 1;
        }
        *self = Self::default();
    }
}

pub(super) fn game_line(line: &str) -> bool {
    let line = line.trim();
    !line.is_empty() && !line.starts_with("[LanGame ") && !line.starts_with("[LanGame]")
}

pub(super) fn diagnostic_markers(line: &str) -> (bool, bool) {
    let lower = line.trim_start().to_ascii_lowercase();
    let Some(message) = lower.strip_prefix("[langame]").map(str::trim_start) else {
        return (false, false);
    };
    (
        message.starts_with("terminal output capture stopped:")
            || message.starts_with("output capture is incomplete")
            || message.starts_with("console streaming ended before all final output could be read"),
        message.starts_with("earlier console output expired under the log retention policy")
            || message.starts_with("the final console line exceeded the streaming limit")
            || message.starts_with("console log segment unavailable"),
    )
}

#[test]
fn existing_acceptance_diagnostics_require_manager_marker_prefix() {
    for ordinary in [
        "0 objects omitted",
        "retention policy loaded",
        "Output capture is incomplete in a game tutorial",
    ] {
        assert_eq!(diagnostic_markers(ordinary), (false, false));
    }
    assert_eq!(
        diagnostic_markers(
            "[LanGame] Console streaming ended before all final output could be read. Check the retained log files for additional output."
        ),
        (true, false)
    );
    assert_eq!(
        diagnostic_markers(
            "[LanGame] Earlier console output expired under the log retention policy."
        ),
        (false, true)
    );
    assert_eq!(
        diagnostic_markers(
            "[LanGame] The final console line exceeded the streaming limit and was omitted."
        ),
        (false, true)
    );
}

pub(super) fn write_receipt(
    directory: &Path,
    module: &str,
    report: &impl Serialize,
) -> Result<(), String> {
    use std::io::Write;
    let bytes =
        serde_json::to_vec_pretty(report).map_err(|_| "cannot serialize acceptance receipt")?;
    if bytes.len() > 1024 * 1024 {
        return Err("acceptance receipt exceeds its bound".into());
    }
    let path = directory.join(format!("{module}.json"));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "cannot exclusively create acceptance receipt")?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| "acceptance receipt cannot be persisted".into())
}

#[test]
fn existing_acceptance_log_summary_rejects_only_banners_and_split_capture_failure() {
    let mut evidence = LogEvidence::default();
    let mut scan = Scan::default();
    scan.push(
        b"[LanGame startup] Preparing Server ready startup...\n",
        &mut evidence,
    );
    assert_eq!(evidence.game_lines, 0);
    scan.push(
        b"Game server ready\n[LanGame] Terminal output capture stop",
        &mut evidence,
    );
    scan.push(b"ped: stream failed\n", &mut evidence);
    assert_eq!(evidence.game_lines, 1);
    assert!(evidence.capture_incomplete);
    assert!(!evidence.sound());
}

#[test]
fn existing_acceptance_native_summary_excludes_history_and_detects_rewrites() {
    use std::io::Write;
    let path =
        std::env::temp_dir().join(format!("lg-existing-evidence-{}.log", uuid::Uuid::new_v4()));
    struct Remove(std::path::PathBuf);
    impl Drop for Remove {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _remove = Remove(path.clone());
    fs::write(
        &path,
        b"Old server ready\n[LanGame] Output capture is incomplete\n",
    )
    .unwrap();
    let baseline = NativeBaseline::capture(&path);
    let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(b"[LanGame startup] Preparing new run\n")
        .unwrap();
    drop(file);
    let new = inspect_since(&path, baseline);
    assert_eq!(
        new.game_lines, 0,
        "old ready lines must not establish current game output"
    );
    assert!(
        !new.capture_incomplete,
        "old capture failures belong to their previous run"
    );
    assert_eq!(new.generation, "appended");
    assert!(new.scan_start_offset > 0);
    assert!(new.sound());

    let baseline = NativeBaseline::capture(&path);
    fs::write(&path, b"Fresh game output\n").unwrap();
    let replaced = inspect_since(&path, baseline);
    assert_eq!(replaced.generation, "truncated_or_rewritten");
    assert_eq!(replaced.scan_start_offset, 0);
    assert_eq!(replaced.game_lines, 1);
    assert!(replaced.sound());
}
