use std::io::{Read, Write};
use std::time::Duration;

use crate::live_players::response_codecs::humanitz::{
    info_player_count, players_response_is_complete, response_is_complete,
};
use crate::runtime_transport::{DeadlineTcpStream, source_rcon_write_packet};

#[path = "runtime_transport_humanitz_save.rs"]
mod save;
pub(crate) use save::humanitz_rcon_save;

const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_FRAMES: usize = 64;
const TOTAL_TIMEOUT: Duration = Duration::from_secs(12);
const AUTH_ID: i32 = 1;
const INFO_ID: i32 = 2;
const PLAYERS_ID: i32 = 3;

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
    let mut stream = connect(endpoint, password)?;
    exchange(&mut stream, password)
}

pub(crate) fn humanitz_rcon_players(
    endpoint: &str,
    password: &str,
    command: &str,
) -> Result<String, String> {
    if command != "Players" {
        return Err(String::from(
            "The HumanitZ player transport only supports Players.",
        ));
    }
    let mut stream = connect(endpoint, password)?;
    authenticate(&mut stream, password)?;
    players_exchange(&mut stream)
}

pub(crate) fn humanitz_rcon_exec(
    endpoint: &str,
    password: &str,
    command: &str,
) -> Result<String, String> {
    match command {
        "save" => return humanitz_rcon_save(endpoint, password, command),
        "info" => return humanitz_rcon_info(endpoint, password, command),
        "Players" => {
            let response = humanitz_rcon_players(endpoint, password, command)?;
            let response: serde_json::Value = serde_json::from_str(&response)
                .map_err(|error| format!("Invalid HumanitZ player capture: {error}"))?;
            return response["players"]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| String::from("HumanitZ player capture is missing Players."));
        }
        "fetchbanned" => {}
        _ => {
            let valid = command.split_once(' ').is_some_and(|(verb, target)| {
                matches!(verb, "kick" | "ban" | "unban") && app_core::is_humanitz_net_id(target)
            });
            if !valid {
                return Err(String::from(
                    "Unsupported HumanitZ command or invalid complete NetID.",
                ));
            }
        }
    }
    let mut stream = connect(endpoint, password)?;
    authenticate(&mut stream, password)?;
    command_exchange(&mut stream, command)
}

fn connect(endpoint: &str, password: &str) -> Result<DeadlineTcpStream, String> {
    if password.is_empty() || password.len() > 4096 || password.contains('\0') {
        return Err(String::from(
            "The HumanitZ RCON password is empty or exceeds its protocol limits.",
        ));
    }
    DeadlineTcpStream::connect_with_budget(endpoint, TOTAL_TIMEOUT, TOTAL_TIMEOUT)
        .map_err(|error| format!("Unable to connect to HumanitZ RCON: {error}"))
}

fn exchange<S: Read + Write>(stream: &mut S, password: &str) -> Result<String, String> {
    authenticate(stream, password)?;
    info_exchange(stream)
}

fn authenticate<S: Read + Write>(stream: &mut S, password: &str) -> Result<(), String> {
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
    Ok(())
}

fn info_exchange<S: Read + Write>(stream: &mut S) -> Result<String, String> {
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

fn players_exchange<S: Read + Write>(stream: &mut S) -> Result<String, String> {
    let info = info_exchange(stream)?;
    let count = info_player_count(&info)?
        .ok_or_else(|| String::from("HumanitZ info response is incomplete."))?;
    let mut players = String::new();
    if count != 0 {
        source_rcon_write_packet(stream, PLAYERS_ID, 2, "Players")?;
        for _ in 0..MAX_FRAMES {
            let packet = read_packet(stream)?;
            if packet.packet_type != 0 || !matches!(packet.id, 0 | PLAYERS_ID) {
                return Err(String::from(
                    "HumanitZ returned an invalid Players response frame.",
                ));
            }
            if players.len().saturating_add(packet.body.len()) > MAX_RESPONSE_BYTES {
                return Err(String::from(
                    "HumanitZ player response exceeds the capture limit.",
                ));
            }
            players.push_str(&packet.body);
            if players_response_is_complete(&players, count)? {
                return Ok(serde_json::json!({ "info": info, "players": players }).to_string());
            }
        }
        return Err(String::from(
            "HumanitZ Players response did not reach its declared count.",
        ));
    }
    // info already establishes an explicit zero. No undocumented Players empty
    // marker or timeout is interpreted as an empty actionable snapshot.
    Ok(serde_json::json!({ "info": info, "players": players }).to_string())
}

fn command_exchange<S: Read + Write>(stream: &mut S, command: &str) -> Result<String, String> {
    source_rcon_write_packet(stream, INFO_ID, 2, command).map_err(|error| {
        format!("HumanitZ command delivery is unconfirmed and was not retried: {error}")
    })?;
    let packet = read_packet(stream).map_err(|error| {
        format!("HumanitZ command acknowledgement is unconfirmed and was not retried: {error}")
    })?;
    // Native commands use fixed response IDs (0 for queries, 1 for command
    // replies), rather than Source's echoed ID/empty-command completion marker.
    // This receipt acknowledges a response, not the requested game side effect.
    if packet.packet_type != 0
        || !matches!(packet.id, 0 | 1 | INFO_ID)
        || (command != "fetchbanned" && packet.body.trim().is_empty())
    {
        return Err(String::from(
            "HumanitZ returned an invalid command acknowledgement; the command was not retried.",
        ));
    }
    Ok(packet.body)
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
