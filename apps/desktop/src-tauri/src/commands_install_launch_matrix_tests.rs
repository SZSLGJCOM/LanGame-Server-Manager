use super::*;
use app_storage::{
    StoragePaths, bootstrap_storage_with_paths, materialize_instance_configuration_for_start,
};

type MatrixResult<T> = Result<T, Box<dyn std::error::Error>>;

struct MatrixRoot(PathBuf);

impl Drop for MatrixRoot {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(env::temp_dir().as_path()));
        fs::remove_dir_all(&self.0).expect("remove owned lifecycle matrix fixture");
    }
}

/// Files model package layout only. They are never executed or downloaded, and
/// their existence must not be reported as a successful real-game installation.
pub(super) fn seed_installed_package(
    descriptor: &ModuleDescriptor,
    install_root: &Path,
) -> MatrixResult<()> {
    let install = descriptor
        .install
        .as_ref()
        .expect("catalog install contract");
    let process = descriptor
        .process
        .as_ref()
        .expect("catalog process contract");
    let source = install
        .verification_path
        .as_deref()
        .unwrap_or(&process.executable);
    assert!(
        !source.contains("{{"),
        "{} package source must be concrete",
        descriptor.summary.id
    );
    let mut files = vec![source];
    if !process.executable.contains("{{") && !process.executable.ends_with(".bat") {
        files.push(&process.executable);
    }
    if descriptor.summary.id == "necesse" {
        files.push("Server.jar");
    }
    for relative in files {
        let target = install_root.join(relative);
        fs::create_dir_all(target.parent().unwrap())?;
        fs::write(target, b"package-layout fixture; not an executable")?;
    }
    if descriptor.summary.id == "barotrauma" {
        for directory in ["Content", "Data"] {
            fs::create_dir_all(install_root.join(directory))?;
        }
    }
    Ok(())
}

fn matrix_paths(root: &Path) -> StoragePaths {
    StoragePaths {
        app_data_root: root.join("app-data"),
        settings_path: root.join("app-data/settings.json"),
        database_path: root.join("app-data/db/lgs.db"),
        logs_root: root.join("app-data/logs"),
        modules_root: workspace_root().join("modules"),
        migrations_root: workspace_root().join("migrations"),
        steamcmd_root: root.join("steamcmd"),
        games_root: root.join("games"),
        instances_root: root.join("instances"),
        archives_root: root.join("instances").join(".trash"),
    }
}

fn record(failures: &mut Vec<String>, valid: bool, message: impl Into<String>) {
    if !valid {
        failures.push(message.into());
    }
}

fn inspect_plan(
    settings: &AppSettings,
    descriptor: &ModuleDescriptor,
    details: &InstanceDetails,
    library_root: &Path,
    label: &str,
    failures: &mut Vec<String>,
) -> MatrixResult<LaunchPlan> {
    let plan = build_instance_launch_preview(settings, descriptor, details)?;
    let label = format!("{} {label}", descriptor.summary.id);
    record(
        failures,
        plan.instance_id == details.summary.id && plan.module_id == descriptor.summary.id,
        format!("{label}: launch plan targets a different instance or module"),
    );
    record(
        failures,
        details.ports.len() == descriptor.default_ports.len()
            && descriptor.default_ports.iter().all(|expected| {
                details
                    .ports
                    .iter()
                    .filter(|actual| {
                        actual.name == expected.name
                            && actual.protocol == expected.protocol
                            && actual.port > 0
                    })
                    .count()
                    == 1
            }),
        format!("{label}: declared port bindings missing, duplicated, or unallocated"),
    );
    record(
        failures,
        plan.install_state == InstallState::Installed,
        format!("{label}: prepared package not installed"),
    );
    record(
        failures,
        plan.uses_private_runtime
            == (descriptor.storage.program_sharing
                == app_modules::ModuleProgramSharing::Independent),
        format!("{label}: runtime ownership mismatch"),
    );
    record(
        failures,
        plan.executable_exists,
        format!(
            "{label}: executable not materialized: {}",
            plan.executable_path
        ),
    );
    record(
        failures,
        Path::new(&plan.working_directory).is_dir(),
        format!("{label}: working directory missing"),
    );
    record(
        failures,
        plan.ready_to_launch,
        format!(
            "{label}: unexpected startup blockers: {:?}",
            plan.validation_issues
        ),
    );
    record(
        failures,
        plan.args
            .iter()
            .all(|arg| !arg.contains("{{") && !arg.contains("}}")),
        format!("{label}: unresolved arguments"),
    );
    record(
        failures,
        plan.environment
            .values()
            .all(|value| !value.contains("{{") && !value.contains("}}")),
        format!("{label}: unresolved environment"),
    );
    let expected_root = match descriptor.storage.program_sharing {
        app_modules::ModuleProgramSharing::Shared => library_root.to_path_buf(),
        app_modules::ModuleProgramSharing::Independent => Path::new(&details.config_file_path)
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("runtime"),
    };
    record(
        failures,
        fs::canonicalize(&plan.install_root)? == fs::canonicalize(expected_root)?,
        format!("{label}: launch does not use its declared program owner"),
    );
    Ok(plan)
}

async fn prepare_available_matrix_ports(
    paths: &StoragePaths,
    descriptor: &ModuleDescriptor,
    mut details: InstanceDetails,
) -> MatrixResult<InstanceDetails> {
    // This matrix tests lifecycle/template contracts, not ownership of the host's
    // default game ports. Keep the real startup probe and persist any remapping.
    for _ in 0..16 {
        let Some(ports) = app_runtime::remap_taken_port_bindings_for_module(
            &descriptor.summary.id,
            &details.summary.bind_ip,
            &details.ports,
            &descriptor.runtime.port_groups,
        )?
        else {
            return Ok(details);
        };
        app_storage::update_instance_ports(paths, &details.summary.id, &ports).await?;
        // Storage also reserves ports across instances and normalizes groups.
        // Probe that persisted result before materializing its launch files.
        details = read_instance_details(paths, &details.summary.id).await?;
    }
    Err(format!(
        "{}: could not isolate matrix ports after 16 allocation attempts: {:?}",
        descriptor.summary.id, details.ports
    )
    .into())
}

async fn prepare_launchable_instance(
    paths: &StoragePaths,
    descriptor: &ModuleDescriptor,
    label: &str,
) -> MatrixResult<InstanceDetails> {
    let created = app_storage::create_instance(
        paths,
        descriptor,
        CreateInstanceInput {
            name: format!("{} {label}", descriptor.summary.id),
            module_id: descriptor.summary.id.clone(),
        },
    )
    .await?;
    let details = read_instance_details(paths, &created.summary.id).await?;
    let details = prepare_available_matrix_ports(paths, descriptor, details).await?;
    if descriptor.summary.id == "dontstarve" {
        // An unconfigured online DST token is an intentional startup blocker.
        // Use its supported offline mode, never a real account token.
        let mut settings: serde_json::Value = serde_json::from_str(&details.settings_json)?;
        settings["offline_cluster"] = serde_json::json!(true);
        app_storage::update_instance(
            paths,
            UpdateInstanceInput {
                id: details.summary.id.clone(),
                bind_ip: details.summary.bind_ip,
                auto_backup_on_stop: details.auto_backup_on_stop,
                backup_retention_count: details.backup_retention_count,
                settings_json: settings.to_string(),
                ports: details.ports,
            },
        )
        .await?;
    }
    Ok(materialize_instance_configuration_for_start(paths, &created.summary.id).await?)
}

async fn acquire_matrix_package(
    paths: &StoragePaths,
    settings: &AppSettings,
    descriptor: &ModuleDescriptor,
    install_root: &Path,
) -> MatrixResult<()> {
    seed_installed_package(descriptor, install_root)?;
    fs::write(
        install_root.join("matrix-package-isolation.fixture"),
        b"official package",
    )?;
    app_storage::record_library_program_baseline(install_root, descriptor, true, None)?;
    let record = build_game_install_sync_record(
        settings,
        descriptor,
        true,
        Some(&install_root.to_string_lossy()),
    );
    app_storage::sync_game_installs(paths, &[record]).await?;
    Ok(())
}

async fn exercise_module(
    paths: &StoragePaths,
    settings: &AppSettings,
    descriptor: &ModuleDescriptor,
) -> MatrixResult<Vec<String>> {
    let mut failures = Vec::new();
    let module_id = descriptor.summary.id.as_str();
    let shared = descriptor.storage.program_sharing == app_modules::ModuleProgramSharing::Shared;
    // A stored install can differ from settings.games_root; preview and storage
    // must agree on this authoritative root throughout instance creation.
    let install_root = paths
        .app_data_root
        .join("relocated-packages")
        .join(module_id);
    let override_text = install_root.to_string_lossy().into_owned();
    let missing = build_game_install_sync_record(settings, descriptor, false, Some(&override_text));
    record(
        &mut failures,
        missing.install_state == InstallState::NotInstalled,
        "missing installation must be NotInstalled",
    );
    fs::create_dir_all(&install_root)?;
    let empty = build_game_install_sync_record(settings, descriptor, false, Some(&override_text));
    record(
        &mut failures,
        empty.install_state == InstallState::Corrupted,
        "empty installation must not be Installed",
    );
    acquire_matrix_package(paths, settings, descriptor, &install_root).await?;
    let installed =
        build_game_install_sync_record(settings, descriptor, false, Some(&override_text));
    record(
        &mut failures,
        installed.install_state == InstallState::Installed,
        "package files must satisfy installation probe",
    );
    let package_probe = install_root.join("matrix-package-isolation.fixture");
    let library_before = app_storage::read_library_program_install(paths, module_id)
        .await?
        .ok_or("acquisition did not register the library")?;

    let first = prepare_launchable_instance(paths, descriptor, "first").await?;
    let first_plan = inspect_plan(
        settings,
        descriptor,
        &first,
        &install_root,
        "first",
        &mut failures,
    )?;
    let first_runtime = Path::new(&first_plan.install_root);
    let first_package_probe = first_runtime.join("matrix-package-isolation.fixture");
    record(
        &mut failures,
        fs::read(&first_package_probe)? == b"official package",
        "first instance cannot read its acquired program",
    );
    record(
        &mut failures,
        install_root.is_dir() && fs::read(&package_probe)? == b"official package",
        "creating the first instance changed the installed library",
    );
    if !shared {
        fs::write(&first_package_probe, b"first instance change")?;
    }
    let first_config = fs::read(&first.config_file_path)?;
    let first_saves = Path::new(&first.saves_path);
    let declares_install_saves = descriptor
        .storage
        .saves_path_template
        .as_deref()
        .is_some_and(|template| template.contains("paths.install_root"));
    record(
        &mut failures,
        !first_saves.starts_with(&install_root),
        "first instance saves are inside the shared package",
    );
    record(
        &mut failures,
        first_saves.starts_with(first_runtime) == declares_install_saves,
        "save ownership disagrees with the module declaration",
    );
    fs::create_dir_all(first_saves)?;
    let sentinel = first_saves.join("first-instance-save.sentinel");
    fs::write(&sentinel, b"first instance world must stay private")?;

    let second = prepare_launchable_instance(paths, descriptor, "second").await?;
    let second_plan = inspect_plan(
        settings,
        descriptor,
        &second,
        &install_root,
        "second",
        &mut failures,
    )?;
    let second_runtime = Path::new(&second_plan.install_root);
    record(
        &mut failures,
        (first_runtime == second_runtime) == shared,
        "program reuse disagrees with declared ownership",
    );
    record(
        &mut failures,
        fs::read(second_runtime.join("matrix-package-isolation.fixture"))? == b"official package",
        "second instance inherited the first instance package change",
    );
    record(
        &mut failures,
        first.saves_path != second.saves_path,
        "two instances share one save location",
    );
    record(
        &mut failures,
        !Path::new(&second.saves_path)
            .join("first-instance-save.sentinel")
            .exists(),
        "new instance inherited the first instance save",
    );
    if let Ok(relative) = first_saves.strip_prefix(first_runtime) {
        record(
            &mut failures,
            !second_runtime
                .join(relative)
                .join("first-instance-save.sentinel")
                .exists(),
            "second runtime copied the first instance save tree",
        );
    }
    record(
        &mut failures,
        install_root.is_dir()
            && fs::read(&package_probe)? == b"official package"
            && fs::read(&first_package_probe)?
                == if shared {
                    b"official package".as_slice()
                } else {
                    b"first instance change".as_slice()
                },
        "creating the second instance changed another program owner",
    );
    let library_after = app_storage::read_library_program_install(paths, module_id)
        .await?
        .ok_or("creation removed the library registration")?;
    record(
        &mut failures,
        library_after.id == library_before.id
            && library_after.install_state == InstallState::Installed
            && library_after.install_root == library_before.install_root
            && library_after.current_version == library_before.current_version,
        "creation changed the installed library identity or version",
    );
    record(
        &mut failures,
        fs::read(&sentinel)? == b"first instance world must stay private",
        "second instance modified the first instance save",
    );
    record(
        &mut failures,
        fs::read(&first.config_file_path)? == first_config,
        "second instance modified the first instance configuration",
    );
    for first_port in &first.ports {
        record(
            &mut failures,
            second.ports.iter().all(|second_port| {
                second_port.protocol != first_port.protocol || second_port.port != first_port.port
            }),
            format!(
                "instances collide on {}:{}",
                first_port.protocol, first_port.port
            ),
        );
    }
    let after_second = build_instance_launch_preview(settings, descriptor, &first)?;
    record(
        &mut failures,
        after_second.args == first_plan.args
            && after_second.executable_path == first_plan.executable_path,
        "creating a second instance changed the first launch plan",
    );
    for instance in [&first, &second] {
        let deleted = app_storage::delete_instance(paths, &instance.summary.id).await?;
        record(
            &mut failures,
            !Path::new(&deleted.deleted_instance_root).exists(),
            "deleted instance directory still occupies disk space",
        );
        let archives = app_storage::list_instance_archives(paths).await?;
        record(
            &mut failures,
            archives.archives.is_empty() && archives.pending_deletions.is_empty(),
            "completed deletion left a retained archive or unfinished cleanup",
        );
        let library = app_storage::read_library_program_install(paths, module_id)
            .await?
            .ok_or("deletion removed the program library record")?;
        record(
            &mut failures,
            library.install_state == InstallState::Installed
                && library.install_root == install_root,
            "deletion changed the installed program library",
        );
    }
    record(
        &mut failures,
        app_storage::list_instances(paths)
            .await?
            .iter()
            .all(|instance| instance.module_id != module_id),
        "replacement scenario still has a registered instance",
    );
    record(
        &mut failures,
        first_runtime.exists() == shared && second_runtime.exists() == shared,
        "deleting instances did not preserve declared program ownership",
    );
    record(
        &mut failures,
        fs::read(&package_probe)? == b"official package",
        "deleting instances changed the installed library package",
    );
    let fresh = prepare_launchable_instance(paths, descriptor, "after deletion").await?;
    let fresh_plan = inspect_plan(
        settings,
        descriptor,
        &fresh,
        &install_root,
        "after deletion",
        &mut failures,
    )?;
    record(
        &mut failures,
        fs::read(Path::new(&fresh_plan.install_root).join("matrix-package-isolation.fixture"))?
            == b"official package",
        "replacement instance inherited a deleted instance package change",
    );
    record(
        &mut failures,
        !Path::new(&fresh.saves_path)
            .join("first-instance-save.sentinel")
            .exists(),
        "replacement instance inherited the deleted instance's world",
    );
    if let Ok(relative) = first_saves.strip_prefix(first_runtime) {
        record(
            &mut failures,
            !Path::new(&fresh_plan.install_root)
                .join(relative)
                .join("first-instance-save.sentinel")
                .exists(),
            "replacement runtime copied the deleted instance save tree",
        );
    }
    Ok(failures)
}

#[tokio::test]
async fn all_catalog_modules_preserve_instance_isolation_with_declared_program_ownership() {
    let _lock = command_smoke_lock().lock().await;
    let root = MatrixRoot(temp_test_dir("modulematrix"));
    let _environment = ProgramDataEnvGuard::set(&root.0.join("programdata"));
    let storage = bootstrap_storage_with_paths(matrix_paths(&root.0)).expect("matrix storage");
    initialize_database(&storage.paths)
        .await
        .expect("matrix database");
    let descriptors =
        discover_modules(&storage.paths.modules_root).expect("repository module catalog");
    assert_eq!(
        descriptors.len(),
        32,
        "review lifecycle coverage when catalog changes"
    );
    app_storage::sync_modules(&storage.paths, &descriptors)
        .await
        .expect("sync catalog");
    let mut failures = Vec::new();
    for descriptor in &descriptors {
        match exercise_module(&storage.paths, &storage.settings, descriptor).await {
            Ok(issues) => failures.extend(
                issues
                    .into_iter()
                    .map(|issue| format!("{}: {issue}", descriptor.summary.id)),
            ),
            Err(error) => failures.push(format!("{}: {error}", descriptor.summary.id)),
        }
    }
    assert!(
        failures.is_empty(),
        "deterministic package/create/prestart contract failures:\n{}",
        failures.join("\n")
    );
    println!(
        "Verified 32 module contracts and 96 instance preparations; no game programs executed."
    );
}
