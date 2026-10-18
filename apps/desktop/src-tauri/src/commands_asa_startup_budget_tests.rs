use super::*;
use app_core::ProcessIdentity;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

const HASH: &str = "fe6057de0123456789012345678901234567890123456789012345678901234567";
fn cold_log(hash: &str) -> String {
    format!(
        "10/15/26 20:54 [API][info] No verified local cache matches the current executable\n10/15/26 20:54 [API][info] Downloading cache archive {hash}.zip\n"
    )
}
fn cold() -> Observation {
    Observation {
        cold_download: true,
        initialized: false,
    }
}
fn loaded() -> Observation {
    Observation {
        cold_download: false,
        initialized: true,
    }
}
fn normal() -> Duration {
    Duration::from_secs(300)
}
fn clock() -> PhaseClock {
    PhaseClock::new(normal(), Duration::ZERO, Duration::ZERO)
}
#[test]
fn only_complete_current_hash_cold_cache_records_enable_the_phase() {
    assert_eq!(parse_observation(&cold_log(HASH), HASH), cold());
    for text in [
        cold_log("wrong-hash"),
        cold_log(HASH).trim_end_matches('\n').into(),
        format!("[API][info] Downloading cache archive {HASH}.zip\n"),
        cold_log(HASH).replace("[API][info]", "[other][info]"),
        cold_log(HASH).replace(".zip\n", ".zip extra\n"),
    ] {
        assert!(!parse_observation(&text, HASH).cold_download);
    }
}
#[test]
fn loaded_api_transitions_to_normal_budget_without_becoming_listener_ready() {
    let mut clock = clock();
    clock
        .observe(&cold(), Duration::from_secs(100), normal())
        .unwrap();
    clock
        .observe(&loaded(), Duration::from_secs(800), normal())
        .unwrap();
    assert_eq!(clock.deadline(), Duration::from_secs(1100));
}
#[test]
fn repeated_records_do_not_renew_either_deadline() {
    let mut clock = clock();
    for n in [100, 500, 799] {
        clock
            .observe(&cold(), Duration::from_secs(n), normal())
            .unwrap();
        assert_eq!(clock.deadline(), Duration::from_secs(900));
    }
    clock
        .observe(&loaded(), Duration::from_secs(800), normal())
        .unwrap();
    clock
        .observe(&loaded(), Duration::from_secs(900), normal())
        .unwrap();
    assert_eq!(clock.deadline(), Duration::from_secs(1100));
    assert!(
        clock
            .observe(&loaded(), Duration::from_secs(1100), normal())
            .is_err()
    );
}
#[test]
fn late_initialized_record_cannot_reopen_expired_cold_phase() {
    let mut first = clock();
    first
        .observe(&cold(), Duration::from_secs(1), normal())
        .unwrap();
    assert!(
        first
            .observe(&loaded(), Duration::from_secs(900), normal())
            .is_err()
    );
    assert!(!first.initialized);
    let mut late = clock();
    assert!(
        late.observe(
            &Observation {
                cold_download: true,
                initialized: true
            },
            Duration::from_secs(901),
            normal()
        )
        .is_err()
    );
    assert!(!late.initialized);
}
#[test]
fn preparation_delay_is_counted_exactly_once() {
    let mut clock = PhaseClock::new(normal(), Duration::from_secs(40), Duration::from_secs(30));
    clock
        .observe(&cold(), Duration::from_secs(31), normal())
        .unwrap();
    assert_eq!(clock.deadline(), Duration::from_secs(890));
    assert_eq!(
        clock.deadline() - Duration::from_secs(130),
        Duration::from_secs(760)
    );
}
#[test]
fn warm_or_unrecognized_startup_keeps_normal_budget() {
    let mut clock = clock();
    clock
        .observe(&Observation::default(), Duration::from_secs(100), normal())
        .unwrap();
    clock
        .observe(&loaded(), Duration::from_secs(200), normal())
        .unwrap();
    assert_eq!(clock.deadline(), normal());
    assert!(
        clock
            .observe(&cold(), Duration::from_secs(300), normal())
            .is_err()
    );
}
fn model_budget<'a>(
    session: &'a mut StartupSession,
    entries: Vec<(&str, PhaseClock, u8)>,
) -> StartupBudget<'a> {
    let mut targets = Vec::new();
    for (index, (key, clock, ready_samples)) in entries.into_iter().enumerate() {
        let id = (key.to_string(), index as u32, 1, String::from("fixture"));
        session.states.insert(
            id.clone(),
            TargetState {
                clock,
                ready_samples,
                mods: None,
            },
        );
        targets.push(Target {
            id,
            evidence: None,
            required_ports: vec![format!("{key}:game")],
        });
    }
    StartupBudget {
        session,
        started: tokio::time::Instant::now(),
        normal_timeout: normal(),
        targets,
        console_log: None,
        reported_cold: false,
        reported_binding: false,
        reported_mod_check: false,
        reported_mod_complete: false,
        reported_mod_count: 0,
    }
}
#[test]
fn a_second_cold_map_cannot_extend_an_unready_maps_normal_deadline() {
    let mut session = StartupSession::default();
    let mut b = clock();
    b.observe(&cold(), Duration::from_secs(1), normal())
        .unwrap();
    let budget = model_budget(&mut session, vec![("a", clock(), 0), ("b", b, 0)]);
    assert!(
        budget
            .remaining_at(Duration::from_secs(301))
            .unwrap_err()
            .contains("`a`")
    );
}

#[test]
fn asa_curseforge_pending_validation_cannot_be_satisfied_by_owned_listeners() {
    let mut session = StartupSession::default();
    let mut required = clock();
    required.require_mods();
    let mut budget = model_budget(&mut session, vec![("main", required, 0)]);
    for seconds in [1, 2, 3] {
        budget
            .record_evaluation_at(&StrictBindEvaluation::Ready, Duration::from_secs(seconds))
            .unwrap();
    }
    assert_eq!(
        budget.session.states[&budget.targets[0].id].ready_samples,
        0
    );
    assert_eq!(
        budget.filter_evaluation(StrictBindEvaluation::Ready),
        StrictBindEvaluation::Pending {
            missing_port_names: vec!["main:CurseForge-final-validation".into()],
        }
    );
    budget
        .session
        .states
        .get_mut(&budget.targets[0].id)
        .unwrap()
        .clock
        .observe_mods(true, Duration::from_secs(600), normal())
        .unwrap();
    assert_eq!(
        budget.filter_evaluation(StrictBindEvaluation::Ready),
        StrictBindEvaluation::Ready
    );
    for seconds in [601, 602, 603] {
        budget
            .record_evaluation_at(&StrictBindEvaluation::Ready, Duration::from_secs(seconds))
            .unwrap();
    }
    assert_eq!(
        budget.session.states[&budget.targets[0].id].ready_samples,
        3
    );
}

#[test]
fn asa_curseforge_late_listener_evaluation_preserves_validation_error_context() {
    for evaluation in [
        StrictBindEvaluation::Ready,
        StrictBindEvaluation::Pending {
            missing_port_names: vec!["main:game".into()],
        },
    ] {
        let mut session = StartupSession::default();
        let mut required = clock();
        required.require_mods();
        let mut budget = model_budget(&mut session, vec![("main", required, 0)]);
        let id = budget.targets[0].id.clone();
        budget.session.states.get_mut(&id).unwrap().mods = Some(
            curseforge_startup::Target::new(
                curseforge_plan::Requirements {
                    project_ids: vec!["42".into()],
                    native_log: PathBuf::from("inert-native-log"),
                },
                1,
                ProcessIdentity {
                    creation_time: 116_444_736_000_000_001,
                    image_path: "inert-process".into(),
                },
            )
            .unwrap(),
        );
        let error = budget
            .record_evaluation_at(&evaluation, Duration::from_secs(900))
            .unwrap_err();
        let diagnostic: serde_json::Value = serde_json::from_str(&error).unwrap();
        assert_eq!(diagnostic["code"], "asa_curseforge_startup_failed");
        assert_eq!(diagnostic["stage"], "validation");
        assert_eq!(diagnostic["reason"], "timeout");
        assert_eq!(diagnostic["map_key"], "main");
        assert_eq!(diagnostic["native_log_path"], "inert-native-log");
        assert_eq!(budget.session.states[&id].ready_samples, 0);
    }
}

#[test]
fn asa_curseforge_each_map_requires_its_own_validation_and_preserves_bind_failures() {
    let mut session = StartupSession::default();
    let mut complete = clock();
    complete.require_mods();
    complete
        .observe_mods(true, Duration::from_secs(10), normal())
        .unwrap();
    let mut pending = clock();
    pending.require_mods();
    let budget = model_budget(
        &mut session,
        vec![("main", complete, 3), ("map-second", pending, 0)],
    );
    assert_eq!(
        budget.filter_evaluation(StrictBindEvaluation::Ready),
        StrictBindEvaluation::Pending {
            missing_port_names: vec!["map-second:CurseForge-final-validation".into()],
        }
    );
    let failure = StrictBindEvaluation::Failed {
        message: "wrong bind address".into(),
    };
    assert_eq!(budget.filter_evaluation(failure.clone()), failure);
}

#[test]
fn asa_curseforge_partial_prior_map_log_during_a_long_download_uses_absolute_start_deadlines() {
    let mut session = StartupSession::default();
    let mut first = clock();
    first.require_mods();
    first
        .observe_mods(true, Duration::from_secs(10), normal())
        .unwrap();
    let mut second = clock();
    second.require_mods();
    let mut budget = model_budget(
        &mut session,
        vec![("main", first, 3), ("map-second", second, 0)],
    );
    // A previously verified reader temporarily lacks a complete log tail while
    // the second map legitimately remains in its 15-minute native Mod phase.
    let first_id = budget.targets[0].id.clone();
    budget.session.states.get_mut(&first_id).unwrap().mods = Some(
        curseforge_startup::Target::new(
            curseforge_plan::Requirements {
                project_ids: vec!["42".into()],
                native_log: PathBuf::from("inert-native-log"),
            },
            1,
            ProcessIdentity {
                creation_time: 116_444_736_000_000_001,
                image_path: "inert-process".into(),
            },
        )
        .unwrap(),
    );
    budget.started = tokio::time::Instant::now() - Duration::from_secs(600);
    assert_eq!(
        budget.remaining_at(Duration::from_secs(600)).unwrap(),
        Duration::from_secs(300)
    );
    let second_id = budget.targets[1].id.clone();
    let second_state = budget.session.states.get_mut(&second_id).unwrap();
    second_state
        .clock
        .observe_mods(true, Duration::from_secs(600), normal())
        .unwrap();
    second_state.ready_samples = 3;
    assert!(budget.check_at(Duration::from_secs(1199)).is_ok());
    assert!(
        budget
            .check_at(Duration::from_secs(1200))
            .unwrap_err()
            .contains("absolute deadline")
    );
}
#[test]
fn verified_map_is_excluded_until_its_listener_disappears() {
    let mut session = StartupSession::default();
    let mut b = clock();
    b.observe(&cold(), Duration::from_secs(1), normal())
        .unwrap();
    let mut budget = model_budget(&mut session, vec![("a", clock(), 3), ("b", b, 0)]);
    assert_eq!(
        budget.remaining_at(Duration::from_secs(301)).unwrap(),
        Duration::from_secs(599)
    );
    let missing = StrictBindEvaluation::Pending {
        missing_port_names: vec!["a:game".into(), "b:game".into()],
    };
    assert!(
        budget
            .record_evaluation_at(&missing, Duration::from_secs(301))
            .unwrap_err()
            .contains("`a`")
    );
}
#[test]
fn late_third_owned_listener_sample_is_rejected() {
    let mut session = StartupSession::default();
    let mut c = clock();
    c.initialized = true;
    let mut budget = model_budget(&mut session, vec![("a", c, 2)]);
    assert!(
        budget
            .record_evaluation_at(&StrictBindEvaluation::Ready, Duration::from_secs(300))
            .is_err()
    );
    assert_eq!(
        budget.session.states[&budget.targets[0].id].ready_samples,
        2
    );
}
#[test]
fn cold_listener_cannot_finish_after_its_phase_deadline() {
    let mut session = StartupSession::default();
    let mut c = clock();
    c.observe(&cold(), Duration::from_secs(1), normal())
        .unwrap();
    let mut budget = model_budget(&mut session, vec![("a", c, 0)]);
    assert!(
        budget
            .record_evaluation_at(&StrictBindEvaluation::Ready, Duration::from_secs(900))
            .is_err()
    );
}
#[test]
fn serial_to_final_wait_retains_original_phase_and_ready_state() {
    let mut session = StartupSession::default();
    let mut c = clock();
    c.observe(&cold(), Duration::from_secs(1), normal())
        .unwrap();
    c.observe(&loaded(), Duration::from_secs(500), normal())
        .unwrap();
    let first = model_budget(&mut session, vec![("a", c, 3)]);
    let targets = first.targets.clone();
    drop(first);
    let second = StartupBudget {
        session: &mut session,
        started: tokio::time::Instant::now(),
        normal_timeout: normal(),
        targets,
        console_log: None,
        reported_cold: false,
        reported_binding: false,
        reported_mod_check: false,
        reported_mod_complete: false,
        reported_mod_count: 0,
    };
    assert_eq!(
        second.session.states[&second.targets[0].id]
            .clock
            .deadline(),
        Duration::from_secs(800)
    );
    assert_eq!(
        second.session.states[&second.targets[0].id].ready_samples,
        3
    );
}
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("lg-asa-budget-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn log(&self, pid: u32, text: &str) -> PathBuf {
        let path = self.0.join(format!("ArkApi_{pid}_2026-10-15_20-54.log"));
        fs::write(&path, text).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
        assert!(
            self.0
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("lg-asa-budget-")
        );
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[cfg(windows)]
#[tokio::test]
async fn asa_curseforge_verified_map_is_reobserved_and_partial_appends_do_not_renew_it() {
    // Native output is a fixture. OS ownership uses this live test process;
    // neither ASA nor a download is started by this test.
    let fixture = Fixture::new();
    let path = fixture.0.join("ark-native.log");
    fs::write(
        &path,
        "[2099.10.17-10.00.00:100][ 0]LogCFCore: Mod valid: Fixture (42)\n",
    )
    .unwrap();
    let pid = std::process::id();
    let identity = app_runtime::inspect_process_identity(pid).unwrap().unwrap();
    let mods = curseforge_startup::Target::new(
        curseforge_plan::Requirements {
            project_ids: vec!["42".into()],
            native_log: path.clone(),
        },
        pid,
        identity,
    )
    .unwrap();
    let mut required = clock();
    required.require_mods();
    let mut session = StartupSession::default();
    let mut budget = model_budget(&mut session, vec![("main", required, 0)]);
    let id = budget.targets[0].id.clone();
    budget.session.states.get_mut(&id).unwrap().mods = Some(mods);
    budget.remaining().await.unwrap();
    for _ in 0..3 {
        budget
            .record_evaluation(&StrictBindEvaluation::Ready)
            .unwrap();
    }
    let deadline = budget.session.states[&id].clock.deadline();
    assert_eq!(budget.session.states[&id].ready_samples, 3);
    let append = |text: &str| {
        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    };
    append("[2099.10.17-10.00.00:101][ 0]LogOther: ongoing ");
    budget.remaining().await.unwrap();
    let pending = budget.filter_evaluation(StrictBindEvaluation::Ready);
    assert!(matches!(pending, StrictBindEvaluation::Pending { .. }));
    budget.record_evaluation(&pending).unwrap();
    assert_eq!(budget.session.states[&id].ready_samples, 3);
    assert_eq!(budget.session.states[&id].clock.deadline(), deadline);
    append("ordinary log record\n");
    budget.remaining().await.unwrap();
    assert_eq!(
        budget.filter_evaluation(StrictBindEvaluation::Ready),
        StrictBindEvaluation::Ready
    );
    append(
        "[2099.10.17-10.00.00:102][ 0]LogCFCore: Error querying server mods: boundary failure\n",
    );
    assert!(
        budget
            .remaining()
            .await
            .unwrap_err()
            .contains("metadata query failed")
    );
}

#[cfg(windows)]
#[tokio::test]
async fn asa_curseforge_current_log_cannot_replace_the_owned_process_identity() {
    let fixture = Fixture::new();
    let path = fixture.0.join("ark-native.log");
    fs::write(
        &path,
        "[2099.10.17-10.00.00:100][ 0]LogCFCore: Mod valid: Fixture (42)\n",
    )
    .unwrap();
    let pid = std::process::id();
    let mut identity = app_runtime::inspect_process_identity(pid).unwrap().unwrap();
    identity.creation_time += 1;
    let target = curseforge_startup::Target::new(
        curseforge_plan::Requirements {
            project_ids: vec!["42".into()],
            native_log: path,
        },
        pid,
        identity,
    )
    .unwrap();
    assert!(
        curseforge_startup::observe(vec![(0, target)])
            .await
            .err()
            .unwrap()
            .contains("changed identity")
    );
}

#[test]
fn log_files_are_bound_to_pid_and_os_process_creation_time() {
    let fixture = Fixture::new();
    let path = fixture.log(123, &cold_log(HASH));
    let created = fs::metadata(path).unwrap().created().unwrap();
    assert!(
        read_observation(&fixture.0, 123, created, HASH)
            .unwrap()
            .cold_download
    );
    assert!(
        !read_observation(&fixture.0, 124, created, HASH)
            .unwrap()
            .cold_download
    );
    assert!(
        !read_observation(&fixture.0, 123, created + Duration::from_secs(1), HASH)
            .unwrap()
            .cold_download
    );
}

#[test]
fn bounded_tail_does_not_reuse_an_old_cold_cache_header() {
    let fixture = Fixture::new();
    let text = cold_log(HASH) + &"irrelevant line\n".repeat(MAX_LOG_BYTES as usize / 8);
    let path = fixture.log(123, &text);
    let created = fs::metadata(path).unwrap().created().unwrap();
    assert!(
        !read_observation(&fixture.0, 123, created, HASH)
            .unwrap()
            .cold_download
    );
}

#[test]
fn incomplete_records_remain_ignored_until_the_newline_arrives() {
    let fixture = Fixture::new();
    let path = fixture.log(123, cold_log(HASH).trim_end_matches('\n'));
    let created = fs::metadata(&path).unwrap().created().unwrap();
    assert!(
        !read_observation(&fixture.0, 123, created, HASH)
            .unwrap()
            .cold_download
    );
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"\n")
        .unwrap();
    assert!(
        read_observation(&fixture.0, 123, created, HASH)
            .unwrap()
            .cold_download
    );
}

#[test]
fn executable_hash_and_framework_inspection_are_shared_only_by_this_start() {
    let fixture = Fixture::new();
    let executable = fixture.0.join("ArkAscendedServer.exe");
    fs::write(&executable, b"one exact image").unwrap();
    let mut images = ImageCache::default();
    let first = cached_image(&mut images, &executable, || Ok(true))
        .unwrap()
        .unwrap();
    let second = cached_image(&mut images, &executable, || {
        panic!("must reuse the validated image")
    })
    .unwrap()
    .unwrap();
    assert_eq!(
        first.0,
        Sha256::digest(b"one exact image")
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    assert!(Arc::ptr_eq(&first.1, &second.1));
    drop((first, second, images));
    assert!(
        cached_image(&mut ImageCache::default(), &executable, || Ok(false))
            .unwrap()
            .is_none()
    );
}

#[test]
fn invalid_process_creation_tokens_cannot_authorize_extra_time() {
    let identity = ProcessIdentity {
        creation_time: 0,
        image_path: String::new(),
    };
    assert!(process_creation_time(&identity).is_none());
}

#[cfg(windows)]
#[tokio::test]
async fn current_process_identity_is_rechecked_during_cold_wait() {
    let fixture = Fixture::new();
    let pid = std::process::id();
    let identity = app_runtime::inspect_process_identity(pid).unwrap().unwrap();
    let path = fixture.log(pid, &cold_log(HASH));
    let file = File::open(path).unwrap();
    let evidence = Evidence {
        pid,
        identity: identity.clone(),
        executable_hash: HASH.into(),
        log_directory: fixture.0.clone(),
        created_at: process_creation_time(&identity).unwrap(),
        _executable: Arc::new(file),
    };
    let mut session = StartupSession::default();
    let mut budget = model_budget(&mut session, vec![("a", clock(), 0)]);
    budget.targets[0].evidence = Some(evidence);
    assert!(budget.remaining().await.unwrap() > Duration::from_secs(890));
    budget.targets[0]
        .evidence
        .as_mut()
        .unwrap()
        .identity
        .creation_time += 1;
    assert!(
        budget
            .remaining()
            .await
            .unwrap_err()
            .contains("changed identity")
    );
    drop(budget);
}
