use super::*;

#[path = "commands_rust_shutdown_tests.rs"]
mod rust_shutdown_tests;

#[test]
pub(super) fn battleye_rcon_command_response_acks_server_message_and_reorders_fragments() {
    let server = UdpSocket::bind("127.0.0.1:0").unwrap();
    let client = UdpSocket::bind("127.0.0.1:0").unwrap();
    server.connect(client.local_addr().unwrap()).unwrap();
    client.connect(server.local_addr().unwrap()).unwrap();
    server
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    client
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();

    server
        .send(&battleye_rcon_wrap_payload(&[0x02, 0x41]))
        .unwrap();
    server
        .send(&battleye_rcon_wrap_payload(&[
            0x01, 0x07, 0x00, 0x02, 0x01, b'p', b'a', b'g', b'e', b' ', b'2',
        ]))
        .unwrap();
    server
        .send(&battleye_rcon_wrap_payload(&[
            0x01, 0x07, 0x00, 0x02, 0x00, b'p', b'a', b'g', b'e', b' ', b'1', b'\n',
        ]))
        .unwrap();

    let response = battleye_rcon_read_command_response(&client, 0x07).unwrap();
    let ack = battleye_rcon_read_packet(&server).unwrap();

    assert_eq!(response, "page 1\npage 2");
    assert_eq!(ack.payload, vec![0x02, 0x41]);
}

#[test]
pub(super) fn source_rcon_command_response_combines_fragments_until_terminator() {
    let mut bytes = Vec::new();
    source_rcon_write_packet(&mut bytes, 14002, 0, "players page 1\n").unwrap();
    source_rcon_write_packet(&mut bytes, 14002, 0, "players page 2\n").unwrap();
    source_rcon_write_packet(&mut bytes, 14003, 0, "").unwrap();

    let mut cursor = std::io::Cursor::new(bytes);
    let response =
        source_rcon_read_command_response(&mut cursor, 14002, 14003, Vec::new()).unwrap();

    assert_eq!(response, "players page 1\nplayers page 2\n");
}

#[test]
pub(super) fn source_rcon_command_response_errors_on_rejected_packet() {
    let mut bytes = Vec::new();
    source_rcon_write_packet(&mut bytes, -1, 0, "").unwrap();

    let mut cursor = std::io::Cursor::new(bytes);
    let error = source_rcon_read_command_response(&mut cursor, 14002, 14003, Vec::new())
        .expect_err("rejected RCON packet should fail");

    assert!(error.contains("rejected"));
}

#[test]
pub(super) fn source_rcon_exec_sends_command_to_server() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let (tx, rx) = std::sync::mpsc::channel();

    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();

        let auth = source_rcon_read_packet(&mut stream).unwrap();
        source_rcon_write_packet(&mut stream, 14001, 2, "").unwrap();

        let exec = source_rcon_read_packet(&mut stream).unwrap();
        source_rcon_write_packet(&mut stream, 14002, 0, "broadcast accepted").unwrap();
        let terminator = source_rcon_read_packet(&mut stream).unwrap();
        tx.send((
            String::from_utf8(auth.body).unwrap(),
            String::from_utf8(exec.body).unwrap(),
            terminator.id,
        ))
        .unwrap();

        source_rcon_write_packet(&mut stream, 14003, 0, "").unwrap();
    });

    let response = source_rcon_exec(
        &endpoint,
        "test-rcon-password",
        "say LanGame broadcast smoke",
    )
    .unwrap();
    let (password, command, terminator_id) = rx.recv_timeout(Duration::from_secs(3)).unwrap();
    server.join().unwrap();

    assert_eq!(password, "test-rcon-password");
    assert_eq!(command, "say LanGame broadcast smoke");
    assert_eq!(terminator_id, 14003);
    assert_eq!(response, "broadcast accepted");
}

pub(super) fn spawn_source_rcon_capture_server(
    response: &'static str,
) -> (
    u16,
    std::sync::mpsc::Receiver<(String, String, i32)>,
    std::thread::JoinHandle<()>,
) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = std::sync::mpsc::channel();

    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .unwrap();

        let auth = source_rcon_read_packet(&mut stream).unwrap();
        source_rcon_write_packet(&mut stream, 14001, 2, "").unwrap();
        let exec = source_rcon_read_packet(&mut stream).unwrap();
        source_rcon_write_packet(&mut stream, 14002, 0, response).unwrap();
        let terminator = source_rcon_read_packet(&mut stream).unwrap();
        tx.send((
            String::from_utf8(auth.body).unwrap(),
            String::from_utf8(exec.body).unwrap(),
            terminator.id,
        ))
        .unwrap();
        source_rcon_write_packet(&mut stream, 14003, 0, "").unwrap();
    });

    (port, rx, server)
}

pub(super) fn push_server_websocket_frame(
    bytes: &mut Vec<u8>,
    fin: bool,
    opcode: u8,
    payload: &[u8],
) {
    bytes.push(if fin { 0x80 | opcode } else { opcode });
    if payload.len() <= 125 {
        bytes.push(payload.len() as u8);
    } else if payload.len() <= u16::MAX as usize {
        bytes.push(126);
        bytes.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        bytes.push(127);
        bytes.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }
    bytes.extend_from_slice(payload);
}

pub(super) fn write_server_websocket_text_frame<W: Write>(stream: &mut W, text: &str) {
    let mut bytes = Vec::new();
    push_server_websocket_frame(&mut bytes, true, 0x1, text.as_bytes());
    stream.write_all(&bytes).unwrap();
    stream.flush().unwrap();
}

pub(super) fn websocket_capture_password_from_headers(headers: &str) -> String {
    let request_line = headers.lines().next().unwrap_or_default();
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or_default()
        .trim_start_matches('/');
    path.to_string()
}

pub(super) fn spawn_websocket_rcon_capture_server(
    response: &'static str,
) -> (
    u16,
    std::sync::mpsc::Receiver<(String, String)>,
    std::thread::JoinHandle<()>,
) {
    spawn_websocket_rcon_capture_server_commands(vec![response])
}

fn spawn_websocket_rcon_capture_server_commands(
    responses: Vec<&'static str>,
) -> (
    u16,
    std::sync::mpsc::Receiver<(String, String)>,
    std::thread::JoinHandle<()>,
) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = std::sync::mpsc::channel();

    let server = std::thread::spawn(move || {
        for response in responses {
            let mut stream = accept_runtime_transport_test_client(&listener);
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(10)))
                .unwrap();

            let headers = websocket_read_http_headers(&mut stream).unwrap();
            let password = websocket_capture_password_from_headers(&headers);
            stream
                .write_all(
                    b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n",
                )
                .unwrap();
            stream.flush().unwrap();

            let frame = websocket_read_frame(&mut stream).unwrap();
            let request_text = String::from_utf8(frame.payload).unwrap();
            let request_json: Value = serde_json::from_str(&request_text).unwrap();
            let identifier = request_json
                .get("Identifier")
                .and_then(Value::as_i64)
                .unwrap_or(14003);
            let command = request_json
                .get("Message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            tx.send((password, command)).unwrap();
            write_server_websocket_text_frame(
                &mut stream,
                &json!({
                    "Identifier": identifier,
                    "Message": response,
                    "Type": "Generic"
                })
                .to_string(),
            );
        }
    });

    (port, rx, server)
}

fn accept_runtime_transport_test_client(listener: &std::net::TcpListener) -> TcpStream {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                return stream;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "RCON client did not connect");
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("RCON mock accept failed: {error}"),
        }
    }
}

#[test]
pub(super) fn websocket_read_text_message_combines_continuation_frames() {
    let mut bytes = Vec::new();
    push_server_websocket_frame(
        &mut bytes,
        false,
        0x1,
        br#"{"Identifier":14003,"Message":"players "#,
    );
    push_server_websocket_frame(&mut bytes, true, 0x0, br#"continued","Type":"Generic"}"#);

    let mut cursor = std::io::Cursor::new(bytes);
    let text = websocket_read_text_message(&mut cursor).unwrap();

    assert_eq!(
        text,
        r#"{"Identifier":14003,"Message":"players continued","Type":"Generic"}"#
    );
}

#[test]
pub(super) fn websocket_read_text_message_rejects_orphan_continuation_frame() {
    let mut bytes = Vec::new();
    push_server_websocket_frame(&mut bytes, true, 0x0, b"orphan");

    let mut cursor = std::io::Cursor::new(bytes);
    let error =
        websocket_read_text_message(&mut cursor).expect_err("orphan continuation must fail");

    assert!(error.contains("continuation"));
}

#[test]
pub(super) fn telnet_text_from_bytes_strips_iac_negotiation() {
    let text = telnet_text_from_bytes(&[
        255, 251, 1, b'L', b'a', b'n', b'G', b'a', b'm', b'e', b'\n', 255, 250, 31, 0, 80, 0, 24,
        255, 240, b'O', b'K',
    ]);

    assert_eq!(text, "LanGame\nOK");
}

#[test]
fn seven_days_telnet_padding_does_not_invalidate_a_complete_player_response() {
    let text = telnet_text_from_bytes(
        b"Please enter password:\r\n\0\0\0\0\0\0Logon successful.\r\nTotal of 0 in the game\r\n",
    );
    let list = crate::live_players::response_codecs::parse(
        app_core::ModulePlayerListCodec::SevenDaysPlayers,
        &text,
        "native-empty",
        1,
        &[],
    )
    .expect("complete native zero-player reply");
    assert!(list.complete);
    assert_eq!(list.current_players, Some(0));
}

pub(super) fn read_test_line(stream: &mut TcpStream) -> String {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    while stream.read_exact(&mut byte).is_ok() {
        if byte[0] == b'\n' {
            break;
        }
        line.push(byte[0]);
    }
    String::from_utf8_lossy(&line)
        .trim_end_matches('\r')
        .to_string()
}

pub(super) fn spawn_telnet_capture_server(
    response: &'static str,
) -> (
    u16,
    std::sync::mpsc::Receiver<(String, String)>,
    std::thread::JoinHandle<()>,
) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = std::sync::mpsc::channel();

    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .unwrap();

        stream.write_all(b"Password:\n").unwrap();
        stream.flush().unwrap();
        let password = read_test_line(&mut stream);
        stream.write_all(b"Welcome admin\n").unwrap();
        stream.flush().unwrap();
        let command = read_test_line(&mut stream);
        tx.send((password, command)).unwrap();
        stream.write_all(response.as_bytes()).unwrap();
        stream.write_all(b"\n").unwrap();
        stream.flush().unwrap();
    });

    (port, rx, server)
}

#[test]
pub(super) fn telnet_exec_writes_password_and_command() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let (tx, rx) = std::sync::mpsc::channel();

    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();

        stream.write_all(b"Password:\n").unwrap();
        stream.flush().unwrap();
        let password = read_test_line(&mut stream);
        stream.write_all(b"Welcome admin\n").unwrap();
        stream.flush().unwrap();
        let command = read_test_line(&mut stream);
        tx.send((password, command)).unwrap();
        stream.write_all(b"Command queued\n").unwrap();
        stream.flush().unwrap();
    });

    let transcript = telnet_exec(
        &endpoint,
        "test-telnet-password",
        "say LanGame broadcast smoke",
    )
    .unwrap();
    let (password, command) = rx.recv_timeout(Duration::from_secs(3)).unwrap();
    server.join().unwrap();

    assert_eq!(password, "test-telnet-password");
    assert_eq!(command, "say LanGame broadcast smoke");
    assert!(transcript.contains("Password:"));
    assert!(transcript.contains("Welcome admin"));
    assert!(transcript.contains("Command queued"));
}

#[test]
pub(super) fn runtime_command_response_text_marks_truncation() {
    let response =
        runtime_command_response_text("x".repeat(16 * 1024 + 8)).expect("truncated response");

    assert!(response.starts_with(&"x".repeat(16 * 1024)));
    assert!(response.ends_with("[LanGame: response truncated at 16384 characters]"));
}
