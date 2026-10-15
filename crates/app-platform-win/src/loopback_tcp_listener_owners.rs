//! Global listener ownership for authenticated IPv4 loopback requests.
use std::collections::BTreeSet;
use std::net::IpAddr;

use super::system_utility::{self, QUERY_TIMEOUT, SystemUtility};
use super::{parse_netstat_local_endpoint, parse_netstat_network_endpoints};

pub(super) fn inspect(port: u16) -> Result<BTreeSet<u32>, String> {
    if port == 0 {
        return Err("cannot inspect listener ownership for port zero".into());
    }
    let output =
        system_utility::capture(SystemUtility::Network, &["-a", "-n", "-o"], QUERY_TIMEOUT)
            .map_err(|error| format!("failed to inspect Windows TCP listener owners: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Windows TCP listener ownership inspection failed with status {}",
            output.status
        ));
    }
    parse(&String::from_utf8_lossy(&output.stdout), port)
}

fn parse(stdout: &str, port: u16) -> Result<BTreeSet<u32>, String> {
    // The general endpoint parser skips malformed rows. Ownership is a trust
    // boundary: an unreadable listener must not silently hide a competing PID.
    for line in stdout.lines() {
        let columns = line.split_whitespace().collect::<Vec<_>>();
        if !columns
            .first()
            .is_some_and(|value| value.eq_ignore_ascii_case("tcp"))
        {
            continue;
        }
        if columns.len() != 5 {
            return Err("unreadable Windows TCP endpoint row".into());
        }
        if columns[3].eq_ignore_ascii_case("LISTENING") {
            let Some((address, _)) = parse_netstat_local_endpoint(columns[1]) else {
                return Err("unreadable Windows TCP listener address".into());
            };
            if address.parse::<IpAddr>().is_err() || columns[4].parse::<u32>().is_err() {
                return Err("unreadable Windows TCP listener owner".into());
            }
        }
    }
    Ok(parse_netstat_network_endpoints(stdout)
        .into_iter()
        .filter(|endpoint| {
            endpoint.protocol == "tcp"
                && endpoint.local_port == port
                // An IPv6 wildcard can also accept IPv4 on a dual-stack socket.
                // ::1 cannot receive the explicitly IPv4 destination 127.0.0.1.
                && matches!(endpoint.local_address.as_str(), "127.0.0.1" | "0.0.0.0" | "::")
        })
        .map(|endpoint| endpoint.owning_pid)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WindowsPlatform;

    #[test]
    fn loopback_tcp_listener_owners_retains_foreign_and_dual_stack_owners() {
        let output = "\
  TCP    0.0.0.0:8212             0.0.0.0:0      LISTENING    100\r\n\
  TCP    127.0.0.1:8212           0.0.0.0:0      LISTENING    200\r\n\
  TCP    [::]:8212                [::]:0         LISTENING    300\r\n\
  TCP    [::ffff:127.0.0.1]:8212   [::]:0         LISTENING    400\r\n\
  TCP    192.0.2.1:8212           0.0.0.0:0      LISTENING    500\r\n\
  TCP    [::1]:8212               [::]:0         LISTENING    600\r\n\
  TCP    127.0.0.1:8213           0.0.0.0:0      LISTENING    700\r\n\
  TCP    127.0.0.1:8212           127.0.0.1:1000 ESTABLISHED  800\r\n\
  UDP    127.0.0.1:8212           *:*                           900\r\n";
        assert_eq!(
            parse(output, 8212).unwrap(),
            BTreeSet::from([100, 200, 300, 400])
        );
        assert!(parse(output, 8214).unwrap().is_empty());
    }

    #[test]
    fn loopback_tcp_listener_owners_rejects_unreadable_listener_rows() {
        for output in [
            "TCP 127.0.0.1:8212 0.0.0.0:0 LISTENING",
            "TCP 127.0.0.1:8212 0.0.0.0:0 LISTENING unknown",
            "TCP 127.0.0.1:invalid 0.0.0.0:0 LISTENING 100",
            "TCP invalid:8212 0.0.0.0:0 LISTENING 100",
        ] {
            assert!(parse(output, 8212).is_err());
        }
        // Unknown/system PID must stay visible, so callers cannot mistake it for their PID.
        assert_eq!(
            parse("TCP 127.0.0.1:8212 0.0.0.0:0 LISTENING 0", 8212).unwrap(),
            BTreeSet::from([0])
        );
        assert!(WindowsPlatform::loopback_tcp_listener_owners(0).is_err());
    }

    #[test]
    fn loopback_tcp_listener_owners_inspects_real_windows_listener() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert_eq!(
            WindowsPlatform::loopback_tcp_listener_owners(port).unwrap(),
            BTreeSet::from([std::process::id()])
        );
        listener.set_nonblocking(true).unwrap();
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
}
