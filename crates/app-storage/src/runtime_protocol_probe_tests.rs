use super::*;
use std::cell::Cell;

fn info_reply() -> Vec<u8> {
    let mut reply = b"\xff\xff\xff\xffI\x11Fixture\0Map\0GameFolder\0Game\0".to_vec();
    reply.extend_from_slice(&346110_u32.to_le_bytes()[..2]);
    reply.extend_from_slice(&[0, 20, 0, b'd', b'w', 0, 1]);
    reply.extend_from_slice(b"1.0.0\0");
    reply
}

fn extra_fields(reply: &mut Vec<u8>, edf: u8) {
    reply.push(edf);
    if edf & 0x80 != 0 {
        reply.extend_from_slice(&7777_u16.to_le_bytes());
    }
    if edf & 0x10 != 0 {
        reply.extend_from_slice(&123_u64.to_le_bytes());
    }
    if edf & 0x40 != 0 {
        reply.extend_from_slice(&1234_u16.to_le_bytes());
        reply.extend_from_slice(b"Spectator\0");
    }
    if edf & 0x20 != 0 {
        reply.extend_from_slice(b"tag1,tag2\0");
    }
    if edf & 0x01 != 0 {
        reply.extend_from_slice(&346110_u64.to_le_bytes());
    }
}

#[test]
fn a2s_ready_requires_the_entire_mandatory_info_packet() {
    let complete = info_reply();
    assert!(complete_a2s_info(&complete));
    for length in 0..complete.len() {
        assert!(
            !complete_a2s_info(&complete[..length]),
            "truncated at {length}"
        );
    }
    let mut wrong_type = complete.clone();
    wrong_type[4] = b'D';
    assert!(!complete_a2s_info(&wrong_type));
    let mut oversize = complete;
    oversize.resize(A2S_MAX_RESPONSE_BYTES + 1, 0);
    assert!(!complete_a2s_info(&oversize));
}

#[test]
fn a2s_ready_rejects_incomplete_advertised_optional_fields() {
    for edf in 0_u8..=u8::MAX {
        if edf & !0xf1 != 0 {
            continue;
        }
        let mut complete = info_reply();
        let base_length = complete.len();
        extra_fields(&mut complete, edf);
        assert!(complete_a2s_info(&complete), "EDF {edf:#x}");
        for length in base_length + 1..complete.len() {
            assert!(
                !complete_a2s_info(&complete[..length]),
                "EDF {edf:#x}, truncated at {length}"
            );
        }
    }
}

#[test]
fn a2s_ready_rejects_unknown_flags_and_unparsed_trailing_data() {
    for edf in [2, 4, 8] {
        let mut reply = info_reply();
        reply.push(edf);
        assert!(!complete_a2s_info(&reply));
    }
    let mut reply = info_reply();
    extra_fields(&mut reply, 0xf1);
    reply.push(0);
    assert!(!complete_a2s_info(&reply));
}

fn endpoint(address: &str, pid: u32) -> EndpointOwner {
    EndpointOwner {
        address: address.parse().unwrap(),
        port: 27015,
        pid,
    }
}

#[test]
fn only_the_current_process_local_endpoint_can_satisfy_readiness() {
    let target = "127.0.0.1:27015".parse().unwrap();
    assert!(!owns_endpoint(&[], 42, target).unwrap());
    assert!(owns_endpoint(&[endpoint("127.0.0.1", 42)], 42, target).unwrap());
    assert!(owns_endpoint(&[endpoint("0.0.0.0", 42)], 42, target).unwrap());
    assert!(!owns_endpoint(&[endpoint("192.0.2.1", 42)], 42, target).unwrap());
    for pid in [0, 43] {
        assert!(owns_endpoint(&[endpoint("127.0.0.1", pid)], 42, target).is_err());
        assert!(
            owns_endpoint(
                &[endpoint("127.0.0.1", 42), endpoint("0.0.0.0", pid)],
                42,
                target,
            )
            .is_err()
        );
    }
    assert!(
        owns_endpoint(
            &[endpoint("127.0.0.1", 42), endpoint("192.0.2.1", 43)],
            42,
            target,
        )
        .unwrap()
    );
}

#[test]
fn foreign_possible_dual_stack_owners_are_not_ignored() {
    let target = "127.0.0.1:27015".parse().unwrap();
    assert!(owns_endpoint(&[endpoint("127.0.0.1", 42), endpoint("::", 43)], 42, target,).is_err());
    assert!(!owns_endpoint(&[endpoint("::", 42)], 42, target).unwrap());
    assert!(owns_endpoint(&[endpoint("::ffff:127.0.0.1", 42)], 42, target).unwrap());
    assert!(owns_endpoint(&[endpoint("::", 42)], 42, "[::1]:27015".parse().unwrap()).unwrap());
}

#[test]
fn a_missing_endpoint_does_not_send_any_query() {
    let queried = Cell::new(false);
    assert!(
        !poll_owned(
            || Ok(false),
            || {
                queried.set(true);
                Ok(true)
            }
        )
        .unwrap()
    );
    assert!(!queried.get());
    assert!(
        poll_owned(
            || Err("foreign process".into()),
            || {
                queried.set(true);
                Ok(true)
            }
        )
        .is_err()
    );
    assert!(!queried.get());
}

#[test]
fn completed_queries_cannot_bypass_the_second_identity_and_endpoint_check() {
    for ready in [false, true] {
        let observations = Cell::new(0);
        let result = poll_owned(
            || {
                observations.set(observations.get() + 1);
                if observations.get() == 1 {
                    Ok(true)
                } else {
                    Err("identity changed".into())
                }
            },
            || Ok(ready),
        );
        assert_eq!(result, Err("identity changed".into()));
        assert_eq!(observations.get(), 2);
    }
    let observations = Cell::new(0);
    assert!(
        !poll_owned(
            || {
                observations.set(observations.get() + 1);
                Ok(observations.get() == 1)
            },
            || Ok(true)
        )
        .unwrap()
    );
    assert_eq!(observations.get(), 2);
}

#[test]
fn a_query_ownership_failure_is_fatal_even_if_the_endpoint_then_disappears() {
    let observations = Cell::new(0);
    let result = poll_owned(
        || {
            observations.set(observations.get() + 1);
            Ok(observations.get() == 1)
        },
        || Err("foreign connected peer".into()),
    );
    assert_eq!(result, Err("foreign connected peer".into()));
}

#[cfg(windows)]
fn current_target() -> ProcessProbeTarget {
    ProcessProbeTarget {
        pid: std::process::id(),
        identity: app_runtime::inspect_process_identity(std::process::id())
            .unwrap()
            .unwrap(),
        host: "127.0.0.1".into(),
    }
}

#[cfg(windows)]
#[test]
fn a2s_probe_uses_a_real_owned_socket_and_rejects_partial_info() {
    for complete in [true, false] {
        let listener = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let server = listener.try_clone().unwrap();
        let worker = std::thread::spawn(move || {
            let mut request = [0; 128];
            let (length, address) = server.recv_from(&mut request).unwrap();
            assert_eq!(
                &request[..length],
                b"\xff\xff\xff\xffTSource Engine Query\0"
            );
            let mut reply = info_reply();
            if !complete {
                reply.truncate(reply.len() - 1);
            }
            server.send_to(&reply, address).unwrap();
        });
        let port = listener.local_addr().unwrap().port();
        assert_eq!(
            poll_owned_a2s_info(&current_target(), port).unwrap(),
            complete
        );
        worker.join().unwrap();
        // Keep the owner alive through the post-query ownership observation.
        drop(listener);
    }
}

#[cfg(windows)]
#[test]
fn invalid_or_reused_process_identity_is_rejected_before_querying() {
    let listener = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut target = current_target();
    target.identity.creation_time += 1;
    let port = listener.local_addr().unwrap().port();
    assert!(poll_owned_a2s_info(&target, port).is_err());
    let mut request = [0; 128];
    assert_eq!(
        listener.recv_from(&mut request).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}
