use super::*;
use std::io::{Cursor, Error};

fn response_packet(id: i32, packet_type: i32, body: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    source_rcon_write_packet(&mut bytes, id, packet_type, body).unwrap();
    bytes
}

fn response_fragment(id: i32, body: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(10_i32 + body.len() as i32).to_le_bytes());
    bytes.extend_from_slice(&id.to_le_bytes());
    bytes.extend_from_slice(&0_i32.to_le_bytes());
    bytes.extend_from_slice(body);
    bytes.extend_from_slice(&[0, 0]);
    bytes
}

struct ProtocolPeer {
    replies: Cursor<Vec<u8>>,
    earliest_request_offsets: Vec<u64>,
    requests: Vec<SourceRconPacket>,
}

impl Read for ProtocolPeer {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.replies.read(buffer)
    }
}

impl Write for ProtocolPeer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() < 14
            || i32::from_le_bytes(bytes[..4].try_into().unwrap()) != (bytes.len() - 4) as i32
        {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "request frame was split",
            ));
        }
        let expected_offset = self
            .earliest_request_offsets
            .get(self.requests.len())
            .ok_or_else(|| Error::other("unexpected additional request"))?;
        if self.replies.position() < *expected_offset {
            return Err(Error::other("request sent before the preceding response"));
        }
        let packet = source_rcon_read_packet(&mut Cursor::new(bytes)).map_err(Error::other)?;
        self.requests.push(packet);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn source_rcon_exchange_serializes_minecraft_requests_and_preserves_all_response_fragments() {
    let auth = response_packet(14001, 2, "");
    let first = response_packet(14002, 0, "page one\n");
    let second = response_packet(14002, 0, "page two");
    let marker = response_packet(14003, 0, "Unknown request 0");
    let mut peer = ProtocolPeer {
        earliest_request_offsets: vec![0, auth.len() as u64, (auth.len() + first.len()) as u64],
        replies: Cursor::new([auth, first, second, marker].concat()),
        requests: Vec::new(),
    };

    let response = source_rcon_exchange(
        &mut peer,
        "test-password",
        "list",
        SourceRconCompletion::ResponseValue,
    )
    .unwrap();

    assert_eq!(response, "page one\npage two");
    assert_eq!(peer.requests.len(), 3);
    assert_eq!(peer.requests[0].packet_type, 3);
    assert_eq!(peer.requests[0].body, b"test-password");
    assert_eq!(peer.requests[1].packet_type, 2);
    assert_eq!(peer.requests[1].body, b"list");
    assert_eq!(peer.requests[2].id, 14003);
    assert_eq!(peer.requests[2].packet_type, 0);
}

#[test]
fn source_rcon_exchange_waits_for_source_auth_type_after_empty_value_packet() {
    let empty_value = response_packet(14001, 0, "");
    let auth = response_packet(14001, 2, "");
    let first = response_packet(14002, 0, "");
    let marker = response_packet(14003, 0, "");
    let auth_bytes = empty_value.len() + auth.len();
    let mut peer = ProtocolPeer {
        earliest_request_offsets: vec![0, auth_bytes as u64, (auth_bytes + first.len()) as u64],
        replies: Cursor::new([empty_value, auth, first, marker].concat()),
        requests: Vec::new(),
    };

    assert_eq!(
        source_rcon_exchange(
            &mut peer,
            "test-password",
            "status",
            SourceRconCompletion::ResponseValue
        )
        .unwrap(),
        ""
    );
    assert_eq!(peer.requests.len(), 3);
}

#[test]
fn source_rcon_exchange_never_executes_after_authentication_rejection() {
    let mut peer = ProtocolPeer {
        earliest_request_offsets: vec![0],
        replies: Cursor::new([response_packet(14001, 0, ""), response_packet(-1, 2, "")].concat()),
        requests: Vec::new(),
    };
    let error = source_rcon_exchange(
        &mut peer,
        "test-password",
        "list",
        SourceRconCompletion::ResponseValue,
    )
    .unwrap_err();

    assert_eq!(error, "RCON authentication failed");
    assert_eq!(peer.requests.len(), 1);
}

#[test]
fn source_rcon_reader_accepts_minecraft_4096_character_response_chunks() {
    for body in ["x".repeat(4096), "中".repeat(4096)] {
        let bytes = response_packet(14002, 0, &body);
        let packet = source_rcon_read_packet(&mut Cursor::new(bytes)).unwrap();
        assert_eq!(packet.body, body.as_bytes());
    }
}

#[test]
fn source_rcon_reader_rejects_oversized_and_incomplete_frames() {
    for size in [9_i32, 4096 * 4 + 11, i32::MAX] {
        let error = source_rcon_read_packet(&mut Cursor::new(size.to_le_bytes()))
            .err()
            .expect("invalid frame size must fail");
        assert!(error.contains("invalid RCON packet size"));
    }
    let mut bytes = response_packet(14002, 0, "truncated");
    bytes.pop();
    let error = source_rcon_read_packet(&mut Cursor::new(bytes))
        .err()
        .expect("incomplete response must fail");
    assert!(error.contains("failed to read RCON packet body"));
}

#[test]
fn source_rcon_reader_requires_both_nul_terminators() {
    for terminators in [[0, 1], [1, 0], [1, 1]] {
        let mut bytes = response_packet(14002, 0, "Alice");
        let ending = bytes.len() - 2;
        bytes[ending..].copy_from_slice(&terminators);
        let error = source_rcon_read_packet(&mut Cursor::new(bytes))
            .err()
            .unwrap();
        assert!(error.contains("double-NUL terminator"));
    }
}

#[test]
fn source_rcon_exchange_decodes_utf8_only_after_joining_all_response_frames() {
    let expected = "Players connected (1): 玩家";
    let body = expected.as_bytes();
    let split = body.len() - 2;
    let auth = response_packet(14001, 2, "");
    let first = response_fragment(14002, &body[..split]);
    let second = response_fragment(14002, &body[split..]);
    let marker = response_packet(14003, 0, "");
    let mut peer = ProtocolPeer {
        earliest_request_offsets: vec![0, auth.len() as u64, (auth.len() + first.len()) as u64],
        replies: Cursor::new([auth, first, second, marker].concat()),
        requests: Vec::new(),
    };
    assert_eq!(
        source_rcon_exchange(
            &mut peer,
            "",
            "players",
            SourceRconCompletion::ResponseValue
        )
        .unwrap(),
        expected
    );
}

#[test]
fn source_rcon_response_rejects_invalid_and_unfinished_utf8_without_replacement() {
    for body in [b"Alice\xff".as_slice(), b"Alice\xe7\x8e".as_slice()] {
        let mut bytes = response_fragment(14002, body);
        bytes.extend(response_packet(14003, 0, ""));
        let error =
            source_rcon_read_command_response(&mut Cursor::new(bytes), 14002, 14003, Vec::new())
                .unwrap_err();
        assert_eq!(error, "RCON command response contains invalid UTF-8");

        let auth = response_packet(14001, 2, "");
        let first = response_fragment(14002, body);
        let marker = response_packet(14003, 0, "");
        let mut peer = ProtocolPeer {
            earliest_request_offsets: vec![0, auth.len() as u64, (auth.len() + first.len()) as u64],
            replies: Cursor::new([auth, first, marker].concat()),
            requests: Vec::new(),
        };
        assert_eq!(
            source_rcon_exchange(
                &mut peer,
                "",
                "players",
                SourceRconCompletion::ResponseValue
            )
            .unwrap_err(),
            "RCON command response contains invalid UTF-8"
        );
    }
}

#[test]
fn source_rcon_binary_marker_is_not_decoded_as_player_response_text() {
    // Source's terminator marker can contain binary bytes; only command payloads
    // become text. Their strict decoding must not reject or append marker data.
    let marker = response_fragment(14003, &[0, 1, 0, 0]);
    let response = source_rcon_read_command_response(
        &mut Cursor::new(marker),
        14002,
        14003,
        "玩家".as_bytes().to_vec(),
    )
    .unwrap();
    assert_eq!(response, "玩家");
}

fn ark_peer(response_frames: Vec<Vec<u8>>) -> ProtocolPeer {
    let auth = response_packet(14001, 2, "");
    let first_length = response_frames.first().map_or(0, Vec::len);
    ProtocolPeer {
        earliest_request_offsets: vec![0, auth.len() as u64, (auth.len() + first_length) as u64],
        replies: Cursor::new([vec![auth], response_frames].concat().concat()),
        requests: Vec::new(),
    }
}

#[test]
fn ark_rcon_completion_preserves_large_fragmented_response_until_read_only_reply() {
    let response = format!("{}玩家", "a".repeat(12_287));
    let fragments = response.as_bytes().chunks(4096).collect::<Vec<_>>();
    let mut frames = vec![response_fragment(14002, fragments[0])];
    // Unrelated IDs never finish a response or become part of the command text.
    frames.push(response_packet(15000, 0, "unrelated"));
    frames.extend(
        fragments[1..]
            .iter()
            .map(|body| response_fragment(14002, body)),
    );
    frames.push(response_packet(14003, 0, "No Players Connected \n "));
    let mut peer = ark_peer(frames);

    assert_eq!(
        source_rcon_exchange(
            &mut peer,
            "fixture-password",
            "SaveWorld",
            SourceRconCompletion::PlayerList
        )
        .unwrap(),
        response
    );
    assert_eq!(peer.requests.len(), 3);
    assert_eq!(peer.requests[1].body, b"SaveWorld");
    assert_eq!(peer.requests[2].id, 14003);
    assert_eq!(peer.requests[2].packet_type, 2);
    assert_eq!(peer.requests[2].body, b"ListPlayers");
}

#[test]
fn ark_rcon_completion_rejects_wrong_types_in_first_fragment_later_fragment_and_boundary() {
    for frames in [
        vec![response_packet(14002, 2, "")],
        vec![
            response_packet(14002, 0, "first"),
            response_packet(14002, 2, "later"),
        ],
        vec![
            response_packet(14002, 0, "first"),
            response_packet(14003, 2, ""),
        ],
    ] {
        let mut peer = ark_peer(frames);
        let error = source_rcon_exchange(
            &mut peer,
            "fixture-password",
            "ListPlayers",
            SourceRconCompletion::PlayerList,
        )
        .unwrap_err();
        assert!(error.contains("invalid response type"));
    }
}

#[test]
fn ark_rcon_completion_rejects_truncated_frames_and_missing_boundary() {
    let first = response_packet(14002, 0, "partial");
    let mut truncated = response_packet(14002, 0, "second");
    truncated.pop();
    for frames in [vec![first.clone()], vec![first, truncated]] {
        let mut peer = ark_peer(frames);
        let error = source_rcon_exchange(
            &mut peer,
            "fixture-password",
            "ListPlayers",
            SourceRconCompletion::PlayerList,
        )
        .unwrap_err();
        assert!(error.contains("failed to read RCON packet"));
    }
}

struct TimedOutPeer(ProtocolPeer);

impl Read for TimedOutPeer {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.0.replies.position() as usize == self.0.replies.get_ref().len() {
            return Err(Error::new(
                ErrorKind::TimedOut,
                "fixture response timed out",
            ));
        }
        self.0.read(buffer)
    }
}

impl Write for TimedOutPeer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.write(bytes)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

fn assert_rcon_timeout_stage(replies: Vec<Vec<u8>>, phase: &str, request_count: usize) {
    let mut offset = 0;
    let mut earliest_request_offsets = vec![0];
    for reply in &replies {
        offset += reply.len() as u64;
        earliest_request_offsets.push(offset);
    }
    let mut peer = TimedOutPeer(ProtocolPeer {
        replies: Cursor::new(replies.concat()),
        earliest_request_offsets,
        requests: Vec::new(),
    });
    let error = source_rcon_exchange(
        &mut peer,
        "fixture-password-not-for-diagnostics",
        "ListPlayers",
        SourceRconCompletion::PlayerList,
    )
    .unwrap_err();
    assert!(error.starts_with(phase), "{error}");
    assert!(error.contains("failed to read RCON packet size"));
    assert!(error.contains("fixture response timed out"));
    assert!(!error.contains("fixture-password-not-for-diagnostics"));
    assert_eq!(peer.0.requests.len(), request_count);
}

#[test]
fn ark_rcon_authentication_timeout_identifies_stage_without_sending_command() {
    assert_rcon_timeout_stage(Vec::new(), "RCON authentication read failed:", 1);
}

#[test]
fn ark_rcon_command_timeout_identifies_stage_without_sending_completion() {
    assert_rcon_timeout_stage(
        vec![response_packet(14001, 2, "")],
        "RCON command response read failed:",
        2,
    );
}

#[test]
fn ark_rcon_completion_timeout_identifies_stage_without_repeating_command() {
    assert_rcon_timeout_stage(
        vec![
            response_packet(14001, 2, ""),
            response_packet(14002, 0, "No Players Connected"),
        ],
        "RCON completion read failed:",
        3,
    );
}

#[test]
fn ark_rcon_completion_never_treats_timeout_after_a_fragment_as_success() {
    let mut peer = TimedOutPeer(ark_peer(vec![response_packet(14002, 0, "partial")]));
    let error = source_rcon_exchange(
        &mut peer,
        "fixture-password",
        "ListPlayers",
        SourceRconCompletion::PlayerList,
    )
    .unwrap_err();
    assert!(error.contains("fixture response timed out"));
    assert_eq!(peer.0.requests.len(), 3);
}

#[test]
fn ark_rcon_completion_is_bounded_when_only_unrelated_ids_arrive() {
    let mut frames = vec![response_packet(14002, 0, "partial")];
    frames.extend((0..64).map(|_| response_packet(15000, 0, "unrelated")));
    let mut peer = ark_peer(frames);
    let error = source_rcon_exchange(
        &mut peer,
        "fixture-password",
        "ListPlayers",
        SourceRconCompletion::PlayerList,
    )
    .unwrap_err();
    assert_eq!(error, "RCON command response terminator was not received");
}

#[test]
fn ark_rcon_authentication_failure_sends_neither_command_nor_completion_request() {
    let mut peer = ProtocolPeer {
        earliest_request_offsets: vec![0],
        replies: Cursor::new(response_packet(-1, 2, "")),
        requests: Vec::new(),
    };
    let error = source_rcon_exchange(
        &mut peer,
        "fixture-password",
        "SaveWorld",
        SourceRconCompletion::PlayerList,
    )
    .unwrap_err();
    assert_eq!(error, "RCON authentication failed");
    assert_eq!(peer.requests.len(), 1);
}
