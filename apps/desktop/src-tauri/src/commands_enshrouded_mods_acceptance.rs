use super::*;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};

fn inventory(path: &Path) -> ProbeResult<Files> {
    match fs::symlink_metadata(path) {
        Ok(_) => read_files(&plain_existing(path)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Files::new()),
        Err(error) => Err(error.into()),
    }
}

fn config_bytes(path: &Path) -> ProbeResult<Option<Vec<u8>>> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(Some(fs::read(plain_existing(path)?)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn digest(path: &Path) -> ProbeResult<String> {
    let mut file = fs::File::open(plain_existing(path)?)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>())
}

pub(super) async fn run(
    state: tauri::State<'_, DesktopState>,
    details: &InstanceDetails,
    runtime: &Path,
    shared: &Path,
    fixture: &Path,
    evidence: &Path,
) -> ProbeResult<()> {
    let fixture = plain_existing(fixture)?;
    let runtime = plain_existing(runtime)?;
    let shared = plain_existing(shared)?;
    let evidence = plain_existing(evidence)?;
    let saves = plain_existing(Path::new(&details.saves_path))?;
    let config = plain_existing(Path::new(&details.config_file_path))?;
    if details.summary.module_id != MODULE
        || !runtime.starts_with(&fixture)
        || !shared.starts_with(&fixture)
        || !saves.starts_with(&fixture)
        || !config.starts_with(&fixture)
        || runtime.starts_with(&shared)
        || shared.starts_with(&runtime)
    {
        return Err(
            "manual Mod acceptance requires separate fixture-owned package and runtime".into(),
        );
    }
    let target = runtime.join("mods");
    let shared_mods = shared.join("mods");
    let previous_mods = inventory(&target)?;
    let previous_shared = inventory(&shared_mods)?;
    let previous_saves = read_files(&saves)?;
    let shared_exe = shared.join("enshrouded_server.exe");
    let previous_exe = digest(&shared_exe)?;
    let configs = [
        config,
        runtime.join("enshrouded_server.json"),
        shared.join("enshrouded_server.json"),
    ]
    .into_iter()
    .map(|path| config_bytes(&path).map(|bytes| (path, bytes)))
    .collect::<ProbeResult<Vec<_>>>()?;

    let token = uuid::Uuid::new_v4().simple().to_string();
    let source_root = fixture.join(format!("manual-mod-probe-{token}"));
    fs::create_dir(&source_root)?;
    let bare_name = format!("probe-{token}.dll");
    let folder_name = format!("probe-folder-{token}");
    let zip_name = format!("probe-zip-{token}");
    for name in [&bare_name, &folder_name, &zip_name] {
        match fs::symlink_metadata(target.join(name)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err("synthetic Mod names must never replace an existing entry".into()),
        }
    }
    let bare_bytes = b"synthetic DLL import fixture; not executable PE bytes";
    let folder_bytes = b"synthetic folder import payload";
    let zip_bytes = b"synthetic ZIP import payload";
    let bare = source_root.join(&bare_name);
    let folder = source_root.join(&folder_name);
    let archive = source_root.join("synthetic.zip");
    fs::write(&bare, bare_bytes)?;
    fs::create_dir(&folder)?;
    fs::write(folder.join("payload.cfg"), folder_bytes)?;
    let mut zip = zip::ZipWriter::new(fs::File::create(&archive)?);
    zip.start_file(
        format!("{zip_name}/payload.cfg"),
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
    )?;
    zip.write_all(zip_bytes)?;
    zip.finish()?;

    let expected_payloads = [
        (PathBuf::from(&bare_name), bare_bytes.as_slice()),
        (
            PathBuf::from(&folder_name).join("payload.cfg"),
            folder_bytes.as_slice(),
        ),
        (
            PathBuf::from(&zip_name).join("payload.cfg"),
            zip_bytes.as_slice(),
        ),
    ];
    let mut expected_mods = previous_mods.clone();
    for (source, (relative, bytes)) in [bare, folder, archive].into_iter().zip(&expected_payloads) {
        let result = stage_manual_mod_files(
            state.clone(),
            details.summary.id.clone(),
            vec![source.to_string_lossy().into_owned()],
        )
        .await?;
        if result.instance_id != details.summary.id
            || result.module_id != MODULE
            || plain_existing(Path::new(&result.target_path))? != plain_existing(&target)?
            || plain_existing(&target)?.parent() != Some(runtime.as_path())
            || result.copied_file_count != 1
            || result.copied_total_bytes != bytes.len() as u64
            || fs::read(plain_existing(&target.join(relative))?)? != *bytes
        {
            return Err(
                "manual Mod import must publish exact bytes only into this runtime/mods".into(),
            );
        }
        expected_mods.insert(relative.clone(), bytes.to_vec());
        if inventory(&target)? != expected_mods {
            return Err(
                "manual Mod import changed unrelated payloads or inventory metadata".into(),
            );
        }
    }
    let rejected = source_root.join("unsupported.exe");
    fs::write(&rejected, b"synthetic unsupported input; never execute")?;
    let rejection = stage_manual_mod_files(
        state,
        details.summary.id.clone(),
        vec![rejected.to_string_lossy().into_owned()],
    )
    .await
    .expect_err("Enshrouded manual staging must reject an unsupported exe");
    if !rejection.contains("unsupported type") || inventory(&target)? != expected_mods {
        return Err("rejected Mod import must leave every installed payload unchanged".into());
    }

    // Remove only our exact, verified files and then their now-empty directories.
    // Synthetic DLL bytes must never reach the subsequent native-game launch.
    for (relative, _) in &expected_payloads {
        let owned = plain_existing(&target.join(relative))?;
        if !owned.starts_with(&plain_existing(&target)?) {
            return Err("manual Mod cleanup escaped the owned runtime target".into());
        }
        fs::remove_file(owned)?;
    }
    for name in [&folder_name, &zip_name] {
        let owned = plain_existing(&target.join(name))?;
        if owned.parent() != Some(plain_existing(&target)?.as_path()) {
            return Err("manual Mod directory cleanup escaped the owned runtime target".into());
        }
        fs::remove_dir(owned)?;
    }
    if inventory(&target)? != previous_mods
        || inventory(&shared_mods)? != previous_shared
        || digest(&shared_exe)? != previous_exe
        || read_files(&saves)? != previous_saves
    {
        return Err(
            "manual Mod acceptance changed protected package, saves or existing Mods".into(),
        );
    }
    for (path, before) in configs {
        if config_bytes(&path)? != before {
            return Err("manual Mod acceptance changed protected configuration".into());
        }
    }
    write_new(
        &evidence.join("manual-mod-acceptance.json"),
        &serde_json::to_vec_pretty(&json!({
            "module": MODULE,
            "synthetic_inputs": [bare_name, folder_name, "synthetic.zip"],
            "target": target,
            "exact_payload_imports": 3,
            "unsupported_exe_rejected": true,
            "existing_mods_and_metadata_preserved": true,
            "shared_package_configuration_and_saves_unchanged": true,
            "owned_payloads_removed_before_native_launch": true,
            "loader_installed_or_mod_gameplay_verified": false
        }))?,
    )?;
    Ok(())
}
