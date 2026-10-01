use std::io::Read;

use super::source_rcon_read_packet;

pub(super) fn read_command_response<R: Read>(
    stream: &mut R,
    exec_id: i32,
) -> Result<String, String> {
    const MAX_RESPONSE_BYTES: usize = 256 * 1024;
    let mut response = Vec::new();
    let mut received_bytes = 0usize;
    for _ in 0..64 {
        let packet = source_rcon_read_packet(stream)?;
        received_bytes += packet.body.len();
        if received_bytes > MAX_RESPONSE_BYTES {
            return Err(String::from("Rust RCON response exceeded 256 KiB"));
        }
        match (packet.id, packet.packet_type) {
            // Called only after authentication and our command write. The
            // native Rust client emits this completion even without output.
            (-1, 0) if packet.body.is_empty() => {
                return String::from_utf8(response)
                    .map_err(|_| String::from("Rust RCON response contains invalid UTF-8"));
            }
            (id, 0) if id == exec_id => response.extend_from_slice(&packet.body),
            // Rust broadcasts console output separately from command output.
            (0, 4) => {}
            _ => {
                return Err(format!(
                    "Rust RCON returned an unexpected response id {} or type {}",
                    packet.id, packet.packet_type
                ));
            }
        }
    }
    Err(String::from(
        "Rust RCON command completion was not received within 64 frames",
    ))
}

#[cfg(test)]
#[path = "runtime_transport_rust_tests.rs"]
mod tests;
