use super::*;
use app_core::{InstanceProcessState, ProcessIdentity};
use std::fs::{self, FileTimes};
use std::path::PathBuf;
use std::time::{Duration, UNIX_EPOCH};

struct Fixture {
    root: PathBuf,
    run: ActiveInstanceRun,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("lg-moria-health-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(root.join("Moria/Saved/Config")).unwrap();
        let image = root.join("MoriaServer.exe");
        fs::write(&image, []).unwrap();
        let process = InstanceProcessState {
            run_id: 7,
            session_id: Some("fixture-session".into()),
            process_key: "main".into(),
            display_name: "Moria fixture".into(),
            pid: Some(42),
            process_identity: Some(ProcessIdentity {
                creation_time: WINDOWS_UNIX_EPOCH_TICKS + 200 * 10_000_000,
                image_path: image.to_string_lossy().into_owned(),
            }),
            status: "running".into(),
            started_at: None,
            stopped_at: None,
            exit_code: None,
            crash_flag: false,
            log_path: None,
            is_primary: true,
        };
        Self {
            root,
            run: ActiveInstanceRun {
                run_id: 7,
                session_id: process.session_id.clone(),
                pid: process.pid,
                log_path: None,
                process_count: 1,
                processes: vec![process],
            },
        }
    }

    fn write(&self, bytes: &[u8], millis: u64) {
        let path = self.root.join(STATUS_PATH);
        fs::write(&path, bytes).unwrap();
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_millis(millis)))
            .unwrap();
    }

    fn health(&self) -> app_core::RuntimeHealth {
        analyze(&InstanceStatus::Running, &self.root, Some(&self.run)).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn returntomoria_health_requires_current_native_running_status() {
    let mut fixture = Fixture::new();
    assert_eq!(fixture.health().status, "starting");
    fixture.write(
        b"\xEF\xBB\xBF{\"Status\":\"running\",\"InviteCode\":\"private-fixture\"}",
        199_999,
    );
    assert_eq!(
        fixture.health().status,
        "starting",
        "a copied previous-run status is not readiness"
    );
    fixture.write(
        b"\xEF\xBB\xBF{\"Status\":\"running\",\"InviteCode\":\"private-fixture\"}",
        200_001,
    );
    let ready = fixture.health();
    assert_eq!(ready.status, "ready");
    assert!(ready.matched_line.is_none());
    assert!(!ready.summary.contains("private-fixture"));
    fixture.run.processes[0]
        .process_identity
        .as_mut()
        .unwrap()
        .creation_time += 20_000;
    assert_eq!(
        fixture.health().status,
        "starting",
        "the next process cannot inherit old readiness"
    );
}

#[test]
fn returntomoria_health_rechecks_status_and_rejects_partial_or_non_running_json() {
    let fixture = Fixture::new();
    fixture.write(br#"{"Status":"running"}"#, 200_001);
    assert_eq!(fixture.health().status, "ready");
    for bytes in [
        b"{".as_slice(),
        br#"{"Status":"loading"}"#,
        br#"{"Status":"stopping"}"#,
        br#"{"Status":"stopped"}"#,
        br#"{"Status":"RUNNING"}"#,
        br#"{"Players":0}"#,
        b"[]",
    ] {
        fixture.write(bytes, 200_002);
        assert_eq!(fixture.health().status, "starting");
    }
    fixture.write(&vec![b' '; MAX_STATUS_BYTES as usize + 1], 200_003);
    assert_eq!(fixture.health().status, "warning");
}

#[test]
fn returntomoria_health_requires_one_matching_active_primary_owner() {
    let fixture = Fixture::new();
    fixture.write(br#"{"Status":"running"}"#, 200_001);
    let check = |run: &ActiveInstanceRun| {
        assert_eq!(
            analyze(&InstanceStatus::Running, &fixture.root, Some(run))
                .unwrap()
                .status,
            "starting"
        );
    };
    assert_eq!(
        analyze(&InstanceStatus::Running, &fixture.root, None)
            .unwrap()
            .status,
        "starting"
    );
    let mut run = fixture.run.clone();
    run.processes[0].process_identity = None;
    check(&run);
    let mut run = fixture.run.clone();
    run.processes[0].pid = Some(43);
    check(&run);
    let mut run = fixture.run.clone();
    run.processes[0].session_id = Some("other-session".into());
    check(&run);
    let mut run = fixture.run.clone();
    run.processes[0].run_id += 1;
    check(&run);
    let mut run = fixture.run.clone();
    run.processes[0].status = "stopped".into();
    check(&run);
    let mut run = fixture.run.clone();
    run.processes[0].exit_code = Some(0);
    check(&run);
    let mut run = fixture.run.clone();
    run.processes[0].is_primary = false;
    check(&run);
    let mut run = fixture.run.clone();
    run.processes.push(run.processes[0].clone());
    run.process_count = 2;
    check(&run);
    let mut run = fixture.run.clone();
    run.processes[0]
        .process_identity
        .as_mut()
        .unwrap()
        .image_path = std::env::current_exe()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    check(&run);
    for status in [
        InstanceStatus::Stopped,
        InstanceStatus::Stopping,
        InstanceStatus::Error,
    ] {
        assert!(analyze(&status, &fixture.root, Some(&fixture.run)).is_none());
    }
}

#[test]
fn returntomoria_health_rejects_future_status_timestamps() {
    let fixture = Fixture::new();
    let future = SystemTime::now().duration_since(UNIX_EPOCH).unwrap() + Duration::from_secs(3600);
    fixture.write(br#"{"Status":"running"}"#, future.as_millis() as u64);
    assert_eq!(fixture.health().status, "starting");
}

#[cfg(windows)]
#[test]
fn returntomoria_health_rejects_status_directory_reparse_points() {
    use std::os::windows::process::CommandExt;
    let fixture = Fixture::new();
    let outside = Fixture::new();
    outside.write(br#"{"Status":"running"}"#, 200_001);
    let config = fixture.root.join("Moria/Saved/Config");
    fs::remove_dir(&config).unwrap();
    // cmd's builtin treats forward-slash path components as switches.
    let output = std::process::Command::new("cmd.exe")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(config.to_string_lossy().replace('/', "\\"))
        .arg(
            outside
                .root
                .join("Moria/Saved/Config")
                .to_string_lossy()
                .replace('/', "\\"),
        )
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "fixture junction creation failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let observed = fixture.health();
    // Remove only the fixture's junction before either ordinary fixture cleanup.
    fs::remove_dir(&config).unwrap();
    assert_eq!(observed.status, "warning");
}
