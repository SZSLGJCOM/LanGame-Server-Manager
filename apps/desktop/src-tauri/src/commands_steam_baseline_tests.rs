use super::*;
use app_core::InstallVerificationStage;

// Independent depot wire fixture for the standard SHA-1 vector "abc". Keep the
// acquisition pipeline test independent of the manifest parser's constructors.
const ORIGINAL_DEPOT: &str = "d017f671230000000a210a05782e62696e100318002a14a9993e364706816aba3e25717850c26c9cd0d89dbe12481f0d000000080710092000280348b8e1fe5317b8811b00000000ab15c432";
const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

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
    let depot = ORIGINAL_DEPOT
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect::<Vec<_>>();
    fs::write(steamcmd.join("depotcache/7_9.manifest"), depot)?;
    Ok((root, steamcmd))
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
