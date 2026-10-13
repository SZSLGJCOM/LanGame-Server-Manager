use super::*;

const MOD_FILE: &str = "data/ugc/Master/content/322330/123/modmain.lua";

struct FileProbeFixture {
    root: PathBuf,
    app: tauri::App<tauri::test::MockRuntime>,
    instance_id: Option<String>,
    unregistered: Option<app_runtime::SpawnedProcess>,
}

impl FileProbeFixture {
    fn probe_diagnostics(&self) -> String {
        use std::io::{Read, Seek, SeekFrom};

        let tail = |name: &str| -> std::io::Result<String> {
            let mut file = fs::File::open(self.root.join(name))?;
            let start = file.metadata()?.len().saturating_sub(4096);
            file.seek(SeekFrom::Start(start))?;
            let mut bytes = Vec::new();
            file.take(4096).read_to_end(&mut bytes)?;
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        };
        let state = self.app.state::<DesktopState>();
        let running = match (
            self.instance_id.as_deref(),
            state.runtime_supervisor.try_lock(),
        ) {
            (Some(id), Ok(mut supervisor)) => {
                format!("{:?}", supervisor.instance_process_tree_is_running(id))
            }
            (None, _) => String::from("instance not registered"),
            (_, Err(error)) => format!("supervisor unavailable: {error}"),
        };
        format!(
            "process_tree_running={running}; script_started={:?}; ready={:?}; release={:?}; master_log_tail={:?}",
            tail("probe-started"),
            tail("probe-ready"),
            tail("probe-release"),
            tail("master.log"),
        )
    }
}

impl Drop for FileProbeFixture {
    fn drop(&mut self) {
        let mut errors = Vec::new();
        if let Some(mut process) = self.unregistered.take()
            && let Err(error) = stop_spawned_process(&mut process)
        {
            errors.push(error.to_string());
        }
        if let Some(id) = &self.instance_id {
            let state = self.app.state::<DesktopState>();
            if let Some(mut running) = state
                .runtime_supervisor
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take_running_for_stop(id)
                && let Err(error) = stop_managed_instance(&mut running)
            {
                errors.push(error.to_string());
            }
        }
        if let Err(error) = fs::remove_dir_all(&self.root)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            errors.push(error.to_string());
        }
        if !errors.is_empty() {
            if std::thread::panicking() {
                eprintln!("Native probe fixture cleanup failed: {}", errors.join("; "));
            } else {
                panic!("Native probe fixture cleanup failed: {}", errors.join("; "));
            }
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_task_rechecks_patched_file_after_waiting_for_native_mod_probe() {
    // This shared fixture holds the command-test lock while its environment is
    // overridden. The new database, files and process all belong to this test.
    let (_lock, _environment, app, mut storage) =
        crate::commands::tests::assistant_assessment_fixture().await;
    let root = storage.paths.app_data_root.parent().unwrap().to_path_buf();
    assert!(
        storage
            .paths
            .app_data_root
            .ends_with("uninitialized-assessment")
    );
    assert!(
        root.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("lg-test-assessment-")
    );
    assert!(
        fs::canonicalize(&root)
            .unwrap()
            .starts_with(fs::canonicalize(std::env::temp_dir()).unwrap())
    );
    let mut fixture = FileProbeFixture {
        root,
        app,
        instance_id: None,
        unregistered: None,
    };
    tokio::time::timeout(
        Duration::from_secs(45),
        exercise_file_probe(&mut fixture, &mut storage),
    )
    .await
    .expect("bounded native Mod/file verification fixture");
}

async fn exercise_file_probe(fixture: &mut FileProbeFixture, storage: &mut StorageBootstrap) {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    storage.paths.modules_root = workspace.join("modules");
    storage.paths.migrations_root = workspace.join("migrations");
    storage.settings = storage.paths.settings();
    fixture
        .app
        .state::<DesktopState>()
        .app_state
        .write()
        .unwrap()
        .settings = storage.settings.clone();
    fs::create_dir_all(storage.paths.database_path.parent().unwrap()).unwrap();
    initialize_database(&storage.paths).await.unwrap();
    let descriptors = discover_modules(&storage.paths.modules_root).unwrap();
    let descriptor = find_descriptor(&descriptors, "dontstarve").unwrap();
    sync_modules(&storage.paths, std::slice::from_ref(descriptor))
        .await
        .unwrap();
    crate::commands::tests::prepare_fake_registered_program(&storage.paths, descriptor)
        .await
        .unwrap();
    let created = create_instance(
        &storage.paths,
        descriptor,
        CreateInstanceInput {
            name: String::from("Native Mod file verification fixture"),
            module_id: String::from("dontstarve"),
        },
    )
    .await
    .unwrap();
    fixture.instance_id = Some(created.summary.id.clone());
    let initial = read_instance_details(&storage.paths, &created.summary.id)
        .await
        .unwrap();
    let mut settings: Value = serde_json::from_str(&initial.settings_json).unwrap();
    settings["enable_caves"] = json!(false);
    settings["master_modoverrides_lua"] = json!("return {['workshop-123']={enabled=true}}");
    let details = update_instance(
        &storage.paths,
        UpdateInstanceInput {
            id: initial.summary.id.clone(),
            bind_ip: initial.summary.bind_ip.clone(),
            auto_backup_on_stop: false,
            backup_retention_count: initial.backup_retention_count,
            settings_json: settings.to_string(),
            ports: initial.ports,
        },
    )
    .await
    .unwrap();
    let path = Path::new(&details.config_file_path)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(MOD_FILE);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "return 1\n").unwrap();
    let source =
        app_storage::read_instance_patch_file(&storage.paths, &details.summary.id, MOD_FILE)
            .await
            .unwrap();
    let prepared = app_storage::prepare_instance_file_patch(
        &storage.paths,
        &details.summary.id,
        app_storage::InstanceTextPatch {
            file: MOD_FILE.into(),
            source_sha256: source.source_sha256,
            before: "return 1".into(),
            after: "return 2".into(),
        },
    )
    .await
    .unwrap();
    let patched =
        app_storage::apply_instance_file_patch(&storage.paths, &details.summary.id, prepared)
            .await
            .unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "return 2\n");

    let script = fixture.root.join("mod-probe.ps1");
    // Only the transport/ACK is simulated. Receipt assessment and its source
    // hash read are production code, and the process has a real native identity.
    fs::write(
        &script,
        r#"
[IO.File]::WriteAllText((Join-Path $PSScriptRoot 'probe-started'), [string]$PID)
while ($null -ne ($line = [Console]::In.ReadLine())) {
    if ($line -eq '[LGSM-MOD-PROBE-EXIT]') { exit 0 }
    if ($line -notmatch "local nonce='([a-f0-9]{32})'") { exit 70 }
    $nonce = $Matches[1]
    [IO.File]::WriteAllText((Join-Path $PSScriptRoot 'probe-ready'), $nonce)
    $wait = [Diagnostics.Stopwatch]::StartNew()
    while (-not [IO.File]::Exists((Join-Path $PSScriptRoot 'probe-release'))) {
        if ($wait.Elapsed.TotalSeconds -gt 12) { exit 71 }
        Start-Sleep -Milliseconds 10
    }
    [Console]::WriteLine('[LGSM-DST-MODS-BEGIN:' + $nonce + ']')
    [Console]::WriteLine('[LGSM-DST-MOD:' + $nonce + '] 1 1 1 1 1 0 1')
    [Console]::WriteLine('[LGSM-DST-MODS-END:' + $nonce + '] 1')
    [Console]::Out.Flush()
}
"#,
    )
    .unwrap();
    let powershell = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:/Windows"))
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let launch = LaunchPlan {
        environment: Default::default(),
        instance_id: details.summary.id.clone(),
        instance_name: details.summary.name.clone(),
        module_id: "dontstarve".into(),
        install_root: fixture.root.to_string_lossy().into_owned(),
        install_state: InstallState::Installed,
        uses_private_runtime: false,
        working_directory: fixture.root.to_string_lossy().into_owned(),
        executable_path: powershell.to_string_lossy().into_owned(),
        executable_exists: true,
        ready_to_launch: true,
        validation_issues: Vec::new(),
        args: vec![
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-File".into(),
            script.to_string_lossy().into_owned(),
        ],
        command_line: "synthetic native Mod probe".into(),
        window_policy: ProcessWindowPolicy::Background,
        uses_script_entrypoint: false,
        requires_admin: false,
        host_surface: app_core::ProcessHostSurface::ManagedTerminal,
        host_notes: None,
        performance_policy: RuntimePerformancePolicy::default(),
        performance_preview: Default::default(),
    };
    fixture.unregistered =
        Some(spawn_launch_plan(&launch, fixture.root.join("master.log")).unwrap());
    let spawned = fixture.unregistered.as_mut().unwrap();
    let started = mark_instance_process_started_with_identity(
        &storage.paths,
        &StartedInstanceProcess {
            instance_id: &details.summary.id,
            session_id: Some("native-mod-file-probe"),
            process_key: "master",
            display_name: "Master",
            pid: spawned.pid,
            log_path: &spawned.log_path,
            is_primary: true,
        },
        Some(&spawned.process_identity),
    )
    .await
    .unwrap();
    let managed = ManagedProcess {
        run_id: started.run_id,
        process_key: "master".into(),
        display_name: "Master".into(),
        pid: spawned.pid,
        process_identity: spawned.process_identity.clone(),
        root_process_identity: spawned.root_process_identity.clone(),
        log_path: spawned.log_path.clone(),
        is_primary: true,
        uses_script_entrypoint: false,
        performance_policy: RuntimePerformancePolicy::default(),
        last_performance_refresh: None,
        last_performance_target_count: None,
        last_performance_application: None,
        child: spawned.child.take(),
        hidden_desktop: spawned.hidden_desktop.take(),
    };
    let state = fixture.app.state::<DesktopState>();
    state.runtime_supervisor.lock().unwrap().insert_running(
        details.summary.clone(),
        Some("native-mod-file-probe".into()),
        vec![managed],
    );
    fixture.unregistered = None;
    let after = read_instance_details(&storage.paths, &details.summary.id)
        .await
        .unwrap();
    let (template, _) = super::task_tests::task_fixture(false);
    let mut task = Arc::try_unwrap(template).unwrap();
    task.request.goal = AssistantTaskGoal::RestoreService;
    task.instance_id = Some(details.summary.id.clone());
    task.module_id = Some("dontstarve".into());
    task.initial_settings = settings;
    task.file_changes = vec![patched];
    let mut plan =
        assistant_safe_none_plan("Verify native Mod state and the preceding patch.".into());
    plan.action = AssistantOperationAction::StartServer;
    let output = assistant_operation_output(&plan, 0);
    // Startup readiness is an input to assessment; the following Mod probe
    // still traverses the managed stdin, native process identity and log reader.
    let verification = AssistantOperationVerification {
        status: AssistantVerificationStatus::Verified,
        summary: "Fixture readiness input".into(),
        run_id: Some(started.run_id),
        evidence: Value::Null,
        can_continue: false,
    };
    for (round, change_during_probe) in [false, true].into_iter().enumerate() {
        let round = round + 1;
        let ready = fixture.root.join("probe-ready");
        let release = fixture.root.join("probe-release");
        for gate in [&ready, &release] {
            if gate.exists() {
                fs::remove_file(gate).unwrap();
            }
        }
        let assessment = assess_assistant_task(
            &state,
            storage,
            &task,
            Some(&after),
            &output,
            None,
            Some(&verification),
        );
        tokio::pin!(assessment);
        let gate = tokio::time::timeout(Duration::from_secs(10), async {
            while !ready.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });
        tokio::select! {
            biased;
            receipt = &mut assessment => {
                panic!(
                    "native probe assessment completed before release in round {round} (change_during_probe={change_during_probe}): {receipt:?}; {}",
                    fixture.probe_diagnostics(),
                );
            }
            result = gate => {
                assert!(
                    result.is_ok(),
                    "native probe must reach its reply gate within 10s in round {round} (change_during_probe={change_during_probe}); {}",
                    fixture.probe_diagnostics(),
                );
            }
        }
        assert_eq!(fs::read_to_string(&path).unwrap(), "return 2\n");
        if change_during_probe {
            fs::write(&path, "return 3\n").unwrap();
        }
        fs::write(&release, "release").unwrap();
        let receipt = assessment.await;
        let check = |name| {
            receipt
                .checks
                .iter()
                .find(|check| check.name == name)
                .unwrap()
                .status
        };
        assert_eq!(
            check("required_mods_running"),
            AssistantTaskCheckStatus::Satisfied
        );
        assert_eq!(
            check("task_snapshot_unchanged"),
            AssistantTaskCheckStatus::Satisfied
        );
        if change_during_probe {
            assert_eq!(
                check("file_changes_preserved"),
                AssistantTaskCheckStatus::Unknown
            );
            assert_eq!(receipt.status, AssistantTaskStatus::Inconclusive);
        } else {
            assert_eq!(
                check("file_changes_preserved"),
                AssistantTaskCheckStatus::Satisfied
            );
            assert_eq!(receipt.status, AssistantTaskStatus::Completed);
        }
    }

    // Both file-race assertions are complete. Release the synthetic stdin loop;
    // Drop still checks the original owner and reaps its complete process tree.
    state
        .runtime_supervisor
        .lock()
        .unwrap()
        .dispatch_command(&details.summary.id, Some("master"), "[LGSM-MOD-PROBE-EXIT]")
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if state
                .runtime_supervisor
                .lock()
                .unwrap()
                .instance_process_tree_is_running(&details.summary.id)
                .unwrap()
                == Some(false)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("synthetic Mod probe must exit after all assertions");
}
