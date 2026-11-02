//! Shared protocol readiness bound to an exact live process and owned endpoint.
use crate::{A2S_MAX_RESPONSE_BYTES, A2sClient, normalize_query_host};
use app_core::ProcessIdentity;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

const QUERY_TIMEOUT: Duration = Duration::from_secs(3);

pub struct ProcessProbeTarget {
    pub pid: u32,
    pub identity: ProcessIdentity,
    pub host: String,
}

/// One bounded INFO exchange; port presence and partial count replies are insufficient.
pub fn poll_owned_a2s_info(target: &ProcessProbeTarget, port: u16) -> Result<bool, String> {
    let host = normalize_query_host(&target.host)
        .parse::<IpAddr>()
        .map_err(|_| "Protocol query requires a local IP address.")?;
    if port == 0 {
        return Err("Protocol query port must be nonzero.".into());
    }
    let address = SocketAddr::new(host, port);
    poll_owned(
        || verify_endpoint(target, Protocol::Udp, address),
        || {
            Ok(A2sClient::connect(address, QUERY_TIMEOUT)
                .and_then(|client| client.info())
                .is_ok_and(|reply| complete_a2s_info(&reply)))
        },
    )
}

/// Authenticated callers additionally verify the connected peer before writing secrets.
pub fn poll_owned_tcp(
    target: &ProcessProbeTarget,
    port: u16,
    query: impl FnOnce() -> Result<bool, String>,
) -> Result<bool, String> {
    if port == 0 {
        return Err("Protocol query port must be nonzero.".into());
    }
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    poll_owned(|| verify_endpoint(target, Protocol::Tcp, address), query)
}

fn poll_owned(
    mut owned: impl FnMut() -> Result<bool, String>,
    query: impl FnOnce() -> Result<bool, String>,
) -> Result<bool, String> {
    if !owned()? {
        return Ok(false);
    }
    let result = query();
    // A valid reply cannot outlive the owned endpoint or the process identity.
    // Recheck after failed queries too, so replacement is never a retry signal.
    let still_owned = owned()?;
    result.map(|ready| ready && still_owned)
}

#[derive(Clone, Copy)]
enum Protocol {
    Tcp,
    Udp,
}

#[derive(Clone, Copy)]
struct EndpointOwner {
    address: IpAddr,
    port: u16,
    pid: u32,
}

#[cfg(windows)]
fn verify_identity(target: &ProcessProbeTarget) -> Result<(), String> {
    if target.pid == 0
        || target.identity.creation_time == 0
        || target.identity.image_path.is_empty()
    {
        return Err("Startup process identity is missing or invalid.".into());
    }
    let current = app_runtime::inspect_process_identity(target.pid)
        .map_err(|_| "Cannot verify the startup process identity.")?;
    if current.as_ref() != Some(&target.identity) {
        return Err("Startup process exited or its identity changed.".into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn verify_identity(_: &ProcessProbeTarget) -> Result<(), String> {
    Err("Protocol process ownership verification requires Windows.".into())
}

fn verify_endpoint(
    target: &ProcessProbeTarget,
    protocol: Protocol,
    address: SocketAddr,
) -> Result<bool, String> {
    verify_identity(target)?;
    let rows = endpoint_owners(protocol)?;
    verify_identity(target)?;
    owns_endpoint(&rows, target.pid, address)
}

fn address_accepts(bound: IpAddr, destination: IpAddr) -> bool {
    match (bound, destination) {
        (IpAddr::V4(bound), IpAddr::V4(destination)) => {
            bound.is_unspecified() || bound == destination
        }
        (IpAddr::V6(bound), IpAddr::V6(destination)) => {
            bound.is_unspecified() || bound == destination
        }
        (IpAddr::V6(bound), IpAddr::V4(destination)) => bound.to_ipv4_mapped() == Some(destination),
        _ => false,
    }
}

fn owns_endpoint(rows: &[EndpointOwner], pid: u32, target: SocketAddr) -> Result<bool, String> {
    let mut found = false;
    for row in rows.iter().filter(|row| row.port == target.port()) {
        let accepts = address_accepts(row.address, target.ip());
        // Windows' owner table does not expose IPV6_V6ONLY. A foreign wildcard
        // IPv6 socket may also receive IPv4, so ambiguity must fail closed.
        let possible_dual_stack =
            row.address == IpAddr::V6(Ipv6Addr::UNSPECIFIED) && target.is_ipv4();
        if (accepts || possible_dual_stack) && row.pid != pid {
            return Err(
                "Startup query endpoint is owned by another process or is ambiguous.".into(),
            );
        }
        found |= accepts && row.pid == pid;
    }
    Ok(found)
}

#[cfg(windows)]
fn endpoint_owners(protocol: Protocol) -> Result<Vec<EndpointOwner>, String> {
    use std::ffi::c_void;
    type TableFn = unsafe extern "system" fn(*mut c_void, *mut u32, i32, u32, i32, u32) -> u32;
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
        fn GetExtendedUdpTable(
            table: *mut c_void,
            size: *mut u32,
            order: i32,
            family: u32,
            class: i32,
            reserved: u32,
        ) -> u32;
    }
    let (query, class): (TableFn, i32) = match protocol {
        Protocol::Tcp => (GetExtendedTcpTable, 5), // TCP_TABLE_OWNER_PID_ALL
        Protocol::Udp => (GetExtendedUdpTable, 1), // UDP_TABLE_OWNER_PID
    };
    let mut owners = Vec::new();
    for family in [2, 23] {
        // AF_INET and AF_INET6
        let width = match (protocol, family) {
            (Protocol::Tcp, 2) => 6,
            (Protocol::Tcp, _) => 14,
            (Protocol::Udp, 2) => 3,
            (Protocol::Udp, _) => 7,
        };
        let mut size = 0;
        let first = unsafe { query(std::ptr::null_mut(), &mut size, 0, family, class, 0) };
        if !matches!(first, 0 | 122) {
            return Err("Cannot inspect startup query endpoint ownership.".into());
        }
        let mut complete = false;
        for _ in 0..3 {
            if !(4..=4 * 1024 * 1024).contains(&size) {
                break;
            }
            // DWORD-aligned storage follows the documented MIB_*_OWNER_PID
            // layouts; bounded retries only accommodate table-size changes.
            let mut words = vec![0_u32; (size as usize).div_ceil(4)];
            let result =
                unsafe { query(words.as_mut_ptr().cast(), &mut size, 0, family, class, 0) };
            if result == 122 {
                continue;
            }
            if result != 0 || size < 4 || size as usize > words.len() * 4 {
                break;
            }
            let count = words[0] as usize;
            if count > (size as usize - 4) / (width * 4) {
                break;
            }
            for row in words[1..1 + count * width].chunks_exact(width) {
                let (address, port, pid) = match (protocol, family) {
                    (Protocol::Tcp, 2) if row[0] != 2 => continue, // LISTEN only
                    (Protocol::Tcp, 2) => (
                        IpAddr::V4(Ipv4Addr::from(row[1].to_ne_bytes())),
                        row[2],
                        row[5],
                    ),
                    (Protocol::Udp, 2) => (
                        IpAddr::V4(Ipv4Addr::from(row[0].to_ne_bytes())),
                        row[1],
                        row[2],
                    ),
                    (Protocol::Tcp, _) if row[12] != 2 => continue,
                    (_, _) => {
                        let mut bytes = [0; 16];
                        for (chunk, word) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(&row[..4])
                        {
                            chunk.copy_from_slice(&word.to_ne_bytes());
                        }
                        (IpAddr::V6(Ipv6Addr::from(bytes)), row[5], row[width - 1])
                    }
                };
                owners.push(EndpointOwner {
                    address,
                    port: u16::from_be(port as u16),
                    pid,
                });
            }
            complete = true;
            break;
        }
        if !complete {
            return Err("Cannot obtain a complete startup endpoint-owner table.".into());
        }
    }
    Ok(owners)
}

#[cfg(not(windows))]
fn endpoint_owners(_: Protocol) -> Result<Vec<EndpointOwner>, String> {
    Err("Startup protocol ownership verification requires Windows.".into())
}

// A player-count prefix alone is not a complete INFO reply. Validate every
// mandatory field and every optional field advertised by the EDF byte.
fn complete_a2s_info(reply: &[u8]) -> bool {
    fn take<'a>(input: &mut &'a [u8], length: usize) -> Option<&'a [u8]> {
        let (value, remaining) = input.split_at_checked(length)?;
        *input = remaining;
        Some(value)
    }
    fn string(input: &mut &[u8]) -> Option<()> {
        let length = input.iter().position(|byte| *byte == 0)?;
        take(input, length + 1)?;
        Some(())
    }
    fn parse(mut input: &[u8]) -> Option<()> {
        if take(&mut input, 5)? != b"\xff\xff\xff\xffI" {
            return None;
        }
        take(&mut input, 1)?; // protocol
        for _ in 0..4 {
            string(&mut input)?;
        }
        let app_id = take(&mut input, 2)?;
        take(&mut input, 3)?; // players, capacity, bots
        let flags = take(&mut input, 4)?;
        if !matches!(flags[0], b'd' | b'l' | b'p')
            || !matches!(flags[1], b'l' | b'w' | b'm' | b'o')
            || flags[2] > 1
            || flags[3] > 1
        {
            return None;
        }
        if u16::from_le_bytes([app_id[0], app_id[1]]) == 2400 {
            take(&mut input, 3)?; // The Ship's mode, witnesses and duration
        }
        string(&mut input)?; // version is mandatory even when EDF is absent
        if input.is_empty() {
            return Some(());
        }
        let edf = take(&mut input, 1)?[0];
        if edf & !0xf1 != 0 {
            return None;
        }
        if edf & 0x80 != 0 {
            take(&mut input, 2)?;
        }
        if edf & 0x10 != 0 {
            take(&mut input, 8)?;
        }
        if edf & 0x40 != 0 {
            take(&mut input, 2)?;
            string(&mut input)?;
        }
        if edf & 0x20 != 0 {
            string(&mut input)?;
        }
        if edf & 0x01 != 0 {
            take(&mut input, 8)?;
        }
        input.is_empty().then_some(())
    }
    reply.len() <= A2S_MAX_RESPONSE_BYTES && parse(reply).is_some()
}

#[cfg(test)]
#[path = "runtime_protocol_probe_tests.rs"]
mod tests;
