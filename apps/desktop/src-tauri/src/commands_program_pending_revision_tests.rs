use super::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn ownership_transfer_rejects_an_original_library_without_changing_its_files() -> TestResult {
    let fixture = Fixture::new().await?;
    let before = snapshot(&fixture.original)?;
    let error = app_storage::create_instance_with_options(
        &fixture.storage.paths,
        &fixture.descriptor,
        fixture.input("Invalid ownership transfer"),
        app_storage::InstanceCreationOptions {
            take_program_ownership: true,
            require_clean_program: true,
            program_mode: Some(app_core::InstanceProgramMode::Independent),
            program_install_root: Some(fixture.original.clone()),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("only an unused managed acquisition")
    );
    assert_eq!(snapshot(&fixture.original)?, before);
    assert!(list_instances(&fixture.storage.paths).await?.is_empty());
    let owner = app_storage::read_program_install_owner(&fixture.storage.paths, &fixture.original)
        .await?
        .unwrap();
    assert_eq!(owner.scope, app_storage::ProgramInstallScope::Library);
    assert!(owner.owner_instance_id.is_none());
    Ok(())
}

fn revision_directory(fixture: &Fixture) -> TestResult<PathBuf> {
    // Match the persisted program-revision identity, then verify the fixture
    // through the public reader before exercising desktop creation.
    let canonical = fs::canonicalize(&fixture.original)?;
    let root = canonical.to_str().ok_or("fixture root is not UTF-8")?;
    #[cfg(windows)]
    let root = root.replace('\\', "/").to_lowercase();
    let key = format!("{}\0program-root\0{root}", fixture.descriptor.summary.id);
    let hash = Sha256::digest(key.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(Path::new(&fixture.storage.settings.servers_root)
        .join(".langame")
        .join("install-revisions")
        .join(hash))
}

fn assert_pending_revision(fixture: &Fixture) {
    let error = app_steamcmd::read_program_install_revision(
        Path::new(&fixture.storage.settings.servers_root),
        &fixture.descriptor.summary.id,
        &fixture.original,
    )
    .expect_err("fixture must target this exact program's pending revision");
    assert!(
        error
            .to_string()
            .contains("the last game install did not finish")
    );
}

fn snapshot(root: &Path) -> TestResult<BTreeMap<PathBuf, Vec<u8>>> {
    fn collect(root: &Path, path: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) -> TestResult {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            assert!(!kind.is_symlink(), "fixture must not follow external links");
            if kind.is_dir() {
                collect(root, &entry.path(), files)?;
            } else {
                files.insert(
                    entry.path().strip_prefix(root)?.to_owned(),
                    fs::read(entry.path())?,
                );
            }
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    collect(root, root, &mut files)?;
    Ok(files)
}

async fn exercise_pending_revision(modified: bool) -> TestResult {
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
    let owner_id = &owner.provisioning.summary.id;
    let old_binding = app_storage::read_instance_program_install(&fixture.storage.paths, owner_id)
        .await?
        .unwrap();
    assert_eq!(
        fs::canonicalize(&old_binding.install.install_root)?,
        fs::canonicalize(&fixture.original)?
    );
    if modified {
        fs::write(
            fixture.executable(),
            b"interrupted update changed the program",
        )?;
    }
    for relative in [
        "custom-loader.dll",
        "custom.cfg",
        "Saved/world.sav",
        "mods/plugin.dll",
    ] {
        let path = fixture.original.join(relative);
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(path, b"existing user data")?;
    }
    assert_eq!(
        app_storage::library_program_is_pristine(&fixture.original, &fixture.descriptor, None)?,
        !modified,
        "fixture must distinguish intact and changed official bytes"
    );
    assert_eq!(
        app_steamcmd::read_program_install_revision(
            Path::new(&fixture.storage.settings.servers_root),
            &fixture.descriptor.summary.id,
            &fixture.original,
        )?,
        0
    );
    let revisions = revision_directory(&fixture)?;
    fs::create_dir_all(&revisions)?;
    fs::write(
        revisions.join("rev-00000000000000000001.pending"),
        b"interrupted update",
    )?;
    assert_pending_revision(&fixture);
    let original_files = snapshot(&fixture.original)?;
    let owner_root = fixture.storage.paths.instances_root.join(owner_id);
    let owner_files = snapshot(&owner_root)?;
    let revision_files = snapshot(&revisions)?;
    let (operation, job) = fixture.operation()?;
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let created = create_with_program_repair(
        fixture.request(&operation, &guard, &job),
        |module, root, token| {
            calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(
                fs::canonicalize(&root).unwrap(),
                fs::canonicalize(&fixture.repair).unwrap()
            );
            install_fixture(module, root, token)
        },
    )
    .await?;
    if modified {
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "changed bytes require official repair"
        );
    } else {
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "a complete verified seed must not invoke the provider"
        );
    }
    assert_eq!(list_instances(&fixture.storage.paths).await?.len(), 2);
    let new_binding =
        app_storage::read_instance_program_install(&fixture.storage.paths, &created.summary.id)
            .await?
            .unwrap();
    assert_ne!(new_binding.install.id, old_binding.install.id);
    assert_eq!(
        fs::canonicalize(&new_binding.install.install_root)?,
        fs::canonicalize(
            fixture
                .storage
                .paths
                .instances_root
                .join(&created.summary.id)
                .join("runtime")
        )?
    );
    assert_eq!(
        new_binding.install.scope,
        app_storage::ProgramInstallScope::Instance
    );
    assert_eq!(
        new_binding.install.owner_instance_id.as_deref(),
        Some(created.summary.id.as_str())
    );
    let healthy = app_storage::read_program_install_owner(&fixture.storage.paths, &fixture.repair)
        .await?
        .unwrap();
    assert_eq!(healthy.scope, app_storage::ProgramInstallScope::Library);
    assert_eq!(healthy.owner_instance_id, None);
    assert_eq!(healthy.install_state, InstallState::Installed);
    assert_ne!(healthy.id, new_binding.install.id);
    assert_eq!(
        fs::read(new_binding.install.install_root.join("AstroServer.exe"))?,
        b"official fixture program"
    );
    for relative in [
        "custom-loader.dll",
        "custom.cfg",
        "Saved/world.sav",
        "mods/plugin.dll",
    ] {
        assert!(
            !new_binding.install.install_root.join(relative).exists(),
            "inherited {relative}"
        );
    }
    let retained = app_storage::read_instance_program_install(&fixture.storage.paths, owner_id)
        .await?
        .unwrap();
    assert_eq!(retained.install.id, old_binding.install.id);
    assert_eq!(
        retained.install.install_root,
        old_binding.install.install_root
    );
    assert_eq!(snapshot(&fixture.original)?, original_files);
    assert_eq!(snapshot(&owner_root)?, owner_files);
    assert_eq!(snapshot(&revisions)?, revision_files);
    assert_pending_revision(&fixture);
    assert!(
        app_storage::list_instance_archives(&fixture.storage.paths)
            .await?
            .archives
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn pending_revision_with_intact_baseline_creates_from_a_fresh_root() -> TestResult {
    exercise_pending_revision(false).await
}

#[tokio::test]
async fn pending_revision_with_changed_program_uses_official_repair_without_touching_old_data()
-> TestResult {
    exercise_pending_revision(true).await
}

#[tokio::test]
async fn pending_revision_metadata_obstructed_by_a_file_does_not_invoke_repair() -> TestResult {
    let fixture = Fixture::new().await?;
    let revisions = revision_directory(&fixture)?;
    fs::create_dir_all(revisions.parent().unwrap())?;
    fs::write(&revisions, b"preserve obstructing revision metadata")?;
    let revision_error = app_steamcmd::read_program_install_revision(
        Path::new(&fixture.storage.settings.servers_root),
        &fixture.descriptor.summary.id,
        &fixture.original,
    )
    .expect_err("ordinary file must obstruct the exact program revision directory");
    assert!(
        revision_error
            .to_string()
            .contains("revision path is not a plain directory")
    );
    let original_files = snapshot(&fixture.original)?;
    let guard = fixture.guard().await?;
    let (operation, job) = fixture.operation()?;
    let called = std::sync::atomic::AtomicBool::new(false);
    let error = create_with_program_repair(fixture.request(&operation, &guard, &job), |_, _, _| {
        called.store(true, Ordering::SeqCst);
        async { Err("metadata errors must not invoke the provider".into()) }
    })
    .await
    .unwrap_err();
    assert!(
        error.contains("revision path is not a plain directory"),
        "{error}"
    );
    assert!(!called.load(Ordering::SeqCst));
    assert!(list_instances(&fixture.storage.paths).await?.is_empty());
    assert!(!fixture.repair.exists());
    assert_eq!(snapshot(&fixture.original)?, original_files);
    assert_eq!(
        fs::read(revisions)?,
        b"preserve obstructing revision metadata"
    );
    Ok(())
}

#[tokio::test]
async fn pending_revision_repair_does_not_swallow_foreign_ownership_with_missing_executable()
-> TestResult {
    let fixture = Fixture::new().await?;
    let foreign_descriptor = discover_modules(&fixture.storage.paths.modules_root)?
        .into_iter()
        .find(|module| module.summary.id == "minecraft")
        .unwrap();
    sync_modules(
        &fixture.storage.paths,
        std::slice::from_ref(&foreign_descriptor),
    )
    .await?;
    let foreign_root = fixture.storage.paths.games_root.join("foreign-owned");
    let foreign_text = foreign_root.to_string_lossy();
    let foreign_module = map_module_details_with_install_state(
        &fixture.storage.settings,
        &foreign_descriptor,
        Some(&foreign_text),
    );
    install_fixture(
        foreign_module,
        foreign_root.clone(),
        app_steamcmd::InstallCancellation::new(),
    )
    .await?;
    fs::write(
        foreign_root.join("server.properties"),
        b"foreign operator configuration",
    )?;
    sync_game_installs(
        &fixture.storage.paths,
        &[GameInstallSyncRecord {
            module_id: foreign_descriptor.summary.id.clone(),
            install_root: foreign_root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some("foreign-fixture-build".into()),
            mark_verified: true,
        }],
    )
    .await?;
    let selected = map_module_details_with_install_state(
        &fixture.storage.settings,
        &fixture.descriptor,
        Some(&foreign_text),
    );
    assert!(!foreign_root.join("AstroServer.exe").exists());
    assert_eq!(selected.summary.install_state, InstallState::Corrupted);
    let before = app_storage::read_program_install_owner(&fixture.storage.paths, &foreign_root)
        .await?
        .unwrap();
    assert_eq!(before.module_id, "minecraft");
    let foreign_files = snapshot(&foreign_root)?;
    let original_files = snapshot(&fixture.original)?;
    let guard = app_steamcmd::acquire_game_install_lifecycle(
        &fixture.descriptor.summary.id,
        &[foreign_root.clone(), fixture.repair.clone()],
    )
    .await?;
    let (operation, job) = fixture.operation()?;
    let mut request = fixture.request(&operation, &guard, &job);
    request.program_root = &foreign_root;
    let called = std::sync::atomic::AtomicBool::new(false);
    let error = create_with_program_repair(request, |_, _, _| {
        called.store(true, Ordering::SeqCst);
        async { Err("ownership errors must not invoke the provider".into()) }
    })
    .await
    .unwrap_err();
    assert_eq!(
        error,
        "Selected program directory does not belong to this game's library."
    );
    assert!(!called.load(Ordering::SeqCst));
    assert!(list_instances(&fixture.storage.paths).await?.is_empty());
    assert!(!fixture.repair.exists());
    assert_eq!(snapshot(&foreign_root)?, foreign_files);
    assert_eq!(snapshot(&fixture.original)?, original_files);
    let retained = app_storage::read_program_install_owner(&fixture.storage.paths, &foreign_root)
        .await?
        .unwrap();
    assert_eq!(retained.id, before.id);
    assert_eq!(retained.module_id, before.module_id);
    assert_eq!(retained.install_root, before.install_root);
    assert_eq!(retained.install_state, before.install_state);
    assert_eq!(retained.current_version, before.current_version);
    Ok(())
}
