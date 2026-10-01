use super::*;
use std::time::{Duration, Instant};

/// Explicitly disposable program copy, with no existing saves. This probe never
/// operates on an instance registered in the desktop application's database.
#[test]
#[ignore = "requires LANGAME_ABIOTIC_CONSOLE_PROBE with a marked, isolated server copy"]
fn abiotic_isolated_console_captures_readiness_and_normal_exit() {
    let root =
        PathBuf::from(std::env::var_os("LANGAME_ABIOTIC_CONSOLE_PROBE").expect("probe root"));
    assert!(root.is_absolute());
    assert!(root.join("langame-console-probe").is_file());
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let module = app_modules::discover_modules(repository.join("modules"))
        .unwrap()
        .into_iter()
        .find(|module| module.summary.id == "abioticfactor")
        .unwrap();
    let process = module.process.unwrap();
    let label = "managed";
    let executable =
        root.join("AbioticFactor/Binaries/Win64/AbioticFactorServer-Win64-Shipping.exe");
    let evidence_directory = root.join(format!(
        "acceptance-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&evidence_directory).unwrap();
    let log_path = evidence_directory.join(format!("console-{label}.log"));
    // Use the module's real fixed console switches, replacing only the probe's
    // ports, world and bind address. Never copy existing operator settings.
    let mut args = process
        .args_template
        .iter()
        .filter(|arg| !arg.contains("{{") && arg.starts_with('-'))
        .cloned()
        .collect::<Vec<_>>();
    args.extend(
        [
            "-PORT=17797",
            "-QueryPort=27897",
            "-MultiHome=127.0.0.1",
            "-LANOnly",
            "-MaxServerPlayers=1",
            "-WorldSaveName=LGSMDisposableConsoleProbe",
            "-SteamServerName=LGSM isolated console validation",
        ]
        .map(str::to_owned),
    );
    let plan = app_core::LaunchPlan {
        environment: Default::default(),
        instance_id: "abiotic-console-probe".into(),
        instance_name: "Isolated console probe".into(),
        module_id: "abioticfactor".into(),
        install_root: root.to_string_lossy().into_owned(),
        install_state: app_core::InstallState::Installed,
        uses_private_runtime: true,
        working_directory: executable.parent().unwrap().to_string_lossy().into_owned(),
        executable_path: executable.to_string_lossy().into_owned(),
        executable_exists: executable.is_file(),
        ready_to_launch: true,
        validation_issues: Vec::new(),
        args,
        command_line: String::new(),
        window_policy: ProcessWindowPolicy::Background,
        uses_script_entrypoint: false,
        requires_admin: false,
        host_surface: process.host_surface,
        host_notes: None,
        performance_policy: Default::default(),
        performance_preview: Default::default(),
    };
    let mut spawned = spawn_launch_plan_with_log_writer(
        &plan,
        &log_path,
        Box::new(File::create(&log_path).unwrap()),
    )
    .unwrap();
    let read_log = || fs::read_to_string(&log_path).unwrap_or_default();
    let deadline = Instant::now() + Duration::from_secs(45);
    let mut ready = false;
    loop {
        let output = read_log();
        if output.contains("Session creation completed") {
            ready = true;
            break;
        }
        if spawned
            .child
            .as_mut()
            .unwrap()
            .try_wait()
            .unwrap()
            .is_some()
            || Instant::now() >= deadline
            || output.contains("Output capture is incomplete")
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let stop = crate::windows_console_control::request_windows_console_ctrl_c(
        spawned.pid,
        &spawned.process_identity,
    );
    let exit_deadline = Instant::now() + Duration::from_secs(15);
    let mut exit_code = None;
    if stop.is_ok() {
        loop {
            if let Some(status) = spawned.child.as_mut().unwrap().try_wait().unwrap() {
                exit_code = status.code();
                break;
            }
            if Instant::now() >= exit_deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    let pid = spawned.pid;
    drop(spawned); // Only this disposable probe's Job may be recovered on failure.
    let output = read_log();
    let receipt = serde_json::json!({
        "host": label, "pid": pid, "native_ready": ready,
        "normal_interrupt_sent": stop.is_ok(), "normal_exit_code": exit_code,
        "stop_error": stop.as_ref().err().map(ToString::to_string),
        "captured_bytes": output.len(),
        "capture_incomplete": output.contains("Output capture is incomplete"),
        "shutdown_output": output.contains("RequestExit") || output.contains("Exiting"),
    });
    fs::write(
        evidence_directory.join(format!("console-{label}.json")),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    println!("{receipt}");
    assert!(ready, "native readiness was not captured");
    assert!(stop.is_ok(), "native interrupt failed: {stop:?}");
    assert_eq!(exit_code, Some(0), "native server did not exit normally");
    assert!(
        receipt["shutdown_output"].as_bool().unwrap(),
        "normal exit must retain the native shutdown tail"
    );
    assert!(!output.contains("Output capture is incomplete"));
}
