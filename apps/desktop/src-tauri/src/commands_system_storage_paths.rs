use super::*;

pub(super) async fn monitored_storage_paths(settings: &AppSettings) -> Result<Vec<String>, String> {
    let storage = bootstrap_storage()
        .map_err(|error| format!("read storage paths for telemetry: {error}"))?;
    let instance_paths = app_storage::read_instance_storage_paths(&storage.paths)
        .await
        .map_err(|error| format!("read instance paths for telemetry: {error}"))?;
    let mut paths = configured_storage_paths(settings, &storage.paths);
    paths.extend(
        instance_paths
            .into_iter()
            .map(|path| path.to_string_lossy().into_owned()),
    );
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn configured_storage_paths(
    settings: &AppSettings,
    storage: &app_storage::StoragePaths,
) -> Vec<String> {
    [
        (&settings.games_root, &storage.games_root),
        (&settings.servers_root, &storage.instances_root),
        (&settings.archives_root, &storage.archives_root),
        (&settings.steamcmd_root, &storage.steamcmd_root),
    ]
    .into_iter()
    .map(|(configured, fallback)| {
        if configured.trim().is_empty() {
            fallback.to_string_lossy().into_owned()
        } else {
            configured.clone()
        }
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_configured_storage_roots_are_monitored_and_empty_archive_uses_resolved_root() {
        let mut settings = AppSettings {
            games_root: "C:/games".into(),
            servers_root: "D:/servers".into(),
            archives_root: "E:/archives".into(),
            steamcmd_root: "F:/steamcmd".into(),
            ..AppSettings::default()
        };
        let storage = app_storage::StoragePaths {
            archives_root: PathBuf::from("G:/resolved-archives"),
            ..app_storage::StoragePaths::default()
        };
        let paths = configured_storage_paths(&settings, &storage);
        assert_eq!(
            paths,
            ["C:/games", "D:/servers", "E:/archives", "F:/steamcmd"]
        );
        settings.archives_root.clear();
        assert_eq!(
            configured_storage_paths(&settings, &storage)[2],
            "G:/resolved-archives"
        );
    }
}
