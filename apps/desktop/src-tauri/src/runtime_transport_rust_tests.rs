use std::io::{Cursor, Read, Write};

use super::super::{
    SourceRconCompletion, source_rcon_exchange, source_rcon_read_packet, source_rcon_write_packet,
};

#[derive(Default)]
struct Peer {
    replies: Cursor<Vec<u8>>,
    requests: Vec<u8>,
}

impl Read for Peer {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.replies.read(buffer)
    }
}

impl Write for Peer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.requests.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn frame(id: i32, kind: i32, body: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    source_rcon_write_packet(&mut bytes, id, kind, body).unwrap();
    bytes
}

fn exchange(replies: Vec<u8>) -> (Result<String, String>, Vec<super::super::SourceRconPacket>) {
    let mut peer = Peer {
        replies: Cursor::new(replies),
        ..Peer::default()
    };
    let result = source_rcon_exchange(
        &mut peer,
        "test-password",
        "server.save",
        SourceRconCompletion::Rust,
    );
    let mut sent = Cursor::new(peer.requests);
    let mut requests = Vec::new();
    while sent.position() < sent.get_ref().len() as u64 {
        requests.push(source_rcon_read_packet(&mut sent).unwrap());
    }
    (result, requests)
}

#[test]
fn rust_rcon_accepts_native_completion_and_interleaved_console_frames() {
    let (result, requests) = exchange(
        [
            frame(14001, 0, ""),
            frame(14001, 2, ""),
            frame(0, 4, "unrelated console output"),
            frame(14002, 0, "Saved "),
            frame(0, 4, "Saved "),
            frame(14002, 0, "world"),
            frame(-1, 0, ""),
        ]
        .concat(),
    );
    assert_eq!(result.unwrap(), "Saved world");
    assert_eq!(
        requests.len(),
        2,
        "Rust needs no additional completion marker"
    );
    assert_eq!((requests[0].id, requests[0].packet_type), (14001, 3));
    assert_eq!((requests[1].id, requests[1].packet_type), (14002, 2));
    assert_eq!(requests[1].body, b"server.save");
}

#[test]
fn rust_rcon_accepts_completion_without_command_output() {
    let (result, requests) = exchange([frame(14001, 2, ""), frame(-1, 0, "")].concat());
    assert_eq!(result.unwrap(), "");
    assert_eq!(requests.len(), 2);
}

#[test]
fn rust_rcon_never_executes_after_negative_authentication_id() {
    for kind in [0, 2] {
        let (result, requests) = exchange([frame(14001, 0, ""), frame(-1, kind, "")].concat());
        assert!(result.unwrap_err().contains("authentication failed"));
        assert_eq!(requests.len(), 1);
    }
}

#[test]
fn rust_rcon_rejects_invalid_completion_and_unknown_frames() {
    for (id, kind, body) in [
        (-1, 0, "unexpected"),
        (-1, 2, ""),
        (14002, 2, ""),
        (14003, 0, "unknown"),
        (0, 0, ""),
    ] {
        let (result, _) = exchange([frame(14001, 2, ""), frame(id, kind, body)].concat());
        assert!(result.unwrap_err().contains("unexpected response"));
    }
}

#[test]
fn rust_rcon_requires_completion_before_disconnect() {
    let (result, _) = exchange([frame(14001, 2, ""), frame(14002, 0, "partial")].concat());
    assert!(result.is_err(), "partial output cannot prove completion");
}

#[test]
fn rust_rcon_bounds_frame_count_and_total_bytes() {
    let mut frames = frame(14001, 2, "");
    for _ in 0..64 {
        frames.extend(frame(0, 4, ""));
    }
    assert!(exchange(frames).0.unwrap_err().contains("64 frames"));

    let mut bytes = frame(14001, 2, "");
    for _ in 0..17 {
        bytes.extend(frame(0, 4, &"x".repeat(16 * 1024)));
    }
    assert!(exchange(bytes).0.unwrap_err().contains("256 KiB"));
}
