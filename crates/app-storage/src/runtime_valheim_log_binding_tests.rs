use super::super::super::read_log_snapshot;
use super::super::observe_for_run_with;
use super::*;
use app_core::InstanceProcessState;
use std::fs;
use std::io::Write;

const OPENED: &str = "09/08/2026 16:50:28: Opened PlayFab server\n";
const ACTIVE: &str = "09/08/2026 16:50:29: Session \"Private Realm\" with join code 758304 and IP 192.0.2.8:2456 is active with 0 player(s)\n";
const REVOKED: &str = "09/08/2026 16:50:30: Deactivated PlayFab lobby private-lobby\n";
const NOISE: &str = "09/08/2026 16:50:31: Periodic world diagnostics\n";

struct Fixture {
    root: PathBuf,
    native: PathBuf,
    run: ActiveInstanceRun,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("lg-valheim-binding-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("logs")).unwrap();
        fs::create_dir(root.join("external")).unwrap();
        let console = root
            .join("logs/run-main.log")
            .to_string_lossy()
            .into_owned();
        let identity = ProcessIdentity {
            creation_time: 100,
            image_path: root
                .join("valheim_server.exe")
                .to_string_lossy()
                .into_owned(),
        };
        let process = InstanceProcessState {
            run_id: 41,
            session_id: Some("session-first".into()),
            process_key: "main".into(),
            display_name: "Valheim".into(),
            pid: Some(77),
            process_identity: Some(identity),
            status: "running".into(),
            started_at: None,
            stopped_at: None,
            exit_code: None,
            crash_flag: false,
            log_path: Some(console.clone()),
            is_primary: true,
        };
        fs::write(&console, "[LanGame] Managed stdout remains separate\n").unwrap();
        Self {
            native: root.join("external/custom native.log"),
            root,
            run: ActiveInstanceRun {
                run_id: 41,
                session_id: process.session_id.clone(),
                pid: process.pid,
                log_path: Some(console),
                process_count: 1,
                processes: vec![process],
            },
        }
    }

    fn readiness() -> StartupLogReadiness {
        let mut ready = StartupLogReadiness::new("valheim").unwrap();
        ready.observe(OPENED.trim_end());
        ready.observe(ACTIVE.trim_end());
        ready
    }

    fn publish_for(&self, run: &ActiveInstanceRun) -> io::Result<()> {
        let mut file = File::open(&self.native)?;
        let offset = file.metadata()?.len();
        let owner = RunOwner::from_run(run).unwrap();
        record_with(
            ValheimLogHandoff {
                console_path: Path::new(run.log_path.as_deref().unwrap()),
                native_path: &self.native,
                file: &mut file,
                offset,
                discard_fragment: false,
                session_id: &owner.session_id,
                process_key: &owner.process_key,
                pid: owner.pid,
                identity: &owner.identity,
            },
            &Self::readiness(),
            |pid, identity| Ok(pid == owner.pid && identity == &owner.identity),
        )
    }

    fn publish(&self) {
        self.publish_for(&self.run).unwrap();
    }

    fn append(&self, text: &str) {
        fs::OpenOptions::new()
            .append(true)
            .open(&self.native)
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }

    fn health(&self) -> app_core::RuntimeHealth {
        self.health_for(&self.run)
    }

    fn health_for(&self, run: &ActiveInstanceRun) -> app_core::RuntimeHealth {
        let owner = RunOwner::from_run(&self.run).unwrap();
        observe_for_run_with(run, |pid, identity| {
            Ok(pid == owner.pid && identity == &owner.identity)
        })
        .unwrap()
    }

    fn checkpoint(&self) -> PathBuf {
        self.root.join("logs/.valheim-session.json")
    }

    fn next_run(&self) -> ActiveInstanceRun {
        let mut run = self.run.clone();
        run.run_id += 1;
        run.session_id = Some("session-next".into());
        run.pid = Some(88);
        run.log_path = Some(
            self.root
                .join("logs/next-main.log")
                .to_string_lossy()
                .into_owned(),
        );
        let process = &mut run.processes[0];
        process.run_id = run.run_id;
        process.session_id = run.session_id.clone();
        process.pid = run.pid;
        process.log_path = run.log_path.clone();
        process.process_identity.as_mut().unwrap().creation_time = 200;
        fs::write(run.log_path.as_ref().unwrap(), "Opened Steam server\n").unwrap();
        run
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn valheim_native_handoff_reloads_state_and_preserves_managed_console() {
    let fixture = Fixture::new();
    fs::write(
        &fixture.native,
        format!("FATAL: old process failed\n{OPENED}{ACTIVE}"),
    )
    .unwrap();
    let native_before = fs::read(&fixture.native).unwrap();
    let console_before = fs::read(fixture.run.log_path.as_ref().unwrap()).unwrap();
    fixture.publish();
    assert_eq!(
        fixture.health().status,
        "ready",
        "old fatal was before the handoff cursor"
    );
    assert_eq!(
        fixture.health().status,
        "ready",
        "fresh observer restores the typed state"
    );
    assert_eq!(fs::read(&fixture.native).unwrap(), native_before);
    assert_eq!(
        fs::read(fixture.run.log_path.as_ref().unwrap()).unwrap(),
        console_before
    );
    assert_eq!(
        fs::read_dir(fixture.root.join("external")).unwrap().count(),
        1
    );
    let tail = read_log_snapshot(fixture.run.log_path.clone(), 32);
    assert!(
        tail.lines
            .iter()
            .any(|line| line.contains("Managed stdout"))
    );
    assert!(!tail.lines.iter().any(|line| line.contains("join code")));
    let saved = fs::read_to_string(fixture.checkpoint()).unwrap();
    for private in [
        "758304",
        "Private Realm",
        "192.0.2.8",
        "FATAL: old",
        "join code",
    ] {
        assert!(!saved.contains(private), "checkpoint leaked {private}");
    }
}

#[test]
fn valheim_native_latest_revocation_and_partial_lines_survive_reload() {
    let fixture = Fixture::new();
    fs::write(&fixture.native, format!("{OPENED}{ACTIVE}")).unwrap();
    fixture.publish();
    fixture.append(&format!("{}{REVOKED}", NOISE.repeat(30_000)));
    assert_eq!(
        fixture.health().status,
        "starting",
        "unread suffix withholds cached ready"
    );
    assert_eq!(fixture.health().status, "starting");
    fixture.append(ACTIVE.trim_end());
    assert_eq!(
        fixture.health().status,
        "starting",
        "partial line is not readiness"
    );
    fixture.append("\n");
    assert_eq!(fixture.health().status, "ready");
    fixture.append(REVOKED);
    assert_eq!(fixture.health().status, "starting");
    fixture.publish();
    assert_eq!(
        fixture.health().status,
        "starting",
        "duplicate handoff cannot rewind a revocation"
    );
}

#[test]
fn valheim_native_replacement_truncation_and_same_length_rewrite_never_replay() {
    for mode in ["replace", "truncate", "rewrite"] {
        let fixture = Fixture::new();
        let original = format!("{OPENED}{ACTIVE}{}", NOISE.repeat(200));
        fs::write(&fixture.native, &original).unwrap();
        fixture.publish();
        assert_eq!(fixture.health().status, "ready");
        match mode {
            "replace" => {
                let other = fixture.root.join("external/replacement.log");
                fs::write(&other, &original).unwrap();
                fs::remove_file(&fixture.native).unwrap();
                fs::rename(other, &fixture.native).unwrap();
            }
            "truncate" => fs::write(&fixture.native, format!("{OPENED}{ACTIVE}")).unwrap(),
            _ => {
                let rewrite = original.replacen("Opened", "Closed", 1);
                assert_eq!(rewrite.len(), original.len());
                fs::write(&fixture.native, rewrite).unwrap();
            }
        }
        assert_eq!(fixture.health().status, "starting", "{mode}");
        assert_eq!(
            fixture.health().status,
            "starting",
            "old ready was not replayed: {mode}"
        );
        fixture.append(&format!("{OPENED}{ACTIVE}"));
        assert_eq!(
            fixture.health().status,
            "ready",
            "new transitions are consumed: {mode}"
        );
    }
}

#[test]
fn valheim_native_missing_then_same_file_return_does_not_restore_old_ready() {
    let fixture = Fixture::new();
    fs::write(&fixture.native, format!("{OPENED}{ACTIVE}")).unwrap();
    fixture.publish();
    let parked = fixture.root.join("external/parked.log");
    fs::rename(&fixture.native, &parked).unwrap();
    assert_eq!(fixture.health().status, "starting");
    assert_eq!(
        fixture.health().status,
        "starting",
        "missing is a persisted revocation"
    );
    fs::rename(parked, &fixture.native).unwrap();
    assert_eq!(fixture.health().status, "starting");
    assert_eq!(fixture.health().status, "starting");
    fixture.append(&format!("{OPENED}{ACTIVE}"));
    assert_eq!(fixture.health().status, "ready");
}

#[test]
fn valheim_native_wrong_owner_or_session_cannot_read_or_overwrite_binding() {
    let fixture = Fixture::new();
    fs::write(&fixture.native, format!("{OPENED}{ACTIVE}")).unwrap();
    fixture.publish();
    assert_eq!(fixture.health().status, "ready");
    let before = fs::read(fixture.checkpoint()).unwrap();
    for mode in ["session", "key", "pid", "creation", "image", "console"] {
        let mut wrong = fixture.run.clone();
        match mode {
            "session" => wrong.session_id = Some("wrong".into()),
            "key" => wrong.processes[0].process_key = "wrong".into(),
            "pid" => wrong.pid = Some(999),
            "creation" => {
                wrong.processes[0]
                    .process_identity
                    .as_mut()
                    .unwrap()
                    .creation_time += 1
            }
            "image" => wrong.processes[0]
                .process_identity
                .as_mut()
                .unwrap()
                .image_path
                .push_str(".other"),
            _ => {
                wrong.log_path = Some(
                    fixture
                        .root
                        .join("logs/wrong.log")
                        .to_string_lossy()
                        .into_owned(),
                )
            }
        }
        assert_eq!(fixture.health_for(&wrong).status, "starting", "{mode}");
        assert_eq!(fs::read(fixture.checkpoint()).unwrap(), before, "{mode}");
    }
    let mut calls = 0;
    assert_eq!(
        observe_for_run_with(&fixture.run, |_, _| {
            calls += 1;
            Ok(calls == 1)
        })
        .unwrap()
        .status,
        "starting",
        "identity must survive the read"
    );
    assert_eq!(fs::read(fixture.checkpoint()).unwrap(), before);
}

#[test]
fn valheim_native_old_requests_cannot_replace_new_binding() {
    let fixture = Fixture::new();
    fs::write(&fixture.native, format!("{OPENED}{ACTIVE}")).unwrap();
    fixture.publish();
    assert_eq!(fixture.health().status, "ready");
    let next = fixture.next_run();
    fixture.append(NOISE);
    let mut checks = 0;
    let stale = observe_for_run_with(&fixture.run, |_, _| {
        checks += 1;
        if checks == 2 {
            // Publish the replacement after the old observer read its input,
            // immediately before its compare-and-swap can commit progress.
            fixture.publish_for(&next)?;
        }
        Ok(true)
    })
    .unwrap();
    assert_eq!(stale.status, "starting");
    let before = fs::read(fixture.checkpoint()).unwrap();
    assert_eq!(fixture.health().status, "starting");
    assert!(fixture.publish_for(&fixture.run).is_err());
    assert_eq!(fs::read(fixture.checkpoint()).unwrap(), before);
    let owner = RunOwner::from_run(&next).unwrap();
    assert_eq!(
        observe_for_run_with(&next, |pid, identity| {
            Ok(pid == owner.pid && identity == &owner.identity)
        })
        .unwrap()
        .status,
        "ready"
    );
}

#[test]
fn valheim_native_binding_can_only_be_cleared_by_verified_newer_managed_run() {
    let fixture = Fixture::new();
    fs::write(&fixture.native, format!("{OPENED}{ACTIVE}")).unwrap();
    fixture.publish();
    assert_eq!(fixture.health().status, "ready");
    let next = fixture.next_run();
    let owner = RunOwner::from_run(&next).unwrap();
    let mut same_id = next.clone();
    same_id.run_id = fixture.run.run_id;
    same_id.processes[0].run_id = same_id.run_id;
    assert_eq!(
        observe_for_run_with(&same_id, |pid, identity| {
            Ok(pid == owner.pid && identity == &owner.identity)
        })
        .unwrap()
        .status,
        "starting",
        "creation time alone cannot replace a run"
    );
    assert_eq!(
        observe_for_run_with(&next, |pid, identity| {
            Ok(pid == owner.pid && identity == &owner.identity)
        })
        .unwrap()
        .status,
        "ready"
    );
    let before = fs::read(fixture.checkpoint()).unwrap();
    assert_eq!(fixture.health().status, "starting");
    assert_eq!(fs::read(fixture.checkpoint()).unwrap(), before);
    let state: serde_json::Value = serde_json::from_slice(&before).unwrap();
    assert!(state.get("native_binding").is_none());
    assert!(state.get("owner").is_some());
}

#[test]
fn valheim_native_receipt_failure_is_reported_without_touching_external_logs() {
    let fixture = Fixture::new();
    fs::write(&fixture.native, format!("{OPENED}{ACTIVE}")).unwrap();
    fs::create_dir(fixture.checkpoint()).unwrap();
    assert!(fixture.publish_for(&fixture.run).is_err());
    assert_eq!(
        fs::read_dir(fixture.root.join("external")).unwrap().count(),
        1
    );
    assert_eq!(
        fs::read_to_string(&fixture.native).unwrap(),
        format!("{OPENED}{ACTIVE}")
    );
}

#[test]
fn valheim_native_handoff_rejects_replaced_handle_and_incomplete_boundary() {
    let fixture = Fixture::new();
    fs::write(&fixture.native, format!("{OPENED}{ACTIVE}")).unwrap();
    let mut original = File::open(&fixture.native).unwrap();
    let offset = original.metadata().unwrap().len();
    fs::rename(&fixture.native, fixture.root.join("external/original.log")).unwrap();
    fs::write(&fixture.native, format!("{OPENED}{ACTIVE}")).unwrap();
    let owner = RunOwner::from_run(&fixture.run).unwrap();
    assert!(
        record_with(
            ValheimLogHandoff {
                console_path: Path::new(&owner.console_path),
                native_path: &fixture.native,
                file: &mut original,
                offset,
                discard_fragment: false,
                session_id: &owner.session_id,
                process_key: &owner.process_key,
                pid: owner.pid,
                identity: &owner.identity,
            },
            &Fixture::readiness(),
            |_, _| Ok(true),
        )
        .is_err()
    );
    assert!(!fixture.checkpoint().exists());
    fixture.append("partial record");
    assert!(fixture.publish_for(&fixture.run).is_err());
    assert!(!fixture.checkpoint().exists());
}

#[cfg(windows)]
#[test]
fn valheim_native_health_public_entry_uses_real_process_identity_and_handoff() {
    let mut fixture = Fixture::new();
    let pid = std::process::id();
    let identity = app_runtime::inspect_process_identity(pid).unwrap().unwrap();
    fixture.run.pid = Some(pid);
    fixture.run.processes[0].pid = Some(pid);
    fixture.run.processes[0].process_identity = Some(identity.clone());
    fs::write(&fixture.native, format!("{OPENED}{ACTIVE}")).unwrap();
    let mut file = File::open(&fixture.native).unwrap();
    let offset = file.metadata().unwrap().len();
    record_valheim_native_log(
        ValheimLogHandoff {
            console_path: Path::new(fixture.run.log_path.as_deref().unwrap()),
            native_path: &fixture.native,
            file: &mut file,
            offset,
            discard_fragment: false,
            session_id: fixture.run.session_id.as_deref().unwrap(),
            process_key: "main",
            pid,
            identity: &identity,
        },
        &Fixture::readiness(),
    )
    .unwrap();
    let tail = read_log_snapshot(fixture.run.log_path.clone(), 32);
    let health = super::super::super::analyze_runtime_health_with_startup_evidence(
        "valheim",
        &app_core::InstanceStatus::Running,
        &tail,
        Some(&fixture.run),
        &[],
    );
    assert_eq!(health.status, "ready");
    fixture.append(REVOKED);
    let health = super::super::super::analyze_runtime_health_with_startup_evidence(
        "valheim",
        &app_core::InstanceStatus::Running,
        &tail,
        Some(&fixture.run),
        &[],
    );
    assert_eq!(
        health.status, "starting",
        "native revocation wins even with unchanged console tail"
    );
    fixture.append(&format!("FATAL: current process failed\n{OPENED}{ACTIVE}"));
    let health = super::super::super::analyze_runtime_health_with_startup_evidence(
        "valheim",
        &app_core::InstanceStatus::Running,
        &tail,
        Some(&fixture.run),
        &[],
    );
    assert_eq!(health.status, "error", "fatal wins over subsequent ready");
}
