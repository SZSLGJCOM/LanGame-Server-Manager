//! Only the opt-in native acceptance may supply this local diagnostic proxy.
use std::net::{Ipv4Addr, SocketAddrV4};

pub(super) fn parse(input: Option<&str>) -> Result<Option<SocketAddrV4>, &'static str> {
    let Some(input) = input.filter(|input| !input.is_empty()) else {
        return Ok(None);
    };
    let address = input
        .parse::<SocketAddrV4>()
        .map_err(|_| "ASA acceptance HTTP proxy must be 127.0.0.1 with a nonzero port")?;
    // The disposable SSH reverse forward listens at this exact IPv4 address.
    // Never silently redirect another address or accept URL/userinfo syntax.
    if *address.ip() != Ipv4Addr::LOCALHOST || address.port() == 0 {
        return Err("ASA acceptance HTTP proxy must be 127.0.0.1 with a nonzero port");
    }
    Ok(Some(address))
}

pub(super) fn launch_flag(address: Option<SocketAddrV4>) -> String {
    address
        .map(|address| format!("-httpproxy={address}"))
        .unwrap_or_default()
}

#[test]
fn native_acceptance_proxy_generates_one_local_argument_or_no_argument() {
    assert_eq!(parse(None), Ok(None));
    assert_eq!(parse(Some("")), Ok(None));
    assert_eq!(launch_flag(parse(None).unwrap()), "");
    for port in [1, 27897, u16::MAX] {
        let input = format!("127.0.0.1:{port}");
        assert_eq!(
            launch_flag(parse(Some(&input)).unwrap()),
            format!("-httpproxy={input}")
        );
    }
}

#[test]
fn native_acceptance_proxy_rejects_remote_credentials_urls_and_argument_injection() {
    for input in [
        "127.0.0.1:0",
        "127.0.0.1:65536",
        "127.0.0.1",
        "localhost:27897",
        "192.0.2.1:27897",
        "0.0.0.0:27897",
        "127.0.0.2:27897",
        "[::1]:27897",
        "http://127.0.0.1:27897",
        "user:password@127.0.0.1:27897",
        "127.0.0.1:27897/path",
        "127.0.0.1:27897 -NoVerifyPeer",
        "127.0.0.1:27897\n-log",
        " 127.0.0.1:27897",
        "127.0.0.1:27897 ",
        "\"127.0.0.1:27897\"",
        " ",
    ] {
        let error = parse(Some(input)).unwrap_err();
        assert_eq!(
            error,
            "ASA acceptance HTTP proxy must be 127.0.0.1 with a nonzero port"
        );
    }
}
