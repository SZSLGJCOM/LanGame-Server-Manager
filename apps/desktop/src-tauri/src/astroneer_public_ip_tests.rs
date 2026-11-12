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
        "26.0.0.0",
        "26.255.255.255",
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
fn advertised_ipv4_accepts_lan_and_vpn_addresses_without_weakening_discovery() {
    for value in [
        "10.0.0.1",
        "10.255.255.254",
        "172.16.0.1",
        "172.31.255.254",
        "192.168.1.2",
        "100.64.0.1",
        "100.127.255.254",
    ] {
        assert!(parse_advertised_address(value).is_ok(), "{value}");
        assert!(parse_public_address(value).is_err(), "{value}");
        assert!(parse_discovery(value.as_bytes()).is_err(), "{value}");
    }
    for value in ["26.1.2.3", "1.1.1.1"] {
        assert!(parse_advertised_address(value).is_ok());
    }
    for value in [
        "",
        " ",
        "0.0.0.0",
        "127.0.0.1",
        "169.254.1.2",
        "192.0.2.1",
        "224.0.0.1",
        "255.255.255.255",
        "::1",
        "192.168.1.2\n",
        "192.168.01.2",
    ] {
        assert!(parse_advertised_address(value).is_err(), "{value}");
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

#[tokio::test]
async fn discovery_reuses_first_success_and_falls_back_on_connection_failure() {
    for fail_primary in [false, true] {
        let mut called = Vec::new();
        let address = discover_with(|source, _, budget| {
            called.push(source);
            assert!(budget <= DISCOVERY_BUDGET);
            std::future::ready(if source == "ipify" && fail_primary {
                Err(DiscoveryFailure::new(source, "connection", true))
            } else {
                Ok(Ipv4Addr::new(1, 1, 1, 1))
            })
        })
        .await
        .unwrap();
        assert_eq!(address, Ipv4Addr::new(1, 1, 1, 1));
        assert_eq!(
            called,
            if fail_primary {
                vec!["ipify", "aws"]
            } else {
                vec!["ipify"]
            }
        );
    }
}

#[tokio::test]
async fn discovery_reports_each_failed_source_without_claiming_a_timeout_or_no_public_ip() {
    let error = discover_with(|source, _, _| {
        std::future::ready(Err(DiscoveryFailure::new(source, "connection", true)))
    })
    .await
    .unwrap_err();
    let value: Value = serde_json::from_str(&error).unwrap();
    assert_eq!(value["code"], "astroneer_network_preflight_failed");
    assert_eq!(value["reason"], "discovery_failed");
    assert_eq!(
        value["failures"],
        serde_json::json!([
            {"source":"ipify", "reason":"connection"},
            {"source":"aws", "reason":"connection"},
        ])
    );
    assert!(!error.contains("eight seconds"));
    assert!(!error.contains("permits_fallback"));
}

#[tokio::test]
async fn discovery_respects_throttling_authorization_and_invalid_response_boundaries() {
    for status in [401, 403, 429] {
        let mut count = 0;
        let error = discover_with(|source, _, _| {
            count += 1;
            std::future::ready(Err(DiscoveryFailure::network(
                source,
                app_network::NetworkError::Status {
                    status: reqwest::StatusCode::from_u16(status).unwrap(),
                    origin: "https://api.ipify.org".into(),
                },
            )))
        })
        .await
        .unwrap_err();
        assert_eq!(count, 1);
        let error: Value = serde_json::from_str(&error).unwrap();
        assert_eq!(error["failures"][0]["reason"], "http");
        assert_eq!(error["failures"][0]["status"], status);
    }
    let mut count = 0;
    let error = discover_with(|source, _, _| {
        count += 1;
        std::future::ready(Err(DiscoveryFailure::new(source, "response", false)))
    })
    .await
    .unwrap_err();
    assert_eq!(count, 1);
    assert_eq!(
        serde_json::from_str::<Value>(&error).unwrap()["failures"][0]["reason"],
        "response"
    );
}

#[tokio::test]
async fn discovery_reserves_time_for_backup_when_primary_stalls() {
    let mut called = Vec::new();
    let address = discover_with(|source, _, _| {
        called.push(source);
        async move {
            if source == "ipify" {
                std::future::pending::<()>().await;
            }
            Ok(Ipv4Addr::new(1, 1, 1, 1))
        }
    })
    .await
    .unwrap();
    assert_eq!(address, Ipv4Addr::new(1, 1, 1, 1));
    assert_eq!(called, vec!["ipify", "aws"]);
}

#[tokio::test]
#[ignore = "bounded live public IPv4 lookup; no game process or configuration writes"]
async fn astroneer_public_ip_live_discovery() {
    let client = discovery_client().unwrap();
    let address = discover_with(|source, url, budget| {
        let client = client.clone();
        async move {
            let result = query_source(client, source, url, budget).await;
            println!(
                "ASTRONEER_ADDRESS_LOOKUP source={source} result={}",
                result
                    .as_ref()
                    .map(|_| "valid_ipv4")
                    .unwrap_or_else(|failure| failure.reason)
            );
            result
        }
    })
    .await
    .unwrap();
    assert!(is_public_unicast(address));
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
    assert!(
        verify_startup_settings(
            r#"{"public_ip":""}"#,
            r#"{"public_ip":"1.1.1.1","connection_mode":"public"}"#,
        )
        .unwrap()
    );
    assert!(
        verify_startup_settings(
            r#"{"public_ip":"","connection_mode":"radmin"}"#,
            r#"{"public_ip":"26.1.2.3","connection_mode":"radmin"}"#,
        )
        .unwrap()
    );
    assert!(
        !verify_startup_settings(
            r#"{"public_ip":"26.1.2.3"}"#,
            r#"{"public_ip":"26.1.2.3","connection_mode":"manual"}"#,
        )
        .unwrap()
    );
    for before in [
        r#"{"public_ip":""}"#,
        r#"{"public_ip":"","connection_mode":"public"}"#,
    ] {
        assert!(
            verify_startup_settings(
                before,
                r#"{"public_ip":"26.1.2.3","connection_mode":"radmin"}"#
            )
            .unwrap()
        );
    }
    for (before, after) in [
        (r#"{"public_ip":""}"#, r#"{"public_ip":"0.0.0.0"}"#),
        (r#"{"public_ip":""}"#, r#"{"public_ip":"10.0.0.1"}"#),
        (r#"{"public_ip":"1.1.1.1"}"#, r#"{"public_ip":"8.8.8.8"}"#),
        (
            r#"{"public_ip":""}"#,
            r#"{"public_ip":"1.1.1.1","connection_mode":"manual"}"#,
        ),
        (
            r#"{"public_ip":"1.1.1.1"}"#,
            r#"{"public_ip":"1.1.1.1","connection_mode":"public"}"#,
        ),
        (
            r#"{"public_ip":"","connection_mode":"manual"}"#,
            r#"{"public_ip":"1.1.1.1","connection_mode":"public"}"#,
        ),
        (
            r#"{"public_ip":"","connection_mode":"manual"}"#,
            r#"{"public_ip":"1.1.1.1","connection_mode":"manual"}"#,
        ),
        (
            r#"{"public_ip":"","server_name":"Fixture"}"#,
            r#"{"public_ip":"1.1.1.1","server_name":"Other"}"#,
        ),
    ] {
        assert!(verify_startup_settings(before, after).is_err());
    }
}

#[test]
fn connection_mode_retains_legacy_addresses_and_rejects_unknown_modes() {
    let mut settings = Map::new();
    let address = Ipv4Addr::new(26, 1, 2, 3);
    assert_eq!(
        connection_mode(&settings, None).unwrap(),
        ConnectionMode::Public
    );
    assert_eq!(
        connection_mode(&settings, Some(address)).unwrap(),
        ConnectionMode::Manual,
    );
    for (value, expected) in [
        ("public", ConnectionMode::Public),
        ("radmin", ConnectionMode::Radmin),
        ("manual", ConnectionMode::Manual),
    ] {
        settings.insert("connection_mode".into(), Value::String(value.into()));
        for address in [None, Some(address)] {
            assert_eq!(connection_mode(&settings, address).unwrap(), expected);
        }
    }
    for value in [
        Value::Null,
        Value::Bool(false),
        Value::String("auto".into()),
    ] {
        settings.insert("connection_mode".into(), value);
        let error = connection_mode(&settings, Some(address)).unwrap_err();
        assert_eq!(
            serde_json::from_str::<Value>(&error).unwrap()["reason"],
            "invalid_connection_mode",
        );
        assert!(!error.contains("26.1.2.3"));
    }
}

#[test]
fn radmin_selection_requires_known_adapter_membership_and_an_unambiguous_address() {
    let candidate = |address: &str, kind: &str, family: Option<&str>| BindAddressCandidate {
        address: address.into(),
        kind: kind.into(),
        adapter_name: Some("Private adapter label".into()),
        family_name: family.map(str::to_owned),
    };
    let radmin = |address| candidate(address, "overlay", Some("Radmin VPN"));
    let address = Ipv4Addr::new(26, 1, 2, 3);
    let unsupported = vec![
        candidate("26.1.2.3", "public", None),
        candidate("26.1.2.3", "overlay", Some("ZeroTier One")),
        candidate("26.1.2.3", "lan", Some("Radmin VPN")),
        radmin("0.0.0.0"),
        radmin("127.0.0.1"),
        radmin("224.0.0.1"),
    ];
    let error = select_radmin_address(&unsupported, None).unwrap_err();
    assert_eq!(
        serde_json::from_str::<Value>(&error).unwrap()["reason"],
        "radmin_unavailable",
    );
    assert!(!error.contains("Private adapter label"));
    assert!(!error.contains("26.1.2.3"));

    let mut candidates = unsupported;
    candidates.push(radmin("26.1.2.3"));
    assert_eq!(select_radmin_address(&candidates, None).unwrap(), address);
    assert_eq!(
        select_radmin_address(&candidates, Some(address)).unwrap(),
        address
    );
    candidates.push(radmin("26.4.5.6"));
    assert_eq!(
        serde_json::from_str::<Value>(&select_radmin_address(&candidates, None).unwrap_err())
            .unwrap()["reason"],
        "radmin_address_ambiguous",
    );
    assert_eq!(
        select_radmin_address(&candidates, Some(address)).unwrap(),
        address
    );
    assert_eq!(
        serde_json::from_str::<Value>(
            &select_radmin_address(&candidates, Some(Ipv4Addr::new(1, 1, 1, 1))).unwrap_err(),
        )
        .unwrap()["reason"],
        "radmin_address_unavailable",
    );
}
