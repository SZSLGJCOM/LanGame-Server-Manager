use super::*;

pub(super) struct OwnedConsoleInterrupt {
    pid: u32,
    identity: app_core::ProcessIdentity,
}

impl OwnedConsoleInterrupt {
    pub(super) fn capture(child: &RuntimeChild) -> io::Result<Self> {
        let RuntimeChild::Windows(child) = child else {
            return Err(io::Error::other("elevated workload has no native owner"));
        };
        if child.job.is_none() {
            return Err(io::Error::other("elevated workload has no owned Job"));
        }
        let identity = crate::query_windows_process_identity_from_handle(
            child.process_id,
            child.process_handle as *mut _,
        )
        .map_err(io::Error::other)?;
        Ok(Self {
            pid: child.process_id,
            identity,
        })
    }

    pub(super) fn request(&self) -> serde_json::Value {
        // The parent cannot select a PID or path. This helper only controls the
        // workload it created, using the same identity/console-member checks as
        // non-elevated native control, under its already-approved token.
        match crate::request_windows_console_ctrl_c(self.pid, &self.identity) {
            Ok(()) => json!({ "interrupted": true }),
            Err(error) => json!({ "interrupted": false, "error": error.to_string() }),
        }
    }
}

pub(super) fn is_interrupt_response(response: &serde_json::Value) -> bool {
    response
        .get("interrupted")
        .and_then(serde_json::Value::as_bool)
        .is_some()
}

/// Unlike startup, a control timeout leaves the channel and workload alive.
/// Retain a partially received frame so the next observation can finish it.
#[derive(Debug, Default)]
pub(super) struct ReceiptReader {
    bytes: Vec<u8>,
}

impl ReceiptReader {
    pub(super) fn receive(
        &mut self,
        channel: &pipe::Channel,
        deadline: Instant,
    ) -> io::Result<serde_json::Value> {
        loop {
            let needed = if self.bytes.len() < 4 {
                4
            } else {
                let length = u32::from_le_bytes(self.bytes[..4].try_into().unwrap()) as usize;
                if length == 0 || length > pipe::MAX_FRAME {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid launcher receipt size",
                    ));
                }
                length + 4
            };
            if self.bytes.len() == needed && needed > 4 {
                let result = serde_json::from_slice(&self.bytes[4..])
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error));
                self.bytes.clear();
                return result;
            }
            pipe::check_deadline(deadline)?;
            let mut buffer = [0_u8; 1024];
            let capacity = (needed - self.bytes.len()).min(buffer.len());
            let read = channel.read_available(&mut buffer[..capacity])?;
            self.bytes.extend_from_slice(&buffer[..read]);
            if read == 0 {
                std::thread::sleep(
                    pipe::POLL.min(deadline.saturating_duration_since(Instant::now())),
                );
            }
        }
    }
}

impl ElevatedGuard {
    pub(crate) fn request_console_interrupt(&mut self, process: usize) -> io::Result<()> {
        let deadline = Instant::now() + STOP_TIMEOUT;
        if !self.interrupt_pending {
            if self.confirmed || !running(process)? {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "elevated launcher already exited",
                ));
            }
            // Once a write is attempted, an uncertain response must not cause a
            // duplicate broadcast. A later request first resolves this receipt.
            self.interrupt_pending = true;
            self.channel.write_all(b"C", deadline)?;
        }
        let response = self.receipts.receive(&self.channel, deadline)?;
        match response
            .get("interrupted")
            .and_then(serde_json::Value::as_bool)
        {
            Some(true) => {
                self.interrupt_pending = false;
                Ok(())
            }
            Some(false) => {
                self.interrupt_pending = false;
                Err(io::Error::other(
                    response
                        .get("error")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("elevated console interrupt failed")
                        .to_owned(),
                ))
            }
            None if response.get("stopped").and_then(serde_json::Value::as_bool) == Some(true) => {
                self.interrupt_pending = false;
                self.confirmed = true;
                Ok(())
            }
            None => Err(invalid("unexpected elevated console interrupt response")),
        }
    }
}
