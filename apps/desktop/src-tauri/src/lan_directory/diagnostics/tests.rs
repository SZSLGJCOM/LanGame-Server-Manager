use super::*;

struct BrokenWriter;

impl Write for BrokenWriter {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "reader closed"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "reader closed"))
    }
}

#[test]
fn closed_console_does_not_panic_and_preserves_directory_diagnostic() {
    let mut persisted = None;
    record_with(
        &mut BrokenWriter,
        |entry| {
            persisted = Some(entry.clone());
            Ok(())
        },
        "warning",
        "lan_directory.interface_failed",
        "Directory interface became unavailable",
    );
    let entry = persisted.expect("closed console must not prevent persistent diagnostics");
    assert_eq!(entry["level"], "warning");
    assert_eq!(entry["action"], "lan_directory.interface_failed");
    assert_eq!(entry["message"], "Directory interface became unavailable");
    assert!(entry["ts_unix_ms"].as_u64().is_some());
}

#[test]
fn unavailable_console_and_log_do_not_panic_during_directory_failure() {
    record_with(
        &mut BrokenWriter,
        |_| {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "log unavailable",
            ))
        },
        "warning",
        "lan_directory.interface_failed",
        "Directory interface became unavailable",
    );
}

#[test]
fn persistent_log_failure_remains_visible_on_an_available_console() {
    let mut console = Vec::new();
    record_with(
        &mut console,
        |_| {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "log unavailable",
            ))
        },
        "warning",
        "lan_directory.interface_failed",
        "Directory interface became unavailable",
    );
    let output = String::from_utf8(console).expect("diagnostic is UTF-8");
    assert!(output.contains("Directory interface became unavailable"));
    assert!(output.contains("diagnostic log unavailable: log unavailable"));
}
