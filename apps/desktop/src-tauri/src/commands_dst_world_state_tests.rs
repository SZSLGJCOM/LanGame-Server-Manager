use super::*;
use serde_json::json;
use std::fs;
use std::path::PathBuf;

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lg-dst-world-preview-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove isolated preview fixture");
    }
}

fn write_snapshot(cluster: &Path, shard: &str) {
    let save = cluster.join(shard).join("save");
    let session = save.join("session/A1B2C3D4E5F60789");
    fs::create_dir_all(&session).unwrap();
    fs::write(
        save.join("shardindex"),
        "return { session_id = 'A1B2C3D4E5F60789' }",
    )
    .unwrap();
    fs::write(
        session.join("0000000001"),
        b"\0synthetic native world snapshot",
    )
    .unwrap();
    fs::write(
        session.join("0000000001.meta"),
        b"\0synthetic native snapshot metadata",
    )
    .unwrap();
}

#[cfg(windows)]
#[test]
fn dst_world_preview_rejects_a_linked_session_parent() {
    use std::os::windows::process::CommandExt;
    let root = TestRoot::new();
    let cluster = root.0.join("cluster");
    write_snapshot(&cluster, "Master");
    let session = cluster.join("Master/save/session");
    let external = root.0.join("external_sessions");
    fs::rename(&session, &external).unwrap();
    // Directory junctions exercise the same reparse boundary without requiring
    // the workstation's optional symbolic-link privilege.
    let linked = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(session.to_string_lossy().replace('/', "\\"))
        .arg(external.to_string_lossy().replace('/', "\\"))
        .creation_flags(crate::commands::CREATE_NO_WINDOW)
        .output()
        .expect("create isolated session junction");
    assert!(
        linked.status.success(),
        "{}",
        String::from_utf8_lossy(&linked.stderr)
    );
    let inspected = inspect_dst_world_shards(&cluster, &json!({ "enable_caves": false })).unwrap();
    fs::remove_dir(&session).expect("remove only the fixture link before cleanup");
    assert_eq!(inspected[0].state, DstWorldState::Unrecognized);
    assert!(external.join("A1B2C3D4E5F60789/0000000001").is_file());
}

#[test]
fn dst_world_preview_missing_empty_and_configuration_only_shards_are_new() {
    let root = TestRoot::new();
    let cluster = root.0.join("cluster");
    assert!(
        inspect_dst_world_shards(&cluster, &json!({ "enable_caves": true }))
            .unwrap()
            .iter()
            .all(|shard| shard.state == DstWorldState::New)
    );
    fs::create_dir_all(cluster.join("Master")).unwrap();
    fs::create_dir_all(cluster.join("Caves")).unwrap();
    for name in [
        "server.ini",
        "worldgenoverride.lua",
        "leveldataoverride.lua",
        "modoverrides.lua",
    ] {
        fs::write(cluster.join("Master").join(name), "managed configuration").unwrap();
    }
    fs::write(cluster.join("cluster.ini"), "managed cluster configuration").unwrap();
    assert!(
        inspect_dst_world_shards(&cluster, &json!({ "enable_caves": true }))
            .unwrap()
            .iter()
            .all(|shard| shard.state == DstWorldState::New)
    );
}

#[test]
fn dst_world_preview_native_snapshot_pairs_are_existing_and_disabled_caves_are_reported() {
    let root = TestRoot::new();
    write_snapshot(&root.0, "Master");
    write_snapshot(&root.0, "Caves");
    let states = inspect_dst_world_shards(&root.0, &json!({ "enable_caves": false })).unwrap();
    assert_eq!(states[0].state, DstWorldState::Existing);
    assert!(states[0].enabled);
    assert_eq!(states[1].state, DstWorldState::Existing);
    assert!(!states[1].enabled);
    assert!(
        root.0
            .join("Caves/save/session/A1B2C3D4E5F60789/0000000001")
            .is_file()
    );
}

#[test]
fn dst_world_preview_recognizes_all_island_adventures_worlds() {
    let root = TestRoot::new();
    for shard in ["Master", "Caves", "Islands", "Volcano"] {
        write_snapshot(&root.0, shard);
    }
    let states = inspect_dst_world_shards(
        &root.0,
        &json!({ "shard_layout": "island_adventures", "enable_caves": false }),
    )
    .unwrap();
    assert_eq!(
        states
            .iter()
            .map(|state| state.shard.as_str())
            .collect::<Vec<_>>(),
        ["Master", "Caves", "Islands", "Volcano"]
    );
    assert!(states.iter().all(|state| state.enabled));
    assert!(
        states
            .iter()
            .all(|state| state.state == DstWorldState::Existing)
    );
}

#[test]
fn dst_world_preview_reports_disabled_island_world_data_in_standard_layout() {
    let root = TestRoot::new();
    for shard in ["Islands", "Volcano"] {
        write_snapshot(&root.0, shard);
    }
    let states = inspect_dst_world_shards(&root.0, &json!({ "shard_layout": "standard" })).unwrap();
    assert_eq!(states.len(), 4);
    for state in &states[2..] {
        assert!(!state.enabled);
        assert_eq!(state.state, DstWorldState::Existing);
        assert!(root.0.join(&state.shard).join("save/shardindex").is_file());
    }
}

#[test]
fn dst_world_preview_does_not_accept_an_orphan_snapshot() {
    let root = TestRoot::new();
    write_snapshot(&root.0, "Master");
    fs::write(
        root.0.join("Master/save/shardindex"),
        "KLEI     1 return { session_id = '0011223344556677' }",
    )
    .unwrap();

    assert_eq!(
        inspect_dst_world_shards(&root.0, &json!({ "enable_caves": false })).unwrap()[0].state,
        DstWorldState::Unrecognized
    );
}

#[test]
fn dst_world_preview_rejects_additional_shard_directories() {
    let root = TestRoot::new();
    write_snapshot(&root.0, "Master");
    write_snapshot(&root.0, "Moon");

    let error = inspect_dst_world_shards(&root.0, &json!({ "enable_caves": false })).unwrap_err();
    assert!(error.contains("Moon"), "{error}");
    assert!(error.contains("Islands and Volcano"), "{error}");
}

#[test]
fn dst_world_preview_partial_saves_and_unknown_data_are_not_new() {
    let root = TestRoot::new();
    fs::create_dir_all(root.0.join("Master/save")).unwrap();
    fs::write(root.0.join("Master/save/shardindex"), "partial index").unwrap();
    fs::create_dir_all(root.0.join("Caves")).unwrap();
    fs::write(root.0.join("Caves/world.backup"), "unrecognized data").unwrap();
    assert!(
        inspect_dst_world_shards(&root.0, &json!({ "enable_caves": true }))
            .unwrap()
            .iter()
            .take(2)
            .all(|shard| shard.state == DstWorldState::Unrecognized)
    );
    write_snapshot(&root.0, "Master");
    fs::remove_file(
        root.0
            .join("Master/save/session/A1B2C3D4E5F60789/0000000001.meta"),
    )
    .unwrap();
    assert_eq!(
        inspect_dst_world_shards(&root.0, &json!({ "enable_caves": true })).unwrap()[0].state,
        DstWorldState::Unrecognized
    );
}

#[test]
fn dst_world_preview_filesystem_errors_are_propagated() {
    let root = TestRoot::new();
    let file = root.0.join("not-a-directory");
    fs::write(&file, "fixture").unwrap();
    let error = inspect_dst_world_shards(&file.join("cluster"), &json!({ "enable_caves": false }))
        .unwrap_err();
    assert!(error.contains("Failed to inspect"), "{error}");
}

#[test]
fn dst_world_preview_unknown_cluster_data_is_not_new() {
    let root = TestRoot::new();
    fs::write(root.0.join("orphan-world-data"), "retained data").unwrap();
    assert!(
        inspect_dst_world_shards(&root.0, &json!({ "enable_caves": true }))
            .unwrap()
            .iter()
            .all(|shard| shard.state == DstWorldState::Unrecognized)
    );
}

#[test]
fn dst_world_preview_scan_budget_cannot_turn_uninspected_data_into_new_worlds() {
    let root = TestRoot::new();
    write_snapshot(&root.0, "Master");
    assert_eq!(
        inspect_shard(&root.0.join("Master"), &mut 0).unwrap(),
        DstWorldState::Unrecognized
    );
}

#[test]
fn dst_world_preview_directory_disguised_as_cluster_configuration_is_not_new() {
    let root = TestRoot::new();
    fs::create_dir(root.0.join("cluster.ini")).unwrap();
    fs::write(root.0.join("cluster.ini/retained-world"), "data").unwrap();
    assert!(
        inspect_dst_world_shards(&root.0, &json!({ "enable_caves": true }))
            .unwrap()
            .iter()
            .all(|shard| shard.state == DstWorldState::Unrecognized)
    );
}

#[test]
fn dst_world_preview_configuration_and_current_logs_without_save_are_new() {
    let root = TestRoot::new();
    let master = root.0.join("Master");
    fs::create_dir(&master).unwrap();
    for name in [
        "server.ini",
        "worldgenoverride.lua",
        "server_log.txt",
        "server_chat_log.txt",
    ] {
        fs::write(master.join(name), "synthetic startup configuration or log").unwrap();
    }
    assert_eq!(
        inspect_dst_world_shards(&root.0, &json!({ "enable_caves": true })).unwrap()[0].state,
        DstWorldState::New
    );
}

#[test]
fn dst_world_preview_native_rotated_log_tree_without_save_is_new() {
    let root = TestRoot::new();
    for category in ["server_log", "server_chat_log"] {
        let archive = root.0.join("Master/backup").join(category);
        fs::create_dir_all(&archive).unwrap();
        fs::write(
            archive.join(format!("{category}_2026-09-19-09-06-24.txt")),
            "synthetic rotated log",
        )
        .unwrap();
    }
    assert_eq!(
        inspect_dst_world_shards(&root.0, &json!({ "enable_caves": false })).unwrap()[0].state,
        DstWorldState::New
    );
}

#[test]
fn dst_world_preview_log_names_cannot_hide_unknown_data_or_directories() {
    for unknown in [
        "Master/server_log.txt/retained-world",
        "Master/backup/server_log/unknown-world.bin",
        "Master/backup/server_log/server_log_2026-09-19-09-06-24.txt/retained-world",
        "Master/backup/save/session/data",
        "Master/backup/server_chat_log/server_chat_log_not-a-timestamp.txt",
        "Master/save/session/data",
    ] {
        let root = TestRoot::new();
        let file = root.0.join(unknown);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "retained unknown data").unwrap();
        assert_eq!(
            inspect_dst_world_shards(&root.0, &json!({ "enable_caves": true })).unwrap()[0].state,
            DstWorldState::Unrecognized,
            "{unknown}"
        );
    }
}

fn confirmation_fixture(root: &TestRoot) -> (app_core::InstanceDetails, DstWorldStartPreview) {
    let config = root.0.join("instance.json");
    let settings = "{\"enable_caves\":false,\"world_seed\":\"reviewed\"}";
    fs::write(&config, settings).unwrap();
    let instance: app_core::InstanceDetails = serde_json::from_value(serde_json::json!({
        "summary": { "id": "dst-confirmation", "name": "DST", "module_id": "dontstarve",
            "status": "Stopped", "bind_ip": "0.0.0.0", "port_count": 0, "autostart": false },
        "config_file_path": config.to_string_lossy(), "saves_path": "", "auto_backup_on_stop": false,
        "backup_retention_count": 10, "settings_json": settings, "ports": [], "active_run": null
    })).unwrap();
    let expected = DstWorldStartPreview {
        instance_id: instance.summary.id.clone(),
        settings_json: settings.to_owned(),
        shards: inspect_dst_world_shards(
            &root.0.join("clusters/main"),
            &json!({ "enable_caves": false }),
        )
        .unwrap(),
    };
    (instance, expected)
}

#[test]
fn dst_world_preview_confirmation_rejects_settings_written_after_review() {
    let root = TestRoot::new();
    let (mut instance, expected) = confirmation_fixture(&root);
    validate_dst_world_start_confirmation(&expected, &instance).unwrap();
    fs::write(
        &instance.config_file_path,
        "{\"enable_caves\":false,\"world_seed\":\"changed\"}",
    )
    .unwrap();
    instance.settings_json = fs::read_to_string(&instance.config_file_path).unwrap();
    let error = validate_dst_world_start_confirmation(&expected, &instance).unwrap_err();
    assert!(error.contains("dst_world_start_changed"), "{error}");
    assert!(
        !error.contains("world_seed"),
        "configuration values must not enter errors"
    );
}

#[test]
fn dst_world_preview_confirmation_rejects_world_created_after_review() {
    let root = TestRoot::new();
    let (instance, expected) = confirmation_fixture(&root);
    write_snapshot(&root.0.join("clusters/main"), "Master");
    let error = validate_dst_world_start_confirmation(&expected, &instance).unwrap_err();
    assert!(error.contains("dst_world_start_changed"), "{error}");
}
