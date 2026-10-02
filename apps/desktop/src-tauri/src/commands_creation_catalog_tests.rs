use super::install_launch_matrix_tests::seed_installed_package;
use super::*;
use crate::commands::tests::creation_lifecycle_tests::CREATION_INSTALL_FORBIDDEN;
use std::collections::BTreeMap;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
type FileTree = BTreeMap<PathBuf, Vec<u8>>;
const FIRST_MARKER: &str = "first-instance-only-fixture";

struct FixtureRoot(PathBuf);

impl Drop for FixtureRoot {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(env::temp_dir().as_path()));
        fs::remove_dir_all(&self.0).expect("remove owned creation fixture");
    }
}

fn check(condition: bool, message: &str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn app(storage: &StorageBootstrap) -> TestResult<tauri::App<tauri::test::MockRuntime>> {
    Ok(tauri::test::mock_builder()
        .manage(DesktopState::from_storage(storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?)
}

fn instance_root(details: &InstanceDetails) -> &Path {
    Path::new(&details.config_file_path)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
}

fn files(root: &Path) -> TestResult<FileTree> {
    fn collect(root: &Path, path: &Path, result: &mut FileTree) -> TestResult {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            check(!kind.is_symlink(), "fixture contains an external link")?;
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

fn write_marker(fixture: &Path, target: &Path, bytes: &[u8]) -> TestResult {
    check(
        target.is_absolute()
            && !target
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir)),
        "marker requires an absolute path without parent traversal",
    )?;
    let ancestor = target
        .ancestors()
        .find(|path| path.exists())
        .ok_or("no existing ancestor")?;
    // Runtime bindings use canonical Windows paths (including the verbatim
    // prefix), while instance-local paths may use the ordinary spelling.
    check(
        ancestor
            .canonicalize()?
            .starts_with(fixture.canonicalize()?),
        "marker traversed an external link",
    )?;
    fs::create_dir_all(target.parent().ok_or("marker needs a parent")?)
        .map_err(|error| format!("create marker directory {}: {error}", target.display()))?;
    fs::write(target, bytes)
        .map_err(|error| format!("write marker {}: {error}", target.display()))?;
    Ok(())
}

// Change an observable setting without selecting a different world or enabling
// provider-dependent content. Native formats and launch arguments remain real.
fn field(module_id: &str, first: bool) -> (&'static str, Value) {
    let text = if first {
        "FirstConfig123"
    } else {
        "SecondConfig123"
    };
    match module_id {
        "scum" => ("/server_general/server_name", json!(text)),
        "satisfactory" => ("/max_players", json!(if first { 3 } else { 2 })),
        "nightingale" => (
            "/server_password",
            json!(format!("{text}-password-12345678")),
        ),
        "romestead" => ("/password", json!(format!("{text}-password-12345678"))),
        "minecraft" | "necesse" | "terraria" => ("/motd", json!(text)),
        "dontstarve" => ("/cluster_name", json!(text)),
        _ => ("/server_name", json!(text)),
    }
}

async fn customize(
    app: &tauri::App<tauri::test::MockRuntime>,
    storage: &StorageBootstrap,
    descriptor: &ModuleDescriptor,
    details: &InstanceDetails,
    first: bool,
) -> TestResult<InstanceDetails> {
    let mut settings: Value = serde_json::from_str(&details.settings_json)?;
    let (pointer, value) = field(&descriptor.summary.id, first);
    if descriptor.summary.id == "scum" {
        settings["server_general"]["server_name"] = value.clone();
    } else {
        *settings
            .pointer_mut(pointer)
            .ok_or("declared setting missing")? = value.clone();
    }
    update_instance_record_if_current(
        app.state::<DesktopState>(),
        UpdateInstanceInput {
            id: details.summary.id.clone(),
            bind_ip: details.summary.bind_ip.clone(),
            auto_backup_on_stop: details.auto_backup_on_stop,
            backup_retention_count: details.backup_retention_count,
            settings_json: settings.to_string(),
            ports: details.ports.clone(),
        },
        details.settings_json.clone(),
    )
    .await?;
    let updated =
        app_storage::materialize_instance_configuration(&storage.paths, &details.summary.id)
            .await?;
    let root = instance_root(&updated);
    let program = app_storage::resolve_instance_runtime_root(root)?;
    if descriptor.summary.id == "satisfactory" {
        let native = fs::read_to_string(root.join("data/Saved/Config/WindowsServer/Game.ini"))?;
        check(
            native
                .lines()
                .any(|line| line.trim() == format!("MaxPlayers={value}")),
            "native MaxPlayers not updated",
        )?;
    } else {
        let marker = value.as_str().ok_or("text setting expected")?;
        let mut native_files = files(root)?;
        native_files.remove(Path::new(&updated.config_file_path).strip_prefix(root)?);
        let native_matches = native_files
            .values()
            .chain(files(&program)?.values())
            .any(|bytes| String::from_utf8_lossy(bytes).contains(marker));
        let launch = build_instance_launch_preview(&storage.settings, descriptor, &updated)?;
        check(
            native_matches || launch.args.iter().any(|arg| arg.contains(marker)),
            "setting did not reach native configuration or launch arguments",
        )?;
    }
    Ok(updated)
}

async fn verify_module(
    descriptor: &ModuleDescriptor,
    mode: app_core::InstanceProgramMode,
) -> TestResult {
    let root = FixtureRoot(temp_test_dir(&format!(
        "creation-catalog-{}",
        descriptor.summary.id
    )));
    let _environment = ProgramDataEnvGuard::set(&root.0.join("programdata"));
    save_app_settings(AppSettings {
        archives_root: String::new(),
        servers_root: root.0.join("instances").to_string_lossy().into_owned(),
        games_root: root.0.join("games").to_string_lossy().into_owned(),
        modules_root: workspace_root()
            .join("modules")
            .to_string_lossy()
            .into_owned(),
        steamcmd_root: root.0.join("steamcmd").to_string_lossy().into_owned(),
    })?;
    let storage = bootstrap_storage()?;
    initialize_database(&storage.paths).await?;
    sync_modules(
        &storage.paths,
        &discover_modules(&storage.paths.modules_root)?,
    )
    .await?;
    let install = descriptor
        .install
        .as_ref()
        .ok_or("missing install contract")?;
    let library = storage.paths.games_root.join(&install.shared_game_dir);
    seed_installed_package(descriptor, &library)?;
    app_storage::record_library_program_baseline(&library, descriptor, true, None)?;
    sync_game_installs(
        &storage.paths,
        &[GameInstallSyncRecord {
            module_id: descriptor.summary.id.clone(),
            install_root: library.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: None,
            mark_verified: true,
        }],
    )
    .await?;
    let first_app = app(&storage)?;
    let input = |name: &str| CreateInstanceInput {
        name: name.into(),
        module_id: descriptor.summary.id.clone(),
    };
    let first = create_instance_record(
        first_app.state::<DesktopState>(),
        input("MatrixFirst"),
        Some(mode),
    )
    .await?;
    let first = read_instance_details(&storage.paths, &first.summary.id).await?;
    let defaults: Value = serde_json::from_str(&first.settings_json)?;
    let first = customize(&first_app, &storage, descriptor, &first, true).await?;
    let first_root = instance_root(&first).to_owned();
    let first_program = app_storage::resolve_instance_runtime_root(&first_root)?;
    let first_owner = app_storage::read_instance_program_install(&storage.paths, &first.summary.id)
        .await?
        .ok_or("missing first program owner")?;
    let save_relative = Path::new("nested/isolation-world.sav");
    write_marker(
        &root.0,
        &Path::new(&first.saves_path).join(save_relative),
        FIRST_MARKER.as_bytes(),
    )?;
    write_marker(
        &root.0,
        &first_root.join("config/first-only.cfg"),
        FIRST_MARKER.as_bytes(),
    )?;
    write_marker(
        &root.0,
        &first_root.join("logs/first-only.log"),
        FIRST_MARKER.as_bytes(),
    )?;
    let has_mod_target = module_mods_spec_from_manifest(&descriptor.manifest_toml)
        .and_then(|mods| mods.manual_staging)
        .is_some();
    if has_mod_target {
        let target = crate::commands::commands_mods::resolve_manual_mod_target(
            first.summary.id.clone(),
            true,
        )
        .await?;
        write_marker(
            &root.0,
            &target
                .target_path
                .join("first-only-mod/isolation-marker.bin"),
            FIRST_MARKER.as_bytes(),
        )?;
    }
    let first_files = files(&first_root)?;
    let first_program_files = files(&first_program)?;
    drop(first_app);

    // Recreate desktop state; neither instance is removed or archived.
    let second_app = app(&storage)?;
    let second = create_instance_record(
        second_app.state::<DesktopState>(),
        input("MatrixSecond"),
        Some(mode),
    )
    .await?;
    let second =
        app_storage::materialize_instance_configuration(&storage.paths, &second.summary.id).await?;
    let second_root = instance_root(&second).to_owned();
    let second_program = app_storage::resolve_instance_runtime_root(&second_root)?;
    let second_settings: Value = serde_json::from_str(&second.settings_json)?;
    let (pointer, first_value) = field(&descriptor.summary.id, true);
    let expected = match descriptor.summary.id.as_str() {
        "scum"
        | "satisfactory"
        | "nightingale"
        | "romestead"
        | "necesse"
        | "terraria"
        | "runescapedragonwilds" => defaults.pointer(pointer).cloned(),
        _ => Some(json!("MatrixSecond")),
    };
    check(
        second_settings.pointer(pointer).cloned() == expected,
        "second instance did not get fresh default settings",
    )?;
    if descriptor.summary.id == "satisfactory" {
        let native =
            fs::read_to_string(second_root.join("data/Saved/Config/WindowsServer/Game.ini"))?;
        check(
            native.lines().any(|line| line.trim() == "MaxPlayers=4"),
            "second native MaxPlayers inherited the first value",
        )?;
    } else if let Some(marker) = first_value.as_str() {
        let launch = build_instance_launch_preview(&storage.settings, descriptor, &second)?;
        check(
            !launch.args.iter().any(|arg| arg.contains(marker)),
            "second launch arguments inherited first configuration",
        )?;
    }
    check(
        first.summary.id != second.summary.id
            && first_root.canonicalize()? != second_root.canonicalize()?,
        "instance roots overlap",
    )?;
    check(first.saves_path != second.saves_path, "save paths overlap")?;
    check(
        !Path::new(&second.saves_path).join(save_relative).exists(),
        "second inherited first save",
    )?;
    for bytes in files(&second_root)?
        .values()
        .chain(files(&second_program)?.values())
    {
        let text = String::from_utf8_lossy(bytes);
        check(
            !text.contains(FIRST_MARKER),
            "second inherited first save, mod, log or custom config",
        )?;
        if let Some(marker) = first_value.as_str() {
            check(
                !text.contains(marker),
                "second inherited first native configuration",
            )?;
        }
    }
    for port in &first.ports {
        check(
            !second
                .ports
                .iter()
                .any(|other| other.protocol == port.protocol && other.port == port.port),
            "instance ports overlap",
        )?;
    }
    let second_owner =
        app_storage::read_instance_program_install(&storage.paths, &second.summary.id)
            .await?
            .ok_or("missing second program owner")?;
    let shared = mode == app_core::InstanceProgramMode::Shared;
    check(
        (first_owner.install.id == second_owner.install.id) == shared,
        "program ownership disagrees with mode",
    )?;
    check(
        (first_program.canonicalize()? == second_program.canonicalize()?) == shared,
        "program roots disagree with mode",
    )?;
    let executable = install.verification_path.as_deref().unwrap_or(
        &descriptor
            .process
            .as_ref()
            .ok_or("missing process")?
            .executable,
    );
    check(
        fs::read(first_program.join(executable))? == fs::read(second_program.join(executable))?,
        "program package bytes changed",
    )?;
    check(
        files(&first_root)? == first_files && files(&first_program)? == first_program_files,
        "creating second changed first files",
    )?;

    customize(&second_app, &storage, descriptor, &second, false).await?;
    write_marker(
        &root.0,
        &Path::new(&second.saves_path).join(save_relative),
        b"second world bytes",
    )?;
    check(
        files(&first_root)? == first_files && files(&first_program)? == first_program_files,
        "updating second changed first files",
    )?;
    let reopened = read_instance_details(&storage.paths, &first.summary.id).await?;
    check(
        reopened.settings_json == first.settings_json
            && reopened.summary.bind_ip == first.summary.bind_ip
            && serde_json::to_value(&reopened.ports)? == serde_json::to_value(&first.ports)?,
        "first persisted settings or ports changed",
    )?;
    let owner = app_storage::read_instance_program_install(&storage.paths, &first.summary.id)
        .await?
        .ok_or("first ownership disappeared")?;
    check(
        owner.install.id == first_owner.install.id
            && owner.install.install_root == first_owner.install.install_root,
        "first ownership changed",
    )?;
    check(
        list_instances(&storage.paths).await?.len() == 2,
        "expected two live instances",
    )?;
    check(
        app_storage::list_instance_archives(&storage.paths)
            .await?
            .archives
            .is_empty(),
        "test must not archive either instance",
    )?;
    println!(
        "HEADLESS_CATALOG_CREATION module={} mode={mode:?} first_preserved=true second_config=fresh save_log_config_leak=false mod={} ports=distinct state_recreated_readback=passed",
        descriptor.summary.id,
        if has_mod_target {
            "isolated"
        } else {
            "not_declared"
        }
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn all_catalog_modules_create_clean_second_instances_after_first_configuration_and_saves()
-> TestResult {
    let _serial = command_smoke_lock().lock().await;
    let descriptors = discover_modules(workspace_root().join("modules"))?;
    check(
        descriptors.len() == 32,
        "update creation matrix when catalog changes",
    )?;
    let mut failures = Vec::new();
    let mut cases = 0;
    for descriptor in &descriptors {
        let modes: &[app_core::InstanceProgramMode] =
            if descriptor.storage.program_sharing == app_modules::ModuleProgramSharing::Shared {
                &[
                    app_core::InstanceProgramMode::Shared,
                    app_core::InstanceProgramMode::Independent,
                ]
            } else {
                &[app_core::InstanceProgramMode::Independent]
            };
        for &mode in modes {
            cases += 1;
            if let Err(error) = CREATION_INSTALL_FORBIDDEN
                .scope(true, verify_module(descriptor, mode))
                .await
            {
                let failure = format!("{} {mode:?}: {error}", descriptor.summary.id);
                eprintln!("HEADLESS_CATALOG_CREATION_FAILED {failure}");
                failures.push(failure);
            }
        }
    }
    check(failures.is_empty(), &failures.join("\n"))?;
    println!(
        "HEADLESS_CATALOG_CREATION_COMPLETE modules={} cases={cases} downloads=0 real_game_processes=0",
        descriptors.len()
    );
    Ok(())
}
