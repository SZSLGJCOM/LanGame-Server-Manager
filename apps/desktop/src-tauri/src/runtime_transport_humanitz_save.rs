//! Fixed HumanitZ save exchange. The native ACK is not a durable-write claim.
use std::io::{Read, Write};

use super::{DeadlineTcpStream, TOTAL_TIMEOUT, read_packet, source_rcon_write_packet};

const REQUEST_ID: i32 = 0;
const SAVE_ACK_ID: i32 = 1;
const SAVE_ACK: &str = "Game saved!";

pub(crate) fn humanitz_rcon_save(
    endpoint: &str,
    password: &str,
    command: &str,
) -> Result<String, String> {
    if command != "save" {
        return Err("The HumanitZ save transport only supports the declared save command.".into());
    }
    if password.is_empty() || password.len() > 4096 || password.contains('\0') {
        return Err("The HumanitZ RCON password is empty or exceeds its protocol limits.".into());
    }
    let mut stream = DeadlineTcpStream::connect_with_budget(endpoint, TOTAL_TIMEOUT, TOTAL_TIMEOUT)
        .map_err(|error| format!("Unable to connect to HumanitZ RCON: {error}"))?;
    exchange(&mut stream, password)
}

fn exchange<S: Read + Write>(stream: &mut S, password: &str) -> Result<String, String> {
    // The shipped HMZ RCON utility uses ID zero for auth and command packets.
    // The current native save reply uses ID one, not the submitted request ID.
    source_rcon_write_packet(stream, REQUEST_ID, 3, password)?;
    let mut authenticated = false;
    for index in 0..2 {
        let packet = read_packet(stream)?;
        if packet.id == -1 {
            return Err("HumanitZ RCON authentication failed.".into());
        }
        match (packet.id, packet.packet_type) {
            (REQUEST_ID, 2) => {
                authenticated = true;
                break;
            }
            (REQUEST_ID, 0) if index == 0 => {}
            _ => return Err("HumanitZ save returned an invalid authentication response.".into()),
        }
    }
    if !authenticated {
        return Err("HumanitZ save did not confirm authentication.".into());
    }
    source_rcon_write_packet(stream, REQUEST_ID, 2, "save").map_err(|error| {
        format!("HumanitZ save may have been sent; input delivery is unconfirmed and was not retried: {error}")
    })?;
    let packet = read_packet(stream).map_err(|error| {
        format!(
            "HumanitZ native save acknowledgement is unconfirmed; the save was not retried: {error}"
        )
    })?;
    if packet.id != SAVE_ACK_ID || packet.packet_type != 0 || packet.body != SAVE_ACK {
        return Err("HumanitZ returned an unrecognized native save acknowledgement; the save was not retried.".into());
    }
    // No Source response-value terminator or second command is sent. The caller
    // retains write_confirmation_pending=true until stronger native evidence exists.
    Ok(packet.body)
}

#[cfg(test)]
#[path = "runtime_transport_humanitz_save_tests.rs"]
mod tests;
