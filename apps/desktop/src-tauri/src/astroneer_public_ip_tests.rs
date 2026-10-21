use super::*;

#[test]
fn public_ipv4_rejects_non_global_and_multicast_ranges_at_their_boundaries() {
    for value in [
        "0.0.0.0",
        "0.255.255.255",
        "10.0.0.0",
        "10.255.255.255",
        "100.64.0.0",
        "100.127.255.255",
        "127.0.0.1",
        "169.254.0.0",
        "169.254.255.255",
        "172.16.0.0",
        "172.31.255.255",
        "192.0.0.0",
        "192.0.0.8",
        "192.0.0.11",
        "192.0.0.170",
        "192.0.0.171",
        "192.0.2.0",
        "192.0.2.255",
        "192.88.99.2",
        "192.168.0.0",
        "192.168.255.255",
        "198.18.0.0",
        "198.19.255.255",
        "198.51.100.0",
        "198.51.100.255",
        "203.0.113.0",
        "203.0.113.255",
        "224.0.0.0",
        "239.255.255.255",
        "240.0.0.0",
        "255.255.255.255",
    ] {
        assert!(parse_public_address(value).is_err(), "non-public test case");
    }
    for value in [
        "1.1.1.1",
        "8.8.8.8",
        "9.255.255.255",
        "11.0.0.0",
        "100.63.255.255",
        "100.128.0.0",
        "169.253.255.255",
        "169.255.0.0",
        "172.15.255.255",
        "172.32.0.0",
        "192.0.0.9",
        "192.0.0.10",
        "192.31.196.1",
        "192.52.193.1",
        "192.88.99.1",
        "192.88.99.3",
        "192.175.48.1",
        "198.17.255.255",
        "198.20.0.0",
        "223.255.255.255",
    ] {
        assert!(parse_public_address(value).is_ok(), "public test case");
    }
    for value in ["", " ", "1.1.1.1\n", " 1.1.1.1", "::1", "1.1.1", "01.1.1.1"] {
        assert!(parse_public_address(value).is_err(), "malformed test case");
    }
}

#[test]
fn discovery_body_is_ascii_bounded_and_one_public_ipv4() {
    assert_eq!(
        parse_discovery(b"1.1.1.1\r\n").unwrap(),
        Ipv4Addr::new(1, 1, 1, 1)
    );
    for body in [
        vec![b' '; MAX_RESPONSE_BYTES + 1],
        vec![0xff],
        b"".to_vec(),
        b"<html>unavailable</html>".to_vec(),
        b"1.1.1.1\n8.8.8.8".to_vec(),
        b"10.1.2.3".to_vec(),
        b"224.0.0.1".to_vec(),
        b"::1".to_vec(),
    ] {
        assert_eq!(parse_discovery(&body).unwrap_err(), DISCOVERY_FAILED);
    }
    // Output must never echo response bodies or an operator-provided address.
    assert!(
        !parse_public_address("10.1.2.3")
            .unwrap_err()
            .contains("10.1.2.3")
    );
}

#[tokio::test]
async fn discovery_client_rejects_cleartext_before_connection() {
    let client = discovery_client().unwrap();
    // Reject before connection. This test performs no network request.
    let error = client.get("http://127.0.0.1:9/").send().await.unwrap_err();
    assert!(error.is_builder());
}

#[test]
fn startup_readback_allows_only_an_empty_address_to_be_prepared() {
    assert!(
        verify_startup_settings(
            r#"{"public_ip":"","server_name":"Fixture"}"#,
            r#"{"public_ip":"1.1.1.1","server_name":"Fixture"}"#
        )
        .unwrap()
    );
    assert!(
        !verify_startup_settings(r#"{"public_ip":"1.1.1.1"}"#, r#"{"public_ip":"1.1.1.1"}"#)
            .unwrap()
    );
    for (before, after) in [
        (r#"{"public_ip":""}"#, r#"{"public_ip":"10.0.0.1"}"#),
        (r#"{"public_ip":"1.1.1.1"}"#, r#"{"public_ip":"8.8.8.8"}"#),
        (
            r#"{"public_ip":"","server_name":"Fixture"}"#,
            r#"{"public_ip":"1.1.1.1","server_name":"Other"}"#,
        ),
    ] {
        assert!(verify_startup_settings(before, after).is_err());
    }
}
