use super::*;
use std::io::{Read, Seek, SeekFrom, Write};

type ProbeResult<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProgramMaintenance {
    None,
    Update,
    Validate,
}

impl ProgramMaintenance {
    pub(super) fn from_env(mode: &creation::CreationMode) -> Result<Self, String> {
        let selected = Self::parse(
            std::env::var("LANGAME_NATIVE_PROGRAM_MAINTENANCE")
                .ok()
                .as_deref(),
        )?;
        if selected != Self::None && mode.instances == creation::InstanceMode::SharedPair {
            return Err("native program maintenance and shared_pair require separate runs; the pair probe preserves its peer's original program bytes".into());
        }
        if selected != Self::None && std::env::var_os("LANGAME_NATIVE_STEAMCMD_ROOT").is_some() {
            return Err("native program maintenance requires its disposable SteamCMD root; unset LANGAME_NATIVE_STEAMCMD_ROOT".into());
        }
        Ok(selected)
    }

    fn parse(value: Option<&str>) -> Result<Self, String> {
        match value {
            None => Ok(Self::None),
            Some("update") => Ok(Self::Update),
            Some("validate") => Ok(Self::Validate),
            Some(_) => Err("unsupported native program maintenance mode".into()),
        }
    }
}

pub(super) async fn verify_backup_restore<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    details: &InstanceDetails,
) -> ProbeResult<bool> {
    let original = save_inventory(Path::new(&details.saves_path))?;
    let Some((relative, _)) = original.iter().find(|(_, (bytes, _))| *bytes > 0) else {
        println!(
            "NATIVE_LIFECYCLE module={} phase=backup_restore outcome=not_tested reason=no_nonempty_native_save",
            details.summary.module_id
        );
        return Ok(false);
    };
    let fixture = &runtime.package.root;
    let saves = seed_probe::checked_path(fixture, Path::new(&details.saves_path), true)?;
    let changed = seed_probe::checked_path(fixture, &saves.join(relative), false)?;
    let config = fs::read(&details.config_file_path)?;
    let programs = inventory::program_files(runtime.descriptor, runtime.effective_install_root)?;
    let backup =
        create_instance_backup((*runtime.state).clone(), details.summary.id.clone()).await?;
    let backup_root = seed_probe::checked_path(fixture, Path::new(&backup.backup_path), true)?;
    if save_inventory(&backup_root.join("saves"))? != original {
        return Err("native restore source does not contain the stopped save bytes".into());
    }
    let marker = saves.join(format!("native-restore-probe-{}.txt", uuid::Uuid::new_v4()));
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&marker)?
        .write_all(b"disposable post-backup data")?;
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(changed)?;
    let mut byte = [0_u8; 1];
    file.read_exact(&mut byte)?;
    byte[0] ^= 0xff;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&byte)?;
    file.sync_all()?;
    drop(file);
    let changed_inventory = save_inventory(&saves)?;
    if changed_inventory == original {
        return Err("native restore probe did not change its disposable save data".into());
    }
    let restored = restore_instance_backup(
        (*runtime.state).clone(),
        details.summary.id.clone(),
        backup.backup_id.clone(),
        None,
    )
    .await?;
    let safeguard =
        seed_probe::checked_path(fixture, Path::new(&restored.safeguard_backup_path), true)?;
    if restored.instance_id != details.summary.id
        || restored.backup_id != backup.backup_id
        || restored.restored_file_count != original.len()
        || restored.restored_total_bytes != original.values().map(|(bytes, _)| *bytes).sum::<u64>()
        || save_inventory(&saves)? != original
        || marker.exists()
        || save_inventory(&safeguard.join("saves"))? != changed_inventory
        || fs::read(&details.config_file_path)? != config
        || inventory::program_files(runtime.descriptor, runtime.effective_install_root)? != programs
    {
        return Err("native backup restore did not restore exact save bytes, retain its safeguard, or preserve program/configuration".into());
    }
    println!(
        "NATIVE_LIFECYCLE module={} phase=backup_restore outcome=passed restored_files={} safeguard=verified program_config=preserved",
        details.summary.module_id, restored.restored_file_count
    );
    Ok(true)
}

pub(super) async fn verify_program_maintenance<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    details: &InstanceDetails,
    mode: ProgramMaintenance,
) -> ProbeResult<bool> {
    let replacement = runtime
        .descriptor
        .install
        .as_ref()
        .is_some_and(|install| install.download_url_windows.is_some());
    if mode == ProgramMaintenance::None && !replacement {
        println!(
            "NATIVE_LIFECYCLE module={} phase=program_maintenance outcome=not_tested reason=network_update_not_selected",
            details.summary.module_id
        );
        return Ok(false);
    }
    let fixture = &runtime.package.root;
    let root = seed_probe::checked_path(fixture, runtime.effective_install_root, true)?;
    let before =
        app_storage::read_instance_program_install(&runtime.storage.paths, &details.summary.id)
            .await?
            .ok_or("native maintenance has no registered program installation")?;
    let config = fs::read(&details.config_file_path)?;
    let saves = save_inventory(Path::new(&details.saves_path))?;
    let programs = inventory::program_files(runtime.descriptor, &root)?;
    let retained = runtime
        .descriptor
        .storage
        .retained_paths
        .iter()
        .map(|relative| {
            let path = root.join(relative);
            save_inventory(&path).map(|inventory| (path, inventory))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let marker = root.join(format!("native-unknown-mod-{}.txt", uuid::Uuid::new_v4()));
    let marker_bytes = b"unknown instance modification must survive native maintenance";
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&marker)?
        .write_all(marker_bytes)?;
    // A failed native installer may retain a child process. Keep the owned
    // package until shutdown is known, just as the runtime cycle does.
    if !replacement {
        runtime
            .package
            .cleanup_allowed
            .store(false, Ordering::SeqCst);
    }
    let result = update_instance_program(
        runtime.app.clone(),
        details.summary.id.clone(),
        mode == ProgramMaintenance::Validate,
    )
    .await;
    if replacement {
        let error = result
            .err()
            .ok_or("whole-package replacement unexpectedly overwrote an instance")?;
        if !error.contains("此游戏使用整包替换安装")
            || inventory::program_files(runtime.descriptor, &root)? != programs
        {
            return Err("whole-package replacement failed without preserving programs at its explicit refusal boundary".into());
        }
    } else {
        let updated = result?;
        runtime
            .package
            .cleanup_allowed
            .store(true, Ordering::SeqCst);
        if updated.module_id != details.summary.module_id
            || updated.install_state != InstallState::Installed
            || !updated.executable_exists
            || updated.operation
                != if mode == ProgramMaintenance::Validate {
                    "validate"
                } else {
                    "update"
                }
            || fs::canonicalize(&updated.install_root)? != root
        {
            return Err(
                "native maintenance did not verify the registered instance program root".into(),
            );
        }
        inventory::program_files(runtime.descriptor, &root)?;
    }
    let after =
        app_storage::read_instance_program_install(&runtime.storage.paths, &details.summary.id)
            .await?
            .ok_or("native maintenance lost program ownership")?;
    if before.install.id != after.install.id
        || before.install.scope != after.install.scope
        || before.install.owner_instance_id != after.install.owner_instance_id
        || after.install.install_state != InstallState::Installed
        || fs::canonicalize(&after.install.install_root)? != root
        || fs::read(&details.config_file_path)? != config
        || save_inventory(Path::new(&details.saves_path))? != saves
        || fs::read(&marker)? != marker_bytes
    {
        return Err("native maintenance changed ownership, saves, configuration, or unknown modification bytes".into());
    }
    for (path, original) in retained {
        if save_inventory(&path)? != original {
            return Err("native maintenance changed retained native configuration".into());
        }
    }
    fs::remove_file(marker)?;
    println!(
        "NATIVE_LIFECYCLE module={} phase=program_maintenance outcome={} selected={mode:?} ownership_config_saves_mod=preserved",
        details.summary.module_id,
        if replacement {
            "whole_package_rejected"
        } else {
            "passed"
        }
    );
    Ok(!replacement)
}

#[test]
fn native_program_maintenance_requires_an_explicit_supported_mode() {
    assert_eq!(
        ProgramMaintenance::parse(None).unwrap(),
        ProgramMaintenance::None
    );
    assert_eq!(
        ProgramMaintenance::parse(Some("update")).unwrap(),
        ProgramMaintenance::Update
    );
    assert_eq!(
        ProgramMaintenance::parse(Some("validate")).unwrap(),
        ProgramMaintenance::Validate
    );
    assert!(ProgramMaintenance::parse(Some("yes")).is_err());
}
