use super::*;
use std::io::{self, Cursor};

const INFO: &str = include_str!("../test-data/live-players/humanitz/normal.txt");
const EMPTY: &str = include_str!("../test-data/live-players/humanitz/empty.txt");

struct ScriptedStream {
    input: Cursor<Vec<u8>>,
    output: Vec<u8>,
}

impl ScriptedStream {
    fn new(packets: &[(i32, i32, &str)]) -> Self {
        let mut bytes = Vec::new();
        for (id, kind, body) in packets {
            source_rcon_write_packet(&mut bytes, *id, *kind, body).unwrap();
        }
        Self {
            input: Cursor::new(bytes),
            output: Vec::new(),
        }
    }
}

impl Read for ScriptedStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        // Exercise read_exact with fragmented TCP delivery independently of
        // the logical response-frame boundaries.
        let maximum = buffer.len().min(3);
        self.input.read(&mut buffer[..maximum])
    }
}

impl Write for ScriptedStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.output.write(buffer)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn fixed_zero_ids_and_fragmented_names_collect_without_a_marker_command() {
    let split = INFO.find("Another Survivor").unwrap() + "Another Surv".len();
    let mut stream = ScriptedStream::new(&[
        (0, 0, "None"),
        (0, 2, ""),
        (0, 0, &INFO[..split]),
        (0, 0, &INFO[split..]),
    ]);
    assert_eq!(exchange(&mut stream, "synthetic-password").unwrap(), INFO);
    let mut sent = Cursor::new(stream.output);
    let auth = read_packet(&mut sent).unwrap();
    assert_eq!((auth.id, auth.packet_type), (AUTH_ID, 3));
    let query = read_packet(&mut sent).unwrap();
    assert_eq!(
        (query.id, query.packet_type, query.body.as_str()),
        (INFO_ID, 2, "info")
    );
    assert_eq!(sent.position() as usize, sent.get_ref().len());
}

#[test]
fn explicit_empty_and_echoed_request_ids_are_supported() {
    let mut stream = ScriptedStream::new(&[(AUTH_ID, 2, ""), (INFO_ID, 0, EMPTY)]);
    assert_eq!(exchange(&mut stream, "synthetic-password").unwrap(), EMPTY);
}

#[test]
fn native_empty_marker_returns_without_waiting_for_another_frame() {
    let text = format!("{}No players connected", EMPTY.replace('\n', "\r\n"));
    let mut stream = ScriptedStream::new(&[(1, 0, "None"), (1, 2, "None"), (0, 0, &text)]);
    assert_eq!(exchange(&mut stream, "synthetic-password").unwrap(), text);
}

#[test]
fn authentication_failure_missing_confirmation_and_wrong_ids_are_errors() {
    for packets in [
        vec![(-1, 2, "")],
        vec![(0, 0, "None"), (0, 0, "None"), (0, 0, "None")],
        vec![(9, 2, "")],
        vec![(0, 2, ""), (9, 0, INFO)],
        vec![(0, 2, ""), (0, 2, INFO)],
    ] {
        let mut stream = ScriptedStream::new(&packets);
        assert!(exchange(&mut stream, "synthetic-password").is_err());
    }
}

#[test]
fn incomplete_unknown_and_unterminated_responses_are_errors() {
    for text in ["", "Unknown command\n", INFO.trim_end_matches('\n')] {
        let mut stream = ScriptedStream::new(&[(0, 2, ""), (0, 0, text)]);
        assert!(exchange(&mut stream, "synthetic-password").is_err());
    }
    let mut packets = vec![(0, 2, "")];
    packets.extend((0..MAX_FRAMES).map(|_| (0, 0, "")));
    let mut stream = ScriptedStream::new(&packets);
    assert!(
        exchange(&mut stream, "synthetic-password")
            .unwrap_err()
            .contains("frame limit")
    );
    let oversized = "x".repeat(MAX_RESPONSE_BYTES);
    let mut stream = ScriptedStream::new(&[(0, 2, ""), (0, 0, &oversized), (0, 0, "x")]);
    assert!(
        exchange(&mut stream, "synthetic-password")
            .unwrap_err()
            .contains("capture limit")
    );
}

#[test]
fn invalid_lengths_utf8_terminators_and_truncated_frames_fail_closed() {
    for length in [-1, 9, MAX_RESPONSE_BYTES as i32 + 11] {
        assert!(read_packet(&mut Cursor::new(length.to_le_bytes())).is_err());
    }
    let mut valid = Vec::new();
    source_rcon_write_packet(&mut valid, 0, 0, "a").unwrap();
    let mut invalid_utf8 = valid.clone();
    invalid_utf8[12] = 0xff;
    let mut missing_terminator = valid.clone();
    *missing_terminator.last_mut().unwrap() = 1;
    let mut truncated = valid;
    truncated.pop();
    for bytes in [invalid_utf8, missing_terminator, truncated] {
        assert!(read_packet(&mut Cursor::new(bytes)).is_err());
    }
    let mut embedded_nul = Vec::new();
    source_rcon_write_packet(&mut embedded_nul, 0, 0, "a\0b").unwrap();
    assert!(read_packet(&mut Cursor::new(embedded_nul)).is_err());
}

#[test]
fn transport_rejects_unverified_commands_before_network_access() {
    assert!(humanitz_rcon_info("invalid", "synthetic-password", "Players").is_err());
    assert!(humanitz_rcon_info("invalid", "", "info").is_err());
    assert!(humanitz_rcon_info("not-a-numeric-host:8888", "synthetic-password", "info").is_err());
}
