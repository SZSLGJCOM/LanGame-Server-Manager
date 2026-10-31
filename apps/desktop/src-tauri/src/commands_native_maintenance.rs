use super::*;
use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom, Write};

#[path = "commands_native_dst_restore.rs"]
mod dst_restore;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MaintenanceRefusal {
    Pinned,
    WholePackage,
}

impl MaintenanceRefusal {
    fn expected(policy: app_core::InstanceProgramUpdatePolicy, replacement: bool) -> Option<Self> {
        if policy == app_core::InstanceProgramUpdatePolicy::Pinned {
            Some(Self::Pinned)
        } else if replacement {
            Some(Self::WholePackage)
        } else {
            None
        }
    }

    fn verify(self, error: &str) -> Result<(), String> {
        let reason = match self {
            Self::Pinned => "当前版本已固定",
            Self::WholePackage => "此游戏使用整包替换安装",
        };
        if !error.contains(reason) {
            return Err(format!(
                "native maintenance did not reach its expected {self:?} refusal: {error}"
            ));
        }
        Ok(())
    }

    fn outcome(self) -> &'static str {
        match self {
            Self::Pinned => "pinned_rejected",
            Self::WholePackage => "whole_package_rejected",
        }
    }
}

pub(super) fn capture_dst_managed_ini(
    details: &InstanceDetails,
) -> ProbeResult<Option<BTreeMap<PathBuf, Vec<u8>>>> {
    if details.summary.module_id == "dontstarve" {
        Ok(Some(dst_restore::capture_managed_ini(Path::new(
            &details.saves_path,
        ))?))
    } else {
        Ok(None)
    }
}

pub(super) async fn verify_backup_restore<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    details: &InstanceDetails,
    dst_managed_ini: Option<&BTreeMap<PathBuf, Vec<u8>>>,
) -> ProbeResult<Option<InstanceDetails>> {
    let original = save_inventory(Path::new(&details.saves_path))?;
    let is_dst = details.summary.module_id == "dontstarve";
    let Some((relative, _)) = original.iter().find(|(relative, (bytes, _))| {
        *bytes > 0 && (!is_dst || dst_restore::is_native_save_file(relative))
    }) else {
        if is_dst {
            return Err("DST native backup acceptance requires a nonempty world snapshot".into());
        }
        println!(
            "NATIVE_LIFECYCLE module={} phase=backup_restore outcome=not_tested reason=no_nonempty_native_save",
            details.summary.module_id
        );
        return Ok(None);
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
    let dst = if is_dst {
        Some(dst_restore::NoModsRestore::capture(
            &backup_root.join("saves"),
            &config,
            &runtime
                .storage
                .paths
                .modules_root
                .join("dontstarve/templates/clusters/main"),
            dst_managed_ini.ok_or("DST native restore lacks its pre-start managed INI baseline")?,
        )?)
    } else {
        None
    };
    let expected_copy = if is_dst {
        dst_restore::copied_world_data(&original)
    } else {
        (
            original.len(),
            original.values().map(|(bytes, _)| *bytes).sum::<u64>(),
        )
    };
    let mut expected_saves = original.clone();
    if is_dst {
        // Cluster-level files belong to the operator. DST restores shard
        // worlds while preserving these, including post-backup additions.
        let retained = PathBuf::from(format!("native-retained-root-{}.txt", uuid::Uuid::new_v4()));
        let path = saves.join(&retained);
        fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)?
            .write_all(b"unrelated cluster-root data must survive world restoration")?;
        let entry = save_inventory(&path)?
            .remove(Path::new(""))
            .ok_or("native retained cluster-root probe is missing")?;
        expected_saves.insert(retained, entry);
    }
    let marker_root = if is_dst {
        saves.join("Master/save")
    } else {
        saves.clone()
    };
    let marker = marker_root.join(format!("native-restore-probe-{}.txt", uuid::Uuid::new_v4()));
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
    let restore_started = std::time::SystemTime::now();
    let restored = restore_instance_backup(
        (*runtime.state).clone(),
        details.summary.id.clone(),
        backup.backup_id.clone(),
        None,
    )
    .await?;
    let restore_finished = std::time::SystemTime::now();
    let restored_details =
        read_instance_details_from_storage((*runtime.state).clone(), details.summary.id.clone())
            .await?;
    let configuration_check = if let Some(expected) = dst {
        expected.verify_saves(&saves, &expected_saves)?;
        expected.verify_configuration(
            &fs::read(&details.config_file_path)?,
            restore_started,
            restore_finished,
        )?;
        expected.verify_stopped_details(
            details,
            &restored_details,
            runtime.effective_install_root,
        )?;
        "dst_canonical_restore_verified"
    } else {
        if save_inventory(&saves)? != original || fs::read(&details.config_file_path)? != config {
            return Err("native backup restore changed saved bytes or configuration".into());
        }
        settings_readback::verify_stopped(
            details,
            &restored_details,
            runtime.effective_install_root,
        )?;
        "preserved"
    };
    let safeguard =
        seed_probe::checked_path(fixture, Path::new(&restored.safeguard_backup_path), true)?;
    if restored.instance_id != details.summary.id
        || restored.backup_id != backup.backup_id
        || restored.restored_file_count != expected_copy.0
        || restored.restored_total_bytes != expected_copy.1
        || marker.exists()
        || save_inventory(&safeguard.join("saves"))? != changed_inventory
        || inventory::program_files(runtime.descriptor, runtime.effective_install_root)? != programs
    {
        return Err(format!(
            "native backup restore did not restore exact save bytes, retain its safeguard, or preserve program/configuration: copied_files={} expected_files={} copied_bytes={} expected_bytes={}",
            restored.restored_file_count, expected_copy.0, restored.restored_total_bytes, expected_copy.1
        ).into());
    }
    if is_dst {
        println!(
            "NATIVE_DST_RESTORE scope=world_data world_save_bytes=verified normalized_worldgen_files=4 managed_ini_baseline_files=4 unchanged_mod_files=4 cluster_root_addition=preserved post_backup_world_marker=removed"
        );
    }
    println!(
        "NATIVE_LIFECYCLE module={} phase=backup_restore outcome=passed restored_files={} safeguard=verified program_config={configuration_check}",
        details.summary.module_id, restored.restored_file_count,
    );
    Ok(Some(restored_details))
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
    let refusal = MaintenanceRefusal::expected(
        app_core::InstanceProgramUpdatePolicy::from_settings_json(&details.settings_json)?,
        replacement,
    );
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
    // Rejection must preserve every stopped installation byte, including
    // support files and unknown modifications, not only its entry executable.
    let refused_programs = refusal.map(|_| save_inventory(&root)).transpose()?;
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
    if let Some(refusal) = refusal {
        let error = result
            .err()
            .ok_or("native maintenance unexpectedly accepted a prohibited program mutation")?;
        refusal.verify(&error)?;
        if Some(save_inventory(&root)?) != refused_programs
            || inventory::program_files(runtime.descriptor, &root)? != programs
        {
            return Err("native maintenance refusal changed the program files".into());
        }
        runtime
            .package
            .cleanup_allowed
            .store(true, Ordering::SeqCst);
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
        refusal.map_or("passed", MaintenanceRefusal::outcome)
    );
    Ok(refusal.is_none())
}

#[test]
fn native_maintenance_refusal_observes_pinned_policy_before_package_format() {
    use app_core::InstanceProgramUpdatePolicy::{Automatic, Pinned};

    for replacement in [false, true] {
        let expected = MaintenanceRefusal::expected(Pinned, replacement).unwrap();
        assert_eq!(expected, MaintenanceRefusal::Pinned);
        expected
            .verify("实例“n”的当前版本已固定，请先切换到启动前更新，再更新它使用的服务器程序。")
            .unwrap();
        assert!(expected.verify("此游戏使用整包替换安装").is_err());
        assert!(expected.verify("network request failed").is_err());
    }
    let replacement = MaintenanceRefusal::expected(Automatic, true).unwrap();
    assert_eq!(replacement, MaintenanceRefusal::WholePackage);
    replacement.verify("此游戏使用整包替换安装").unwrap();
    assert!(replacement.verify("当前版本已固定").is_err());
    assert_eq!(MaintenanceRefusal::expected(Automatic, false), None);
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
