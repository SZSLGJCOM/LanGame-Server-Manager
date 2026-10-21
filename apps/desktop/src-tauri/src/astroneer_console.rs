//! Authenticated, local-only ASTRONEER queries.
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[path = "astroneer_console_startup.rs"]
pub(crate) mod startup;

pub(crate) fn validate_start_settings(instance: &app_core::InstanceDetails) -> Result<(), String> {
    if instance.summary.module_id != "astroneer" {
        return Ok(());
    }
    let settings: serde_json::Value = serde_json::from_str(&instance.settings_json)
        .map_err(|_| String::from("ASTRONEER PublicIP settings are invalid."))?;
    let value = settings
        .get("public_ip")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| String::from("ASTRONEER PublicIP was not prepared before startup."))?;
    crate::astroneer_public_ip::parse_public_address(value).map(|_| ())
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
        read_json_response(&mut stream, max_bytes).await
    })
    .await
    .map_err(|_| FetchError::Timeout)?
}

async fn read_json_response(
    stream: &mut TcpStream,
    max_bytes: usize,
) -> Result<Vec<u8>, FetchError> {
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
        // Fragment boundaries and braces in strings do not terminate responses.
        if body.ends_with(b"\r\n") {
            match serde_json::from_slice::<serde_json::Value>(&body) {
                Ok(_) => return Ok(body),
                Err(error) if error.is_eof() => {}
                Err(_) => return Err(FetchError::Incomplete),
            }
        }
    }
}
