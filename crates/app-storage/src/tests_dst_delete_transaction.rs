use super::*;

struct DstDeleteFixture {
    root: PathBuf,
    paths: StoragePaths,
    instance: InstanceDetails,
    peer: InstanceDetails,
}

impl DstDeleteFixture {
    async fn new() -> Self {
        let root = unique_test_root();
        let paths = test_paths(&root);
        let descriptor = test_descriptor(&root);
        prepare_environment(&root, &descriptor);
        initialize_database(&paths).await.unwrap();
        sync_modules(&paths, std::slice::from_ref(&descriptor))
            .await
            .unwrap();
        let shared_setup = paths
            .games_root
            .join("dontstarve/mods/dedicated_server_mods_setup.lua");
        fs::create_dir_all(shared_setup.parent().unwrap()).unwrap();
        fs::write(
            shared_setup,
            b"-- shared package source must survive deletion\n",
        )
        .unwrap();
        let mut instances = Vec::new();
        for (name, mod_id) in [
            ("DST delete transaction", "4777777777"),
            ("DST retained peer", "4888888888"),
        ] {
            replenish_test_library(&paths, &descriptor).await;
            let created = create_instance(
                &paths,
                &descriptor,
                CreateInstanceInput {
                    name: name.to_owned(),
                    module_id: "dontstarve".to_owned(),
                },
            )
            .await
            .unwrap();
            let details = read_instance_details(&paths, &created.summary.id)
                .await
                .unwrap();
            let mut settings: Value = serde_json::from_str(&details.settings_json).unwrap();
            settings["shared_workshop_mod_ids"] = json!(mod_id);
            let details = update_instance(
                &paths,
                UpdateInstanceInput {
                    id: details.summary.id,
                    bind_ip: details.summary.bind_ip,
                    auto_backup_on_stop: details.auto_backup_on_stop,
                    backup_retention_count: details.backup_retention_count,
                    settings_json: settings.to_string(),
                    ports: details.ports,
                },
            )
            .await
            .unwrap();
            fs::create_dir_all(&details.saves_path).unwrap();
            fs::write(
                Path::new(&details.saves_path).join("world.sentinel"),
                format!("{name}\0{mod_id}"),
            )
            .unwrap();
            instances.push(details);
        }
        let peer = instances.pop().unwrap();
        let instance = instances.pop().unwrap();
        Self {
            root,
            paths,
            instance,
            peer,
        }
    }
}

fn dst_delete_file_snapshot(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    let mut snapshot = std::collections::BTreeMap::new();
    let mut directories = vec![root.to_owned()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                directories.push(path);
            } else {
                assert!(
                    entry.file_type().unwrap().is_file(),
                    "unexpected fixture link"
                );
                snapshot.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    snapshot
}

#[tokio::test]
async fn dst_delete_commit_failure_restores_database_archive_and_private_files() {
    let fixture = DstDeleteFixture::new().await;
    let instance_root = fixture
        .paths
        .instances_root
        .join(&fixture.instance.summary.id);
    let peer_root = fixture.paths.instances_root.join(&fixture.peer.summary.id);
    let before_instance = dst_delete_file_snapshot(&instance_root);
    let before_peer = dst_delete_file_snapshot(&peer_root);
    let before_package = dst_delete_file_snapshot(&fixture.paths.games_root);
    let before_ports = serde_json::to_value(&fixture.instance.ports).unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    for statement in [
        "CREATE TABLE fixture_delete_parent (id INTEGER PRIMARY KEY)",
        "CREATE TABLE fixture_delete_child (parent_id INTEGER REFERENCES fixture_delete_parent(id) DEFERRABLE INITIALLY DEFERRED)",
        "CREATE TRIGGER fixture_reject_delete_commit AFTER DELETE ON instances BEGIN INSERT INTO fixture_delete_child VALUES (1); END",
    ] {
        sqlx::query(statement).execute(&pool).await.unwrap();
    }
    pool.close().await;

    let error = delete_instance(&fixture.paths, &fixture.instance.summary.id)
        .await
        .expect_err("deferred constraint must reject deletion at commit");
    assert!(
        matches!(error, StorageError::Sqlx(_)),
        "unexpected deletion error: {error}"
    );
    let restored = read_instance_details(&fixture.paths, &fixture.instance.summary.id)
        .await
        .expect("failed deletion must retain the database instance");
    assert_eq!(restored.summary.id, fixture.instance.summary.id);
    assert_eq!(restored.summary.bind_ip, fixture.instance.summary.bind_ip);
    assert_eq!(serde_json::to_value(&restored.ports).unwrap(), before_ports);
    assert_eq!(
        dst_delete_file_snapshot(&instance_root),
        before_instance,
        "archive rollback must restore config, Mod setup and saves unchanged"
    );
    assert_eq!(dst_delete_file_snapshot(&peer_root), before_peer);
    assert_eq!(
        dst_delete_file_snapshot(&fixture.paths.games_root),
        before_package
    );
    let archive = fixture.paths.instances_root.join(".trash");
    assert!(!archive.exists() || fs::read_dir(archive).unwrap().next().is_none());
    let lock = crate::instance_settings_lock::acquire_instance_settings_mutation_lock(
        &fixture.paths,
        &fixture.instance.summary.id,
    )
    .expect("failed deletion must release its mutation lease");
    drop(lock);
    fs::remove_dir_all(fixture.root).unwrap();
}
