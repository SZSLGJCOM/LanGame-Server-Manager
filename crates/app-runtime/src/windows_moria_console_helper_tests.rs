use super::*;

const NONCE: &str = "1234567890abcdef1234567890abcdef";

fn reply(value: Value) -> Vec<u8> {
    format!("\nrunning 1 test\n{FRAME}{value}\n").into_bytes()
}

#[test]
fn exit_reply_requires_one_complete_current_owner_receipt() {
    let good = serde_json::json!({"nonce":NONCE,"root":40,"target":41,"queued":true,"input_read":"cancel_requested","error":null});
    assert!(parse_reply(&reply(good.clone()), NONCE, 40, 41).is_ok());
    for (key, bad) in [
        ("nonce", Value::from("ffffffffffffffffffffffffffffffff")),
        ("root", Value::from(42)),
        ("target", Value::from(42)),
        ("queued", Value::from("true")),
        ("input_read", Value::from("cancelled_and_saved")),
        ("error", Value::from("ignored error")),
    ] {
        let mut value = good.clone();
        value[key] = bad;
        assert!(parse_reply(&reply(value), NONCE, 40, 41).is_err(), "{key}");
    }
    let mut extra = good.clone();
    extra["command"] = Value::from("shutdown");
    assert!(parse_reply(&reply(extra), NONCE, 40, 41).is_err());
    assert!(parse_reply(good.to_string().as_bytes(), NONCE, 40, 41).is_err());
    let mut duplicate = reply(good.clone());
    duplicate.extend(reply(good));
    assert!(parse_reply(&duplicate, NONCE, 40, 41).is_err());
    let rejected = serde_json::json!({"nonce":NONCE,"root":40,"target":41,"queued":false,"input_read":null,"error":"input delivery uncertain"});
    let failure = parse_reply(&reply(rejected), NONCE, 40, 41)
        .unwrap_err()
        .to_string();
    assert!(failure.contains("input delivery uncertain") && failure.contains("no retry"));
}

#[test]
fn exit_helper_rejects_other_commands_modules_and_malformed_authority() {
    let base = serde_json::json!({
        "version":1,"module":"returntomoria","nonce":NONCE,"remaining_ms":12000,
        "root":40,"target":41,"job":0,"members":[],
    });
    for (key, bad) in [
        ("module", Value::from("conanexiles")),
        ("version", Value::from(2)),
        ("nonce", Value::from("nonce\rexit")),
        ("root", Value::from(0)),
        ("target", Value::from(u64::MAX)),
        ("remaining_ms", Value::from(0)),
        ("remaining_ms", Value::from(12_001)),
        ("remaining_ms", Value::from("12000")),
    ] {
        let mut value = base.clone();
        value[key] = bad;
        assert!(authorize(&value).is_none(), "{key}");
    }
    let mut extra = base.clone();
    extra["command"] = Value::from("save");
    assert!(authorize(&extra).is_none());
    assert!(authorize(&base).is_none());
}

#[test]
fn exit_helper_refuses_a_process_outside_the_transferred_job() {
    use windows_sys::Win32::System::JobObjects::CreateJobObjectW;
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    assert!(!job.is_null());
    let pid = std::process::id();
    let identity = inspect_process_identity(pid).unwrap().unwrap();
    let value = serde_json::json!({
        "version":1,"module":"returntomoria","nonce":NONCE,"remaining_ms":12000,
        "root":pid,"target":pid,"job":job as usize as u64,
        "members":[{"pid":pid,"identity":identity}],
    });
    // authorize owns and closes this distinct empty Job. It must refuse before
    // attachment/input; the current process belongs to no such Job.
    assert!(authorize(&value).is_none());
}

#[test]
#[ignore = "private child entry for the fixed Moria exit helper"]
fn moria_exit_helper_entry() {
    if std::env::var(CHILD_ENV).as_deref() != Ok("1") {
        return;
    }
    if std::env::var("LANGAME_MORIA_EXIT_FIXTURE_ROLE").as_deref() == Ok("holder") {
        hold_game_query_after_authorization();
        return;
    }
    if std::env::var("LANGAME_MORIA_EXIT_FIXTURE_SLEEP").as_deref() == Ok("true") {
        std::thread::sleep(Duration::from_secs(30));
        return;
    }
    if let Ok(mode) = std::env::var("LANGAME_MORIA_EXIT_FIXTURE_OUTPUT") {
        if mode == "delayed" {
            std::thread::sleep(Duration::from_millis(80));
        }
        let value = serde_json::json!({"nonce":NONCE,"root":40,"target":41,"queued":true,"input_read":"cancel_requested","error":null});
        std::io::stdout().write_all(&reply(value)).unwrap();
        std::process::exit(0);
    }
    std::process::exit(run_worker());
}

fn console_members() -> Vec<u32> {
    let mut members = [0; MAX_MEMBERS];
    let count =
        unsafe { GetConsoleProcessList(members.as_mut_ptr(), members.len() as u32) } as usize;
    assert!(count <= members.len());
    let mut members = members[..count].to_vec();
    members.sort_unstable();
    members
}

#[test]
fn isolated_exit_helper_rejection_preserves_the_managers_console() {
    windows_console_control::with_windows_console_ctrl_lock(|| {
        let before = console_members();
        let mut child = OwnedHelper::spawn(command().unwrap()).unwrap();
        let mut input = child.process.stdin.take().unwrap();
        input.write_all(b"{\"module\":\"wrong\"}").unwrap();
        drop(input);
        let stdout = child.process.stdout.take().unwrap();
        let output = unsafe { File::from_raw_handle(stdout.into_raw_handle()) };
        assert!(read_reply(&mut child, &output, Instant::now() + BUDGET).is_err());
        assert_eq!(child.process.try_wait().unwrap().unwrap().code(), Some(2));
        assert_eq!(console_members(), before);
    });
}

#[test]
fn identified_job_zero_refusal_keeps_worker_frame_boundary() {
    windows_console_control::with_windows_console_ctrl_lock(|| {
        let before = console_members();
        let mut child = OwnedHelper::spawn(command().unwrap()).unwrap();
        let input = child.process.stdin.take().unwrap();
        let mut writer = WindowsPipeWriter::new(input);
        let request = serde_json::json!({
            "version":1,"module":"returntomoria","nonce":NONCE,"remaining_ms":12000,
            "root":40,"target":41,"job":0,"members":[],
        });
        writer
            .write_line(&request.to_string(), &RuntimeStdinCancellation::default())
            .unwrap();
        drop(writer);
        let stdout = child.process.stdout.take().unwrap();
        let output = unsafe { File::from_raw_handle(stdout.into_raw_handle()) };
        let bytes = read_reply(&mut child, &output, Instant::now() + BUDGET).unwrap();
        let failure = parse_reply(&bytes, NONCE, 40, 41).unwrap_err().to_string();
        assert!(
            failure.contains("authorization rejected before console attachment"),
            "identified worker refusal was not preserved: {failure}"
        );
        assert!(!failure.contains("reply is invalid"));
        assert_eq!(console_members(), before);
    });
}

#[test]
fn exit_reply_pipe_drains_both_exited_and_delayed_senders() {
    for mode in ["exited", "delayed"] {
        let mut command = command().unwrap();
        command.env("LANGAME_MORIA_EXIT_FIXTURE_OUTPUT", mode);
        let mut child = OwnedHelper::spawn(command).unwrap();
        drop(child.process.stdin.take());
        if mode == "exited" {
            assert_eq!(
                unsafe { WaitForSingleObject(child.process.as_raw_handle(), 2000) },
                0
            );
        }
        let stdout = child.process.stdout.take().unwrap();
        let output = unsafe { File::from_raw_handle(stdout.into_raw_handle()) };
        let bytes = read_reply(&mut child, &output, Instant::now() + BUDGET).unwrap();
        assert!(parse_reply(&bytes, NONCE, 40, 41).is_ok(), "{mode}");
    }
}

#[test]
fn exit_helper_timeout_cleanup_reaps_only_the_owned_sender() {
    let mut command = command().unwrap();
    command.env("LANGAME_MORIA_EXIT_FIXTURE_SLEEP", "true");
    let child = OwnedHelper::spawn(command).unwrap();
    let pinned = WindowsProcessHandle::open(child.process.id(), 0)
        .unwrap()
        .unwrap();
    let began = Instant::now();
    drop(child);
    assert!(began.elapsed() < Duration::from_secs(2));
    assert!(pinned.wait_for_exit(0).unwrap());
}

const LOSS_ENTRY: &str = "windows_moria_console::helper::tests::moria_exit_parent_loss_fixture";

struct FixtureChild(Child);
impl Drop for FixtureChild {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            unsafe { WaitForSingleObject(self.0.as_raw_handle(), 500) };
            let _ = self.0.try_wait();
        }
    }
}
struct FixturePins(Vec<WindowsProcessHandle>);
impl Drop for FixturePins {
    fn drop(&mut self) {
        for process in &self.0 {
            if process.is_running().unwrap_or(false) {
                // Only exact identities issued by our fresh controller are held.
                // Failure cleanup must not make an orphaned fixture outlive a test.
                let _ = process.terminate();
            }
        }
    }
}

fn fixture_root() -> PathBuf {
    let root = PathBuf::from(std::env::var_os("LANGAME_MORIA_EXIT_FIXTURE_ROOT").unwrap());
    assert_eq!(
        std::fs::read(root.join("fixture-marker")).unwrap(),
        b"moria-exit-parent-loss"
    );
    root
}

fn fixture_command(root: &Path, role: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            LOSS_ENTRY,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("LANGAME_MORIA_EXIT_FIXTURE_ROOT", root)
        .env("LANGAME_MORIA_EXIT_FIXTURE_ROLE", role)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);
    command
}

fn wait_marker(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !path.is_file() {
        assert!(
            Instant::now() < deadline,
            "private lifetime fixture did not become ready"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn hold_game_query_after_authorization() {
    let root = fixture_root();
    let mut bytes = Vec::new();
    std::io::stdin().take(8193).read_to_end(&mut bytes).unwrap();
    assert!(bytes.len() <= 8192);
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    let job = OwnedWindowsHandle::new(value["job"].as_u64().unwrap() as usize as *mut c_void);
    let pid = u32::try_from(value["game"]["pid"].as_u64().unwrap()).unwrap();
    let identity: ProcessIdentity =
        serde_json::from_value(value["game"]["identity"].clone()).unwrap();
    let game = open_verified_windows_process(pid, &identity, 0)
        .unwrap()
        .unwrap();
    assert!(belongs(&game, job.as_raw()).unwrap());
    std::fs::write(root.join("query-held.ready"), b"query-capability-verified").unwrap();
    // The owner will be killed while this sender deliberately retains the real
    // query capability. Correct reclamation must come from its lifetime Job,
    // not from the usual helper deadline or a graceful Rust destructor.
    std::thread::sleep(Duration::from_secs(30));
    drop(job);
}

#[test]
#[ignore = "private controller/game entries for the parent-death fixture"]
fn moria_exit_parent_loss_fixture() {
    let Ok(role) = std::env::var("LANGAME_MORIA_EXIT_FIXTURE_ROLE") else {
        return;
    };
    let root = fixture_root();
    if role == "game" {
        std::thread::sleep(Duration::from_secs(30));
        return;
    }
    assert_eq!(role, "controller");
    // No resource-group Job: this tests the default per-launch last-handle rule.
    let game_job = windows_process_job::OwnedProcessJob::new().unwrap();
    let mut game_command = fixture_command(&root, "game");
    let game = FixtureChild(game_command.spawn().unwrap());
    assert_ne!(
        unsafe { AssignProcessToJobObject(game_job.as_raw(), game.0.as_raw_handle()) },
        0
    );
    let game_identity = inspect_process_identity(game.0.id()).unwrap().unwrap();
    let mut sender_command = command().unwrap();
    sender_command
        .env("LANGAME_MORIA_EXIT_FIXTURE_ROOT", &root)
        .env("LANGAME_MORIA_EXIT_FIXTURE_ROLE", "holder");
    let mut sender = OwnedHelper::spawn(sender_command).unwrap();
    let capability = query_capability(game_job.as_raw(), &sender).unwrap();
    let sender_identity = inspect_process_identity(sender.process.id())
        .unwrap()
        .unwrap();
    let mut input = sender.process.stdin.take().unwrap();
    let authorization = serde_json::json!({"job":capability as u64,"game":{"pid":game.0.id(),"identity":game_identity}});
    input
        .write_all(authorization.to_string().as_bytes())
        .unwrap();
    drop(input);
    wait_marker(&root.join("query-held.ready"));
    let identities = serde_json::json!({
        "game":{"pid":game.0.id(),"identity":game_identity},
        "sender":{"pid":sender.process.id(),"identity":sender_identity},
        "query_transfer_verified":true,
    });
    std::fs::write(root.join("children.json"), identities.to_string()).unwrap();
    std::fs::write(root.join("controller.ready"), b"ready").unwrap();
    std::thread::sleep(Duration::from_secs(30));
    drop(sender);
    drop(game);
}

#[test]
fn manager_death_reclaims_sender_and_game_after_query_authorization() {
    let root = crate::test_support::unique_test_root();
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("fixture-marker"), b"moria-exit-parent-loss").unwrap();
    let mut command = fixture_command(&root, "controller");
    let mut controller = FixtureChild(command.spawn().unwrap());
    let controller_identity = inspect_process_identity(controller.0.id())
        .unwrap()
        .unwrap();
    wait_marker(&root.join("controller.ready"));
    let value: Value =
        serde_json::from_slice(&std::fs::read(root.join("children.json")).unwrap()).unwrap();
    assert_eq!(value["query_transfer_verified"], true);
    let owned = collect_windows_process_tree(controller.0.id(), &controller_identity).unwrap();
    let mut pinned = FixturePins(Vec::new());
    for key in ["game", "sender"] {
        let pid = u32::try_from(value[key]["pid"].as_u64().unwrap()).unwrap();
        let identity: ProcessIdentity =
            serde_json::from_value(value[key]["identity"].clone()).unwrap();
        assert!(
            owned
                .iter()
                .any(|record| record.process_id == pid
                    && record.identity.as_ref() == Some(&identity))
        );
        assert!(process_identities_match(
            &identity,
            &ProcessIdentity {
                image_path: std::env::current_exe()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                creation_time: identity.creation_time
            }
        ));
        let handle = open_verified_windows_process(pid, &identity, PROCESS_TERMINATE)
            .unwrap()
            .unwrap();
        assert!(handle.is_running().unwrap());
        pinned.0.push(handle);
    }
    // Terminate only this owned synthetic manager. Neither the outer test nor
    // any other process owns a game/sender Job handle that could mask this loss.
    controller.0.kill().unwrap();
    assert_eq!(
        unsafe { WaitForSingleObject(controller.0.as_raw_handle(), 2000) },
        0
    );
    let _ = controller.0.try_wait().unwrap();
    for process in &pinned.0 {
        assert!(
            process.wait_for_exit(2000).unwrap(),
            "owned fixture survived manager loss: {}",
            process.pid
        );
    }
    drop(pinned);
    drop(controller);
    let canonical = std::fs::canonicalize(&root).unwrap();
    assert_eq!(
        canonical.parent(),
        Some(
            std::fs::canonicalize(std::env::temp_dir())
                .unwrap()
                .as_path()
        )
    );
    assert_eq!(
        std::fs::read(canonical.join("fixture-marker")).unwrap(),
        b"moria-exit-parent-loss"
    );
    std::fs::remove_dir_all(canonical).unwrap();
}
