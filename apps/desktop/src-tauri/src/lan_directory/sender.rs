use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

use socket2::{Domain, Protocol, Socket, Type};

use super::{
    DIRECTORY_EMIT_INTERVAL, DIRECTORY_MULTICAST_GROUP, DIRECTORY_MULTICAST_PORT,
    DIRECTORY_MULTICAST_TTL, DIRECTORY_RETRY_INITIAL_INTERVAL, diagnostics, next_retry_interval,
};

const MAX_DIRECTORY_INTERFACES: usize = 64;

#[derive(Debug)]
pub(super) struct DirectorySender {
    pub(super) socket: UdpSocket,
    pub(super) source_ip: Ipv4Addr,
}

struct InterfaceSender {
    sender: Option<DirectorySender>,
    next_attempt: Instant,
    retry_interval: Duration,
    last_error: Option<String>,
}

#[derive(Default)]
pub(super) struct DirectorySenders {
    interfaces: BTreeMap<Ipv4Addr, InterfaceSender>,
}

impl DirectorySenders {
    pub(super) fn refresh(&mut self, now: Instant) -> Result<(), String> {
        let interfaces = if_addrs::get_if_addrs()
            .map_err(|error| format!("failed to enumerate LAN directory interfaces: {error}"))?;
        let addresses = eligible_interfaces(
            interfaces
                .into_iter()
                .map(|interface| (interface.ip(), interface.is_oper_up())),
        )?;
        self.synchronize(addresses, now);
        if self.interfaces.is_empty() {
            return Err(String::from(
                "no active IPv4 LAN directory interface is available",
            ));
        }
        Ok(())
    }

    fn synchronize(&mut self, addresses: BTreeSet<Ipv4Addr>, now: Instant) {
        self.interfaces
            .retain(|address, _| addresses.contains(address));
        for address in addresses {
            self.interfaces.entry(address).or_insert(InterfaceSender {
                sender: None,
                next_attempt: now,
                retry_interval: DIRECTORY_RETRY_INITIAL_INTERVAL,
                last_error: None,
            });
        }
    }

    pub(super) fn publish(
        &mut self,
        now: Instant,
        emit: impl FnMut(&DirectorySender) -> Result<(), String>,
    ) {
        let target = SocketAddrV4::new(DIRECTORY_MULTICAST_GROUP, DIRECTORY_MULTICAST_PORT);
        self.publish_with(
            now,
            |address| create_directory_sender(address, target),
            emit,
            diagnostics::record,
        );
    }

    fn publish_with(
        &mut self,
        now: Instant,
        mut create: impl FnMut(Ipv4Addr) -> Result<DirectorySender, String>,
        mut emit: impl FnMut(&DirectorySender) -> Result<(), String>,
        mut report: impl FnMut(&str, &str, &str),
    ) {
        for (address, state) in &mut self.interfaces {
            if now < state.next_attempt {
                continue;
            }
            let result = (|| {
                if state.sender.is_none() {
                    state.sender = Some(create(*address)?);
                }
                let sender = state.sender.as_ref().ok_or_else(|| {
                    String::from("LAN directory interface sender was not initialized")
                })?;
                emit(sender)
            })();
            match result {
                Ok(()) => {
                    if state.last_error.take().is_some() {
                        report(
                            "info",
                            "lan_directory.interface_recovered",
                            &format!("LanGame LAN directory multicast recovered on {address}"),
                        );
                    }
                    state.retry_interval = DIRECTORY_RETRY_INITIAL_INTERVAL;
                    state.next_attempt = now + DIRECTORY_EMIT_INTERVAL;
                }
                Err(error) => {
                    if state.last_error.as_deref() != Some(error.as_str()) {
                        report(
                            "warning",
                            "lan_directory.interface_failed",
                            &format!(
                                "LanGame LAN directory multicast failed on {address}: {error}"
                            ),
                        );
                    }
                    state.last_error = Some(error);
                    state.sender = None;
                    state.next_attempt = now + state.retry_interval;
                    state.retry_interval = next_retry_interval(state.retry_interval);
                }
            }
        }
    }

    pub(super) fn next_delay(&self, now: Instant) -> Duration {
        self.interfaces
            .values()
            .map(|state| state.next_attempt.saturating_duration_since(now))
            .min()
            .unwrap_or(DIRECTORY_EMIT_INTERVAL)
            .min(DIRECTORY_EMIT_INTERVAL)
    }
}

fn eligible_interfaces(
    interfaces: impl IntoIterator<Item = (IpAddr, bool)>,
) -> Result<BTreeSet<Ipv4Addr>, String> {
    let addresses = interfaces
        .into_iter()
        .filter_map(|(address, operational)| match address {
            IpAddr::V4(address) if operational && supported_source(address) => Some(address),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    if addresses.len() > MAX_DIRECTORY_INTERFACES {
        return Err(format!(
            "LAN directory interface count {} exceeds {MAX_DIRECTORY_INTERFACES}",
            addresses.len()
        ));
    }
    Ok(addresses)
}

fn supported_source(address: Ipv4Addr) -> bool {
    let octets = address.octets();
    address.is_private()
        || address.is_link_local()
        || matches!(octets[0], 25 | 26)
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
}

pub(super) fn create_directory_sender(
    source_ip: Ipv4Addr,
    target: SocketAddrV4,
) -> Result<DirectorySender, String> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
        .map_err(|error| format!("failed to create LAN directory socket: {error}"))?;
    socket
        .bind(&SocketAddrV4::new(source_ip, 0).into())
        .map_err(|error| format!("failed to bind LAN directory source {source_ip}: {error}"))?;
    // Binding alone does not select Windows' multicast egress interface.
    // Pin both source and IP_MULTICAST_IF so VPN/default-route changes cannot
    // redirect announcements away from this interface.
    socket.set_multicast_if_v4(&source_ip).map_err(|error| {
        format!("failed to select LAN directory interface {source_ip}: {error}")
    })?;
    socket
        .set_multicast_ttl_v4(DIRECTORY_MULTICAST_TTL)
        .map_err(|error| format!("failed to set LAN directory multicast TTL: {error}"))?;
    socket
        .set_multicast_loop_v4(true)
        .map_err(|error| format!("failed to enable LAN directory multicast loopback: {error}"))?;
    socket
        .set_nonblocking(true)
        .map_err(|error| format!("failed to make LAN directory socket nonblocking: {error}"))?;
    socket
        .connect(&target.into())
        .map_err(|error| format!("failed to connect LAN directory sender: {error}"))?;
    Ok(DirectorySender {
        socket: socket.into(),
        source_ip,
    })
}

#[cfg(test)]
mod tests;
