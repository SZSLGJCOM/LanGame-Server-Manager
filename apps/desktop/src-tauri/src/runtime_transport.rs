use std::io::{ErrorKind, Read, Write};
use std::net::{ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

#[path = "runtime_transport_deadline.rs"]
mod deadline;
pub(crate) use deadline::DeadlineTcpStream;

#[path = "runtime_transport_conan.rs"]
mod conan;
pub(crate) use conan::conan_rcon_exec;

#[path = "runtime_transport_rcon_delivery.rs"]
mod rcon_delivery;
pub(crate) use rcon_delivery::{RconCommandFailure, source_rcon_shutdown_exec};

#[path = "runtime_transport_seven_days.rs"]
mod seven_days;
pub(crate) use seven_days::seven_days_ban_telnet_exec;

#[path = "runtime_transport_rust.rs"]
mod rust_rcon;

pub(crate) fn source_rcon_exec(
    endpoint: &str,
    password: &str,
    command: &str,
) -> Result<String, String> {
    let mut stream = DeadlineTcpStream::connect(endpoint, Duration::from_secs(3))
        .map_err(|error| format!("failed to connect to RCON `{endpoint}`: {error}"))?;

    source_rcon_exchange(
        &mut stream,
        password,
        command,
        SourceRconCompletion::ResponseValue,
    )
}

pub(crate) fn player_list_rcon_exec(
    endpoint: &str,
    password: &str,
    command: &str,
) -> Result<String, String> {
    let mut stream = DeadlineTcpStream::connect(endpoint, Duration::from_secs(3))
        .map_err(|error| format!("failed to connect to RCON `{endpoint}`: {error}"))?;
    source_rcon_exchange(
        &mut stream,
        password,
        command,
        SourceRconCompletion::PlayerList,
    )
}

pub(crate) fn rust_rcon_exec(
    endpoint: &str,
    password: &str,
    command: &str,
) -> Result<String, String> {
    let mut stream = DeadlineTcpStream::connect(endpoint, Duration::from_secs(3))
        .map_err(|error| format!("failed to connect to RCON `{endpoint}`: {error}"))?;
    source_rcon_exchange(&mut stream, password, command, SourceRconCompletion::Rust)
}

#[derive(Clone, Copy)]
enum SourceRconCompletion {
    ResponseValue,
    PlayerList,
    Rust,
}

fn source_rcon_exchange<S: Read + Write>(
    stream: &mut S,
    password: &str,
    command: &str,
    completion: SourceRconCompletion,
) -> Result<String, String> {
    source_rcon_exchange_with_delivery(stream, password, command, completion, &mut false)
}

fn source_rcon_exchange_with_delivery<S: Read + Write>(
    stream: &mut S,
    password: &str,
    command: &str,
    completion: SourceRconCompletion,
    command_may_have_been_sent: &mut bool,
) -> Result<String, String> {
    const AUTH_ID: i32 = 14001;
    const EXEC_ID: i32 = 14002;
    source_rcon_write_packet(stream, AUTH_ID, 3, password)?;

    let mut authenticated = false;
    for _ in 0..3 {
        let packet = source_rcon_read_packet(stream)
            .map_err(|error| format!("RCON authentication read failed: {error}"))?;
        if packet.id == -1 {
            return Err(String::from("RCON authentication failed"));
        }
        if packet.id == AUTH_ID && packet.packet_type == 2 {
            authenticated = true;
            break;
        }
    }
    if !authenticated {
        return Err(String::from(
            "RCON authentication response was not received",
        ));
    }

    // A partial write can already deliver the command. Mark before attempting
    // I/O, never only after a successful write or response.
    *command_may_have_been_sent = true;
    source_rcon_write_packet(stream, EXEC_ID, 2, command)?;
    let (marker_type, marker_command) = match completion {
        SourceRconCompletion::Rust => return rust_rcon::read_command_response(stream, EXEC_ID),
        SourceRconCompletion::ResponseValue => (0, ""),
        // ARK and Squad ignore empty response-value requests. A separate
        // read-only command supplies their ordered response boundary.
        SourceRconCompletion::PlayerList => (2, "ListPlayers"),
    };
    // Minecraft treats one socket read as one request. Wait for its first reply
    // before sending the marker so the two requests cannot be read together.
    let mut response = None;
    for _ in 0..64 {
        let packet = source_rcon_read_packet(stream)
            .map_err(|error| format!("RCON command response read failed: {error}"))?;
        if packet.id == -1 {
            return Err(String::from("RCON command was rejected"));
        }
        if packet.id == EXEC_ID {
            if packet.packet_type != 0 {
                return Err(String::from(
                    "RCON command returned an invalid response type",
                ));
            }
            response = Some(packet.body);
            break;
        }
    }
    let response =
        response.ok_or_else(|| String::from("RCON command response was not received"))?;
    const TERMINATOR_ID: i32 = 14003;
    source_rcon_write_packet(stream, TERMINATOR_ID, marker_type, marker_command)?;
    source_rcon_read_command_response(stream, EXEC_ID, TERMINATOR_ID, response)
}

pub(crate) struct SourceRconPacket {
    pub(crate) id: i32,
    pub(crate) packet_type: i32,
    // A response may split a UTF-8 code point across frames. Decode only after
    // every command-response fragment has arrived, never by replacing bytes.
    pub(crate) body: Vec<u8>,
}

pub(crate) fn source_rcon_write_packet<W: Write>(
    stream: &mut W,
    request_id: i32,
    packet_type: i32,
    body: &str,
) -> Result<(), String> {
    let body_bytes = body.as_bytes();
    let size = i32::try_from(10usize.saturating_add(body_bytes.len()))
        .map_err(|_| String::from("RCON command is too large"))?;
    // Submit the complete frame together; Minecraft closes a connection when
    // a read contains only the size/header written by separate socket writes.
    let mut packet = Vec::with_capacity(body_bytes.len() + 14);
    packet.extend_from_slice(&size.to_le_bytes());
    packet.extend_from_slice(&request_id.to_le_bytes());
    packet.extend_from_slice(&packet_type.to_le_bytes());
    packet.extend_from_slice(body_bytes);
    packet.extend_from_slice(&[0, 0]);
    stream
        .write_all(&packet)
        .and_then(|_| stream.flush())
        .map_err(|error| format!("failed to write RCON packet: {error}"))
}

pub(crate) fn source_rcon_read_packet<R: Read>(stream: &mut R) -> Result<SourceRconPacket, String> {
    let mut size_bytes = [0u8; 4];
    stream
        .read_exact(&mut size_bytes)
        .map_err(|error| format!("failed to read RCON packet size: {error}"))?;
    let size = i32::from_le_bytes(size_bytes);
    // Minecraft chunks response text at 4096 characters before UTF-8 encoding.
    const MAX_PACKET_SIZE: i32 = 4096 * 4 + 10;
    if !(10..=MAX_PACKET_SIZE).contains(&size) {
        return Err(format!("invalid RCON packet size {size}"));
    }

    let mut payload = vec![0u8; usize::try_from(size).unwrap_or(0)];
    stream
        .read_exact(&mut payload)
        .map_err(|error| format!("failed to read RCON packet body: {error}"))?;
    if payload.len() < 10 {
        return Err(String::from("RCON packet body is too short"));
    }
    if !payload.ends_with(&[0, 0]) {
        return Err(String::from(
            "RCON packet is missing its double-NUL terminator",
        ));
    }

    let id = i32::from_le_bytes(payload[0..4].try_into().unwrap_or_default());
    let packet_type = i32::from_le_bytes(payload[4..8].try_into().unwrap_or_default());
    let body = payload[8..payload.len() - 2].to_vec();
    Ok(SourceRconPacket {
        id,
        packet_type,
        body,
    })
}

#[cfg(test)]
#[path = "runtime_transport_rcon_tests.rs"]
mod source_rcon_tests;

#[cfg(test)]
#[path = "runtime_transport_rcon_delivery_tests.rs"]
mod rcon_delivery_tests;

pub(crate) fn source_rcon_read_command_response<R: Read>(
    stream: &mut R,
    exec_id: i32,
    terminator_id: i32,
    mut response: Vec<u8>,
) -> Result<String, String> {
    for _ in 0..64 {
        let packet = source_rcon_read_packet(stream)
            .map_err(|error| format!("RCON completion read failed: {error}"))?;
        if packet.id == -1 {
            return Err(String::from("RCON command was rejected"));
        }
        if packet.id == terminator_id {
            if packet.packet_type != 0 {
                return Err(String::from(
                    "RCON completion returned an invalid response type",
                ));
            }
            return String::from_utf8(response)
                .map_err(|_| String::from("RCON command response contains invalid UTF-8"));
        }
        if packet.id == exec_id {
            if packet.packet_type != 0 {
                return Err(String::from(
                    "RCON command returned an invalid response type",
                ));
            }
            response.extend_from_slice(&packet.body);
        }
    }

    Err(String::from(
        "RCON command response terminator was not received",
    ))
}

pub(crate) fn websocket_rcon_exec(
    endpoint: &str,
    password: &str,
    command: &str,
) -> Result<String, String> {
    let mut stream = DeadlineTcpStream::connect(endpoint, Duration::from_secs(3))
        .map_err(|error| format!("failed to connect to WebSocket RCON `{endpoint}`: {error}"))?;

    websocket_rcon_handshake(&mut stream, endpoint, password)?;

    const REQUEST_ID: i64 = 14003;
    let request = json!({
        "Identifier": REQUEST_ID,
        "Message": command,
        "Name": "LanGame",
    })
    .to_string();
    websocket_write_frame(&mut stream, 0x1, request.as_bytes())?;

    for _ in 0..8 {
        let response_text = websocket_read_text_message(&mut stream)?;
        let Ok(response_json) = serde_json::from_str::<Value>(&response_text) else {
            continue;
        };
        let identifier_matches = response_json
            .get("Identifier")
            .and_then(Value::as_i64)
            .map(|value| value == REQUEST_ID)
            .unwrap_or_else(|| {
                response_json
                    .get("Identifier")
                    .and_then(Value::as_u64)
                    .map(|value| value == REQUEST_ID as u64)
                    .unwrap_or(false)
            });
        if !identifier_matches {
            continue;
        }

        let message = response_json
            .get("Message")
            .and_then(Value::as_str)
            .unwrap_or(response_text.as_str())
            .to_string();
        if response_json
            .get("Type")
            .and_then(Value::as_str)
            .map(|value| value.eq_ignore_ascii_case("error"))
            .unwrap_or(false)
        {
            return Err(format!("WebSocket RCON command failed: {message}"));
        }
        return Ok(message);
    }

    Err(String::from(
        "WebSocket RCON command response was not received",
    ))
}

fn websocket_rcon_handshake<S: Read + Write>(
    stream: &mut S,
    endpoint: &str,
    password: &str,
) -> Result<(), String> {
    let key = websocket_client_key();
    let password_path = websocket_rcon_path_segment(password);
    let request = format!(
        "GET /{password_path} HTTP/1.1\r\n\
         Host: {endpoint}\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: {key}\r\n\
         Sec-WebSocket-Version: 13\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .and_then(|_| stream.flush())
        .map_err(|error| format!("failed to write WebSocket RCON handshake: {error}"))?;

    let headers = websocket_read_http_headers(stream)?;
    let status_line = headers.lines().next().unwrap_or_default();
    if !status_line.contains(" 101 ") {
        return Err(format!(
            "WebSocket RCON handshake failed with `{status_line}`"
        ));
    }
    if !headers.to_ascii_lowercase().contains("upgrade: websocket") {
        return Err(String::from(
            "WebSocket RCON handshake response did not upgrade the connection",
        ));
    }
    Ok(())
}

pub(crate) fn websocket_read_http_headers<R: Read>(stream: &mut R) -> Result<String, String> {
    let mut bytes = Vec::with_capacity(512);
    let mut next = [0u8; 1];
    while bytes.len() < 8192 {
        stream
            .read_exact(&mut next)
            .map_err(|error| format!("failed to read WebSocket RCON handshake: {error}"))?;
        bytes.push(next[0]);
        if bytes.ends_with(b"\r\n\r\n") {
            return Ok(String::from_utf8_lossy(&bytes).into_owned());
        }
    }
    Err(String::from(
        "WebSocket RCON handshake response headers were too large",
    ))
}

pub(crate) fn websocket_read_text_message<S: Read + Write>(
    stream: &mut S,
) -> Result<String, String> {
    const MAX_MESSAGE_BYTES: usize = 256 * 1024;
    let mut fragmented_opcode: Option<u8> = None;
    let mut fragmented_payload = Vec::new();

    for _ in 0..1024 {
        let frame = websocket_read_frame(stream)?;
        match frame.opcode {
            0x1 => {
                if fragmented_opcode.is_some() {
                    return Err(String::from(
                        "WebSocket RCON started a new message before finishing the previous fragment",
                    ));
                }
                if frame.fin {
                    return websocket_payload_to_text(frame.opcode, frame.payload);
                }
                fragmented_opcode = Some(frame.opcode);
                fragmented_payload.extend_from_slice(&frame.payload);
            }
            0x2 => {
                if fragmented_opcode.is_some() {
                    return Err(String::from(
                        "WebSocket RCON started a new message before finishing the previous fragment",
                    ));
                }
                if frame.fin {
                    return websocket_payload_to_text(frame.opcode, frame.payload);
                }
                fragmented_opcode = Some(frame.opcode);
                fragmented_payload.extend_from_slice(&frame.payload);
            }
            0x0 => {
                let Some(opcode) = fragmented_opcode else {
                    return Err(String::from(
                        "WebSocket RCON returned a continuation frame without a message start",
                    ));
                };
                fragmented_payload.extend_from_slice(&frame.payload);
                if fragmented_payload.len() > MAX_MESSAGE_BYTES {
                    return Err(String::from(
                        "WebSocket RCON response message exceeded 256 KiB",
                    ));
                }
                if frame.fin {
                    return websocket_payload_to_text(
                        opcode,
                        std::mem::take(&mut fragmented_payload),
                    );
                }
            }
            0x8 => return Err(String::from("WebSocket RCON connection closed")),
            0x9 => {
                websocket_write_frame(stream, 0xA, &frame.payload)?;
            }
            0xA => {}
            _ => {}
        }
    }
    Err(String::from(
        "WebSocket RCON response exceeded the frame limit",
    ))
}

pub(crate) struct WebSocketFrame {
    pub(crate) fin: bool,
    pub(crate) opcode: u8,
    pub(crate) payload: Vec<u8>,
}

fn websocket_payload_to_text(opcode: u8, payload: Vec<u8>) -> Result<String, String> {
    match opcode {
        0x1 => String::from_utf8(payload)
            .map_err(|error| format!("WebSocket RCON returned invalid UTF-8: {error}")),
        0x2 => Ok(String::from_utf8_lossy(&payload).into_owned()),
        _ => Err(format!(
            "WebSocket RCON returned unsupported message opcode {opcode}"
        )),
    }
}

pub(crate) fn websocket_read_frame<R: Read>(stream: &mut R) -> Result<WebSocketFrame, String> {
    let mut header = [0u8; 2];
    stream
        .read_exact(&mut header)
        .map_err(|error| format!("failed to read WebSocket RCON frame header: {error}"))?;
    let fin = (header[0] & 0x80) != 0;
    let opcode = header[0] & 0x0F;
    let masked = (header[1] & 0x80) != 0;
    let mut payload_len = u64::from(header[1] & 0x7F);
    if payload_len == 126 {
        let mut len_bytes = [0u8; 2];
        stream
            .read_exact(&mut len_bytes)
            .map_err(|error| format!("failed to read WebSocket RCON frame length: {error}"))?;
        payload_len = u64::from(u16::from_be_bytes(len_bytes));
    } else if payload_len == 127 {
        let mut len_bytes = [0u8; 8];
        stream
            .read_exact(&mut len_bytes)
            .map_err(|error| format!("failed to read WebSocket RCON frame length: {error}"))?;
        payload_len = u64::from_be_bytes(len_bytes);
    }
    if payload_len > 64 * 1024 {
        return Err(String::from(
            "WebSocket RCON response frame exceeded 64 KiB",
        ));
    }

    let mask = if masked {
        let mut mask = [0u8; 4];
        stream
            .read_exact(&mut mask)
            .map_err(|error| format!("failed to read WebSocket RCON frame mask: {error}"))?;
        Some(mask)
    } else {
        None
    };

    let mut payload = vec![0u8; payload_len as usize];
    stream
        .read_exact(&mut payload)
        .map_err(|error| format!("failed to read WebSocket RCON frame payload: {error}"))?;
    if let Some(mask) = mask {
        for (index, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[index % 4];
        }
    }

    Ok(WebSocketFrame {
        fin,
        opcode,
        payload,
    })
}

fn websocket_write_frame<W: Write>(
    stream: &mut W,
    opcode: u8,
    payload: &[u8],
) -> Result<(), String> {
    let mut frame = Vec::with_capacity(payload.len().saturating_add(16));
    frame.push(0x80 | (opcode & 0x0F));
    if payload.len() <= 125 {
        frame.push(0x80 | payload.len() as u8);
    } else if payload.len() <= u16::MAX as usize {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        frame.push(0x80 | 127);
        frame.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }

    let mask = websocket_client_mask();
    frame.extend_from_slice(&mask);
    for (index, byte) in payload.iter().enumerate() {
        frame.push(*byte ^ mask[index % 4]);
    }

    stream
        .write_all(&frame)
        .and_then(|_| stream.flush())
        .map_err(|error| format!("failed to write WebSocket RCON frame: {error}"))
}

fn websocket_client_key() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&(nanos as u64).to_be_bytes());
    bytes[8..].copy_from_slice(&((nanos >> 64) as u64 ^ 0x4C_61_6E_47_61_6D_65).to_be_bytes());
    base64_encode(&bytes)
}

fn websocket_client_mask() -> [u8; 4] {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u32)
        .unwrap_or(0);
    nanos.to_be_bytes()
}

fn websocket_rcon_path_segment(password: &str) -> String {
    let mut encoded = String::with_capacity(password.len());
    for byte in password.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = *chunk.get(1).unwrap_or(&0);
        let third = *chunk.get(2).unwrap_or(&0);
        let combined = ((first as u32) << 16) | ((second as u32) << 8) | third as u32;
        encoded.push(TABLE[((combined >> 18) & 0x3F) as usize] as char);
        encoded.push(TABLE[((combined >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            encoded.push(TABLE[((combined >> 6) & 0x3F) as usize] as char);
        } else {
            encoded.push('=');
        }
        if chunk.len() > 2 {
            encoded.push(TABLE[(combined & 0x3F) as usize] as char);
        } else {
            encoded.push('=');
        }
    }
    encoded
}

pub(crate) fn battleye_rcon_exec(
    endpoint: &str,
    password: &str,
    command: &str,
) -> Result<String, String> {
    let address = endpoint
        .to_socket_addrs()
        .map_err(|error| format!("failed to resolve BattlEye RCON endpoint `{endpoint}`: {error}"))?
        .next()
        .ok_or_else(|| format!("BattlEye RCON endpoint `{endpoint}` did not resolve"))?;
    let bind_addr = if address.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let socket = UdpSocket::bind(bind_addr)
        .map_err(|error| format!("failed to bind BattlEye RCON UDP socket: {error}"))?;
    socket
        .connect(address)
        .map_err(|error| format!("failed to connect BattlEye RCON `{endpoint}`: {error}"))?;
    socket
        .set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|error| format!("failed to set BattlEye RCON read timeout: {error}"))?;
    socket
        .set_write_timeout(Some(Duration::from_secs(3)))
        .map_err(|error| format!("failed to set BattlEye RCON write timeout: {error}"))?;

    battleye_rcon_send_payload(&socket, &battleye_rcon_login_payload(password))?;
    let login = battleye_rcon_read_packet(&socket)?;
    if login.payload.first().copied() != Some(0x00) || login.payload.get(1).copied() != Some(0x01) {
        return Err(String::from("BattlEye RCON authentication failed"));
    }

    const COMMAND_SEQUENCE: u8 = 0;
    battleye_rcon_send_payload(
        &socket,
        &battleye_rcon_command_payload(COMMAND_SEQUENCE, command),
    )?;
    battleye_rcon_read_command_response(&socket, COMMAND_SEQUENCE)
}

pub(crate) struct BattleyeRconPacket {
    pub(crate) payload: Vec<u8>,
}

fn battleye_rcon_login_payload(password: &str) -> Vec<u8> {
    let mut payload = Vec::with_capacity(1 + password.len());
    payload.push(0x00);
    payload.extend_from_slice(password.as_bytes());
    payload
}

fn battleye_rcon_command_payload(sequence: u8, command: &str) -> Vec<u8> {
    let mut payload = Vec::with_capacity(2 + command.len());
    payload.push(0x01);
    payload.push(sequence);
    payload.extend_from_slice(command.as_bytes());
    payload
}

fn battleye_rcon_server_message_ack_payload(sequence: u8) -> [u8; 2] {
    [0x02, sequence]
}

fn battleye_rcon_send_payload(socket: &UdpSocket, payload: &[u8]) -> Result<(), String> {
    let packet = battleye_rcon_wrap_payload(payload);
    socket
        .send(&packet)
        .map_err(|error| format!("failed to send BattlEye RCON packet: {error}"))?;
    Ok(())
}

pub(crate) fn battleye_rcon_wrap_payload(payload: &[u8]) -> Vec<u8> {
    let checksum = crc32_ieee(payload);
    let mut packet = Vec::with_capacity(7 + payload.len());
    packet.extend_from_slice(b"BE");
    packet.extend_from_slice(&checksum.to_le_bytes());
    packet.push(0xFF);
    packet.extend_from_slice(payload);
    packet
}

pub(crate) fn battleye_rcon_read_packet(socket: &UdpSocket) -> Result<BattleyeRconPacket, String> {
    let mut buffer = [0u8; 65535];
    let len = socket
        .recv(&mut buffer)
        .map_err(|error| format!("failed to receive BattlEye RCON packet: {error}"))?;
    if len < 8 {
        return Err(String::from("BattlEye RCON packet is too short"));
    }
    if &buffer[0..2] != b"BE" || buffer[6] != 0xFF {
        return Err(String::from("BattlEye RCON packet header is invalid"));
    }
    let checksum = u32::from_le_bytes(buffer[2..6].try_into().unwrap_or_default());
    let payload = buffer[7..len].to_vec();
    let actual = crc32_ieee(&payload);
    if checksum != actual {
        return Err(String::from("BattlEye RCON packet checksum did not match"));
    }
    Ok(BattleyeRconPacket { payload })
}

pub(crate) fn battleye_rcon_read_command_response(
    socket: &UdpSocket,
    expected_sequence: u8,
) -> Result<String, String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut fragments: Option<(usize, Vec<Option<Vec<u8>>>)> = None;

    while Instant::now() < deadline {
        let packet = match battleye_rcon_read_packet(socket) {
            Ok(packet) => packet,
            Err(error) if battleye_rcon_error_is_timeout(&error) => break,
            Err(error) => return Err(error),
        };

        match packet.payload.first().copied() {
            Some(0x01) => {
                if packet.payload.get(1).copied() != Some(expected_sequence) {
                    continue;
                }
                let body = &packet.payload[2..];
                if body.first().copied() == Some(0x00) && body.len() >= 3 {
                    let total = usize::from(body[1]);
                    let index = usize::from(body[2]);
                    if total == 0 || index >= total {
                        return Err(String::from(
                            "BattlEye RCON fragmented response header is invalid",
                        ));
                    }
                    let (_, slots) = fragments.get_or_insert_with(|| (total, vec![None; total]));
                    if slots.len() != total {
                        return Err(String::from(
                            "BattlEye RCON fragmented response changed packet count",
                        ));
                    }
                    slots[index] = Some(body[3..].to_vec());
                    if slots.iter().all(Option::is_some) {
                        let mut combined = Vec::new();
                        for slot in slots.iter_mut() {
                            if let Some(part) = slot.take() {
                                combined.extend_from_slice(&part);
                            }
                        }
                        return Ok(String::from_utf8_lossy(&combined).into_owned());
                    }
                    continue;
                }
                return Ok(String::from_utf8_lossy(body).into_owned());
            }
            Some(0x02) => {
                if let Some(sequence) = packet.payload.get(1).copied() {
                    battleye_rcon_send_payload(
                        socket,
                        &battleye_rcon_server_message_ack_payload(sequence),
                    )?;
                }
            }
            _ => {}
        }
    }

    Err(String::from(
        "BattlEye RCON command response was not received",
    ))
}

fn battleye_rcon_error_is_timeout(error: &str) -> bool {
    let normalized = error.to_ascii_lowercase();
    normalized.contains("timed out")
        || normalized.contains("would block")
        || normalized.contains("wouldblock")
}

pub(crate) fn crc32_ieee(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

pub(crate) fn telnet_exec(endpoint: &str, password: &str, command: &str) -> Result<String, String> {
    let mut stream = DeadlineTcpStream::connect(endpoint, Duration::from_millis(750))
        .map_err(|error| format!("failed to connect to Telnet `{endpoint}`: {error}"))?;

    let mut transcript = String::new();
    transcript.push_str(&telnet_read_available(&mut stream, 4096)?);

    if !password.is_empty() {
        telnet_write_line(&mut stream, password, "password")?;
        stream
            .pause(Duration::from_millis(150))
            .map_err(|error| format!("Telnet authentication deadline: {error}"))?;
        let auth_response = telnet_read_available(&mut stream, 4096)?;
        if telnet_response_looks_auth_failed(&auth_response) {
            return Err(String::from("Telnet authentication failed"));
        }
        transcript.push_str(&auth_response);
    }

    telnet_write_line(&mut stream, command, "command")?;
    stream
        .pause(Duration::from_millis(250))
        .map_err(|error| format!("Telnet command deadline: {error}"))?;
    transcript.push_str(&telnet_read_available(&mut stream, 65_536)?);
    let _ = stream.shutdown();
    Ok(transcript)
}

fn telnet_write_line(
    stream: &mut DeadlineTcpStream,
    line: &str,
    label: &str,
) -> Result<(), String> {
    stream
        .write_all(line.as_bytes())
        .and_then(|_| stream.write_all(b"\n"))
        .and_then(|_| stream.flush())
        .map_err(|error| format!("failed to write Telnet {label}: {error}"))
}

fn telnet_read_available(
    stream: &mut DeadlineTcpStream,
    max_bytes: usize,
) -> Result<String, String> {
    let mut output = Vec::new();
    let mut buffer = [0u8; 1024];

    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(read_count) => {
                output.extend_from_slice(&buffer[..read_count]);
                if output.len() > max_bytes {
                    return Err(String::from("Telnet response exceeds the collection limit"));
                }
            }
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                stream
                    .check_deadline()
                    .map_err(|error| format!("Telnet response deadline: {error}"))?;
                break;
            }
            Err(error) => return Err(format!("failed to read Telnet response: {error}")),
        }
    }

    Ok(telnet_text_from_bytes(&output))
}

pub(crate) fn telnet_text_from_bytes(bytes: &[u8]) -> String {
    let mut text = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        // Telnet NVT padding, including the native 7DTD login prompt, is not text.
        if bytes[index] == 0 {
            index += 1;
            continue;
        }
        if bytes[index] == 255 {
            index = index.saturating_add(telnet_iac_sequence_len(bytes, index));
            continue;
        }
        text.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&text).into_owned()
}

fn telnet_iac_sequence_len(bytes: &[u8], index: usize) -> usize {
    let Some(command) = bytes.get(index + 1).copied() else {
        return 1;
    };
    match command {
        250 => bytes[index + 2..]
            .iter()
            .position(|byte| *byte == 240)
            .map(|position| position + 3)
            .unwrap_or_else(|| bytes.len().saturating_sub(index)),
        251..=254 => 3.min(bytes.len().saturating_sub(index)),
        _ => 2.min(bytes.len().saturating_sub(index)),
    }
}

fn telnet_response_looks_auth_failed(response: &str) -> bool {
    let normalized = response.to_ascii_lowercase();
    normalized.contains("authentication failed")
        || normalized.contains("password incorrect")
        || normalized.contains("wrong password")
        || normalized.contains("login failed")
}

pub(crate) fn runtime_command_response_text(response: String) -> Option<String> {
    let trimmed = response.trim();
    if trimmed.is_empty() {
        return None;
    }
    const LIMIT: usize = 16 * 1024;
    let mut chars = trimmed.chars();
    let mut output = chars.by_ref().take(LIMIT).collect::<String>();
    if chars.next().is_some() {
        output.push_str("\n[LanGame: response truncated at 16384 characters]");
    }
    Some(output)
}
