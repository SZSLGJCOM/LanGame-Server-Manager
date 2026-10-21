use super::*;
use std::io::Cursor;

struct DeliveryPeer {
    replies: Cursor<Vec<u8>>,
    writes: usize,
    partial_command: bool,
}

impl Read for DeliveryPeer {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.replies.position() as usize == self.replies.get_ref().len() {
            return Err(std::io::Error::new(
                ErrorKind::TimedOut,
                "fixture response timeout",
            ));
        }
        self.replies.read(buffer)
    }
}

impl Write for DeliveryPeer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.writes += 1;
        if self.partial_command && self.writes == 2 {
            return Ok(5.min(bytes.len()));
        }
        if self.partial_command && self.writes == 3 {
            return Err(std::io::Error::new(
                ErrorKind::BrokenPipe,
                "fixture partial command write",
            ));
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn rcon_delivery_stage_distinguishes_auth_failure_partial_write_and_response_timeout() {
    for conan in [false, true] {
        for (auth_id, partial, expected_attempted) in [
            (-1, false, false),
            (14001, true, true),
            (14001, false, true),
        ] {
            let mut auth = Vec::new();
            source_rcon_write_packet(
                &mut auth,
                auth_id,
                2,
                if conan { "Authenticated." } else { "" },
            )
            .unwrap();
            let mut peer = DeliveryPeer {
                replies: Cursor::new(auth),
                writes: 0,
                partial_command: partial,
            };
            let mut attempted = false;
            let error = if conan {
                conan::exchange_with_delivery(
                    &mut peer,
                    "fixture",
                    "shutdown",
                    "fixture-boundary",
                    &mut attempted,
                )
            } else {
                source_rcon_exchange_with_delivery(
                    &mut peer,
                    "fixture",
                    "quit",
                    SourceRconCompletion::ResponseValue,
                    &mut attempted,
                )
            }
            .unwrap_err();
            assert_eq!(
                attempted, expected_attempted,
                "conan={conan}, partial={partial}: {error}"
            );
            if partial {
                assert!(error.contains("fixture partial command write"), "{error}");
                assert_eq!(
                    peer.writes, 3,
                    "the command frame was actually partially written"
                );
            } else if expected_attempted {
                assert!(error.contains("fixture response timeout"), "{error}");
            } else {
                assert_eq!(
                    peer.writes, 1,
                    "authentication failure must never write the command"
                );
            }
        }
    }
}

#[test]
fn ark_shutdown_first_ack_preserves_auth_rejection_partial_write_and_unknown_delivery() {
    for (auth_id, partial, expected_attempted) in [
        (-1, false, false),
        (14001, true, true),
        (14001, false, true),
    ] {
        let mut replies = Vec::new();
        source_rcon_write_packet(&mut replies, auth_id, 2, "").unwrap();
        let mut peer = DeliveryPeer {
            replies: Cursor::new(replies),
            writes: 0,
            partial_command: partial,
        };
        let mut attempted = false;
        let result = source_rcon_exchange_with_delivery(
            &mut peer,
            "fixture",
            "DoExit",
            SourceRconCompletion::ArkShutdownFirstResponse,
            &mut attempted,
        );
        assert!(result.is_err());
        assert_eq!(attempted, expected_attempted);
        assert_eq!(
            peer.writes,
            if partial {
                3
            } else if expected_attempted {
                2
            } else {
                1
            }
        );
    }
    for (id, kind, body) in [(-1, 0, ""), (14002, 2, ""), (14003, 0, "unrelated")] {
        let mut replies = Vec::new();
        source_rcon_write_packet(&mut replies, 14001, 2, "").unwrap();
        source_rcon_write_packet(&mut replies, id, kind, body).unwrap();
        let mut peer = DeliveryPeer {
            replies: Cursor::new(replies),
            writes: 0,
            partial_command: false,
        };
        let mut attempted = false;
        assert!(
            source_rcon_exchange_with_delivery(
                &mut peer,
                "fixture",
                "DoExit",
                SourceRconCompletion::ArkShutdownFirstResponse,
                &mut attempted
            )
            .is_err()
        );
        assert!(attempted);
        assert_eq!(
            peer.writes, 2,
            "rejected/malformed/unknown delivery cannot send a marker or retry"
        );
    }
}

#[test]
fn ark_shutdown_first_ack_body_is_receipt_only_and_never_exit_confirmation() {
    let mut replies = Vec::new();
    source_rcon_write_packet(&mut replies, 14001, 2, "").unwrap();
    source_rcon_write_packet(&mut replies, 14002, 0, "Unknown command!").unwrap();
    let mut peer = DeliveryPeer {
        replies: Cursor::new(replies),
        writes: 0,
        partial_command: false,
    };
    let mut attempted = false;
    let result = source_rcon_exchange_with_delivery(
        &mut peer,
        "fixture",
        "DoExit",
        SourceRconCompletion::ArkShutdownFirstResponse,
        &mut attempted,
    )
    .unwrap();
    assert_eq!(result, "Unknown command!");
    assert!(attempted);
    assert_eq!(peer.writes, 2);
    // Production lifecycle separately requires the retained whole-tree owner
    // to prove exit. A response body cannot bypass that condition.
}

// A real TCP peer exposes a post-DoExit packet; a byte-vector mock cannot prove
// that the production socket is actually closed after the native first ACK.
fn shutdown_loopback_peer(command: &str) -> (String, std::thread::JoinHandle<bool>) {
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let command = command.to_owned();
    let peer = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let auth = source_rcon_read_packet(&mut socket).unwrap();
        assert_eq!(
            (auth.id, auth.packet_type, auth.body.as_slice()),
            (14001, 3, b"fixture".as_slice())
        );
        source_rcon_write_packet(&mut socket, 14001, 2, "").unwrap();
        let exec = source_rcon_read_packet(&mut socket).unwrap();
        assert_eq!((exec.id, exec.packet_type), (14002, 2));
        assert_eq!(exec.body, command.as_bytes());
        source_rcon_write_packet(&mut socket, 14002, 0, "Exiting...\r\n").unwrap();
        let mut byte = [0];
        match socket.read(&mut byte).unwrap() {
            0 => false,
            _ => {
                // Let the old boundary implementation finish without waiting
                // out its deadline, then fail on its unwanted third frame.
                let mut header = [0; 4];
                header[0] = byte[0];
                socket.read_exact(&mut header[1..]).unwrap();
                let size = i32::from_le_bytes(header);
                assert!((10..=16394).contains(&size));
                let mut payload = vec![0; size as usize];
                socket.read_exact(&mut payload).unwrap();
                assert_eq!(i32::from_le_bytes(payload[..4].try_into().unwrap()), 14003);
                source_rcon_write_packet(&mut socket, 14003, 0, "").unwrap();
                true
            }
        }
    });
    (endpoint, peer)
}

#[test]
fn ark_shutdown_doexit_closes_real_socket_after_first_ack_without_third_frame() {
    for module in ["arksurvivalevolved", "arksurvivalascended"] {
        let (endpoint, peer) = shutdown_loopback_peer("DoExit");
        let response = source_rcon_shutdown_exec(module, &endpoint, "fixture", "DoExit").unwrap();
        assert_eq!(response, "Exiting...\r\n");
        assert!(
            !peer.join().unwrap(),
            "{module}: no command or boundary may follow DoExit's first ACK"
        );
    }
}

#[test]
fn ark_shutdown_first_ack_selection_never_changes_other_commands_or_games() {
    for (module, command) in [
        ("arksurvivalevolved", "saveworld"),
        ("arksurvivalascended", "ListPlayers"),
        ("arksurvivalascended", "doexit"),
        ("arksurvivalascended", "DoExit extra"),
        ("minecraft", "DoExit"),
        ("squad", "DoExit"),
    ] {
        let (endpoint, peer) = shutdown_loopback_peer(command);
        source_rcon_shutdown_exec(module, &endpoint, "fixture", command).unwrap();
        assert!(
            peer.join().unwrap(),
            "{module}/{command}: ordinary command still requires its boundary"
        );
    }
}
