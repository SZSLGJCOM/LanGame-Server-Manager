use std::io::{Cursor, Read};

use super::{MAGIC, MAX_OUTPUT, read_worker_output};

fn frame(status: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = MAGIC.to_vec();
    bytes.push(status);
    bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

#[test]
fn protocol_reads_utf8_and_accepts_only_the_bounded_harness_prefix() {
    let expected = "Server guide: 保存世界，然后重启。";
    for prefix_len in [0, 128] {
        let mut bytes = vec![b' '; prefix_len];
        bytes.extend(frame(0, expected.as_bytes()));
        assert_eq!(
            read_worker_output(Cursor::new(bytes)).unwrap().unwrap(),
            expected
        );
    }
    let mut bytes = vec![b' '; 129];
    bytes.extend(frame(0, expected.as_bytes()));
    assert!(read_worker_output(Cursor::new(bytes)).is_err());
}

#[test]
fn protocol_rejects_malformed_status_text_and_incomplete_frames() {
    let mut incomplete_payload = frame(0, b"manual");
    incomplete_payload.pop();
    let mut trailing_data = frame(0, b"manual");
    trailing_data.push(b'!');
    for bytes in [
        b"not a worker response".to_vec(),
        MAGIC[..MAGIC.len() - 1].to_vec(),
        MAGIC.to_vec(),
        frame(2, b"manual"),
        frame(0, &[0xff]),
        incomplete_payload,
        trailing_data,
    ] {
        assert!(read_worker_output(Cursor::new(bytes)).is_err());
    }
    let error = read_worker_output(Cursor::new(frame(1, b"Unsupported PDF")))
        .unwrap()
        .unwrap_err();
    assert!(error.to_string().contains("Unsupported PDF"));
}

struct HeaderOnlyReader(Cursor<Vec<u8>>);

impl Read for HeaderOnlyReader {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        assert!(
            self.0.position() < self.0.get_ref().len() as u64,
            "Rejected length must not cause a payload read"
        );
        self.0.read(output)
    }
}

#[test]
fn protocol_rejects_oversized_declared_payloads_before_reading_them() {
    for (status, length) in [(0, MAX_OUTPUT as u64 + 1), (0, u64::MAX), (1, 4097)] {
        let mut header = MAGIC.to_vec();
        header.push(status);
        header.extend_from_slice(&length.to_le_bytes());
        assert!(read_worker_output(HeaderOnlyReader(Cursor::new(header))).is_err());
    }
    let payload = vec![b'a'; MAX_OUTPUT];
    let output = read_worker_output(Cursor::new(frame(0, &payload)))
        .unwrap()
        .unwrap();
    assert_eq!(output.as_bytes(), payload);
}

// This is an ordinary one-page PDF with computed byte offsets, not a parser mock.
#[cfg(windows)]
fn valid_pdf() -> Vec<u8> {
    use std::fmt::Write;

    let content =
        "BT /F1 12 Tf 72 720 Td (Server guide: preserve the world before restart.) Tj ET\n";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        format!("<< /Length {} >>\nstream\n{content}endstream", content.len()),
    ];
    let mut pdf = "%PDF-1.4\n".to_owned();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        writeln!(pdf, "{} 0 obj\n{object}\nendobj", index + 1).unwrap();
    }
    let xref_offset = pdf.len();
    writeln!(pdf, "xref\n0 {}\n0000000000 65535 f ", objects.len() + 1).unwrap();
    for offset in offsets {
        writeln!(pdf, "{offset:010} 00000 n ").unwrap();
    }
    writeln!(
        pdf,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF",
        objects.len() + 1
    )
    .unwrap();
    pdf.into_bytes()
}

#[test]
#[ignore = "Entry point for an isolated PDF worker child process"]
fn worker_entry() {
    #[cfg(windows)]
    if let Ok(mode) = std::env::var("LANGAME_PDF_TEST_MODE") {
        run_test_worker(&mode);
        std::process::exit(0);
    }
    std::process::exit(super::run_worker_stdio());
}

#[cfg(windows)]
fn run_test_worker(mode: &str) {
    use std::io::Write;

    let mut input = std::io::stdin().lock();
    let mut magic = [0; 16];
    input.read_exact(&mut magic).unwrap();
    assert_eq!(&magic, MAGIC);
    super::verify_worker_isolation().expect("Test worker must inherit the production Job limits");
    let mut length = [0; 8];
    input.read_exact(&mut length).unwrap();
    let length = u64::from_le_bytes(length);
    assert!(length <= super::MAX_INPUT as u64);
    assert_eq!(
        std::io::copy(&mut input.take(length), &mut std::io::sink()).unwrap(),
        length
    );

    if let Some(path) = std::env::var_os("LANGAME_PDF_TEST_READY") {
        std::fs::write(path, "ready").unwrap();
    }
    match mode {
        "sleep" => std::thread::sleep(std::time::Duration::from_secs(60)),
        "oversized" => {
            let mut output = std::io::stdout().lock();
            output.write_all(MAGIC).unwrap();
            output.write_all(&[0]).unwrap();
            output
                .write_all(&(MAX_OUTPUT as u64 + 1).to_le_bytes())
                .unwrap();
            output.flush().unwrap();
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
        _ => panic!("Unknown PDF test worker mode"),
    }
}

#[cfg(windows)]
mod windows {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use crate::KnowledgeError;

    fn command(mode: &str) -> std::process::Command {
        let mut command = super::super::worker_command().unwrap();
        command.env("LANGAME_PDF_TEST_MODE", mode);
        command
    }

    #[test]
    fn ordinary_pdf_round_trips_through_the_restricted_worker() {
        let output = super::super::extract(
            &super::valid_pdf(),
            &AtomicBool::new(false),
            Instant::now() + Duration::from_secs(10),
        )
        .unwrap();
        assert!(output.contains("Server guide: preserve the world before restart."));

        if let Some(executable) = std::env::var_os("LANGAME_PDF_DESKTOP_EXE") {
            use std::os::windows::process::CommandExt;
            use std::process::{Command, Stdio};

            let executable = std::path::PathBuf::from(executable);
            assert!(
                executable.is_absolute(),
                "Desktop executable must be absolute"
            );
            assert!(executable.is_file(), "Desktop executable must exist");
            assert_eq!(
                executable.file_name().and_then(|name| name.to_str()),
                Some("langame-desktop.exe"),
                "Use the built desktop executable for the startup-routing check"
            );
            let mut command = Command::new(&executable);
            command
                .arg(super::super::WORKER_ARGUMENT)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
            let output = super::super::windows::run(
                command,
                &super::valid_pdf(),
                &AtomicBool::new(false),
                Instant::now() + Duration::from_secs(10),
            )
            .expect("Packaged desktop PDF worker must start before desktop initialization");
            assert!(output.contains("Server guide: preserve the world before restart."));
            eprintln!("Packaged desktop PDF worker round trip passed");
        }
    }

    #[test]
    fn malformed_pdf_returns_the_parser_cause_from_the_worker() {
        let error = super::super::extract(
            b"This is not a PDF document.",
            &AtomicBool::new(false),
            Instant::now() + Duration::from_secs(10),
        )
        .unwrap_err();
        assert!(error.to_string().contains("Cannot parse PDF"), "{error}");
    }

    #[test]
    fn preflight_rejects_cancelled_oversized_and_expired_requests() {
        let result = super::super::extract(
            &super::valid_pdf(),
            &AtomicBool::new(true),
            Instant::now() + Duration::from_secs(10),
        );
        assert!(matches!(result, Err(KnowledgeError::Cancelled)));
        let oversized = super::super::extract(
            &vec![0; super::super::MAX_INPUT + 1],
            &AtomicBool::new(false),
            Instant::now() + Duration::from_secs(10),
        )
        .unwrap_err();
        assert!(
            oversized.to_string().contains("exceeds 8 MiB"),
            "{oversized}"
        );
        let expired = super::super::extract(
            &super::valid_pdf(),
            &AtomicBool::new(false),
            Instant::now() - Duration::from_secs(1),
        )
        .unwrap_err();
        assert!(
            expired.to_string().contains("deadline has expired"),
            "{expired}"
        );
    }

    #[test]
    fn a_nonresponsive_worker_is_terminated_at_the_deadline() {
        let error = super::super::windows::run(
            command("sleep"),
            &super::valid_pdf(),
            &AtomicBool::new(false),
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap_err();
        assert!(error.to_string().contains("timed out"), "{error}");
    }

    #[test]
    fn an_oversized_worker_response_is_rejected_without_waiting_for_exit() {
        let error = super::super::windows::run(
            command("oversized"),
            &super::valid_pdf(),
            &AtomicBool::new(false),
            Instant::now() + Duration::from_secs(10),
        )
        .unwrap_err();
        assert!(error.to_string().contains("exceed"), "{error}");
    }

    struct ReadyDirectory(std::path::PathBuf);

    impl ReadyDirectory {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("langame-pdf-worker-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for ReadyDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(self.0.join("ready"));
            let _ = std::fs::remove_dir(&self.0);
        }
    }

    #[test]
    fn cancellation_terminates_a_worker_that_has_received_its_input() {
        let directory = ReadyDirectory::new();
        let ready = directory.0.join("ready");
        let cancel = AtomicBool::new(false);
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut command = command("sleep");
        command.env("LANGAME_PDF_TEST_READY", &ready);
        std::thread::scope(|scope| {
            let observer = scope.spawn(|| {
                while !ready.is_file() {
                    assert!(
                        Instant::now() < deadline,
                        "PDF worker did not signal readiness"
                    );
                    std::thread::sleep(Duration::from_millis(5));
                }
                cancel.store(true, Ordering::Release);
            });
            let result =
                super::super::windows::run(command, &super::valid_pdf(), &cancel, deadline);
            observer.join().unwrap();
            assert!(
                matches!(result, Err(KnowledgeError::Cancelled)),
                "{result:?}"
            );
        });
    }
}
