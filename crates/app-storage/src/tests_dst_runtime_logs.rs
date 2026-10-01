use super::*;

struct DstLogFixture {
    root: PathBuf,
    paths: StoragePaths,
    instance: InstanceDetails,
    native_log: PathBuf,
}

impl DstLogFixture {
    fn logs_dir(&self) -> PathBuf {
        Path::new(&self.instance.config_file_path)
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("logs")
    }

    fn wrapper(&self, stamp: u128, text: &str) -> PathBuf {
        let logs = self.logs_dir();
        fs::create_dir_all(&logs).unwrap();
        let path = logs.join(format!("run-{stamp}-master.log"));
        fs::write(&path, text).unwrap();
        path
    }

    async fn new() -> Self {
        let root = unique_test_root();
        let paths = test_paths(&root);
        let mut descriptor = test_descriptor(&root);
        descriptor.manifest_toml =
            fs::read_to_string(repo_root().join("modules/dontstarve/module.toml")).unwrap();
        prepare_environment(&root, &descriptor);
        let descriptors = app_modules::discover_modules(&paths.modules_root).unwrap();
        let actual = descriptors
            .iter()
            .find(|module| module.summary.id == "dontstarve")
            .unwrap();
        let args = &actual.process.as_ref().unwrap().args_template;
        for (flag, value) in [
            ("-persistent_storage_root", "{{paths.config_dir}}"),
            ("-conf_dir", "clusters"),
            ("-cluster", "main"),
            ("-shard", "Master"),
        ] {
            assert!(
                args.windows(2)
                    .any(|pair| pair[0] == flag && pair[1] == value),
                "DST native log location must follow the actual module arguments: {flag}"
            );
        }
        initialize_database(&paths).await.unwrap();
        sync_modules(&paths, &descriptors).await.unwrap();
        let created = create_instance(
            &paths,
            &descriptor,
            CreateInstanceInput {
                name: String::from("DST first startup evidence"),
                module_id: String::from("dontstarve"),
            },
        )
        .await
        .unwrap();
        let instance = read_instance_details(&paths, &created.summary.id)
            .await
            .unwrap();
        let native_log = Path::new(&instance.config_file_path)
            .parent()
            .unwrap()
            .join("clusters/main/Master/server_log.txt");
        fs::create_dir_all(native_log.parent().unwrap()).unwrap();
        Self {
            root,
            paths,
            instance,
            native_log,
        }
    }
}

impl Drop for DstLogFixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("remove isolated DST log fixture");
    }
}

#[tokio::test]
async fn dst_native_log_fallback_reads_first_failure_without_a_recorded_run() {
    let fixture = DstLogFixture::new().await;
    let failure = "[00:00:01]: ERROR: Failed to load modoverrides.lua";
    fs::write(&fixture.native_log, format!("Starting server\n{failure}\n")).unwrap();

    let log = read_instance_log_document(&fixture.paths, &fixture.instance.summary.id, 1, None)
        .await
        .unwrap();
    assert_eq!(
        log.source_path,
        Some(fixture.native_log.to_string_lossy().into_owned())
    );
    assert_eq!(log.lines, vec![failure]);
    assert!(log.truncated);
    assert!(log.read_error.is_none());

    let overview = read_instance_runtime_overview(&fixture.paths, &fixture.instance.summary.id)
        .await
        .unwrap();
    assert!(
        overview.recent_runs.is_empty(),
        "a failed first launch has no registered run"
    );
    assert!(overview.log_tail.lines.iter().any(|line| line == failure));
    assert_ne!(
        overview.health.status, "ready",
        "native diagnostic text cannot prove an active run"
    );
    assert!(
        read_active_instance_run(&fixture.paths, &fixture.instance.summary.id)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn dst_native_log_fallback_never_replaces_an_active_wrapper_log() {
    let fixture = DstLogFixture::new().await;
    let wrapper = fixture.root.join("current-run.log");
    fs::write(&wrapper, "Starting current run\n").unwrap();
    fs::write(&fixture.native_log, "Server ready\n").unwrap();
    fixture.wrapper(9_999, "Another attempt must not replace active evidence\n");
    let earlier = UNIX_EPOCH + Duration::from_secs(1_000);
    fs::File::options()
        .write(true)
        .open(&wrapper)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(earlier))
        .unwrap();
    fs::File::options()
        .write(true)
        .open(&fixture.native_log)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(earlier + Duration::from_secs(1)))
        .unwrap();
    record_started_test_instance(
        &fixture.paths,
        &fixture.instance.summary.id,
        7001,
        &wrapper.to_string_lossy(),
    )
    .await
    .unwrap();

    let log = read_instance_log_document(&fixture.paths, &fixture.instance.summary.id, 10, None)
        .await
        .unwrap();
    assert_eq!(
        log.source_path,
        Some(wrapper.to_string_lossy().into_owned())
    );
    assert_eq!(log.lines, vec!["Starting current run"]);
    let overview = read_instance_runtime_overview(&fixture.paths, &fixture.instance.summary.id)
        .await
        .unwrap();
    assert_eq!(overview.health.status, "starting");
    assert_eq!(overview.log_tail.source_path, log.source_path);

    fs::write(&wrapper, "").unwrap();
    let empty = read_instance_log_document(&fixture.paths, &fixture.instance.summary.id, 10, None)
        .await
        .unwrap();
    assert_eq!(empty.source_path, log.source_path);
    assert!(empty.lines.is_empty());
    fs::remove_file(&wrapper).unwrap();
    let unreadable =
        read_instance_log_document(&fixture.paths, &fixture.instance.summary.id, 10, None)
            .await
            .unwrap();
    assert_eq!(unreadable.source_path, log.source_path);
    assert!(
        unreadable.read_error.is_some(),
        "missing current output is an evidence gap, not old readiness"
    );
}

#[tokio::test]
async fn dst_native_log_fallback_requires_an_existing_file() {
    let fixture = DstLogFixture::new().await;
    let log = read_instance_log_document(&fixture.paths, &fixture.instance.summary.id, 10, None)
        .await
        .unwrap();
    assert!(log.source_path.is_none());
    assert!(log.lines.is_empty());
    assert!(log.read_error.is_none());
}

#[tokio::test]
async fn dst_failed_start_wrapper_is_read_without_native_log_or_registered_run() {
    let fixture = DstLogFixture::new().await;
    let failure = "[LanGame startup] ERROR: Failed to load ../worldgenoverride.lua";
    let wrapper = fixture.wrapper(1_789_818_982_062, &format!("Starting\n{failure}\n"));
    assert!(!fixture.native_log.exists());
    let log = read_instance_log_document(&fixture.paths, &fixture.instance.summary.id, 1, None)
        .await
        .unwrap();
    assert_eq!(
        log.source_path,
        Some(wrapper.to_string_lossy().into_owned())
    );
    assert_eq!(log.lines, vec![failure]);
    assert!(log.truncated);
    let overview = read_instance_runtime_overview(&fixture.paths, &fixture.instance.summary.id)
        .await
        .unwrap();
    assert!(overview.recent_runs.is_empty());
    assert_eq!(overview.log_tail.source_path, log.source_path);
    assert!(overview.log_tail.lines.iter().any(|line| line == failure));
    assert_ne!(overview.health.status, "ready");
}

#[tokio::test]
async fn dst_latest_failed_wrapper_supersedes_prior_run_but_preserves_explicit_history() {
    let fixture = DstLogFixture::new().await;
    let old_wrapper = fixture.wrapper(100, "Previous server ready\n");
    let previous = record_started_test_instance(
        &fixture.paths,
        &fixture.instance.summary.id,
        7002,
        &old_wrapper.to_string_lossy(),
    )
    .await
    .unwrap();
    mark_instance_process_stopped(
        &fixture.paths,
        &fixture.instance.summary.id,
        previous.run_id,
        Some(0),
        false,
    )
    .await
    .unwrap();
    let failed_wrapper = fixture.wrapper(200, "New startup failed\n");
    // Copying or touching an older log must not change the launch sequence.
    fs::File::options()
        .write(true)
        .open(&failed_wrapper)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(1)))
        .unwrap();
    fs::write(
        &fixture.native_log,
        "Stale native readiness touched more recently\n",
    )
    .unwrap();
    let latest = read_instance_log_document(&fixture.paths, &fixture.instance.summary.id, 10, None)
        .await
        .unwrap();
    assert_eq!(
        latest.source_path,
        Some(failed_wrapper.to_string_lossy().into_owned())
    );
    assert_eq!(latest.lines, vec!["New startup failed"]);
    let overview = read_instance_runtime_overview(&fixture.paths, &fixture.instance.summary.id)
        .await
        .unwrap();
    assert_eq!(overview.log_tail.source_path, latest.source_path);
    let historical = read_instance_log_document(
        &fixture.paths,
        &fixture.instance.summary.id,
        10,
        Some(previous.run_id),
    )
    .await
    .unwrap();
    assert_eq!(
        historical.source_path,
        Some(old_wrapper.to_string_lossy().into_owned())
    );
    assert_eq!(historical.lines, vec!["Previous server ready"]);
}

#[tokio::test]
async fn dst_wrapper_discovery_ignores_fake_names_nonfiles_and_nested_logs() {
    let fixture = DstLogFixture::new().await;
    let valid = fixture.wrapper(20, "Valid master attempt\n");
    let logs = fixture.logs_dir();
    for name in [
        "run-999-caves.log",
        "run-999-master.log.bak",
        "run--master.log",
        "run-+999-master.log",
        "run-0999-master.log",
        "run-99x-master.log",
        "run-340282366920938463463374607431768211456-master.log",
    ] {
        fs::write(logs.join(name), "Not a managed Master attempt\n").unwrap();
    }
    let nested = logs.join("run-999-master.log");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("run-1000-master.log"), "Nested\n").unwrap();
    let log = read_instance_log_document(&fixture.paths, &fixture.instance.summary.id, 10, None)
        .await
        .unwrap();
    assert_eq!(log.source_path, Some(valid.to_string_lossy().into_owned()));
    assert_eq!(log.lines, vec!["Valid master attempt"]);
}

#[tokio::test]
async fn dst_wrapper_discovery_reports_exhausted_directory_budget() {
    let fixture = DstLogFixture::new().await;
    fixture.wrapper(1, "Not safe to label this the latest partial result\n");
    for index in 0..4096 {
        fs::write(fixture.logs_dir().join(format!("unrelated-{index}")), "").unwrap();
    }
    let error = read_instance_log_document(&fixture.paths, &fixture.instance.summary.id, 10, None)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("4096"));
}

#[cfg(windows)]
#[tokio::test]
async fn dst_wrapper_discovery_rejects_a_reparse_point_log_directory() {
    let fixture = DstLogFixture::new().await;
    let outside = fixture.root.join("outside-logs");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("run-999-master.log"), "Outside content\n").unwrap();
    let logs = fixture.logs_dir();
    if logs.exists() {
        fs::remove_dir(&logs).expect("new fixture log directory is empty");
    }
    let output = Command::new("cmd")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&logs)
        .arg(&outside)
        .output()
        .unwrap();
    assert!(output.status.success(), "create isolated log junction");
    let result =
        read_instance_log_document(&fixture.paths, &fixture.instance.summary.id, 10, None).await;
    fs::remove_dir(&logs).expect("remove the test junction without deleting its target");
    assert!(result.unwrap_err().to_string().contains("reparse points"));
    assert!(outside.join("run-999-master.log").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn dst_wrapper_discovery_ignores_symbolic_link_files() {
    let fixture = DstLogFixture::new().await;
    let valid = fixture.wrapper(20, "Valid master attempt\n");
    let outside = fixture.root.join("outside.log");
    fs::write(&outside, "Outside content\n").unwrap();
    std::os::unix::fs::symlink(&outside, fixture.logs_dir().join("run-999-master.log")).unwrap();
    let log = read_instance_log_document(&fixture.paths, &fixture.instance.summary.id, 10, None)
        .await
        .unwrap();
    assert_eq!(log.source_path, Some(valid.to_string_lossy().into_owned()));
    assert_eq!(log.lines, vec!["Valid master attempt"]);
}
