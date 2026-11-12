use super::*;
use app_core::{InstanceProgramMode, InstanceProgramSource};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const DEFAULTS: [&str; 2] = [
    "mods/dedicated_server_mods_setup.lua",
    "mods/modsettings.lua",
];
// Independent wire fixture: all three official files contain the SHA-1 vector abc.
// Metadata CRC32 covers the little-endian payload length followed by the payload.
const DST_DEPOT: &str = "d017f671970000000a210a05782e62696e100318002a14a9993e364706816aba3e25717850c26c9cd0d89d0a400a246d6f64732f6465646963617465645f7365727665725f6d6f64735f73657475702e6c7561100318002a14a9993e364706816aba3e25717850c26c9cd0d89d0a300a146d6f64732f6d6f6473657474696e67732e6c7561100318002a14a9993e364706816aba3e25717850c26c9cd0d89dbe12481f0e000000080710092000280948b6e08b850617b8811b00000000ab15c432";
// Depot 8, GID 10 overlaps only the program file, so it contributes no defaults.
const AUXILIARY_DEPOT: &str = "d017f671230000000a210a05782e62696e100318002a14a9993e364706816aba3e25717850c26c9cd0d89dbe12481f0d0000000808100a2000280348b8e1fe5317b8811b00000000ab15c432";

struct Fixture {
    baseline: ProgramBaselineFixture,
    storage: StorageBootstrap,
    program: PathBuf,
}

impl Fixture {
    async fn new() -> TestResult<Self> {
        let mut baseline = ProgramBaselineFixture::new()?;
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap();
        baseline.descriptor = discover_modules(workspace.join("modules"))?
            .into_iter()
            .find(|module| module.summary.id == "dontstarve")
            .unwrap();
        baseline
            .descriptor
            .install
            .as_mut()
            .unwrap()
            .shared_game_dir = "download".into();
        baseline.descriptor.process.as_mut().unwrap().executable = "x.bin".into();
        let (program, steamcmd) = prepare_steam_acquisition(&mut baseline)?;
        // This scenario is a first official download, not an existing user tree.
        fs::remove_file(program.join("operator.cfg"))?;
        fs::create_dir_all(program.join("mods"))?;
        for relative in DEFAULTS {
            fs::write(program.join(relative), b"abc")?;
        }
        fs::write(
            steamcmd.join("depotcache/7_9.manifest"),
            depot_bytes(DST_DEPOT),
        )?;
        let paths = app_storage::StoragePaths {
            app_data_root: baseline.root.join("app-data"),
            settings_path: baseline.root.join("app-data/settings.json"),
            database_path: baseline.root.join("app-data/db/lgs.db"),
            logs_root: baseline.root.join("app-data/logs"),
            modules_root: workspace.join("modules"),
            migrations_root: workspace.join("migrations"),
            steamcmd_root: steamcmd,
            games_root: baseline.root.clone(),
            instances_root: baseline.root.join("instances"),
            archives_root: baseline.root.join("instances/.trash"),
        };
        let storage = app_storage::bootstrap_storage_with_paths(paths)?;
        initialize_database(&storage.paths).await?;
        sync_modules(&storage.paths, std::slice::from_ref(&baseline.descriptor)).await?;
        baseline.state.app_state.write().unwrap().settings = storage.settings.clone();
        Ok(Self {
            baseline,
            storage,
            program,
        })
    }

    async fn finish(&self) -> TestResult {
        let guard = app_steamcmd::acquire_game_install_lifecycle(
            &self.baseline.state.install_locks,
            "dontstarve",
            std::slice::from_ref(&self.program),
        )
        .await?;
        let _guard = self
            .baseline
            .recorder()?
            .finish_steam_acquisition(
                guard,
                self.program.clone(),
                self.storage.paths.steamcmd_root.clone(),
                7,
                "17".into(),
                |_| {},
            )
            .await?;
        sync_game_installs(
            &self.storage.paths,
            &[GameInstallSyncRecord {
                module_id: "dontstarve".into(),
                install_root: self.program.to_string_lossy().into_owned(),
                install_state: InstallState::Installed,
                current_version: Some("17".into()),
                mark_verified: true,
            }],
        )
        .await?;
        Ok(())
    }

    fn inventory(&self, name: &str) -> TestResult<serde_json::Value> {
        Ok(serde_json::from_slice(&fs::read(self.program.join(name))?)?)
    }

    async fn preview(&self) -> TestResult<app_storage::ModuleProgramInventory> {
        Ok(app_storage::inspect_module_programs(
            &self.storage.paths,
            &self.baseline.descriptor,
            Some(InstanceProgramMode::Independent),
            InstanceProgramSource::Verified,
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            false,
        )
        .await?)
    }

    async fn create(
        &self,
        name: &str,
    ) -> Result<app_storage::CreateInstanceResult, app_storage::StorageError> {
        app_storage::create_instance_with_options(
            &self.storage.paths,
            &self.baseline.descriptor,
            CreateInstanceInput {
                name: name.into(),
                module_id: "dontstarve".into(),
            },
            app_storage::InstanceCreationOptions {
                program_mode: Some(InstanceProgramMode::Independent),
                prefer_existing_install: true,
                require_clean_program: true,
                ..Default::default()
            },
        )
        .await
    }
}

#[tokio::test]
async fn steam_shipped_defaults_allow_first_reuse_and_keep_second_instance_isolated() -> TestResult
{
    let fixture = Fixture::new().await?;
    fixture.finish().await?;
    let initial = fixture.inventory(".langame-initial-package.json")?;
    let clean = fixture.inventory(".langame-clean-package.json")?;
    for relative in DEFAULTS {
        assert_eq!(initial["files"][relative], ABC_SHA256);
        assert!(clean["files"].get(relative).is_none());
    }
    let plan = fixture.preview().await?;
    assert_eq!(plan.creation.action, "existing_install");
    assert_eq!(plan.creation.additional_bytes, Some(0));
    let first = fixture.create("First from official Steam download").await?;
    assert_eq!(
        fs::canonicalize(&first.effective_install_root)?,
        fs::canonicalize(&fixture.program)?
    );
    assert!(
        !fixture
            .storage
            .paths
            .instances_root
            .join(&first.provisioning.summary.id)
            .join("runtime/x.bin")
            .exists()
    );
    let details =
        read_instance_details(&fixture.storage.paths, &first.provisioning.summary.id).await?;
    fs::create_dir_all(&details.saves_path)?;
    let world = Path::new(&details.saves_path).join("first-world.sentinel");
    fs::write(&world, b"existing first world")?;
    let selected = b"ServerModSetup(\"4111111111\")\n-- first operator configuration";
    fs::write(fixture.program.join(DEFAULTS[0]), selected)?;
    fs::write(
        fixture.program.join("mods/operator-only.lua"),
        b"first instance only",
    )?;
    let second = fixture.create("Second with independent defaults").await?;
    assert_ne!(
        fs::canonicalize(&second.effective_install_root)?,
        fs::canonicalize(&fixture.program)?
    );
    assert_eq!(
        fs::read(second.effective_install_root.join("x.bin"))?,
        b"abc"
    );
    assert!(
        !fs::read_to_string(second.effective_install_root.join(DEFAULTS[0]))?
            .contains("4111111111")
    );
    assert!(
        !second
            .effective_install_root
            .join("mods/operator-only.lua")
            .exists()
    );
    assert_eq!(fs::read(fixture.program.join(DEFAULTS[0]))?, selected);
    assert_eq!(fs::read(world)?, b"existing first world");
    Ok(())
}

#[tokio::test]
async fn steam_optional_defaults_can_be_changed_or_absent_without_failing_installation()
-> TestResult {
    for relative in DEFAULTS {
        for missing in [false, true] {
            let fixture = Fixture::new().await?;
            let path = fixture.program.join(relative);
            if missing {
                fs::remove_file(&path)?;
            } else {
                fs::write(&path, b"xyz")?;
            }
            fixture.finish().await?;
            let initial = fixture.inventory(".langame-initial-package.json")?;
            let clean = fixture.inventory(".langame-clean-package.json")?;
            assert!(initial["files"].get(relative).is_none());
            assert!(clean["files"].get(relative).is_none());
            assert_eq!(clean["files"]["x.bin"], ABC_SHA256);
            assert!(app_storage::library_program_is_pristine(
                &fixture.program,
                &fixture.baseline.descriptor,
                None,
            )?);
            assert!(
                !fixture
                    .program
                    .join(".langame-program-acquisition.json")
                    .exists()
            );
            if missing {
                assert!(!path.exists());
            } else {
                assert_eq!(fs::read(path)?, b"xyz");
                assert_eq!(
                    fixture.preview().await?.creation.action,
                    "independent_install"
                );
            }
        }
    }
    Ok(())
}

#[tokio::test]
async fn creation_recovers_legacy_steam_initial_defaults_locally_before_reuse() -> TestResult {
    use crate::commands::commands_program_storage::{
        CreationProgramPreparation, prepare_creation_program,
    };

    let fixture = Fixture::new().await?;
    let auxiliary_depot = fixture
        .storage
        .paths
        .steamcmd_root
        .join("depotcache/8_10.manifest");
    fs::write(&auxiliary_depot, depot_bytes(AUXILIARY_DEPOT))?;
    fs::write(
        fixture.program.join("steamapps/appmanifest_7.acf"),
        r#""AppState" { "appid" "7" "StateFlags" "4" "buildid" "17" "TargetBuildID" "17" "InstalledDepots" { "7" { "manifest" "9" } "8" { "manifest" "10" } } }"#,
    )?;
    fixture.finish().await?;
    // Steam may evict an unrelated depot after the complete package was certified.
    fs::remove_file(&auxiliary_depot)?;
    let initial_path = fixture.program.join(".langame-initial-package.json");
    let mut initial = fixture.inventory(".langame-initial-package.json")?;
    for relative in DEFAULTS {
        initial["files"].as_object_mut().unwrap().remove(relative);
        if let Some(verified) = initial["verified_files"].as_object_mut() {
            verified.remove(relative);
        }
    }
    fs::write(&initial_path, serde_json::to_vec(&initial)?)?;
    assert_eq!(
        fixture.preview().await?.creation.action,
        "independent_install"
    );
    let clean_path = fixture.program.join(".langame-clean-package.json");
    let initial_before_preview = fs::read(&initial_path)?;
    let clean_before_preview = fs::read(&clean_path)?;
    let original_defaults =
        crate::commands::commands_program_storage::verify_steam_original_defaults(
            &fixture.program,
            &fixture.storage.paths.steamcmd_root,
            &fixture.baseline.descriptor,
            7,
            "17",
            &std::sync::atomic::AtomicBool::new(false),
            &mut |_| {},
        )?;
    for relative in DEFAULTS {
        assert_eq!(
            original_defaults.get(relative).map(String::as_str),
            Some(ABC_SHA256)
        );
    }
    assert!(app_storage::library_reuse_preview_with_original_defaults(
        &fixture.program,
        "dontstarve",
        &original_defaults,
        None,
    )?);
    assert_eq!(fs::read(&initial_path)?, initial_before_preview);
    assert_eq!(fs::read(&clean_path)?, clean_before_preview);
    assert!(
        !fixture
            .storage
            .paths
            .steamcmd_root
            .join("steamcmd.exe")
            .exists()
    );
    let operation = fixture
        .baseline
        .state
        .begin_storage_context_operation("legacy default recovery test")?;
    let guard = app_steamcmd::acquire_game_install_lifecycle(
        &fixture.baseline.state.install_locks,
        "dontstarve",
        std::slice::from_ref(&fixture.program),
    )
    .await?;
    let prepared = prepare_creation_program(
        &fixture.storage,
        &fixture.baseline.descriptor,
        &operation,
        &guard,
        &fixture.program,
        InstanceProgramSource::Verified,
    )
    .await?;
    assert!(
        matches!(prepared, CreationProgramPreparation::Ready(_)),
        "{prepared:?}"
    );
    let restored = fixture.inventory(".langame-initial-package.json")?;
    for relative in DEFAULTS {
        assert_eq!(restored["files"][relative], ABC_SHA256);
    }
    let (_guard, repaired_again) = fixture
        .baseline
        .recorder()?
        .restore_steam_initial_defaults(
            guard,
            fixture.program.clone(),
            fixture.storage.paths.steamcmd_root.clone(),
            7,
            "17".into(),
            |_| {},
        )
        .await?;
    assert!(!repaired_again, "complete evidence must not be republished");
    let plan = fixture.preview().await?;
    assert_eq!(plan.creation.action, "existing_install");
    assert_eq!(plan.creation.additional_bytes, Some(0));
    let created = fixture.create("Reuse recovered initial defaults").await?;
    assert_eq!(
        fs::canonicalize(&created.effective_install_root)?,
        fs::canonicalize(&fixture.program)?
    );
    assert!(
        !fixture
            .storage
            .paths
            .steamcmd_root
            .join("steamcmd.exe")
            .exists()
    );
    assert!(!auxiliary_depot.exists());
    Ok(())
}

#[tokio::test]
async fn creation_rechecks_shipped_defaults_after_preview_without_overwriting_user_changes()
-> TestResult {
    for relative in [DEFAULTS[0], "mods/operator-added.lua"] {
        let fixture = Fixture::new().await?;
        fixture.finish().await?;
        assert_eq!(fixture.preview().await?.creation.action, "existing_install");
        let path = fixture.program.join(relative);
        fs::write(&path, b"operator changed after preview")?;
        assert_eq!(
            fixture.preview().await?.creation.action,
            "independent_install"
        );
        let result = fixture.create("Recheck before exclusive reuse").await;
        if let Ok(created) = result {
            assert_ne!(
                fs::canonicalize(&created.effective_install_root)?,
                fs::canonicalize(&fixture.program)?
            );
            let copied = created.effective_install_root.join(relative);
            if copied.is_file() {
                assert_ne!(fs::read(copied)?, b"operator changed after preview");
            }
        }
        assert_eq!(fs::read(path)?, b"operator changed after preview");
    }
    Ok(())
}

#[tokio::test]
async fn legacy_creation_with_unreadable_optional_depot_keeps_verified_copy_available() -> TestResult
{
    use crate::commands::commands_program_storage::verify_steam_original_defaults;
    use crate::commands::commands_program_storage::{
        CreationProgramPreparation, prepare_creation_program,
    };

    let fixture = Fixture::new().await?;
    fixture.finish().await?;
    let initial_path = fixture.program.join(".langame-initial-package.json");
    let clean_path = fixture.program.join(".langame-clean-package.json");
    let mut initial = fixture.inventory(".langame-initial-package.json")?;
    for relative in DEFAULTS {
        initial["files"].as_object_mut().unwrap().remove(relative);
        if let Some(verified) = initial["verified_files"].as_object_mut() {
            verified.remove(relative);
        }
    }
    fs::write(&initial_path, serde_json::to_vec(&initial)?)?;
    let operator_path = fixture.program.join("mods/operator-only.lua");
    fs::write(&operator_path, b"operator configuration must survive")?;
    fs::write(
        fixture
            .storage
            .paths
            .steamcmd_root
            .join("depotcache/7_9.manifest"),
        b"damaged optional depot manifest",
    )?;
    assert!(app_storage::library_program_is_pristine(
        &fixture.program,
        &fixture.baseline.descriptor,
        None,
    )?);
    let initial_before = fs::read(&initial_path)?;
    let clean_before = fs::read(&clean_path)?;
    assert_eq!(
        fixture.preview().await?.creation.action,
        "independent_install"
    );
    let original_defaults = verify_steam_original_defaults(
        &fixture.program,
        &fixture.storage.paths.steamcmd_root,
        &fixture.baseline.descriptor,
        7,
        "17",
        &std::sync::atomic::AtomicBool::new(false),
        &mut |_| {},
    )?;
    assert!(original_defaults.is_empty());
    assert!(!app_storage::library_reuse_preview_with_original_defaults(
        &fixture.program,
        "dontstarve",
        &original_defaults,
        None,
    )?);
    assert_eq!(fs::read(&initial_path)?, initial_before);
    assert_eq!(fs::read(&clean_path)?, clean_before);

    let operation = fixture
        .baseline
        .state
        .begin_storage_context_operation("legacy copy without optional depot test")?;
    let guard = app_steamcmd::acquire_game_install_lifecycle(
        &fixture.baseline.state.install_locks,
        "dontstarve",
        std::slice::from_ref(&fixture.program),
    )
    .await?;
    let prepared = prepare_creation_program(
        &fixture.storage,
        &fixture.baseline.descriptor,
        &operation,
        &guard,
        &fixture.program,
        InstanceProgramSource::Verified,
    )
    .await?;
    assert!(
        matches!(prepared, CreationProgramPreparation::Ready(_)),
        "{prepared:?}"
    );
    assert_eq!(
        fixture.preview().await?.creation.action,
        "independent_install"
    );
    let created = fixture
        .create("Copy verified program without optional depot")
        .await?;
    assert_ne!(
        fs::canonicalize(&created.effective_install_root)?,
        fs::canonicalize(&fixture.program)?
    );
    assert_eq!(
        fs::read(created.effective_install_root.join("x.bin"))?,
        b"abc"
    );
    assert!(
        !created
            .effective_install_root
            .join("mods/operator-only.lua")
            .exists()
    );
    assert_eq!(
        fs::read(operator_path)?,
        b"operator configuration must survive"
    );
    for relative in DEFAULTS {
        assert_eq!(fs::read(fixture.program.join(relative))?, b"abc");
    }
    assert_eq!(fs::read(fixture.program.join("x.bin"))?, b"abc");
    assert_eq!(fs::read(&initial_path)?, initial_before);
    assert_eq!(fs::read(&clean_path)?, clean_before);
    Ok(())
}

#[tokio::test]
async fn optional_steam_default_verification_propagates_cancellation() -> TestResult {
    let fixture = Fixture::new().await?;
    let error = crate::commands::commands_program_storage::verify_steam_original_defaults(
        &fixture.program,
        &fixture.storage.paths.steamcmd_root,
        &fixture.baseline.descriptor,
        7,
        "17",
        &std::sync::atomic::AtomicBool::new(true),
        &mut |_| {},
    )
    .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
    Ok(())
}
