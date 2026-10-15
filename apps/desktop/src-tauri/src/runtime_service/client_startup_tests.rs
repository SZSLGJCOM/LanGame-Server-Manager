//! Reconnect against disposable real processes, never the user's service or storage.
use super::*;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::future::poll_fn;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::mpsc;
use std::task::Poll;
use std::time::Instant;
use tokio::net::windows::named_pipe::NamedPipeServer;

const FIXTURE: &str = "runtime_service::client::startup_tests::startup_process_fixture";
const MARKER: &str = "langame-startup-fixture";
const ROLES: [&str; 9] = [
    "exiting",
    "exit-window",
    "recovering",
    "healthy",
    "handoff",
    "silent",
    "malformed",
    "crash",
    "new",
];

fn fixture_arguments(log: &Path) -> Vec<OsString> {
    [
        "--exact",
        FIXTURE,
        "--ignored",
        "--nocapture",
        "--test-threads=1",
        "--logfile",
    ]
    .map(OsString::from)
    .into_iter()
    .chain([log.as_os_str().to_owned()])
    .collect()
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(6);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "isolated startup fixture timed out"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct Fixture {
    root: PathBuf,
    nonce: String,
    old: Option<Child>,
}

impl Fixture {
    fn empty() -> Self {
        let nonce = uuid::Uuid::new_v4().to_string();
        let root = std::env::temp_dir().join(format!("lgsm-startup-{nonce}"));
        fs::create_dir(&root).unwrap();
        fs::write(root.join(MARKER), &nonce).unwrap();
        Self {
            root,
            nonce,
            old: None,
        }
    }

    fn new(role: &str) -> Self {
        let mut fixture = Self::empty();
        fixture.old = Some(
            Command::new(std::env::current_exe().unwrap())
                .args(fixture_arguments(&fixture.root.join(format!("{role}.log"))))
                .spawn()
                .unwrap(),
        );
        wait_until(|| fixture.root.join(format!("{role}.ready")).exists());
        fixture
    }

    fn endpoint(&self) -> security::Endpoint {
        security::Endpoint::isolated(&self.nonce).unwrap()
    }

    fn new_arguments(&self) -> Vec<OsString> {
        fixture_arguments(&self.root.join("new.log"))
    }

    fn old_pid(&self) -> u32 {
        self.old.as_ref().unwrap().id()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Cooperative release first. Fallback termination uses identities written
        // only inside this newly created, marked root by our own child fixtures.
        let _ = fs::write(self.root.join("stop"), b"stop");
        let _ = fs::write(self.root.join("release"), b"release");
        let mut targets = Vec::new();
        for role in ROLES {
            if let Ok(bytes) = fs::read(self.root.join(format!("{role}.identity.json"))) {
                let (pid, identity): (u32, app_core::ProcessIdentity) =
                    serde_json::from_slice(&bytes).unwrap();
                if let Some(target) =
                    app_runtime::ProcessExitTarget::capture(pid, &identity).unwrap()
                {
                    targets.push(target);
                }
            }
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        for target in targets {
            while target.is_running().unwrap() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            if target.is_running().unwrap() {
                target.terminate().unwrap();
                wait_until(|| !target.is_running().unwrap());
            }
        }
        if let Some(child) = &mut self.old {
            let deadline = Instant::now() + Duration::from_secs(2);
            while child.try_wait().unwrap().is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            if child.try_wait().unwrap().is_none() {
                child.kill().unwrap();
            }
            child.wait().unwrap();
        }
        let canonical = fs::canonicalize(&self.root).unwrap();
        let managed_temp = fs::canonicalize(std::env::temp_dir()).unwrap();
        assert_eq!(canonical.parent(), Some(managed_temp.as_path()));
        assert_eq!(
            fs::read_to_string(canonical.join(MARKER)).unwrap(),
            self.nonce
        );
        fs::remove_dir_all(canonical).unwrap();
    }
}

fn reconnect_after_release(fixture: &Fixture, role: &str) -> (Client, Result<Value, String>, bool) {
    let endpoint = fixture.endpoint();
    let arguments = fixture.new_arguments();
    let (sender, receiver) = mpsc::channel();
    std::thread::scope(|scope| {
        let worker = scope.spawn(move || {
            let result = connect_or_start_with_arguments(endpoint, &arguments).map(|client| {
                let storage = tauri::async_runtime::block_on(
                    client.request("ensure_storage_ready", Value::Null),
                );
                (client, storage)
            });
            assert!(sender.send(result).is_ok());
        });
        let mut received = None;
        wait_until(|| match receiver.try_recv() {
            Ok(result) => {
                received = Some(result);
                true
            }
            Err(mpsc::TryRecvError::Empty) => fixture.root.join(format!("{role}.probed")).exists(),
            Err(mpsc::TryRecvError::Disconnected) => panic!("startup worker disconnected"),
        });
        let returned_before_release = received.is_some();
        fs::write(fixture.root.join("release"), b"release").unwrap();
        let (client, storage) = received
            .unwrap_or_else(|| receiver.recv_timeout(Duration::from_secs(6)).unwrap())
            .unwrap();
        worker.join().unwrap();
        (client, storage, returned_before_release)
    })
}

#[test]
fn immediate_reopen_waits_for_final_exit_before_starting_a_fresh_service() {
    // Both the stopping phase and the window after stopping but before process
    // exit must remain unavailable to a new desktop's storage initialization.
    for role in ["exiting", "exit-window"] {
        let fixture = Fixture::new(role);
        let (client, storage, returned_early) = reconnect_after_release(&fixture, role);
        assert!(
            storage.is_ok(),
            "reopened desktop must initialize storage: {storage:?}"
        );
        assert!(
            !returned_early,
            "final-exit service was admitted before release"
        );
        assert_ne!(
            client.service_identity.as_ref().unwrap().pid,
            fixture.old_pid()
        );
        assert!(!fixture.root.join(format!("{role}.storage")).exists());
        let previous_alive: bool = serde_json::from_slice(
            &fs::read(fixture.root.join("new.previous-alive.json")).unwrap(),
        )
        .unwrap();
        assert!(
            !previous_alive,
            "replacement launched before the original process exited"
        );
    }
}

#[test]
fn reconnect_reuses_the_same_process_after_an_ordinary_shutdown_recovers() {
    let fixture = Fixture::new("recovering");
    let (client, storage, returned_early) = reconnect_after_release(&fixture, "recovering");
    assert!(
        storage.is_ok(),
        "recovered service must initialize storage: {storage:?}"
    );
    assert!(
        !returned_early,
        "shutdown service was admitted before recovery"
    );
    assert_eq!(
        client.service_identity.as_ref().unwrap().pid,
        fixture.old_pid()
    );
    assert!(!fixture.root.join("new.ready").exists());
}

#[test]
fn reconnect_reuses_a_healthy_service_without_spawning_a_replacement() {
    let fixture = Fixture::new("healthy");
    let client =
        connect_or_start_with_arguments(fixture.endpoint(), &fixture.new_arguments()).unwrap();
    let storage =
        tauri::async_runtime::block_on(client.request("ensure_storage_ready", Value::Null));
    assert!(
        storage.is_ok(),
        "healthy service must initialize storage: {storage:?}"
    );
    assert_eq!(
        client.service_identity.as_ref().unwrap().pid,
        fixture.old_pid()
    );
    assert!(!fixture.root.join("new.ready").exists());
}

fn short_budgets(shutdown: Duration) -> startup::StartupBudgets {
    startup::StartupBudgets {
        shutdown,
        launch: Duration::from_secs(2),
        probe: Duration::from_millis(150),
    }
}

#[test]
fn native_exit_handoff_blocks_healthy_status_until_the_original_process_exits() {
    let fixture = Fixture::new("handoff");
    let old =
        tauri::async_runtime::block_on(async { Client::connect(fixture.endpoint()).unwrap() });
    let identity = old.service_identity.as_ref().unwrap();
    let _intent = crate::runtime_service::exit_intent::mark(identity).unwrap();
    let cancelled = AtomicBool::new(false);
    let arguments = fixture.new_arguments();
    // A zero wait budget makes marker recognition observable without a sleep or
    // fixture status callback: both service flags are still false in this gap.
    let error = startup::connect(
        fixture.endpoint(),
        &arguments,
        &cancelled,
        short_budgets(Duration::ZERO),
    )
    .err()
    .expect("native exit intent must prevent healthy service admission");
    assert!(error.contains("has not finished shutting down"), "{error}");
    assert!(old.service_target().unwrap().is_running().unwrap());
    assert!(!fixture.root.join("handoff.probed").exists());
    assert!(!fixture.root.join("handoff.storage").exists());
    assert!(!fixture.root.join("new.ready").exists());

    let client = tauri::async_runtime::block_on(async {
        let mut connecting = std::pin::pin!(startup::connect_ready(
            fixture.endpoint(),
            &arguments,
            &cancelled,
            short_budgets(Duration::from_secs(2)),
        ));
        assert!(
            poll_fn(|cx| Poll::Ready(connecting.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        fs::write(fixture.root.join("release"), b"release").unwrap();
        connecting.await.unwrap()
    });
    assert_ne!(
        client.service_identity.as_ref().unwrap().pid,
        fixture.old_pid()
    );
    let storage =
        tauri::async_runtime::block_on(client.request("ensure_storage_ready", Value::Null));
    assert!(
        storage.is_ok(),
        "replacement must initialize storage: {storage:?}"
    );
    assert!(!old.service_target().unwrap().is_running().unwrap());
    assert!(!fixture.root.join("handoff.storage").exists());
    let previous_alive: bool =
        serde_json::from_slice(&fs::read(fixture.root.join("new.previous-alive.json")).unwrap())
            .unwrap();
    assert!(
        !previous_alive,
        "replacement launched before the handoff process exited"
    );
}

#[test]
fn cancelling_reopen_leaves_the_original_exiting_process_alive() {
    let fixture = Fixture::new("exiting");
    let cancelled = AtomicBool::new(false);
    let endpoint = fixture.endpoint();
    let arguments = fixture.new_arguments();
    std::thread::scope(|scope| {
        let cancelled = &cancelled;
        let worker = scope.spawn(move || {
            startup::connect(
                endpoint,
                &arguments,
                cancelled,
                short_budgets(Duration::from_secs(2)),
            )
        });
        wait_until(|| fixture.root.join("exiting.probed").exists());
        cancelled.store(true, Ordering::SeqCst);
        let error = worker
            .join()
            .unwrap()
            .err()
            .expect("waiting reopen must cancel");
        assert!(error.contains("cancelled"), "{error}");
    });
    let identity = app_runtime::inspect_process_identity(fixture.old_pid())
        .unwrap()
        .unwrap();
    assert!(app_runtime::process_matches_identity(fixture.old_pid(), &identity).unwrap());
    assert!(!fixture.root.join("new.ready").exists());
    assert!(!fixture.root.join("exiting.storage").exists());
}

#[test]
fn startup_rejects_unresponsive_or_incomplete_status_without_replacing_its_owner() {
    for (role, expected) in [
        ("silent", "status probe"),
        ("malformed", "Invalid runtime service startup status"),
    ] {
        let fixture = Fixture::new(role);
        let error = startup::connect(
            fixture.endpoint(),
            &fixture.new_arguments(),
            &AtomicBool::new(false),
            short_budgets(Duration::from_secs(1)),
        )
        .err()
        .expect("unusable status must fail startup admission");
        assert!(error.contains(expected), "{role}: {error}");
        assert!(
            app_runtime::inspect_process_identity(fixture.old_pid())
                .unwrap()
                .is_some()
        );
        assert!(!fixture.root.join("new.ready").exists());
        assert!(!fixture.root.join(format!("{role}.storage")).exists());
    }
}

#[test]
fn failed_new_service_retains_shutdown_ownership_when_the_desktop_is_cancelled() {
    let fixture = Fixture::empty();
    let cancelled = AtomicBool::new(false);
    let arguments = fixture_arguments(&fixture.root.join("silent.log"));
    let endpoint = fixture.endpoint();
    let failure = std::thread::scope(|scope| {
        let cancelled = &cancelled;
        let worker = scope.spawn(move || {
            startup::connect_owned(
                endpoint,
                &arguments,
                cancelled,
                startup::StartupBudgets {
                    shutdown: Duration::from_secs(1),
                    launch: Duration::from_secs(2),
                    probe: Duration::from_millis(400),
                },
            )
        });
        wait_until(|| fixture.root.join("silent.probed").exists());
        cancelled.store(true, Ordering::SeqCst);
        worker
            .join()
            .unwrap()
            .err()
            .expect("unresponsive new service must fail admission")
    });
    assert!(
        failure.message.contains("status probe"),
        "{}",
        failure.message
    );
    let cleanup = failure
        .cleanup_client
        .expect("new process must remain owned for final exit");
    let (pid, identity): (u32, app_core::ProcessIdentity) =
        serde_json::from_slice(&fs::read(fixture.root.join("silent.identity.json")).unwrap())
            .unwrap();
    let retained = cleanup.service_identity.as_ref().unwrap();
    assert_eq!(retained.pid, pid);
    assert_eq!(retained.process, identity);
    assert!(cleanup.service_target().unwrap().is_running().unwrap());
    assert!(!fixture.root.join("silent.storage").exists());
}

#[test]
fn a_new_service_exiting_during_its_first_status_probe_is_not_spawned_again() {
    let fixture = Fixture::empty();
    let cancelled = AtomicBool::new(false);
    let arguments = fixture_arguments(&fixture.root.join("crash.log"));
    let endpoint = fixture.endpoint();
    let (sender, receiver) = mpsc::channel();
    let (failure, within_budget) = std::thread::scope(|scope| {
        let cancelled = &cancelled;
        let worker = scope.spawn(move || {
            let result = startup::connect_owned(
                endpoint,
                &arguments,
                cancelled,
                short_budgets(Duration::from_millis(300)),
            );
            assert!(sender.send(result).is_ok());
        });
        // The outer watchdog also bounds a regression that repeatedly clears
        // the child and restarts its launch allowance after every fixture crash.
        let initial = receiver.recv_timeout(Duration::from_secs(3));
        let within_budget = initial.is_ok();
        cancelled.store(true, Ordering::SeqCst);
        let result =
            initial.unwrap_or_else(|_| receiver.recv_timeout(Duration::from_secs(3)).unwrap());
        worker.join().unwrap();
        (
            result.err().expect("crashed new service must fail startup"),
            within_budget,
        )
    });
    assert!(
        within_budget,
        "repeated child launches exceeded the startup allowance"
    );
    assert!(failure.message.contains("exited"), "{}", failure.message);
    assert_eq!(
        fs::read_to_string(fixture.root.join("crash.starts"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    assert!(!fixture.root.join("crash.storage").exists());
    assert!(
        failure.cleanup_client.is_none(),
        "exited child has no live shutdown owner"
    );
}

#[test]
fn a_new_service_shutdown_cannot_extend_its_launch_deadline() {
    let fixture = Fixture::empty();
    let endpoint = fixture.endpoint();
    let arguments = fixture_arguments(&fixture.root.join("recovering.log"));
    let cancelled = AtomicBool::new(false);
    let launch = Duration::from_secs(1);
    let (sender, receiver) = mpsc::channel();
    let (failure, within_launch_budget) = std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            let result = startup::connect_owned(
                endpoint,
                &arguments,
                &cancelled,
                startup::StartupBudgets {
                    shutdown: Duration::from_secs(10),
                    launch,
                    probe: Duration::from_millis(400),
                },
            );
            assert!(sender.send(result).is_ok());
        });
        // This role reports shutdown on every status and never recovers until
        // release. A scheduling allowance stays far below its shutdown budget.
        let initial = receiver.recv_timeout(launch + Duration::from_secs(1));
        let within_launch_budget = initial.is_ok();
        if !within_launch_budget {
            // Bound the red regression and return the owned child to Fixture's
            // cleanup rather than waiting through the longer shutdown budget.
            fs::write(fixture.root.join("stop"), b"stop").unwrap();
        }
        let result =
            initial.unwrap_or_else(|_| receiver.recv_timeout(Duration::from_secs(3)).unwrap());
        worker.join().unwrap();
        (
            result
                .err()
                .expect("new shutdown service must fail admission"),
            within_launch_budget,
        )
    });
    assert!(
        fixture.root.join("recovering.probed").exists(),
        "fixture must expose shutdown before the deadline"
    );
    assert!(
        within_launch_budget,
        "shutdown status extended the child's launch deadline"
    );
    assert!(
        failure.message.contains("shutting down"),
        "{}",
        failure.message
    );
    let cleanup = failure
        .cleanup_client
        .expect("timed-out launch must retain its live child");
    let (pid, identity): (u32, app_core::ProcessIdentity) =
        serde_json::from_slice(&fs::read(fixture.root.join("recovering.identity.json")).unwrap())
            .unwrap();
    let retained = cleanup.service_identity.as_ref().unwrap();
    assert_eq!(retained.pid, pid);
    assert_eq!(retained.process, identity);
    assert!(cleanup.service_target().unwrap().is_running().unwrap());
    assert!(!fixture.root.join("recovering.storage").exists());
}

#[test]
fn a_busy_control_pipe_with_no_wait_budget_never_starts_another_process() {
    let fixture = Fixture::empty();
    let control = fixture.endpoint().control();
    let (occupied, _connection) = tauri::async_runtime::block_on(async {
        let occupied = security::create_pipe(&control, true).unwrap();
        let connection = security::connect_verified(&control).unwrap();
        occupied.connect().await.unwrap();
        (occupied, connection)
    });
    let failure = startup::connect_owned(
        fixture.endpoint(),
        &fixture.new_arguments(),
        &AtomicBool::new(false),
        startup::StartupBudgets {
            shutdown: Duration::ZERO,
            launch: Duration::ZERO,
            probe: Duration::ZERO,
        },
    )
    .err()
    .expect("busy owner must not be replaced");
    assert!(
        failure.message.contains("remained busy"),
        "{}",
        failure.message
    );
    assert!(failure.cleanup_client.is_none());
    assert!(!fixture.root.join("new.identity.json").exists());
    drop(occupied);
}

#[test]
#[ignore = "child entry: isolated pipes and synthetic storage responses only"]
fn startup_process_fixture() {
    let arguments: Vec<_> = std::env::args_os().collect();
    let Some(index) = arguments
        .iter()
        .position(|argument| argument == OsStr::new("--logfile"))
    else {
        return;
    };
    let log = PathBuf::from(arguments.get(index + 1).expect("fixture logfile"));
    let root = log.parent().unwrap();
    assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
    let nonce = fs::read_to_string(root.join(MARKER)).expect("explicit startup fixture marker");
    assert_eq!(
        root.file_name().unwrap().to_string_lossy(),
        format!("lgsm-startup-{nonce}")
    );
    let role = log.file_stem().and_then(OsStr::to_str).unwrap();
    assert!(ROLES.contains(&role));
    if role == "crash" {
        use std::io::Write;
        let mut starts = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join("crash.starts"))
            .unwrap();
        writeln!(starts, "{}", std::process::id()).unwrap();
    }
    let identity = app_runtime::inspect_process_identity(std::process::id())
        .unwrap()
        .unwrap();
    fs::write(
        root.join(format!("{role}.identity.json")),
        serde_json::to_vec(&(std::process::id(), identity)).unwrap(),
    )
    .unwrap();
    if role == "new" {
        let previous_alive = ["exiting", "exit-window", "handoff"]
            .iter()
            .any(|old_role| {
                let Ok(bytes) = fs::read(root.join(format!("{old_role}.identity.json"))) else {
                    return false;
                };
                let (pid, identity): (u32, app_core::ProcessIdentity) =
                    serde_json::from_slice(&bytes).unwrap();
                app_runtime::process_matches_identity(pid, &identity).unwrap()
            });
        fs::write(
            root.join("new.previous-alive.json"),
            serde_json::to_vec(&previous_alive).unwrap(),
        )
        .unwrap();
    }
    let endpoint = security::Endpoint::isolated(&nonce).unwrap();
    tauri::async_runtime::block_on(async {
        let control = security::create_pipe(&endpoint.control(), true).unwrap();
        let work = security::create_pipe(&endpoint, true).unwrap();
        fs::write(root.join(format!("{role}.ready")), b"ready").unwrap();
        tokio::select! {
            _ = serve_pipe(endpoint.control(), control, root.to_owned(), role.to_owned()) => (),
            _ = serve_pipe(endpoint, work, root.to_owned(), role.to_owned()) => (),
            _ = async {
                let deadline = Instant::now() + Duration::from_secs(12);
                loop {
                    if root.join("stop").exists()
                        || (matches!(role, "exiting" | "exit-window" | "handoff") && root.join("release").exists())
                        || Instant::now() >= deadline {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            } => (),
        }
    });
}

async fn serve_pipe(
    endpoint: security::Endpoint,
    mut listener: NamedPipeServer,
    root: PathBuf,
    role: String,
) {
    loop {
        listener.connect().await.unwrap();
        let mut connection = listener;
        listener = security::create_replacement_pipe(&endpoint).await.unwrap();
        let root = root.clone();
        let role = role.clone();
        tokio::spawn(async move {
            // Client::connect opens once solely to pin the server's identity.
            let Ok(request) = wire::read_frame::<wire::Request>(&mut connection).await else {
                return;
            };
            assert_eq!(request.protocol, wire::PROTOCOL);
            if request.command == "runtime_service_status" && role == "crash" {
                std::process::exit(23);
            }
            if request.command == "runtime_service_status" && role == "silent" {
                fs::write(root.join("silent.probed"), b"probed").unwrap();
                tokio::time::sleep(Duration::from_secs(30)).await;
                return;
            }
            let final_exit = matches!(role.as_str(), "exiting" | "exit-window");
            let shutdown =
                role == "exiting" || (role == "recovering" && !root.join("release").exists());
            let result = match request.command.as_str() {
                "runtime_service_status" if role == "malformed" => Ok(serde_json::json!({
                    "pid": std::process::id(), "protocol": wire::PROTOCOL,
                    "shutdown_in_progress": false,
                })),
                "runtime_service_status" => Ok(serde_json::json!({
                    "pid": std::process::id(), "protocol": wire::PROTOCOL,
                    "shutdown_in_progress": shutdown, "final_exit_requested": final_exit,
                })),
                "ensure_storage_ready" => {
                    fs::write(root.join(format!("{role}.storage")), b"storage").unwrap();
                    if final_exit || shutdown {
                        Err("storage initialization cannot begin while application shutdown is in progress".into())
                    } else {
                        Ok(serde_json::json!({"ready": true}))
                    }
                }
                _ => Err("unexpected startup fixture command".into()),
            };
            if wire::write_response(&mut connection, &wire::Response { result })
                .await
                .is_ok()
                && request.command == "runtime_service_status"
            {
                fs::write(root.join(format!("{role}.probed")), b"probed").unwrap();
            }
        });
    }
}
