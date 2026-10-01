use super::*;
use std::collections::BTreeMap;

#[path = "commands_program_acquisition_recovery_tests.rs"]
mod acquisition_recovery;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
type FileSnapshot = BTreeMap<PathBuf, Vec<u8>>;

fn snapshot(root: &Path) -> TestResult<FileSnapshot> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_owned()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            assert!(!kind.is_symlink(), "fixture must not follow external links");
            if kind.is_dir() {
                pending.push(entry.path());
            } else {
                assert!(kind.is_file());
                files.insert(
                    entry.path().strip_prefix(root)?.to_owned(),
                    fs::read(entry.path())?,
                );
            }
        }
    }
    Ok(files)
}

struct OriginalState {
    binding: app_storage::InstanceProgramInstall,
    instance_root: PathBuf,
    program_files: FileSnapshot,
    instance_files: FileSnapshot,
}

impl OriginalState {
    async fn new(fixture: &Fixture) -> TestResult<Self> {
        let created = app_storage::create_instance_with_options(
            &fixture.storage.paths,
            &fixture.descriptor,
            fixture.input("Existing server"),
            app_storage::InstanceCreationOptions {
                prefer_existing_install: true,
                require_clean_program: true,
                program_mode: Some(app_core::InstanceProgramMode::Independent),
                ..Default::default()
            },
        )
        .await?;
        let binding = app_storage::read_instance_program_install(
            &fixture.storage.paths,
            &created.provisioning.summary.id,
        )
        .await?
        .unwrap();
        assert_eq!(
            binding.install.scope,
            app_storage::ProgramInstallScope::Library
        );
        assert_eq!(
            fs::canonicalize(&binding.install.install_root)?,
            fs::canonicalize(&fixture.original)?
        );
        fs::write(fixture.executable(), b"operator modified program")?;
        for relative in ["custom.cfg", "Saved/world.sav", "mods/plugin.dll"] {
            let path = fixture.original.join(relative);
            fs::create_dir_all(path.parent().unwrap())?;
            fs::write(path, b"original server data")?;
        }
        let instance_root = fixture
            .storage
            .paths
            .instances_root
            .join(&binding.instance_id);
        Ok(Self {
            program_files: snapshot(&fixture.original)?,
            instance_files: snapshot(&instance_root)?,
            instance_root,
            binding,
        })
    }

    async fn assert_unchanged(&self, fixture: &Fixture) -> TestResult {
        let retained = app_storage::read_instance_program_install(
            &fixture.storage.paths,
            &self.binding.instance_id,
        )
        .await?
        .unwrap();
        assert_eq!(retained.install.id, self.binding.install.id);
        assert_eq!(
            retained.install.install_root,
            self.binding.install.install_root
        );
        assert_eq!(retained.install.scope, self.binding.install.scope);
        assert_eq!(snapshot(&fixture.original)?, self.program_files);
        assert_eq!(snapshot(&self.instance_root)?, self.instance_files);
        Ok(())
    }
}

async fn execute_fixture_sql(fixture: &Fixture, statement: &'static str) -> TestResult {
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new().filename(&fixture.storage.paths.database_path),
    )
    .await?;
    let result = sqlx::query(statement).execute(&pool).await;
    pool.close().await;
    result?;
    Ok(())
}

async fn leave_completed_repair_library(
    fixture: &Fixture,
) -> TestResult<app_storage::ProgramInstallRecord> {
    execute_fixture_sql(
        fixture,
        "CREATE TRIGGER reject_fixture_creation BEFORE INSERT ON instances
         BEGIN SELECT RAISE(ABORT, 'fixture creation publication rejected'); END",
    )
    .await?;
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let result = {
        let guard = fixture.guard().await?;
        let (operation, job) = fixture.operation()?;
        create_with_program_repair(
            fixture.request(&operation, &guard, &job),
            |module, root, token| {
                calls.fetch_add(1, Ordering::SeqCst);
                install_fixture(module, root, token)
            },
        )
        .await
    };
    execute_fixture_sql(fixture, "DROP TRIGGER reject_fixture_creation").await?;
    let error = result.expect_err("fixture must fail after official repair finishes");
    assert!(
        error.contains("fixture creation publication rejected"),
        "{error}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(list_instances(&fixture.storage.paths).await?.len(), 1);
    let completed =
        app_storage::read_program_install_owner(&fixture.storage.paths, &fixture.repair)
            .await?
            .unwrap();
    assert_eq!(completed.install_state, InstallState::Installed);
    assert_eq!(completed.scope, app_storage::ProgramInstallScope::Library);
    assert_eq!(completed.owner_instance_id, None);
    assert!(!app_storage::is_instance_program_acquisition(
        &fixture.storage.paths,
        &fixture.descriptor.summary.id,
        &fixture.repair,
    )?);
    assert!(app_storage::library_program_is_pristine(
        &fixture.repair,
        &fixture.descriptor,
        None,
    )?);
    assert!(!app_storage::library_program_acquisition_is_trusted(
        &fixture.repair,
        &fixture.descriptor,
    )?);
    Ok(completed)
}

#[tokio::test]
async fn completed_repair_survives_failed_creation_and_cancelled_retry_as_a_retained_library()
-> TestResult {
    let fixture = Fixture::new().await?;
    let original = OriginalState::new(&fixture).await?;
    let completed = leave_completed_repair_library(&fixture).await?;
    let completed_files = snapshot(&fixture.repair)?;
    original.assert_unchanged(&fixture).await?;

    {
        let guard = fixture.guard().await?;
        let (operation, job) = fixture.operation()?;
        let mut request = fixture.request(&operation, &guard, &job);
        request.program_root = &fixture.repair;
        job.cancellation().cancel();
        let error = create_with_program_repair(request, |_, _, _| async {
            Err("a cancelled Ready retry must not invoke the installer".into())
        })
        .await
        .unwrap_err();
        assert_eq!(error, "installation_cancelled");
    }
    assert_eq!(snapshot(&fixture.repair)?, completed_files);
    let retained = app_storage::read_program_install_owner(&fixture.storage.paths, &fixture.repair)
        .await?
        .unwrap();
    assert_eq!(retained.id, completed.id);
    assert_eq!(retained.scope, app_storage::ProgramInstallScope::Library);
    assert_eq!(list_instances(&fixture.storage.paths).await?.len(), 1);

    let created = {
        let guard = fixture.guard().await?;
        let (operation, job) = fixture.operation()?;
        let mut request = fixture.request(&operation, &guard, &job);
        request.program_root = &fixture.repair;
        create_with_program_repair(request, |_, _, _| async {
            Err("a complete Ready source must not invoke the installer".into())
        })
        .await?
    };
    let binding =
        app_storage::read_instance_program_install(&fixture.storage.paths, &created.summary.id)
            .await?
            .unwrap();
    assert_ne!(binding.install.id, completed.id);
    assert_eq!(
        binding.install.scope,
        app_storage::ProgramInstallScope::Instance
    );
    assert_eq!(
        binding.install.owner_instance_id.as_deref(),
        Some(created.summary.id.as_str())
    );
    let library = app_storage::read_program_install_owner(&fixture.storage.paths, &fixture.repair)
        .await?
        .unwrap();
    assert_eq!(library.id, completed.id);
    assert_eq!(library.scope, app_storage::ProgramInstallScope::Library);
    assert_eq!(library.owner_instance_id, None);
    assert_eq!(snapshot(&fixture.repair)?, completed_files);
    app_storage::delete_instance(&fixture.storage.paths, &created.summary.id).await?;
    assert!(!binding.install.install_root.exists());
    assert!(
        app_storage::read_program_install_owner(
            &fixture.storage.paths,
            &binding.install.install_root
        )
        .await?
        .is_none()
    );
    assert_eq!(list_instances(&fixture.storage.paths).await?.len(), 1);
    assert_eq!(snapshot(&fixture.repair)?, completed_files);
    original.assert_unchanged(&fixture).await
}

#[tokio::test]
async fn incomplete_registration_with_intact_files_recovers_to_a_retained_library_without_downloading()
-> TestResult {
    let fixture = Fixture::new().await?;
    let original_files = snapshot(&fixture.original)?;
    sync_game_installs(
        &fixture.storage.paths,
        &[GameInstallSyncRecord {
            module_id: fixture.descriptor.summary.id.clone(),
            install_root: fixture.original.to_string_lossy().into_owned(),
            install_state: InstallState::Incomplete,
            current_version: Some("fixture-build".into()),
            mark_verified: false,
        }],
    )
    .await?;
    let created = {
        let guard = fixture.guard().await?;
        let (operation, job) = fixture.operation()?;
        create_with_program_repair(fixture.request(&operation, &guard, &job), |_, _, _| async {
            Err("an intact allowlist must recover without downloading".into())
        })
        .await?
    };
    let library = app_storage::read_program_install_owner(&fixture.storage.paths, &fixture.repair)
        .await?
        .unwrap();
    assert_eq!(library.scope, app_storage::ProgramInstallScope::Library);
    assert_eq!(library.owner_instance_id, None);
    assert_eq!(library.install_state, InstallState::Installed);
    let binding =
        app_storage::read_instance_program_install(&fixture.storage.paths, &created.summary.id)
            .await?
            .unwrap();
    assert_eq!(
        binding.install.scope,
        app_storage::ProgramInstallScope::Instance
    );
    assert_eq!(
        binding.install.owner_instance_id.as_deref(),
        Some(created.summary.id.as_str())
    );
    assert_ne!(binding.install.id, library.id);
    assert_ne!(
        fs::canonicalize(&binding.install.install_root)?,
        fs::canonicalize(&fixture.repair)?
    );
    assert_eq!(snapshot(&fixture.original)?, original_files);
    let original =
        app_storage::read_program_install_owner(&fixture.storage.paths, &fixture.original)
            .await?
            .unwrap();
    assert_eq!(original.install_state, InstallState::Incomplete);
    assert!(app_storage::library_program_is_pristine(
        &fixture.repair,
        &fixture.descriptor,
        None
    )?);
    Ok(())
}

#[tokio::test]
async fn repaired_private_program_follows_archive_restore_and_permanent_deletion() -> TestResult {
    let fixture = Fixture::new().await?;
    let original = OriginalState::new(&fixture).await?;
    let created = {
        let guard = fixture.guard().await?;
        let (operation, job) = fixture.operation()?;
        create_with_program_repair(fixture.request(&operation, &guard, &job), install_fixture)
            .await?
    };
    let id = &created.summary.id;
    let root = fs::canonicalize(fixture.storage.paths.instances_root.join(id))?;
    let binding = app_storage::read_instance_program_install(&fixture.storage.paths, id)
        .await?
        .unwrap();
    assert_eq!(
        binding.install.scope,
        app_storage::ProgramInstallScope::Instance
    );
    let executable = fs::canonicalize(
        binding
            .install
            .install_root
            .join(&fixture.descriptor.process.as_ref().unwrap().executable),
    )?;
    let relative_executable = executable.strip_prefix(&root)?.to_owned();
    let details = app_storage::read_instance_details(&fixture.storage.paths, id).await?;
    let world = PathBuf::from(&details.saves_path).join("ownership-world.fixture");
    fs::create_dir_all(world.parent().unwrap())?;
    fs::write(&world, b"repaired server world")?;
    let relative_world = fs::canonicalize(&world)?.strip_prefix(&root)?.to_owned();
    let config = fs::read(&created.config_file_path)?;
    let archived = app_storage::archive_instance(&fixture.storage.paths, id).await?;
    let archived_root = PathBuf::from(archived.archived_instance_root.as_ref().unwrap());
    assert!(!root.exists());
    assert!(fixture.repair.is_dir());
    assert!(!archived_root.join(&relative_executable).exists());
    assert_eq!(
        fs::read(archived_root.join(&relative_world))?,
        b"repaired server world"
    );
    assert!(
        app_storage::read_instance_program_install(&fixture.storage.paths, id)
            .await?
            .is_none()
    );
    assert!(
        app_storage::read_program_install_owner(
            &fixture.storage.paths,
            &binding.install.install_root
        )
        .await?
        .is_none()
    );
    let archives = app_storage::list_instance_archives(&fixture.storage.paths).await?;
    assert_eq!(archives.archives.len(), 1);
    assert!(
        archives.archives[0].can_restore,
        "{:?}",
        archives.archives[0].issues
    );
    assert_eq!(archives.archives[0].program_storage, "reconstructable");
    assert!(archives.archives[0].omitted_program_files > 0);
    assert!(
        app_storage::ensure_program_archive_dependencies(&fixture.storage.paths, &fixture.repair)
            .await
            .is_err()
    );
    original.assert_unchanged(&fixture).await?;

    let restored =
        app_storage::restore_instance_archive(&fixture.storage.paths, &archived.archive_id).await?;
    assert_eq!(&restored.instance_id, id);
    assert!(!archived_root.exists());
    assert_eq!(fs::read(&executable)?, b"official fixture program");
    assert_eq!(fs::read(&world)?, b"repaired server world");
    assert_eq!(fs::read(&created.config_file_path)?, config);
    let restored_binding = app_storage::read_instance_program_install(&fixture.storage.paths, id)
        .await?
        .unwrap();
    assert_eq!(restored_binding.install.id, binding.install.id);
    assert_eq!(
        restored_binding.install.install_root,
        binding.install.install_root
    );
    assert_eq!(
        restored_binding.install.scope,
        app_storage::ProgramInstallScope::Instance
    );
    assert_eq!(
        restored_binding.install.owner_instance_id.as_deref(),
        Some(id.as_str())
    );
    original.assert_unchanged(&fixture).await?;

    app_storage::delete_instance(&fixture.storage.paths, id).await?;
    assert!(!root.exists());
    assert!(!binding.install.install_root.exists());
    assert!(fixture.repair.is_dir());
    assert!(
        app_storage::read_program_install_owner(
            &fixture.storage.paths,
            &binding.install.install_root
        )
        .await?
        .is_none()
    );
    let archives = app_storage::list_instance_archives(&fixture.storage.paths).await?;
    assert!(archives.archives.is_empty());
    assert!(archives.pending_deletions.is_empty());
    assert!(app_storage::library_program_is_pristine(
        &fixture.repair,
        &fixture.descriptor,
        None
    )?);
    assert_eq!(list_instances(&fixture.storage.paths).await?.len(), 1);
    original.assert_unchanged(&fixture).await
}
