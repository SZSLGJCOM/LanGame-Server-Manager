use super::*;

#[tokio::test]
async fn private_servers_report_visibility_limits_without_querying_or_claiming_zero_players() {
    for (module, setting_key, disabled, enabled) in [
        (
            "valheim",
            "public_server",
            serde_json::json!(0),
            serde_json::json!(1),
        ),
        (
            "vrising",
            "list_on_steam",
            serde_json::json!(false),
            serde_json::json!(true),
        ),
        (
            "abioticfactor",
            "lan_only",
            serde_json::json!(true),
            serde_json::json!(false),
        ),
    ] {
        let details: InstanceDetails = serde_json::from_value(serde_json::json!({
        "summary": {
            "id": "private-server", "name": "Private fixture", "module_id": module,
            "status": "Running", "bind_ip": "127.0.0.1", "port_count": 0, "autostart": false
        },
        "config_file_path": "", "saves_path": "", "auto_backup_on_stop": false,
        "backup_retention_count": 1, "settings_json": serde_json::json!({setting_key: disabled}).to_string(),
        "ports": [], "active_run": null
    }))
    .expect("private server fixture");
        let query = ModulePlayerQuerySpec {
            protocol: "a2s_info".into(),
            port_names: vec!["query".into()],
        };
        let snapshot = collect_a2s_players(&details, &query, "private-request", 1_000)
            .await
            .expect_err("private mode has no A2S capability");
        assert_eq!(snapshot.status, RuntimeLivePlayerStatus::Unsupported);
        assert_eq!(snapshot.current_players, None);
        assert!(!snapshot.complete);
        assert!(snapshot.entries.is_empty());
        assert_eq!(snapshot.issue.as_ref().unwrap().setting_keys, [setting_key]);

        let mut public = details;
        public.settings_json = serde_json::json!({setting_key: enabled}).to_string();
        let snapshot = collect_a2s_players(&public, &query, "public-request", 1_000)
            .await
            .expect_err("public mode still validates the missing port");
        assert_eq!(snapshot.status, RuntimeLivePlayerStatus::Misconfigured);
        assert!(snapshot.issue.unwrap().setting_keys.is_empty());
    }
}

#[test]
fn repeated_wire_indices_and_names_are_distinct_read_only_rows() {
    let mut full = response(b"same name", 1.0);
    full[5] = 2;
    full.extend_from_slice(&response(b"same name", 2.0)[6..]);
    let players = parse_players(&full).expect("wire indices are not identity");
    let snapshot = project_players("instance", "request", 1000, players, None).public_snapshot;
    assert_eq!(snapshot.entries.len(), 2);
    assert_ne!(
        snapshot.entries[0].player_key,
        snapshot.entries[1].player_key
    );
    assert!(
        snapshot
            .entries
            .iter()
            .all(|row| row.identifiers.is_empty() && row.available_action_ids.is_empty())
    );
}
fn response(name: &[u8], duration: f32) -> Vec<u8> {
    let mut data = b"\xff\xff\xff\xffD\x01\x00".to_vec();
    data.extend_from_slice(name);
    data.push(0);
    data.extend_from_slice(&10_i32.to_le_bytes());
    data.extend_from_slice(&duration.to_le_bytes());
    data
}
#[test]
fn query_names_are_read_only_and_do_not_become_account_ids() {
    let players = parse_players(&response("玩家".as_bytes(), 10.5)).expect("valid fixture");
    let result = project_players("instance", "request", 20_000, players, None);
    let entry = &result.public_snapshot.entries[0];
    assert_eq!(entry.display_name, "玩家");
    assert_eq!(entry.session_started_at_unix_ms, Some(9500));
    assert!(entry.identifiers.is_empty());
    assert!(entry.available_action_ids.is_empty());
    assert!(result.private_action_bindings.is_empty());
}
#[test]
fn anonymous_and_empty_queries_do_not_claim_empty_servers() {
    let info = Some(app_storage::QueriedPlayerCount {
        current_players: 1,
        max_players: 10,
    });
    for players in [
        parse_players(&response(b"", 0.0)).expect("anonymous response"),
        Vec::new(),
    ] {
        let result = project_players("instance", "request", 1000, players, info);
        assert!(!result.public_snapshot.complete);
        assert_eq!(result.public_snapshot.current_players, Some(1));
        assert!(result.public_snapshot.entries.is_empty());
    }
    assert!(
        !project_players("instance", "request", 1000, Vec::new(), None)
            .public_snapshot
            .complete
    );
    assert_eq!(
        project_players("instance", "request", 1000, Vec::new(), None)
            .public_snapshot
            .current_players,
        None
    );
    assert!(
        project_players(
            "instance",
            "request",
            1000,
            Vec::new(),
            Some(app_storage::QueriedPlayerCount {
                current_players: 0,
                max_players: 10
            })
        )
        .public_snapshot
        .complete
    );
}
#[test]
fn incomplete_invalid_and_nonfinite_packets_are_rejected() {
    let good = response(b"player", 1.0);
    for end in 0..good.len() {
        assert!(parse_players(&good[..end]).is_err());
    }
    for bad in [
        response(b"player", f32::NAN),
        response(b"player", -1.0),
        response(b"\xff", 0.0),
        response(b"line\nname", 0.0),
    ] {
        assert!(parse_players(&bad).is_err());
    }
    let mut trailing = good;
    trailing.push(0);
    assert!(parse_players(&trailing).is_err());
}
#[test]
fn query_protocol_handles_challenge_and_preserves_info_counts() {
    let socket = UdpSocket::bind("127.0.0.1:0").expect("bind fixture");
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("timeout");
    let address = socket.local_addr().expect("fixture address");
    let server = std::thread::spawn(move || {
        let mut buf = [0; 128];
        let (size, peer) = socket.recv_from(&mut buf).expect("player request");
        assert_eq!(&buf[..size], b"\xff\xff\xff\xffU\xff\xff\xff\xff");
        socket
            .send_to(b"\xff\xff\xff\xffA1234", peer)
            .expect("challenge");
        let (size, peer) = socket.recv_from(&mut buf).expect("challenge reply");
        assert_eq!(&buf[..size], b"\xff\xff\xff\xffU1234");
        socket
            .send_to(&response(b"Player", 2.0), peer)
            .expect("players");
        let (_, peer) = socket.recv_from(&mut buf).expect("info query");
        socket
            .send_to(
                b"\xff\xff\xff\xffI\x11server\0map\0folder\0game\0\x01\x00\x01\x0a",
                peer,
            )
            .expect("info");
    });
    let (players, count) = query_players(address).expect("player query");
    server.join().expect("fixture server");
    assert_eq!(players[0].name, "Player");
    assert_eq!(count.expect("count").current_players, 1);
}
