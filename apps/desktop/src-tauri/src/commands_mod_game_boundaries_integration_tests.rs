//! External managers own these games' effective Mod sets. Rejected commands
//! must preserve the independent instance and its original program library.
use super::*;
use crate::commands::commands_file_mods::manage_manual_mod;
use crate::commands::commands_program_storage::{instance_root, program_mode};
use std::collections::BTreeMap;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
type Tree = BTreeMap<PathBuf, Option<Vec<u8>>>;

struct Fixture(PathBuf);

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("external Mod manager fixture cleanup failed: {error}");
        }
    }
}

fn tree(root: &Path) -> Result<Tree> {
    let mut result = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err("unexpected link in inert boundary fixture".into());
        }
        let relative = path.strip_prefix(root)?.to_path_buf();
        if metadata.is_dir() {
            result.insert(relative, None);
            for entry in fs::read_dir(&path)? {
                pending.push(entry?.path());
            }
        } else {
            result.insert(relative, Some(fs::read(path)?));
        }
    }
    Ok(result)
}

fn rejected(error: &str, module: &str) -> Result {
    let details: Value = serde_json::from_str(error)?;
    assert_eq!(details["module_id"], module);
    assert_eq!(
        details["code"],
        if module == "astroneer" {
            "manual_mod_integration_required"
        } else {
            "mod_package_layout_unsupported"
        }
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn external_manager_commands_reject_without_changing_instance_programs() -> Result {
    let _serial = command_smoke_lock().lock().await;
    let fixture = Fixture(temp_test_dir("external-mod-manager-boundary"));
    let _environment = ProgramDataEnvGuard::set(&fixture.0.join("programdata"));
    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: fixture.0.join("instances").to_string_lossy().into_owned(),
        games_root: fixture.0.join("games").to_string_lossy().into_owned(),
        modules_root: workspace_root()
            .join("modules")
            .to_string_lossy()
            .into_owned(),
        steamcmd_root: fixture.0.join("steamcmd").to_string_lossy().into_owned(),
    };
    save_app_settings(settings)?;
    let storage = bootstrap_storage()?;
    initialize_database(&storage.paths).await?;
    let descriptors = discover_modules(&storage.paths.modules_root)?;
    sync_modules(&storage.paths, &descriptors).await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;

    for (module, mod_path, external_output) in [
        (
            "astroneer",
            "Astro/Saved/Paks",
            "999-AstroModIntegrator_P.pak",
        ),
        ("satisfactory", "FactoryGame/Mods", "SML/SML.uplugin"),
    ] {
        let descriptor = find_descriptor(&descriptors, module)?;
        prepare_fake_registered_program(&storage.paths, descriptor).await?;
        let created = app_storage::create_instance_with_options(
            &storage.paths,
            descriptor,
            CreateInstanceInput {
                name: format!("External manager {module}"),
                module_id: module.into(),
            },
            app_storage::InstanceCreationOptions {
                prefer_existing_install: false,
                program_mode: Some(app_core::InstanceProgramMode::Independent),
                require_clean_program: true,
                ..Default::default()
            },
        )
        .await?
        .provisioning;
        let id = created.summary.id;
        let before = read_instance_details(&storage.paths, &id).await?;
        assert_eq!(
            program_mode(&before)?,
            app_core::InstanceProgramMode::Independent
        );
        let instance = instance_root(&before)?.to_path_buf();
        let library = storage.paths.games_root.join(module);
        let program = app_storage::resolve_instance_runtime_root(&instance)?;
        assert_ne!(program.canonicalize()?, library.canonicalize()?);
        assert!(
            program
                .canonicalize()?
                .starts_with(instance.canonicalize()?)
        );
        // Both roots contain operator files so an accidental cross-root change
        // is observable. The instance follows each game's real private layout.
        for program_root in [&program, &library] {
            let mods = program_root.join(mod_path);
            fs::create_dir_all(mods.join("Example/config"))?;
            fs::write(mods.join("Example/payload.pak"), b"existing Mod payload")?;
            fs::write(
                mods.join("Example/config/operator.ini"),
                b"\xef\xbb\xbfoperator=true\r\n",
            )?;
            let integrated = mods.join(external_output);
            fs::create_dir_all(
                integrated
                    .parent()
                    .ok_or("missing external output parent")?,
            )?;
            fs::write(integrated, b"external manager generated output")?;
        }
        let local = fixture.0.join(format!("{module}-import.zip"));
        let mut archive = zip::ZipWriter::new(fs::File::create(&local)?);
        archive.start_file(
            "Example/payload.pak",
            zip::write::SimpleFileOptions::default(),
        )?;
        archive.write_all(b"new inert Mod payload")?;
        archive.finish()?;
        let library_before = tree(&library)?;
        let instance_before = tree(&instance)?;
        let details_before = serde_json::to_value(&before)?;

        let online_error = install_manual_mod_references(
            app.state::<DesktopState>(),
            id.clone(),
            vec![
                "https://thunderstore.io/c/valheim/p/JereKuusela/Server_devcommands/1.115.0/"
                    .into(),
            ],
            None,
        )
        .await
        .expect_err("unsupported online workflow must reject before copying the program");
        rejected(&online_error, module)?;
        assert_eq!(
            tree(&instance)?,
            instance_before,
            "{module}: online instance"
        );
        assert_eq!(tree(&library)?, library_before, "{module}: online library");

        let local_error = stage_manual_mod_files(
            app.state::<DesktopState>(),
            id.clone(),
            vec![local.to_string_lossy().into_owned()],
        )
        .await
        .expect_err("unsupported local workflow must reject before copying the program");
        rejected(&local_error, module)?;
        assert_eq!(
            tree(&instance)?,
            instance_before,
            "{module}: local instance"
        );
        assert_eq!(tree(&library)?, library_before, "{module}: local library");

        for action in ["adopt", "enable", "disable", "delete"] {
            let error = manage_manual_mod(
                app.state::<DesktopState>(),
                id.clone(),
                if action == "adopt" {
                    "Example"
                } else {
                    "local-Example"
                }
                .into(),
                action.into(),
            )
            .await
            .expect_err("external manager must retain ownership of Mod mutations");
            rejected(&error, module)?;
            assert_eq!(
                tree(&instance)?,
                instance_before,
                "{module}/{action}: instance"
            );
            assert_eq!(
                tree(&library)?,
                library_before,
                "{module}/{action}: library"
            );
        }
        let after = read_instance_details(&storage.paths, &id).await?;
        assert_eq!(serde_json::to_value(&after)?, details_before);
        assert_eq!(
            program_mode(&after)?,
            app_core::InstanceProgramMode::Independent
        );
        assert_eq!(
            app_storage::resolve_instance_runtime_root(&instance)?.canonicalize()?,
            program.canonicalize()?
        );
    }
    Ok(())
}
