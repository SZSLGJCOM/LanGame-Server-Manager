use super::*;
use crate::{GameInstallSyncRecord, initialize_database, sync_game_installs, sync_modules};

#[path = "program_library_cleanup_recovery_tests.rs"]
mod recovery;

struct Fixture {
    root: PathBuf,
    paths: StoragePaths,
    descriptor: ModuleDescriptor,
}

impl Fixture {
    async fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("langame-library-cleanup-{}", uuid::Uuid::new_v4()));
        let paths = StoragePaths {
            app_data_root: root.join("appdata"),
            settings_path: root.join("appdata/settings.json"),
            database_path: root.join("appdata/db/store.db"),
            logs_root: root.join("appdata/logs"),
            modules_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules"),
            migrations_root: root.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances/.trash"),
        };
        fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
        fs::create_dir_all(&paths.games_root).unwrap();
        let descriptors = app_modules::discover_modules(&paths.modules_root).unwrap();
        let descriptor = descriptors
            .iter()
            .find(|descriptor| descriptor.summary.id == "palworld")
            .unwrap()
            .clone();
        initialize_database(&paths).await.unwrap();
        sync_modules(&paths, &descriptors).await.unwrap();
        Self {
            root,
            paths,
            descriptor,
        }
    }

    fn directory(&self, name: &str) -> PathBuf {
        let root = self.paths.games_root.join(name);
        fs::create_dir_all(&root).unwrap();
        root
    }

    async fn register(&self, module: &str, root: &Path, state: InstallState) -> i64 {
        sync_game_installs(
            &self.paths,
            &[GameInstallSyncRecord {
                module_id: module.into(),
                install_root: root.to_string_lossy().into_owned(),
                install_state: state,
                current_version: Some("fixture-build".into()),
                mark_verified: true,
            }],
        )
        .await
        .unwrap();
        crate::read_program_install_owner(&self.paths, root)
            .await
            .unwrap()
            .unwrap()
            .id
    }

    fn baseline(&self, root: &Path) {
        crate::record_library_program_baseline(root, &self.descriptor, true, None).unwrap();
    }

    async fn plan(&self, keep: bool) -> LibraryCleanupPlan {
        plan_module_library_cleanup(&self.paths, &self.descriptor, keep)
            .await
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Each test owns its UUID tree; unsafe-tree tests unlink their fixture first.
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[tokio::test]
async fn library_cleanup_plan_excludes_foreign_modules_and_instance_owned_programs() {
    let fixture = Fixture::new().await;
    let empty = fixture.plan(true).await;
    assert!(empty.installations.is_empty());
    assert_eq!(empty.keep_install_id, None);
    fixture
        .register(
            "minecraft",
            &fixture.directory("foreign"),
            InstallState::Installed,
        )
        .await;
    let owned = fixture.directory("private");
    let private_id = fixture
        .register("palworld", &owned, InstallState::Installed)
        .await;
    let library_id = fixture
        .register(
            "palworld",
            &fixture.directory("palworld"),
            InstallState::Installed,
        )
        .await;
    let pool = crate::storage_db::connect_pool(&fixture.paths)
        .await
        .unwrap();
    sqlx::query("INSERT INTO instances(id,name,module_id,install_id,config_path,data_path,logs_path,saves_path) VALUES ('owner','Owner','palworld',?1,'config','data','logs','saves')")
        .bind(private_id).execute(&pool).await.unwrap();
    sqlx::query("UPDATE game_installs SET scope='instance',owner_instance_id='owner' WHERE id=?1")
        .bind(private_id)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let plan = fixture.plan(true).await;
    assert_eq!(
        plan.installations
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        vec![library_id]
    );
    assert_eq!(plan.keep_install_id, Some(library_id));
    assert_eq!(fixture.plan(false).await.keep_install_id, None);
}

#[tokio::test]
async fn library_cleanup_plan_keeps_default_or_oldest_existing_installed_library() {
    let fixture = Fixture::new().await;
    let first = fixture.directory("palworld-original-first");
    let first_id = fixture
        .register("palworld", &first, InstallState::Installed)
        .await;
    let second = fixture.directory("palworld-original-second");
    let second_id = fixture
        .register("palworld", &second, InstallState::Installed)
        .await;
    let default = fixture.directory("palworld");
    let default_id = fixture
        .register("palworld", &default, InstallState::Installed)
        .await;
    fixture
        .register(
            "palworld",
            &fixture.directory("pending"),
            InstallState::Incomplete,
        )
        .await;
    assert_eq!(fixture.plan(true).await.keep_install_id, Some(default_id));
    fs::remove_dir(&default).unwrap();
    assert_eq!(fixture.plan(true).await.keep_install_id, Some(first_id));
    fs::remove_dir(&first).unwrap();
    assert_eq!(fixture.plan(true).await.keep_install_id, Some(second_id));
    fixture
        .register("palworld", &second, InstallState::Incomplete)
        .await;
    assert_eq!(fixture.plan(true).await.keep_install_id, None);
}

#[cfg(windows)]
#[tokio::test]
async fn library_cleanup_single_retained_candidate_does_not_read_program_payload() {
    use std::os::windows::fs::OpenOptionsExt;

    for (name, with_incomplete) in [("palworld", false), ("palworld-original-only", true)] {
        let fixture = Fixture::new().await;
        let root = fixture.directory(name);
        let payload = root.join("server.bin");
        fs::write(&payload, b"official program").unwrap();
        fixture.baseline(&root);
        let id = fixture
            .register("palworld", &root, InstallState::Installed)
            .await;
        if with_incomplete {
            fixture
                .register(
                    "palworld",
                    &fixture.directory("pending"),
                    InstallState::Incomplete,
                )
                .await;
        }

        // A valid baseline makes the old selection path read the payload.
        // Deny that read while leaving the library's metadata available.
        let exclusive = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&payload)
            .unwrap();
        assert_eq!(
            fs::File::open(&payload).unwrap_err().raw_os_error(),
            Some(32)
        );
        let result = plan_module_library_cleanup(&fixture.paths, &fixture.descriptor, true).await;
        drop(exclusive);

        let plan =
            result.expect("the sole retained candidate does not require reading its payload");
        assert_eq!(plan.keep_install_id, Some(id));
        assert_eq!(
            plan.installations.len(),
            if with_incomplete { 2 } else { 1 }
        );
        assert_eq!(fs::read(&payload).unwrap(), b"official program");
    }
}

#[tokio::test]
async fn library_cleanup_retains_modified_unknown_excluded_and_metadata_user_files() {
    let mut fixture = Fixture::new().await;
    let root = fixture.directory("palworld");
    fs::write(root.join("server.bin"), b"official server").unwrap();
    fs::write(root.join("modified.dll"), b"original bytes").unwrap();
    fs::write(root.join("later-excluded.ini"), b"shipped settings").unwrap();
    fs::create_dir_all(root.join("Pal/Saved/Config")).unwrap();
    fs::write(root.join("Pal/Saved/Config/server.ini"), b"private config").unwrap();
    fixture.baseline(&root);
    fixture
        .descriptor
        .storage
        .runtime_copy_exclusions
        .push("later-excluded.ini".into());
    fs::write(root.join("modified.dll"), b"modified bytes").unwrap();
    fs::create_dir_all(root.join("Mods/custom")).unwrap();
    fs::write(root.join("Mods/custom/user.dll"), b"user mod").unwrap();
    fs::write(root.join("world.sav"), b"private save").unwrap();
    fs::create_dir(root.join("empty-personal-directory")).unwrap();
    fs::write(root.join(".langame-personal-notes"), b"user notes").unwrap();
    fs::write(
        root.join(".langame-program-identity.json"),
        serde_json::to_vec(&serde_json::json!({
            "version": 1, "module_id": "palworld", "id": "manager-identity"
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(root.join(".langame-program-usage.json"), serde_json::to_vec(&serde_json::json!({
        "version": 1, "module_id": "palworld", "program_id": "manager-identity", "instance_id": "old-instance", "personal_note": "keep"
    })).unwrap()).unwrap();
    let before = crate::test_file_snapshot::tree_snapshot(&root).unwrap();
    let retained = library_cleanup_retained_paths(&root, &fixture.descriptor)
        .unwrap()
        .unwrap();
    let protected = |relative: &str| {
        let path = fs::canonicalize(root.join(relative)).unwrap();
        retained.iter().any(|keep| contains(keep, &path))
    };
    for path in [
        "modified.dll",
        "later-excluded.ini",
        "Pal/Saved/Config/server.ini",
        "Mods/custom/user.dll",
        "world.sav",
        "empty-personal-directory",
        ".langame-personal-notes",
        ".langame-program-usage.json",
    ] {
        assert!(
            protected(path),
            "untrusted or private bytes must be retained: {path}"
        );
    }
    for path in [
        "server.bin",
        ".langame-clean-package.json",
        ".langame-initial-package.json",
        ".langame-program-identity.json",
    ] {
        assert!(
            !protected(path),
            "verified payload or recognized metadata can be removed: {path}"
        );
    }
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&root).unwrap(),
        before
    );
}

#[tokio::test]
async fn library_cleanup_requires_a_valid_allowlist_for_the_same_module() {
    let fixture = Fixture::new().await;
    let root = fixture.directory("pending");
    fs::write(root.join("partial.bin"), b"unverified partial bytes").unwrap();
    assert!(
        library_cleanup_retained_paths(&root, &fixture.descriptor)
            .unwrap()
            .is_none()
    );
    fixture.baseline(&root);
    assert!(
        library_cleanup_retained_paths(&root, &fixture.descriptor)
            .unwrap()
            .unwrap()
            .is_empty()
    );
    let manifest = root.join(".langame-clean-package.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    value["module_id"] = serde_json::json!("minecraft");
    fs::write(&manifest, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(
        library_cleanup_retained_paths(&root, &fixture.descriptor)
            .unwrap()
            .is_none()
    );
    fs::write(&manifest, b"invalid manifest").unwrap();
    assert!(matches!(
        library_cleanup_retained_paths(&root, &fixture.descriptor),
        Err(StorageError::PrivateRuntimeRefresh { .. })
    ));
    assert_eq!(
        fs::read(root.join("partial.bin")).unwrap(),
        b"unverified partial bytes"
    );
}

#[cfg(any(windows, unix))]
#[tokio::test]
async fn library_cleanup_preflights_links_inside_unknown_retained_subtrees() {
    let fixture = Fixture::new().await;
    let root = fixture.directory("palworld");
    fs::write(root.join("server.bin"), b"official").unwrap();
    fixture.baseline(&root);
    let outside = fixture.directory("outside");
    fs::write(outside.join("save.dat"), b"outside sentinel").unwrap();
    fs::create_dir(root.join("unknown-data")).unwrap();
    let link = root.join("unknown-data/outside-link");
    #[cfg(windows)]
    {
        let output = std::process::Command::new("cmd")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(link.to_string_lossy().replace('/', "\\"))
            .arg(outside.to_string_lossy().replace('/', "\\"))
            .output()
            .unwrap();
        assert!(output.status.success(), "junction fixture: {output:?}");
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    let result = library_cleanup_retained_paths(&root, &fixture.descriptor);
    #[cfg(windows)]
    fs::remove_dir(&link).unwrap();
    #[cfg(unix)]
    fs::remove_file(&link).unwrap();
    assert!(matches!(
        result,
        Err(StorageError::UnsafeManagedPath { .. })
    ));
    assert_eq!(
        fs::read(outside.join("save.dat")).unwrap(),
        b"outside sentinel"
    );
}

#[tokio::test]
async fn library_cleanup_tree_entry_limit_stops_analysis_without_modifying_files() {
    let fixture = Fixture::new().await;
    let root = fixture.directory("bounded");
    for name in ["one", "two", "three"] {
        fs::write(root.join(name), b"keep").unwrap();
    }
    let before = crate::test_file_snapshot::tree_snapshot(&root).unwrap();
    assert!(matches!(
        inspect_tree(&root, 2),
        Err(StorageError::InvalidInstancePath { .. })
    ));
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&root).unwrap(),
        before
    );
}
