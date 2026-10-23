use super::*;
use app_core::{InstanceProcessState, ProcessIdentity};

const WORLD: &str = "244CFF6E493547F66C9223BDB46D65A2";

struct Fixture {
    root: PathBuf,
    stopped: InstanceDetails,
    record: InstanceRunRecord,
    running: InstanceDetails,
    native: Option<PathBuf>,
    console: PathBuf,
    world: PathBuf,
    saved: String,
    loaded: String,
}

impl Fixture {
    fn new(module: &str) -> Self {
        let root = crate::commands::tests::temp_test_dir("restored-world");
        let root = dunce::canonicalize(root).unwrap();
        for child in ["config", "runtime", "saves", "logs/managed-console"] {
            fs::create_dir_all(root.join(child)).unwrap();
        }
        fs::write(root.join("runtime/.langame-private-runtime"), b"managed\n").unwrap();
        fs::write(
            root.join("runtime/server.exe"),
            b"fixture image; never executed",
        )
        .unwrap();
        let config = root.join("config/instance.json");
        fs::write(&config, br#"{"generated_at_unix_ms":1}"#).unwrap();
        let mut stopped =
            crate::commands::tests::manual_runtime_tests::player_snapshot_fixture_instance(
                module,
                InstanceStatus::Stopped,
            );
        stopped.summary.id = format!("restore-{}", uuid::Uuid::new_v4());
        stopped.active_run = None;
        stopped.config_file_path = config.to_string_lossy().into_owned();
        stopped.saves_path = root.join("saves").to_string_lossy().into_owned();
        let world = root.join(match module {
            "valheim" => "saves/worlds_local/n/n.db2",
            "vrising" => "saves/v4/n/AutoSave_2.save.gz",
            _ => "saves/world.sav",
        });
        fs::create_dir_all(world.parent().unwrap()).unwrap();
        fs::write(&world, b"restored native persistence").unwrap();
        let (saved, loaded) = match module {
            "valheim" => (
                "Load world: n (n)\nWorld save (1/5) Cloud & Backup checks done [2ms] => Save number 2\nWorld save (5/5) done. Total time [33ms]\n".into(),
                "ZNet.LoadWorld: n (n), save number 2\nZoneSystem.Load => DB2: unpacking compressed data 144,682 => 206,874 bytes\nZDOMan.LoadChunks => Loading ZDOs done [4ms]\n".into(),
            ),
            "vrising" => (
                format!("PersistenceV2 - Finished Saving to '{}'. Total Persistent ChunkCount 10. Total Save Size: 80 bytes.\n", world.display()),
                format!("PersistenceV2 - Loading save at {}.\nPersistenceV2 - Deserialized Header. Persistent Component Types: 10\nPersistenceV2 - Finished Loading 100 Entities spread over 20 Archetypes.\n", world.display()),
            ),
            _ => (
                format!("LogPersistence: [DedicatedServer] SaveGameInternal() : Starting save (Guid[{WORLD}] WorldName[Ashenfall] SlotName[Ashenfall])\n"),
                format!("LogPersistence: PostLoadWorldState() : World load SUCCEEDED (slot: Ashenfall) (Guid[{WORLD}])\nLogSpudSubsystem: Load: Success\n"),
            ),
        };
        let old_console = root.join("logs/managed-console/run-31-main.log");
        fs::write(&old_console, format!("{saved}{loaded}")).unwrap();
        let identity = ProcessIdentity {
            creation_time: 1,
            image_path: root
                .join("runtime/server.exe")
                .to_string_lossy()
                .into_owned(),
        };
        // The fixture models a reused PID with a different historic identity.
        // No game process is launched and no native-pass claim follows this test.
        let process = InstanceProcessState {
            run_id: 31,
            session_id: Some("prior-session".into()),
            process_key: "main".into(),
            display_name: "Server".into(),
            pid: Some(std::process::id()),
            process_identity: Some(identity),
            status: "stopped".into(),
            started_at: Some("2026-10-23 00:00:00".into()),
            stopped_at: Some("2026-10-23 00:00:01".into()),
            exit_code: Some(0),
            crash_flag: false,
            log_path: Some(old_console.to_string_lossy().into_owned()),
            is_primary: true,
        };
        let record = InstanceRunRecord {
            run_id: 31,
            session_id: process.session_id.clone(),
            status: "stopped".into(),
            pid: process.pid,
            started_at: process.started_at.clone(),
            stopped_at: process.stopped_at.clone(),
            exit_code: Some(0),
            crash_flag: false,
            log_path: process.log_path.clone(),
            process_count: 1,
            processes: vec![process.clone()],
        };
        let native = profiles::native_logs(module, &root.join("runtime"), &stopped)
            .into_iter()
            .next();
        if let Some(path) = &native {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, format!("{saved}{loaded}")).unwrap();
        }
        let console = root.join("logs/managed-console/run-32-main.log");
        let mut process = process;
        process.run_id = 32;
        process.session_id = Some("new-session".into());
        process.process_identity.as_mut().unwrap().creation_time = 2;
        process.status = "running".into();
        process.stopped_at = None;
        process.exit_code = None;
        process.log_path = Some(console.to_string_lossy().into_owned());
        let mut running = stopped.clone();
        running.summary.status = InstanceStatus::Running;
        running.active_run = Some(ActiveInstanceRun {
            run_id: 32,
            session_id: process.session_id.clone(),
            pid: process.pid,
            log_path: process.log_path.clone(),
            process_count: 1,
            processes: vec![process],
        });
        Self {
            root,
            stopped,
            record,
            running,
            native,
            console,
            world,
            saved,
            loaded,
        }
    }

    fn capture(&self) -> Prepared {
        Prepared::capture(&self.root, &self.stopped, &self.record).unwrap()
    }

    fn append_native(&self, text: &str) {
        fs::OpenOptions::new()
            .append(true)
            .open(self.native.as_ref().unwrap())
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn restored_world_uses_stopped_history_and_rejects_old_load_or_readiness_only() {
    for module in ["valheim", "runescapedragonwilds", "vrising"] {
        let fixture = Fixture::new(module);
        assert!(fixture.stopped.active_run.is_none());
        let mut snapshot = fixture.capture();
        fs::write(&fixture.console, "Server ready\n").unwrap();
        assert!(snapshot.verify(&fixture.root, &fixture.running).is_err());
        fs::OpenOptions::new()
            .append(true)
            .open(&fixture.console)
            .unwrap()
            .write_all(fixture.loaded.as_bytes())
            .unwrap();
        snapshot.verify(&fixture.root, &fixture.running).unwrap();
        assert!(snapshot.verify(&fixture.root, &fixture.running).is_err());
    }
}

#[test]
fn restored_world_rejects_different_saved_identity_and_new_world() {
    for module in ["valheim", "runescapedragonwilds", "vrising"] {
        let fixture = Fixture::new(module);
        let wrong = fixture
            .loaded
            .replace("save number 2", "save number 1")
            .replace(WORLD, "11111111111111111111111111111111")
            .replace("AutoSave_2.save.gz", "AutoSave_1.save.gz");
        let mut snapshot = fixture.capture();
        fs::write(&fixture.console, wrong).unwrap();
        assert!(snapshot.verify(&fixture.root, &fixture.running).is_err());
    }
    let fixture = Fixture::new("valheim");
    let mut snapshot = fixture.capture();
    fs::write(
        &fixture.console,
        "Load world: n (n)\nLoading: Generating locations\nServer ready\n",
    )
    .unwrap();
    assert!(snapshot.verify(&fixture.root, &fixture.running).is_err());
}

#[test]
fn restored_world_accepts_fresh_native_rotation_but_not_incomplete_output() {
    let fixture = Fixture::new("runescapedragonwilds");
    let mut snapshot = fixture.capture();
    fs::write(&fixture.console, "ready\n").unwrap();
    fixture.append_native(fixture.loaded.trim_end_matches('\n'));
    assert!(snapshot.verify(&fixture.root, &fixture.running).is_err());
    fixture.append_native("\n");
    snapshot.verify(&fixture.root, &fixture.running).unwrap();

    let fixture = Fixture::new("valheim");
    let mut snapshot = fixture.capture();
    fs::write(&fixture.console, "ready\n").unwrap();
    fs::write(fixture.native.as_ref().unwrap(), &fixture.loaded).unwrap();
    snapshot.verify(&fixture.root, &fixture.running).unwrap();
}

#[test]
fn restored_world_rejects_reused_generation_or_any_preexisting_console_log() {
    let fixture = Fixture::new("valheim");
    fs::write(&fixture.console, &fixture.loaded).unwrap();
    let mut snapshot = fixture.capture();
    assert!(
        snapshot
            .verify(&fixture.root, &fixture.running)
            .unwrap_err()
            .contains("prior log")
    );

    for changed in ["run", "session", "process", "path"] {
        let fixture = Fixture::new("valheim");
        let mut snapshot = fixture.capture();
        fs::write(&fixture.console, &fixture.loaded).unwrap();
        let mut running = fixture.running.clone();
        let run = running.active_run.as_mut().unwrap();
        match changed {
            "run" => {
                run.run_id = 31;
                run.processes[0].run_id = 31;
            }
            "session" => {
                run.session_id = fixture.record.session_id.clone();
                run.processes[0].session_id = run.session_id.clone();
            }
            "process" => {
                run.processes[0].process_identity =
                    fixture.record.processes[0].process_identity.clone();
            }
            _ => {
                run.processes[0].log_path = Some(
                    fixture
                        .root
                        .join("config/reused.log")
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
        assert!(
            snapshot.verify(&fixture.root, &running).is_err(),
            "{changed}"
        );
    }
}

#[test]
fn restored_world_requires_unchanged_restored_persistence_and_bound_configuration() {
    for changed in ["world", "config", "settings", "fixture"] {
        let fixture = Fixture::new("vrising");
        let mut snapshot = fixture.capture();
        fs::write(&fixture.console, &fixture.loaded).unwrap();
        let mut running = fixture.running.clone();
        match changed {
            "world" => fs::write(&fixture.world, b"newly created world").unwrap(),
            "config" => fs::write(&fixture.stopped.config_file_path, b"changed").unwrap(),
            "settings" => running.settings_json.push(' '),
            _ => running.summary.id.push_str("-other"),
        }
        assert!(
            snapshot.verify(&fixture.root, &running).is_err(),
            "{changed}"
        );
    }
}

#[test]
fn restored_world_rejects_incomplete_stop_history_and_unbound_saved_path() {
    let fixture = Fixture::new("vrising");
    let mut record = fixture.record.clone();
    record.processes[0].status = "running".into();
    assert!(Prepared::capture(&fixture.root, &fixture.stopped, &record).is_err());
    record = fixture.record.clone();
    record.exit_code = Some(1);
    assert!(previous_run(&record).is_err());
    record = fixture.record.clone();
    record.processes[0].process_identity = None;
    assert!(previous_run(&record).is_err());
    let foreign = fixture.root.join("other.save.gz");
    fs::write(&foreign, b"not in restored save family").unwrap();
    fs::write(
        fixture.record.log_path.as_ref().unwrap(),
        fixture.saved.replace(
            &fixture.world.to_string_lossy().to_string(),
            &foreign.to_string_lossy(),
        ),
    )
    .unwrap();
    assert!(Prepared::capture(&fixture.root, &fixture.stopped, &fixture.record).is_err());
}

#[test]
fn restored_world_never_promotes_profiles_without_content_binding() {
    for cycle in [
        "after_restore",
        "after_archive_restore",
        "after_program_maintenance",
    ] {
        for module in ["valheim", "runescapedragonwilds", "vrising"] {
            assert!(applies(module, cycle).unwrap());
        }
        for module in [
            "corekeeper",
            "enshrouded",
            "nightingale",
            "conanexiles",
            "scum",
            "theforest",
            "unknown",
        ] {
            assert!(applies(module, cycle).is_err());
        }
        assert!(!applies("returntomoria", cycle).unwrap());
        assert!(!applies("terraria", cycle).unwrap());
    }
    assert!(!applies("valheim", "initial").unwrap());
    assert!(!applies("valheim", "restart").unwrap());
}

#[cfg(windows)]
#[test]
fn restored_world_accepts_windows_identity_spelling_in_both_generations() {
    for extended in [false, true] {
        let mut fixture = Fixture::new("valheim");
        let canonical =
            dunce::canonicalize(fixture.root.join("runtime").join("server.exe")).unwrap();
        let mut image = canonical.to_string_lossy().to_lowercase();
        if extended {
            image = format!(r"\\?\{image}");
        }
        // The previous raw strip_prefix check rejects the actual runtime
        // identity representation even though it names the same file.
        assert!(relative(&fixture.root.join("runtime"), Path::new(&image)).is_err());
        fixture.record.processes[0]
            .process_identity
            .as_mut()
            .unwrap()
            .image_path = image.clone();
        let mut snapshot = fixture.capture();
        fixture.running.active_run.as_mut().unwrap().processes[0]
            .process_identity
            .as_mut()
            .unwrap()
            .image_path = image;
        fs::write(&fixture.console, &fixture.loaded).unwrap();
        snapshot.verify(&fixture.root, &fixture.running).unwrap();
    }
}

#[test]
fn restored_world_rejects_unowned_or_missing_images_in_both_generations() {
    let fixture = Fixture::new("valheim");
    let foreign = fixture.root.join("runtime-other/server.exe");
    fs::create_dir_all(foreign.parent().unwrap()).unwrap();
    fs::write(&foreign, b"outside this program").unwrap();
    let mut snapshot = fixture.capture();
    fs::write(&fixture.console, &fixture.loaded).unwrap();
    for image in [
        foreign,
        fixture.root.join("runtime/missing.exe"),
        fixture.root.join("runtime"),
    ] {
        let image = image.to_string_lossy().into_owned();
        let mut record = fixture.record.clone();
        record.processes[0]
            .process_identity
            .as_mut()
            .unwrap()
            .image_path = image.clone();
        assert!(Prepared::capture(&fixture.root, &fixture.stopped, &record).is_err());
        let mut running = fixture.running.clone();
        running.active_run.as_mut().unwrap().processes[0]
            .process_identity
            .as_mut()
            .unwrap()
            .image_path = image;
        assert!(snapshot.verify(&fixture.root, &running).is_err());
    }
}

#[test]
fn restored_world_rejects_redirected_image_ancestors_in_both_generations() {
    let fixture = Fixture::new("valheim");
    let link = fixture.root.join("runtime").join("redirect");
    let target = fixture.root.join("runtime").join("ordinary");
    fs::create_dir(&target).unwrap();
    fs::write(
        target.join("server.exe"),
        b"even an owned target cannot use a link",
    )
    .unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let output = std::process::Command::new("cmd")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .creation_flags(0x0800_0000)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let image = link.join("server.exe").to_string_lossy().into_owned();
    let mut record = fixture.record.clone();
    record.processes[0]
        .process_identity
        .as_mut()
        .unwrap()
        .image_path = image.clone();
    assert!(Prepared::capture(&fixture.root, &fixture.stopped, &record).is_err());
    let mut snapshot = fixture.capture();
    let mut running = fixture.running.clone();
    running.active_run.as_mut().unwrap().processes[0]
        .process_identity
        .as_mut()
        .unwrap()
        .image_path = image;
    fs::write(&fixture.console, &fixture.loaded).unwrap();
    assert!(snapshot.verify(&fixture.root, &running).is_err());
    #[cfg(windows)]
    fs::remove_dir(&link).unwrap();
    #[cfg(unix)]
    fs::remove_file(&link).unwrap();
    assert!(target.join("server.exe").is_file());
}

#[test]
fn restored_world_accepts_only_generated_configuration_timestamp_changes() {
    let fixture = Fixture::new("valheim");
    let before = serde_json::json!({
        "generated_at_unix_ms": 1,
        "instance_id": fixture.stopped.summary.id,
        "module_id": "valheim",
        "autostart": false,
        "settings": {"world_name": "n"},
        "ports": [{"name": "game", "port": 2456}],
        "future_field": {"retained": true}
    });
    fs::write(
        &fixture.stopped.config_file_path,
        serde_json::to_vec(&before).unwrap(),
    )
    .unwrap();
    let mut snapshot = fixture.capture();
    let mut after = before.clone();
    after["generated_at_unix_ms"] = serde_json::json!(2);
    fs::write(
        &fixture.stopped.config_file_path,
        serde_json::to_vec_pretty(&after).unwrap(),
    )
    .unwrap();
    fs::write(&fixture.console, &fixture.loaded).unwrap();
    snapshot.verify(&fixture.root, &fixture.running).unwrap();

    for field in [
        "instance_id",
        "module_id",
        "autostart",
        "settings",
        "ports",
        "future_field",
    ] {
        let mut changed = after.clone();
        changed[field] = serde_json::Value::Null;
        fs::write(
            &fixture.stopped.config_file_path,
            serde_json::to_vec(&changed).unwrap(),
        )
        .unwrap();
        assert!(
            snapshot
                .unchanged()
                .unwrap_err()
                .contains("configuration content changed"),
            "{field}"
        );
    }
    for invalid in [
        serde_json::Value::Null,
        serde_json::json!("2"),
        serde_json::json!(-1),
        serde_json::json!(1.5),
    ] {
        let mut changed = after.clone();
        changed["generated_at_unix_ms"] = invalid;
        fs::write(
            &fixture.stopped.config_file_path,
            serde_json::to_vec(&changed).unwrap(),
        )
        .unwrap();
        assert!(
            snapshot
                .unchanged()
                .unwrap_err()
                .contains("invalid generated timestamp")
        );
    }
    after
        .as_object_mut()
        .unwrap()
        .remove("generated_at_unix_ms");
    fs::write(
        &fixture.stopped.config_file_path,
        serde_json::to_vec(&after).unwrap(),
    )
    .unwrap();
    assert!(
        snapshot
            .unchanged()
            .unwrap_err()
            .contains("no generated timestamp")
    );
}

#[test]
fn restored_world_reports_bounded_persistence_differences_without_accepting_them() {
    let fixture = Fixture::new("valheim");
    let snapshot = fixture.capture();
    fs::write(&fixture.world, b"different world bytes").unwrap();
    let error = snapshot.unchanged().unwrap_err();
    assert!(error.contains("changed_files=1"));
    assert!(error.contains("changed:worlds_local"));
    assert!(!error.contains("configuration"));
    fs::remove_file(&fixture.world).unwrap();
    for index in 0..10 {
        fs::write(
            fixture
                .world
                .parent()
                .unwrap()
                .join(format!("extra-{index:02}.db2")),
            b"unexpected generation",
        )
        .unwrap();
    }
    let error = snapshot.unchanged().unwrap_err();
    assert!(error.contains("changed_files=11"));
    assert_eq!(error.matches("added:").count(), 8);
    assert!(!error.contains("extra-08"));
}
