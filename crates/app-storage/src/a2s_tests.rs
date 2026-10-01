use super::*;

fn info_response() -> Vec<u8> {
    b"\xff\xff\xff\xffI\x11server\0map\0folder\0game\0\x01\x00\x03\x50\x00dw\x00\x01version\0\x00"
        .to_vec()
}

fn split_packet(id: u32, total: u8, index: u8, body: &[u8]) -> Vec<u8> {
    let mut packet = vec![0xfe, 0xff, 0xff, 0xff];
    packet.extend_from_slice(&id.to_le_bytes());
    packet.extend_from_slice(&[total, index, 0, 4]);
    packet.extend_from_slice(body);
    packet
}

fn receive_packets(packets: &[Vec<u8>]) -> Result<Vec<u8>, A2sQueryError> {
    let receiver = UdpSocket::bind("127.0.0.1:0").expect("fixture receiver");
    let sender = UdpSocket::bind("127.0.0.1:0").expect("fixture sender");
    for packet in packets {
        sender
            .send_to(packet, receiver.local_addr().expect("address"))
            .expect("fixture send");
    }
    receive_message(&receiver, Instant::now() + Duration::from_secs(1))
}

#[test]
fn split_packets_are_reassembled_by_index_and_identical_duplicates_are_safe() {
    let full = info_response();
    let first = split_packet(7, 3, 0, &full[..8]);
    let second = split_packet(7, 3, 1, &full[8..12]);
    let last = split_packet(7, 3, 2, &full[12..]);
    assert_eq!(
        receive_packets(&[last, first.clone(), first, second]).expect("complete split"),
        full
    );
}

#[test]
fn compressed_mismatched_and_conflicting_split_packets_fail_closed() {
    for packets in [
        vec![split_packet(0x80000000, 1, 0, b"compressed")],
        vec![
            split_packet(1, 2, 0, b"first"),
            split_packet(2, 2, 1, b"last"),
        ],
        vec![
            split_packet(1, 2, 0, b"first"),
            split_packet(1, 2, 0, b"conflict"),
        ],
        vec![split_packet(1, 17, 0, b"too many")],
        vec![split_packet(1, 0, 0, b"empty count")],
        vec![split_packet(1, 2, 2, b"invalid index")],
    ] {
        assert!(matches!(
            receive_packets(&packets),
            Err(A2sQueryError::InvalidResponse(_))
        ));
    }
}

fn server_socket() -> UdpSocket {
    let socket = UdpSocket::bind("127.0.0.1:0").expect("fixture server");
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("fixture timeout");
    socket
}

#[test]
fn production_player_count_accepts_split_info_response_after_challenge() {
    let socket = server_socket();
    let address = socket.local_addr().expect("address");
    let server = std::thread::spawn(move || {
        let mut buf = [0; 128];
        let (size, peer) = socket.recv_from(&mut buf).expect("info request");
        assert_eq!(&buf[..size], b"\xff\xff\xff\xffTSource Engine Query\0");
        socket
            .send_to(b"\xff\xff\xff\xffA1234", peer)
            .expect("challenge");
        let (size, second_peer) = socket.recv_from(&mut buf).expect("challenge response");
        assert_eq!(peer, second_peer);
        assert_eq!(
            &buf[..size],
            b"\xff\xff\xff\xffTSource Engine Query\x001234"
        );
        let full = info_response();
        for packet in [
            split_packet(12, 2, 1, &full[10..]),
            split_packet(12, 2, 0, &full[..10]),
        ] {
            socket.send_to(&packet, peer).expect("info fragment");
        }
    });
    let result = crate::query_live_player_count("a2s_info", "127.0.0.1", address.port());
    server.join().expect("fixture server");
    assert_eq!(
        result,
        Some(crate::QueriedPlayerCount {
            current_players: 3,
            max_players: 80,
        })
    );
}

#[test]
fn first_request_loss_does_not_consume_the_challenge_round_trip() {
    let socket = server_socket();
    let address = socket.local_addr().expect("address");
    let server = std::thread::spawn(move || {
        let mut buf = [0; 128];
        let (first_size, first_peer) = socket.recv_from(&mut buf).expect("lost initial request");
        assert_eq!(
            &buf[..first_size],
            b"\xff\xff\xff\xffTSource Engine Query\0"
        );
        // No arbitrary sleep: the server intentionally drops the first request
        // and responds only once the client has retransmitted it.
        let (size, peer) = socket.recv_from(&mut buf).expect("retransmission");
        assert_eq!(peer, first_peer);
        assert_eq!(&buf[..size], b"\xff\xff\xff\xffTSource Engine Query\0");
        socket
            .send_to(b"\xff\xff\xff\xffAabcd", peer)
            .expect("late challenge");
        let (size, peer) = socket.recv_from(&mut buf).expect("challenge response");
        assert_eq!(&buf[..size], b"\xff\xff\xff\xffTSource Engine Query\0abcd");
        socket.send_to(&info_response(), peer).expect("info");
    });
    let result = crate::query_live_player_count("a2s_info", "127.0.0.1", address.port());
    server.join().expect("fixture server");
    assert_eq!(result.map(|count| count.current_players), Some(3));
}

#[test]
fn unexpected_reply_type_and_unbounded_budgets_are_rejected() {
    let socket = server_socket();
    let address = socket.local_addr().expect("address");
    for budget in [Duration::ZERO, Duration::from_secs(4)] {
        assert!(matches!(
            A2sClient::connect(address, budget),
            Err(A2sQueryError::InvalidBudget)
        ));
    }
    let server = std::thread::spawn(move || {
        let mut buf = [0; 128];
        let (_, peer) = socket.recv_from(&mut buf).expect("info request");
        socket
            .send_to(b"\xff\xff\xff\xffD\0", peer)
            .expect("wrong reply");
    });
    let result = A2sClient::connect(address, Duration::from_secs(1))
        .expect("client")
        .info();
    server.join().expect("fixture server");
    assert!(matches!(result, Err(A2sQueryError::InvalidResponse(_))));
}

#[test]
fn incomplete_fragments_cannot_outlive_the_query_deadline() {
    let receiver = UdpSocket::bind("127.0.0.1:0").expect("receiver");
    let sender = UdpSocket::bind("127.0.0.1:0").expect("sender");
    sender
        .send_to(
            &split_packet(1, 2, 0, b"incomplete"),
            receiver.local_addr().expect("address"),
        )
        .expect("partial fragment");
    assert!(matches!(
        receive_message(&receiver, Instant::now() + Duration::from_millis(20)),
        Err(A2sQueryError::TimedOut)
    ));
}
