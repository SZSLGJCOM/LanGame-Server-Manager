//! Existing Minecraft instances may deliberately disable GameSpy Query. Their
//! game port still exposes Minecraft's read-only status handshake when enabled.
use app_platform_win::ProcessNetworkEndpoint;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const STATUS_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_STATUS_BYTES: usize = 256 * 1024;

pub(super) fn required(
    existing: bool,
    module_id: &str,
    settings_json: &str,
) -> Result<bool, String> {
    if !existing || module_id != "minecraft" {
        return Ok(false);
    }
    let settings: serde_json::Value = serde_json::from_str(settings_json)
        .map_err(|_| "Minecraft readiness settings are not valid JSON")?;
    match settings
        .get("enable_query")
        .and_then(serde_json::Value::as_bool)
    {
        Some(enabled) => Ok(!enabled),
        None => Err("Minecraft readiness requires a boolean enable_query setting".into()),
    }
}

pub(super) fn owns_game_endpoint(endpoints: &[ProcessNetworkEndpoint], port: u16) -> bool {
    // The caller obtains this snapshot from the pinned process tree, including
    // owned descendants. A different process key cannot satisfy this probe.
    endpoints.iter().any(|endpoint| {
        endpoint.process_key == "main"
            && endpoint.local_port == port
            && endpoint.protocol.eq_ignore_ascii_case("tcp")
    })
}

pub(super) fn done_line(line: &str) -> bool {
    let Some((clock, message)) = line
        .trim()
        .strip_prefix('[')
        .and_then(|line| line.split_once(']'))
    else {
        return false;
    };
    let clock = clock.as_bytes();
    if clock.len() != 8
        || !clock.iter().enumerate().all(|(index, byte)| {
            if matches!(index, 2 | 5) {
                *byte == b':'
            } else {
                byte.is_ascii_digit()
            }
        })
    {
        return false;
    }
    message
        .strip_prefix(" [Server thread/INFO]: Done (")
        .and_then(|value| value.strip_suffix("s)! For help, type \"help\""))
        .and_then(|elapsed| elapsed.parse::<f64>().ok())
        .is_some_and(|elapsed| elapsed.is_finite() && elapsed >= 0.0)
}

pub(super) async fn query_status(bind_ip: &str, port: u16) -> Result<(), &'static str> {
    query_with_budget(bind_ip, port, STATUS_TIMEOUT).await
}

async fn query_with_budget(bind_ip: &str, port: u16, budget: Duration) -> Result<(), &'static str> {
    let host = app_storage::normalize_query_host(bind_ip);
    let address: IpAddr = host
        .parse()
        .map_err(|_| "minecraft_status_invalid_binding")?;
    if port == 0 {
        return Err("minecraft_status_invalid_port");
    }
    tokio::time::timeout(budget, exchange(SocketAddr::new(address, port), &host))
        .await
        .map_err(|_| "minecraft_status_timed_out")?
}

async fn exchange(address: SocketAddr, host: &str) -> Result<(), &'static str> {
    let mut stream = TcpStream::connect(address)
        .await
        .map_err(|_| "minecraft_status_connect_failed")?;
    let mut handshake = vec![0]; // Handshake packet ID.
    write_varint(&mut handshake, u32::MAX); // Protocol -1: status discovery, not a game login.
    write_varint(&mut handshake, host.len() as u32);
    handshake.extend_from_slice(host.as_bytes());
    handshake.extend_from_slice(&address.port().to_be_bytes());
    handshake.push(1); // Next state: status.
    let mut request = Vec::new();
    write_varint(&mut request, handshake.len() as u32);
    request.extend(handshake);
    request.extend_from_slice(&[1, 0]); // Length 1, status request packet ID 0.
    stream
        .write_all(&request)
        .await
        .map_err(|_| "minecraft_status_write_failed")?;
    let length = read_varint(&mut stream).await? as usize;
    if length == 0 || length > MAX_STATUS_BYTES {
        return Err("minecraft_status_frame_size_invalid");
    }
    let mut packet = vec![0; length];
    stream
        .read_exact(&mut packet)
        .await
        .map_err(|_| "minecraft_status_frame_incomplete")?;
    validate_response(&packet)
}

fn write_varint(bytes: &mut Vec<u8>, mut value: u32) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        bytes.push(if value == 0 { byte } else { byte | 0x80 });
        if value == 0 {
            break;
        }
    }
}

async fn read_varint(reader: &mut (impl AsyncRead + Unpin)) -> Result<u32, &'static str> {
    let mut value = 0u32;
    for index in 0..5 {
        let byte = reader
            .read_u8()
            .await
            .map_err(|_| "minecraft_status_varint_incomplete")?;
        if index == 4 && byte & 0xf0 != 0 {
            return Err("minecraft_status_varint_invalid");
        }
        value |= u32::from(byte & 0x7f) << (index * 7);
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err("minecraft_status_varint_invalid")
}

fn take_varint(packet: &mut &[u8]) -> Result<u32, &'static str> {
    let mut value = 0u32;
    for index in 0..5 {
        let Some((&byte, remainder)) = packet.split_first() else {
            return Err("minecraft_status_varint_incomplete");
        };
        *packet = remainder;
        if index == 4 && byte & 0xf0 != 0 {
            return Err("minecraft_status_varint_invalid");
        }
        value |= u32::from(byte & 0x7f) << (index * 7);
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err("minecraft_status_varint_invalid")
}

fn validate_response(mut packet: &[u8]) -> Result<(), &'static str> {
    if take_varint(&mut packet)? != 0 {
        return Err("minecraft_status_packet_id_invalid");
    }
    if take_varint(&mut packet)? as usize != packet.len() {
        return Err("minecraft_status_json_length_invalid");
    }
    let status: serde_json::Value =
        serde_json::from_slice(packet).map_err(|_| "minecraft_status_json_invalid")?;
    let valid_count = |value: Option<&serde_json::Value>| {
        value
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|value| value <= i32::MAX as u64)
    };
    if !status
        .pointer("/version/name")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|name| !name.trim().is_empty())
        || !valid_count(status.pointer("/version/protocol"))
        || !valid_count(status.pointer("/players/online"))
        || !valid_count(status.pointer("/players/max"))
        || !status.get("description").is_some_and(|description| {
            description.is_string() || description.is_object() || description.is_array()
        })
    {
        return Err("minecraft_status_fields_invalid");
    }
    Ok(())
}

#[path = "commands_native_minecraft_readiness_tests.rs"]
mod tests;
