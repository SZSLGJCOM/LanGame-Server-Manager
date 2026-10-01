use std::io::{Read, Write};
use std::time::Duration;

use super::{DeadlineTcpStream, source_rcon_read_packet, source_rcon_write_packet};

pub(crate) fn conan_rcon_exec(
    endpoint: &str,
    password: &str,
    command: &str,
) -> Result<String, String> {
    let mut stream = DeadlineTcpStream::connect(endpoint, Duration::from_secs(3))
        .map_err(|error| format!("failed to connect to Conan RCON `{endpoint}`: {error}"))?;
    let marker = format!("LgsmBoundary{}", uuid::Uuid::new_v4().simple());
    exchange(&mut stream, password, command, &marker)
}

fn exchange<S: Read + Write>(
    stream: &mut S,
    password: &str,
    command: &str,
    marker: &str,
) -> Result<String, String> {
    exchange_with_delivery(stream, password, command, marker, &mut false)
}

pub(super) fn exchange_with_delivery<S: Read + Write>(
    stream: &mut S,
    password: &str,
    command: &str,
    marker: &str,
    command_may_have_been_sent: &mut bool,
) -> Result<String, String> {
    source_rcon_write_packet(stream, 14001, 3, password)?;
    let auth = source_rcon_read_packet(stream)?;
    if auth.id < 0 {
        return Err(String::from("Conan RCON authentication failed"));
    }
    // Enhanced replies with type 2 and the previous request's ID, including an
    // initial ID of 0. Its explicit acknowledgement establishes authentication.
    if auth.packet_type != 2 || auth.body.trim_ascii() != b"Authenticated." {
        return Err(String::from(
            "Conan RCON authentication response was not received",
        ));
    }

    *command_may_have_been_sent = true;
    source_rcon_write_packet(stream, 14002, 2, command)?;
    let boundary = format!("Commands matching search string: {marker}").into_bytes();
    let mut response = Vec::new();
    for index in 0..64 {
        let packet = source_rcon_read_packet(stream)?;
        if packet.id < 0 || packet.packet_type != 2 {
            return Err(String::from(
                "Conan RCON command returned an invalid response",
            ));
        }
        response.extend_from_slice(&packet.body);
        if let Some(offset) = response
            .windows(boundary.len())
            .position(|window| window == boundary.as_slice())
        {
            if !response[offset + boundary.len()..].trim_ascii().is_empty() {
                return Err(String::from("Conan RCON completion response was invalid"));
            }
            response.truncate(offset);
            return String::from_utf8(response)
                .map_err(|_| String::from("Conan RCON command response contains invalid UTF-8"));
        }
        if index == 0 {
            // A unique, read-only help filter provides an ordered boundary.
            // Empty Source response-value packets do not complete on this server.
            source_rcon_write_packet(stream, 14003, 2, &format!("help {marker}"))?;
        }
    }
    Err(String::from(
        "Conan RCON command response terminator was not received",
    ))
}

#[cfg(test)]
#[path = "runtime_transport_conan_tests.rs"]
mod tests;
