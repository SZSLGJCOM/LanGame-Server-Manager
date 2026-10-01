use super::inventory::save_inventory;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Instance archiving retains its personal files. Declared data outside that
/// directory must remain at its original path instead of following the archive.
pub(super) fn verify_preserved(
    original: &Path,
    previous_instance: &Path,
    archive: &Path,
    expected: &BTreeMap<PathBuf, (u64, u64)>,
    label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let current = match original.strip_prefix(previous_instance) {
        Ok(relative) => archive.join(relative),
        Err(_) => original.to_path_buf(),
    };
    if save_inventory(&current)? != *expected {
        return Err(format!("native instance archiving changed {label}").into());
    }
    Ok(())
}

/// Verify stored bytes for native data that an exclusive instance wrote into
/// its installation. The program dependency itself is never substituted here.
pub(super) async fn verify_archived_native_data(
    paths: &app_storage::StoragePaths,
    archive_id: &str,
    original: &Path,
    previous_instance: &Path,
    archive: &Path,
    expected: &BTreeMap<PathBuf, (u64, u64)>,
    label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use sqlx::Connection;
    if original.starts_with(previous_instance) {
        return verify_preserved(original, previous_instance, archive, expected, label);
    }
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&paths.database_path)
        .read_only(true);
    let mut connection = sqlx::SqliteConnection::connect_with(&options).await?;
    let snapshot: String =
        sqlx::query_scalar("SELECT snapshot_json FROM instance_archives WHERE archive_id=?1")
            .bind(archive_id)
            .fetch_one(&mut connection)
            .await?;
    connection.close().await?;
    let snapshot: serde_json::Value = serde_json::from_str(&snapshot)?;
    let Some(external) = snapshot
        .get("external_program")
        .filter(|value| !value.is_null())
    else {
        return verify_preserved(original, previous_instance, archive, expected, label);
    };
    let program_root = Path::new(
        external["root"]
            .as_str()
            .ok_or("external archive has no root")?,
    );
    let Ok(relative) = original.strip_prefix(program_root) else {
        return verify_preserved(original, previous_instance, archive, expected, label);
    };
    let entries = external["files"]
        .as_object()
        .ok_or("external archive has no files")?
        .iter()
        .collect::<BTreeMap<_, _>>();
    for (file, bytes) in expected {
        let key = relative.join(file).to_string_lossy().replace('\\', "/");
        let (index, (_, entry)) = entries
            .iter()
            .enumerate()
            .find(|(_, (name, _))| name.as_str() == key)
            .ok_or("native data is missing from the external archive manifest")?;
        if entry["stored"] != true {
            return Err("native data was treated as rebuildable program".into());
        }
        let payload = archive
            .join(".langame-archived-program")
            .join(format!("{index:06}"));
        if save_inventory(&payload)?.get(Path::new("")) != Some(bytes) {
            return Err(format!("external archive changed {label}").into());
        }
    }
    Ok(())
}

pub(super) async fn restore_and_verify<R: tauri::Runtime>(
    runtime: &super::runtime::NativeRuntime<'_, '_, R>,
    previous: &super::InstanceDetails,
    archived_root: &Path,
    expected_saves: &BTreeMap<PathBuf, (u64, u64)>,
    expected_programs: &BTreeMap<PathBuf, (u64, u64)>,
) -> Result<super::InstanceDetails, Box<dyn std::error::Error>> {
    use crate::commands::commands_storage_management as management;
    let state = runtime.state;
    let id = &previous.summary.id;
    println!(
        "NATIVE_LIFECYCLE module={} phase=archive_recovery_begin",
        previous.summary.module_id
    );
    let listed = management::list_instance_archives((*state).clone()).await?;
    let archived = listed
        .archives
        .iter()
        .find(|archive| archive.instance_id.as_ref() == Some(id))
        .ok_or("native archiving did not record a recoverable archive")?;
    if !archived.can_restore || Path::new(&archived.archived_instance_root) != archived_root {
        return Err("native archive is not available for restoration".into());
    }
    let config = std::fs::read(archived_root.join("config/instance.json"))?;
    interrupt_restore_commit(
        runtime,
        previous,
        &archived.archive_id,
        archived_root,
        &config,
        expected_saves,
        expected_programs,
    )
    .await?;
    let restored = management::restore_instance_archive(
        runtime.app.clone(),
        management::InstanceArchiveInput {
            archive_id: archived.archive_id.clone(),
        },
    )
    .await?;
    println!(
        "NATIVE_LIFECYCLE module={} phase=archive_restored",
        previous.summary.module_id
    );
    // Large command futures otherwise compound the nested debug polling stack.
    let details = Box::pin(super::read_instance_details_from_storage(
        (*state).clone(),
        id.clone(),
    ))
    .await?;
    if restored.instance_id != *id
        || archived_root.exists()
        || std::fs::read(&details.config_file_path)? != config
        || details.settings_json != previous.settings_json
        || serde_json::to_value(&details.ports)? != serde_json::to_value(&previous.ports)?
        || details.saves_path != previous.saves_path
        || save_inventory(Path::new(&details.saves_path))? != *expected_saves
        || super::inventory::program_files(runtime.descriptor, runtime.effective_install_root)?
            != *expected_programs
        || details.summary.autostart
        || !matches!(details.summary.status, app_core::InstanceStatus::Stopped)
    {
        return Err(
            "native archive restoration did not preserve the stopped instance contract".into(),
        );
    }
    println!(
        "NATIVE_LIFECYCLE module={} phase=archive_readback state=stopped ownership=verified interrupted_commit=recovered config_saves_program_bytes=preserved",
        previous.summary.module_id
    );
    Ok(details)
}

async fn interrupt_restore_commit<R: tauri::Runtime>(
    runtime: &super::runtime::NativeRuntime<'_, '_, R>,
    previous: &super::InstanceDetails,
    archive_id: &str,
    archived_root: &Path,
    config: &[u8],
    expected_saves: &BTreeMap<PathBuf, (u64, u64)>,
    expected_programs: &BTreeMap<PathBuf, (u64, u64)>,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::commands::commands_storage_management as management;
    use sqlx::Connection;
    let paths = &runtime.storage.paths;
    let id = &previous.summary.id;
    if !runtime
        .package
        .cleanup_allowed
        .load(std::sync::atomic::Ordering::SeqCst)
        || app_storage::read_active_instance_run(paths, id)
            .await?
            .is_some()
    {
        return Err("native commit interruption requires a confirmed stopped fixture".into());
    }
    super::seed_probe::checked_path(&runtime.package.root, &paths.database_path, false)?;
    super::seed_probe::checked_path(&runtime.package.root, archived_root, true)?;
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&paths.database_path)
        .foreign_keys(true);
    let mut connection = sqlx::SqliteConnection::connect_with(&options).await?;
    let mut setup = connection.begin().await?;
    // The deferred FK fails the real COMMIT, after recovery restored files and
    // moved the directory. The target row limits injection to this one archive.
    for statement in [
        "CREATE TABLE native_restore_commit_parent (id INTEGER PRIMARY KEY, archive_id TEXT NOT NULL)",
        "CREATE TABLE native_restore_commit_child (parent_id INTEGER REFERENCES native_restore_commit_parent(id) DEFERRABLE INITIALLY DEFERRED)",
        "CREATE TRIGGER native_restore_commit_failure AFTER UPDATE OF state ON instance_archives WHEN NEW.state='restored' AND EXISTS (SELECT 1 FROM native_restore_commit_parent WHERE archive_id=NEW.archive_id) BEGIN INSERT INTO native_restore_commit_child VALUES (1); END",
    ] {
        sqlx::query(statement).execute(&mut *setup).await?;
    }
    sqlx::query("INSERT INTO native_restore_commit_parent VALUES (0, ?1)")
        .bind(archive_id)
        .execute(&mut *setup)
        .await?;
    setup.commit().await?;
    let attempted = management::restore_instance_archive(
        runtime.app.clone(),
        management::InstanceArchiveInput {
            archive_id: archive_id.into(),
        },
    )
    .await;
    // Remove only the injection that this transaction created, including when
    // restoration failed earlier or unexpectedly succeeded. Preserve both errors.
    let cleanup = async {
        let mut tx = connection.begin().await?;
        for statement in [
            "DROP TRIGGER native_restore_commit_failure",
            "DROP TABLE native_restore_commit_child",
            "DROP TABLE native_restore_commit_parent",
        ] {
            sqlx::query(statement).execute(&mut *tx).await?;
        }
        tx.commit().await
    }
    .await;
    if let Err(error) = cleanup {
        return Err(format!(
            "native restore fault cleanup failed: {error}; restore result: {attempted:?}"
        )
        .into());
    }
    let message = attempted
        .err()
        .ok_or("native restoration bypassed the injected commit failure")?;
    let (state, problem, registered): (String, Option<String>, i64) = sqlx::query_as(
        "SELECT state,problem,(SELECT COUNT(*) FROM instances WHERE id=?2) FROM instance_archives WHERE archive_id=?1",
    ).bind(archive_id).bind(id).fetch_one(&mut connection).await?;
    connection.close().await?;
    let original = super::creation::instance_root(previous)?;
    if !message
        .to_ascii_lowercase()
        .contains("foreign key constraint failed")
        || state != "restoring"
        || problem.as_deref().is_none_or(str::is_empty)
        || registered != 0
        || !original.is_dir()
        || archived_root.exists()
        || std::fs::read(&previous.config_file_path)? != config
        || save_inventory(Path::new(&previous.saves_path))? != *expected_saves
        || super::inventory::program_files(runtime.descriptor, runtime.effective_install_root)?
            != *expected_programs
    {
        return Err(format!("native commit interruption missed its recoverable boundary: state={state}, registered={registered}, error={message}").into());
    }
    println!(
        "NATIVE_LIFECYCLE module={} phase=archive_restore_interrupted simulation=deferred_commit_failure journal=restoring instance_row=absent config_saves_program_bytes=preserved",
        previous.summary.module_id,
    );
    Ok(())
}

pub(super) async fn delete_restored<R: tauri::Runtime>(
    runtime: &super::runtime::NativeRuntime<'_, '_, R>,
    previous: &super::InstanceDetails,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::commands::commands_storage_management as management;
    let state = runtime.state;
    let id = &previous.summary.id;
    let paths = &runtime.storage.paths;
    let root = super::creation::instance_root(previous)?.to_owned();
    super::seed_probe::checked_path(&runtime.package.root, &root, true)?;
    let program = super::creation::program_root(previous)?;
    let retained_program = app_storage::instance_uses_library_program(&root)?;
    let library = app_storage::read_library_program_install(paths, &previous.summary.module_id)
        .await?
        .ok_or("native deletion has no registered library")?;
    let library_files = if library.install_root.is_dir() {
        Some(super::inventory::program_files(
            runtime.descriptor,
            &library.install_root,
        )?)
    } else {
        None
    };
    let deleted = Box::pin(super::delete_instance_record(
        runtime.app.clone(),
        id.clone(),
    ))
    .await?;
    let after = app_storage::read_library_program_install(paths, &previous.summary.module_id)
        .await?
        .ok_or("native deletion removed the library record")?;
    if root.exists()
        || Path::new(&deleted.deleted_instance_root).exists()
        || app_storage::read_instance_program_install(paths, id)
            .await?
            .is_some()
        || (!retained_program && program.exists())
        || (retained_program
            && (!program.is_dir() || after.install_state != app_core::InstallState::Installed))
        || after.id != library.id
        || after.install_root != library.install_root
        || after.current_version != library.current_version
        || after.install_state != library.install_state
    {
        return Err(
            "native deletion did not release only the selected instance and its owned program"
                .into(),
        );
    }
    if let Some(expected) = library_files
        && super::inventory::program_files(runtime.descriptor, &after.install_root)? != expected
    {
        return Err("native deletion changed retained library program bytes".into());
    }
    let listed = management::list_instance_archives((*state).clone()).await?;
    if listed
        .pending_deletions
        .iter()
        .any(|item| item.instance_id == *id)
    {
        return Err("native deletion left an unfinished cleanup task".into());
    }
    println!(
        "NATIVE_LIFECYCLE module={} phase=instance_deleted program={} library_record=preserved cleanup=verified",
        previous.summary.module_id,
        if retained_program {
            "retained_installation"
        } else {
            "independent_program_removed"
        },
    );
    Ok(())
}

pub(super) async fn verify_retirement_with_library<R: tauri::Runtime>(
    runtime: &super::runtime::NativeRuntime<'_, '_, R>,
    kept: &super::InstanceDetails,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::commands::commands_storage_management as management;
    let paths = &runtime.storage.paths;
    let module_id = &kept.summary.module_id;
    let previous_library = app_storage::read_library_program_install(paths, module_id)
        .await?
        .ok_or("native retirement requires an installed library")?;
    let previous_library_files =
        super::inventory::program_files(runtime.descriptor, &previous_library.install_root)?;
    let kept_world = save_inventory(Path::new(&kept.saves_path))?;
    let kept_config = std::fs::read(&kept.config_file_path)?;
    let input = app_core::CreateInstanceInput {
        name: "Native retirement isolation".into(),
        module_id: module_id.clone(),
    };
    let acquisition = super::creation::CreationMode::from_env(runtime.descriptor)?.acquisition;
    let created = match acquisition {
        super::creation::Acquisition::Official => {
            crate::commands::commands_storage::create_instance_record(
                (*runtime.state).clone(),
                input,
                Some(app_core::InstanceProgramMode::Independent),
            )
            .await?
        }
        super::creation::Acquisition::LocalPackage => {
            app_storage::create_instance_with_options(
                paths,
                runtime.descriptor,
                input,
                app_storage::InstanceCreationOptions {
                    program_mode: Some(app_core::InstanceProgramMode::Independent),
                    ..Default::default()
                },
            )
            .await?
            .provisioning
        }
    };
    let library = app_storage::read_library_program_install(paths, module_id)
        .await?
        .ok_or("native creation removed its installed library")?;
    if library.id != previous_library.id
        && (acquisition != super::creation::Acquisition::Official
            || library.install_state != app_core::InstallState::Installed
            || !app_storage::library_program_is_pristine(
                &library.install_root,
                runtime.descriptor,
                None,
            )?)
    {
        return Err(
            "native retirement creation did not establish an intact official source".into(),
        );
    }
    let library_files = super::inventory::program_files(runtime.descriptor, &library.install_root)?;
    let details = app_storage::read_instance_details(paths, &created.summary.id).await?;
    let root = super::creation::instance_root(&details)?.to_owned();
    let saves = super::seed_probe::checked_path(
        &runtime.package.root,
        Path::new(&details.saves_path),
        true,
    )?;
    if !saves.starts_with(std::fs::canonicalize(&root)?) {
        return Err("native retirement sentinel requires instance-owned saves".into());
    }
    let sentinel = saves.join("retirement-world.fixture");
    std::fs::write(&sentinel, b"disposable retirement world")?;
    let backup = app_storage::create_instance_backup(paths, &created.summary.id).await?;
    let archived =
        super::archive_instance_record(runtime.app.clone(), created.summary.id.clone()).await?;
    let listed = management::list_instance_archives((*runtime.state).clone()).await?;
    let summary = listed
        .archives
        .iter()
        .find(|item| item.archive_id == archived.archive_id)
        .ok_or("native archive was not listed")?;
    let archive_root = Path::new(
        archived
            .archived_instance_root
            .as_ref()
            .ok_or("native archive is missing")?,
    );
    if std::fs::read(
        archive_root
            .join("backups")
            .join(&backup.backup_id)
            .join("saves/retirement-world.fixture"),
    )? != b"disposable retirement world"
    {
        return Err("native archive lost its saved world backup".into());
    }
    println!(
        "NATIVE_LIFECYCLE module={module_id} phase=archive_with_library program_storage={} omitted_bytes={} library=preserved",
        summary.program_storage, summary.omitted_program_bytes
    );
    management::restore_instance_archive(
        runtime.app.clone(),
        management::InstanceArchiveInput {
            archive_id: archived.archive_id,
        },
    )
    .await?;
    if std::fs::read(&sentinel)? != b"disposable retirement world" {
        return Err("native archive restoration changed its world bytes".into());
    }
    let removed =
        super::delete_instance_record(runtime.app.clone(), created.summary.id.clone()).await?;
    if Path::new(&removed.deleted_instance_root).exists() || root.exists() || archive_root.exists()
    {
        return Err("native permanent deletion left instance-owned files".into());
    }
    let remaining = management::list_instance_archives((*runtime.state).clone()).await?;
    let after = app_storage::read_library_program_install(paths, module_id)
        .await?
        .ok_or("native deletion removed the library record")?;
    let retained_previous =
        app_storage::read_program_install_owner(paths, &previous_library.install_root)
            .await?
            .ok_or("native retirement removed the peer's previous library")?;
    if after.id != library.id
        || after.install_state != app_core::InstallState::Installed
        || after.current_version != library.current_version
        || super::inventory::program_files(runtime.descriptor, &after.install_root)?
            != library_files
        || retained_previous.id != previous_library.id
        || retained_previous.current_version != previous_library.current_version
        || retained_previous.install_state != previous_library.install_state
        || super::inventory::program_files(runtime.descriptor, &previous_library.install_root)?
            != previous_library_files
        || std::fs::read(&kept.config_file_path)? != kept_config
        || save_inventory(Path::new(&kept.saves_path))? != kept_world
        || remaining
            .archives
            .iter()
            .any(|entry| entry.instance_id.as_ref() == Some(&created.summary.id))
        || remaining
            .pending_deletions
            .iter()
            .any(|entry| entry.instance_id == created.summary.id)
    {
        return Err(
            "native archive/delete changed the library, peer, or left unfinished cleanup".into(),
        );
    }
    println!(
        "NATIVE_LIFECYCLE module={module_id} phase=permanent_delete library=installed peer=preserved owned_files=removed"
    );
    Ok(())
}

#[test]
fn native_archive_checks_moved_native_config_and_preserved_external_saves() {
    use std::fs;
    let root = std::env::temp_dir().join(format!("native-archive-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let instance = root.join("instance");
    let native_config = instance.join("runtime/Configs/server.ini");
    let external_saves = root.join("external-saves");
    let archive = root.join("archive");
    fs::create_dir_all(native_config.parent().unwrap()).unwrap();
    fs::create_dir(&external_saves).unwrap();
    fs::write(&native_config, b"native config").unwrap();
    fs::write(external_saves.join("world.sav"), b"saved world").unwrap();
    let config = save_inventory(&native_config).unwrap();
    let saves = save_inventory(&external_saves).unwrap();
    fs::rename(&instance, &archive).unwrap();
    verify_preserved(&native_config, &instance, &archive, &config, "config").unwrap();
    verify_preserved(&external_saves, &instance, &archive, &saves, "saves").unwrap();
    fs::write(
        archive.join("runtime/Configs/server.ini"),
        b"changed config",
    )
    .unwrap();
    assert!(verify_preserved(&native_config, &instance, &archive, &config, "config").is_err());
    fs::remove_file(external_saves.join("world.sav")).unwrap();
    assert!(verify_preserved(&external_saves, &instance, &archive, &saves, "saves").is_err());
    assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
    fs::remove_dir_all(root).unwrap();
}
