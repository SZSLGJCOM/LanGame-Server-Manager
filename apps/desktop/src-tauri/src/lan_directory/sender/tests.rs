use super::*;
use std::net::SocketAddr;

fn test_sender(address: Ipv4Addr) -> Result<DirectorySender, String> {
    Ok(DirectorySender {
        socket: UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|error| error.to_string())?,
        source_ip: address,
    })
}

#[test]
fn sender_pins_the_actual_source_and_multicast_interface() {
    let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind receiver");
    receiver
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("receive timeout");
    let SocketAddr::V4(target) = receiver.local_addr().expect("receiver address") else {
        panic!("expected an IPv4 receiver");
    };
    let sender = create_directory_sender(Ipv4Addr::LOCALHOST, target).expect("create sender");
    assert_eq!(sender.source_ip, Ipv4Addr::LOCALHOST);
    assert_eq!(
        socket2::SockRef::from(&sender.socket)
            .multicast_if_v4()
            .expect("read multicast interface"),
        Ipv4Addr::LOCALHOST
    );
    assert_eq!(sender.socket.multicast_ttl_v4().expect("TTL"), 1);
    assert!(sender.socket.multicast_loop_v4().expect("loopback"));
    sender.socket.send(b"source verification").expect("send");
    let mut buffer = [0; 64];
    let (length, source) = receiver.recv_from(&mut buffer).expect("receive");
    assert_eq!(source.ip(), IpAddr::V4(sender.source_ip));
    assert_eq!(&buffer[..length], b"source verification");
}

#[test]
fn interface_selection_covers_lan_and_virtual_lan_without_names_or_route_priority() {
    let addresses = eligible_interfaces([
        ("192.168.31.150".parse().unwrap(), true),
        ("10.88.0.2".parse().unwrap(), true),
        ("26.94.69.218".parse().unwrap(), true),
        ("100.65.2.3".parse().unwrap(), true),
        ("169.254.1.2".parse().unwrap(), true),
        ("192.168.31.150".parse().unwrap(), true),
        ("192.168.9.2".parse().unwrap(), false),
        ("127.0.0.1".parse().unwrap(), true),
        ("198.18.0.1".parse().unwrap(), true),
        ("8.8.8.8".parse().unwrap(), true),
        ("0.0.0.0".parse().unwrap(), true),
        ("239.255.76.71".parse().unwrap(), true),
        ("::1".parse().unwrap(), true),
    ])
    .expect("select interfaces");
    assert_eq!(
        addresses,
        [
            "10.88.0.2",
            "26.94.69.218",
            "100.65.2.3",
            "169.254.1.2",
            "192.168.31.150"
        ]
        .map(|address| address.parse().unwrap())
        .into()
    );
}

#[test]
fn interface_capacity_is_reported_instead_of_silently_omitting_networks() {
    let error =
        eligible_interfaces((1..=65).map(|host| (IpAddr::V4(Ipv4Addr::new(10, 1, 1, host)), true)))
            .expect_err("exceeds bounded interface set");
    assert!(error.contains("65 exceeds 64"));
}

#[test]
fn one_interface_creation_failure_preserves_other_networks_and_retries_independently() {
    let now = Instant::now();
    let failed = Ipv4Addr::new(10, 1, 1, 2);
    let healthy = Ipv4Addr::new(192, 168, 1, 2);
    let mut senders = DirectorySenders::default();
    senders.synchronize([failed, healthy].into(), now);
    let mut emitted = Vec::new();
    senders.publish_with(
        now,
        |address| {
            if address == failed {
                Err(String::from("interface unavailable"))
            } else {
                test_sender(address)
            }
        },
        |sender| {
            emitted.push(sender.source_ip);
            Ok(())
        },
        |_, _, _| {},
    );
    assert_eq!(emitted, [healthy]);
    assert_eq!(
        senders.interfaces[&failed].last_error.as_deref(),
        Some("interface unavailable")
    );
    assert!(senders.interfaces[&healthy].sender.is_some());
    assert_eq!(senders.next_delay(now), Duration::from_secs(1));
    senders.publish_with(
        now + Duration::from_millis(500),
        |_| panic!("retry too early"),
        |_| panic!("healthy network must preserve its five-second cadence"),
        |_, _, _| {},
    );
    senders.publish_with(
        now + Duration::from_secs(1),
        test_sender,
        |sender| {
            emitted.push(sender.source_ip);
            Ok(())
        },
        |_, _, _| {},
    );
    assert_eq!(emitted, [healthy, failed]);
    assert!(senders.interfaces[&failed].last_error.is_none());
}

#[test]
fn send_failure_does_not_abort_remaining_interfaces_or_recreate_healthy_sockets() {
    let now = Instant::now();
    let failed = Ipv4Addr::new(10, 1, 1, 2);
    let healthy = Ipv4Addr::new(192, 168, 1, 2);
    let mut senders = DirectorySenders::default();
    senders.synchronize([failed, healthy].into(), now);
    let mut emitted = Vec::new();
    senders.publish_with(
        now,
        test_sender,
        |sender| {
            emitted.push(sender.source_ip);
            if sender.source_ip == failed {
                Err(String::from("network disappeared"))
            } else {
                Ok(())
            }
        },
        |_, _, _| {},
    );
    assert_eq!(emitted, [failed, healthy]);
    assert!(senders.interfaces[&failed].sender.is_none());
    let healthy_port = senders.interfaces[&healthy]
        .sender
        .as_ref()
        .unwrap()
        .socket
        .local_addr()
        .unwrap();
    senders.publish_with(
        now + Duration::from_secs(1),
        test_sender,
        |_| Ok(()),
        |_, _, _| {},
    );
    assert_eq!(
        senders.interfaces[&healthy]
            .sender
            .as_ref()
            .unwrap()
            .socket
            .local_addr()
            .unwrap(),
        healthy_port
    );
}

#[test]
fn removed_interfaces_stop_publishing_and_new_interfaces_publish_immediately() {
    let now = Instant::now();
    let old = Ipv4Addr::new(192, 168, 1, 2);
    let new = Ipv4Addr::new(192, 168, 2, 2);
    let mut senders = DirectorySenders::default();
    senders.synchronize([old].into(), now);
    senders.publish_with(now, test_sender, |_| Ok(()), |_, _, _| {});
    senders.synchronize([new].into(), now + Duration::from_secs(1));
    let mut emitted = Vec::new();
    senders.publish_with(
        now + Duration::from_secs(1),
        test_sender,
        |sender| {
            emitted.push(sender.source_ip);
            Ok(())
        },
        |_, _, _| {},
    );
    assert_eq!(emitted, [new]);
    assert!(!senders.interfaces.contains_key(&old));
}

#[test]
fn repeated_interface_failure_is_logged_once_until_recovery() {
    let now = Instant::now();
    let address = Ipv4Addr::new(192, 168, 1, 2);
    let mut senders = DirectorySenders::default();
    senders.synchronize([address].into(), now);
    let mut reports = Vec::new();
    for elapsed in [0, 1] {
        senders.publish_with(
            now + Duration::from_secs(elapsed),
            |_| Err(String::from("interface unavailable")),
            |_| panic!("failed interface must not publish"),
            |level, action, message| {
                reports.push((level.to_owned(), action.to_owned(), message.to_owned()));
            },
        );
    }
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].0, "warning");
    assert_eq!(reports[0].1, "lan_directory.interface_failed");
    senders.publish_with(
        now + Duration::from_secs(3),
        test_sender,
        |_| Ok(()),
        |level, action, message| {
            reports.push((level.to_owned(), action.to_owned(), message.to_owned()));
        },
    );
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[1].0, "info");
    assert_eq!(reports[1].1, "lan_directory.interface_recovered");
}
