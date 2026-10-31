//! Keep exported native-test binaries bound to their reviewed source resources.
use super::*;

pub(super) fn prepare(
    settings: AppSettings,
) -> Result<(StorageBootstrap, DesktopState), app_storage::StorageError> {
    let source = workspace_root();
    let mut paths = app_storage::StoragePaths::resolve_default()?;
    // Production deliberately ignores a modules_root supplied in AppSettings.
    // Bind the test's source override at the storage boundary instead, including
    // commands that bootstrap storage again during archive/restore or startup.
    paths.modules_root = source.join("modules");
    paths.migrations_root = source.join("migrations");
    app_storage::save_app_settings_with_paths(settings, &paths)?;
    let storage = app_storage::bootstrap_storage_with_paths(paths)?;
    let desktop = DesktopState::from_storage(&storage);
    Ok((storage, desktop))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHILD: &str = "commands::tests::manual_runtime_tests::native_lifecycle_tests::storage_fixture::tests::relocated_storage_process_fixture";
    const ROOT_VARIABLE: &str = "LANGAME_NATIVE_STORAGE_TEST_ROOT";

    #[tokio::test]
    async fn exported_native_fixture_keeps_its_source_resources() {
        let root = crate::commands::tests::temp_test_dir("native-storage");
        let source = root.join("reviewed-source");
        fs::create_dir_all(source.join("modules")).unwrap();
        fs::create_dir_all(source.join("migrations")).unwrap();
        let stdout_path = root.join("stdout.log");
        let stderr_path = root.join("stderr.log");
        // Only this child changes user-data variables. No game or network tool
        // is launched, and the parent test process retains its environment.
        let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                CHILD,
                "--ignored",
                "--test-threads=1",
                "--nocapture",
            ])
            .env(ROOT_VARIABLE, &root)
            .env("LANGAME_SMOKE_SOURCE_ROOT", &source)
            .stdout(fs::File::create(&stdout_path).unwrap())
            .stderr(fs::File::create(&stderr_path).unwrap())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let completed = tokio::time::timeout(Duration::from_secs(60), child.wait()).await;
        let success = matches!(&completed, Ok(Ok(status)) if status.success());
        if !success && child.try_wait().unwrap().is_none() {
            child.kill().await.unwrap();
            child.wait().await.unwrap();
        }
        let stdout = fs::read_to_string(&stdout_path).unwrap();
        let stderr = fs::read_to_string(&stderr_path).unwrap();
        assert!(
            success && stdout.contains("test result: ok. 1 passed;"),
            "relocated native storage failed ({completed:?}); retained {}\n{stdout}\n{stderr}",
            root.display(),
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "child entry: changes only its own user-data environment"]
    fn relocated_storage_process_fixture() -> Result<(), Box<dyn std::error::Error>> {
        let root = PathBuf::from(
            std::env::var_os(ROOT_VARIABLE).ok_or("missing native storage fixture root")?,
        );
        let _environment = ProgramDataEnvGuard::set(&root.join("programdata"));
        let source = root.join("reviewed-source").canonicalize()?;
        assert_eq!(workspace_root(), source);
        assert_ne!(
            app_storage::StoragePaths::resolve_default()?.modules_root,
            source.join("modules")
        );
        let (storage, desktop) = prepare(AppSettings {
            servers_root: root.join("instances").to_string_lossy().into_owned(),
            archives_root: String::new(),
            games_root: root.join("games").to_string_lossy().into_owned(),
            steamcmd_root: root.join("steamcmd").to_string_lossy().into_owned(),
            modules_root: root
                .join("ignored-settings-value")
                .to_string_lossy()
                .into_owned(),
        })?;
        let reloaded = desktop.bootstrap_storage()?;
        for actual in [&storage, &reloaded] {
            assert_eq!(actual.paths.modules_root, source.join("modules"));
            assert_eq!(actual.paths.migrations_root, source.join("migrations"));
            assert_eq!(
                Path::new(&actual.settings.modules_root),
                actual.paths.modules_root
            );
            assert!(
                actual
                    .paths
                    .app_data_root
                    .starts_with(root.join("localappdata"))
            );
        }
        assert_eq!(storage.paths.database_path, reloaded.paths.database_path);
        Ok(())
    }
}
