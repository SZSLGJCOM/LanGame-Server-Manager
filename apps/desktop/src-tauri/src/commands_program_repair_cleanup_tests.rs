use super::*;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn snapshot(root: &Path) -> TestResult<BTreeMap<PathBuf, Vec<u8>>> {
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
                assert!(kind.is_file(), "fixture must contain only regular files");
                files.insert(
                    entry.path().strip_prefix(root)?.to_owned(),
                    fs::read(entry.path())?,
                );
            }
        }
    }
    Ok(files)
}

async fn inventory(fixture: &Fixture) -> TestResult<app_storage::ModuleProgramInventory> {
    Ok(app_storage::inspect_module_programs(
        &fixture.storage.paths,
        &fixture.descriptor,
        Some(app_core::InstanceProgramMode::Independent),
        app_core::InstanceProgramSource::Verified,
        Arc::new(AtomicBool::new(false)),
        false,
    )
    .await?)
}

fn assert_installation_counts(
    inventory: &app_storage::ModuleProgramInventory,
    library_ids: &[i64],
    private_count: usize,
) {
    assert_eq!(
        inventory.installations.len(),
        library_ids.len() + private_count
    );
    assert_eq!(
        inventory
            .installations
            .iter()
            .filter(|installation| installation.install_state == InstallState::Installed)
            .count(),
        library_ids.len() + private_count,
        "the library detail's installed-program count must follow the live installations"
    );
    let mut libraries = inventory
        .installations
        .iter()
        .filter(|installation| installation.scope == "library")
        .map(|installation| installation.id)
        .collect::<Vec<_>>();
    libraries.sort_unstable();
    let mut expected = library_ids.to_vec();
    expected.sort_unstable();
    assert_eq!(
        libraries, expected,
        "repeated creation must reuse the retained healthy library"
    );
    assert_eq!(
        inventory
            .installations
            .iter()
            .filter(|installation| installation.scope == "instance")
            .count(),
        private_count
    );
}

#[tokio::test]
async fn repaired_instance_deletion_retains_a_healthy_library_for_repeated_and_empty_recreation()
-> TestResult {
    let fixture = Fixture::new().await?;
    let first = app_storage::create_instance_with_options(
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
    let first_id = &first.provisioning.summary.id;
    let first_binding =
        app_storage::read_instance_program_install(&fixture.storage.paths, first_id)
            .await?
            .unwrap();
    assert_eq!(
        first_binding.install.scope,
        app_storage::ProgramInstallScope::Library
    );
    assert_eq!(
        fs::canonicalize(&first_binding.install.install_root)?,
        fs::canonicalize(&fixture.original)?
    );
    fs::write(fixture.executable(), b"operator modified program")?;
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
    let first_root = fixture.storage.paths.instances_root.join(first_id);
    let original_files = snapshot(&fixture.original)?;
    let first_files = snapshot(&first_root)?;
    assert_installation_counts(&inventory(&fixture).await?, &[first_binding.install.id], 0);
    let mut healthy_id = None;
    let mut healthy_files = None;
    let calls = AtomicUsize::new(0);

    for cycle in 1..=3 {
        let created = {
            let guard = fixture.guard().await?;
            let (operation, job) = fixture.operation()?;
            let mut request = fixture.request(&operation, &guard, &job);
            request.input = fixture.input(&format!("Repaired server {cycle}"));
            if cycle > 1 {
                request.program_root = &fixture.repair;
            }
            create_with_program_repair(request, |module, root, token| {
                calls.fetch_add(1, Ordering::SeqCst);
                assert_eq!(
                    cycle, 1,
                    "a retained healthy library must not invoke the installer again"
                );
                assert_ne!(
                    fs::canonicalize(&root).unwrap(),
                    fs::canonicalize(&fixture.original).unwrap(),
                    "official repair must not overwrite the first server"
                );
                install_fixture(module, root, token)
            })
            .await?
        };
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let id = &created.summary.id;
        let root = fixture.storage.paths.instances_root.join(id);
        let binding = app_storage::read_instance_program_install(&fixture.storage.paths, id)
            .await?
            .unwrap();
        assert_eq!(
            binding.install.scope,
            app_storage::ProgramInstallScope::Instance
        );
        assert_eq!(
            binding.install.owner_instance_id.as_deref(),
            Some(id.as_str())
        );
        assert_ne!(binding.install.id, first_binding.install.id);
        let healthy =
            app_storage::read_program_install_owner(&fixture.storage.paths, &fixture.repair)
                .await?
                .unwrap();
        assert_eq!(healthy.scope, app_storage::ProgramInstallScope::Library);
        assert_eq!(healthy.owner_instance_id, None);
        assert_eq!(healthy.install_state, InstallState::Installed);
        assert_ne!(healthy.id, binding.install.id);
        if let Some(id) = healthy_id {
            assert_eq!(healthy.id, id);
        } else {
            healthy_id = Some(healthy.id);
            healthy_files = Some(snapshot(&fixture.repair)?);
        }
        assert_eq!(
            fs::read(
                binding
                    .install
                    .install_root
                    .join(&fixture.descriptor.process.as_ref().unwrap().executable)
            )?,
            b"official fixture program"
        );
        assert_installation_counts(
            &inventory(&fixture).await?,
            &[first_binding.install.id, healthy.id],
            1,
        );
        assert_eq!(snapshot(&fixture.original)?, original_files);
        assert_eq!(snapshot(&first_root)?, first_files);

        let removed = app_storage::delete_instance(&fixture.storage.paths, id).await?;
        assert_eq!(&removed.instance_id, id);
        assert!(
            !root.exists(),
            "deleted instance directory survived cycle {cycle}"
        );
        assert!(
            !binding.install.install_root.exists(),
            "deleted instance program survived cycle {cycle}"
        );
        assert!(
            fixture.repair.is_dir(),
            "healthy library disappeared after cycle {cycle}"
        );
        assert!(
            app_storage::read_program_install_owner(
                &fixture.storage.paths,
                &binding.install.install_root
            )
            .await?
            .is_none(),
            "deleted private program retained an installation record"
        );
        assert_eq!(snapshot(&fixture.repair)?, *healthy_files.as_ref().unwrap());
        assert!(
            app_storage::read_instance_program_install(&fixture.storage.paths, id)
                .await?
                .is_none()
        );
        assert_installation_counts(
            &inventory(&fixture).await?,
            &[first_binding.install.id, healthy.id],
            0,
        );
        let remaining = list_instances(&fixture.storage.paths).await?;
        assert_eq!(remaining.len(), 1);
        assert_eq!(&remaining[0].id, first_id);
        let retained = app_storage::read_instance_program_install(&fixture.storage.paths, first_id)
            .await?
            .unwrap();
        assert_eq!(retained.install.id, first_binding.install.id);
        assert_eq!(
            retained.install.install_root,
            first_binding.install.install_root
        );
        assert_eq!(retained.install.scope, first_binding.install.scope);
        assert_eq!(
            retained.install.current_version,
            first_binding.install.current_version
        );
        assert_eq!(snapshot(&fixture.original)?, original_files);
        assert_eq!(snapshot(&first_root)?, first_files);
        assert!(
            app_storage::list_instance_archives(&fixture.storage.paths)
                .await?
                .archives
                .is_empty(),
            "permanent deletion must not leave an archived repair copy"
        );
    }
    let retirement = crate::commands::commands_storage_lifecycle::acquire_retirement(
        &fixture.storage.paths,
        first_id,
    )
    .await?;
    app_storage::delete_instance(&fixture.storage.paths, first_id).await?;
    crate::commands::commands_mods::after_instance_deletion(
        &fixture.storage.paths,
        &fixture.descriptor.summary.id,
        &retirement,
    )
    .await;
    drop(retirement);
    assert!(list_instances(&fixture.storage.paths).await?.is_empty());
    assert_eq!(snapshot(&fixture.repair)?, *healthy_files.as_ref().unwrap());
    let selected = app_storage::read_library_program_install(
        &fixture.storage.paths,
        &fixture.descriptor.summary.id,
    )
    .await?
    .unwrap();
    assert_eq!(Some(selected.id), healthy_id);
    let recreated = {
        let guard = fixture.guard().await?;
        let (operation, job) = fixture.operation()?;
        let mut request = fixture.request(&operation, &guard, &job);
        request.program_root = &selected.install_root;
        request.input = fixture.input("Fresh after deleting every instance");
        create_with_program_repair(request, |_, _, _| async {
            Err("empty-instance recreation must retain and reuse the healthy library".into())
        })
        .await?
    };
    let binding =
        app_storage::read_instance_program_install(&fixture.storage.paths, &recreated.summary.id)
            .await?
            .unwrap();
    assert_eq!(
        binding.install.scope,
        app_storage::ProgramInstallScope::Instance
    );
    assert_eq!(
        binding.install.owner_instance_id.as_deref(),
        Some(recreated.summary.id.as_str())
    );
    assert_ne!(
        fs::canonicalize(&binding.install.install_root)?,
        fs::canonicalize(&fixture.repair)?
    );
    fs::write(
        binding
            .install
            .install_root
            .join(&fixture.descriptor.process.as_ref().unwrap().executable),
        b"new instance modification",
    )?;
    assert_eq!(snapshot(&fixture.repair)?, *healthy_files.as_ref().unwrap());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}
