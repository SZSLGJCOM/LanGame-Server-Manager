use super::*;
use std::io::Write;

const NONCE: &str = "0123456789abcdef0123456789abcdef";
const OTHER: &str = "abcdef0123456789abcdef0123456789";

fn probe(nonce: &str) -> String {
    format!("[00:00:35]: [LGSM-DST-PROBE:{nonce}]\t\n")
}

fn ready(nonce: &str) -> String {
    format!("[00:00:37]: [LGSM-DST-READY:{nonce}]\t\n")
}

struct Fixture {
    root: PathBuf,
    settings: String,
    run: ActiveInstanceRun,
}

impl Fixture {
    fn new(four_shards: bool) -> Self {
        let root = std::env::temp_dir().join(format!("lg-dst-health-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("managed-console")).unwrap();
        let settings = serde_json::json!({
            "enable_caves": true,
            "shard_layout": if four_shards { "island_adventures" } else { "standard" }
        });
        let processes = app_core::dst_shards::dst_shards(&settings)
            .unwrap()
            .iter()
            .enumerate()
            .map(|(index, shard)| {
                let path = root.join(format!("run-1-{}.log", shard.process_key));
                fs::write(&path, "").unwrap();
                app_core::InstanceProcessState {
                    run_id: index as i64 + 1,
                    session_id: Some("current-session".into()),
                    process_key: shard.process_key.into(),
                    display_name: shard.directory.into(),
                    pid: Some(index as u32 + 100),
                    process_identity: Some(app_core::ProcessIdentity {
                        creation_time: index as u64 + 1,
                        image_path: "fixture-dst.exe".into(),
                    }),
                    status: "running".into(),
                    started_at: None,
                    stopped_at: None,
                    exit_code: None,
                    crash_flag: false,
                    log_path: Some(path.to_string_lossy().into_owned()),
                    is_primary: index == 0,
                }
            })
            .collect::<Vec<_>>();
        let run = ActiveInstanceRun {
            run_id: processes[0].run_id,
            session_id: processes[0].session_id.clone(),
            pid: processes[0].pid,
            log_path: processes[0].log_path.clone(),
            process_count: processes.len(),
            processes,
        };
        Self {
            root,
            settings: settings.to_string(),
            run,
        }
    }

    fn write(&self, index: usize, content: &str) {
        fs::write(
            self.run.processes[index].log_path.as_ref().unwrap(),
            content,
        )
        .unwrap();
    }

    fn append(&self, index: usize, content: &str) {
        fs::OpenOptions::new()
            .append(true)
            .open(self.run.processes[index].log_path.as_ref().unwrap())
            .unwrap()
            .write_all(content.as_bytes())
            .unwrap();
    }

    fn ready_all(&self) {
        for index in 0..self.run.processes.len() {
            self.write(index, &(probe(NONCE) + &ready(NONCE)));
        }
    }

    fn health(&self) -> app_core::RuntimeHealth {
        let tail = read_log_snapshot(self.run.log_path.clone(), RUNTIME_HEALTH_SCAN_LINE_LIMIT);
        let previous = analyze_runtime_health(
            "dontstarve",
            &InstanceStatus::Running,
            &tail,
            Some(&self.run),
            &[],
        );
        analyze(
            &self.settings,
            &InstanceStatus::Running,
            Some(&self.run),
            previous,
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn dst_health_requires_correlated_native_ack_from_every_enabled_shard() {
    for four_shards in [false, true] {
        let fixture = Fixture::new(four_shards);
        fixture.write(0, "[00:00:01]: Server ready\n");
        assert_eq!(fixture.health().status, "starting");
        for index in 0..fixture.run.processes.len() {
            fixture.write(index, &(probe(NONCE) + &ready(NONCE)));
            assert_eq!(
                fixture.health().status == "ready",
                index + 1 == fixture.run.processes.len()
            );
        }
        fixture.write(1, &(probe(OTHER) + &ready(OTHER)));
        assert_eq!(
            fixture.health().status,
            "starting",
            "Different startup probes cannot be combined"
        );
    }
}

#[test]
fn dst_health_rejects_echoes_unpaired_malformed_and_partial_acknowledgements() {
    for content in [
        ready(NONCE),
        probe(NONCE) + &ready(OTHER),
        probe(NONCE)
            + &format!("[00:00:37]: RemoteCommandInput: print('[LGSM-DST-READY:{NONCE}]')\n"),
        probe(NONCE) + &format!("[player]: [LGSM-DST-READY:{NONCE}]\n"),
        probe(NONCE) + &format!("[00:00:37]: [LGSM-DST-READY:{NONCE}] trailing\n"),
        probe(NONCE) + "[00:00:37]: [LGSM-DST-READY:short]\n",
        probe(NONCE) + ready(NONCE).trim_end(),
    ] {
        let fixture = Fixture::new(false);
        fixture.ready_all();
        fixture.write(1, &content);
        assert_eq!(fixture.health().status, "starting", "{content}");
    }
}

#[test]
fn dst_health_keeps_shard_proofs_beyond_tail_and_revokes_rewritten_logs() {
    let fixture = Fixture::new(false);
    fixture.ready_all();
    assert_eq!(fixture.health().status, "ready");
    for index in 0..2 {
        fixture.append(index, &"ordinary world stats\n".repeat(250));
    }
    assert_eq!(fixture.health().status, "ready");
    fixture.write(1, &probe(NONCE));
    assert_eq!(fixture.health().status, "starting");
    fixture.append(1, &ready(NONCE));
    assert_eq!(fixture.health().status, "ready");
}

#[test]
fn dst_health_does_not_publish_ready_before_bounded_catchup_reaches_latest_output() {
    let fixture = Fixture::new(false);
    fixture.ready_all();
    fixture.append(1, &"world generation progress\n".repeat(50_000));
    fixture.append(1, &probe(OTHER));
    assert_eq!(fixture.health().status, "starting");
    assert_eq!(
        fixture.health().status,
        "starting",
        "A later unready probe revokes old READY"
    );
    fixture.append(0, &(probe(OTHER) + &ready(OTHER)));
    fixture.append(1, &ready(OTHER));
    assert_eq!(fixture.health().status, "ready");
}

#[test]
fn dst_health_preserves_correlated_failure_and_existing_fatal_diagnostics() {
    for after_ready in [false, true] {
        let fixture = Fixture::new(false);
        fixture.ready_all();
        if after_ready {
            assert_eq!(fixture.health().status, "ready");
        } else {
            fixture.write(1, &probe(NONCE));
        }
        fixture.append(
            1,
            &(format!("[00:00:38]: [LGSM-DST-FAILED:{NONCE}] Mod initialization failed\n")
                + &ready(NONCE)),
        );
        assert_eq!(fixture.health().status, "error");
        fixture.append(1, &"ordinary world stats\n".repeat(250));
        assert_eq!(
            fixture.health().status,
            "error",
            "READY and tail eviction must not clear the failure"
        );
        fixture.ready_all();
        fixture.append(1, "[00:00:39]: FATAL: runtime failure\n");
        assert_eq!(fixture.health().status, "error");
        fixture.append(1, &"ordinary world stats\n".repeat(250));
        assert_eq!(
            fixture.health().status,
            "error",
            "A later quiet tail cannot restore READY after a fatal"
        );
    }
}

#[test]
fn dst_health_clears_only_native_missing_prefab_failures_after_completed_retry() {
    let panic =
        "[00:00:10]: PANIC: missing required prefab [sculpture_bishop]! Expected 1, got 0\n";
    let retry = "[00:00:10]: An error occured during world gen we will retry! [was 1 of 5]\n";
    let complete = "[00:00:12]: Generation complete, injecting world entities.\n";
    for (events, recovered) in [
        (format!("{panic}{retry}{complete}"), true),
        (panic.to_owned(), false),
        (format!("{panic}{retry}"), false),
        (format!("{panic}{complete}"), false),
        (format!("{panic}{retry}{panic}{complete}"), false),
        (
            format!(
                "{panic}[00:00:11]: RemoteCommandInput: print('An error occured during world gen we will retry!')\n{complete}"
            ),
            false,
        ),
        (
            format!("[00:00:09]: FATAL: unrelated failure\n{panic}{retry}{complete}"),
            false,
        ),
    ] {
        let fixture = Fixture::new(false);
        fixture.ready_all();
        fixture.write(1, &(events.clone() + &probe(NONCE) + &ready(NONCE)));
        assert_eq!(fixture.health().status == "ready", recovered, "{events}");
        fixture.append(1, &"ordinary world stats\n".repeat(250));
        assert_eq!(fixture.health().status == "ready", recovered, "{events}");
    }
    // Recovery can arrive after the failed generation left the UI tail.
    let fixture = Fixture::new(false);
    fixture.ready_all();
    fixture.write(1, panic);
    assert_eq!(fixture.health().status, "error");
    fixture.append(1, &"ordinary world stats\n".repeat(250));
    assert_eq!(fixture.health().status, "error");
    fixture.append(
        1,
        &(retry.to_owned() + complete + &probe(NONCE) + &ready(NONCE)),
    );
    assert_eq!(fixture.health().status, "ready");
}

#[test]
fn dst_health_rejects_missing_stopped_duplicate_and_foreign_session_shards() {
    for invalid in 0..6 {
        let mut fixture = Fixture::new(false);
        fixture.ready_all();
        match invalid {
            0 => {
                fixture.run.processes.pop();
            }
            1 => fixture.run.processes[1].status = "stopped".into(),
            2 => fixture.run.processes[1].session_id = Some("old-session".into()),
            3 => fixture.run.processes[1].run_id = fixture.run.run_id,
            4 => fixture.run.processes[1].log_path = fixture.run.log_path.clone(),
            _ => fixture.run.processes[1].process_identity = None,
        }
        assert_eq!(fixture.health().status, "starting");
    }
}

#[test]
fn dst_health_retains_both_shard_proofs_after_managed_log_rotation() {
    let mut fixture = Fixture::new(false);
    for process in &mut fixture.run.processes {
        process.log_path = Some(
            fixture
                .root
                .join("managed-console")
                .join(format!("run-1-{}.log", process.process_key))
                .to_string_lossy()
                .into_owned(),
        );
    }
    fixture.run.log_path = fixture.run.processes[0].log_path.clone();
    let writers = fixture
        .run
        .processes
        .iter()
        .map(|process| {
            crate::managed_console_log::ManagedConsoleLog::open(process.log_path.as_ref().unwrap())
                .unwrap()
        })
        .collect::<Vec<_>>();
    for writer in &writers {
        writer
            .write_all((probe(NONCE) + &ready(NONCE)).as_bytes())
            .unwrap();
    }
    assert_eq!(fixture.health().status, "ready");
    let noise = "Periodic stats \n".repeat(65_536);
    for _ in 0..33 {
        for writer in &writers {
            writer.write_all(noise.as_bytes()).unwrap();
        }
        assert_eq!(fixture.health().status, "ready");
    }
    for process in &fixture.run.processes {
        let segments = crate::managed_console_log::open_log_segments(Path::new(
            process.log_path.as_ref().unwrap(),
        ))
        .unwrap()
        .unwrap();
        assert!(
            segments[0].start_offset > 0,
            "The original nonce proof has expired from retained logs"
        );
    }
    writers[1].write_all(probe(OTHER).as_bytes()).unwrap();
    assert_eq!(fixture.health().status, "starting");
}

#[test]
fn dst_health_new_run_cannot_inherit_old_shard_checkpoints() {
    let mut fixture = Fixture::new(false);
    fixture.ready_all();
    assert_eq!(fixture.health().status, "ready");
    for (index, process) in fixture.run.processes.iter_mut().enumerate() {
        process.run_id += 2;
        process.session_id = Some("next-session".into());
        let path = fixture.root.join(format!("run-2-{index}.log"));
        fs::write(&path, ready(NONCE)).unwrap();
        process.log_path = Some(path.to_string_lossy().into_owned());
    }
    fixture.run.run_id = fixture.run.processes[0].run_id;
    fixture.run.session_id = fixture.run.processes[0].session_id.clone();
    fixture.run.log_path = fixture.run.processes[0].log_path.clone();
    assert_eq!(fixture.health().status, "starting");
    fixture.ready_all();
    assert_eq!(fixture.health().status, "ready");
}
