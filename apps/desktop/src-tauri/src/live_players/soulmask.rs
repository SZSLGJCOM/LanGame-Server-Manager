use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use app_core::{
    InstanceDetails, ModulePlayerListSource, RuntimeLivePlayerIssueCode, RuntimeLivePlayerSnapshot,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpSocket, TcpStream};

use super::cache::LivePlayerCollectionResult;
use super::service::{failed_snapshot, misconfigured_snapshot};

#[path = "soulmask_response.rs"]
mod response;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_RESPONSE_BYTES: usize = 512 * 1024;
const MAX_PAGES: usize = 64;

#[derive(Debug, PartialEq, Eq)]
enum FetchError {
    Timeout,
    TooLarge,
    Transport,
    Incomplete,
}

pub(crate) async fn collect_soulmask(
    details: &InstanceDetails,
    instance_id: &str,
    request_id: &str,
    observed_at: u64,
) -> LivePlayerCollectionResult {
    let config_error = |summary| {
        Box::new(misconfigured_snapshot(
            instance_id,
            request_id.to_owned(),
            ModulePlayerListSource::TcpConsole,
            RuntimeLivePlayerIssueCode::RuntimeActionUnavailable,
            summary,
            Vec::new(),
        ))
    };
    if details.summary.module_id != "soulmask" || details.summary.id != instance_id {
        return Err(config_error(
            "The Soulmask instance context does not match.",
        ));
    }
    let mut ports = details.ports.iter().filter(|port| port.name == "echo");
    let port = ports
        .next()
        .ok_or_else(|| config_error("The instance has no Soulmask Echo TCP port."))?;
    if port.port == 0 || !port.protocol.eq_ignore_ascii_case("tcp") || ports.next().is_some() {
        return Err(config_error(
            "The instance Echo port is invalid or ambiguous.",
        ));
    }
    let host = app_storage::normalize_query_host(&details.summary.bind_ip)
        .parse::<Ipv4Addr>()
        .ok()
        .filter(|ip| !ip.is_multicast() && !ip.is_broadcast())
        .ok_or_else(|| {
            config_error("The Soulmask Echo endpoint requires a local IPv4 bind address.")
        })?;
    let body = fetch_players(SocketAddr::from((host, port.port)), REQUEST_TIMEOUT)
        .await
        .map_err(|error| {
            let (code, summary, truncated) = match error {
                FetchError::Timeout => (
                    RuntimeLivePlayerIssueCode::CollectionTimeout,
                    "The Soulmask Echo console did not complete the player query in time.",
                    false,
                ),
                FetchError::TooLarge => (
                    RuntimeLivePlayerIssueCode::CaptureLimit,
                    "The Soulmask player response exceeds the collection limit.",
                    true,
                ),
                FetchError::Transport => (
                    RuntimeLivePlayerIssueCode::IoFailed,
                    "The local Soulmask Echo console could not provide a player response.",
                    false,
                ),
                FetchError::Incomplete => (
                    RuntimeLivePlayerIssueCode::ProtocolIncomplete,
                    "The Soulmask Echo console returned an incomplete player response.",
                    false,
                ),
            };
            failure(instance_id, request_id, code, summary, truncated)
        })?;
    response::parse(instance_id, &body, request_id, observed_at)
}

async fn fetch_players(endpoint: SocketAddr, timeout: Duration) -> Result<Vec<u8>, FetchError> {
    // The Echo port follows MULTIHOME. Binding the outgoing socket to that same
    // address rejects non-local destinations without DNS or an advertised IP.
    // One deadline owns connect, every page and the final native disconnect.
    tokio::time::timeout(timeout, async {
        let socket = TcpSocket::new_v4().map_err(|_| FetchError::Transport)?;
        socket
            .bind(SocketAddr::new(endpoint.ip(), 0))
            .map_err(|_| FetchError::Transport)?;
        let mut stream = socket
            .connect(endpoint)
            .await
            .map_err(|_| FetchError::Transport)?;
        let mut body = Vec::new();
        let mut received = 0;
        let mut total_pages = None;
        for page in 1..=MAX_PAGES {
            // Native unknown commands echo an exact error line and leave the
            // interactive query cursor intact. A fresh nonce closes this page;
            // short reads and idle gaps never establish completeness.
            let nonce = format!("LGM_PLAYER_QUERY_END_{}", uuid::Uuid::new_v4().simple());
            let command = if page == 1 { "lp" } else { "n" };
            stream
                .write_all(format!("{command}\r\n{nonce}\r\n").as_bytes())
                .await
                .map_err(|_| FetchError::Transport)?;
            let page_body = read_page(&mut stream, &nonce, &mut received).await?;
            let response::NativePage {
                payload,
                pagination,
            } = response::split_page(&page_body)?;
            let last_page = match pagination {
                None if page == 1 => true,
                Some((current, total)) if current == page => {
                    if total_pages.is_some_and(|expected| expected != total) {
                        return Err(FetchError::Incomplete);
                    }
                    total_pages = Some(total);
                    current == total
                }
                _ => return Err(FetchError::Incomplete),
            };
            body.extend_from_slice(payload);
            if last_page {
                stream
                    .write_all(b"dc\r\n")
                    .await
                    .map_err(|_| FetchError::Transport)?;
                let mut tail = [0_u8; 1];
                // dc closes only this management connection, not the server.
                // Any bytes after the final nonce or a missing EOF invalidate it.
                return match stream.read(&mut tail).await {
                    Ok(0) => Ok(body),
                    Ok(_) => Err(FetchError::Incomplete),
                    Err(_) => Err(FetchError::Transport),
                };
            }
        }
        Err(FetchError::TooLarge)
    })
    .await
    .map_err(|_| FetchError::Timeout)?
}

async fn read_page(
    stream: &mut TcpStream,
    nonce: &str,
    received: &mut usize,
) -> Result<Vec<u8>, FetchError> {
    let marker = format!("{nonce} Not Found!\r\n");
    let marker = marker.as_bytes();
    let mut body = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let length = stream
            .read(&mut buffer)
            .await
            .map_err(|_| FetchError::Transport)?;
        if length == 0 {
            return Err(FetchError::Incomplete);
        }
        *received = received.saturating_add(length);
        if *received > MAX_RESPONSE_BYTES {
            return Err(FetchError::TooLarge);
        }
        let search_start = body.len().saturating_sub(marker.len() + 2);
        body.extend_from_slice(&buffer[..length]);
        if let Some(offset) = body[search_start..]
            .windows(marker.len())
            .position(|window| window == marker)
        {
            let offset = search_start + offset;
            if (offset != 0 && !body[..offset].ends_with(b"\r\n"))
                || offset + marker.len() != body.len()
            {
                return Err(FetchError::Incomplete);
            }
            body.truncate(offset);
            return Ok(body);
        }
    }
}

fn failure(
    instance_id: &str,
    request_id: &str,
    code: RuntimeLivePlayerIssueCode,
    summary: &str,
    truncated: bool,
) -> Box<RuntimeLivePlayerSnapshot> {
    Box::new(failed_snapshot(
        instance_id,
        request_id.to_owned(),
        ModulePlayerListSource::TcpConsole,
        code,
        summary,
        truncated,
    ))
}

#[cfg(test)]
#[path = "soulmask_tests.rs"]
mod tests;
