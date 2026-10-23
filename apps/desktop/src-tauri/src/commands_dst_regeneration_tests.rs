use super::*;
use serde_json::json;
use std::cell::RefCell;

#[tokio::test]
async fn dst_regeneration_stops_before_deleting_and_starts_last() {
    let log = RefCell::new(Vec::new());
    let events = &log;
    let preview = &DstWorldStartPreview {
        instance_id: "dst".into(),
        settings_json: "{}".into(),
        shards: Vec::new(),
    };
    let result = sequence(
        |phase| events.borrow_mut().push(format!("{phase:?}")),
        || async {
            events.borrow_mut().push("stop".into());
            Ok(())
        },
        || async {
            events.borrow_mut().push("delete".into());
            Ok(preview.clone())
        },
        |started| async move {
            assert_eq!(&started, preview);
            events.borrow_mut().push("start".into());
            Ok(42)
        },
    )
    .await;
    assert_eq!(result, Ok(42));
    assert_eq!(
        log.into_inner(),
        ["Stopping", "stop", "delete", "Generating", "start", "Done"]
    );
}

#[tokio::test]
async fn dst_regeneration_failure_skips_every_later_step() {
    for failing in ["stop", "delete"] {
        let log = RefCell::new(Vec::new());
        let events = &log;
        let result: Result<(), String> = sequence(
            |phase| events.borrow_mut().push(format!("{phase:?}")),
            || async {
                events.borrow_mut().push("stop".into());
                if failing == "stop" {
                    return Err("stop failed".into());
                }
                Ok(())
            },
            || async {
                events.borrow_mut().push("delete".into());
                Err("delete failed".into())
            },
            |_| async {
                events.borrow_mut().push("start".into());
                Ok(())
            },
        )
        .await;
        assert!(result.is_err());
        let expected: &[&str] = if failing == "stop" {
            &["Stopping", "stop", "Failed"]
        } else {
            &["Stopping", "stop", "delete", "Failed"]
        };
        assert_eq!(log.into_inner(), expected, "{failing}");
    }
}

#[test]
fn dst_regeneration_deletes_world_saves_and_keeps_configuration() {
    let root = std::env::temp_dir().join(format!(
        "lg-dst-regeneration-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ));
    let config = root.join("config/instance.json");
    let cluster = root.join("config/clusters/main");
    let kept = [
        "cluster.ini",
        "cluster_token.txt",
        "Master/server.ini",
        "Master/leveldataoverride.lua",
        "Master/modoverrides.lua",
        "Caves/server.ini",
        "Caves/worldgenoverride.lua",
    ];
    for file in kept {
        let path = cluster.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, file).unwrap();
    }
    for shard in ["Master", "Caves"] {
        let session = cluster.join(shard).join("save/session/A1B2C3D4E5F60789");
        std::fs::create_dir_all(&session).unwrap();
        std::fs::write(
            cluster.join(shard).join("save/shardindex"),
            "return { session_id = 'A1B2C3D4E5F60789' }",
        )
        .unwrap();
        std::fs::write(session.join("0000000001"), b"old world").unwrap();
        std::fs::write(session.join("0000000001.meta"), b"meta").unwrap();
    }
    let settings = json!({"enable_caves": true}).to_string();
    std::fs::write(&config, &settings).unwrap();
    let details: InstanceDetails = serde_json::from_value(json!({
        "summary": {"id": "dst", "name": "DST", "module_id": "dontstarve",
            "status": "Stopped", "bind_ip": "0.0.0.0", "port_count": 0, "autostart": false},
        "config_file_path": config.to_string_lossy(), "saves_path": cluster.to_string_lossy(),
        "auto_backup_on_stop": false, "backup_retention_count": 10,
        "settings_json": settings, "ports": [], "active_run": null
    }))
    .unwrap();
    let before = build_dst_world_start_preview(&details).unwrap();
    assert!(
        before
            .shards
            .iter()
            .any(|shard| shard.state == DstWorldState::Existing)
    );

    let preview = delete_world_saves(&details).unwrap();

    for shard in ["Master", "Caves"] {
        assert!(
            !cluster.join(shard).join("save").exists(),
            "{shard} world kept"
        );
    }
    for file in kept {
        assert_eq!(std::fs::read_to_string(cluster.join(file)).unwrap(), file);
    }
    assert!(
        preview
            .shards
            .iter()
            .filter(|shard| shard.enabled)
            .all(|shard| shard.state == DstWorldState::New)
    );
    assert_eq!(preview, build_dst_world_start_preview(&details).unwrap());
    // Deleting an already new world is a no-op.
    assert_eq!(delete_world_saves(&details).unwrap(), preview);
    std::fs::remove_dir_all(&root).unwrap();
}

/// The managed instance layout resolves save directories from the
/// instance's config file (`<instance>/config/clusters/main/<Shard>/save`), never
/// from `saves_path` or another cluster.
#[test]
fn dst_regeneration_resolves_each_shard_save_under_the_instance_cluster() {
    let root = std::env::temp_dir().join(format!(
        "lg-dst-regeneration-paths-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ));
    let instance = root.join("instances/don-t-starve-together-1-8dc70e89a8261411");
    let config = instance.join("config/instance.json");
    let cluster = instance.join("config/clusters/main");
    let decoys = [
        root.join("saves/Master/save/shardindex"),
        instance.join("config/clusters/other/Master/save/shardindex"),
    ];
    let worlds = [
        cluster.join("Master/save/shardindex"),
        cluster.join("Caves/save/shardindex"),
    ];
    for path in worlds.iter().chain(&decoys) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "return { session_id = '11A5458B79F426F8' }").unwrap();
    }
    let settings = json!({"enable_caves": true}).to_string();
    std::fs::write(&config, &settings).unwrap();
    let details: InstanceDetails = serde_json::from_value(json!({
        "summary": {"id": "dst", "name": "DST", "module_id": "dontstarve",
            "status": "Stopped", "bind_ip": "0.0.0.0", "port_count": 0, "autostart": false},
        "config_file_path": config.to_string_lossy(), "saves_path": root.join("saves").to_string_lossy(),
        "auto_backup_on_stop": false, "backup_retention_count": 10,
        "settings_json": settings, "ports": [], "active_run": null
    }))
    .unwrap();

    assert_eq!(
        dontstarve_cluster_root_from_config_file_path(&details.config_file_path),
        cluster
    );
    let preview = delete_world_saves(&details).unwrap();

    for shard in ["Master", "Caves"] {
        assert!(
            !cluster.join(shard).join("save").exists(),
            "{shard} world kept"
        );
        assert!(
            cluster.join(shard).is_dir(),
            "{shard} configuration removed"
        );
    }
    for decoy in &decoys {
        assert!(
            decoy.is_file(),
            "{} outside the instance cluster was touched",
            decoy.display()
        );
    }
    assert!(
        preview
            .shards
            .iter()
            .filter(|shard| shard.enabled)
            .all(|shard| shard.state == DstWorldState::New)
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn dst_regeneration_publishes_each_step_as_the_activity_bar_job() {
    let jobs = RwLock::new(AppState::default());
    for (phase, status, detail) in [
        (Phase::Stopping, "Running", "stopping"),
        (Phase::Generating, "Running", "generating"),
        (Phase::Done, "Completed", "done"),
    ] {
        report(&jobs, "dst", "Forest", phase);
        let state = jobs.read().unwrap();
        assert_eq!(state.jobs.len(), 1, "each step replaces the same job");
        // The frontend selects on exactly this wire shape (dst-regeneration-job.ts).
        let job = serde_json::to_value(&state.jobs[0]).unwrap();
        assert_eq!(job["id"], "dst-world-regeneration:dst");
        assert_eq!(job["kind"], "RegenerateWorld");
        assert_eq!(job["status"], status);
        assert_eq!(job["detail"], detail);
        assert_eq!(job["target_id"], "dst");
    }
}
