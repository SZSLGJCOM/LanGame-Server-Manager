use super::*;

struct Fixture {
    root: PathBuf,
    paths: StoragePaths,
    details: InstanceDetails,
}

impl Fixture {
    async fn running() -> Self {
        let (root, paths, created) =
            provision_real_module_instance("valheim", "Live role roster").await;
        let initial = read_instance_details(&paths, &created.summary.id)
            .await
            .unwrap();
        let mut settings: Value = serde_json::from_str(&initial.settings_json).unwrap();
        settings["world_name"] = json!("Preserved Rules");
        settings["world_preset"] = json!("hard");
        settings["world_modifiers"] = json!("combat veryhard\nresources muchmore");
        settings["world_set_keys"] = json!("nomap\nplayerevents");
        settings["admin_list"] = json!("Steam_76561198000000001");
        settings["permitted_list"] = json!("Xbox_Previous");
        settings["banned_list"] = json!("Existing Character");
        let input = |settings: &Value| UpdateInstanceInput {
            id: initial.summary.id.clone(),
            bind_ip: initial.summary.bind_ip.clone(),
            auto_backup_on_stop: initial.auto_backup_on_stop,
            backup_retention_count: initial.backup_retention_count,
            settings_json: settings.to_string(),
            ports: initial.ports.clone(),
        };
        update_instance(&paths, input(&settings)).await.unwrap();
        let world = Path::new(&initial.saves_path).join("worlds_local/Preserved Rules");
        fs::create_dir_all(&world).unwrap();
        fs::write(
            world.join("_main.1.fwl2"),
            b"opaque retained world metadata\0rules",
        )
        .unwrap();
        fs::write(
            world.join("_main.1.db2"),
            b"opaque retained world data\0progress",
        )
        .unwrap();
        record_started_test_instance(
            &paths,
            &initial.summary.id,
            12345,
            root.join("native-fixture.log").to_str().unwrap(),
        )
        .await
        .unwrap();
        settings["admin_list"] = json!("Steam_76561198000000001\nSteam_76561198000000002");
        settings["permitted_list"] = json!("Xbox_Previous\nXbox_Current");
        update_instance(&paths, input(&settings)).await.unwrap();
        let details = read_instance_details(&paths, &initial.summary.id)
            .await
            .unwrap();
        assert!(matches!(details.summary.status, InstanceStatus::Running));
        assert!(details.active_run.is_some());
        Self {
            root,
            paths,
            details,
        }
    }

    fn config_dir(&self) -> &Path {
        Path::new(&self.details.config_file_path).parent().unwrap()
    }

    fn native(&self, name: &str) -> PathBuf {
        Path::new(&self.details.saves_path).join(name)
    }

    fn snapshot(&self) -> Vec<(PathBuf, Vec<u8>)> {
        let mut paths = vec![PathBuf::from(&self.details.config_file_path)];
        for name in ["adminlist.txt", "permittedlist.txt", "bannedlist.txt"] {
            paths.push(self.config_dir().join(name));
            paths.push(self.native(name));
        }
        for name in ["_main.1.fwl2", "_main.1.db2"] {
            paths.push(
                Path::new(&self.details.saves_path)
                    .join("worlds_local/Preserved Rules")
                    .join(name),
            );
        }
        paths
            .into_iter()
            .map(|path| {
                let bytes = fs::read(&path).unwrap();
                (path, bytes)
            })
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        cleanup_root(&self.root);
    }
}

#[tokio::test]
async fn valheim_running_role_publication_changes_only_the_selected_two_files() {
    for (field, filename, added_id) in [
        ("admin_list", "adminlist.txt", "Steam_76561198000000002"),
        ("permitted_list", "permittedlist.txt", "Xbox_Current"),
    ] {
        let fixture = Fixture::running().await;
        let before = fixture.snapshot();
        materialize_instance_configuration(&fixture.paths, &fixture.details.summary.id)
            .await
            .unwrap();
        for (path, original) in &before {
            let actual = fs::read(path).unwrap();
            if path == Path::new(&fixture.details.config_file_path) {
                // General materialization refreshes its generation timestamp;
                // all committed configuration values must remain unchanged.
                let mut previous: Value = serde_json::from_slice(original).unwrap();
                let mut current: Value = serde_json::from_slice(&actual).unwrap();
                previous
                    .as_object_mut()
                    .unwrap()
                    .remove("generated_at_unix_ms");
                current
                    .as_object_mut()
                    .unwrap()
                    .remove("generated_at_unix_ms");
                assert!(
                    current == previous,
                    "general materialization preserves configuration values"
                );
            } else {
                assert!(
                    &actual == original,
                    "general materialization protects {}",
                    path.display()
                );
            }
        }
        assert!(
            !fs::read_to_string(fixture.native(filename))
                .unwrap()
                .contains(added_id)
        );
        // Preserve the committed document's bytes, including supported BOM.
        let config = Path::new(&fixture.details.config_file_path);
        let mut config_bytes = vec![0xef, 0xbb, 0xbf];
        config_bytes.extend(fs::read(config).unwrap());
        fs::write(config, &config_bytes).unwrap();
        let protected = fixture.snapshot();
        let applied =
            materialize_valheim_player_roster(&fixture.paths, &fixture.details.summary.id, field)
                .await
                .expect("explicit running role publication");
        assert_eq!(applied.settings_json, fixture.details.settings_json);
        assert!(applied.active_run.is_some());
        let settings: Value = serde_json::from_str(&applied.settings_json).unwrap();
        let expected = format!("{}\n", settings[field].as_str().unwrap()).into_bytes();
        for (path, original) in &protected {
            let actual = fs::read(path).unwrap();
            if *path == fixture.config_dir().join(filename) || *path == fixture.native(filename) {
                assert_eq!(actual, expected);
                assert_ne!(
                    &actual, original,
                    "the selected native roster must actually change"
                );
            } else {
                assert_eq!(&actual, original, "preserve unrelated {}", path.display());
            }
        }
        apply_instance_player_access_mutation(
            &fixture.paths,
            ApplyInstancePlayerAccessMutationInput {
                instance_id: fixture.details.summary.id.clone(),
                field_key: field.to_owned(),
                operation: PlayerAccessMutationOperation::Remove,
                value: json!(added_id),
                expected_value: None,
            },
        )
        .await
        .unwrap();
        assert!(
            fs::read_to_string(fixture.native(filename))
                .unwrap()
                .contains(added_id),
            "saving alone still cannot bypass the running guard"
        );
        let before_remove = fixture.snapshot();
        let removed =
            materialize_valheim_player_roster(&fixture.paths, &fixture.details.summary.id, field)
                .await
                .unwrap();
        let settings: Value = serde_json::from_str(&removed.settings_json).unwrap();
        for (path, original) in &before_remove {
            let actual = fs::read(path).unwrap();
            if *path == fixture.config_dir().join(filename) || *path == fixture.native(filename) {
                assert_eq!(
                    actual,
                    format!("{}\n", settings[field].as_str().unwrap()).into_bytes()
                );
                assert!(!String::from_utf8(actual).unwrap().contains(added_id));
            } else {
                assert_eq!(&actual, original);
            }
        }
    }
}

#[tokio::test]
async fn valheim_role_publication_rejects_unsupported_fields_and_wrong_modules() {
    let fixture = Fixture::running().await;
    let before = fixture.snapshot();
    for field in ["banned_list", "world_name", "../admin_list"] {
        assert!(
            materialize_valheim_player_roster(&fixture.paths, &fixture.details.summary.id, field)
                .await
                .is_err()
        );
        assert_eq!(fixture.snapshot(), before);
    }
    let (root, paths, created) =
        provision_real_module_instance("necesse", "Wrong roster module").await;
    let config = fs::read(&created.config_file_path).unwrap();
    assert!(
        materialize_valheim_player_roster(&paths, &created.summary.id, "admin_list")
            .await
            .is_err()
    );
    assert_eq!(fs::read(&created.config_file_path).unwrap(), config);
    cleanup_root(&root);
}

#[tokio::test]
async fn valheim_role_publication_rejects_invalid_selected_values_without_filtering_them() {
    for invalid in [json!("Steam_Valid\ninvalid entry"), json!(false)] {
        let fixture = Fixture::running().await;
        let config = Path::new(&fixture.details.config_file_path);
        let mut document: Value = serde_json::from_slice(&fs::read(config).unwrap()).unwrap();
        document["settings"]["admin_list"] = invalid;
        fs::write(config, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
        let before = fixture.snapshot();
        assert!(
            materialize_valheim_player_roster(
                &fixture.paths,
                &fixture.details.summary.id,
                "admin_list"
            )
            .await
            .is_err()
        );
        assert_eq!(
            fixture.snapshot(),
            before,
            "no silent invalid-entry removal or settings rewrite"
        );
    }
}

#[tokio::test]
async fn valheim_role_publication_rolls_back_the_first_file_when_native_write_fails() {
    let fixture = Fixture::running().await;
    let before = fixture.snapshot();
    crate::atomic_file::fail_next_atomic_write_for_test(&fixture.native("adminlist.txt"));
    let error = materialize_valheim_player_roster(
        &fixture.paths,
        &fixture.details.summary.id,
        "admin_list",
    )
    .await
    .expect_err("second-file publication failure");
    assert!(
        error
            .to_string()
            .contains("injected atomic configuration write failure")
    );
    assert_eq!(
        fixture.snapshot(),
        before,
        "config roster is compensated and all saved data remains intact"
    );
}

#[tokio::test]
async fn valheim_role_publication_rejects_a_nonordinary_native_leaf_before_writing_config() {
    let fixture = Fixture::running().await;
    let before = fixture.snapshot();
    let native = fixture.native("adminlist.txt");
    let native_before = fs::read(&native).unwrap();
    fs::remove_file(&native).unwrap();
    fs::create_dir(&native).unwrap();
    assert!(
        materialize_valheim_player_roster(
            &fixture.paths,
            &fixture.details.summary.id,
            "admin_list"
        )
        .await
        .is_err()
    );
    assert!(native.is_dir());
    assert!(fs::read_dir(&native).unwrap().next().is_none());
    fs::remove_dir(&native).unwrap();
    fs::write(&native, native_before).unwrap();
    assert_eq!(fixture.snapshot(), before);
}

#[cfg(windows)]
#[tokio::test]
async fn valheim_role_publication_rejects_a_save_root_junction_without_external_writes() {
    use std::os::windows::process::CommandExt;
    let fixture = Fixture::running().await;
    let before = fixture.snapshot();
    let cmd_path = |path: &Path| {
        let canonical = fs::canonicalize(path).unwrap();
        let value = canonical.to_str().unwrap();
        PathBuf::from(value.strip_prefix(r"\\?\").unwrap_or(value))
    };
    let saves = cmd_path(Path::new(&fixture.details.saves_path));
    let retained = saves.with_file_name("retained-saves");
    let outside = fixture.root.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("adminlist.txt"), b"outside native roster").unwrap();
    fs::rename(&saves, &retained).unwrap();
    let output = std::process::Command::new("cmd.exe")
        .args(["/C", "mklink", "/J"])
        .arg(&saves)
        .arg(cmd_path(&outside))
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "junction fixture: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        materialize_valheim_player_roster(
            &fixture.paths,
            &fixture.details.summary.id,
            "admin_list"
        )
        .await
        .is_err()
    );
    assert_eq!(
        fs::read(outside.join("adminlist.txt")).unwrap(),
        b"outside native roster"
    );
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
    fs::remove_dir(&saves).unwrap();
    fs::rename(retained, &saves).unwrap();
    assert_eq!(fixture.snapshot(), before);
}
