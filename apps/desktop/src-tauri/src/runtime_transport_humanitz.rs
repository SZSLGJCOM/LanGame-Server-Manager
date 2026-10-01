use std::io::{Read, Write};
use std::time::Duration;

use crate::live_players::response_codecs::humanitz::response_is_complete;
use crate::runtime_transport::{DeadlineTcpStream, source_rcon_write_packet};

const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_FRAMES: usize = 64;
const TOTAL_TIMEOUT: Duration = Duration::from_secs(12);
const AUTH_ID: i32 = 1;
const INFO_ID: i32 = 2;

pub(crate) fn humanitz_rcon_info(
    endpoint: &str,
    password: &str,
    command: &str,
) -> Result<String, String> {
    if !command.eq_ignore_ascii_case("info") {
        return Err(String::from(
            "The HumanitZ player transport only supports the read-only info command.",
        ));
    }
    if password.is_empty() || password.len() > 4096 || password.contains('\0') {
        return Err(String::from(
            "The HumanitZ RCON password is empty or exceeds its protocol limits.",
        ));
    }
    let mut stream = DeadlineTcpStream::connect_with_budget(endpoint, TOTAL_TIMEOUT, TOTAL_TIMEOUT)
        .map_err(|error| format!("Unable to connect to HumanitZ RCON: {error}"))?;
    exchange(&mut stream, password)
}

fn exchange<S: Read + Write>(stream: &mut S, password: &str) -> Result<String, String> {
    source_rcon_write_packet(stream, AUTH_ID, 3, password)?;
    let mut authenticated = false;
    for _ in 0..3 {
        let packet = read_packet(stream)?;
        if packet.id == -1 {
            return Err(String::from("HumanitZ RCON authentication failed."));
        }
        if packet.packet_type == 2 && matches!(packet.id, 0 | AUTH_ID) {
            authenticated = true;
            break;
        }
        if packet.packet_type != 0 || !matches!(packet.id, 0 | AUTH_ID) {
            return Err(String::from(
                "HumanitZ RCON returned an invalid authentication response.",
            ));
        }
    }
    if !authenticated {
        return Err(String::from(
            "HumanitZ RCON did not confirm authentication.",
        ));
    }
    source_rcon_write_packet(stream, INFO_ID, 2, "info")?;
    let mut response = String::new();
    for _ in 0..MAX_FRAMES {
        let packet = read_packet(stream)?;
        if packet.packet_type != 0 || !matches!(packet.id, 0 | INFO_ID) {
            return Err(String::from(
                "HumanitZ RCON returned an invalid info response frame.",
            ));
        }
        if response.len().saturating_add(packet.body.len()) > MAX_RESPONSE_BYTES {
            return Err(String::from(
                "HumanitZ player response exceeds the capture limit.",
            ));
        }
        response.push_str(&packet.body);
        if response_is_complete(&response)? {
            return Ok(response);
        }
    }
    Err(String::from(
        "HumanitZ did not return a complete info response within the frame limit.",
    ))
}

struct Packet {
    id: i32,
    packet_type: i32,
    body: String,
}

fn read_packet<R: Read>(stream: &mut R) -> Result<Packet, String> {
    let mut length = [0_u8; 4];
    stream
        .read_exact(&mut length)
        .map_err(|error| format!("Unable to read HumanitZ RCON frame length: {error}"))?;
    let length = i32::from_le_bytes(length);
    if !(10..=MAX_RESPONSE_BYTES as i32 + 10).contains(&length) {
        return Err(String::from(
            "HumanitZ RCON returned an invalid frame length.",
        ));
    }
    let mut frame = vec![0_u8; length as usize];
    stream
        .read_exact(&mut frame)
        .map_err(|error| format!("Unable to read HumanitZ RCON frame: {error}"))?;
    if !frame.ends_with(&[0, 0]) {
        return Err(String::from(
            "HumanitZ RCON returned a frame without its terminators.",
        ));
    }
    let id = i32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]);
    let packet_type = i32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
    let body = std::str::from_utf8(&frame[8..frame.len() - 2])
        .map_err(|_| String::from("HumanitZ RCON returned invalid UTF-8."))?;
    if body.contains('\0') {
        return Err(String::from("HumanitZ RCON returned embedded NUL data."));
    }
    Ok(Packet {
        id,
        packet_type,
        body: body.to_owned(),
    })
}

#[cfg(test)]
#[path = "runtime_transport_humanitz_tests.rs"]
mod tests;
