//! Startup-only mutation, sent once to a socket owned by the exact Shipping PID.
use app_core::ProcessIdentity;
use serde_json::Value;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub(crate) fn new_game_command(label: &str) -> Result<String, String> {
    // No quoting or sanitization is guessed. The native FString constructor
    // preserves these descriptive labels exactly; '$' is a composite-name
    // delimiter. Existing worlds never reach this first-world-only restriction.
    if label.is_empty()
        || label.len() > 128
        || !label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err("ASTRONEER 自动创建首个世界目前需要 1–128 位英文字母、数字、下划线或连字符的存档名称；原配置与数据已保留，未发送创建命令。".into());
    }
    Ok(format!("DSNewGame {label} false"))
}

#[derive(Clone, Copy)]
struct Row {
    state: u32,
    local_address: u32,
    local_port: u32,
    remote_address: u32,
    remote_port: u32,
    pid: u32,
}

fn peer_row(row: Row, local: SocketAddrV4, remote: SocketAddrV4, pid: u32) -> bool {
    row.state == 5
        && row.pid == pid
        && row.local_address.to_ne_bytes() == remote.ip().octets()
        && row.remote_address.to_ne_bytes() == local.ip().octets()
        && u16::from_be(row.local_port as u16) == remote.port()
        && u16::from_be(row.remote_port as u16) == local.port()
}

#[cfg(windows)]
fn rows() -> Result<Vec<Row>, String> {
    use std::ffi::c_void;
    #[link(name = "iphlpapi")]
    unsafe extern "system" {
        fn GetExtendedTcpTable(
            table: *mut c_void,
            size: *mut u32,
            order: i32,
            family: u32,
            class: i32,
            reserved: u32,
        ) -> u32;
    }
    // AF_INET, TCP_TABLE_OWNER_PID_ALL. Microsoft documents the six DWORD
    // MIB_TCPROW_OWNER_PID fields and network byte order for both ports.
    let mut size = 0;
    let first = unsafe { GetExtendedTcpTable(std::ptr::null_mut(), &mut size, 0, 2, 5, 0) };
    if !matches!(first, 0 | 122) {
        return Err("Cannot verify the ASTRONEER console socket owner.".into());
    }
    for _ in 0..3 {
        if !(4..=4 * 1024 * 1024).contains(&size) {
            break;
        }
        let mut words = vec![0_u32; (size as usize).div_ceil(4)];
        let result =
            unsafe { GetExtendedTcpTable(words.as_mut_ptr().cast(), &mut size, 0, 2, 5, 0) };
        if result == 122 {
            continue;
        }
        if result != 0 || size as usize > words.len() * 4 || size < 4 {
            break;
        }
        let count = words[0] as usize;
        if count > (size as usize - 4) / 24 {
            break;
        }
        return Ok(words[1..1 + count * 6]
            .as_chunks::<6>()
            .0
            .iter()
            .map(|word| Row {
                state: word[0],
                local_address: word[1],
                local_port: word[2],
                remote_address: word[3],
                remote_port: word[4],
                pid: word[5],
            })
            .collect());
    }
    Err("Cannot obtain a complete ASTRONEER console socket-owner table.".into())
}
#[cfg(not(windows))]
fn rows() -> Result<Vec<Row>, String> {
    Err("ASTRONEER automatic world initialization requires Windows.".into())
}

fn verify_peer(stream: &TcpStream, pid: u32, identity: &ProcessIdentity) -> Result<(), String> {
    let (SocketAddr::V4(local), SocketAddr::V4(remote)) = (
        stream
            .local_addr()
            .map_err(|_| "ASTRONEER console socket unavailable.")?,
        stream
            .peer_addr()
            .map_err(|_| "ASTRONEER console socket unavailable.")?,
    ) else {
        return Err("ASTRONEER console must use local IPv4.".into());
    };
    if !local.ip().is_loopback()
        || !remote.ip().is_loopback()
        || !rows()?
            .into_iter()
            .any(|row| peer_row(row, local, remote, pid))
        || app_runtime::inspect_process_identity(pid)
            .ok()
            .flatten()
            .as_ref()
            != Some(identity)
    {
        return Err(
            "ASTRONEER console is not owned by the current Shipping process; no command was sent."
                .into(),
        );
    }
    Ok(())
}

async fn connect(port: u16, pid: u32, identity: &ProcessIdentity) -> Result<TcpStream, String> {
    let stream = TcpStream::connect(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
        .await
        .map_err(|_| "ASTRONEER native console is not ready.")?;
    // Check the connected server endpoint's complete four-tuple before sending
    // even the password. A same-port listener precheck cannot prove this peer.
    verify_peer(&stream, pid, identity)?;
    Ok(stream)
}

async fn response(stream: &mut TcpStream, games_only: bool) -> Result<Value, String> {
    let mut bytes = Vec::new();
    let mut block = [0; 4096];
    let mut total = 0_usize;
    let mut frames = 0;
    loop {
        let length = stream
            .read(&mut block)
            .await
            .map_err(|_| "ASTRONEER native response unavailable.")?;
        if length == 0 {
            return Err("ASTRONEER closed the console before the required query response.".into());
        }
        total = total.saturating_add(length);
        if total > 64 * 1024 {
            return Err("ASTRONEER native response exceeded its limit.".into());
        }
        bytes.extend_from_slice(&block[..length]);
        loop {
            let mut values = serde_json::Deserializer::from_slice(&bytes).into_iter::<Value>();
            let value = match values.next() {
                Some(Ok(value)) => value,
                Some(Err(error)) if error.is_eof() => break,
                None => break,
                Some(Err(_)) => return Err("ASTRONEER returned invalid native JSON.".into()),
            };
            let consumed = values.byte_offset();
            bytes.drain(..consumed);
            frames += 1;
            if frames > 4 {
                return Err("ASTRONEER returned unexpected console frames.".into());
            }
            if !value.is_object() {
                return Err("ASTRONEER returned invalid native JSON.".into());
            }
            if !games_only
                || (value.get("gameList").is_some_and(Value::is_array)
                    && value.get("activeSaveName").is_some_and(Value::is_string))
            {
                return Ok(value);
            }
        }
    }
}

pub(crate) async fn query(
    port: u16,
    password: &str,
    command: super::Query,
    pid: u32,
    identity: &ProcessIdentity,
) -> Result<Value, String> {
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut stream = connect(port, pid, identity).await?;
        stream
            .write_all(format!("{password}\n{}\n", command.command()).as_bytes())
            .await
            .map_err(|_| "ASTRONEER native query could not be delivered.")?;
        response(&mut stream, false).await
    })
    .await
    .map_err(|_| "ASTRONEER native query timed out.")?
}

pub(crate) async fn create_once(
    port: u16,
    password: &str,
    label: &str,
    pid: u32,
    identity: &ProcessIdentity,
    before_send: impl FnOnce() -> Result<(), String>,
) -> Result<Value, String> {
    let command = new_game_command(label)?;
    tokio::time::timeout(Duration::from_secs(15), async {
        let mut stream = connect(port, pid, identity).await?;
        before_send()?;
        stream
            .write_all(format!("{password}\n{command}\nDSListGames\n").as_bytes())
            .await
            .map_err(|_| "ASTRONEER 创建命令的送达结果未知；不会自动重发。")?;
        // This response only proves FIFO delivery on the same live connection.
        // The caller must continue with separate, read-only world observations.
        response(&mut stream, true).await
    })
    .await
    .map_err(|_| "ASTRONEER 创建命令的送达结果超时未知；不会自动重发。")?
}

/// DSSaveGame has no native acknowledgement. Keep its socket alive until the
/// following read-only command has crossed the native FIFO consumer; closing
/// immediately after write_all can discard commands still queued by that peer.
pub(crate) async fn save_once(
    port: u16,
    password: &str,
    world: &str,
    pid: u32,
    identity: &ProcessIdentity,
    before_send: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    if password.trim().is_empty()
        || password.chars().count() > 128
        || password.chars().any(char::is_control)
        || world.trim().is_empty()
        || world.chars().count() > 256
        || world.chars().any(char::is_control)
    {
        return Err(
            "ASTRONEER console credentials or selected world are invalid; no save was sent.".into(),
        );
    }
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut stream = connect(port, pid, identity).await?;
        before_send()?;
        stream
            .write_all(format!("{password}\nDSSaveGame\nDSListGames\n").as_bytes())
            .await
            .map_err(|_| "ASTRONEER save delivery is unknown; the request was not retried.")?;
        let games = response(&mut stream, true)
            .await
            .map_err(|_| "ASTRONEER save delivery is unknown; the request was not retried.")?;
        if games.get("activeSaveName").and_then(Value::as_str) != Some(world) {
            return Err(
                "ASTRONEER selected world changed after submission; the save was not retried."
                    .into(),
            );
        }
        // This proves command processing, not native storage completion.
        Ok(())
    })
    .await
    .map_err(|_| "ASTRONEER save delivery timed out and is unknown; the request was not retried.")?
}

#[cfg(test)]
#[path = "astroneer_console_startup_tests.rs"]
mod tests;
