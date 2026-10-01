use super::*;

fn capture_bytes(bytes: &[u8], log: impl Write, state: Arc<TerminalState>) {
    let (read, write) = create_pipe().unwrap();
    let mut write = unsafe { File::from_raw_handle(write.into_raw()) };
    write.write_all(bytes).unwrap();
    drop(write);
    drain_output(
        unsafe { File::from_raw_handle(read.into_raw()) },
        log,
        state,
    );
}

#[test]
fn unsupported_terminal_output_is_visible_in_the_run_log() {
    let state = Arc::new(TerminalState::default());
    let mut bytes = Vec::new();
    capture_bytes(b"\x1bZ\r\nlater text\r\n", &mut bytes, Arc::clone(&state));
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("[LanGame] Terminal output capture stopped:"));
    assert!(!text.contains("later text"));
    assert!(!text.contains('\u{1b}'));
    assert!(state.output_failed.load(Ordering::Acquire));
}

#[test]
fn malformed_terminal_chunk_preserves_completed_lines_and_decoded_crash_tail() {
    let state = Arc::new(TerminalState::default());
    let mut bytes = Vec::new();
    capture_bytes(
        b"ready\r\nfatal: configuration rejected\x1bZhidden\r\n",
        &mut bytes,
        Arc::clone(&state),
    );
    let text = String::from_utf8(bytes).unwrap();
    assert!(
        text.starts_with("ready\nfatal: configuration rejected\n"),
        "{text}"
    );
    assert!(text.contains("[LanGame] Terminal output capture stopped:"));
    assert!(!text.contains("hidden"));
    assert!(!text.contains('\u{1b}'));
    assert!(state.output_failed.load(Ordering::Acquire));
}

#[test]
fn incomplete_terminal_tail_preserves_the_known_text_before_rejected_bytes() {
    for tail in [b"\xe7\x8e".as_slice(), b"\x1b[", b"\x1b]title"] {
        let state = Arc::new(TerminalState::default());
        let mut input = b"fatal: server stopped".to_vec();
        input.extend_from_slice(tail);
        let mut bytes = Vec::new();
        capture_bytes(&input, &mut bytes, Arc::clone(&state));
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with("fatal: server stopped\n"), "{text}");
        assert!(text.contains("[LanGame] Terminal output capture stopped:"));
        assert!(!text.contains('\u{fffd}'));
        assert!(!text.contains('\u{1b}'));
        assert!(state.output_failed.load(Ordering::Acquire));
    }
}

#[test]
fn incomplete_terminal_tail_is_reported_instead_of_silently_omitted() {
    let state = Arc::new(TerminalState::default());
    let mut bytes = Vec::new();
    capture_bytes(b"complete\r\n\xe7\x8e", &mut bytes, Arc::clone(&state));
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.starts_with("complete\n"));
    assert!(text.contains("[LanGame] Terminal output capture stopped:"));
    assert!(state.output_failed.load(Ordering::Acquire));
}

struct FlushFailure {
    flushed: bool,
}

impl Write for FlushFailure {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flushed = true;
        Err(io::Error::other("synthetic flush failure"))
    }
}

#[test]
fn terminal_completion_flushes_the_sink_and_retains_failure_state() {
    let state = Arc::new(TerminalState::default());
    let mut sink = FlushFailure { flushed: false };
    capture_bytes(b"last line", &mut sink, Arc::clone(&state));
    assert!(sink.flushed);
    assert!(state.output_failed.load(Ordering::Acquire));
}

#[test]
fn interrupted_terminal_output_preserves_decoded_rows_before_the_failure_marker() {
    for timeout in [false, true] {
        let state = Arc::new(TerminalState::default());
        let prefix = b"ready\r\nfatal: last known prefix";
        let mut first_read = true;
        let mut output = Vec::new();
        drain_output_with_reader(
            |buffer| {
                if first_read {
                    first_read = false;
                    buffer[..prefix.len()].copy_from_slice(prefix);
                    state.closed.store(timeout, Ordering::Release);
                    return Ok(prefix.len());
                }
                assert!(!timeout, "an expired drain must not read again");
                Err(io::Error::other("synthetic non-EOF pipe failure"))
            },
            &mut output,
            Arc::clone(&state),
            Duration::ZERO,
        );
        let output = String::from_utf8(output).unwrap();
        assert!(
            output.starts_with("ready\nfatal: last known prefix\n"),
            "{output}"
        );
        assert_eq!(
            output
                .matches("[LanGame] Terminal output capture stopped:")
                .count(),
            1
        );
        assert!(output.contains("Output capture is incomplete"), "{output}");
        assert!(
            output.contains(if timeout {
                "before EOF"
            } else {
                "synthetic non-EOF pipe failure"
            }),
            "{output}"
        );
        assert!(state.output_failed.load(Ordering::Acquire));
    }
}
