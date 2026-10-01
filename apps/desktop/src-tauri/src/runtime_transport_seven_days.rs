use std::io::{ErrorKind, Read};
use std::time::Duration;

use super::{DeadlineTcpStream, telnet_text_from_bytes, telnet_write_line};

const MAX_CAPTURE_BYTES: usize = 512 * 1024;

pub(crate) fn seven_days_ban_telnet_exec(
    endpoint: &str,
    password: &str,
    command: &str,
) -> Result<String, String> {
    let mut stream =
        DeadlineTcpStream::connect(endpoint, Duration::from_millis(750)).map_err(|error| {
            format!("Seven Days Telnet connection failed before submission: {error}")
        })?;
    let result = exchange(&mut stream, password, command);
    let _ = stream.shutdown();
    result
}

fn exchange(
    stream: &mut DeadlineTcpStream,
    password: &str,
    command: &str,
) -> Result<String, String> {
    if command.contains(['\r', '\n']) || password.contains(['\r', '\n']) {
        return Err(String::from(
            "Seven Days Telnet input must be a single line.",
        ));
    }
    let mut capture = Vec::new();
    if !password.is_empty() {
        telnet_write_line(stream, password, "password")?;
        read_until_line(stream, &mut capture, "Logon successful.")?;
    }
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let begin = format!("lsgm_ban_begin_{nonce}");
    let end = format!("lsgm_ban_end_{nonce}");
    // Native ExecuteAsync appends to one FIFO. Update finishes executeCommand
    // and SendLines before removing its first item. Unknown commands only emit
    // the exact error below, so these per-request markers have no game effect.
    telnet_write_line(stream, &begin, "begin marker")?;
    let (_, envelope_start) = read_until_line(stream, &mut capture, &marker_response(&begin))?;
    telnet_write_line(stream, command, "ban command")?;
    telnet_write_line(stream, &end, "completion marker").map_err(|error| {
        format!("The ban command was sent, but its completion marker could not be sent: {error}")
    })?;
    let (envelope_end, _) = read_until_line(stream, &mut capture, &marker_response(&end))
        .map_err(|error| format!("The ban command was sent, but its complete response could not be confirmed: {error}"))?;
    let text = telnet_text_from_bytes(&capture);
    Ok(text[envelope_start..envelope_end].to_string())
}

fn marker_response(marker: &str) -> String {
    format!("*** ERROR: unknown command '{marker}'")
}

fn read_until_line(
    stream: &mut DeadlineTcpStream,
    capture: &mut Vec<u8>,
    expected: &str,
) -> Result<(usize, usize), String> {
    let mut buffer = [0_u8; 4096];
    loop {
        let text = telnet_text_from_bytes(capture);
        let mut offset = 0;
        for line in text.split_inclusive('\n') {
            let Some(body) = line.strip_suffix('\n') else {
                break;
            };
            let body = body.strip_suffix('\r').unwrap_or(body);
            if body == expected {
                return Ok((offset, offset + line.len()));
            }
            if body.starts_with("Password incorrect") || body == "Too many failed login attempts!" {
                return Err(String::from("Seven Days Telnet authentication failed."));
            }
            offset += line.len();
        }
        match stream.read(&mut buffer) {
            Ok(0) => {
                return Err(String::from(
                    "Seven Days Telnet closed before the expected completion line.",
                ));
            }
            Ok(count) => {
                if capture.len().saturating_add(count) > MAX_CAPTURE_BYTES {
                    return Err(String::from(
                        "Seven Days Telnet response exceeds the capture limit.",
                    ));
                }
                capture.extend_from_slice(&buffer[..count]);
            }
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                // Idle is not completion. Partial reads never renew the stream's
                // existing eight-second deadline, and the command is never resent.
                stream.check_deadline().map_err(|error| error.to_string())?;
            }
            Err(error) => return Err(format!("Seven Days Telnet response failed: {error}")),
        }
    }
}

#[cfg(test)]
#[path = "runtime_transport_seven_days_tests.rs"]
mod tests;
