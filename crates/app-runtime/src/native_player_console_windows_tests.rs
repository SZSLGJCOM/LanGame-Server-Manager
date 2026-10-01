use super::*;

#[test]
fn native_console_waiting_child() {
    if std::env::var_os("LGSM_NATIVE_CONSOLE_WAITING_CHILD").is_some() {
        std::thread::sleep(Duration::from_secs(30));
    }
}

#[test]
fn native_console_output_child() {
    if let Ok(mode) = std::env::var("LGSM_NATIVE_CONSOLE_OUTPUT_CHILD") {
        if mode == "delayed" {
            std::thread::sleep(Duration::from_millis(80));
        }
        let mut output = std::io::stdout().lock();
        output.write_all(b"LGM_BEGIN_PRIVATE_REPLY\n").unwrap();
        output
            .write_all(&vec![b'x'; if mode == "delayed" { 16 * 1024 } else { 512 }])
            .unwrap();
        output.write_all(b"\nLGM_END_PRIVATE_REPLY\n").unwrap();
        output.flush().unwrap();
    }
}

#[test]
fn native_console_drains_private_reply_before_and_after_helper_exit() {
    for mode in ["exited", "delayed"] {
        let mut child = OwnedHelper(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "native_player_console::windows::tests::native_console_output_child",
                    "--nocapture",
                ])
                .env("LGSM_NATIVE_CONSOLE_OUTPUT_CHILD", mode)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .creation_flags(crate::CREATE_NO_WINDOW)
                .spawn()
                .unwrap(),
        );
        if mode == "exited" {
            assert_eq!(
                unsafe { crate::WaitForSingleObject(child.0.as_raw_handle(), 2000) },
                0
            );
        }
        let stdout = child.0.stdout.take().unwrap();
        let response =
            read_helper_output(&mut child, &stdout, Instant::now() + Duration::from_secs(2))
                .unwrap();
        let response = String::from_utf8(response).unwrap();
        let body = response
            .split_once("LGM_BEGIN_PRIVATE_REPLY\n")
            .unwrap()
            .1
            .split_once("\nLGM_END_PRIVATE_REPLY\n")
            .unwrap()
            .0;
        assert_eq!(body.len(), if mode == "delayed" { 16 * 1024 } else { 512 });
        assert!(body.bytes().all(|byte| byte == b'x'));
    }
}

#[test]
fn native_console_cleanup_terminates_only_the_owned_helper_with_a_bounded_wait() {
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "native_player_console::windows::tests::native_console_waiting_child",
            "--nocapture",
        ])
        .env("LGSM_NATIVE_CONSOLE_WAITING_CHILD", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(crate::CREATE_NO_WINDOW)
        .spawn()
        .unwrap();
    let verification = crate::WindowsProcessHandle::open(child.id(), 0)
        .unwrap()
        .unwrap();
    let started = Instant::now();
    drop(OwnedHelper(child));
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(
        verification.wait_for_exit(0).unwrap(),
        "owned helper survived cleanup"
    );
}

#[test]
fn native_console_refuses_a_reused_or_mismatched_process_before_spawning_helper() {
    let pid = std::process::id();
    let mut identity = crate::inspect_process_identity(pid).unwrap().unwrap();
    identity.creation_time = identity.creation_time.saturating_sub(1);
    assert_eq!(
        collect_with_executable(
            std::path::Path::new("missing-helper.exe"),
            pid,
            &identity,
            "1234567890abcdef1234567890abcdef"
        ),
        Err(Error::ProcessUnavailable)
    );
}

#[test]
#[ignore = "requires an explicitly marked Moria scratch installation and a built desktop helper"]
fn native_console_owned_moria_empty_server_end_to_end() {
    let executable = std::path::PathBuf::from(std::env::var_os("LGSM_MORIA_PROBE_HELPER").unwrap());
    let root = std::path::PathBuf::from(std::env::var_os("LGSM_MORIA_PROBE_ROOT").unwrap());
    assert!(root.join(".langame-owned-player-probe").is_file());
    let config = std::fs::read_to_string(root.join("MoriaServerConfig.ini")).unwrap();
    assert!(config.contains("ListenAddress=127.0.0.1"));
    assert!(config.contains("AdvertiseAddress=127.0.0.1"));
    assert!(
        config.contains("[Console]\nEnabled=true") || config.contains("[Console]\r\nEnabled=true")
    );
    let plan = app_core::LaunchPlan {
        environment: Default::default(),
        instance_id: "moria-console-probe".into(),
        instance_name: "Isolated console probe".into(),
        module_id: "returntomoria".into(),
        install_root: root.to_string_lossy().into_owned(),
        install_state: app_core::InstallState::Installed,
        uses_private_runtime: true,
        working_directory: root.to_string_lossy().into_owned(),
        executable_path: root.join("MoriaServer.exe").to_string_lossy().into_owned(),
        executable_exists: true,
        ready_to_launch: true,
        validation_issues: Vec::new(),
        args: Vec::new(),
        command_line: String::new(),
        window_policy: app_core::ProcessWindowPolicy::Background,
        uses_script_entrypoint: false,
        requires_admin: false,
        host_surface: app_core::ProcessHostSurface::ManagedTerminal,
        host_notes: None,
        performance_policy: app_core::RuntimePerformancePolicy::default(),
        performance_preview: app_core::RuntimePerformancePolicyPreview::default(),
    };
    struct OwnedServer(crate::SpawnedProcess);
    impl Drop for OwnedServer {
        fn drop(&mut self) {
            crate::stop_spawned_process(&mut self.0)
                .expect("stop only the owned scratch process tree");
        }
    }
    let log = root.join("native-console-production-probe.log");
    let mut server = OwnedServer(crate::spawn_launch_plan(&plan, &log).unwrap());
    assert!(
        crate::stabilize_spawned_process(
            &plan.executable_path,
            &mut server.0,
            Duration::from_millis(300)
        )
        .unwrap()
        .is_none()
    );
    let pid = server.0.pid;
    let identity = server.0.process_identity.clone();
    assert!(identity.image_path.ends_with("moriaserver.exe"));
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut nonce_number = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let mut completed = 0;
    while completed < 2 {
        nonce_number += 1;
        let nonce = format!("{nonce_number:032x}");
        let result = match collect_with_executable(&executable, pid, &identity, &nonce) {
            Ok(result) => result,
            Err(error) if Instant::now() < deadline && completed == 0 => {
                println!("Waiting for owned Moria console: {error}");
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
            Err(error) => panic!("owned Moria query failed: {error}"),
        };
        assert!(result.contains("Players: 0/8\n"));
        assert!(result.starts_with(&format!(
            "Unknown command \"LGM_PLAYER_QUERY_BEGIN_{nonce}\"!"
        )));
        assert!(result.ends_with(&format!(
            "Unknown command \"LGM_PLAYER_QUERY_END_{nonce}\"!\n"
        )));
        completed += 1;
    }
    assert!(crate::process_matches_identity(pid, &identity).unwrap());
    println!(
        "Validated production bootstrap PID ownership, hidden native console, two fresh empty frames and private helper IPC."
    );
}
