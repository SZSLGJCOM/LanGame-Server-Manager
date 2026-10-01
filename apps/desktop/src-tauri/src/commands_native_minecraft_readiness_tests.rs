use super::*;
use serde_json::json;
use tokio::net::TcpListener;

const DONE: &str = "[11:48:05] [Server thread/INFO]: Done (0.142s)! For help, type \"help\"";

fn status_packet(value: serde_json::Value) -> Vec<u8> {
    let json = serde_json::to_vec(&value).unwrap();
    let mut packet = vec![0];
    write_varint(&mut packet, json.len() as u32);
    packet.extend(json);
    packet
}

fn valid_status() -> serde_json::Value {
    json!({"version":{"name":"native fixture","protocol":1},
        "players":{"online":0,"max":20},"description":{"text":"ready"}})
}

// This is a literal client-wire expectation, independent of write_varint and
// exchange: protocol -1, loopback hostname, big-endian port, status state/request.
fn expected_request(port: u16) -> Vec<u8> {
    let mut expected = vec![19, 0, 255, 255, 255, 255, 15, 9];
    expected.extend_from_slice(b"127.0.0.1");
    expected.extend_from_slice(&port.to_be_bytes());
    expected.extend_from_slice(&[1, 1, 0]);
    expected
}

#[test]
fn minecraft_native_readiness_uses_status_only_for_existing_query_disabled_instances() {
    assert!(required(true, "minecraft", r#"{"enable_query":false}"#).unwrap());
    assert!(!required(true, "minecraft", r#"{"enable_query":true}"#).unwrap());
    assert!(!required(false, "minecraft", r#"{"enable_query":false}"#).unwrap());
    assert!(!required(true, "terraria", "not JSON").unwrap());
    for settings in ["not JSON", "{}", r#"{"enable_query":"false"}"#] {
        assert!(required(true, "minecraft", settings).is_err());
    }
}

#[test]
fn minecraft_native_readiness_requires_the_managed_game_tcp_endpoint() {
    let mut endpoint = ProcessNetworkEndpoint {
        protocol: "tcp".into(),
        local_address: "0.0.0.0".into(),
        local_port: 25565,
        owning_pid: 7,
        process_key: "main".into(),
        relation: "root".into(),
    };
    assert!(!owns_game_endpoint(&[], 25565));
    assert!(owns_game_endpoint(std::slice::from_ref(&endpoint), 25565));
    assert!(!owns_game_endpoint(std::slice::from_ref(&endpoint), 25566));
    endpoint.protocol = "udp".into();
    assert!(!owns_game_endpoint(std::slice::from_ref(&endpoint), 25565));
    endpoint.protocol = "tcp".into();
    endpoint.process_key = "unrelated".into();
    assert!(!owns_game_endpoint(&[endpoint], 25565));
}

#[test]
fn minecraft_native_readiness_done_signal_is_strict_and_belongs_to_new_log_content() {
    use super::super::native_logs::{LogBaseline, new_log_line_matches};
    use std::io::Write;
    assert!(done_line(DONE));
    for line in [
        DONE.replace("0.142", "NaN"),
        DONE.replace("0.142", "-1"),
        format!("startup arguments {DONE}"),
        DONE.replace("[11:48:05]", "[old]"),
        DONE.replace("Server thread/INFO", "RCON Listener/INFO"),
    ] {
        assert!(!done_line(&line), "{line}");
    }
    let path = std::env::temp_dir().join(format!("minecraft-ready-{}.log", uuid::Uuid::new_v4()));
    std::fs::write(&path, format!("{DONE}\n")).unwrap();
    let mut baseline = LogBaseline::capture(&path).unwrap();
    assert!(!new_log_line_matches(&path, &mut baseline, done_line).unwrap());
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    writeln!(file, "[11:49:00] [Server thread/INFO]: Preparing level").unwrap();
    assert!(!new_log_line_matches(&path, &mut baseline, done_line).unwrap());
    writeln!(file, "{DONE}").unwrap();
    assert!(new_log_line_matches(&path, &mut baseline, done_line).unwrap());
    drop(file);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn minecraft_native_readiness_performs_status_handshake_over_a_real_tcp_stream() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let expected = expected_request(port);
        let mut request = vec![0; expected.len()];
        stream.read_exact(&mut request).await.unwrap();
        assert_eq!(request, expected);
        // Literal minimal valid Minecraft status response; packet length 99,
        // packet ID 0, JSON string length 97. No production encoder is reused.
        let json = br#"{"version":{"name":"fixture","protocol":1},"players":{"online":0,"max":20},"description":"ready"}"#;
        assert_eq!(json.len(), 97);
        stream.write_all(&[99, 0, 97]).await.unwrap();
        for chunk in json.chunks(3) {
            stream.write_all(chunk).await.unwrap();
        }
    });
    assert_eq!(query_status("0.0.0.0", port).await, Ok(()));
    server.await.unwrap();
}

#[test]
fn minecraft_native_readiness_rejects_wrong_packet_trailing_data_and_invalid_status_fields() {
    assert_eq!(validate_response(&status_packet(valid_status())), Ok(()));
    let mut packet = status_packet(valid_status());
    packet[0] = 1;
    assert_eq!(
        validate_response(&packet),
        Err("minecraft_status_packet_id_invalid")
    );
    packet[0] = 0;
    packet.push(0);
    assert_eq!(
        validate_response(&packet),
        Err("minecraft_status_json_length_invalid")
    );
    assert_eq!(
        validate_response(&[0, 1, 255]),
        Err("minecraft_status_json_invalid")
    );
    for value in [
        json!({}),
        json!({"description":"a non-Minecraft service"}),
        json!({"version":{"name":"fixture","protocol":1},"players":{"online":-1,"max":20},"description":"ready"}),
    ] {
        assert_eq!(
            validate_response(&status_packet(value)),
            Err("minecraft_status_fields_invalid")
        );
    }
    for varint in [
        &[128, 128, 128, 128, 128, 0][..],
        &[255, 255, 255, 255, 16][..],
        &[128][..],
    ] {
        assert!(take_varint(&mut &*varint).is_err());
    }
}

#[tokio::test]
async fn minecraft_native_readiness_bounds_frame_sizes_and_rejects_truncated_tcp_replies() {
    for (reply, expected) in [
        (vec![0], "minecraft_status_frame_size_invalid"),
        (vec![129, 128, 16], "minecraft_status_frame_size_invalid"), // 262145 bytes.
        (
            vec![128, 128, 128, 128, 128, 0],
            "minecraft_status_varint_invalid",
        ),
        (vec![8, 0, 7, b'{'], "minecraft_status_frame_incomplete"),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0; expected_request(port).len()];
            stream.read_exact(&mut request).await.unwrap();
            stream.write_all(&reply).await.unwrap();
        });
        assert_eq!(query_status("127.0.0.1", port).await, Err(expected));
        server.await.unwrap();
    }
}

#[tokio::test]
async fn minecraft_native_readiness_total_timeout_closes_the_connection_without_a_response() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = vec![0; expected_request(port).len()];
        stream.read_exact(&mut request).await.unwrap();
        // Await client EOF instead of using an arbitrary sleep in the server.
        let mut byte = [0];
        assert_eq!(stream.read(&mut byte).await.unwrap(), 0);
    });
    assert_eq!(
        query_with_budget("127.0.0.1", port, Duration::from_millis(200)).await,
        Err("minecraft_status_timed_out")
    );
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap();
}
