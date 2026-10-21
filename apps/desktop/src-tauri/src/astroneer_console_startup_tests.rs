use super::*;
use tokio::net::TcpListener;

#[test]
fn only_proven_descriptive_tokens_are_encoded_without_changing_the_name() {
    assert_eq!(
        new_game_command("SAVE_1").unwrap(),
        "DSNewGame SAVE_1 false"
    );
    assert_eq!(
        new_game_command("World-2").unwrap(),
        "DSNewGame World-2 false"
    );
    for label in [
        "",
        " ",
        "SAVE 1",
        " SAVE_1",
        "SAVE_1 ",
        "SAVE\"1",
        "世界",
        "SAVE$2026",
        "x\nDSNewGame",
        "x\\y",
    ] {
        assert!(new_game_command(label).is_err(), "{label:?}");
    }
    assert!(new_game_command(&"a".repeat(129)).is_err());
}

#[test]
fn peer_ownership_requires_the_server_direction_and_entire_four_tuple() {
    let client = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 41001);
    let server = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 1234);
    let row = Row {
        state: 5,
        local_address: u32::from_ne_bytes([127, 0, 0, 1]),
        local_port: u16::to_be(1234) as u32,
        remote_address: u32::from_ne_bytes([127, 0, 0, 1]),
        remote_port: u16::to_be(41001) as u32,
        pid: 42,
    };
    assert!(peer_row(row, client, server, 42));
    assert!(!peer_row(row, client, server, 43));
    assert!(!peer_row(row, server, client, 42));
    assert!(!peer_row(Row { state: 2, ..row }, client, server, 42));
    assert!(!peer_row(
        Row {
            remote_port: u16::to_be(41002) as u32,
            ..row
        },
        client,
        server,
        42
    ));
    assert!(!peer_row(
        Row {
            local_address: 0,
            ..row
        },
        client,
        server,
        42
    ));
}

#[cfg(windows)]
fn current_identity() -> ProcessIdentity {
    app_runtime::inspect_process_identity(std::process::id())
        .unwrap()
        .unwrap()
}

async fn read_lines(socket: &mut TcpStream, count: usize) -> Vec<u8> {
    let mut result = Vec::new();
    let mut bytes = [0; 128];
    while result.iter().filter(|byte| **byte == b'\n').count() < count {
        let length = socket.read(&mut bytes).await.unwrap();
        assert_ne!(length, 0, "socket closed before all FIFO commands");
        result.extend_from_slice(&bytes[..length]);
        assert!(result.len() <= 1024);
    }
    result
}

#[cfg(windows)]
#[tokio::test]
async fn create_once_keeps_one_verified_connection_through_fifo_but_returns_no_world_success() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let received = read_lines(&mut socket, 3).await;
        assert_eq!(
            received,
            b"fixture-secret\nDSNewGame SAVE_1 false\nDSListGames\n"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
        // The compact acknowledgement and pretty FIFO query can share a read.
        socket
            .write_all(
                b"{\"status\":true}\r\n{\n\"gameList\": [],\n\"activeSaveName\": \"\"\n}\r\n",
            )
            .await
            .unwrap();
    });
    let response = create_once(
        port,
        "fixture-secret",
        "SAVE_1",
        std::process::id(),
        &current_identity(),
        || Ok(()),
    )
    .await
    .unwrap();
    assert_eq!(
        response,
        serde_json::json!({"gameList":[],"activeSaveName":""})
    );
    server.await.unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn save_once_sends_one_native_save_and_waits_for_same_socket_fifo_query() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        assert_eq!(
            read_lines(&mut socket, 3).await,
            b"fixture-secret\nDSSaveGame\nDSListGames\n"
        );
        socket
            .write_all(b"{\"gameList\":[],\"activeSaveName\":\"SAVE_1\"}\r\n")
            .await
            .unwrap();
        let mut extra = [0; 128];
        assert_eq!(socket.read(&mut extra).await.unwrap(), 0);
    });
    save_once(
        port,
        "fixture-secret",
        "SAVE_1",
        std::process::id(),
        &current_identity(),
        || Ok(()),
    )
    .await
    .unwrap();
    server.await.unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn save_once_unknown_response_does_not_resend_or_reconnect() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        assert_eq!(
            read_lines(&mut socket, 3).await,
            b"fixture-secret\nDSSaveGame\nDSListGames\n"
        );
        socket
            .write_all(b"{\"gameList\":[],\"activeSaveName\":\"OTHER\"}\r\n")
            .await
            .unwrap();
        let mut extra = [0; 128];
        assert_eq!(socket.read(&mut extra).await.unwrap(), 0);
        assert!(
            tokio::time::timeout(Duration::from_millis(30), listener.accept())
                .await
                .is_err()
        );
    });
    assert!(
        save_once(
            port,
            "fixture-secret",
            "SAVE_1",
            std::process::id(),
            &current_identity(),
            || Ok(())
        )
        .await
        .is_err()
    );
    server.await.unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn foreign_socket_gets_neither_password_nor_mutation() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = [0; 128];
        assert_eq!(socket.read(&mut bytes).await.unwrap(), 0);
    });
    assert!(
        create_once(
            port,
            "must-not-be-sent",
            "SAVE_1",
            u32::MAX,
            &current_identity(),
            || Ok(())
        )
        .await
        .is_err()
    );
    server.await.unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn failed_final_precondition_sends_no_password_or_new_game() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = [0; 128];
        assert_eq!(socket.read(&mut bytes).await.unwrap(), 0);
    });
    assert!(
        create_once(
            port,
            "must-not-be-sent",
            "SAVE_1",
            std::process::id(),
            &current_identity(),
            || Err("disk changed".into())
        )
        .await
        .is_err()
    );
    server.await.unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn reused_pid_with_different_filetime_cannot_receive_credentials() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = [0; 128];
        assert_eq!(socket.read(&mut bytes).await.unwrap(), 0);
    });
    let mut wrong = current_identity();
    wrong.creation_time += 1;
    assert!(
        create_once(
            port,
            "must-not-be-sent",
            "SAVE_1",
            std::process::id(),
            &wrong,
            || Ok(())
        )
        .await
        .is_err()
    );
    server.await.unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn delivery_failure_is_unknown_and_never_repeats_the_command() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        assert_eq!(
            read_lines(&mut socket, 3).await,
            b"fixture\nDSNewGame SAVE_1 false\nDSListGames\n"
        );
        drop(socket);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
    });
    assert!(
        create_once(
            port,
            "fixture",
            "SAVE_1",
            std::process::id(),
            &current_identity(),
            || Ok(())
        )
        .await
        .is_err()
    );
    server.await.unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn verified_native_query_preserves_fragmented_json_and_string_braces() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        assert_eq!(read_lines(&mut socket, 2).await, b"fixture\nDSListGames\n");
        socket
            .write_all(b"{\"activeSaveName\":\"SAVE{")
            .await
            .unwrap();
        tokio::task::yield_now().await;
        socket
            .write_all(b"}1\",\"gameList\":[]}\r\n")
            .await
            .unwrap();
    });
    let result = query(
        port,
        "fixture",
        super::super::Query::Games,
        std::process::id(),
        &current_identity(),
    )
    .await
    .unwrap();
    assert_eq!(result["activeSaveName"], "SAVE{}1");
    server.await.unwrap();
}
