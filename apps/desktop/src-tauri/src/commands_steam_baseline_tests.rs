use super::*;
use app_core::InstallVerificationStage;

// Independent depot wire fixture for the standard SHA-1 vector "abc". Keep the
// acquisition pipeline test independent of the manifest parser's constructors.
const ORIGINAL_DEPOT: &str = "d017f671230000000a210a05782e62696e100318002a14a9993e364706816aba3e25717850c26c9cd0d89dbe12481f0d000000080710092000280348b8e1fe5317b8811b00000000ab15c432";
// Three official files containing "abc": one program file and two game configs.
const CONFIG_DEPOT: &str = "d017f6718e0000000a210a05782e62696e100318002a14a9993e364706816aba3e25717850c26c9cd0d89d0a400a246d6f64732f6465646963617465645f7365727665725f6d6f64735f73657475702e6c7561100318002a14a9993e364706816aba3e25717850c26c9cd0d89d0a270a0b636f6e6669672e6a736f6e100318002a14a9993e364706816aba3e25717850c26c9cd0d89dbe12481f0e00000008071009200028094891fefbfd0817b8811b00000000ab15c432";
const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

fn depot_bytes(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn prepare_steam_acquisition(
    fixture: &mut ProgramBaselineFixture,
) -> Result<(PathBuf, PathBuf), Box<dyn std::error::Error>> {
    fixture.descriptor.summary.steam_app_id = Some(7);
    let root = fixture.root.join("download");
    let steamcmd = fixture.root.join("steamcmd");
    app_storage::begin_empty_library_program_acquisition(&root, &fixture.descriptor)?;
    fs::create_dir_all(root.join("steamapps"))?;
    fs::create_dir_all(steamcmd.join("depotcache"))?;
    fs::write(root.join("x.bin"), b"abc")?;
    fs::write(root.join("operator.cfg"), b"operator data preserved")?;
    fs::write(
        root.join("steamapps/appmanifest_7.acf"),
        r#""AppState" { "appid" "7" "StateFlags" "4" "buildid" "17" "TargetBuildID" "17" "InstalledDepots" { "7" { "manifest" "9" } } }"#,
    )?;
    fs::write(
        steamcmd.join("depotcache/7_9.manifest"),
        depot_bytes(ORIGINAL_DEPOT),
    )?;
    Ok((root, steamcmd))
}

#[tokio::test]
async fn steam_acquisition_pipeline_preserves_declared_configs_outside_the_program_allowlist()
-> Result<(), Box<dyn std::error::Error>> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap();
    let modules = discover_modules(workspace.join("modules"))?;
    for (module_id, config) in [
        ("dontstarve", "mods/dedicated_server_mods_setup.lua"),
        ("romestead", "config.json"),
    ] {
        let mut fixture = ProgramBaselineFixture::new()?;
        fixture.descriptor = modules
            .iter()
            .find(|module| module.summary.id == module_id)
            .unwrap()
            .clone();
        let (root, steamcmd) = prepare_steam_acquisition(&mut fixture)?;
        fs::create_dir(root.join("mods"))?;
        for path in ["mods/dedicated_server_mods_setup.lua", "config.json"] {
            fs::write(root.join(path), b"abc")?;
        }
        fs::write(
            root.join(config),
            b"operator settings retained across updates",
        )?;
        fs::write(
            steamcmd.join("depotcache/7_9.manifest"),
            depot_bytes(CONFIG_DEPOT),
        )?;
        let guard = app_steamcmd::acquire_game_install_lifecycle(
            &fixture.state.install_locks,
            module_id,
            std::slice::from_ref(&root),
        )
        .await?;
        let _guard = fixture
            .recorder()?
            .finish_steam_acquisition(guard, root.clone(), steamcmd, 7, "17".into(), |_| {})
            .await?;

        assert_eq!(
            fs::read(root.join(config))?,
            b"operator settings retained across updates",
            "{module_id} operator configuration must remain untouched",
        );
        for name in [
            ".langame-clean-package.json",
            ".langame-initial-package.json",
        ] {
            let inventory: serde_json::Value = serde_json::from_slice(&fs::read(root.join(name))?)?;
            let files = inventory["files"].as_object().unwrap();
            assert!(files.get(config).is_none(), "{module_id}: {name}");
            assert_eq!(files["x.bin"], ABC_SHA256);
            assert!(files.contains_key("steamapps/appmanifest_7.acf"));
            assert_eq!(files.len(), 3);
        }
        assert!(app_storage::library_program_is_pristine(
            &root,
            &fixture.descriptor,
            None,
        )?);
        assert!(!root.join(".langame-program-acquisition.json").exists());
    }
    Ok(())
}

#[tokio::test]
async fn steam_acquisition_pipeline_publishes_exact_official_inventory_and_retains_operator_data()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = ProgramBaselineFixture::new()?;
    let (root, steamcmd) = prepare_steam_acquisition(&mut fixture)?;
    let guard = app_steamcmd::acquire_game_install_lifecycle(
        &fixture.state.install_locks,
        &fixture.descriptor.summary.id,
        std::slice::from_ref(&root),
    )
    .await?;
    let mut progress = Vec::new();
    let _guard = fixture
        .recorder()?
        .finish_steam_acquisition(guard, root.clone(), steamcmd, 7, "17".into(), |update| {
            progress.push(update);
        })
        .await?;

    for name in [
        ".langame-clean-package.json",
        ".langame-initial-package.json",
    ] {
        let inventory: serde_json::Value = serde_json::from_slice(&fs::read(root.join(name))?)?;
        let files = inventory["files"].as_object().unwrap();
        assert_eq!(
            files.keys().map(String::as_str).collect::<Vec<_>>(),
            ["steamapps/appmanifest_7.acf", "x.bin"]
        );
        assert_eq!(files["x.bin"], ABC_SHA256);
        assert_eq!(inventory["module_id"], fixture.descriptor.summary.id);
    }
    assert_eq!(
        fs::read(root.join("operator.cfg"))?,
        b"operator data preserved"
    );
    assert!(!root.join(".langame-program-acquisition.json").exists());
    assert!(app_storage::library_program_is_pristine(
        &root,
        &fixture.descriptor,
        None,
    )?);
    let first = progress.first().unwrap();
    assert_eq!(first.stage, InstallVerificationStage::SteamFiles);
    assert_eq!(first.completed_bytes, 0);
    assert_eq!(first.completed_files, 0);
    let last = progress.last().unwrap();
    assert_eq!(last.stage, InstallVerificationStage::RecordingBaseline);
    assert_eq!(last.completed_files, last.total_files);
    Ok(())
}

#[tokio::test]
async fn steam_acquisition_pipeline_rejects_bad_official_hash_without_publishing_or_clearing_owner()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = ProgramBaselineFixture::new()?;
    let (root, steamcmd) = prepare_steam_acquisition(&mut fixture)?;
    let pending = fs::read(root.join(".langame-program-acquisition.json"))?;
    // Same length defeats a size-only check; the official SHA-1 must reject it.
    fs::write(root.join("x.bin"), b"xyz")?;
    let guard = app_steamcmd::acquire_game_install_lifecycle(
        &fixture.state.install_locks,
        &fixture.descriptor.summary.id,
        std::slice::from_ref(&root),
    )
    .await?;
    let mut progress = Vec::new();
    let result = fixture
        .recorder()?
        .finish_steam_acquisition(guard, root.clone(), steamcmd, 7, "17".into(), |update| {
            progress.push(update);
        })
        .await;
    match result {
        Err(app_steamcmd::SteamCmdError::InstallationVerificationFailed { detail, .. }) => {
            assert!(detail.contains("x.bin") && detail.contains("SHA1 mismatch"));
        }
        _ => panic!("corrupted official payload was not rejected"),
    }
    assert!(!root.join(".langame-clean-package.json").exists());
    assert!(!root.join(".langame-initial-package.json").exists());
    assert_eq!(
        fs::read(root.join(".langame-program-acquisition.json"))?,
        pending
    );
    assert!(app_storage::library_program_acquisition_is_trusted(
        &root,
        &fixture.descriptor,
    )?);
    assert_eq!(fs::read(root.join("x.bin"))?, b"xyz");
    assert_eq!(
        fs::read(root.join("operator.cfg"))?,
        b"operator data preserved"
    );
    assert_eq!(
        progress.first().unwrap().stage,
        InstallVerificationStage::SteamFiles
    );
    assert!(
        progress
            .iter()
            .all(|update| update.stage != InstallVerificationStage::RecordingBaseline)
    );
    Ok(())
}
