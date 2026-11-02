use super::*;

const READY: &str = "LogNWXGameMode: OnAllLevelsLoaded: levels finished loading, finished restoring world state and building navigation. Going ready.";
const LOADING: &str = "LogNWXGameMode: SimStateManager ready, waiting for all RRP levels to finish loading before going ready.";
const EXITING: &str = "LogCore: Engine exit requested (reason: ConsoleCtrl RequestExit)";

struct Fixture {
    root: PathBuf,
    run: ActiveInstanceRun,
    native: Option<PathBuf>,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("lgsm-nightingale-health-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let run = ActiveInstanceRun {
            run_id: 1,
            session_id: Some("fixture".into()),
            pid: None,
            log_path: Some(root.join("run-1-main.log").to_string_lossy().into_owned()),
            process_count: 1,
            processes: Vec::new(),
        };
        Self {
            root,
            run,
            native: None,
        }
    }
    fn native() -> Self {
        let mut fixture = Self::new();
        let transcript = fixture.run.log_path.clone();
        fixture.run = health_tests::dragonwilds_health_run();
        fixture.run.log_path = transcript.clone();
        fixture.run.processes[0].log_path = transcript;
        fixture.run.processes[0]
            .process_identity
            .as_mut()
            .unwrap()
            .image_path = fixture
            .root
            .join("runtime/NWX/Binaries/Win64/NWXServer-Win64-Shipping.exe")
            .to_string_lossy()
            .into_owned();
        let native = fixture.root.join("runtime/NWX/Saved/Logs/NWX.log");
        fs::create_dir_all(native.parent().unwrap()).unwrap();
        fixture.native = Some(native);
        fixture
    }
    fn source(&self) -> &Path {
        self.native
            .as_deref()
            .unwrap_or_else(|| Path::new(self.run.log_path.as_deref().unwrap()))
    }
    fn write(&self, text: &str) {
        fs::write(self.source(), text).unwrap();
    }
    fn append(&self, text: &str) {
        use std::io::Write;
        fs::OpenOptions::new()
            .append(true)
            .open(self.source())
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }
    fn health(&self) -> app_core::RuntimeHealth {
        self.health_for(Some(&self.run))
    }
    fn health_for(&self, run: Option<&ActiveInstanceRun>) -> app_core::RuntimeHealth {
        let tail = read_log_snapshot(
            Some(self.source().to_string_lossy().into_owned()),
            RUNTIME_HEALTH_SCAN_LINE_LIMIT,
        );
        analyze_runtime_health_with_startup_evidence(
            "nightingale",
            &InstanceStatus::Running,
            &tail,
            run,
            &[],
        )
    }
}

fn native_line(message: &str) -> String {
    format!("[2026.09.30-01.02.03:010][ 1]{message}\n")
}

#[tokio::test]
async fn nightingale_native_health_selects_instance_log_over_managed_stdout() {
    let fixture = Fixture::native();
    fs::write(
        fixture
            .root
            .join("runtime")
            .join(crate::private_runtime::PRIVATE_RUNTIME_MARKER),
        b"managed\n",
    )
    .unwrap();
    fixture.write(&native_line(READY));
    fs::write(
        fixture.run.log_path.as_ref().unwrap(),
        "Recent stdout diagnostics\n",
    )
    .unwrap();
    let record = StoredInstanceRecord {
        summary: app_core::InstanceSummary {
            id: "nightingale-native".into(),
            name: "Nightingale".into(),
            module_id: "nightingale".into(),
            status: InstanceStatus::Running,
            active_process_count: 1,
            bind_ip: "127.0.0.1".into(),
            port_count: 0,
            autostart: false,
        },
        config_dir: fixture.root.join("config"),
        saves_dir: fixture.root.join("data"),
        runtime_mode: "independent".into(),
        program_install_root: Some(fixture.root.join("runtime")),
        auto_backup_on_stop: false,
        backup_retention_count: 1,
    };
    let source = resolve_instance_log_source_path(&record, Some(&fixture.run), &[])
        .await
        .unwrap();
    assert_eq!(source.as_deref().map(Path::new), Some(fixture.source()));
    assert_eq!(fixture.health().status, "ready");
}

#[test]
fn nightingale_native_health_retains_final_phase_and_revocations_outside_tail() {
    let fixture = Fixture::native();
    let noise = native_line("Periodic realm diagnostics").repeat(500);
    fixture.write(&format!("{}{noise}", native_line(READY)));
    assert_eq!(fixture.health().status, "ready");
    fixture.append(&format!("{}{noise}", native_line(LOADING)));
    assert_eq!(fixture.health().status, "starting");
    fixture.append(&format!(
        "{}{}{noise}",
        native_line(READY),
        native_line(EXITING)
    ));
    assert_eq!(fixture.health().reason.code, "stopping");
    fixture.append(&format!(
        "{}{noise}",
        native_line("Fatal error: current process failed")
    ));
    assert_eq!(fixture.health().status, "error");
    fixture.append(&native_line(READY));
    assert_eq!(fixture.health().status, "error");
}

#[test]
fn nightingale_native_health_rejects_old_undated_and_unowned_evidence() {
    let fixture = Fixture::native();
    fixture.write(&format!("[2026.09.30-01.02.02:999][ 1]{READY}\n[2026.09.30-01.02.02:999][ 1]Fatal error: old process\n{READY}\n"));
    assert_eq!(fixture.health().status, "starting");
    fixture.append(&native_line(READY));
    assert_eq!(fixture.health().status, "ready");
    assert_eq!(fixture.health_for(None).status, "starting");
    for variant in 0..5 {
        let mut run = fixture.run.clone();
        match variant {
            0 => run.processes[0].process_identity = None,
            1 => run.processes[0].session_id = Some("unrelated".into()),
            2 => run.processes[0].pid = Some(101),
            3 => run.processes[0].status = "stopped".into(),
            _ => {
                run.processes[0]
                    .process_identity
                    .as_mut()
                    .unwrap()
                    .image_path = fixture
                    .root
                    .join("other-instance/NWXServer.exe")
                    .to_string_lossy()
                    .into_owned()
            }
        }
        assert_eq!(
            fixture.health_for(Some(&run)).status,
            "starting",
            "variant {variant}"
        );
    }
    let mut launcher = fixture.run.clone();
    launcher.processes[0]
        .process_identity
        .as_mut()
        .unwrap()
        .image_path = fixture
        .root
        .join("runtime/NWXServer.exe")
        .to_string_lossy()
        .into_owned();
    assert_eq!(fixture.health_for(Some(&launcher)).status, "ready");
}

#[test]
fn nightingale_native_health_resets_on_new_generation_and_native_rewrite() {
    let fixture = Fixture::native();
    let noise = native_line("Periodic realm diagnostics").repeat(500);
    fixture.write(&format!("{}{noise}", native_line(READY)));
    assert_eq!(fixture.health().status, "ready");
    let changed = READY.replace("Going ready.", "Not ready!!.");
    assert_eq!(READY.len(), changed.len());
    fixture.write(&format!("{}{noise}", native_line(&changed)));
    assert_eq!(fixture.health().status, "starting");
    fixture.append(&native_line(READY));
    assert_eq!(fixture.health().status, "ready");
    let mut next = fixture.run.clone();
    next.processes[0]
        .process_identity
        .as_mut()
        .unwrap()
        .creation_time += 20_000_000;
    assert_eq!(fixture.health_for(Some(&next)).status, "starting");
    // An older concurrent observation cannot restore the superseded checkpoint.
    assert_eq!(fixture.health().status, "starting");
    fixture.append(&format!("[2026.09.30-01.02.06:010][ 1]{READY}\n"));
    assert_eq!(fixture.health_for(Some(&next)).status, "ready");
    let replacement = fixture.root.join("replacement.log");
    fs::write(&replacement, native_line(READY)).unwrap();
    fs::remove_file(fixture.source()).unwrap();
    fs::rename(replacement, fixture.source()).unwrap();
    assert_eq!(fixture.health_for(Some(&next)).status, "starting");
}

#[test]
fn nightingale_native_health_requires_complete_caught_up_observation() {
    let fixture = Fixture::native();
    let ready = native_line(READY);
    fixture.write(ready.trim_end());
    assert_eq!(fixture.health().status, "starting");
    fixture.append("\n");
    assert_eq!(fixture.health().status, "ready");
    fixture.append(&format!(
        "{}{}",
        native_line("Periodic realm diagnostics").repeat(30_000),
        native_line(LOADING)
    ));
    assert_eq!(fixture.health().status, "starting");
    assert_eq!(fixture.health().status, "starting");
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn runtime_health_nightingale_listener_and_map_load_precede_realm_readiness() {
    // Run 69 bound the socket and loaded the map before reporting that it
    // was still waiting for the RRP levels. Neither event proves join readiness.
    for line in [
        "LogNet: IpNetDriver listening on port 7784",
        "LogGlobalStatus: UEngine::LoadMap Load map complete /Game/NWX/Maps/Temporal/Forest/TestRealm",
        "LogNWXGameMode: InitialFlagsAwaiter: no Agones, so going ready immediately.",
        "LogNWXGameMode: SimStateManager ready, waiting for all RRP levels to finish loading before going ready.",
        "LogInit: Display: Engine is initialized. Leaving FEngineLoop::Init()",
    ] {
        let snapshot = LogTailSnapshot {
            source_path: None,
            lines: vec![line.into()],
            total_lines: 1,
            truncated: false,
            read_error: None,
        };
        let health = analyze_runtime_health(
            "nightingale",
            &InstanceStatus::Running,
            &snapshot,
            None,
            &[],
        );
        assert_eq!(health.status, "starting", "{line}");
    }
}

#[test]
fn runtime_health_nightingale_final_native_world_phase_establishes_ready() {
    let fixture = Fixture::new();
    fixture.write(&format!("{LOADING}\n"));
    assert_eq!(fixture.health().status, "starting");
    let ready = format!("[2026.10.01-08.00.08:016][ 79]{READY}");
    fixture.append(&format!("{ready}\n"));
    let health = fixture.health();
    assert_eq!(health.status, "ready");
    assert_eq!(health.reason.code, "ready_signal");
    assert_eq!(health.matched_line.as_deref(), Some(ready.as_str()));
}

#[test]
fn runtime_health_nightingale_keeps_ready_outside_tail_until_new_world_or_exit() {
    let fixture = Fixture::new();
    fixture.write(&format!(
        "{READY}\n{}",
        "Periodic realm diagnostics\n".repeat(500)
    ));
    assert_eq!(fixture.health().status, "ready");
    fixture.append("LogLoad: LoadMap: /Game/NWX/Maps/OtherRealm\n");
    assert_eq!(fixture.health().status, "starting");
    fixture.append(&format!("{READY}\n{EXITING}\n"));
    assert_eq!(fixture.health().reason.code, "stopping");
}

#[test]
fn runtime_health_nightingale_rejects_spoof_partial_foreign_and_previous_run() {
    let mut fixture = Fixture::new();
    for line in [format!("chat: {READY}"), format!("[LanGame startup] {READY}"),
        format!("[invalid][79]{READY}"), "LogNWXGameMode: OnAllLevelsLoaded: levels finished loading, restoring world state and building navigation.".into()] {
        fixture.write(&format!("{line}\n"));
        assert_eq!(fixture.health().status, "starting");
    }
    fixture.write(READY);
    assert_eq!(fixture.health().status, "starting");
    fixture.append("\n");
    assert_eq!(fixture.health().status, "ready");
    let mut tail = read_log_snapshot(fixture.run.log_path.clone(), RUNTIME_HEALTH_SCAN_LINE_LIMIT);
    tail.source_path = Some(
        fixture
            .root
            .join("shared-native.log")
            .to_string_lossy()
            .into_owned(),
    );
    assert_eq!(
        analyze_runtime_health_with_startup_evidence(
            "nightingale",
            &InstanceStatus::Running,
            &tail,
            Some(&fixture.run),
            &[]
        )
        .status,
        "starting"
    );
    fixture.run.run_id += 1;
    fixture.run.log_path = Some(
        fixture
            .root
            .join("run-2-main.log")
            .to_string_lossy()
            .into_owned(),
    );
    fixture.write(&format!("{LOADING}\n"));
    assert_eq!(fixture.health().status, "starting");
}

#[test]
fn runtime_health_nightingale_unread_suffix_and_fatal_output_override_cached_ready() {
    let fixture = Fixture::new();
    fixture.write(&format!("{READY}\n"));
    assert_eq!(fixture.health().status, "ready");
    fixture.append(&format!(
        "{}{LOADING}\n",
        "Periodic realm diagnostics\n".repeat(60_000)
    ));
    assert_eq!(fixture.health().status, "starting");
    assert_eq!(fixture.health().status, "starting");
    fixture.append(&format!("{READY}\nFatal error: synthetic native failure\n"));
    assert_eq!(fixture.health().status, "error");
}
