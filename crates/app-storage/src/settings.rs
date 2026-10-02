use super::*;
use crate::atomic_file::{read_optional_file_to_string, write_file_atomically};

fn normalize_settings_path_string(value: &str, fallback: &Path) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return fallback.to_string_lossy().into_owned();
    }

    PathBuf::from(trimmed).to_string_lossy().into_owned()
}

fn normalized_app_settings(settings: AppSettings, defaults: &StoragePaths) -> AppSettings {
    let servers_root =
        normalize_settings_path_string(&settings.servers_root, &defaults.instances_root);
    let archives_root = normalize_settings_path_string(
        &settings.archives_root,
        &PathBuf::from(&servers_root).join(".trash"),
    );
    AppSettings {
        servers_root,
        archives_root,
        games_root: normalize_settings_path_string(&settings.games_root, &defaults.games_root),
        modules_root: defaults.modules_root.to_string_lossy().into_owned(),
        steamcmd_root: normalize_settings_path_string(
            &settings.steamcmd_root,
            &defaults.steamcmd_root,
        ),
    }
}

fn load_persisted_app_settings(
    defaults: &StoragePaths,
) -> Result<Option<AppSettings>, StorageError> {
    let Some(raw) = read_optional_file_to_string(&defaults.settings_path).map_err(|source| {
        StorageError::ReadConfig {
            path: defaults.settings_path.clone(),
            source,
        }
    })?
    else {
        return Ok(None);
    };
    if raw.trim().is_empty() {
        return Ok(None);
    }
    let settings = serde_json::from_str::<AppSettings>(&raw).map_err(|source| {
        StorageError::InvalidConfigJson {
            path: defaults.settings_path.clone(),
            source,
        }
    })?;
    Ok(Some(normalized_app_settings(settings, defaults)))
}

pub fn save_app_settings(settings: AppSettings) -> Result<AppSettings, StorageError> {
    let defaults = StoragePaths::resolve_default()?;
    save_app_settings_with_paths(settings, &defaults)
}

pub(crate) fn save_app_settings_with_paths(
    settings: AppSettings,
    defaults: &StoragePaths,
) -> Result<AppSettings, StorageError> {
    let normalized = normalized_app_settings(settings, defaults);
    prepare_app_settings_roots(defaults, &normalized)?;

    if let Some(parent) = defaults.settings_path.parent() {
        fs::create_dir_all(parent).map_err(|source| StorageError::CreatePath {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let payload = serde_json::to_string_pretty(&normalized)?;
    write_file_atomically(&defaults.settings_path, payload.as_bytes()).map_err(|source| {
        StorageError::WriteConfig {
            path: defaults.settings_path.clone(),
            source,
        }
    })?;

    Ok(normalized)
}

pub fn bootstrap_storage() -> Result<StorageBootstrap, StorageError> {
    bootstrap_storage_with_paths(StoragePaths::resolve_default()?)
}

pub fn bootstrap_storage_with_paths(
    default_paths: StoragePaths,
) -> Result<StorageBootstrap, StorageError> {
    let settings =
        load_persisted_app_settings(&default_paths)?.unwrap_or_else(|| default_paths.settings());
    let paths = default_paths.with_app_settings(&settings);

    // Directory reconciliation must observe a missing container before startup
    // can recreate it and make all of its registered instances appear deleted.
    if paths
        .database_path
        .try_exists()
        .map_err(|source| StorageError::ReadPath {
            path: paths.database_path.clone(),
            source,
        })?
    {
        for root in [&paths.instances_root, &paths.archives_root] {
            if !crate::instance_archive_files::plain_directory(root)? {
                return Err(StorageError::ReadPath {
                    path: root.clone(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "Configured instance storage directory is missing.",
                    ),
                });
            }
        }
    }

    for path in [
        paths.app_data_root.clone(),
        paths
            .database_path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| paths.app_data_root.join("db")),
        paths.logs_root.clone(),
        paths.steamcmd_root.clone(),
        paths.games_root.clone(),
        paths.instances_root.clone(),
        paths.archives_root.clone(),
    ] {
        fs::create_dir_all(&path).map_err(|source| StorageError::CreatePath { path, source })?;
    }

    Ok(StorageBootstrap {
        settings,
        storage_status: paths.probe_status(),
        paths,
    })
}

/// Prepare newly selected roots before persisting them. Saving unrelated settings
/// must not recreate a missing container and turn its instances into deletions.
fn prepare_app_settings_roots(
    defaults: &StoragePaths,
    settings: &AppSettings,
) -> Result<(), StorageError> {
    use crate::instance_isolation::paths::{contains, normalize_path};

    let previous = load_persisted_app_settings(defaults)?.unwrap_or_else(|| defaults.settings());
    let current = defaults.with_app_settings(&previous);
    let next = defaults.with_app_settings(settings);
    crate::instance_archive_roots::validate_archive_root(&next)?;
    let existing_database =
        defaults
            .database_path
            .try_exists()
            .map_err(|source| StorageError::ReadPath {
                path: defaults.database_path.clone(),
                source,
            })?;
    for (old_root, new_root) in [
        (&current.instances_root, &next.instances_root),
        (&current.archives_root, &next.archives_root),
    ] {
        let old = normalize_path(old_root)?;
        let new = normalize_path(new_root)?;
        if existing_database
            && contains(&old, &new)
            && contains(&new, &old)
            && !crate::instance_archive_files::plain_directory(new_root)?
        {
            return Err(StorageError::ReadPath {
                path: new_root.clone(),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Configured instance storage directory is missing.",
                ),
            });
        }
    }
    for root in [&next.instances_root, &next.archives_root] {
        fs::create_dir_all(root).map_err(|source| StorageError::CreatePath {
            path: root.clone(),
            source,
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_paths(root: &Path) -> StoragePaths {
        let app_data_root = root.join("current");
        StoragePaths {
            app_data_root: app_data_root.clone(),
            settings_path: app_data_root.join("settings.json"),
            database_path: app_data_root.join("db").join("lgs.db"),
            logs_root: app_data_root.join("logs"),
            modules_root: root.join("bundled-modules"),
            migrations_root: root.join("migrations"),
            steamcmd_root: root.join("runtime").join("cmd").join("steamcmd"),
            games_root: root.join("runtime").join("server-files"),
            instances_root: root.join("runtime").join("instances"),
            archives_root: root.join("runtime").join("instances").join(".trash"),
        }
    }

    #[test]
    fn bootstrap_does_not_import_settings_from_a_retired_directory() {
        let root = std::env::temp_dir().join(format!(
            "lsgm-settings-roots-{}",
            Uuid::new_v4().as_simple()
        ));
        let paths = test_paths(&root);
        let retired_path = root.join("legacy/settings.json");
        fs::create_dir_all(retired_path.parent().unwrap()).unwrap();
        fs::write(&retired_path, b"retired invalid settings must not be read").unwrap();
        let before = fs::read(&retired_path).unwrap();
        let bootstrapped = bootstrap_storage_with_paths(paths.clone()).unwrap();
        assert_eq!(
            bootstrapped.settings.servers_root,
            paths.instances_root.to_string_lossy()
        );
        assert_eq!(
            bootstrapped.settings.modules_root,
            paths.modules_root.to_string_lossy()
        );
        assert!(!paths.settings_path.exists());
        assert_eq!(fs::read(&retired_path).unwrap(), before);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn explicit_bootstrap_uses_only_the_supplied_storage_roots() {
        let root = std::env::temp_dir().join(format!(
            "lsgm-explicit-storage-{}",
            Uuid::new_v4().as_simple()
        ));
        let paths = test_paths(&root);

        let bootstrapped = bootstrap_storage_with_paths(paths.clone()).unwrap();

        assert_eq!(bootstrapped.paths.app_data_root, paths.app_data_root);
        assert_eq!(bootstrapped.paths.games_root, paths.games_root);
        assert_eq!(bootstrapped.paths.instances_root, paths.instances_root);
        assert!(bootstrapped.paths.database_path.parent().unwrap().is_dir());
        assert!(bootstrapped.paths.games_root.is_dir());
        assert!(bootstrapped.paths.instances_root.is_dir());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn bootstrap_preserves_registration_when_existing_instance_container_disappears() {
        let root = std::env::temp_dir().join(format!(
            "lgsm-missing-instance-container-{}",
            Uuid::new_v4()
        ));
        let paths = test_paths(&root);
        bootstrap_storage_with_paths(paths.clone()).unwrap();
        fs::write(&paths.database_path, b"existing database sentinel").unwrap();
        let relocated = root.join("relocated-instances");
        fs::rename(&paths.instances_root, &relocated).unwrap();
        let result = bootstrap_storage_with_paths(paths.clone());
        let saved = save_app_settings_with_paths(paths.settings(), &paths);
        let recreated = paths.instances_root.exists();
        assert_eq!(
            fs::read(&paths.database_path).unwrap(),
            b"existing database sentinel"
        );
        fs::remove_dir_all(&root).unwrap();
        assert!(
            result.is_err(),
            "unavailable instance storage must be surfaced before any reconciliation"
        );
        assert!(
            saved.is_err(),
            "unchanged settings must not repair missing storage"
        );
        assert!(
            !recreated,
            "bootstrap must not recreate a missing parent and turn every instance into an apparent deletion"
        );
    }

    #[test]
    fn old_settings_without_archive_root_keep_their_existing_custom_instance_archive() {
        let root = std::env::temp_dir().join(format!("lgsm-archive-settings-{}", Uuid::new_v4()));
        let paths = test_paths(&root);
        let instances = root.join("original-custom-instances");
        let archive = instances.join(".trash/retained-world");
        fs::create_dir_all(&archive).unwrap();
        fs::write(archive.join("world.dat"), b"existing world").unwrap();
        fs::create_dir_all(&paths.app_data_root).unwrap();
        let mut json = serde_json::to_value(paths.settings()).unwrap();
        json.as_object_mut().unwrap().remove("archives_root");
        json["servers_root"] = serde_json::json!(instances.to_string_lossy());
        fs::write(&paths.settings_path, serde_json::to_vec(&json).unwrap()).unwrap();
        let loaded = bootstrap_storage_with_paths(paths.clone()).unwrap();
        assert_eq!(loaded.paths.archives_root, instances.join(".trash"));
        assert_eq!(
            loaded.settings.archives_root,
            instances.join(".trash").to_string_lossy()
        );
        assert_eq!(
            fs::read(archive.join("world.dat")).unwrap(),
            b"existing world"
        );
        save_app_settings_with_paths(loaded.settings, &paths).unwrap();
        let restarted = bootstrap_storage_with_paths(paths).unwrap();
        assert_eq!(restarted.paths.archives_root, instances.join(".trash"));
        assert_eq!(
            fs::read(archive.join("world.dat")).unwrap(),
            b"existing world"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
