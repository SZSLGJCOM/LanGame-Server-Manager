use super::*;
use app_storage::{StoragePaths, bootstrap_storage_with_paths};

#[path = "commands_program_pending_revision_tests.rs"]
mod pending_revision;

#[path = "commands_program_repair_cleanup_tests.rs"]
mod cleanup;

#[path = "commands_program_repair_ownership_tests.rs"]
mod ownership;

#[path = "commands_program_seed_reuse_tests.rs"]
mod seed_reuse;

#[path = "commands_library_reinstall_tests.rs"]
mod reinstall;

#[path = "commands_program_uninstalled_tests.rs"]
mod uninstalled;

struct Fixture {
    root: PathBuf,
    storage: StorageBootstrap,
    descriptor: ModuleDescriptor,
    state: DesktopState,
    original: PathBuf,
    repair: PathBuf,
    _serial: tokio::sync::MutexGuard<'static, ()>,
}

impl Fixture {
    async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Self::for_module("astroneer").await
    }

    async fn for_module(module_id: &str) -> Result<Self, Box<dyn std::error::Error>> {
        // Lifecycle lock paths read process-wide app-data environment variables.
        // Keep those stable while another command fixture changes its roots.
        let serial = crate::commands::tests::command_smoke_lock().lock().await;
        let root = std::env::temp_dir().join(format!("lgr-{}", uuid::Uuid::new_v4().simple()));
        let manifest_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace = manifest_root.ancestors().nth(3).unwrap();
        let storage = bootstrap_storage_with_paths(StoragePaths {
            app_data_root: root.join("app-data"),
            settings_path: root.join("app-data/settings.json"),
            database_path: root.join("app-data/db/lgs.db"),
            logs_root: root.join("app-data/logs"),
            modules_root: workspace.join("modules"),
            migrations_root: workspace.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances/.trash"),
        })?;
        initialize_database(&storage.paths).await?;
        let mut descriptor = discover_modules(&storage.paths.modules_root)?
            .into_iter()
            .find(|module| module.summary.id == module_id)
            .unwrap();
        descriptor.summary.steam_app_id = None;
        let install = descriptor.install.as_mut().unwrap();
        install.source = None;
        install.download_url_windows = None;
        descriptor.storage.runtime_copy_exclusions.extend([
            "Saved".into(),
            "mods".into(),
            "custom.cfg".into(),
        ]);
        sync_modules(&storage.paths, std::slice::from_ref(&descriptor)).await?;
        let original = storage
            .paths
            .games_root
            .join(&descriptor.install.as_ref().unwrap().shared_game_dir);
        let repair = storage.paths.games_root.join(format!(
            "{}-original-{}",
            descriptor.summary.id,
            uuid::Uuid::new_v4()
        ));
        let root_text = original.to_string_lossy();
        let module =
            map_module_details_with_install_state(&storage.settings, &descriptor, Some(&root_text));
        install_fixture(
            module,
            original.clone(),
            app_steamcmd::InstallCancellation::new(),
        )
        .await?;
        app_storage::record_library_program_baseline(&original, &descriptor, true, None)?;
        sync_game_installs(
            &storage.paths,
            &[GameInstallSyncRecord {
                module_id: descriptor.summary.id.clone(),
                install_root: original.to_string_lossy().into_owned(),
                install_state: InstallState::Installed,
                current_version: Some("fixture-build".into()),
                mark_verified: true,
            }],
        )
        .await?;
        Ok(Self {
            root,
            storage,
            descriptor,
            state: DesktopState::default(),
            original,
            repair,
            _serial: serial,
        })
    }

    fn input(&self, name: &str) -> CreateInstanceInput {
        CreateInstanceInput {
            name: name.into(),
            module_id: self.descriptor.summary.id.clone(),
        }
    }

    fn operation(&self) -> Result<(StorageContextOperationGuard, InstallationJobLease), String> {
        let operation = self
            .state
            .begin_storage_context_operation("program repair test")?;
        let id = format!("create-repair-{}", uuid::Uuid::new_v4());
        self.state
            .app_state
            .write()
            .unwrap()
            .jobs
            .push(BackgroundJob {
                id: id.clone(),
                kind: JobKind::ValidateGame,
                label: "Create fixture".into(),
                status: JobStatus::Pending,
                progress_percent: 1.0,
                install_progress: None,
                cancellable: true,
                cancel_requested: false,
                target_id: Some(self.descriptor.summary.id.clone()),
                detail: None,
                output_excerpt: None,
            });
        Ok((operation, InstallationJobLease::begin(&self.state, id)?))
    }

    fn request<'a>(
        &'a self,
        operation: &'a StorageContextOperationGuard,
        guard: &'a app_steamcmd::GameInstallLifecycleGuard,
        job: &'a InstallationJobLease,
    ) -> CreationProgramRequest<'a> {
        CreationProgramRequest {
            storage: &self.storage,
            descriptor: &self.descriptor,
            operation,
            guard,
            input: self.input("Repaired server"),
            mode: Some(app_core::InstanceProgramMode::Independent),
            source: app_core::InstanceProgramSource::Verified,
            program_root: &self.original,
            repair_root: &self.repair,
            job,
        }
    }

    async fn guard(
        &self,
    ) -> Result<app_steamcmd::GameInstallLifecycleGuard, app_steamcmd::SteamCmdError> {
        app_steamcmd::acquire_game_install_lifecycle(
            &self.descriptor.summary.id,
            &[self.original.clone(), self.repair.clone()],
        )
        .await
    }

    fn executable(&self) -> PathBuf {
        self.original
            .join(&self.descriptor.process.as_ref().unwrap().executable)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

async fn install_fixture(
    module: ModuleDetails,
    root: PathBuf,
    cancellation: app_steamcmd::InstallCancellation,
) -> Result<ModuleInstallResult, String> {
    if cancellation.is_cancelled() {
        return Err("installation_cancelled".into());
    }
    let executable = root.join(&module.process.as_ref().unwrap().executable);
    let verification = root.join(
        module
            .install
            .as_ref()
            .unwrap()
            .verification_path
            .as_deref()
            .unwrap_or(&module.process.as_ref().unwrap().executable),
    );
    for path in [&executable, &verification] {
        fs::create_dir_all(path.parent().unwrap()).map_err(|error| error.to_string())?;
        fs::write(path, b"official fixture program").map_err(|error| error.to_string())?;
    }
    Ok(ModuleInstallResult {
        module_id: module.summary.id,
        steam_app_id: 0,
        operation: "validate".into(),
        install_root: root.to_string_lossy().into_owned(),
        executable_path: executable.to_string_lossy().into_owned(),
        executable_exists: true,
        install_state: InstallState::Installed,
        current_version: Some("fixture-build".into()),
        output_excerpt: "fixture provider completed".into(),
    })
}

#[tokio::test]
async fn verified_creation_reuses_valid_program_without_invoking_installer()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new().await?;
    let guard = fixture.guard().await?;
    let (operation, job) = fixture.operation()?;
    let created =
        create_with_program_repair(fixture.request(&operation, &guard, &job), |_, _, _| async {
            Err("valid package must not invoke installer".into())
        })
        .await?;
    assert_eq!(list_instances(&fixture.storage.paths).await?.len(), 1);
    assert!(!fixture.repair.exists());
    let binding =
        app_storage::read_instance_program_install(&fixture.storage.paths, &created.summary.id)
            .await?
            .unwrap();
    assert_eq!(
        fs::canonicalize(binding.install.install_root)?,
        fs::canonicalize(&fixture.original)?
    );
    Ok(())
}

#[tokio::test]
async fn verified_creation_repairs_missing_or_changed_program_without_rebinding_existing_instance()
-> Result<(), Box<dyn std::error::Error>> {
    for modified in [false, true] {
        let fixture = Fixture::new().await?;
        let guard = fixture.guard().await?;
        let owner = app_storage::create_instance_with_options(
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
        let old = app_storage::read_instance_program_install(
            &fixture.storage.paths,
            &owner.provisioning.summary.id,
        )
        .await?
        .unwrap();
        if modified {
            fs::write(fixture.executable(), b"operator modified program")?;
        } else {
            fs::remove_file(fixture.original.join(".langame-clean-package.json"))?;
            fs::remove_file(fixture.original.join(".langame-initial-package.json"))?;
        }
        let original_program = fs::read(fixture.executable())?;
        for path in [
            "custom-loader.dll",
            "custom.cfg",
            "Saved/world.sav",
            "mods/plugin.dll",
        ] {
            let target = fixture.original.join(path);
            fs::create_dir_all(target.parent().unwrap())?;
            fs::write(target, b"user data must survive")?;
        }
        let (operation, job) = fixture.operation()?;
        let called = std::sync::atomic::AtomicBool::new(false);
        let created = create_with_program_repair(
            fixture.request(&operation, &guard, &job),
            |module, root, token| {
                called.store(true, Ordering::SeqCst);
                assert_eq!(
                    fs::canonicalize(&root).unwrap(),
                    fs::canonicalize(&fixture.repair).unwrap()
                );
                install_fixture(module, root, token)
            },
        )
        .await?;
        assert!(called.load(Ordering::SeqCst));
        assert_eq!(list_instances(&fixture.storage.paths).await?.len(), 2);
        let retained = app_storage::read_instance_program_install(
            &fixture.storage.paths,
            &owner.provisioning.summary.id,
        )
        .await?
        .unwrap();
        assert_eq!(retained.install.id, old.install.id);
        assert_eq!(retained.install.install_root, old.install.install_root);
        assert_eq!(fs::read(fixture.executable())?, original_program);
        let new_root = app_storage::resolve_instance_runtime_root(
            &fixture
                .storage
                .paths
                .instances_root
                .join(&created.summary.id),
        )?;
        for path in [
            "custom-loader.dll",
            "custom.cfg",
            "Saved/world.sav",
            "mods/plugin.dll",
        ] {
            assert_eq!(
                fs::read(fixture.original.join(path))?,
                b"user data must survive"
            );
            assert!(
                !new_root.join(path).exists(),
                "copied old instance data: {path}"
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn failed_or_cancelled_repair_preserves_pending_data_and_retries_into_a_fresh_root()
-> Result<(), Box<dyn std::error::Error>> {
    for failure in ["provider", "shutdown", "user"] {
        let fixture = Fixture::new().await?;
        fs::remove_file(fixture.original.join(".langame-clean-package.json"))?;
        fs::remove_file(fixture.original.join(".langame-initial-package.json"))?;
        let original_program = fs::read(fixture.executable())?;
        let guard = fixture.guard().await?;
        let (operation, job) = fixture.operation()?;
        let cancellation = operation.cancellation_token();
        let failed = create_with_program_repair(
            fixture.request(&operation, &guard, &job),
            |module, root, token| async move {
                fs::write(root.join("partial-download.bin"), b"partial fixture")
                    .map_err(|error| error.to_string())?;
                if failure != "provider" {
                    let result = install_fixture(module, root, token.clone()).await?;
                    if failure == "shutdown" {
                        cancellation.store(true, Ordering::SeqCst);
                    } else {
                        token.cancel();
                    }
                    Ok(result)
                } else {
                    Err("fixture provider failed".into())
                }
            },
        )
        .await
        .unwrap_err();
        assert!(
            failed.contains(if failure == "provider" {
                "fixture provider failed"
            } else {
                "cancel"
            }),
            "{failed}"
        );
        assert!(list_instances(&fixture.storage.paths).await?.is_empty());
        let pending = app_storage::read_library_program_install(
            &fixture.storage.paths,
            &fixture.descriptor.summary.id,
        )
        .await?
        .unwrap();
        assert_eq!(
            fs::canonicalize(&pending.install_root)?,
            fs::canonicalize(&fixture.repair)?
        );
        assert_eq!(pending.install_state, InstallState::Incomplete);
        assert!(app_storage::library_program_acquisition_is_trusted(
            &pending.install_root,
            &fixture.descriptor
        )?);
        assert_eq!(fs::read(fixture.executable())?, original_program);
        fs::write(
            pending.install_root.join("custom-loader.dll"),
            b"added between attempts",
        )?;
        drop(job);
        drop(operation);
        drop(guard);

        let retry_root = fixture.storage.paths.games_root.join(format!(
            "{}-original-{}",
            fixture.descriptor.summary.id,
            uuid::Uuid::new_v4()
        ));
        let guard = app_steamcmd::acquire_game_install_lifecycle(
            &fixture.descriptor.summary.id,
            &[
                fixture.original.clone(),
                pending.install_root.clone(),
                retry_root.clone(),
            ],
        )
        .await?;
        let (operation, job) = fixture.operation()?;
        let mut request = fixture.request(&operation, &guard, &job);
        request.program_root = &pending.install_root;
        request.repair_root = &retry_root;
        let retried = create_with_program_repair(request, |module, root, token| {
            assert_eq!(
                fs::canonicalize(&root).unwrap(),
                fs::canonicalize(&retry_root).unwrap()
            );
            install_fixture(module, root, token)
        })
        .await?;
        assert_eq!(list_instances(&fixture.storage.paths).await?.len(), 1);
        let new_root = app_storage::resolve_instance_runtime_root(
            &fixture
                .storage
                .paths
                .instances_root
                .join(&retried.summary.id),
        )?;
        assert!(!new_root.join("custom-loader.dll").exists());
        assert!(!new_root.join("partial-download.bin").exists());
        assert_eq!(
            fs::read(pending.install_root.join("custom-loader.dll"))?,
            b"added between attempts"
        );
        assert_eq!(
            fs::read(pending.install_root.join("partial-download.bin"))?,
            b"partial fixture"
        );
        let retained =
            app_storage::read_program_install_owner(&fixture.storage.paths, &pending.install_root)
                .await?
                .unwrap();
        assert_eq!(retained.id, pending.id);
        assert_eq!(retained.install_state, InstallState::Incomplete);
        assert!(!app_storage::library_program_acquisition_is_trusted(
            &retry_root,
            &fixture.descriptor
        )?);
        let healthy = app_storage::read_program_install_owner(&fixture.storage.paths, &retry_root)
            .await?
            .unwrap();
        assert_eq!(healthy.scope, app_storage::ProgramInstallScope::Library);
        assert_eq!(healthy.owner_instance_id, None);
        assert_eq!(healthy.install_state, InstallState::Installed);
        assert_ne!(fs::canonicalize(&new_root)?, fs::canonicalize(&retry_root)?);
        assert_eq!(fs::read(fixture.executable())?, original_program);
    }
    Ok(())
}
