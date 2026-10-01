use super::*;
use crate::templates::fail_next_dst_setup_write_for_test;

struct PrivateDstSetupFixture {
    root: PathBuf,
    paths: StoragePaths,
    first: InstanceDetails,
    second: InstanceDetails,
    shared_setup: PathBuf,
}

fn dst_mod_update(details: &InstanceDetails, mod_id: &str) -> UpdateInstanceInput {
    let mut settings: Value = serde_json::from_str(&details.settings_json).unwrap();
    settings["shared_workshop_mod_ids"] = json!(mod_id);
    UpdateInstanceInput {
        id: details.summary.id.clone(),
        bind_ip: details.summary.bind_ip.clone(),
        auto_backup_on_stop: details.auto_backup_on_stop,
        backup_retention_count: details.backup_retention_count,
        settings_json: settings.to_string(),
        ports: details.ports.clone(),
    }
}

fn private_dst_setup_path(details: &InstanceDetails) -> PathBuf {
    Path::new(&details.config_file_path)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("runtime/mods/dedicated_server_mods_setup.lua")
}

impl PrivateDstSetupFixture {
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
        fs::write(&shared_setup, b"-- retained shared package source\n").unwrap();
        let mut instances = Vec::new();
        for (name, mod_id) in [("DST Alpha", "4111111111"), ("DST Beta", "4222222222")] {
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
            instances.push(
                update_instance(&paths, dst_mod_update(&details, mod_id))
                    .await
                    .unwrap(),
            );
        }
        let second = instances.pop().unwrap();
        let first = instances.pop().unwrap();
        // A later library download is separate from both owned installations.
        fs::create_dir_all(shared_setup.parent().unwrap()).unwrap();
        fs::write(&shared_setup, b"-- retained shared package source\n").unwrap();
        for (details, bytes) in [
            (&first, b"alpha saved world".as_slice()),
            (&second, b"beta saved world".as_slice()),
        ] {
            fs::create_dir_all(&details.saves_path).unwrap();
            fs::write(Path::new(&details.saves_path).join("world.sentinel"), bytes).unwrap();
        }
        Self {
            root,
            paths,
            first,
            second,
            shared_setup,
        }
    }

    fn protected_files(&self) -> Vec<(PathBuf, Vec<u8>)> {
        [
            PathBuf::from(&self.first.config_file_path),
            private_dst_setup_path(&self.first),
            Path::new(&self.first.saves_path).join("world.sentinel"),
            PathBuf::from(&self.second.config_file_path),
            private_dst_setup_path(&self.second),
            Path::new(&self.second.saves_path).join("world.sentinel"),
            self.shared_setup.clone(),
        ]
        .into_iter()
        .map(|path| {
            let bytes = fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect()
    }
}

#[tokio::test]
async fn archive_then_delete_private_dst_instance_preserves_peer_and_package_setup() {
    let fixture = PrivateDstSetupFixture::new().await;
    let before = fixture.protected_files();
    let first_root = Path::new(&fixture.first.config_file_path)
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let unowned = before
        .iter()
        .filter(|(path, _)| !path.starts_with(first_root))
        .cloned()
        .collect::<Vec<_>>();
    let deleted = archive_instance(&fixture.paths, &fixture.first.summary.id)
        .await
        .unwrap();
    let archive = PathBuf::from(
        deleted
            .archived_instance_root
            .expect("private instance archive"),
    );
    assert!(deleted.saves_archived_with_instance_root);
    assert!(!first_root.exists());
    for (path, expected) in before {
        let actual = match path.strip_prefix(first_root) {
            Ok(relative) => archive.join(relative),
            Err(_) => path,
        };
        assert_eq!(
            fs::read(&actual).unwrap(),
            expected,
            "changed {}",
            actual.display()
        );
    }
    let remaining = list_instances(&fixture.paths).await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, fixture.second.summary.id);
    restore_instance_archive(&fixture.paths, &deleted.archive_id)
        .await
        .unwrap();
    assert!(first_root.is_dir());
    let removed = delete_instance(&fixture.paths, &fixture.first.summary.id)
        .await
        .unwrap();
    assert!(!Path::new(&removed.deleted_instance_root).exists());
    for (path, expected) in unowned {
        assert_eq!(fs::read(path).unwrap(), expected);
    }
    let archives = list_instance_archives(&fixture.paths).await.unwrap();
    assert!(archives.archives.is_empty());
    assert!(archives.pending_deletions.is_empty());
    fs::remove_dir_all(fixture.root).unwrap();
}

#[tokio::test]
async fn private_dst_setup_write_failure_rolls_back_update_without_touching_peer() {
    let fixture = PrivateDstSetupFixture::new().await;
    let before = fixture.protected_files();
    let runtime = private_dst_setup_path(&fixture.first)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    fail_next_dst_setup_write_for_test(&runtime);
    let error = update_instance(&fixture.paths, dst_mod_update(&fixture.first, "4333333333"))
        .await
        .expect_err("injected private setup write must reject the update");
    assert!(
        matches!(error, StorageError::WriteConfig { .. }),
        "unexpected error: {error}"
    );
    for (path, expected) in before {
        assert_eq!(
            fs::read(&path).unwrap(),
            expected,
            "changed {}",
            path.display()
        );
    }
    let restored = read_instance_details(&fixture.paths, &fixture.first.summary.id)
        .await
        .unwrap();
    assert_eq!(restored.settings_json, fixture.first.settings_json);
    assert_eq!(
        serde_json::to_value(restored.ports).unwrap(),
        serde_json::to_value(&fixture.first.ports).unwrap()
    );
    fs::remove_dir_all(fixture.root).unwrap();
}

#[tokio::test]
async fn damaged_private_dst_runtime_cannot_write_any_mod_setup() {
    let fixture = PrivateDstSetupFixture::new().await;
    let before = fixture.protected_files();
    let runtime = private_dst_setup_path(&fixture.first)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    fs::remove_file(runtime.join(".langame-private-runtime")).unwrap();
    let error = update_instance(&fixture.paths, dst_mod_update(&fixture.first, "4333333333"))
        .await
        .expect_err("damaged private runtime must reject Mod writes");
    assert!(
        matches!(error, StorageError::PrivateRuntimeRefresh { .. }),
        "unexpected error: {error}"
    );
    assert!(
        materialize_instance_configuration_for_start(&fixture.paths, &fixture.first.summary.id)
            .await
            .is_err()
    );
    for (path, expected) in before {
        assert_eq!(
            fs::read(&path).unwrap(),
            expected,
            "changed {}",
            path.display()
        );
    }
    fs::remove_dir_all(fixture.root).unwrap();
}
