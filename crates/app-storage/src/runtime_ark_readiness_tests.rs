use super::*;

struct Fixture {
    root: PathBuf,
    instance: InstanceDetails,
}

impl Fixture {
    fn new(module_id: &str) -> Self {
        let root = std::env::temp_dir().join(format!("lg-ark-readiness-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("config")).unwrap();
        fs::create_dir_all(root.join("logs")).unwrap();
        let processes = ["main", "map-desert"]
            .into_iter()
            .enumerate()
            .map(|(index, key)| InstanceProcessState {
                run_id: index as i64 + 1,
                session_id: Some("same-session".into()),
                process_key: key.into(),
                display_name: key.into(),
                pid: Some(100 + index as u32),
                process_identity: Some(ProcessIdentity {
                    creation_time: 134_352_037_230_000_000,
                    image_path: "ARKServer.exe".into(),
                }),
                status: "running".into(),
                started_at: Some("2026-09-30 01:02:03".into()),
                stopped_at: None,
                exit_code: None,
                crash_flag: false,
                log_path: None,
                is_primary: index == 0,
            })
            .collect::<Vec<_>>();
        let instance: InstanceDetails = serde_json::from_value(json!({
            "summary": {"id":"owned", "name":"ARK cluster", "module_id":module_id,
                "status":"Running", "active_process_count":2, "bind_ip":"127.0.0.1", "port_count":6, "autostart":false},
            "config_file_path":root.join("config/instance.json"), "saves_path":root.join("runtime/ShooterGame/Saved"),
            "auto_backup_on_stop":false, "backup_retention_count":2,
            "settings_json":json!({"additional_maps":[{"id":"desert","name":"Desert","map_name":"ScorchedEarth_P","enabled":true}],"rcon_enabled":false}).to_string(),
            "ports":[{"name":"game","protocol":"udp","port":7777},{"name":"query","protocol":"udp","port":27015},
                {"name":"rcon","protocol":"tcp","port":27020},{"name":"map-desert-game","protocol":"udp","port":7787},
                {"name":"map-desert-query","protocol":"udp","port":27025},{"name":"map-desert-rcon","protocol":"tcp","port":27030}],
            "active_run":{"run_id":1,"session_id":"same-session","pid":100,"log_path":null,"process_count":2,"processes":processes}
        })).unwrap();
        for map in app_core::ark_maps::processes(&instance).unwrap() {
            fs::write(map.native_log_path, "[2026.09.30-01.02.04:001][ 1]Server has completed startup and is now advertising for join.\n").unwrap();
        }
        Self { root, instance }
    }
    fn extra_log(&self) -> String {
        app_core::ark_maps::processes(&self.instance)
            .unwrap()
            .into_iter()
            .find(|map| map.process_key == "map-desert")
            .unwrap()
            .native_log_path
    }
    fn health(&self, snapshot: &str) -> app_core::RuntimeHealth {
        analyze_maps_with(&self.instance, &parse_endpoints(snapshot), |_, _| true)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

const READY_ENDPOINTS: &str = "UDP 127.0.0.1:7777 *:* 100\nUDP 127.0.0.1:27015 *:* 100\nUDP 127.0.0.1:7787 *:* 101\nUDP 127.0.0.1:27025 *:* 101\n";

#[test]
fn every_enabled_ark_map_requires_its_own_registered_process_and_ports() {
    for edition in ["arksurvivalevolved", "arksurvivalascended"] {
        let mut fixture = Fixture::new(edition);
        assert_eq!(fixture.health(READY_ENDPOINTS).status, "ready", "{edition}");
        let wrong_owner = READY_ENDPOINTS.replace("*:* 101", "*:* 100");
        assert_eq!(
            fixture.health(&wrong_owner).status,
            "starting",
            "another map's PID cannot satisfy the endpoint"
        );
        fixture
            .instance
            .active_run
            .as_mut()
            .unwrap()
            .processes
            .pop();
        assert_eq!(
            fixture.health(READY_ENDPOINTS).status,
            "starting",
            "ready primary cannot substitute for a missing map"
        );
    }
}

#[test]
fn asa_primary_readiness_does_not_hide_a_loading_or_failed_secondary_map() {
    let fixture = Fixture::new("arksurvivalascended");
    fs::write(fixture.extra_log(), "Loading the map\n").unwrap();
    assert_eq!(fixture.health(READY_ENDPOINTS).status, "starting");
    fs::write(
        fixture.extra_log(),
        "[2026.09.30-01.02.05:000][ 1]FATAL: map failed\n",
    )
    .unwrap();
    let failed = fixture.health(READY_ENDPOINTS);
    assert_eq!(failed.status, "error");
    assert_eq!(failed.reason.code, "fatal_log_pattern");
    assert!(failed.summary.contains("Desert"));
}

#[test]
fn stale_asa_ready_line_cannot_be_reused_after_restart() {
    let fixture = Fixture::new("arksurvivalascended");
    fs::write(fixture.extra_log(), "[2026.09.30-01.01.59:999][ 1]Server is advertising for join\n[2026.09.30-01.02.04:001][ 1]Loading assets\n").unwrap();
    assert_eq!(fixture.health(READY_ENDPOINTS).status, "starting");
}

#[test]
fn asa_native_readiness_uses_process_creation_before_delayed_database_registration() {
    let mut fixture = Fixture::new("arksurvivalascended");
    for process in &mut fixture.instance.active_run.as_mut().unwrap().processes {
        process.started_at = Some("2026-09-30 01:10:00".into());
    }
    assert_eq!(
        fixture.health(READY_ENDPOINTS).status,
        "ready",
        "database registration follows the native endpoint gate"
    );
    let restarted = &mut fixture.instance.active_run.as_mut().unwrap().processes[1];
    restarted.run_id += 1;
    restarted.process_identity.as_mut().unwrap().creation_time += 10_000_000;
    fs::write(
        fixture.extra_log(),
        "[2026.09.30-01.02.02:999][ 1]Server is advertising for join\n",
    )
    .unwrap();
    assert_eq!(
        fixture.health(READY_ENDPOINTS).status,
        "starting",
        "a prior ready line must still precede the OS creation token"
    );
}

#[test]
fn native_utc_timestamp_obeys_gregorian_dates_and_millisecond_boundaries() {
    // Expected epochs were cross-checked against Python's datetime UTC parser.
    for (text, expected) in [
        ("1970.01.01-00.00.00:000", 0),
        ("2000.02.29-00.00.00:000", 951_782_400_000),
        ("2000.03.01-00.00.00:000", 951_868_800_000),
        ("2024.02.29-23.59.59:999", 1_709_251_199_999),
        ("2100.03.01-00.00.00:000", 4_107_542_400_000),
        ("2400.02.29-00.00.00:000", 13_574_563_200_000),
    ] {
        assert_eq!(
            native_line_timestamp(&format!("[{text}][ 1]Native log")),
            Some(expected),
            "{text}"
        );
    }
    for text in [
        "1969.12.31-23.59.59:999",
        "2000.02.30-00.00.00:000",
        "2100.02.29-00.00.00:000",
        "2026.00.01-00.00.00:000",
        "2026.13.01-00.00.00:000",
        "2026.04.31-00.00.00:000",
        "2026.09.30-24.00.00:000",
        "2026.09.30-00.60.00:000",
        "2026.09.30-00.00.60:000",
        "2026.09.30-00.00.00:00",
        "2026.09.30-00.00.00:0000",
    ] {
        assert_eq!(
            native_line_timestamp(&format!("[{text}][ 1]Native log")),
            None,
            "{text}"
        );
    }
    let fixture = Fixture::new("arksurvivalascended");
    let mut process = fixture.instance.active_run.as_ref().unwrap().processes[0].clone();
    process.process_identity.as_mut().unwrap().creation_time += 5_005_000;
    assert!(!native_line_is_current(
        &process,
        "[2026.09.30-01.02.03:499][ 1]Ready"
    ));
    assert!(native_line_is_current(
        &process,
        "[2026.09.30-01.02.03:500][ 1]Ready"
    ));
    assert!(native_line_is_current(
        &process,
        "[2026.09.30-01.02.03:501][ 1]Ready"
    ));
    process.process_identity = None;
    assert!(!native_line_is_current(
        &process,
        "[2026.09.30-01.02.04:001][ 1]Ready"
    ));
}

#[test]
fn asa_startup_marker_survives_the_recent_log_window() {
    let mut fixture = Fixture::new("arksurvivalascended");
    let mut text = String::from("[2026.09.30-01.02.04:001][ 1]Server is advertising for join\n");
    for index in 0..300 {
        text.push_str(&format!(
            "[2026.09.30-01.02.05:000][ 1]Map update {index}\n"
        ));
    }
    fs::write(fixture.extra_log(), &text).unwrap();
    assert_eq!(fixture.health(READY_ENDPOINTS).status, "ready");
    fs::write(
        fixture.extra_log(),
        text.replace("01.02.04:001", "01.01.59:999"),
    )
    .unwrap();
    assert_eq!(
        fixture.health(READY_ENDPOINTS).status,
        "ready",
        "a log rewrite cannot erase already observed readiness of the same OS process"
    );
    let restarted = &mut fixture.instance.active_run.as_mut().unwrap().processes[1];
    restarted.run_id += 1;
    restarted.process_identity.as_mut().unwrap().creation_time += 10_000_000;
    fs::write(
        fixture.extra_log(),
        text.replace("01.02.04:001", "01.01.59:999"),
    )
    .unwrap();
    assert_eq!(
        fixture.health(READY_ENDPOINTS).status,
        "starting",
        "startup scan must retain the current-run gate"
    );
}

#[test]
fn asa_map_health_eventually_reaches_a_ready_marker_beyond_prefix_and_tail() {
    let fixture = Fixture::new("arksurvivalascended");
    let loading = "[2026.09.30-01.02.03:500][ 1]Loading synthetic Mod assets and native packages\n";
    let mut text = loading.repeat(30_000);
    text.push_str("[2026.09.30-01.02.04:001][ 1]Server is advertising for join\n");
    text.push_str(&loading.repeat(300));
    fs::write(fixture.extra_log(), text).unwrap();
    assert_eq!(fixture.health(READY_ENDPOINTS).status, "starting");
    let mut ready = false;
    for _ in 0..4 {
        if fixture.health(READY_ENDPOINTS).status == "ready" {
            ready = true;
            break;
        }
    }
    assert!(
        ready,
        "bounded polls must reach the current run's native marker even beyond the former fixed prefix"
    );
}

#[test]
fn prior_native_failure_does_not_override_the_current_map_run() {
    for edition in ["arksurvivalevolved", "arksurvivalascended"] {
        let fixture = Fixture::new(edition);
        fs::write(fixture.extra_log(), "[2026.09.30-01.01.59:999][ 1]FATAL: previous map run failed\n[2026.09.30-01.02.04:001][ 1]Server is advertising for join\n").unwrap();
        assert_eq!(
            fixture.health(READY_ENDPOINTS).status,
            "ready",
            "{edition}: stale failure must not apply to the next process"
        );
        fs::write(fixture.extra_log(), "[2026.09.30-01.02.04:001][ 1]Server is advertising for join\n[2026.09.30-01.02.05:000][ 1]FATAL: current map run failed\n").unwrap();
        assert_eq!(
            fixture.health(READY_ENDPOINTS).status,
            "error",
            "{edition}: current failure must remain visible"
        );
    }
}

#[test]
fn paused_map_does_not_block_the_running_main_map() {
    let mut fixture = Fixture::new("arksurvivalascended");
    let mut settings: Value = serde_json::from_str(&fixture.instance.settings_json).unwrap();
    settings["additional_maps"][0]["enabled"] = json!(false);
    fixture.instance.settings_json = settings.to_string();
    fixture
        .instance
        .active_run
        .as_mut()
        .unwrap()
        .processes
        .pop();
    assert_eq!(fixture.health(READY_ENDPOINTS).status, "ready");
}

#[test]
fn rcon_readiness_requires_a_listening_endpoint_of_the_correct_map() {
    for edition in ["arksurvivalevolved", "arksurvivalascended"] {
        let mut fixture = Fixture::new(edition);
        let mut settings: Value = serde_json::from_str(&fixture.instance.settings_json).unwrap();
        settings["rcon_enabled"] = json!(true);
        fixture.instance.settings_json = settings.to_string();
        assert_eq!(fixture.health(READY_ENDPOINTS).status, "starting");
        let listening = format!(
            "{READY_ENDPOINTS}TCP 127.0.0.1:27020 0.0.0.0:0 LISTENING 100\nTCP [::]:27030 [::]:0 LISTENING 101\n"
        );
        assert_eq!(fixture.health(&listening).status, "ready");
        assert_eq!(
            fixture
                .health(&listening.replace("LISTENING 101", "ESTABLISHED 101"))
                .status,
            "starting"
        );
    }
}

#[test]
fn native_log_age_is_bound_to_the_registered_process_creation_token() {
    let fixture = Fixture::new("arksurvivalascended");
    let path = PathBuf::from(fixture.extra_log());
    let mut process = fixture.instance.active_run.as_ref().unwrap().processes[1].clone();
    process.process_identity = Some(ProcessIdentity {
        creation_time: 116_444_736_025_000_000,
        image_path: "ArkAscendedServer.exe".into(),
    });
    for (seconds, expected) in [(1, false), (2, true), (3, true)] {
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(
                fs::FileTimes::new()
                    .set_modified(UNIX_EPOCH + std::time::Duration::from_secs(seconds)),
            )
            .unwrap();
        assert_eq!(current_native_log(&process, &path), expected);
    }
}
