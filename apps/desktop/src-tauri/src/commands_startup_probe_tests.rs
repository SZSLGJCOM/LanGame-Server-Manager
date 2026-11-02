use super::*;
#[cfg(windows)]
use std::net::Ipv4Addr;

fn astroneer_instance(port: u16) -> InstanceDetails {
    serde_json::from_value(serde_json::json!({
        "summary":{"id":"startup-fixture","name":"Fixture","module_id":"astroneer",
            "status":"Starting","bind_ip":"0.0.0.0","port_count":1,"autostart":false},
        "config_file_path":"","saves_path":"","auto_backup_on_stop":false,"backup_retention_count":1,
        "settings_json":"{\"console_password\":\"fixture-password\",\"load_auto_save\":true,\"disable_server_travel\":false}",
        "ports":[{"name":"console","protocol":"tcp","port":port}], "active_run":null
    })).unwrap()
}

#[test]
fn astroneer_probe_reads_existing_configuration_without_needing_a_completed_run() {
    struct RuntimeFixture(PathBuf);
    impl Drop for RuntimeFixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    let fixture = RuntimeFixture(
        std::env::temp_dir().join(format!("langame-startup-probe-{}", uuid::Uuid::new_v4())),
    );
    std::fs::create_dir_all(fixture.0.join("runtime")).unwrap();
    std::fs::write(
        fixture.0.join("runtime/.langame-private-runtime"),
        b"managed\n",
    )
    .unwrap();
    let mut instance = astroneer_instance(1234);
    instance.config_file_path = fixture
        .0
        .join("config/server.ini")
        .to_string_lossy()
        .into_owned();
    instance.settings_json =
        serde_json::json!({"console_password":" fixture-password ","load_auto_save":false,"disable_server_travel":false}).to_string();
    let original = instance.settings_json.clone();
    let probe = StartupProbe::astroneer(&instance).unwrap();
    let StartupProbe::Astroneer {
        port,
        password,
        root,
        allow_empty_world,
        native,
    } = probe
    else {
        panic!("wrong probe")
    };
    assert_eq!(port, 1234);
    assert_eq!(password, " fixture-password ");
    assert_eq!(
        root,
        std::fs::canonicalize(fixture.0.join("runtime")).unwrap()
    );
    assert!(allow_empty_world);
    assert!(native.is_none());
    assert_eq!(instance.settings_json, original);
}

#[test]
fn explicit_management_mode_requires_both_complete_empty_world_observations() {
    let games = serde_json::json!({"gameList":[{"name":"preserved-world"}],"activeSaveName":""});
    let statistics = serde_json::json!({"saveGameName":""});
    assert!(astroneer_ready(&games, &statistics, true));
    assert!(!astroneer_ready(&games, &statistics, false));
    for games in [
        serde_json::json!({"playerInfo":[]}),
        serde_json::json!({"gameList":{},"activeSaveName":""}),
        serde_json::json!({"gameList":[],"activeSaveName":" "}),
        serde_json::json!({"gameList":[],"activeSaveName":null}),
    ] {
        assert!(!astroneer_ready(&games, &statistics, true));
    }
    assert!(!astroneer_ready(&games, &serde_json::json!({}), true));
    assert!(!astroneer_ready(
        &games,
        &serde_json::json!({"saveGameName":"selected"}),
        true
    ));
}

#[test]
fn astroneer_probe_rejects_invalid_configuration_without_disclosing_credentials() {
    for password in [
        String::new(),
        " ".into(),
        "secret\nDSNewGame".into(),
        "x".repeat(129),
    ] {
        let mut instance = astroneer_instance(1234);
        instance.settings_json = serde_json::json!({"console_password":password}).to_string();
        let Err(error) = StartupProbe::astroneer(&instance) else {
            panic!("invalid credential")
        };
        assert_eq!(error, "ASTRONEER startup console credential is invalid.");
    }
    let mut instance = astroneer_instance(1234);
    instance.settings_json = "{ invalid secret".into();
    let Err(error) = StartupProbe::astroneer(&instance) else {
        panic!("invalid JSON")
    };
    assert_eq!(error, "ASTRONEER startup console settings are invalid.");
    for variant in 0..5 {
        let mut instance = astroneer_instance(1234);
        match variant {
            0 => instance.ports.clear(),
            1 => instance.ports[0].port = 0,
            2 => instance.ports[0].protocol = "udp".into(),
            3 => instance.ports.push(instance.ports[0].clone()),
            _ => instance.summary.module_id = "another-game".into(),
        }
        assert!(StartupProbe::astroneer(&instance).is_err());
    }
}

#[cfg(windows)]
fn current_target() -> StartupProcessTarget {
    StartupProcessTarget {
        pid: std::process::id(),
        identity: app_runtime::inspect_process_identity(std::process::id())
            .unwrap()
            .unwrap(),
        host: "127.0.0.1".into(),
    }
}

#[cfg(windows)]
async fn read_query(socket: &mut tokio::net::TcpStream) -> Vec<u8> {
    use tokio::io::AsyncReadExt;
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut bytes = Vec::new();
        let mut block = [0; 256];
        while bytes.iter().filter(|byte| **byte == b'\n').count() < 2 {
            let length = socket.read(&mut block).await.unwrap();
            assert_ne!(length, 0);
            bytes.extend_from_slice(&block[..length]);
            assert!(bytes.len() <= 1024);
        }
        bytes
    })
    .await
    .unwrap()
}

#[cfg(windows)]
#[tokio::test]
async fn astroneer_readiness_queries_the_selected_world_not_the_early_player_list() {
    use tokio::io::AsyncWriteExt;
    for (games, statistics, expected) in [
        (
            serde_json::json!({"playerInfo":[]}),
            serde_json::json!({"saveGameName":"World"}),
            false,
        ),
        (
            serde_json::json!({"gameList":[],"activeSaveName":""}),
            serde_json::json!({"saveGameName":""}),
            false,
        ),
        (
            serde_json::json!({"gameList":[],"activeSaveName":"World"}),
            serde_json::json!({"saveGameName":"world"}),
            false,
        ),
        (
            serde_json::json!({"gameList":[],"activeSaveName":"World"}),
            serde_json::json!({"saveGameName":"World"}),
            true,
        ),
    ] {
        let listener = std::sync::Arc::new(
            tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
                .await
                .unwrap(),
        );
        let port = listener.local_addr().unwrap().port();
        let server_listener = listener.clone();
        let server = tokio::spawn(async move {
            for (command, response) in [("DSListGames", games), ("DSServerStatistics", statistics)]
            {
                let (mut socket, _) =
                    tokio::time::timeout(Duration::from_secs(5), server_listener.accept())
                        .await
                        .unwrap()
                        .unwrap();
                assert_eq!(
                    read_query(&mut socket).await,
                    format!("fixture-password\n{command}\n").as_bytes()
                );
                socket
                    .write_all(format!("{response}\r\n").as_bytes())
                    .await
                    .unwrap();
            }
        });
        let mut target = current_target();
        target.host = "203.0.113.1".into(); // PublicIP must not redirect the console.
        assert_eq!(
            tokio::task::spawn_blocking(move || poll_owned_tcp(&target, port, || query_astroneer(
                port,
                "fixture-password",
                &target,
                false
            )))
            .await
            .unwrap()
            .unwrap(),
            expected
        );
        server.await.unwrap();
        drop(listener);
    }
}

#[cfg(windows)]
#[tokio::test]
async fn astroneer_checked_query_rejects_a_changed_peer_before_sending_credentials() {
    use tokio::io::AsyncReadExt;
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (mut socket, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
            .await
            .unwrap()
            .unwrap();
        let mut bytes = [0; 128];
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), socket.read(&mut bytes))
                .await
                .unwrap()
                .unwrap(),
            0
        );
    });
    let mut target = current_target();
    target.identity.creation_time += 1;
    let result = startup::query_checked(
        port,
        "must-not-be-sent",
        Query::Games,
        target.pid,
        &target.identity,
    )
    .await;
    assert!(matches!(result, Err(startup::QueryError::Ownership(_))));
    server.await.unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn astroneer_checked_query_classifies_an_incomplete_reply_as_pending() {
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (mut socket, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            read_query(&mut socket).await,
            b"fixture-password\nDSListGames\n"
        );
    });
    let target = current_target();
    let result = startup::query_checked(
        port,
        "fixture-password",
        Query::Games,
        target.pid,
        &target.identity,
    )
    .await;
    assert!(matches!(result, Err(startup::QueryError::Unavailable(_))));
    server.await.unwrap();
}
