use std::io::ErrorKind;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

pub const A2S_MAX_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_QUERY_BUDGET: Duration = Duration::from_secs(3);
const RECEIVE_INTERVAL: Duration = Duration::from_millis(350);

#[derive(Debug, thiserror::Error)]
pub enum A2sQueryError {
    #[error("A2S query requires a nonzero budget no longer than three seconds")]
    InvalidBudget,
    #[error("A2S query timed out")]
    TimedOut,
    #[error("A2S {operation} failed ({kind:?})")]
    Io {
        operation: &'static str,
        kind: ErrorKind,
    },
    #[error("Invalid A2S response: {0}")]
    InvalidResponse(&'static str),
}

/// Connected local queries share one deadline, including challenge and split replies.
pub struct A2sClient {
    socket: UdpSocket,
    deadline: Instant,
}

impl A2sClient {
    pub fn connect(address: SocketAddr, budget: Duration) -> Result<Self, A2sQueryError> {
        if budget.is_zero() || budget > MAX_QUERY_BUDGET {
            return Err(A2sQueryError::InvalidBudget);
        }
        let deadline = Instant::now() + budget;
        let socket = UdpSocket::bind(if address.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        })
        .map_err(|error| io_error("bind", error))?;
        socket
            .connect(address)
            .map_err(|error| io_error("connect", error))?;
        Ok(Self { socket, deadline })
    }

    pub fn info(&self) -> Result<Vec<u8>, A2sQueryError> {
        self.exchange(b"\xff\xff\xff\xffTSource Engine Query\0", b'I')
    }

    pub fn players(&self) -> Result<Vec<u8>, A2sQueryError> {
        self.exchange(b"\xff\xff\xff\xffU\xff\xff\xff\xff", b'D')
    }

    fn exchange(&self, request: &[u8], response_type: u8) -> Result<Vec<u8>, A2sQueryError> {
        let mut request = request.to_vec();
        let mut challenges = 0;
        // Retransmission is independent of the challenge stage. A first lost
        // datagram must not make the later challenge consume the final attempt.
        for _ in 0..4 {
            self.socket
                .set_write_timeout(Some(remaining(self.deadline)?.min(RECEIVE_INTERVAL)))
                .map_err(|error| io_error("write timeout", error))?;
            self.socket
                .send(&request)
                .map_err(|error| io_error("send", error))?;
            let reply = match receive_message(&self.socket, self.deadline) {
                Err(A2sQueryError::Io {
                    operation: "receive",
                    kind: ErrorKind::TimedOut | ErrorKind::WouldBlock,
                }) => continue,
                result => result?,
            };
            if reply.get(..5) == Some(b"\xff\xff\xff\xffA") {
                if reply.len() != 9 || challenges >= 2 {
                    return Err(A2sQueryError::InvalidResponse("challenge did not complete"));
                }
                challenges += 1;
                if response_type == b'D' {
                    request.truncate(5);
                } else {
                    request.truncate(b"\xff\xff\xff\xffTSource Engine Query\0".len());
                }
                request.extend_from_slice(&reply[5..9]);
            } else if reply.get(..4) == Some(b"\xff\xff\xff\xff")
                && reply.get(4) == Some(&response_type)
            {
                return Ok(reply);
            } else {
                return Err(A2sQueryError::InvalidResponse("unexpected response type"));
            }
        }
        Err(A2sQueryError::TimedOut)
    }
}

fn remaining(deadline: Instant) -> Result<Duration, A2sQueryError> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or(A2sQueryError::TimedOut)
}

fn io_error(operation: &'static str, error: std::io::Error) -> A2sQueryError {
    A2sQueryError::Io {
        operation,
        kind: error.kind(),
    }
}

fn receive_message(socket: &UdpSocket, deadline: Instant) -> Result<Vec<u8>, A2sQueryError> {
    let mut buffer = [0_u8; A2S_MAX_RESPONSE_BYTES];
    let mut fragments: Vec<Option<Vec<u8>>> = Vec::new();
    let mut request_id = None;
    let mut total_bytes = 0;
    for _ in 0..32 {
        socket
            .set_read_timeout(Some(remaining(deadline)?.min(RECEIVE_INTERVAL)))
            .map_err(|error| io_error("read timeout", error))?;
        let size = match socket.recv(&mut buffer) {
            Ok(size) => size,
            Err(error)
                if !fragments.is_empty()
                    && matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) =>
            {
                // Keep already received fragments until the shared deadline.
                continue;
            }
            Err(error) => return Err(io_error("receive", error)),
        };
        let packet = &buffer[..size];
        if packet.starts_with(b"\xff\xff\xff\xff") && request_id.is_none() {
            return Ok(packet.to_vec());
        }
        if size < 12 || !packet.starts_with(b"\xfe\xff\xff\xff") {
            return Err(A2sQueryError::InvalidResponse("invalid split header"));
        }
        let id = u32::from_le_bytes([packet[4], packet[5], packet[6], packet[7]]);
        // Compressed responses require a separately verified bounded codec.
        if id & 0x80000000 != 0 {
            return Err(A2sQueryError::InvalidResponse(
                "compressed replies are unsupported",
            ));
        }
        let total = packet[8] as usize;
        let index = packet[9] as usize;
        if total == 0 || total > 16 || index >= total {
            return Err(A2sQueryError::InvalidResponse("invalid fragment count"));
        }
        if let Some(expected) = request_id {
            if expected != id || fragments.len() != total {
                return Err(A2sQueryError::InvalidResponse("mismatched fragments"));
            }
        } else {
            request_id = Some(id);
            fragments.resize(total, None);
        }
        let body = &packet[12..];
        if let Some(existing) = &fragments[index] {
            if existing != body {
                return Err(A2sQueryError::InvalidResponse(
                    "conflicting duplicate fragment",
                ));
            }
        } else {
            total_bytes += body.len();
            if total_bytes > A2S_MAX_RESPONSE_BYTES {
                return Err(A2sQueryError::InvalidResponse("response exceeds limit"));
            }
            fragments[index] = Some(body.to_vec());
        }
        if fragments.iter().all(Option::is_some) {
            return Ok(fragments.into_iter().flatten().flatten().collect());
        }
    }
    Err(A2sQueryError::InvalidResponse(
        "fragment packet limit exceeded",
    ))
}

#[cfg(test)]
#[path = "a2s_tests.rs"]
mod tests;
