//! Explicit real Steam download and first-instance reuse; no game is started.
use super::*;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

const DST: &str = "dontstarve";
const EXE: &str = "bin64/dontstarve_dedicated_server_nullrenderer_x64.exe";

#[tokio::test(flavor = "current_thread")]
#[ignore = "opt-in: downloads real Don't Starve Together into isolated storage, creates its first instance with installation forbidden, preserves evidence, and never starts a server"]
async fn native_dontstarve_download_first_instance_reuses_program() -> TestResult {
    let _serial = command_smoke_lock().lock().await;
    let root = ProbeRoot::new_for("dstdownload", "DST_DOWNLOAD_REUSE")?;
    println!(
        "DST_DOWNLOAD_REUSE phase=isolated_root path={}",
        root.path.display()
    );
    let _environment = ProgramDataEnvGuard::set(&root.path.join("programdata"));
    save_app_settings(fixture_settings(&root.path))?;
    let storage = bootstrap_storage()?;
    for path in [
        &storage.paths.app_data_root,
        &storage.paths.database_path,
        &storage.paths.instances_root,
        &storage.paths.games_root,
        &storage.paths.archives_root,
        &storage.paths.steamcmd_root,
    ] {
        check(
            path.starts_with(&root.path),
            "acceptance storage escaped its owned root",
        )?;
    }
    check(
        !storage.paths.games_root.join(DST).exists(),
        "real download must start without a previous server package",
    )?;
    // Reuse only the tool runtime, including its installed-package markers.
    // Config, credentials, content caches and game payload are never copied.
    let seed = PathBuf::from(
        std::env::var_os("LANGAME_DST_STEAMCMD_SEED")
            .ok_or("set LANGAME_DST_STEAMCMD_SEED to the existing SteamCMD executable")?,
    );
    app_steamcmd::steam_depot::check_plain(&seed, false)?;
    fs::create_dir_all(&storage.paths.steamcmd_root)?;
    seed_isolated_steamcmd(&seed, &storage.paths.steamcmd_root)?;
    let app = mock_app(&storage)?;
    println!("DST_DOWNLOAD_REUSE phase=prepare_isolated_steamcmd");
    let tool = Box::pin(ensure_steamcmd_ready(
        app.state::<DesktopState>(),
        "dontstarve-download-reuse".into(),
    ))
    .await?;
    check(
        fs::canonicalize(&tool.root)? == fs::canonicalize(&storage.paths.steamcmd_root)?,
        "acceptance discovered a SteamCMD runtime outside its fixture",
    )?;
    let download_started = Instant::now();
    println!("DST_DOWNLOAD_REUSE phase=download appid=343050");
    let downloaded = Box::pin(install_module_game(app.state::<DesktopState>(), DST.into())).await?;
    let download_elapsed_ms = download_started.elapsed().as_millis();
    check(
        downloaded.install_state == InstallState::Installed && downloaded.executable_exists,
        "official DST installation did not complete",
    )?;
    let source = fs::canonicalize(&downloaded.install_root)?;
    check(
        source == fs::canonicalize(storage.paths.games_root.join(DST))?,
        "download escaped its isolated server directory",
    )?;
    check(
        source.join(CLEAN_PACKAGE).is_file()
            && source.join(".langame-initial-package.json").is_file()
            && !source.join(".langame-program-acquisition.json").exists(),
        "completed download lacks finalized original package inventories",
    )?;
    let main = app_steamcmd::steam_depot::manifest::parse_acf(
        &fs::read(source.join("steamapps/appmanifest_343050.acf"))?,
        343050,
    )?;
    let mut owners = std::collections::BTreeSet::new();
    if let Some(shared) = main.get("shareddepots") {
        for owner in app_steamcmd::steam_depot::manifest::object(shared)?.values() {
            owners.insert(u32::try_from(
                app_steamcmd::steam_depot::manifest::decimal(
                    app_steamcmd::steam_depot::manifest::text(owner)?,
                )?,
            )?);
        }
    }
    let mut acfs = BTreeMap::new();
    for id in std::iter::once(343050).chain(owners.iter().copied()) {
        let path = source.join(format!("steamapps/appmanifest_{id}.acf"));
        app_steamcmd::steam_depot::manifest::parse_acf(&fs::read(&path)?, id)?;
        acfs.insert(id, digest(&path)?);
    }
    let program_sha = digest(&source.join(EXE))?;
    let plan = app_storage::inspect_module_programs(
        &storage.paths,
        &discover_modules(&storage.paths.modules_root)?
            .into_iter()
            .find(|module| module.summary.id == DST)
            .ok_or("DST module missing")?,
        Some(app_core::InstanceProgramMode::Independent),
        app_core::InstanceProgramSource::Verified,
        Arc::new(AtomicBool::new(false)),
        false,
    )
    .await?;
    check(
        plan.creation.action == "existing_install",
        "first instance would copy the downloaded program",
    )?;
    check(
        plan.creation.additional_bytes == Some(0),
        "first instance reserves a program copy",
    )?;
    println!(
        "DST_DOWNLOAD_REUSE phase=download_complete elapsed_ms={download_elapsed_ms} shared_owners={owners:?} first_action=existing_install additional_bytes=0"
    );
    let creation_started = Instant::now();
    let instance = CREATION_INSTALL_FORBIDDEN
        .scope(
            true,
            create_instance_record(
                app.state::<DesktopState>(),
                CreateInstanceInput {
                    name: "DST isolated download acceptance".into(),
                    module_id: DST.into(),
                },
                Some(app_core::InstanceProgramMode::Independent),
            ),
        )
        .await?;
    let creation_elapsed_ms = creation_started.elapsed().as_millis();
    let created = read_instance_details(&storage.paths, &instance.summary.id).await?;
    let instance_root = Path::new(&created.config_file_path)
        .parent()
        .and_then(Path::parent)
        .ok_or("instance root missing")?;
    let bound = app_storage::read_instance_program_install(&storage.paths, &instance.summary.id)
        .await?
        .ok_or("instance program binding missing")?;
    check(
        fs::canonicalize(&bound.install.install_root)? == source,
        "first instance copied or moved its program",
    )?;
    check(
        app_storage::instance_uses_exclusive_program(instance_root)?,
        "first instance is not bound exclusively to its library",
    )?;
    check(
        !instance_root.join("runtime").join(EXE).exists(),
        "first instance contains a copied server executable",
    )?;
    check(
        !instance_root
            .join("runtime/.langame-private-runtime")
            .exists(),
        "first instance created a private program copy",
    )?;
    check(
        created.active_run.is_none(),
        "acceptance unexpectedly started a game server",
    )?;
    check(
        digest(&source.join(EXE))? == program_sha,
        "creation modified the original program",
    )?;
    for (id, expected) in acfs {
        check(
            digest(&source.join(format!("steamapps/appmanifest_{id}.acf")))? == expected,
            "creation rewrote a Steam manifest or ran validation",
        )?;
    }
    let report = json!({
        "passed": true, "host": std::env::var("COMPUTERNAME")?, "module_id": DST,
        "root": root.path, "program_root": source, "database_path": storage.paths.database_path,
        "instance_id": created.summary.id, "shared_owners": owners,
        "download_elapsed_ms": download_elapsed_ms,
        "creation_elapsed_ms": creation_elapsed_ms,
        "manual_validation_calls": 0, "creation_installer_forbidden": true,
        "first_instance_uses_download_root": true, "additional_program_bytes": 0,
        "game_started": false, "server_executable_sha256": program_sha,
    });
    fs::write(
        root.path.join("dst-download-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("DST_DOWNLOAD_REUSE {}", serde_json::to_string(&report)?);
    // Keep only this opt-in run's evidence and downloaded package for inspection.
    // Existing installations are neither opened for mutation nor cleaned here.
    drop(app);
    Ok(())
}

fn seed_isolated_steamcmd(executable: &Path, target: &Path) -> TestResult {
    let source = executable.parent().ok_or("SteamCMD seed has no parent")?;
    fs::copy(executable, target.join("steamcmd.exe"))?;
    for name in [
        "crashhandler.dll",
        "crashhandler64.dll",
        "steam.dll",
        "steamclient.dll",
        "steamclient64.dll",
        "steamconsole.dll",
        "steamconsole64.dll",
        "steamerrorreporter.exe",
        "tier0_s.dll",
        "tier0_s64.dll",
        "vstdlib_s.dll",
        "vstdlib_s64.dll",
        "bin",
        "public",
        "siteserverui",
        "package/steam_cmd_win32.installed",
        "package/steam_cmd_win32.manifest",
        "package/steam_cmd_win64.installed",
        "package/steam_cmd_win64.manifest",
    ] {
        let path = source.join(name);
        if path.try_exists()? {
            copy_tool_asset(&path, &target.join(name))?;
        }
    }
    Ok(())
}

fn copy_tool_asset(source: &Path, target: &Path) -> TestResult {
    let metadata = fs::symlink_metadata(source)?;
    app_steamcmd::steam_depot::check_plain(source, metadata.is_dir())?;
    if metadata.is_dir() {
        fs::create_dir_all(target)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_tool_asset(&entry.path(), &target.join(entry.file_name()))?;
        }
    } else {
        fs::create_dir_all(target.parent().ok_or("tool asset target has no parent")?)?;
        fs::copy(source, target)?;
    }
    Ok(())
}
