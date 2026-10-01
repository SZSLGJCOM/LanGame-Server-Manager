use super::*;
use crate::commands::commands_storage_management::{
    InstanceArchiveInput, restore_instance_archive,
};
use std::collections::BTreeMap;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct Fixture {
    _environment: ProgramDataEnvGuard,
    storage: StorageBootstrap,
    descriptor: ModuleDescriptor,
    base: PathBuf,
    app: tauri::App<tauri::test::MockRuntime>,
}

impl Fixture {
    async fn new(module: &str) -> TestResult<Self> {
        let root = temp_test_dir("library-cleanup");
        let environment = ProgramDataEnvGuard::set(&root.join("programdata"));
        let (storage, mut descriptor) = installed_astroneer_fixture(&root).await?;
        if module != "astroneer" {
            let descriptors = discover_modules(&storage.paths.modules_root)?;
            descriptor = find_descriptor(&descriptors, module)?.clone();
        }
        let base = seed_library(
            &storage,
            &descriptor,
            &storage.paths.games_root.join(module),
        )
        .await?;
        let app = tauri::test::mock_builder()
            .manage(DesktopState::default())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
        Ok(Self {
            _environment: environment,
            storage,
            descriptor,
            base,
            app,
        })
    }

    async fn extra(&self) -> TestResult<PathBuf> {
        seed_library(
            &self.storage,
            &self.descriptor,
            &self.storage.paths.games_root.join(format!(
                "{}-original-{}",
                self.descriptor.summary.id,
                uuid::Uuid::new_v4()
            )),
        )
        .await
    }

    async fn create(
        &self,
        source: &Path,
        mode: app_core::InstanceProgramMode,
    ) -> TestResult<app_storage::CreateInstanceResult> {
        Ok(app_storage::create_instance_with_options(
            &self.storage.paths,
            &self.descriptor,
            CreateInstanceInput {
                name: "Library cleanup fixture".into(),
                module_id: self.descriptor.summary.id.clone(),
            },
            app_storage::InstanceCreationOptions {
                program_install_root: Some(source.to_owned()),
                program_mode: Some(mode),
                require_clean_program: true,
                ..Default::default()
            },
        )
        .await?)
    }

    async fn delete(&self, id: &str) -> TestResult<app_core::InstanceDeletionResult> {
        Ok(delete_instance_record(self.app.handle().clone(), id.to_owned()).await?)
    }

    async fn uninstall(&self) -> TestResult<app_core::ModuleUninstallResult> {
        Ok(uninstall_module_game(
            self.app.handle().clone(),
            self.descriptor.summary.id.clone(),
        )
        .await?)
    }
}

async fn seed_library(
    storage: &StorageBootstrap,
    descriptor: &ModuleDescriptor,
    root: &Path,
) -> TestResult<PathBuf> {
    fs::create_dir_all(root)?;
    if descriptor.summary.id == "minecraft" {
        fs::write(root.join("server.jar"), b"inert official fixture jar")?;
        fs::create_dir_all(root.join("jre/bin"))?;
        fs::write(
            root.join("jre/bin/java.exe"),
            b"inert official fixture runtime",
        )?;
    } else {
        fs::write(
            root.join("AstroServer.exe"),
            b"inert official fixture server",
        )?;
        fs::write(root.join("original.dll"), b"official library bytes")?;
    }
    app_storage::record_library_program_baseline(root, descriptor, true, None)?;
    sync_game_installs(
        &storage.paths,
        &[GameInstallSyncRecord {
            module_id: descriptor.summary.id.clone(),
            install_root: root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some("cleanup-fixture-build".into()),
            mark_verified: true,
        }],
    )
    .await?;
    Ok(root.canonicalize()?)
}

fn files(root: &Path) -> TestResult<BTreeMap<PathBuf, Vec<u8>>> {
    fn collect(root: &Path, path: &Path, result: &mut BTreeMap<PathBuf, Vec<u8>>) -> TestResult {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            assert!(
                !kind.is_symlink(),
                "fixture must not traverse external links"
            );
            if kind.is_dir() {
                collect(root, &entry.path(), result)?;
            } else {
                result.insert(
                    entry.path().strip_prefix(root)?.to_owned(),
                    fs::read(entry.path())?,
                );
            }
        }
        Ok(())
    }
    let mut result = BTreeMap::new();
    collect(root, root, &mut result)?;
    Ok(result)
}

fn reports_root(roots: &[String], root: &Path) -> bool {
    let expected = normalized(root);
    roots
        .iter()
        .any(|candidate| normalized(Path::new(candidate)) == expected)
}

fn normalized(path: &Path) -> PathBuf {
    app_storage::program_removal_files::normalize_path(path)
        .expect("fixture paths must normalize even after a managed directory is removed")
}

#[tokio::test(flavor = "current_thread")]
async fn library_cleanup_last_delete_keeps_one_base_and_preserves_personal_bytes() -> TestResult {
    let _guard = command_smoke_lock().lock().await;
    let fixture = Fixture::new("astroneer").await?;
    let extra = fixture.extra().await?;
    let changed = fixture.extra().await?;
    fs::write(changed.join("original.dll"), b"operator modified library")?;
    fs::create_dir_all(changed.join("Mods"))?;
    fs::write(changed.join("Mods/custom.dll"), b"operator mod")?;
    fs::create_dir_all(changed.join("Astro/Saved/SaveGames"))?;
    fs::write(
        changed.join("Astro/Saved/SaveGames/world.sav"),
        b"player progress",
    )?;
    fs::write(changed.join(".langame-user-notes"), b"personal notes")?;
    let protected = [
        "original.dll",
        "Mods/custom.dll",
        "Astro/Saved/SaveGames/world.sav",
        ".langame-user-notes",
    ]
    .into_iter()
    .map(|relative| {
        let path = changed.join(relative);
        Ok((path.clone(), fs::read(path)?))
    })
    .collect::<TestResult<Vec<_>>>()?;
    let first = fixture
        .create(&fixture.base, app_core::InstanceProgramMode::Independent)
        .await?;
    let second = fixture
        .create(&fixture.base, app_core::InstanceProgramMode::Independent)
        .await?;
    let base_before = files(&fixture.base)?;
    let second_before = files(&second.effective_install_root)?;

    let first_deleted = fixture.delete(&first.provisioning.summary.id).await?;
    assert!(
        first_deleted
            .program_cleanup
            .removed_install_roots
            .is_empty()
    );
    assert!(extra.join("AstroServer.exe").is_file());
    assert!(changed.join("AstroServer.exe").is_file());
    assert_eq!(files(&second.effective_install_root)?, second_before);
    assert_eq!(list_instances(&fixture.storage.paths).await?.len(), 1);

    let last = fixture.delete(&second.provisioning.summary.id).await?;
    assert!(list_instances(&fixture.storage.paths).await?.is_empty());
    assert!(
        app_storage::list_instance_archives(&fixture.storage.paths)
            .await?
            .archives
            .is_empty()
    );
    assert_eq!(files(&fixture.base)?, base_before);
    assert!(
        !extra.exists(),
        "unused original package should be reclaimed"
    );
    assert!(!changed.join("AstroServer.exe").exists());
    assert!(reports_root(
        &last.program_cleanup.removed_install_roots,
        &extra
    ));
    assert!(reports_root(
        &last.program_cleanup.removed_install_roots,
        &changed
    ));
    assert!(last.program_cleanup.retained_installs.is_empty());
    for (path, bytes) in protected {
        assert_eq!(fs::read(&path)?, bytes, "{}", path.display());
        assert!(
            last.program_cleanup
                .preserved_data_paths
                .iter()
                .any(|retained| normalized(&path).starts_with(normalized(Path::new(retained))))
        );
    }
    let plan =
        app_storage::plan_module_library_cleanup(&fixture.storage.paths, &fixture.descriptor, true)
            .await?;
    let installed = plan
        .installations
        .iter()
        .filter(|record| record.install_state == InstallState::Installed)
        .collect::<Vec<_>>();
    assert_eq!(installed.len(), 1);
    assert_eq!(
        normalized(&installed[0].install_root),
        normalized(&fixture.base)
    );
    assert_eq!(plan.keep_install_id, Some(installed[0].id));
    let selected = app_storage::read_library_program_install(
        &fixture.storage.paths,
        &fixture.descriptor.summary.id,
    )
    .await?
    .expect("the retained base remains the next creation source");
    assert_eq!(
        normalized(&selected.install_root),
        normalized(&fixture.base)
    );
    let recreated = crate::commands::tests::creation_lifecycle_tests::CREATION_INSTALL_FORBIDDEN
        .scope(
            true,
            create_instance_record_inner(
                fixture.app.state::<DesktopState>(),
                CreateInstanceInput {
                    name: "Reuse after last deletion".into(),
                    module_id: fixture.descriptor.summary.id.clone(),
                },
            ),
        )
        .await?;
    let binding =
        app_storage::read_instance_program_install(&fixture.storage.paths, &recreated.summary.id)
            .await?
            .unwrap();
    assert_eq!(
        normalized(&binding.install.install_root),
        normalized(&fixture.base)
    );
    assert_eq!(
        binding.install.scope,
        app_storage::ProgramInstallScope::Library
    );
    assert_eq!(list_instances(&fixture.storage.paths).await?.len(), 1);
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn library_cleanup_uninstall_covers_all_registered_roots_preserving_private_and_unregistered()
-> TestResult {
    let _guard = command_smoke_lock().lock().await;
    let fixture = Fixture::new("astroneer").await?;
    let extra = fixture.extra().await?;
    let second_extra = fixture.extra().await?;
    fs::create_dir_all(extra.join("Astro/Saved/SaveGames"))?;
    let library_save = extra.join("Astro/Saved/SaveGames/world.sav");
    fs::write(&library_save, b"preserved library world")?;
    let unknown = fixture
        .storage
        .paths
        .games_root
        .join(format!("astroneer-original-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&unknown)?;
    fs::write(
        unknown.join("AstroServer.exe"),
        b"unregistered program owned by user",
    )?;
    let unknown_before = files(&unknown)?;
    let private = fixture
        .create(&fixture.base, app_core::InstanceProgramMode::Independent)
        .await?;
    let id = &private.provisioning.summary.id;
    let instance_root = Path::new(&private.provisioning.config_file_path)
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let before = files(instance_root)?;
    let owner_before = app_storage::read_instance_program_install(&fixture.storage.paths, id)
        .await?
        .unwrap();
    assert_eq!(
        owner_before.install.scope,
        app_storage::ProgramInstallScope::Instance
    );
    let native = fixture
        .base
        .join("Astro/Saved/Config/WindowsServer/Engine.ini");
    let native_before = fs::read(&native)?;

    let result = fixture.uninstall().await?;

    assert_eq!(result.install_state, InstallState::NotInstalled);
    assert!(!result.executable_exists);
    assert_eq!(result.cleanup.removed_install_roots.len(), 3);
    assert!(result.cleanup.retained_installs.is_empty());
    for root in [&fixture.base, &extra, &second_extra] {
        assert!(!root.join("AstroServer.exe").exists());
        assert!(reports_root(&result.cleanup.removed_install_roots, root));
    }
    assert_eq!(fs::read(&library_save)?, b"preserved library world");
    assert_eq!(fs::read(&native)?, native_before);
    assert_eq!(files(&unknown)?, unknown_before);
    assert_eq!(files(instance_root)?, before);
    let owner_after = app_storage::read_instance_program_install(&fixture.storage.paths, id)
        .await?
        .unwrap();
    assert_eq!(owner_after.install.id, owner_before.install.id);
    assert_eq!(
        normalized(&owner_after.install.install_root),
        normalized(&owner_before.install.install_root)
    );
    assert_eq!(
        owner_after.install.scope,
        app_storage::ProgramInstallScope::Instance
    );
    assert_eq!(owner_after.install.install_state, InstallState::Installed);
    assert_eq!(list_instances(&fixture.storage.paths).await?.len(), 1);
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn library_cleanup_uninstall_retains_shared_program_and_removes_unused_extra() -> TestResult {
    let _guard = command_smoke_lock().lock().await;
    let fixture = Fixture::new("minecraft").await?;
    let extra = fixture.extra().await?;
    let shared = fixture
        .create(&fixture.base, app_core::InstanceProgramMode::Shared)
        .await?;
    let id = &shared.provisioning.summary.id;
    let owner_before = app_storage::read_instance_program_install(&fixture.storage.paths, id)
        .await?
        .unwrap();
    assert_eq!(
        owner_before.install.scope,
        app_storage::ProgramInstallScope::Library
    );
    let base_before = files(&fixture.base)?;
    let config_before = fs::read(&shared.provisioning.config_file_path)?;

    let result = fixture.uninstall().await?;

    assert_eq!(result.install_state, InstallState::Installed);
    assert!(result.executable_exists);
    assert!(!extra.exists());
    assert!(reports_root(&result.cleanup.removed_install_roots, &extra));
    assert_eq!(result.cleanup.retained_installs.len(), 1);
    assert_eq!(
        normalized(Path::new(&result.cleanup.retained_installs[0].install_root)),
        normalized(&fixture.base)
    );
    assert_eq!(result.cleanup.retained_installs[0].reason, "in_use");
    assert_eq!(
        fixture
            .app
            .state::<DesktopState>()
            .app_state
            .read()
            .unwrap()
            .modules
            .iter()
            .find(|module| module.id == fixture.descriptor.summary.id)
            .unwrap()
            .install_state,
        result.install_state,
    );
    assert_eq!(files(&fixture.base)?, base_before);
    assert_eq!(
        fs::read(&shared.provisioning.config_file_path)?,
        config_before
    );
    let owner_after = app_storage::read_instance_program_install(&fixture.storage.paths, id)
        .await?
        .unwrap();
    assert_eq!(owner_after.install.id, owner_before.install.id);
    assert_eq!(list_instances(&fixture.storage.paths).await?.len(), 1);
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn library_cleanup_delete_and_uninstall_preserve_archive_dependency_and_restore() -> TestResult
{
    let _guard = command_smoke_lock().lock().await;
    let fixture = Fixture::new("minecraft").await?;
    let archived_library = fixture.extra().await?;
    let archived = fixture
        .create(&archived_library, app_core::InstanceProgramMode::Shared)
        .await?;
    let archive = archive_instance_record(
        fixture.app.handle().clone(),
        archived.provisioning.summary.id.clone(),
    )
    .await?;
    let archive_root = PathBuf::from(archive.archived_instance_root.as_ref().unwrap());
    let archive_before = files(&archive_root)?;
    let library_before = files(&archived_library)?;
    let dependency =
        app_storage::ensure_program_archive_dependencies(&fixture.storage.paths, &archived_library)
            .await
            .expect_err("fixture must really require its library to restore");
    assert!(dependency.to_string().contains(&archive.archive_id));
    let disposable = fixture
        .create(&fixture.base, app_core::InstanceProgramMode::Independent)
        .await?;

    let deleted = fixture.delete(&disposable.provisioning.summary.id).await?;
    assert!(list_instances(&fixture.storage.paths).await?.is_empty());
    assert!(
        deleted
            .program_cleanup
            .retained_installs
            .iter()
            .any(|retained| normalized(Path::new(&retained.install_root))
                == normalized(&archived_library)
                && retained.reason.contains(&archive.archive_id))
    );
    assert_eq!(files(&archived_library)?, library_before);
    assert_eq!(files(&archive_root)?, archive_before);

    let result = fixture.uninstall().await?;
    assert_eq!(result.install_state, InstallState::Installed);
    assert!(result.executable_exists);
    assert!(reports_root(
        &result.cleanup.removed_install_roots,
        &fixture.base
    ));
    assert!(!fixture.base.exists());
    assert!(
        result
            .cleanup
            .retained_installs
            .iter()
            .any(|retained| normalized(Path::new(&retained.install_root))
                == normalized(&archived_library)
                && retained.reason.contains(&archive.archive_id))
    );
    assert_eq!(files(&archived_library)?, library_before);
    assert_eq!(files(&archive_root)?, archive_before);
    let restored = restore_instance_archive(
        fixture.app.handle().clone(),
        InstanceArchiveInput {
            archive_id: archive.archive_id,
        },
    )
    .await?;
    assert_eq!(restored.instance_id, archived.provisioning.summary.id);
    let owner =
        app_storage::read_instance_program_install(&fixture.storage.paths, &restored.instance_id)
            .await?
            .unwrap();
    assert_eq!(
        normalized(&owner.install.install_root),
        normalized(&archived_library)
    );
    assert_eq!(
        owner.install.scope,
        app_storage::ProgramInstallScope::Library
    );
    assert_eq!(list_instances(&fixture.storage.paths).await?.len(), 1);
    Ok(())
}
