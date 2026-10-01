use super::*;

struct Fixture {
    root: PathBuf,
    executable: PathBuf,
    repository: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = env::temp_dir().join(format!("lgsm-modules-root-{}", uuid::Uuid::new_v4()));
        let executable = root.join("export/langame-desktop.exe");
        let repository = root.join("source");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::create_dir_all(repository.join("modules")).unwrap();
        fs::write(&executable, b"fixture executable path").unwrap();
        Self {
            root,
            executable,
            repository,
        }
    }

    fn bundled(&self) -> PathBuf {
        self.executable.parent().unwrap().join("modules")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn modules_root_prefers_deployed_bundle_over_debug_repository() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.bundled()).unwrap();
    fs::write(fixture.bundled().join("frozen-module"), b"exported").unwrap();
    fs::write(
        fixture.repository.join("modules/frozen-module"),
        b"mutable source",
    )
    .unwrap();
    let resolved = resolve_modules_root(Some(&fixture.executable), Some(&fixture.repository));
    assert_eq!(resolved, fixture.bundled());
    assert_eq!(
        fs::read(resolved.join("frozen-module")).unwrap(),
        b"exported"
    );
}

#[test]
fn modules_root_preserves_debug_repository_fallback_without_a_bundle() {
    let fixture = Fixture::new();
    assert_eq!(
        resolve_modules_root(Some(&fixture.executable), Some(&fixture.repository)),
        fixture.repository.join("modules")
    );
    fs::write(fixture.bundled(), b"not a module directory").unwrap();
    assert_eq!(
        resolve_modules_root(Some(&fixture.executable), Some(&fixture.repository)),
        fixture.repository.join("modules")
    );
}

#[test]
fn modules_root_preserves_release_and_missing_executable_fallbacks() {
    let fixture = Fixture::new();
    assert_eq!(
        resolve_modules_root(Some(&fixture.executable), None),
        fixture.bundled()
    );
    assert_eq!(
        resolve_modules_root(None, Some(&fixture.repository)),
        fixture.repository.join("modules")
    );
    assert_eq!(resolve_modules_root(None, None), PathBuf::from("modules"));
}

#[test]
fn modules_root_settings_cannot_redirect_the_selected_bundle() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.bundled()).unwrap();
    let paths = StoragePaths {
        app_data_root: fixture.root.join("appdata"),
        settings_path: fixture.root.join("appdata/settings.json"),
        database_path: fixture.root.join("appdata/db/lgs.db"),
        logs_root: fixture.root.join("appdata/logs"),
        modules_root: fixture.bundled(),
        migrations_root: fixture.root.join("migrations"),
        steamcmd_root: fixture.root.join("runtime/cmd/steamcmd"),
        games_root: fixture.root.join("runtime/server-files"),
        instances_root: fixture.root.join("runtime/instances"),
        archives_root: fixture.root.join("runtime/instances/.trash"),
    };
    let mut supplied = paths.settings();
    supplied.modules_root = fixture
        .repository
        .join("modules")
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        paths.with_app_settings(&supplied).modules_root,
        fixture.bundled()
    );
    let saved = settings::save_app_settings_with_paths(supplied, &paths).unwrap();
    assert_eq!(PathBuf::from(saved.modules_root), fixture.bundled());
    let persisted: AppSettings =
        serde_json::from_slice(&fs::read(&paths.settings_path).unwrap()).unwrap();
    assert_eq!(PathBuf::from(persisted.modules_root), fixture.bundled());
}
