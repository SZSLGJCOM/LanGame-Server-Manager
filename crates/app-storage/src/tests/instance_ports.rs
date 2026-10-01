include!("windrose_ports.rs");

/// Supply a missing synthetic library without copying a mutable instance runtime.
/// Independent creation preserves this package for subsequent fixtures.
async fn replenish_test_library(paths: &StoragePaths, descriptor: &ModuleDescriptor) {
    let relative = descriptor.install.as_ref()
        .map(|install| install.shared_game_dir.as_str())
        .unwrap_or(&descriptor.summary.id);
    let root = paths.games_root.join(relative);
    if !root.exists() {
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("fixture-program.bin"), b"clean synthetic server program").unwrap();
    }
    if descriptor.storage.program_sharing == app_modules::ModuleProgramSharing::Shared {
        sync_game_installs(paths, &[GameInstallSyncRecord {
            module_id: descriptor.summary.id.clone(),
            install_root: root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some("fixture-version".into()),
            mark_verified: true,
        }]).await.unwrap();
    }
}

#[tokio::test]
async fn instance_port_projection_reads_actual_ports_for_a_bounded_batch() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    let first = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Projection One"),
            module_id: descriptor.summary.id.clone(),
        },
    )
    .await
    .unwrap();
    replenish_test_library(&paths, &descriptor).await;
    let second = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Projection Two"),
            module_id: descriptor.summary.id.clone(),
        },
    )
    .await
    .unwrap();
    replace_instance_ports_for_test(
        &paths,
        &first.summary.id,
        &[
            PortBinding {
                name: String::from("game"),
                protocol: String::from("udp"),
                port: 31_015,
            },
            PortBinding {
                name: String::from("query"),
                protocol: String::from("udp"),
                port: 31_017,
            },
        ],
    )
    .await;
    replace_instance_ports_for_test(
        &paths,
        &second.summary.id,
        &[PortBinding {
            name: String::from("game"),
            protocol: String::from("udp"),
            port: 32_015,
        }],
    )
    .await;

    let projections = read_instance_port_projections(
        &paths,
        &[second.summary.id.clone(), first.summary.id.clone()],
    )
    .await
    .unwrap();

    assert_eq!(projections.len(), 2);
    assert_eq!(projections[0].instance_id, first.summary.id);
    assert_eq!(projections[0].ports[0].port, 31_015);
    assert_eq!(projections[0].ports[1].port, 31_017);
    assert_eq!(projections[1].instance_id, second.summary.id);
    assert_eq!(projections[1].ports[0].port, 32_015);
    cleanup_root(&root);
}

#[tokio::test]
async fn instance_port_projection_rejects_an_unbounded_instance_set_before_storage_access() {
    let instance_ids = (0..=MAX_INSTANCE_PORT_PROJECTION_INSTANCES)
        .map(|index| format!("instance-{index}"))
        .collect::<Vec<_>>();

    let error = read_instance_port_projections(&StoragePaths::default(), &instance_ids)
        .await
        .expect_err("oversized projection must fail");

    assert!(matches!(
        error,
        StorageError::InstancePortProjectionLimitExceeded {
            max: MAX_INSTANCE_PORT_PROJECTION_INSTANCES,
            actual
        } if actual == MAX_INSTANCE_PORT_PROJECTION_INSTANCES + 1
    ));
}

#[tokio::test]
async fn dst_default_player_ports_exhaust_the_lan_discovery_range_without_spilling_past_it() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let mut descriptor = test_descriptor(&root);
    descriptor.default_ports[1].name = String::from("caves");
    prepare_environment(&root, &descriptor);
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let mut created = Vec::new();
    for index in 0..10 {
        replenish_test_library(&paths, &descriptor).await;
        created.push(
            create_instance(
                &paths,
                &descriptor,
                CreateInstanceInput {
                    name: format!("DST LAN {}", index + 1),
                    module_id: String::from("dontstarve"),
                },
            )
            .await
            .unwrap(),
        );
    }
    assert_eq!(created[0].ports[0].port, 10_999);
    assert_eq!(created[9].ports[0].port, 11_017);
    assert_eq!(created[9].ports[1].port, 11_018);

    replenish_test_library(&paths, &descriptor).await;
    let error = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("DST LAN exhausted"),
            module_id: String::from("dontstarve"),
        },
    )
    .await
    .expect_err("automatic DST allocation must not spill beyond the LAN discovery range");
    assert!(matches!(
        error,
        StorageError::PortAllocationExhausted { .. }
    ));

    let wrapped = update_instance_ports(
        &paths,
        &created[0].summary.id,
        &[
            PortBinding {
                name: String::from("master"),
                protocol: String::from("udp"),
                port: 11_018,
            },
            PortBinding {
                name: String::from("caves"),
                protocol: String::from("udp"),
                port: 0,
            },
        ],
    )
    .await
    .expect("bounded allocation must wrap to a lower free LAN discovery port");
    assert_eq!(wrapped[0].port, 10_998);
    assert_eq!(wrapped[1].port, 0);

    let public_direct_ports = [
        PortBinding {
            name: String::from("master"),
            protocol: String::from("udp"),
            port: 12_000,
        },
        PortBinding {
            name: String::from("caves"),
            protocol: String::from("udp"),
            port: 12_001,
        },
    ];
    let updated = update_instance_ports(&paths, &created[0].summary.id, &public_direct_ports)
        .await
        .expect("public/direct-connect ports outside the LAN list range remain configurable");
    assert_eq!(updated[0].name, "master");
    assert_eq!(updated[0].port, 12_000);
    assert_eq!(updated[1].name, "caves");
    assert_eq!(updated[1].port, 12_001);

    cleanup_root(&root);
}

#[tokio::test]
async fn return_to_moria_shared_transport_port_is_coupled_on_create_and_update() {
    let root = unique_test_root();
    let mut paths = test_paths(&root);
    paths.modules_root = repo_root().join("modules");
    let descriptor = app_modules::discover_modules(&paths.modules_root)
        .expect("discover repo modules")
        .into_iter()
        .find(|descriptor| descriptor.summary.id == "returntomoria")
        .expect("missing Return to Moria module");

    assert_eq!(descriptor.runtime.port_groups.len(), 1);
    assert_eq!(descriptor.runtime.port_groups[0].id, "game_transport");
    assert_eq!(
        descriptor.runtime.port_groups[0].members,
        [String::from("game"), String::from("game_tcp")]
    );

    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    prepare_shared_install(&paths, &descriptor);
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    let create = |name: &str| CreateInstanceInput {
        name: String::from(name),
        module_id: String::from("returntomoria"),
    };
    let first = create_instance(&paths, &descriptor, create("Moria One"))
        .await
        .unwrap();
    replenish_test_library(&paths, &descriptor).await;
    let second = create_instance(&paths, &descriptor, create("Moria Two"))
        .await
        .unwrap();

    let port = |bindings: &[PortBinding], name: &str| {
        bindings
            .iter()
            .find(|binding| binding.name == name)
            .map(|binding| binding.port)
            .unwrap_or(0)
    };
    assert_eq!(port(&first.ports, "game"), 7777);
    assert_eq!(port(&first.ports, "game_tcp"), 7777);
    assert_eq!(port(&second.ports, "game"), 7778);
    assert_eq!(port(&second.ports, "game_tcp"), 7778);

    let mut requested = second.ports.clone();
    requested
        .iter_mut()
        .find(|binding| binding.name == "game")
        .unwrap()
        .port = 7000;
    let updated = update_instance_ports(&paths, &second.summary.id, &requested)
        .await
        .unwrap();
    assert_eq!(port(&updated, "game"), 7000);
    assert_eq!(port(&updated, "game_tcp"), 7000);

    let mut requested = updated;
    requested
        .iter_mut()
        .find(|binding| binding.name == "game_tcp")
        .unwrap()
        .port = 6900;
    let updated = update_instance_ports(&paths, &second.summary.id, &requested)
        .await
        .unwrap();
    assert_eq!(port(&updated, "game"), 6900);
    assert_eq!(port(&updated, "game_tcp"), 6900);

    cleanup_root(&root);
}

#[tokio::test]
async fn core_keeper_offset_port_group_allocates_and_updates_as_an_atomic_block() {
    assert_game_offset_group("corekeeper", 27_015, "query", "game_query").await;
}

#[tokio::test]
async fn valheim_offset_port_group_allocates_and_updates_as_an_atomic_block() {
    assert_game_offset_group("valheim", 2456, "query", "game_query").await;
}

#[tokio::test]
async fn ark_evolved_offset_port_group_allocates_and_updates_as_an_atomic_block() {
    assert_game_offset_group("arksurvivalevolved", 7777, "peer", "game_peer").await;
}

async fn assert_game_offset_group(module_id: &str, default_game_port: u16, secondary: &str, group_id: &str) {
    let root = unique_test_root();
    let mut paths = test_paths(&root);
    paths.modules_root = repo_root().join("modules");
    let descriptor = app_modules::discover_modules(&paths.modules_root)
        .expect("discover repo modules")
        .into_iter()
        .find(|descriptor| descriptor.summary.id == module_id)
        .expect("missing game/query offset module");

    let group = descriptor
        .runtime
        .port_groups
        .first()
        .expect("game/query offset group");
    assert_eq!(group.id, group_id);
    assert_eq!(group.members, [String::from("game"), String::from(secondary)]);
    assert_eq!(
        group.member_offsets,
        Some(std::collections::BTreeMap::from([
            (String::from("game"), 0),
            (String::from(secondary), 1),
        ]))
    );

    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    prepare_shared_install(&paths, &descriptor);
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    let create = |name: &str| CreateInstanceInput {
        name: String::from(name),
        module_id: String::from(module_id),
    };
    fs::create_dir_all(paths.games_root.join(module_id)).unwrap();
    let first = create_instance(&paths, &descriptor, create("Offset One"))
        .await
        .unwrap();
    replenish_test_library(&paths, &descriptor).await;
    let second = create_instance(&paths, &descriptor, create("Offset Two"))
        .await
        .unwrap();
    replenish_test_library(&paths, &descriptor).await;
    let third = create_instance(&paths, &descriptor, create("Offset Three"))
        .await
        .unwrap();

    let port = |bindings: &[PortBinding], name: &str| {
        bindings
            .iter()
            .find(|binding| binding.name == name)
            .map(|binding| binding.port)
            .unwrap_or(0)
    };
    assert_eq!(
        (port(&first.ports, "game"), port(&first.ports, secondary)),
        (default_game_port, default_game_port + 1)
    );
    assert_eq!(
        (port(&second.ports, "game"), port(&second.ports, secondary)),
        (default_game_port + 2, default_game_port + 3)
    );
    assert_eq!(
        (port(&third.ports, "game"), port(&third.ports, secondary)),
        (default_game_port + 4, default_game_port + 5)
    );

    let mut first_with_blocker = first.ports.clone();
    first_with_blocker.push(PortBinding {
        name: String::from("synthetic_neighbor_blocker"),
        protocol: String::from("udp"),
        port: 28_001,
    });
    replace_instance_ports_for_test(&paths, &first.summary.id, &first_with_blocker).await;

    let mut requested = third.ports.clone();
    requested
        .iter_mut()
        .find(|binding| binding.name == "game")
        .unwrap()
        .port = 28_000;
    requested
        .iter_mut()
        .find(|binding| binding.name == secondary)
        .unwrap()
        .port = 28_001;
    let shifted = update_instance_ports(&paths, &third.summary.id, &requested)
        .await
        .unwrap();
    assert_eq!(
        (port(&shifted, "game"), port(&shifted, secondary)),
        (28_002, 28_003)
    );

    let mut requested = shifted;
    requested
        .iter_mut()
        .find(|binding| binding.name == secondary)
        .unwrap()
        .port = 29_001;
    let updated = update_instance_ports(&paths, &third.summary.id, &requested)
        .await
        .unwrap();
    assert_eq!(
        (port(&updated, "game"), port(&updated, secondary)),
        (29_000, 29_001)
    );

    let mut overlapping = updated;
    overlapping
        .iter_mut()
        .find(|binding| binding.name == "game")
        .unwrap()
        .port = 29_001;
    let updated = update_instance_ports(&paths, &third.summary.id, &overlapping)
        .await
        .unwrap();
    assert_eq!(
        (port(&updated, "game"), port(&updated, secondary)),
        (29_001, 29_002)
    );

    let mut conflicting = updated.clone();
    conflicting
        .iter_mut()
        .find(|binding| binding.name == "game")
        .unwrap()
        .port = 30_000;
    conflicting
        .iter_mut()
        .find(|binding| binding.name == secondary)
        .unwrap()
        .port = 31_001;
    let error = update_instance_ports(&paths, &third.summary.id, &conflicting)
        .await
        .expect_err("conflicting member bases must fail atomically");
    assert!(matches!(
        error,
        StorageError::InvalidPortGroupRequest { .. }
    ));
    let persisted = read_instance_details(&paths, &third.summary.id)
        .await
        .unwrap();
    assert_eq!(
        (
            port(&persisted.ports, "game"),
            port(&persisted.ports, secondary)
        ),
        (29_001, 29_002)
    );

    let mut overflowing = persisted.ports.clone();
    overflowing
        .iter_mut()
        .find(|binding| binding.name == "game")
        .unwrap()
        .port = u16::MAX;
    let error = update_instance_ports(&paths, &third.summary.id, &overflowing)
        .await
        .expect_err("base plus query offset must not overflow u16");
    assert!(matches!(
        error,
        StorageError::PortAllocationExhausted { .. }
    ));
    let persisted = read_instance_details(&paths, &third.summary.id)
        .await
        .unwrap();
    assert_eq!(
        (
            port(&persisted.ports, "game"),
            port(&persisted.ports, secondary)
        ),
        (29_001, 29_002)
    );

    let mut all_zero = persisted.ports.clone();
    for binding in &mut all_zero {
        if binding.name == "game" || binding.name == secondary {
            binding.port = 0;
        }
    }
    let error = update_instance_ports(&paths, &third.summary.id, &all_zero)
        .await
        .expect_err("fixed-offset groups must reject an unusable zero base");
    assert!(matches!(
        error,
        StorageError::InvalidPortGroupRequest { .. }
    ));
    let persisted = read_instance_details(&paths, &third.summary.id)
        .await
        .unwrap();
    assert_eq!(
        (
            port(&persisted.ports, "game"),
            port(&persisted.ports, secondary)
        ),
        (29_001, 29_002)
    );

    cleanup_root(&root);
}

#[tokio::test]
async fn ark_evolved_creation_skips_an_entire_pair_when_only_native_peer_is_reserved() {
    let root = unique_test_root();
    let mut paths = test_paths(&root);
    paths.modules_root = repo_root().join("modules");
    let descriptor = app_modules::discover_modules(&paths.modules_root)
        .unwrap()
        .into_iter()
        .find(|descriptor| descriptor.summary.id == "arksurvivalevolved")
        .unwrap();
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    prepare_shared_install(&paths, &descriptor);
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor)).await.unwrap();
    let create = |name: &str| CreateInstanceInput {
        name: name.to_owned(), module_id: descriptor.summary.id.clone()
    };
    let blocker = create_instance(&paths, &descriptor, create("Other transport owner")).await.unwrap();
    replace_instance_ports_for_test(&paths, &blocker.summary.id, &[PortBinding {
        name: "external_transport".to_owned(), protocol: "udp".to_owned(), port: 7778
    }]).await;
    replenish_test_library(&paths, &descriptor).await;
    let created = create_instance(&paths, &descriptor, create("ARK paired ports")).await.unwrap();
    let port = |name: &str| created.ports.iter().find(|binding| binding.name == name).unwrap().port;
    assert_eq!(port("game"), 7779);
    assert_eq!(port("peer"), 7780);
    assert_eq!(port("query"), 27015);
    assert_eq!(port("rcon"), 27020);
    cleanup_root(&root);
}
