#[tokio::test]
async fn windrose_direct_transport_reserves_tcp_and_udp_together() {
    let root = unique_test_root();
    let mut paths = test_paths(&root);
    paths.modules_root = repo_root().join("modules");
    let descriptor = app_modules::discover_modules(&paths.modules_root)
        .unwrap()
        .into_iter()
        .find(|descriptor| descriptor.summary.id == "windrose")
        .unwrap();
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    prepare_shared_install(&paths, &descriptor);
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    let create = |name: &str| CreateInstanceInput {
        name: name.to_owned(),
        module_id: descriptor.summary.id.clone(),
    };
    let blocker = create_instance(&paths, &descriptor, create("TCP-only owner"))
        .await
        .unwrap();
    replace_instance_ports_for_test(
        &paths,
        &blocker.summary.id,
        &[PortBinding {
            name: "external_transport".to_owned(),
            protocol: "tcp".to_owned(),
            port: 7777,
        }],
    )
    .await;
    replenish_test_library(&paths, &descriptor).await;

    let created = create_instance(&paths, &descriptor, create("Windrose isolated transport"))
        .await
        .unwrap();
    let port = |bindings: &[PortBinding], name: &str| {
        bindings
            .iter()
            .find(|binding| binding.name == name)
            .unwrap()
            .port
    };
    assert_eq!(port(&created.ports, "direct"), 7778);
    assert_eq!(port(&created.ports, "direct_tcp"), 7778);

    let mut requested = created.ports.clone();
    requested
        .iter_mut()
        .find(|binding| binding.name == "direct_tcp")
        .unwrap()
        .port = 28017;
    let updated = update_instance_ports(&paths, &created.summary.id, &requested)
        .await
        .unwrap();
    assert_eq!(port(&updated, "direct"), 28017);
    assert_eq!(port(&updated, "direct_tcp"), 28017);
    let persisted = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(port(&persisted.ports, "direct"), 28017);
    assert_eq!(port(&persisted.ports, "direct_tcp"), 28017);
    cleanup_root(&root);
}
