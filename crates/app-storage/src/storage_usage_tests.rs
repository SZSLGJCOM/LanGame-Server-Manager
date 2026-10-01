use super::*;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("langame-storage-usage-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn plan(&self, assignments: &[(&str, &str)]) -> ScanPlan {
        let mut entries = Vec::new();
        let mut owners = HashMap::new();
        for (index, (relative, category)) in assignments.iter().enumerate() {
            let path = self.0.join(relative);
            fs::create_dir_all(&path).unwrap();
            let path = fs::canonicalize(path).unwrap();
            owners.insert(path_key(&path), index);
            entries.push(StorageUsageEntry {
                id: format!("entry-{index}"),
                category: (*category).into(),
                label: (*relative).into(),
                path: path.to_string_lossy().into_owned(),
                instance_id: None,
                module_id: None,
                logical_bytes: 0,
                allocated_bytes: Some(0),
                file_count: 0,
                status: "complete".into(),
                issues: Vec::new(),
            });
        }
        ScanPlan {
            entries,
            roots: vec![fs::canonicalize(&self.0).unwrap()],
            assignments: owners,
            issues: Vec::new(),
            cancellation: Arc::new(AtomicBool::new(false)),
            deadline: Instant::now() + MAX_SCAN_DURATION,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // This UUID directory is created by this test; junction tests unlink
        // their junction before recursive cleanup of this owned fixture.
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn inspect(plan: ScanPlan) -> StorageUsageReport {
    scan(
        plan,
        "test".into(),
        timestamp(),
        Arc::new(AtomicBool::new(false)),
    )
}

#[test]
fn nested_categories_count_each_file_once_and_keep_data_separate() {
    let fixture = Fixture::new();
    let plan = fixture.plan(&[
        ("", "other"),
        ("library", "library"),
        ("instance", "instance_data"),
        ("instance/runtime", "instance_program"),
        ("instance/runtime/saves", "instance_data"),
        ("instance/backups", "backups"),
        (".trash", "archives"),
    ]);
    for (path, length) in [
        ("library/server.exe", 11),
        ("instance/config.json", 13),
        ("instance/runtime/server.exe", 17),
        ("instance/runtime/saves/world.dat", 19),
        ("instance/backups/world.zip", 23),
        (".trash/world.dat", 29),
    ] {
        fs::write(fixture.0.join(path), vec![1; length]).unwrap();
    }
    let report = inspect(plan);
    assert_eq!(report.status, "complete");
    assert_eq!(report.logical_bytes, 112);
    assert_eq!(report.file_count, 6);
    assert_eq!(
        report
            .entries
            .iter()
            .map(|entry| entry.logical_bytes)
            .collect::<Vec<_>>(),
        vec![0, 11, 13, 17, 19, 23, 29]
    );
}

#[test]
fn hard_links_count_logical_paths_but_allocate_the_file_only_once() {
    let fixture = Fixture::new();
    let plan = fixture.plan(&[("", "other")]);
    let original = fixture.0.join("original");
    fs::write(&original, vec![7; 8193]).unwrap();
    fs::hard_link(&original, fixture.0.join("linked")).unwrap();
    let usage = native::file_usage(&original, &fs::metadata(&original).unwrap()).unwrap();
    let report = inspect(plan);
    assert_eq!(report.logical_bytes, 16386);
    assert_eq!(report.file_count, 2);
    assert_eq!(report.allocated_bytes, usage.allocated);
}

#[test]
fn cancelled_scan_is_partial_and_never_claims_empty_storage() {
    let fixture = Fixture::new();
    let plan = fixture.plan(&[("", "other")]);
    fs::write(fixture.0.join("unread"), [1; 1024]).unwrap();
    let report = scan(
        plan,
        "cancelled".into(),
        timestamp(),
        Arc::new(AtomicBool::new(true)),
    );
    assert_eq!(report.status, "cancelled");
    assert_eq!(report.entries[0].status, "partial");
    assert!(!report.issues.is_empty());
    assert_eq!(report.file_count, 0);
}

#[test]
fn disappearing_root_is_reported_as_partial() {
    let fixture = Fixture::new();
    let plan = fixture.plan(&[("", "other")]);
    fs::remove_dir(&fixture.0).unwrap();
    let report = inspect(plan);
    assert_eq!(report.status, "partial");
    assert!(!report.entries[0].issues.is_empty());
    fs::create_dir(&fixture.0).unwrap();
}

#[test]
fn deadline_and_entry_limits_stop_without_claiming_complete_totals() {
    let fixture = Fixture::new();
    for expired in [false, true] {
        let mut scanner = Scanner {
            plan: fixture.plan(&[("", "other")]),
            cancellation: Arc::new(AtomicBool::new(false)),
            deadline: if expired {
                Instant::now()
            } else {
                Instant::now() + MAX_SCAN_DURATION
            },
            examined: if expired { 0 } else { MAX_ENTRIES },
            skipped_links: 0,
            stopped: false,
            identities: HashSet::new(),
        };
        scanner.walk(&fixture.0, 0, 0);
        assert!(scanner.stopped);
        assert_eq!(scanner.plan.issues.len(), 1);
        assert_eq!(scanner.plan.entries[0].file_count, 0);
    }
}

#[cfg(windows)]
#[test]
fn junction_contents_are_excluded_and_external_files_are_preserved() {
    let fixture = Fixture::new();
    let outside = Fixture::new();
    let plan = fixture.plan(&[("", "other")]);
    fs::write(outside.0.join("sentinel"), b"outside data").unwrap();
    let junction = fixture.0.join("external");
    let output = std::process::Command::new("cmd")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(&junction)
        .arg(&outside.0)
        .output()
        .unwrap();
    assert!(output.status.success(), "junction fixture: {output:?}");
    let report = inspect(plan);
    fs::remove_dir(&junction).unwrap();
    assert_eq!(report.status, "partial");
    assert_eq!(report.skipped_links, 1);
    assert_eq!(report.file_count, 0);
    assert_eq!(
        fs::read(outside.0.join("sentinel")).unwrap(),
        b"outside data"
    );
}

#[cfg(windows)]
#[test]
fn replacing_a_planned_root_ancestor_with_a_junction_does_not_scan_outside() {
    let fixture = Fixture::new();
    let outside = Fixture::new();
    let mut plan = fixture.plan(&[("", "other"), ("parent/managed", "library")]);
    plan.roots = vec![fs::canonicalize(fixture.0.join("parent/managed")).unwrap()];
    fs::create_dir_all(outside.0.join("managed")).unwrap();
    fs::write(outside.0.join("managed/sentinel"), b"external contents").unwrap();
    let parent = fixture.0.join("parent");
    let parked = fixture.0.join("parked");
    fs::rename(&parent, &parked).unwrap();
    let output = std::process::Command::new("cmd")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(&parent)
        .arg(&outside.0)
        .output()
        .unwrap();
    assert!(output.status.success(), "junction fixture: {output:?}");
    let report = inspect(plan);
    fs::remove_dir(&parent).unwrap();
    fs::rename(&parked, &parent).unwrap();
    assert_eq!(report.status, "partial");
    assert_eq!(report.file_count, 0);
    assert_eq!(
        fs::read(outside.0.join("managed/sentinel")).unwrap(),
        b"external contents"
    );
}

#[tokio::test]
async fn cancellation_and_deadline_stop_planning_before_opening_storage() {
    let fixture = Fixture::new();
    let paths = StoragePaths {
        database_path: fixture.0.join("not-created/db.sqlite"),
        ..StoragePaths::default()
    };
    for cancelled in [true, false] {
        let cancellation = Arc::new(AtomicBool::new(cancelled));
        let deadline = if cancelled {
            Instant::now() + MAX_SCAN_DURATION
        } else {
            Instant::now()
        };
        let plan = plan::build(&paths, cancellation.clone(), deadline)
            .await
            .unwrap();
        assert!(!paths.database_path.exists());
        let report = scan(plan, "preflight".into(), timestamp(), cancellation);
        assert_eq!(
            report.status,
            if cancelled { "cancelled" } else { "partial" }
        );
        assert!(!report.issues.is_empty());
    }
}

#[tokio::test]
async fn real_planner_collapses_nested_roots_and_reports_missing_optional_roots() {
    let fixture = Fixture::new();
    let paths = StoragePaths {
        app_data_root: fixture.0.join("appdata"),
        settings_path: fixture.0.join("appdata/settings.json"),
        database_path: fixture.0.join("appdata/db/store.db"),
        logs_root: fixture.0.join("appdata/logs"),
        modules_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules"),
        migrations_root: fixture.0.join("migrations"),
        steamcmd_root: fixture.0.join("steamcmd"),
        games_root: fixture.0.join("instances/library"),
        instances_root: fixture.0.join("instances"),
        archives_root: fixture.0.join("instances").join(".trash"),
    };
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    fs::create_dir_all(&paths.games_root).unwrap();
    fs::create_dir_all(paths.instances_root.join(".trash")).unwrap();
    crate::initialize_database(&paths).await.unwrap();
    fs::write(paths.games_root.join("server"), [1; 13]).unwrap();
    fs::write(paths.instances_root.join(".trash/archive"), [1; 19]).unwrap();
    let report = scan_storage_usage(&paths, "planner".into(), Arc::new(AtomicBool::new(false)))
        .await
        .unwrap();
    assert_eq!(report.status, "complete");
    let row = |id: &str| report.entries.iter().find(|entry| entry.id == id).unwrap();
    assert_eq!(row("library").logical_bytes, 13);
    assert_eq!(row("archives").logical_bytes, 19);
    assert_eq!(row("unregistered-instances").logical_bytes, 0);
    assert_eq!(row("tools").status, "missing");
}
