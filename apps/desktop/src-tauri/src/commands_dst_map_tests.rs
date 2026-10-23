use super::*;

#[test]
fn map_error_codes_are_stable_bounded_and_do_not_infer_stages_from_messages() {
    for (code, expected) in [
        (MapErrorCode::Busy, "BUSY"),
        (MapErrorCode::ShardNotRunning, "SHARD_NOT_RUNNING"),
        (MapErrorCode::ShardNotEnabled, "SHARD_NOT_ENABLED"),
        (MapErrorCode::ShardChanged, "SHARD_CHANGED"),
        (MapErrorCode::ConsoleUnavailable, "CONSOLE_UNAVAILABLE"),
        (MapErrorCode::WorldNotReady, "WORLD_NOT_READY"),
        (MapErrorCode::GeometryChanged, "GEOMETRY_CHANGED"),
        (MapErrorCode::ResponseTimeout, "RESPONSE_TIMEOUT"),
        (MapErrorCode::ArtworkWorkerFailed, "ARTWORK_WORKER_FAILED"),
    ] {
        let prefix = format!("DST_MAP_{expected}: ");
        let failure = map_error(
            code,
            format!("The shard world is not ready.\n{}\0", "界".repeat(600)),
        );
        let message = failure.strip_prefix(&prefix).unwrap();
        assert_eq!(message.chars().count(), 512);
        assert!(!message.chars().any(char::is_control));
        assert!(message.len() <= 2048);
    }
}

#[test]
fn inventory_completion_requires_all_actual_landmarks_to_agree_and_excludes_avatars() {
    let landmark = |id: &str, icon: &str, atlas: &str, element: &str| {
        serde_json::json!({
            "id":id,"prefab":"entity","icon":icon,"priority":0,"mapX":0,"mapZ":0,
            "inventoryIcon":{"atlas":atlas,"element":element},
        })
    };
    let base = serde_json::json!({
        "instanceId":"owned","shard":"master","runId":1,"capturedAtUnixMs":1,
        "session":"fixture","geometryRevision":"","width":1,"height":1,"tileScale":4,
        "day":1,"season":"autumn","phase":"day","tiles":[1],
        "tileNames":{},"tileKinds":{},"tileTextures":{},"tileBaseTextures":{},"tileColors":{},
        "terrainLayers":[],"landmarks":[landmark("1","Map.PNG","../mods/enabled/item.xml","Item.tex"),
            landmark("2","map.png","../mods/enabled/item.xml","item.TEX")],
        "roads":[],"landmarksTruncated":false,"art":null,"players":[],
    });
    let snapshot: DstMapSnapshot = serde_json::from_value(base.clone()).unwrap();
    let declarations = entity_inventory_icon_sources(&snapshot);
    assert_eq!(declarations.len(), 1);
    assert_eq!(declarations[0].icon, "map.png");
    assert_eq!(declarations[0].atlas, "../mods/enabled/item.xml");
    for mutation in ["missing", "element", "atlas", "truncated", "avatar"] {
        let mut value = base.clone();
        match mutation {
            "missing" => value["landmarks"][1]["inventoryIcon"] = serde_json::Value::Null,
            "element" => value["landmarks"][1]["inventoryIcon"]["element"] = "other.tex".into(),
            "atlas" => {
                value["landmarks"][1]["inventoryIcon"]["atlas"] = "../mods/enabled/Item.xml".into()
            }
            "truncated" => value["landmarksTruncated"] = true.into(),
            "avatar" => {
                value["players"] = serde_json::json!([{"id":"real","name":"player","character":"character",
                "icon":"MAP.png","x":0,"z":0,"mapX":0,"mapZ":0}])
            }
            _ => unreachable!(),
        }
        let snapshot: DstMapSnapshot = serde_json::from_value(value).unwrap();
        assert!(
            entity_inventory_icon_sources(&snapshot).is_empty(),
            "{mutation}"
        );
    }
    let mut snapshot: DstMapSnapshot = serde_json::from_value(base).unwrap();
    snapshot.icons_uncertain = true;
    assert!(entity_inventory_icon_sources(&snapshot).is_empty());
}

#[test]
fn edge_requests_use_final_layers_and_include_only_ocean_without_native_color() {
    let layer = |id: u16, style: Option<u8>| {
        serde_json::json!({
            "tileId":id,"atlas":format!("atlas{id}.xml"),"texture":format!("base{id}.tex"),
            "noiseTexture":format!("noise{id}.tex"),"sampleStyle":style,
        })
    };
    let mut layers = vec![
        layer(1, Some(0)),
        layer(2, None),
        layer(3, Some(0)),
        layer(4, None),
        layer(5, Some(1)),
        layer(6, Some(2)),
        layer(7, Some(3)),
        layer(8, Some(9)),
        layer(9, Some(4)),
    ];
    let mut bad_final = layer(4, None);
    bad_final["atlas"] = serde_json::Value::Null;
    layers.push(bad_final);
    layers.push(layer(8, Some(0)));
    let snapshot: protocol::DstMapSnapshot=serde_json::from_value(serde_json::json!({
        "instanceId":"owned","shard":"master","runId":1,"capturedAtUnixMs":1,
        "session":"material-fixture","geometryRevision":"","width":9,"height":1,"tileScale":4,
        "day":1,"season":"autumn","phase":"day","tiles":[1,2,3,4,5,6,7,8,9],
        "tileNames":{"1":"LAND","2":"TEXTURED_OCEAN","3":"COLORED_OCEAN","4":"BROKEN_FINAL",
            "5":"LAND_OTHER_STYLE","6":"OCEAN_OTHER_STYLE","7":"COLORED_OTHER_STYLE","8":"FINAL_SUPPORTED","9":"VOID"},
        "tileKinds":{"1":"land","2":"ocean","3":"ocean","4":"land","5":"land","6":"ocean","7":"ocean","8":"land","9":"void"},
        "tileTextures":{"1":"noise1.tex","2":"noise2.tex","4":"noise4.tex","5":"noise5.tex","6":"noise6.tex","8":"noise8.tex"},
        "tileBaseTextures":{},"tileColors":{"3":[1,2,3,255],"7":[4,5,6,255]},
        "terrainLayers":layers,"landmarks":[],"roads":[],"landmarksTruncated":false,"art":null,"players":[],
    })).unwrap();
    assert_eq!(
        terrain_edge_requests(&snapshot),
        vec![
            ("atlas1.xml".into(), "base1.tex".into()),
            ("atlas2.xml".into(), "base2.tex".into()),
            ("atlas8.xml".into(), "base8.tex".into())
        ]
    );
    assert_eq!(snapshot.terrain_layers.len(), 11);
    assert_eq!(
        protocol::terrain_material_issues(&snapshot),
        ["Native terrain sample styles {1, 2} are unsupported; native noise/base retained."]
    );
}

#[test]
fn map_request_rejects_unknown_fields_before_native_dispatch() {
    assert!(
        serde_json::from_value::<DstMapInput>(serde_json::json!({
            "instanceId":"instance", "shard":"master", "includeTerrain":true,
            "command":"print('untrusted')"
        }))
        .is_err()
    );
}

#[test]
fn map_selection_uses_only_configured_native_shard_roles() {
    for (settings, allowed) in [
        (serde_json::json!({}), vec!["master"]),
        (
            serde_json::json!({"enable_caves":true}),
            vec!["master", "caves"],
        ),
        (
            serde_json::json!({"shard_layout":"island_adventures"}),
            vec!["master", "caves", "islands", "volcano"],
        ),
    ] {
        assert_eq!(
            app_core::dst_shards::dst_shards(&settings)
                .unwrap()
                .into_iter()
                .map(|shard| shard.process_key)
                .collect::<Vec<_>>(),
            allowed
        );
    }
}

struct OwnedMapChild(Option<std::process::Child>);

impl Drop for OwnedMapChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            if let Err(error) = child.kill() {
                eprintln!("Map identity fixture process cleanup: {error}");
            }
            if let Err(error) = child.wait() {
                eprintln!("Map identity fixture process reap: {error}");
            }
        }
    }
}

struct MapProcessFixture {
    state: DesktopState,
    details: InstanceDetails,
}

impl MapProcessFixture {
    fn new() -> Self {
        use app_core::{ActiveInstanceRun, InstanceStatus, InstanceSummary};
        use app_runtime::{ManagedProcess, RuntimeChild, inspect_process_identity};
        use std::process::{Command, Stdio};

        let state = DesktopState::default();
        let summary = InstanceSummary {
            id: "map-process-identity".into(),
            name: "Map process identity fixture".into(),
            module_id: "dontstarve".into(),
            status: InstanceStatus::Running,
            active_process_count: 4,
            bind_ip: "127.0.0.1".into(),
            port_count: 0,
            autostart: false,
        };
        let mut children = Vec::new();
        let mut processes = Vec::new();
        for (index, key) in ["master", "caves", "islands", "volcano"]
            .into_iter()
            .enumerate()
        {
            #[cfg(windows)]
            let mut command = {
                use std::os::windows::process::CommandExt;
                let mut command = Command::new("cmd.exe");
                command.args(["/D", "/C", "set /p LGSM_MAP_GATE="]);
                command.creation_flags(0x0800_0000);
                command
            };
            #[cfg(not(windows))]
            let mut command = {
                let mut command = Command::new("sh");
                command.args(["-c", "read LGSM_MAP_GATE"]);
                command
            };
            let child = OwnedMapChild(Some(
                command
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .expect("spawn owned stdin-gated map identity fixture"),
            ));
            let pid = child.0.as_ref().unwrap().id();
            let identity = inspect_process_identity(pid).unwrap().unwrap();
            processes.push(InstanceProcessState {
                run_id: 101 + index as i64,
                session_id: Some("shared-map-session".into()),
                process_key: key.into(),
                display_name: key.into(),
                pid: Some(pid),
                process_identity: Some(identity),
                status: "running".into(),
                started_at: None,
                stopped_at: None,
                exit_code: None,
                crash_flag: false,
                log_path: Some(format!("{key}.log")),
                is_primary: index == 0,
            });
            children.push(child);
        }
        let managed = processes
            .iter()
            .zip(children)
            .map(|(process, mut child)| ManagedProcess {
                run_id: process.run_id,
                process_key: process.process_key.clone(),
                display_name: process.display_name.clone(),
                pid: process.pid.unwrap(),
                process_identity: process.process_identity.clone().unwrap(),
                root_process_identity: process.process_identity.clone().unwrap(),
                log_path: process.log_path.clone().unwrap(),
                is_primary: process.is_primary,
                uses_script_entrypoint: false,
                performance_policy: Default::default(),
                last_performance_refresh: None,
                last_performance_target_count: None,
                last_performance_application: None,
                child: Some(RuntimeChild::Standard(child.0.take().unwrap())),
                hidden_desktop: None,
            })
            .collect();
        state.runtime_supervisor.lock().unwrap().insert_running(
            summary.clone(),
            Some("shared-map-session".into()),
            managed,
        );
        Self {
            state,
            details: InstanceDetails {
                summary,
                config_file_path: String::new(),
                saves_path: String::new(),
                backup_uses_declared_saves_path: false,
                auto_backup_on_stop: false,
                backup_retention_count: 0,
                settings_json: "{\"shard_layout\":\"island_adventures\"}".into(),
                ports: Vec::new(),
                active_run: Some(ActiveInstanceRun {
                    run_id: processes[0].run_id,
                    session_id: processes[0].session_id.clone(),
                    pid: processes[0].pid,
                    log_path: processes[0].log_path.clone(),
                    process_count: processes.len(),
                    processes,
                }),
            },
        }
    }

    fn snapshot(&self, shard: &str) -> DstMapSnapshot {
        serde_json::from_value(serde_json::json!({
            "instanceId": self.details.summary.id,
            "shard": shard,
            "runId": self.details.active_run.as_ref().unwrap().run_id,
            "capturedAtUnixMs": 1, "session": "shared-map-session", "geometryRevision": "",
            "width": 1, "height": 1, "tileScale": 4.0,
            "day": 1, "season": "autumn", "phase": "day",
            "tiles": null, "tileNames": {}, "tileKinds": {}, "tileTextures": {}, "tileBaseTextures": {}, "tileColors": {}, "terrainLayers": [],
            "landmarks": null, "roads": null, "landmarksTruncated": false,
            "art": null, "players": []
        }))
        .unwrap()
    }
}

impl Drop for MapProcessFixture {
    fn drop(&mut self) {
        if let Some(mut instance) = self
            .state
            .runtime_supervisor
            .lock()
            .unwrap()
            .take_running_for_stop(&self.details.summary.id)
        {
            for process in &mut instance.processes {
                if let Some(app_runtime::RuntimeChild::Standard(child)) = process.child.take() {
                    drop(OwnedMapChild(Some(child)));
                }
            }
        }
    }
}

#[test]
fn map_verification_distinguishes_session_owner_from_all_four_shard_runs() {
    let fixture = MapProcessFixture::new();
    let run = fixture.details.active_run.as_ref().unwrap();
    for process in &run.processes {
        verify_process(&fixture.state, &fixture.details.summary.id, process).unwrap();
        verify_snapshot_process(
            &fixture.state,
            &fixture.snapshot(&process.process_key),
            &fixture.details,
        )
        .unwrap();
    }
    let caves = &run.processes[1];
    assert_ne!(caves.run_id, run.run_id);
    assert!(
        !fixture
            .state
            .runtime_supervisor
            .lock()
            .unwrap()
            .matches_running_process(
                &fixture.details.summary.id,
                run.run_id,
                &caves.process_key,
                caves.pid.unwrap(),
            )
            .unwrap()
    );
    let mut stale_owner = fixture.snapshot("caves");
    stale_owner.run_id += 1;
    assert!(
        verify_snapshot_process(&fixture.state, &stale_owner, &fixture.details)
            .unwrap_err()
            .starts_with("DST_MAP_SHARD_CHANGED: ")
    );
    let mut disabled = fixture.details.clone();
    disabled.settings_json = "{}".into();
    assert!(
        validate_target(&disabled, "caves")
            .unwrap_err()
            .starts_with("DST_MAP_SHARD_NOT_ENABLED: ")
    );
    let mut stopped = fixture.details.clone();
    stopped.active_run.as_mut().unwrap().processes[1].status = "stopped".into();
    assert!(
        verify_snapshot_process(&fixture.state, &fixture.snapshot("caves"), &stopped)
            .unwrap_err()
            .starts_with("DST_MAP_SHARD_NOT_RUNNING: ")
    );
    let mut no_pid = caves.clone();
    no_pid.pid = None;
    assert!(
        verify_process(&fixture.state, &fixture.details.summary.id, &no_pid)
            .unwrap_err()
            .starts_with("DST_MAP_SHARD_NOT_RUNNING: ")
    );
    for change_pid in [false, true] {
        let mut replaced = fixture.details.clone();
        let process = &mut replaced.active_run.as_mut().unwrap().processes[1];
        if change_pid {
            process.pid = run.processes[0].pid;
        } else {
            process.run_id = run.run_id;
        }
        assert!(
            verify_snapshot_process(&fixture.state, &fixture.snapshot("caves"), &replaced)
                .unwrap_err()
                .starts_with("DST_MAP_SHARD_CHANGED: ")
        );
    }
}
