use super::*;
use std::io;
use std::os::windows::process::ExitStatusExt;
use std::process::{ExitStatus, Output};

fn query_output(status: u32, stdout: &[u8]) -> io::Result<Output> {
    Ok(Output {
        status: ExitStatus::from_raw(status),
        stdout: stdout.to_vec(),
        stderr: b"private adapter details".to_vec(),
    })
}

#[test]
fn bind_address_query_failures_do_not_become_an_empty_inventory() {
    for (query, expected) in [
        (
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "private adapter details",
            )),
            "Querying active IPv4 network adapters timed out.",
        ),
        (
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "private adapter details",
            )),
            "Could not query active IPv4 network adapters.",
        ),
        (
            query_output(1, b"[]"),
            "Could not query active IPv4 network adapters.",
        ),
    ] {
        let error = net_ip_configuration_query_records(query).unwrap_err();
        assert_eq!(error, expected);
        assert!(!error.contains("private adapter details"));
    }
}

#[test]
fn bind_address_query_accepts_explicit_empty_array_but_rejects_malformed_output() {
    for payload in [b"[]".as_slice(), b" \r\n[]\r\n"] {
        let records = net_ip_configuration_query_records(query_output(0, payload)).unwrap();
        assert!(build_bind_address_candidates(records).is_empty());
    }
    for payload in [
        b"".as_slice(),
        b" \r\n",
        b"null",
        b"{}",
        b"true",
        b"private adapter details",
        br#"[{"InterfaceAlias":"private adapter details"}]"#,
        br#"[{"IPAddress":123,"InterfaceAlias":"private adapter details"}]"#,
    ] {
        let error = net_ip_configuration_query_records(query_output(0, payload)).unwrap_err();
        assert_eq!(
            error,
            "The active IPv4 network adapter query returned invalid data."
        );
        assert!(!error.contains("private adapter details"));
    }
}

#[test]
fn bind_address_query_keeps_canonical_radmin_identity_and_existing_filters() {
    let records = net_ip_configuration_query_records(query_output(
        0,
        br#"[
          {"IPAddress":"26.1.2.3","InterfaceAlias":"Radmin VPN","InterfaceDescription":"Famatech Radmin VPN Ethernet Adapter"},
          {"IPAddress":"26.1.2.3","InterfaceAlias":"Radmin VPN","InterfaceDescription":"Famatech Radmin VPN Ethernet Adapter"},
          {"IPAddress":"192.168.1.2","InterfaceAlias":"Ethernet","InterfaceDescription":"Ethernet Adapter"},
          {"IPAddress":"127.0.0.1","InterfaceAlias":"Loopback","InterfaceDescription":"Loopback Adapter"}
        ]"#,
    ))
    .unwrap();
    let candidates = build_bind_address_candidates(records);
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].address, "26.1.2.3");
    assert_eq!(candidates[0].kind, "overlay");
    assert_eq!(candidates[0].family_name.as_deref(), Some("Radmin VPN"));
    assert_eq!(candidates[1].address, "192.168.1.2");
    assert_eq!(candidates[1].kind, "lan");
    assert!(candidates[1].family_name.is_none());
}
