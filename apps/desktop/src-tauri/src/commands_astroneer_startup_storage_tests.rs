#![cfg(windows)]

use super::*;
use crate::commands::commands_astroneer_startup::prepare;

#[tokio::test(flavor = "current_thread")]
async fn astroneer_first_world_accepts_exclusive_and_private_programs_without_touching_saves()
-> Result<(), Box<dyn std::error::Error>> {
    let _lock = command_smoke_lock().lock().await;
    let root = temp_test_dir("astro-first-world");
    let environment = ProgramDataEnvGuard::set(&root.join("programdata"));
    let result = async {
        isolated_smoke_app_settings(&root)?;
        let storage = bootstrap_storage()?;
        let descriptor = discover_modules(&storage.paths.modules_root)?
            .into_iter()
            .find(|value| value.summary.id == "astroneer")
            .ok_or("Astro fixture descriptor missing")?;
        sync_modules(&storage.paths, std::slice::from_ref(&descriptor)).await?;
        prepare_fake_registered_program(&storage.paths, &descriptor).await?;
        let program = storage.paths.games_root.join("astroneer");
        let shipping = program.join("Astro/Binaries/Win64/AstroServer-Win64-Shipping.exe");
        fs::create_dir_all(shipping.parent().ok_or("shipping parent missing")?)?;
        fs::write(&shipping, b"synthetic program fixture, never executed")?;
        app_storage::record_library_program_baseline(&program, &descriptor, true, None)?;
        let app = tauri::test::mock_builder()
            .manage(DesktopState::from_storage(&storage))
            .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
        sync_modules_to_storage(app.state::<DesktopState>()).await?;

        // Production creation gives the first instance exclusive ownership of
        // the clean library; the next instance receives a private runtime copy.
        for exclusive in [true, false] {
            let created = create_fake_module_instance(
                app.state::<DesktopState>(),
                "astroneer",
                if exclusive {
                    "Exclusive world"
                } else {
                    "Private world"
                },
            )
            .await?;
            let details = read_instance_details(&storage.paths, &created.summary.id).await?;
            let instance_root = commands_program_storage::instance_root(&details)?;
            assert_eq!(
                app_storage::instance_uses_exclusive_program(instance_root)?,
                exclusive
            );
            assert_eq!(
                app_storage::instance_program_mode(instance_root)?,
                app_storage::InstanceProgramMode::Independent
            );
            let runtime = app_storage::resolve_instance_runtime_root(instance_root)?;
            if exclusive {
                assert_eq!(runtime, fs::canonicalize(&program)?);
                assert_ne!(runtime, fs::canonicalize(instance_root.join("runtime"))?);
            }
            assert!(prepare(&details)?.is_some());
            let saves = Path::new(&details.saves_path);
            assert!(fs::read_dir(saves)?.next().is_none());

            let mut mismatched = details.clone();
            mismatched.saves_path = root.join("unrelated-saves").to_string_lossy().into_owned();
            assert!(prepare(&mismatched).is_err());

            let retained = saves.join("existing-world.savegame");
            fs::write(&retained, b"preserve existing world")?;
            assert!(prepare(&details)?.is_none());
            assert_eq!(fs::read(&retained)?, b"preserve existing world");
        }
        drop(app);
        Ok::<(), Box<dyn std::error::Error>>(())
    }
    .await;
    drop(environment);
    fs::remove_dir_all(root)?;
    result
}
