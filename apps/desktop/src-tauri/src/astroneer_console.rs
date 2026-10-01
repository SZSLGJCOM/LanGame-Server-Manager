//! Authenticated, local-only ASTRONEER read queries. No mutating command is exposed.
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub(crate) fn validate_start_settings(instance: &app_core::InstanceDetails) -> Result<(), String> {
    if instance.summary.module_id != "astroneer" {
        return Ok(());
    }
    let valid = serde_json::from_str::<serde_json::Value>(&instance.settings_json)
        .ok()
        .and_then(|settings| {
            settings
                .get("public_ip")
                .and_then(serde_json::Value::as_str)
                .and_then(|value| value.parse::<Ipv4Addr>().ok())
        })
        .is_some();
    if valid {
        Ok(())
    } else {
        Err(String::from(
            "ASTRONEER requires a non-empty IPv4 PublicIP for native server registration. Set the server's public IPv4 address in instance settings before starting.",
        ))
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Query {
    Players,
    Games,
    Statistics,
}

impl Query {
    fn command(self) -> &'static str {
        match self {
            Self::Players => "DSListPlayers",
            Self::Games => "DSListGames",
            Self::Statistics => "DSServerStatistics",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FetchError {
    Timeout,
    TooLarge,
    Transport,
    ClosedBeforeResponse,
    Incomplete,
}

pub(crate) async fn fetch(
    port: u16,
    password: &str,
    query: Query,
    timeout: Duration,
    max_bytes: usize,
) -> Result<Vec<u8>, FetchError> {
    // Cancelling any stage drops the socket. PublicIP is deliberately not an input.
    tokio::time::timeout(timeout, async {
        let mut stream = TcpStream::connect(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
            .await
            .map_err(|_| FetchError::Transport)?;
        stream
            .write_all(format!("{password}\n{}\n", query.command()).as_bytes())
            .await
            .map_err(|_| FetchError::Transport)?;
        let mut body = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let length = stream
                .read(&mut buffer)
                .await
                .map_err(|_| FetchError::Transport)?;
            if length == 0 {
                return Err(if body.is_empty() {
                    FetchError::ClosedBeforeResponse
                } else {
                    FetchError::Incomplete
                });
            }
            if body.len().saturating_add(length) > max_bytes {
                return Err(FetchError::TooLarge);
            }
            body.extend_from_slice(&buffer[..length]);
            // Fragment boundaries and braces in strings do not terminate native responses.
            if body.ends_with(b"\r\n") {
                match serde_json::from_slice::<serde_json::Value>(&body) {
                    Ok(_) => return Ok(body),
                    Err(error) if error.is_eof() => {}
                    Err(_) => return Err(FetchError::Incomplete),
                }
            }
        }
    })
    .await
    .map_err(|_| FetchError::Timeout)?
}
