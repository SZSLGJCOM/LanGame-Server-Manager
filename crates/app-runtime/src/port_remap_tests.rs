use super::*;
use std::collections::BTreeMap;

fn binding(name: &str, protocol: &str, port: u16) -> PortBinding {
    PortBinding {
        name: name.to_owned(),
        protocol: protocol.to_owned(),
        port,
    }
}

fn assert_ports(actual: &[PortBinding], expected: &[PortBinding]) {
    let values = |ports: &[PortBinding]| {
        ports
            .iter()
            .map(|port| (port.name.clone(), port.protocol.clone(), port.port))
            .collect::<Vec<_>>()
    };
    assert_eq!(values(actual), values(expected));
}

fn group(secondary: &str, offset: u16) -> ModulePortGroupSpec {
    ModulePortGroupSpec {
        id: format!("game_{secondary}"),
        members: vec!["game".to_owned(), secondary.to_owned()],
        member_offsets: Some(BTreeMap::from([
            ("game".to_owned(), 0),
            (secondary.to_owned(), offset),
        ])),
    }
}

#[test]
fn occupied_derived_port_moves_the_complete_native_transport_block() {
    for (module, secondary) in [
        ("arksurvivalevolved", "peer"),
        ("valheim", "query"),
        ("corekeeper", "query"),
    ] {
        let ports = [
            binding("game", "udp", 7777),
            binding(secondary, "udp", 7778),
            binding("rcon", "tcp", 27020),
        ];
        let result = remap_with_probe(module, &ports, &[group(secondary, 1)], |port| {
            port.port != 7778
        })
        .unwrap()
        .unwrap();
        assert_ports(
            &result,
            &[
                binding("game", "udp", 7779),
                binding(secondary, "udp", 7780),
                ports[2].clone(),
            ],
        );
    }
}

#[test]
fn moving_game_also_moves_its_derived_port_and_preserves_binding_order() {
    let ports = [binding("peer", "udp", 7778), binding("game", "udp", 7777)];
    let result = remap_with_probe("arksurvivalevolved", &ports, &[group("peer", 1)], |port| {
        port.port != 7777
    })
    .unwrap()
    .unwrap();
    assert_ports(
        &result,
        &[binding("peer", "udp", 7779), binding("game", "udp", 7778)],
    );
}

#[test]
fn previously_split_bindings_are_reconciled_to_the_declared_primary() {
    let ports = [binding("game", "udp", 7777), binding("peer", "udp", 7790)];
    let result = remap_with_probe("arksurvivalevolved", &ports, &[group("peer", 1)], |_| true)
        .unwrap()
        .unwrap();
    assert_ports(
        &result,
        &[binding("game", "udp", 7777), binding("peer", "udp", 7778)],
    );
}

#[test]
fn equal_number_mixed_protocol_group_moves_together_when_one_protocol_is_busy() {
    let ports = [
        binding("game", "udp", 7777),
        binding("game_tcp", "tcp", 7777),
    ];
    let result = remap_with_probe("returntomoria", &ports, &[group("game_tcp", 0)], |port| {
        !(port.port == 7777 && port.protocol == "tcp")
    })
    .unwrap()
    .unwrap();
    assert_ports(
        &result,
        &[
            binding("game", "udp", 7778),
            binding("game_tcp", "tcp", 7778),
        ],
    );
}

#[test]
fn group_allocation_does_not_reuse_a_previously_reserved_endpoint() {
    let ports = [
        binding("other", "udp", 7779),
        binding("game", "udp", 7777),
        binding("peer", "udp", 7778),
    ];
    let result = remap_with_probe("arksurvivalevolved", &ports, &[group("peer", 1)], |port| {
        port.port != 7778
    })
    .unwrap()
    .unwrap();
    assert_ports(
        &result,
        &[
            ports[0].clone(),
            binding("game", "udp", 7780),
            binding("peer", "udp", 7781),
        ],
    );
}

#[test]
fn upper_boundary_is_atomic_and_exhaustion_never_returns_a_partial_block() {
    let ports = [binding("game", "udp", 65534), binding("peer", "udp", 65535)];
    assert!(
        remap_with_probe("arksurvivalevolved", &ports, &[group("peer", 1)], |_| true)
            .unwrap()
            .is_none()
    );
    assert!(
        remap_with_probe(
            "arksurvivalevolved",
            &ports,
            &[group("peer", 1)],
            |port| port.port != 65535
        )
        .unwrap_err()
        .contains("No free")
    );
    let invalid = [binding("game", "udp", 65535), binding("peer", "udp", 1)];
    assert!(
        remap_with_probe("arksurvivalevolved", &invalid, &[group("peer", 1)], |_| {
            true
        })
        .is_err()
    );
}

#[test]
fn malformed_groups_and_duplicate_endpoint_bindings_fail_before_probing() {
    let ports = [binding("game", "udp", 7777), binding("peer", "udp", 7778)];
    for groups in [
        vec![group("missing", 1)],
        vec![group("peer", 0)],
        vec![group("peer", 1), group("peer", 1)],
    ] {
        assert!(
            remap_with_probe("", &ports, &groups, |_| panic!(
                "invalid contract must not probe"
            ))
            .is_err()
        );
    }
    let duplicates = [binding("game", "udp", 7777), binding("peer", "udp", 7777)];
    assert!(
        remap_with_probe("", &duplicates, &[group("peer", 1)], |_| panic!(
            "duplicate endpoint must not probe"
        ))
        .is_err()
    );
}

#[test]
fn disabled_equal_number_groups_stay_disabled_but_derived_groups_reject_zero() {
    let ports = [binding("game", "udp", 0), binding("game_tcp", "tcp", 0)];
    assert!(
        remap_with_probe("", &ports, &[group("game_tcp", 0)], |_| panic!(
            "disabled bindings must not probe"
        ))
        .unwrap()
        .is_none()
    );
    let ports = [binding("game", "udp", 0), binding("peer", "udp", 1)];
    assert!(remap_with_probe("", &ports, &[group("peer", 1)], |_| true).is_err());
}

#[test]
fn dst_lan_range_wraps_and_exhausts_without_changing_the_contract() {
    let ports = [binding("master", "udp", 11018)];
    let result = remap_with_probe("dontstarve", &ports, &[], |port| port.port == 10998)
        .unwrap()
        .unwrap();
    assert_eq!(result[0].port, 10998);
    let mut probes = 0;
    let error = remap_with_probe("dontstarve", &ports, &[], |_| {
        probes += 1;
        false
    })
    .unwrap_err();
    assert!(error.contains("10998-11018"));
    assert_eq!(probes, 21);
}

#[test]
fn dst_four_shard_port_remap_wraps_inside_the_discovery_range_without_collisions() {
    let ports = app_core::dst_shards::DST_SHARDS
        .iter()
        .enumerate()
        .map(|(index, shard)| binding(shard.game_port, "udp", 11_018 - index as u16))
        .collect::<Vec<_>>();
    let remapped = remap_with_probe("dontstarve", &ports, &[], |port| {
        assert!((10_998..=11_018).contains(&port.port), "{port:?}");
        (10_998..=11_001).contains(&port.port)
    })
    .unwrap()
    .unwrap();
    assert_eq!(
        remapped.iter().map(|port| port.port).collect::<Vec<_>>(),
        [10_998, 10_999, 11_000, 11_001]
    );
}

#[test]
fn dst_islands_and_volcano_report_exhaustion_after_the_bounded_native_range() {
    for shard in &app_core::dst_shards::DST_SHARDS[2..] {
        let ports = [binding(shard.game_port, "udp", 11_018)];
        let mut probes = 0;
        let error = remap_with_probe("dontstarve", &ports, &[], |port| {
            assert!((10_998..=11_018).contains(&port.port), "{port:?}");
            probes += 1;
            false
        })
        .unwrap_err();
        assert_eq!(probes, 21);
        assert!(error.contains(shard.game_port), "{error}");
        assert!(error.contains("10998-11018"), "{error}");
    }
}

#[test]
fn live_external_udp_owner_causes_atomic_group_remap() {
    let occupied = (0..64)
        .find_map(|_| {
            let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
            (socket.local_addr().unwrap().port() < 65000).then_some(socket)
        })
        .expect("reserve an ephemeral UDP port with remapping headroom");
    let peer = occupied.local_addr().unwrap().port();
    let ports = [
        binding("game", "udp", peer - 1),
        binding("peer", "udp", peer),
    ];
    let result = remap_taken_port_bindings_for_module(
        "arksurvivalevolved",
        "127.0.0.1",
        &ports,
        &[group("peer", 1)],
    )
    .unwrap()
    .unwrap();
    assert_ne!(result[0].port, peer - 1);
    assert_ne!(result[0].port, peer);
    assert_ne!(result[1].port, peer);
    assert_eq!(result[1].port, result[0].port + 1);
}
