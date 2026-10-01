use super::*;
use app_storage::StoragePaths;

#[tokio::test]
async fn dst_save_probe_skips_existing_managed_output_before_and_after_rotation() {
    use app_storage::managed_console_log::ManagedConsoleLog;

    let root = std::env::temp_dir().join(format!("lg-dst-log-cursor-{}", uuid::Uuid::new_v4()));
    for rotated in [false, true] {
        let path = root
            .join(if rotated { "rotated" } else { "initial" })
            .join("managed-console")
            .join("run-1-master.log");
        let writer = ManagedConsoleLog::open(&path).unwrap();
        if rotated {
            writer.write_all(&vec![b'\n'; 8 * 1024 * 1024]).unwrap();
        }
        writer.write_all(b"historical output\n").unwrap();
        let mut log = ProbeLog::new(path.to_str().unwrap());
        log.skip_existing().await.unwrap();
        writer.write_all(b"[LGSM-DST-SAVED:fresh]\n").unwrap();
        assert_eq!(log.lines().await.unwrap(), ["[LGSM-DST-SAVED:fresh]"]);
        assert!(log.lines().await.unwrap().is_empty());
    }
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn dst_save_probe_rejects_expired_output_instead_of_accepting_partial_evidence() {
    use app_storage::managed_console_log::ManagedConsoleLog;

    let root = std::env::temp_dir().join(format!("lg-dst-log-gap-{}", uuid::Uuid::new_v4()));
    let path = root.join("managed-console").join("run-1-master.log");
    let writer = ManagedConsoleLog::open(&path).unwrap();
    let mut log = ProbeLog::new(path.to_str().unwrap());
    log.skip_existing().await.unwrap();
    let noise = b"ordinary stats \n".repeat(65_536);
    for _ in 0..33 {
        writer.write_all(&noise).unwrap();
    }
    writer.write_all(b"[LGSM-DST-SAVED:fresh]\n").unwrap();
    let error = log.lines().await.unwrap_err();
    assert!(
        error.contains("DST process log capture is incomplete"),
        "{error}"
    );
    assert!(error.contains("expired"), "{error}");
    drop(writer);
    fs::remove_dir_all(root).unwrap();
}

struct Fixture {
    root: PathBuf,
    storage: StorageBootstrap,
    state: DesktopState,
    details: InstanceDetails,
    processes: Vec<(ProcessLaunchPlan, app_runtime::SpawnedProcess)>,
}

impl Fixture {
    async fn new(mode: &str) -> Self {
        Self::new_with_layout(mode, false).await
    }

    async fn new_with_layout(mode: &str, four_shards: bool) -> Self {
        let root = std::env::temp_dir().join(format!("lg-dst-lifecycle-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("db")).unwrap();
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let paths = StoragePaths {
            app_data_root: root.clone(),
            settings_path: root.join("settings.json"),
            database_path: root.join("db/lgs.db"),
            logs_root: root.join("logs"),
            modules_root: workspace.join("modules"),
            migrations_root: workspace.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances").join(".trash"),
        };
        initialize_database(&paths).await.unwrap();
        let descriptors = discover_modules(&paths.modules_root).unwrap();
        let descriptor = find_descriptor(&descriptors, "dontstarve").unwrap();
        sync_modules(&paths, std::slice::from_ref(descriptor))
            .await
            .unwrap();
        crate::commands::tests::prepare_fake_registered_program(&paths, descriptor)
            .await
            .unwrap();
        let created = create_instance(
            &paths,
            descriptor,
            CreateInstanceInput {
                name: String::from("DST lifecycle fixture"),
                module_id: String::from("dontstarve"),
            },
        )
        .await
        .unwrap();
        let mut details = read_instance_details(&paths, &created.summary.id)
            .await
            .unwrap();
        let mut settings: serde_json::Value = serde_json::from_str(&details.settings_json).unwrap();
        settings["enable_caves"] = serde_json::json!(false);
        settings["shard_layout"] = serde_json::json!(if four_shards {
            "island_adventures"
        } else {
            "standard"
        });
        details.settings_json = settings.to_string();
        let storage = StorageBootstrap {
            settings: paths.settings(),
            storage_status: paths.probe_status(),
            paths,
        };
        let state = DesktopState::default();
        state.app_state.write().unwrap().settings = storage.settings.clone();
        let script = root.join("shard.ps1");
        // This process models native console echo, readiness/save ACKs and exit.
        // The separate Lua tests exercise the commands against native API shapes.
        fs::write(
            &script,
            format!(
                r#"
param([string]$Shard)
while ($null -ne ($line = [Console]::In.ReadLine())) {{
    [Console]::WriteLine($line)
    [Console]::Out.Flush()
    if ($line -match '\[LGSM-DST-READY:([a-f0-9]+)\]') {{
        if ('{mode}' -eq 'early-exit') {{ exit 7 }}
        if ('{mode}' -eq 'workshop-timeout') {{
            [Console]::WriteLine('[00:00:35]: DownloadServerMods timed out with no response from Workshop...')
        }}
        if ('{mode}' -eq 'failed-mod') {{
            [Console]::WriteLine('[00:00:01]: [LGSM-DST-FAILED:' + $Matches[1] + '] workshop-2039181790: modmain.lua:42: missing dependency')
        }}
        if ('{mode}' -eq 'old-failed-mod') {{
            [Console]::WriteLine('[00:00:01]: [LGSM-DST-FAILED:old] workshop-2039181790: previous run')
        }}
        [Console]::WriteLine('[00:00:01]: [LGSM-DST-READY:' + $Matches[1] + ']')
        [Console]::Out.Flush()
    }}
    if ($line -match '\[LGSM-DST-SAVED:([a-f0-9]+)\]') {{
        if ('{mode}' -eq 'volcano-missing-save' -and $Shard -eq 'Volcano') {{ exit 0 }}
        if ('{mode}' -eq 'stalled-save') {{ Start-Sleep -Seconds 20; continue }}
        if ('{mode}' -eq 'surviving-helper') {{
            $start = New-Object Diagnostics.ProcessStartInfo
            $start.FileName = Join-Path $PSHOME 'powershell.exe'
            $start.Arguments = '-NoProfile -Command "Start-Sleep -Seconds 20"'
            $start.UseShellExecute = $false
            $start.CreateNoWindow = $true
            $helper = [Diagnostics.Process]::Start($start)
            [IO.File]::WriteAllText((Join-Path $PSScriptRoot 'helper-pid'), [string]$helper.Id)
        }}
        if ('{mode}' -ne 'missing-save') {{
            [Console]::WriteLine('[00:00:02]: [LGSM-DST-SAVED:' + $Matches[1] + ']')
            [Console]::Out.Flush()
        }}
        if ('{mode}' -eq 'abnormal-exit') {{ exit 9 }}
        exit 0
    }}
}}
"#
            ),
        )
        .unwrap();
        let launch = LaunchPlan {
            environment: Default::default(),
            instance_id: details.summary.id.clone(),
            instance_name: details.summary.name.clone(),
            module_id: String::from("dontstarve"),
            install_root: root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            uses_private_runtime: false,
            working_directory: root.to_string_lossy().into_owned(),
            executable_path: String::from(
                "C:/Windows/System32/WindowsPowerShell/v1.0/powershell.exe",
            ),
            executable_exists: true,
            ready_to_launch: true,
            validation_issues: Vec::new(),
            args: vec![
                String::from("-NoProfile"),
                String::from("-File"),
                script.to_string_lossy().into_owned(),
            ],
            command_line: String::from("synthetic DST shard"),
            window_policy: ProcessWindowPolicy::Background,
            uses_script_entrypoint: false,
            requires_admin: false,
            host_surface: app_core::ProcessHostSurface::ManagedTerminal,
            host_notes: None,
            performance_policy: RuntimePerformancePolicy::default(),
            performance_preview: Default::default(),
        };
        let processes = app_core::dst_shards::dst_shards(&settings)
            .unwrap()
            .into_iter()
            .map(|shard| {
                let mut shard_launch = launch.clone();
                shard_launch.args.push(String::from(shard.directory));
                let spawned = spawn_launch_plan(
                    &shard_launch,
                    root.join(format!("{}.log", shard.process_key)),
                )
                .unwrap();
                let plan = ProcessLaunchPlan {
                    process_key: String::from(shard.process_key),
                    display_name: String::from(shard.directory),
                    log_path: spawned.log_path.clone(),
                    launch_plan: shard_launch,
                };
                (plan, spawned)
            })
            .collect();
        Self {
            root,
            storage,
            state,
            details,
            processes,
        }
    }

    async fn ready(&mut self) -> Result<(), String> {
        let reservation = match self
            .state
            .try_reserve_runtime_start(&self.details.summary.id, "test")?
        {
            RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
            other => return Err(format!("unexpected readiness reservation: {other:?}")),
        };
        tokio::time::timeout(
            Duration::from_secs(15),
            wait_until_ready(
                &self.state,
                &self.storage,
                &self.details,
                &mut self.processes,
                "test",
                &reservation,
            ),
        )
        .await
        .expect("bounded readiness probe")
    }

    async fn register(&mut self) -> ActiveInstanceRun {
        let session = uuid::Uuid::new_v4().to_string();
        let mut managed = Vec::new();
        for (plan, spawned) in &mut self.processes {
            let process = mark_instance_process_started_with_identity(
                &self.storage.paths,
                &StartedInstanceProcess {
                    instance_id: &self.details.summary.id,
                    session_id: Some(&session),
                    process_key: &plan.process_key,
                    display_name: &plan.display_name,
                    pid: spawned.pid,
                    log_path: &spawned.log_path,
                    is_primary: plan.process_key == "master",
                },
                Some(&spawned.process_identity),
            )
            .await
            .unwrap();
            managed.push(ManagedProcess {
                run_id: process.run_id,
                process_key: plan.process_key.clone(),
                display_name: plan.display_name.clone(),
                pid: spawned.pid,
                process_identity: spawned.process_identity.clone(),
                root_process_identity: spawned.root_process_identity.clone(),
                log_path: spawned.log_path.clone(),
                is_primary: plan.process_key == "master",
                uses_script_entrypoint: false,
                performance_policy: RuntimePerformancePolicy::default(),
                last_performance_refresh: None,
                last_performance_target_count: None,
                last_performance_application: None,
                child: spawned.child.take(),
                hidden_desktop: spawned.hidden_desktop.take(),
            });
        }
        self.state
            .runtime_supervisor
            .lock()
            .unwrap()
            .insert_running(self.details.summary.clone(), Some(session), managed);
        read_active_instance_run(&self.storage.paths, &self.details.summary.id)
            .await
            .unwrap()
            .unwrap()
    }

    async fn shutdown(&self) -> Result<(), String> {
        tokio::time::timeout(
            Duration::from_secs(15),
            save_and_shutdown(&self.state, &self.storage, &self.details),
        )
        .await
        .expect("bounded save confirmation")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.processes
            .retain(|(_, process)| process.child.is_some());
        stop_spawned_processes(&mut self.processes);
        if let Some(mut managed) = self
            .state
            .runtime_supervisor
            .lock()
            .unwrap()
            .take_running_for_stop(&self.details.summary.id)
        {
            stop_managed_instance(&mut managed).unwrap();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn dst_lifecycle_confirms_ready_save_and_actual_zero_exit() {
    let mut fixture = Fixture::new("normal").await;
    fixture.ready().await.unwrap();
    let active = fixture.register().await;
    fixture.shutdown().await.unwrap();
    let stopped = finish_confirmed_stop(
        &fixture.state,
        &fixture.storage,
        &fixture.details.summary.id,
        &active,
    )
    .await
    .unwrap();
    assert_eq!(stopped.len(), 1);
    assert_eq!(stopped[0].exit_code, Some(0));
    assert!(
        read_active_instance_run(&fixture.storage.paths, &fixture.details.summary.id)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn dst_lifecycle_four_shards_have_separate_logs_and_confirm_every_save() {
    let mut fixture = Fixture::new_with_layout("normal", true).await;
    fixture.ready().await.unwrap();
    let active = fixture.register().await;
    assert_eq!(active.processes.len(), 4);
    assert_eq!(
        active
            .processes
            .iter()
            .filter(|process| process.is_primary)
            .count(),
        1
    );
    fixture.shutdown().await.unwrap();
    let stopped = finish_confirmed_stop(
        &fixture.state,
        &fixture.storage,
        &fixture.details.summary.id,
        &active,
    )
    .await
    .unwrap();
    assert_eq!(stopped.len(), 4);
    for key in ["master", "caves", "islands", "volcano"] {
        let process = stopped
            .iter()
            .find(|process| process.process_key == key)
            .unwrap();
        assert_eq!(process.exit_code, Some(0));
        let body = fs::read_to_string(fixture.root.join(format!("{key}.log"))).unwrap();
        assert!(
            body.lines()
                .any(|line| line.starts_with("[00:00:01]: [LGSM-DST-READY:")),
            "{key}: {body}"
        );
        assert!(
            body.lines()
                .any(|line| line.starts_with("[00:00:02]: [LGSM-DST-SAVED:")),
            "{key}: {body}"
        );
    }
    assert!(
        read_active_instance_run(&fixture.storage.paths, &fixture.details.summary.id)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn dst_lifecycle_a_volcano_without_save_ack_blocks_success_and_backup() {
    let mut fixture = Fixture::new_with_layout("volcano-missing-save", true).await;
    fixture.ready().await.unwrap();
    fixture.register().await;
    let error = fixture.shutdown().await.unwrap_err();
    assert!(
        error.contains("Volcano exited without confirming its save"),
        "{error}"
    );
    assert!(error.contains("Automatic backup was skipped"), "{error}");
    assert!(
        read_active_instance_run(&fixture.storage.paths, &fixture.details.summary.id)
            .await
            .unwrap()
            .is_some(),
        "failed save confirmation must keep the cluster managed"
    );
}

#[tokio::test]
async fn dst_lifecycle_confirmed_stop_reaps_surviving_helpers() {
    let mut fixture = Fixture::new("surviving-helper").await;
    fixture.ready().await.unwrap();
    let active = fixture.register().await;
    fixture.shutdown().await.unwrap();
    let helper_pid = fs::read_to_string(fixture.root.join("helper-pid"))
        .unwrap()
        .parse::<u32>()
        .unwrap();
    let helper_identity = inspect_process_identity(helper_pid)
        .unwrap()
        .expect("helper must survive the shard until confirmed-stop cleanup");
    let stopped = finish_confirmed_stop(
        &fixture.state,
        &fixture.storage,
        &fixture.details.summary.id,
        &active,
    )
    .await
    .unwrap();
    assert_eq!(stopped[0].exit_code, Some(0));
    assert!(
        !process_matches_identity(helper_pid, &helper_identity).unwrap(),
        "successful saved exit must also finish the launch's surviving helpers",
    );
}

#[tokio::test]
async fn dst_lifecycle_rejects_exit_before_world_ready() {
    let mut fixture = Fixture::new("early-exit").await;
    assert!(
        fixture
            .ready()
            .await
            .unwrap_err()
            .contains("exited before its world was ready")
    );
}

#[tokio::test]
async fn dst_lifecycle_stop_observes_owned_handle_without_reopening_pid() {
    let mut fixture = Fixture::new("normal").await;
    fixture.ready().await.unwrap();
    let active = fixture.register().await;
    let process = &active.processes[0];
    let inspect = |pid, _identity: &app_core::ProcessIdentity| {
        Err(app_runtime::RuntimeProcessError::InspectProcess {
            pid,
            source: std::io::Error::from_raw_os_error(5),
        })
    };
    assert_eq!(
        shutdown_process_is_running(
            &fixture.state,
            &fixture.details.summary.id,
            process,
            inspect,
        ),
        Ok(true)
    );
    fixture.shutdown().await.unwrap();
    assert_eq!(
        shutdown_process_is_running(
            &fixture.state,
            &fixture.details.summary.id,
            process,
            inspect,
        ),
        Ok(false)
    );
    let stopped = finish_confirmed_stop(
        &fixture.state,
        &fixture.storage,
        &fixture.details.summary.id,
        &active,
    )
    .await
    .unwrap();
    assert_eq!(stopped[0].exit_code, Some(0));
}

#[tokio::test]
async fn dst_lifecycle_stop_preserves_inspection_failure_without_exact_owned_handle() {
    let mut fixture = Fixture::new("normal").await;
    fixture.ready().await.unwrap();
    let active = fixture.register().await;
    let original = &active.processes[0];
    for variant in ["run", "key", "pid", "untracked"] {
        let mut process = original.clone();
        let instance_id = if variant == "untracked" {
            "not-tracked"
        } else {
            &fixture.details.summary.id
        };
        match variant {
            "run" => process.run_id += 1,
            "key" => process.process_key.push_str("-other"),
            "pid" => process.pid = process.pid.map(|pid| pid + 1),
            _ => {}
        }
        let checked = std::cell::Cell::new(false);
        let error =
            shutdown_process_is_running(&fixture.state, instance_id, &process, |pid, identity| {
                checked.set(true);
                assert_eq!(Some(pid), process.pid);
                assert_eq!(Some(identity), process.process_identity.as_ref());
                Err(app_runtime::RuntimeProcessError::InspectProcess {
                    pid,
                    source: std::io::Error::from_raw_os_error(5),
                })
            })
            .unwrap_err();
        assert!(checked.get(), "{variant}");
        assert!(error.contains("os error 5"), "{variant}: {error}");
    }
}

#[tokio::test]
async fn dst_lifecycle_workshop_timeout_blocks_later_ready() {
    let mut fixture = Fixture::new("workshop-timeout").await;
    let error = fixture.ready().await.unwrap_err();
    assert!(error.contains("DownloadServerMods timed out"), "{error}");
    assert!(error.contains("Master"), "{error}");
    assert!(error.contains("master.log"), "{error}");
}

#[tokio::test]
async fn dst_lifecycle_failed_mod_blocks_later_ready_with_its_error() {
    let mut fixture = Fixture::new("failed-mod").await;
    let error = fixture.ready().await.unwrap_err();
    assert!(error.contains("workshop-2039181790"), "{error}");
    assert!(error.contains("missing dependency"), "{error}");
    assert!(error.contains("Master"), "{error}");
}

#[tokio::test]
async fn dst_lifecycle_ignores_failed_mod_from_another_probe() {
    let mut fixture = Fixture::new("old-failed-mod").await;
    fixture.ready().await.unwrap();
}

#[tokio::test]
async fn dst_lifecycle_failed_readiness_preserves_the_survivors_console() {
    let mut fixture = Fixture::new("normal").await;
    fixture
        .state
        .shutdown_in_progress
        .store(true, Ordering::SeqCst);
    assert!(fixture.ready().await.is_err());
    fixture
        .state
        .shutdown_in_progress
        .store(false, Ordering::SeqCst);
    fixture.ready().await.unwrap();
    fixture.register().await;
    fixture.shutdown().await.unwrap();
}

#[tokio::test]
async fn dst_lifecycle_does_not_treat_console_echo_as_completed_save() {
    let mut fixture = Fixture::new("missing-save").await;
    fixture.ready().await.unwrap();
    fixture.register().await;
    let error = fixture.shutdown().await.unwrap_err();
    assert!(
        error.contains("exited without confirming its save"),
        "{error}"
    );
}

#[tokio::test]
async fn dst_lifecycle_retains_abnormal_exit_after_save_ack() {
    let mut fixture = Fixture::new("abnormal-exit").await;
    fixture.ready().await.unwrap();
    let active = fixture.register().await;
    fixture.shutdown().await.unwrap();
    let error = finish_confirmed_stop(
        &fixture.state,
        &fixture.storage,
        &fixture.details.summary.id,
        &active,
    )
    .await
    .unwrap_err();
    assert!(error.contains("did not exit normally"), "{error}");
    let overview =
        read_instance_runtime_overview(&fixture.storage.paths, &fixture.details.summary.id)
            .await
            .unwrap();
    assert_eq!(overview.recent_runs[0].processes[0].exit_code, Some(9));
}

#[tokio::test]
async fn dst_lifecycle_save_timeout_keeps_the_process_alive_and_managed() {
    let mut fixture = Fixture::new("stalled-save").await;
    fixture.ready().await.unwrap();
    let active = fixture.register().await;
    let error = save_and_shutdown_with_timeout(
        &fixture.state,
        &fixture.storage,
        &fixture.details,
        Duration::from_secs(1),
    )
    .await
    .unwrap_err();
    assert!(error.contains("were not forcibly stopped"), "{error}");
    let process = &active.processes[0];
    assert!(
        process_matches_identity(
            process.pid.unwrap(),
            process.process_identity.as_ref().unwrap()
        )
        .unwrap()
    );
    let recorded = read_instance_details(&fixture.storage.paths, &fixture.details.summary.id)
        .await
        .unwrap();
    assert_eq!(recorded.summary.active_process_count, 1);
    assert_eq!(
        fixture
            .state
            .runtime_supervisor
            .lock()
            .unwrap()
            .tracked_instances()
            .len(),
        1
    );
}

#[tokio::test]
async fn dst_lifecycle_waits_for_reaped_exit_record_to_finish_synchronizing() {
    let mut fixture = Fixture::new("normal").await;
    fixture.ready().await.unwrap();
    let active = fixture.register().await;
    fixture.shutdown().await.unwrap();
    let reaped = fixture
        .state
        .runtime_supervisor
        .lock()
        .unwrap()
        .reap_exited()
        .unwrap();
    assert_eq!(reaped.len(), 1);
    let mut finish = Box::pin(finish_confirmed_stop(
        &fixture.state,
        &fixture.storage,
        &fixture.details.summary.id,
        &active,
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(100), finish.as_mut())
            .await
            .is_err()
    );
    for process in reaped {
        mark_instance_process_stopped(
            &fixture.storage.paths,
            &fixture.details.summary.id,
            process.run_id,
            process.exit_code,
            false,
        )
        .await
        .unwrap();
    }
    let stopped = tokio::time::timeout(Duration::from_secs(5), finish)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stopped[0].exit_code, Some(0));
}
