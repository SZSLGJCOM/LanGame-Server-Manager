use super::*;
use std::io::{Cursor, Error, ErrorKind};

fn frame(id: i32, kind: i32, body: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    source_rcon_write_packet(&mut bytes, id, kind, body).unwrap();
    bytes
}
struct Scripted {
    input: Cursor<Vec<u8>>,
    output: Vec<u8>,
    command_write_fault: bool,
    writes: usize,
    read_fault: ErrorKind,
}
impl Scripted {
    fn new(frames: Vec<Vec<u8>>) -> Self {
        Self {
            input: Cursor::new(frames.concat()),
            output: Vec::new(),
            command_write_fault: false,
            writes: 0,
            read_fault: ErrorKind::UnexpectedEof,
        }
    }
    fn packets(&self) -> Vec<(i32, i32, String)> {
        let mut cursor = Cursor::new(&self.output);
        let mut packets = Vec::new();
        while cursor.position() < self.output.len() as u64 {
            let packet = read_packet(&mut cursor).unwrap();
            packets.push((packet.id, packet.packet_type, packet.body));
        }
        packets
    }
}
impl Read for Scripted {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        if self.input.position() == self.input.get_ref().len() as u64 {
            return Err(Error::from(self.read_fault));
        }
        self.input.read(bytes)
    }
}
impl Write for Scripted {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.writes += 1;
        if self.command_write_fault && self.writes == 2 {
            self.output.extend_from_slice(&bytes[..5]);
            return Err(Error::from(ErrorKind::BrokenPipe));
        }
        self.output.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn humanitz_save_accepts_the_observed_native_ack_and_sends_no_completion_marker() {
    let mut io = Scripted::new(vec![
        frame(0, 0, "test"),
        frame(0, 2, "test"),
        frame(1, 0, "Game saved!"),
    ]);
    assert_eq!(
        exchange(&mut io, "synthetic-secret").unwrap(),
        "Game saved!"
    );
    assert_eq!(
        io.packets(),
        [(0, 3, "synthetic-secret".into()), (0, 2, "save".into())]
    );
    let mut direct_auth = Scripted::new(vec![frame(0, 2, ""), frame(1, 0, "Game saved!")]);
    assert!(exchange(&mut direct_auth, "synthetic-secret").is_ok());
}

#[test]
fn humanitz_save_rejects_wrong_auth_and_never_sends_save_before_auth() {
    for packets in [
        vec![frame(-1, 2, "")],
        vec![frame(1, 2, "")],
        vec![frame(0, 3, "")],
        vec![frame(0, 0, ""), frame(0, 0, "")],
        vec![frame(0, 0, ""), frame(14001, 2, "")],
    ] {
        let mut io = Scripted::new(packets);
        assert!(exchange(&mut io, "synthetic-secret").is_err());
        assert_eq!(io.packets(), [(0, 3, "synthetic-secret".into())]);
    }
}

#[test]
fn humanitz_save_rejects_any_other_ack_and_never_replays_the_command() {
    for packet in [
        frame(0, 0, "Game saved!"),
        frame(14002, 0, "Game saved!"),
        frame(1, 2, "Game saved!"),
        frame(-1, 0, "Game saved!"),
        frame(1, 0, ""),
        frame(1, 0, "Game saved!\n"),
        frame(1, 0, "Saving game"),
        frame(1, 0, "Game saved! extra"),
    ] {
        let mut io = Scripted::new(vec![frame(0, 2, ""), packet]);
        assert!(exchange(&mut io, "synthetic-secret").is_err());
        assert_eq!(
            io.packets(),
            [(0, 3, "synthetic-secret".into()), (0, 2, "save".into())]
        );
    }
}

#[test]
fn humanitz_save_eof_timeout_oversize_and_partial_frames_are_unconfirmed_without_retry() {
    let mut partial = frame(1, 0, "Game saved!");
    partial.pop();
    for tail in [Vec::new(), partial, 65547i32.to_le_bytes().to_vec()] {
        for fault in [ErrorKind::UnexpectedEof, ErrorKind::TimedOut] {
            let mut io = Scripted::new(vec![frame(0, 2, ""), tail.clone()]);
            io.read_fault = fault;
            assert!(
                exchange(&mut io, "synthetic-secret")
                    .unwrap_err()
                    .contains("not retried")
            );
            assert_eq!(io.packets().len(), 2);
        }
    }
}

#[test]
fn humanitz_save_partial_command_write_is_not_retried() {
    let auth = frame(0, 3, "synthetic-secret");
    let mut io = Scripted::new(vec![frame(0, 2, "")]);
    io.command_write_fault = true;
    assert!(
        exchange(&mut io, "synthetic-secret")
            .unwrap_err()
            .contains("not retried")
    );
    assert_eq!(io.writes, 2);
    assert_eq!(io.output.len(), auth.len() + 5);
}

#[test]
fn humanitz_save_rejects_other_commands_and_invalid_secrets_before_connecting() {
    for command in ["", "Save", "info", "save\nshutdown", "shutdown"] {
        assert!(
            humanitz_rcon_save("invalid-endpoint", "synthetic-secret", command)
                .unwrap_err()
                .contains("only supports")
        );
    }
    for secret in ["".into(), "a\0b".into(), "a".repeat(4097)] {
        assert!(
            humanitz_rcon_save("invalid-endpoint", &secret, "save")
                .unwrap_err()
                .contains("password")
        );
    }
}
