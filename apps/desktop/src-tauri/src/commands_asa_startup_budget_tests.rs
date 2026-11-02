use super::*;
use app_core::ProcessIdentity;
use std::path::PathBuf;
#[cfg(windows)]
use std::{fs, io::Write};

fn normal() -> Duration {
    Duration::from_secs(300)
}
fn clock() -> PhaseClock {
    PhaseClock::new(normal(), Duration::ZERO, Duration::ZERO)
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
            required_ports: vec![format!("{key}:game")],
        });
    }
    StartupBudget {
        session,
        started: tokio::time::Instant::now(),
        normal_timeout: normal(),
        targets,
        console_log: None,
        reported_mod_check: false,
        reported_mod_complete: false,
        reported_mod_count: 0,
    }
}
#[test]
fn a_second_maps_mod_validation_cannot_extend_an_unready_maps_normal_deadline() {
    let mut session = StartupSession::default();
    let mut b = clock();
    b.require_mods();
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
    b.require_mods();
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
    let mut budget = model_budget(&mut session, vec![("a", clock(), 2)]);
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
fn serial_to_final_wait_retains_original_phase_and_ready_state() {
    let mut session = StartupSession::default();
    let mut c = clock();
    c.require_mods();
    c.observe_mods(true, Duration::from_secs(500), normal())
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
#[cfg(windows)]
struct Fixture(PathBuf);
#[cfg(windows)]
impl Fixture {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("lg-asa-budget-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
#[cfg(windows)]
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
fn invalid_process_creation_tokens_cannot_authorize_extra_time() {
    let identity = ProcessIdentity {
        creation_time: 0,
        image_path: String::new(),
    };
    assert!(process_creation_time(&identity).is_none());
}
