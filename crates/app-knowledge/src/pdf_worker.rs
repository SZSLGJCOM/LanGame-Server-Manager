//! PDF parsing runs in an owned, memory-limited child of this executable.
//! No input reaches the parser until the parent has applied OS resource limits.

use std::io::{self, Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use crate::{KnowledgeError, Result, check_cancel};

const WORKER_ARGUMENT: &str = "--langame-pdf-worker";
const MAGIC: &[u8; 16] = b"LANGAME-PDF-01\0\0";
const MAX_INPUT: usize = 8 * 1024 * 1024;
const MAX_OUTPUT: usize = 2 * 1024 * 1024;
const MAX_ERROR: usize = 4096;
const MAX_PREAMBLE: usize = 128;
const PARSE_TIMEOUT: Duration = Duration::from_secs(30);

#[cfg(windows)]
#[path = "pdf_worker_windows.rs"]
mod windows;

/// Invoke before logging, Tauri, database, or desktop-instance initialization.
/// The dedicated mode accepts bytes on stdin and never interprets paths or URLs.
pub fn run_if_requested() -> Option<i32> {
    let mut arguments = std::env::args_os().skip(1);
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new(WORKER_ARGUMENT)) {
        return None;
    }
    if arguments.next().is_some() {
        return Some(2);
    }
    Some(run_worker_stdio())
}

pub(crate) fn extract(bytes: &[u8], cancel: &AtomicBool, deadline: Instant) -> Result<String> {
    check_cancel(cancel)?;
    if bytes.is_empty() || bytes.len() > MAX_INPUT {
        return Err(KnowledgeError::Invalid(
            "PDF input is empty or exceeds 8 MiB".into(),
        ));
    }
    let deadline = deadline.min(Instant::now() + PARSE_TIMEOUT);
    if Instant::now() >= deadline {
        return Err(KnowledgeError::Unavailable(
            "PDF parsing deadline has expired".into(),
        ));
    }
    #[cfg(windows)]
    {
        windows::run(worker_command()?, bytes, cancel, deadline)
    }
    #[cfg(not(windows))]
    {
        Err(KnowledgeError::Unavailable(
            "PDF ingestion requires Windows process resource isolation".into(),
        ))
    }
}

#[cfg(any(windows, test))]
fn worker_command() -> io::Result<Command> {
    let mut command = Command::new(std::env::current_exe()?);
    #[cfg(not(test))]
    command.arg(WORKER_ARGUMENT);
    #[cfg(test)]
    command.args([
        "--exact",
        "pdf_worker::tests::worker_entry",
        "--ignored",
        "--nocapture",
        "--test-threads=1",
    ]);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }
    Ok(command)
}

fn run_worker_stdio() -> i32 {
    let result = (|| {
        let mut input = io::stdin().lock();
        let mut marker = [0_u8; MAGIC.len()];
        input.read_exact(&mut marker)?;
        if &marker != MAGIC {
            return Err(KnowledgeError::Invalid(
                "Invalid PDF worker protocol".into(),
            ));
        }
        // Receiving the marker means the parent has finished assigning its Job.
        // Direct CLI invocation without the same limits fails before allocation.
        verify_worker_isolation()?;
        let mut length = [0_u8; 8];
        input.read_exact(&mut length)?;
        let length = u64::from_le_bytes(length);
        if length == 0 || length > MAX_INPUT as u64 {
            return Err(KnowledgeError::Invalid(
                "PDF worker input exceeds its limit".into(),
            ));
        }
        let mut bytes = vec![0; length as usize];
        input.read_exact(&mut bytes)?;
        let mut extra = [0];
        if input.read(&mut extra)? != 0 {
            return Err(KnowledgeError::Invalid("Trailing PDF worker input".into()));
        }
        parse_bounded_output(&bytes)
    })();
    let success = result.is_ok();
    if write_worker_output(io::stdout().lock(), result).is_err() {
        return 3;
    }
    if success { 0 } else { 1 }
}

fn verify_worker_isolation() -> Result<()> {
    #[cfg(windows)]
    {
        windows::verify_current_job().map_err(KnowledgeError::Io)
    }
    #[cfg(not(windows))]
    {
        Err(KnowledgeError::Unavailable(
            "PDF worker process isolation is unavailable".into(),
        ))
    }
}

struct BoundedOutput(Vec<u8>);
impl Write for BoundedOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_OUTPUT.saturating_sub(self.0.len()) {
            return Err(io::Error::other("PDF text exceeds 2 MiB"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn parse_bounded_output(bytes: &[u8]) -> Result<String> {
    // This function is reachable from production only after Job verification.
    // The OS limit also covers compressed xrefs, object streams, and recursion,
    // which the parser's output callback cannot constrain.
    let document = pdf_extract::Document::load_mem(bytes)
        .map_err(|error| KnowledgeError::Invalid(format!("Cannot parse PDF: {error}")))?;
    if document.is_encrypted() {
        return Err(KnowledgeError::Invalid(
            "Encrypted PDF documentation is unsupported".into(),
        ));
    }
    let mut body = BoundedOutput(Vec::new());
    let mut output = pdf_extract::PlainTextOutput::new(&mut body as &mut dyn Write);
    pdf_extract::output_doc(&document, &mut output).map_err(|error| {
        KnowledgeError::Invalid(format!("Cannot extract bounded PDF text: {error}"))
    })?;
    String::from_utf8(body.0).map_err(|_| KnowledgeError::Invalid("PDF output is not UTF-8".into()))
}

fn write_worker_output(mut writer: impl Write, result: Result<String>) -> io::Result<()> {
    let (status, mut payload) = match result {
        Ok(body) => (0_u8, body),
        Err(error) => (1_u8, error.to_string()),
    };
    let limit = if status == 0 { MAX_OUTPUT } else { MAX_ERROR };
    if payload.len() > limit {
        if status == 0 {
            return Err(io::Error::other("Oversized PDF worker output"));
        }
        let mut end = limit;
        while !payload.is_char_boundary(end) {
            end -= 1;
        }
        payload.truncate(end);
    }
    writer.write_all(MAGIC)?;
    writer.write_all(&[status])?;
    writer.write_all(&(payload.len() as u64).to_le_bytes())?;
    writer.write_all(payload.as_bytes())?;
    writer.flush()
}

// The outer result validates the transport frame; the inner result is the
// worker's completed parse. Keep them distinct when interpreting process exit.
fn read_worker_output(mut reader: impl Read) -> Result<Result<String>> {
    // libtest prints a small prelude before an explicitly selected child test.
    // The production executable emits the same framed protocol immediately.
    let mut marker = [0_u8; 16];
    reader.read_exact(&mut marker)?;
    let mut skipped = 0;
    while &marker != MAGIC {
        if skipped == MAX_PREAMBLE {
            return Err(KnowledgeError::Invalid(
                "Invalid PDF worker response marker".into(),
            ));
        }
        marker.rotate_left(1);
        reader.read_exact(&mut marker[15..])?;
        skipped += 1;
    }
    let mut status = [0];
    let mut length = [0; 8];
    reader.read_exact(&mut status)?;
    reader.read_exact(&mut length)?;
    let limit = match status[0] {
        0 => MAX_OUTPUT,
        1 => MAX_ERROR,
        _ => {
            return Err(KnowledgeError::Invalid(
                "Invalid PDF worker response status".into(),
            ));
        }
    };
    let length = u64::from_le_bytes(length);
    if length > limit as u64 {
        return Err(KnowledgeError::Invalid(
            "PDF worker response exceeds its byte limit".into(),
        ));
    }
    let mut payload = vec![0; length as usize];
    reader.read_exact(&mut payload)?;
    let mut extra = [0];
    if reader.read(&mut extra)? != 0 {
        return Err(KnowledgeError::Invalid(
            "Trailing PDF worker response".into(),
        ));
    }
    let payload = String::from_utf8(payload)
        .map_err(|_| KnowledgeError::Invalid("PDF worker response is not UTF-8".into()))?;
    Ok(if status[0] == 0 {
        Ok(payload)
    } else {
        Err(KnowledgeError::Invalid(payload))
    })
}

#[cfg(test)]
#[path = "pdf_worker_tests.rs"]
mod tests;
