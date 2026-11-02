use super::*;
use crate::managed_console_log::owned_fs::{identity, reject_links};

pub(super) fn read(
    source_path: String,
    process: &InstanceProcessState,
    boundary: Option<&NativeStartupSource>,
) -> (LogTailSnapshot, bool) {
    // Keep the existing timestamp-based reader for runs without a checkpoint.
    let Some(boundary) = boundary.filter(|value| value.process_key == process.process_key) else {
        return (
            read_log_snapshot(Some(source_path), RUNTIME_STARTUP_SCAN_LINE_LIMIT),
            false,
        );
    };
    let path = Path::new(&source_path);
    let result = (|| -> std::io::Result<_> {
        reject_links(path)?;
        let mut file = fs::File::open(path)?;
        let captured_identity = identity(&file)?;
        let creation_time = process
            .process_identity
            .as_ref()
            .map_or(0, |value| value.creation_time);
        let offset = boundary.validated_offset(path, &mut file, creation_time)?;
        let floor = offset.unwrap_or(0);
        let length = file.metadata()?.len();
        let start = floor
            .max(length.saturating_sub(log_tail_read_byte_limit(RUNTIME_STARTUP_SCAN_LINE_LIMIT)));
        let pending_lf = if start == floor && floor > 0 {
            file.seek(SeekFrom::Start(floor - 1))?;
            let mut previous = [0];
            file.read_exact(&mut previous)?;
            previous[0] == b'\r'
        } else {
            false
        };
        file.seek(SeekFrom::Start(start))?;
        let mut bytes = Vec::new();
        (&mut file).take(length - start).read_to_end(&mut bytes)?;
        // Do not label bytes read from a replaced file as the current document.
        reject_links(path)?;
        if identity(&fs::File::open(path)?)? != captured_identity {
            return Err(std::io::Error::other(
                "Native log changed identity during its read",
            ));
        }
        // A checkpoint may end at CR before the producer writes its paired LF.
        let bytes = if pending_lf { bytes.strip_prefix(b"\n").unwrap_or(&bytes) } else { &bytes };
        let normalized = String::from_utf8_lossy(bytes)
            .replace("\r\n", "\n")
            .replace('\r', "\n");
        let truncated = start > floor;
        let content = if truncated {
            normalized.split_once('\n').map_or("", |(_, tail)| tail)
        } else {
            &normalized
        };
        let lines = content
            .lines()
            .map(app_core::abiotic_output::redact_modio_token_text)
            .collect::<Vec<_>>();
        Ok((lines, truncated, offset.is_some_and(|value| value > 0)))
    })();
    match result {
        Ok((lines, truncated, current)) => (
            LogTailSnapshot {
                source_path: Some(source_path),
                total_lines: lines.len(),
                lines,
                truncated,
                read_error: None,
            },
            current,
        ),
        Err(error) => (
            LogTailSnapshot {
                source_path: Some(source_path),
                total_lines: 0,
                lines: vec![],
                truncated: false,
                read_error: Some(error.to_string()),
            },
            false,
        ),
    }
}
