use std::io::{Cursor, Error, ErrorKind};

use super::*;

const MARKER: &str = "LgsmBoundaryFixture";

struct Peer {
    replies: Cursor<Vec<u8>>,
    requests: Vec<u8>,
}

impl Read for Peer {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.replies.position() as usize == self.replies.get_ref().len() {
            return Err(Error::new(
                ErrorKind::TimedOut,
                "fixture response timed out",
            ));
        }
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

fn packet(id: i32, packet_type: i32, body: &[u8]) -> Vec<u8> {
    let mut frame = Vec::new();
    frame.extend_from_slice(&(10_i32 + body.len() as i32).to_le_bytes());
    frame.extend_from_slice(&id.to_le_bytes());
    frame.extend_from_slice(&packet_type.to_le_bytes());
    frame.extend_from_slice(body);
    frame.extend_from_slice(&[0, 0]);
    frame
}

fn peer(frames: Vec<Vec<u8>>) -> Peer {
    Peer {
        replies: Cursor::new(frames.concat()),
        requests: Vec::new(),
    }
}

#[test]
fn conan_rcon_collects_fragmented_type_two_replies_until_the_unique_help_boundary() {
    let expected = "Idx | Char name | Player name | User ID | Platform ID | Platform Name\n玩家";
    let bytes = expected.as_bytes();
    let split = bytes.len() - 2;
    let boundary = format!("Commands matching search string: {MARKER}");
    let mut peer = peer(vec![
        packet(0, 2, b"Authenticated."),
        packet(14001, 2, &bytes[..split]),
        packet(14001, 2, &bytes[split..]),
        packet(14002, 2, &boundary.as_bytes()[..20]),
        packet(14002, 2, &boundary.as_bytes()[20..]),
    ]);
    assert_eq!(
        exchange(&mut peer, "fixture", "listplayers", MARKER).unwrap(),
        expected
    );
    let mut requests = Cursor::new(peer.requests);
    assert_eq!(
        source_rcon_read_packet(&mut requests).unwrap().packet_type,
        3
    );
    assert_eq!(
        source_rcon_read_packet(&mut requests).unwrap().body,
        b"listplayers"
    );
    assert_eq!(
        source_rcon_read_packet(&mut requests).unwrap().body,
        format!("help {MARKER}").as_bytes()
    );
    assert_eq!(requests.position() as usize, requests.get_ref().len());
}

#[test]
fn conan_rcon_requires_explicit_authentication_before_sending_a_command() {
    for frame in [
        packet(-1, 2, b"Authenticated."),
        packet(0, 0, b"Authenticated."),
        packet(0, 2, b"Authentication failed"),
    ] {
        let mut peer = peer(vec![frame]);
        assert!(exchange(&mut peer, "fixture", "listplayers", MARKER).is_err());
        let mut requests = Cursor::new(peer.requests);
        assert_eq!(
            source_rcon_read_packet(&mut requests).unwrap().packet_type,
            3
        );
        assert_eq!(requests.position() as usize, requests.get_ref().len());
    }
}

#[test]
fn conan_rcon_never_accepts_timeout_or_another_help_response_as_completion() {
    for body in [
        b"partial".as_slice(),
        b"Commands matching search string: another-request",
    ] {
        let mut peer = peer(vec![
            packet(0, 2, b"Authenticated."),
            packet(14001, 2, body),
        ]);
        let error = exchange(&mut peer, "fixture", "listplayers", MARKER).unwrap_err();
        assert!(error.contains("fixture response timed out"));
    }
}

#[test]
fn conan_rcon_rejects_invalid_types_utf8_and_unbounded_response_sequences() {
    let boundary = format!("Commands matching search string: {MARKER}");
    for frames in [
        vec![packet(14001, 0, b"unexpected")],
        vec![packet(-1, 2, b"rejected")],
        vec![
            packet(14001, 2, b"invalid\xff"),
            packet(14002, 2, boundary.as_bytes()),
        ],
        (0..64).map(|_| packet(14001, 2, b"more")).collect(),
    ] {
        let mut replies = vec![packet(0, 2, b"Authenticated.")];
        replies.extend(frames);
        assert!(exchange(&mut peer(replies), "fixture", "listplayers", MARKER).is_err());
    }
}
