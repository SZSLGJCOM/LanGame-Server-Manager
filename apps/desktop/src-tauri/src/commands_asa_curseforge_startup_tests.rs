use super::*;
use std::collections::VecDeque;
use std::fs;

const DENIED: &str = "failed to inspect external process 3380: Access is denied. (os error 5)";
const QUERY_FAILURE: &str =
    "LogCFCore: Error: Error querying server mods: ApiError: Failed (serverUnreachable)";

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lg-asa-cf-startup-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn target(&self, milliseconds: &str, message: &str) -> Target {
        let log = self.0.join("native.log");
        fs::write(&log, line(milliseconds, message)).unwrap();
        let created_ms = app_storage::ark_native_log_message(&line("100", "cutoff"))
            .unwrap()
            .0;
        let mut target = Target::new(
            Requirements {
                project_ids: vec!["42".into()],
                native_log: log,
            },
            3380,
            ProcessIdentity {
                creation_time: created_ms * 10_000 + 116_444_736_000_000_000,
                image_path: "C:\\asa\\ArkAscendedServer.exe".into(),
            },
        )
        .unwrap();
        target.cache_root = Some(self.0.clone());
        target
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove this test's exclusive fixture");
    }
}

fn line(milliseconds: &str, message: &str) -> String {
    format!("[2026.11.08-11.58.53:{milliseconds}][  0]{message}\n")
}

fn observe(target: &mut Target, samples: Vec<Result<bool, String>>) -> Result<(), String> {
    let mut samples: VecDeque<_> = samples.into();
    let result = target.observe(|pid, _| {
        assert_eq!(pid, 3380);
        samples.pop_front().expect("unexpected identity inspection")
    });
    assert!(samples.is_empty(), "every planned identity check must run");
    result
}

#[test]
fn asa_curseforge_identity_errors_cannot_hide_current_native_failure() {
    let fixture = Fixture::new();
    for samples in [vec![Err(DENIED.into())], vec![Ok(true), Err(DENIED.into())]] {
        let mut target = fixture.target("110", QUERY_FAILURE);
        let error = observe(&mut target, samples).unwrap_err();
        let diagnostic: serde_json::Value = serde_json::from_str(&error).unwrap();
        assert_eq!(diagnostic["stage"], "query");
        assert_eq!(diagnostic["reason"], "server_unreachable");
        assert_eq!(diagnostic["native_process_exited"], false);
        assert_eq!(diagnostic["cache_verified"], false);
        assert!(!target.is_complete());
    }
}

#[test]
fn asa_curseforge_unknown_identity_never_accepts_success_or_stale_failure() {
    let fixture = Fixture::new();
    for (timestamp, message) in [
        ("099", QUERY_FAILURE),
        ("110", "LogCFCore: Mod valid: Fixture (42)"),
        ("110", "ordinary startup record"),
    ] {
        for samples in [vec![Err(DENIED.into())], vec![Ok(true), Err(DENIED.into())]] {
            let mut target = fixture.target(timestamp, message);
            assert_eq!(observe(&mut target, samples).unwrap_err(), DENIED);
        }
    }
    let mut target = fixture.target("110", "ordinary startup record");
    fs::remove_file(&target.log).unwrap();
    fs::create_dir(&target.log).unwrap();
    assert_eq!(
        observe(&mut target, vec![Err(DENIED.into())]).unwrap_err(),
        DENIED,
        "a log read error cannot replace the original identity failure"
    );
}

#[test]
fn asa_curseforge_confirmed_exit_retains_native_failure_and_exit_evidence() {
    let fixture = Fixture::new();
    for samples in [vec![Ok(false)], vec![Ok(true), Ok(false)]] {
        let mut target = fixture.target("110", QUERY_FAILURE);
        let error = observe(&mut target, samples).unwrap_err();
        let diagnostic: serde_json::Value = serde_json::from_str(&error).unwrap();
        assert_eq!(diagnostic["reason"], "server_unreachable");
        assert_eq!(diagnostic["native_process_exited"], true);
        assert!(!target.is_complete());
    }
}

#[test]
fn asa_curseforge_success_requires_matching_identity_on_both_sides_of_log_read() {
    let fixture = Fixture::new();
    let mut target = fixture.target("110", "LogCFCore: Mod valid: Fixture (42)");
    observe(&mut target, vec![Ok(true), Ok(true)]).unwrap();
    assert!(target.is_complete());
}
