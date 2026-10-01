use super::*;
use crate::commands::tests::ProgramDataEnvGuard;
use std::collections::BTreeMap;
use std::sync::atomic::AtomicUsize;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

async fn use_command_storage(fixture: &mut Fixture) -> TestResult<ProgramDataEnvGuard> {
    let environment = ProgramDataEnvGuard::set(&fixture.root.join("programdata"));
    save_app_settings(fixture.storage.settings.clone())?;
    fixture.storage = bootstrap_storage()?;
    initialize_database(&fixture.storage.paths).await?;
    let catalog = discover_modules(&fixture.storage.paths.modules_root)?;
    fixture.descriptor.summary.steam_app_id = catalog
        .iter()
        .find(|module| module.summary.id == fixture.descriptor.summary.id)
        .unwrap()
        .summary
        .steam_app_id;
    sync_modules(
        &fixture.storage.paths,
        std::slice::from_ref(&fixture.descriptor),
    )
    .await?;
    sync_game_installs(
        &fixture.storage.paths,
        &[GameInstallSyncRecord {
            module_id: fixture.descriptor.summary.id.clone(),
            install_root: fixture.original.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some("fixture-build".into()),
            mark_verified: true,
        }],
    )
    .await?;
    Ok(environment)
}

fn snapshot(root: &Path) -> TestResult<BTreeMap<PathBuf, Option<Vec<u8>>>> {
    let mut entries = BTreeMap::new();
    let mut pending = vec![root.to_owned()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            assert!(!kind.is_symlink(), "fixture must not follow external links");
            let relative = entry.path().strip_prefix(root)?.to_owned();
            if kind.is_dir() {
                entries.insert(relative, None);
                pending.push(entry.path());
            } else {
                assert!(kind.is_file(), "fixture must contain only regular files");
                entries.insert(relative, Some(fs::read(entry.path())?));
            }
        }
    }
    Ok(entries)
}

#[tokio::test]
async fn uninstalled_without_program_sources_rejects_creation_without_download_or_mutation()
-> TestResult {
    for (retain_data, remove_registration) in [(false, false), (true, false), (false, true)] {
        let mut fixture = Fixture::new().await?;
        let _environment = use_command_storage(&mut fixture).await?;
        let app = tauri::test::mock_builder()
            .manage(DesktopState::default())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
        let config = fixture
            .original
            .join("Astro/Saved/Config/WindowsServer/Engine.ini");
        if retain_data {
            fs::create_dir_all(config.parent().unwrap())?;
            fs::write(&config, b"operator settings must survive")?;
        }
        let removed = crate::commands::uninstall_module_game(
            app.handle().clone(),
            fixture.descriptor.summary.id.clone(),
        )
        .await?;
        assert_eq!(removed.install_state, InstallState::NotInstalled);
        assert!(!fixture.executable().exists());
        if retain_data {
            assert_eq!(fs::read(&config)?, b"operator settings must survive");
        }
        let previous =
            app_storage::read_program_install_owner(&fixture.storage.paths, &fixture.original)
                .await?
                .unwrap();
        assert_eq!(previous.install_state, InstallState::NotInstalled);
        assert_eq!(previous.current_version, None);
        if remove_registration {
            let pool = sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(
                    sqlx::sqlite::SqliteConnectOptions::new()
                        .filename(&fixture.storage.paths.database_path),
                )
                .await?;
            sqlx::query("DELETE FROM game_installs WHERE id = ?1")
                .bind(previous.id)
                .execute(&pool)
                .await?;
            pool.close().await;
        }
        assert!(
            app_storage::list_instance_archives(&fixture.storage.paths)
                .await?
                .archives
                .is_empty()
        );
        let games_before = snapshot(&fixture.storage.paths.games_root)?;
        let instances_before = snapshot(&fixture.storage.paths.instances_root)?;
        let calls = AtomicUsize::new(0);
        let guard = fixture.guard().await?;
        let (operation, job) = fixture.operation()?;
        let result =
            create_with_program_repair(fixture.request(&operation, &guard, &job), |_, _, _| {
                calls.fetch_add(1, Ordering::SeqCst);
                async { Err("unexpected installer call after explicit uninstall".into()) }
            })
            .await;
        assert!(result.is_err(), "uninstalled game has no program source");
        assert_eq!(calls.load(Ordering::SeqCst), 0, "{result:?}");
        assert_eq!(snapshot(&fixture.storage.paths.games_root)?, games_before);
        assert_eq!(
            snapshot(&fixture.storage.paths.instances_root)?,
            instances_before
        );
        assert!(!fixture.repair.exists());
        assert!(list_instances(&fixture.storage.paths).await?.is_empty());
        let current =
            app_storage::read_program_install_owner(&fixture.storage.paths, &fixture.original)
                .await?;
        if remove_registration {
            assert!(
                current.is_none(),
                "rejected creation must not register a new install"
            );
        } else {
            let current = current.unwrap();
            assert_eq!(current.id, previous.id);
            assert_eq!(current.install_state, InstallState::NotInstalled);
            assert_eq!(current.current_version, None);
        }
    }
    Ok(())
}

#[tokio::test]
async fn uninstalled_library_can_create_from_private_or_archived_program_without_download()
-> TestResult {
    for archive in [false, true] {
        let mut fixture = Fixture::new().await?;
        let _environment = use_command_storage(&mut fixture).await?;
        let app = tauri::test::mock_builder()
            .manage(DesktopState::default())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
        let existing = app_storage::create_instance_with_options(
            &fixture.storage.paths,
            &fixture.descriptor,
            fixture.input("Existing private program"),
            app_storage::InstanceCreationOptions {
                prefer_existing_install: false,
                require_clean_program: true,
                program_mode: Some(app_core::InstanceProgramMode::Independent),
                ..Default::default()
            },
        )
        .await?;
        let private = app_storage::read_instance_program_install(
            &fixture.storage.paths,
            &existing.provisioning.summary.id,
        )
        .await?
        .unwrap();
        assert_eq!(
            private.install.scope,
            app_storage::ProgramInstallScope::Instance
        );
        let removed = crate::commands::uninstall_module_game(
            app.handle().clone(),
            fixture.descriptor.summary.id.clone(),
        )
        .await?;
        assert_eq!(removed.install_state, InstallState::NotInstalled);
        assert!(!fixture.executable().exists());
        let private_before = snapshot(&private.install.install_root)?;
        let archive_before = if archive {
            app_storage::archive_instance(
                &fixture.storage.paths,
                &existing.provisioning.summary.id,
            )
            .await?;
            let archives = app_storage::list_instance_archives(&fixture.storage.paths).await?;
            assert_eq!(archives.archives.len(), 1);
            assert_eq!(archives.archives[0].program_storage, "full");
            assert!(list_instances(&fixture.storage.paths).await?.is_empty());
            Some(snapshot(&fixture.storage.paths.archives_root)?)
        } else {
            None
        };
        let calls = AtomicUsize::new(0);
        let guard = fixture.guard().await?;
        let (operation, job) = fixture.operation()?;
        let created =
            create_with_program_repair(fixture.request(&operation, &guard, &job), |_, _, _| {
                calls.fetch_add(1, Ordering::SeqCst);
                async { Err("retained local program must not invoke installer".into()) }
            })
            .await?;
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let binding =
            app_storage::read_instance_program_install(&fixture.storage.paths, &created.summary.id)
                .await?
                .unwrap();
        assert_eq!(
            binding.install.scope,
            app_storage::ProgramInstallScope::Instance
        );
        assert_eq!(
            fs::read(
                binding
                    .install
                    .install_root
                    .join(&fixture.descriptor.process.as_ref().unwrap().executable)
            )?,
            b"official fixture program"
        );
        if let Some(before) = archive_before {
            assert_eq!(snapshot(&fixture.storage.paths.archives_root)?, before);
        } else {
            assert_eq!(snapshot(&private.install.install_root)?, private_before);
        }
    }
    Ok(())
}
