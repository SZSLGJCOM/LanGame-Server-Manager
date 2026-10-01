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
