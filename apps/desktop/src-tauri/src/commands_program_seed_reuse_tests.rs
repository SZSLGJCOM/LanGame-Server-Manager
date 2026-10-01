use super::*;

#[tokio::test]
async fn third_instance_reuses_the_healthy_second_program_without_invoking_installer()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new().await?;
    let guard = fixture.guard().await?;
    let create = |name: &str| {
        app_storage::create_instance_with_options(
            &fixture.storage.paths,
            &fixture.descriptor,
            fixture.input(name),
            app_storage::InstanceCreationOptions {
                program_install_root: Some(fixture.original.clone()),
                prefer_existing_install: true,
                require_clean_program: true,
                program_mode: Some(app_core::InstanceProgramMode::Independent),
                ..Default::default()
            },
        )
    };
    let first = create("First original program").await?;
    let second = create("Second independent program").await?;
    assert_eq!(
        fs::canonicalize(first.effective_install_root)?,
        fs::canonicalize(&fixture.original)?
    );
    let second_root = second.effective_install_root;
    let executable = &fixture.descriptor.process.as_ref().unwrap().executable;
    let expected_program = fs::read(second_root.join(executable))?;
    fs::write(fixture.executable(), b"first instance user modification")?;
    fs::write(
        second_root.join("personal.cfg"),
        b"second instance personal file",
    )?;
    let first_binding = app_storage::read_instance_program_install(
        &fixture.storage.paths,
        &first.provisioning.summary.id,
    )
    .await?
    .unwrap();
    let second_binding = app_storage::read_instance_program_install(
        &fixture.storage.paths,
        &second.provisioning.summary.id,
    )
    .await?
    .unwrap();
    let (operation, job) = fixture.operation()?;
    let created =
        create_with_program_repair(fixture.request(&operation, &guard, &job), |_, _, _| async {
            Err("healthy local second instance must not invoke installer".into())
        })
        .await?;

    let third =
        app_storage::read_instance_program_install(&fixture.storage.paths, &created.summary.id)
            .await?
            .unwrap();
    assert_eq!(
        third.install.scope,
        app_storage::ProgramInstallScope::Instance
    );
    assert_eq!(
        third.install.owner_instance_id.as_deref(),
        Some(created.summary.id.as_str())
    );
    assert_eq!(
        fs::read(third.install.install_root.join(executable))?,
        expected_program
    );
    assert!(!third.install.install_root.join("personal.cfg").exists());
    let healthy = app_storage::read_program_install_owner(&fixture.storage.paths, &fixture.repair)
        .await?
        .unwrap();
    assert_eq!(healthy.scope, app_storage::ProgramInstallScope::Library);
    assert_eq!(healthy.owner_instance_id, None);
    assert_eq!(healthy.install_state, InstallState::Installed);
    assert_ne!(healthy.id, third.install.id);
    assert_eq!(
        fs::read(healthy.install_root.join(executable))?,
        expected_program
    );
    assert_eq!(list_instances(&fixture.storage.paths).await?.len(), 3);
    assert_eq!(
        fs::read(fixture.executable())?,
        b"first instance user modification"
    );
    assert_eq!(fs::read(second_root.join(executable))?, expected_program);
    assert_eq!(
        fs::read(second_root.join("personal.cfg"))?,
        b"second instance personal file"
    );
    for (id, previous) in [
        (first.provisioning.summary.id, first_binding.install),
        (second.provisioning.summary.id, second_binding.install),
    ] {
        let current = app_storage::read_instance_program_install(&fixture.storage.paths, &id)
            .await?
            .unwrap()
            .install;
        assert_eq!(current.id, previous.id);
        assert_eq!(current.install_root, previous.install_root);
    }
    Ok(())
}
