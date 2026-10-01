use super::*;
use crate::instance_creation_io::test_gate::{PausePoint, pause_at};

#[cfg(windows)]
#[path = "program_package_verification_tests.rs"]
mod verification;

struct Fixture {
    root: PathBuf,
    descriptor: ModuleDescriptor,
}
impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("langame-program-seed-{}", uuid::Uuid::new_v4()));
        let module = root.join("modules/fixture");
        fs::create_dir_all(&module).unwrap();
        fs::write(
            module.join("module.toml"),
            r#"
id = "fixture"
name = "Clean seed fixture"
version = "1.0.0"
[install]
shared_game_dir = "fixture"
[process]
executable = "server.bin"
[storage]
runtime_copy_exclusions = ["Saved", "mods", "user.cfg"]
saves_path_template = "{{paths.install_root}}/Saved/{{instance.id}}"
"#,
        )
        .unwrap();
        let descriptor = app_modules::discover_modules(root.join("modules"))
            .unwrap()
            .remove(0);
        Self { root, descriptor }
    }
    fn source(&self) -> PathBuf {
        let root = self.root.join("source");
        fs::create_dir_all(root.join("assets/empty")).unwrap();
        fs::write(root.join("server.bin"), b"official executable").unwrap();
        fs::write(root.join("assets/required.bin"), b"official assets").unwrap();
        root
    }
    fn target(&self) -> PathBuf {
        self.root.join("library/fixture")
    }
    fn paths(&self) -> StoragePaths {
        StoragePaths {
            app_data_root: self.root.join("app-data"),
            settings_path: self.root.join("app-data/settings.json"),
            database_path: self.root.join("app-data/db/seed.db"),
            logs_root: self.root.join("logs"),
            modules_root: self.root.join("modules"),
            migrations_root: self.root.join("migrations"),
            steamcmd_root: self.root.join("steamcmd"),
            games_root: self.root.join("library"),
            instances_root: self.root.join("instances"),
            archives_root: self.root.join("instances").join(".trash"),
        }
    }
    fn record(&self, root: &Path) {
        record_library_program_baseline(root, &self.descriptor, true, None).unwrap();
    }
    fn seed(&self, source: &Path) -> Result<CleanLibrarySeed, StorageError> {
        prepare_seed_at(
            &self.target(),
            &self.descriptor,
            &[SeedSource {
                root: source.to_owned(),
                current_version: Some("official-build-1".into()),
            }],
            None,
        )
    }
    fn no_staging(&self) {
        let parent = self.target().parent().unwrap().to_owned();
        if parent.exists() {
            assert!(fs::read_dir(parent).unwrap().all(|entry| {
                !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".langame-seed-")
            }));
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        remove_owned_tree(&self.root).expect("remove owned clean-seed fixture");
    }
}

fn put(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

#[tokio::test]
async fn registered_library_seed_does_not_require_archive_catalog_admission() {
    let fixture = Fixture::new();
    let paths = fixture.paths();
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    crate::initialize_database(&paths).await.unwrap();
    crate::sync_modules(&paths, std::slice::from_ref(&fixture.descriptor))
        .await
        .unwrap();
    let source = fixture.source();
    fixture.record(&source);
    crate::sync_game_installs(
        &paths,
        &[crate::GameInstallSyncRecord {
            module_id: "fixture".into(),
            install_root: source.to_string_lossy().into_owned(),
            install_state: app_core::InstallState::Installed,
            current_version: Some("library-build".into()),
            mark_verified: true,
        }],
    )
    .await
    .unwrap();
    let _inventory = crate::instance_archive::inventory_lock(&paths).unwrap();
    let seed = prepare_clean_library_seed_at(&paths, &fixture.descriptor, &fixture.target(), None)
        .await
        .unwrap();
    assert!(!seed.requires_validation);
    assert_eq!(seed.current_version.as_deref(), Some("library-build"));
    assert_eq!(
        fs::read(seed.install_root.join("server.bin")).unwrap(),
        b"official executable"
    );
}

#[tokio::test]
async fn explicit_seed_target_uses_custom_library_root_without_creating_default() {
    let fixture = Fixture::new();
    let paths = fixture.paths();
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    crate::initialize_database(&paths).await.unwrap();
    let target = fixture.root.join("custom-library/another-name");
    let seed = prepare_clean_library_seed_at(&paths, &fixture.descriptor, &target, None)
        .await
        .unwrap();
    assert_eq!(seed.install_root, fs::canonicalize(&target).unwrap());
    assert!(seed.requires_validation);
    assert!(seed.current_version.is_none());
    let mut entries = fs::read_dir(&target)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(
        entries,
        [
            ACQUISITION,
            crate::program_library_retention::RETAINED_LIBRARY
        ]
        .map(std::ffi::OsString::from)
    );
    assert!(library_program_acquisition_is_trusted(&target, &fixture.descriptor).unwrap());
    assert!(!fixture.target().exists());
    put(&target, "sentinel", b"preserve existing library");
    assert!(
        prepare_clean_library_seed_at(&paths, &fixture.descriptor, &target, None)
            .await
            .is_err()
    );
    assert_eq!(
        fs::read(target.join("sentinel")).unwrap(),
        b"preserve existing library"
    );
    assert_eq!(fs::read_dir(target.parent().unwrap()).unwrap().count(), 1);
}

#[tokio::test]
async fn seed_target_rejects_instance_directory_overlap_and_relative_paths_before_creation() {
    let fixture = Fixture::new();
    let paths = fixture.paths();
    for target in [
        paths.instances_root.clone(),
        paths.instances_root.join("first/runtime"),
        PathBuf::from("relative-library"),
    ] {
        assert!(
            prepare_clean_library_seed_at(&paths, &fixture.descriptor, &target, None)
                .await
                .is_err()
        );
    }
    let mut nested_paths = paths;
    nested_paths.instances_root = fixture.root.join("uncreated-parent/instances");
    let target = fixture.root.join("uncreated-parent");
    assert!(
        prepare_clean_library_seed_at(&nested_paths, &fixture.descriptor, &target, None)
            .await
            .is_err()
    );
    assert!(!target.exists());
    assert!(!fixture.root.join("instances").exists());
    assert!(!nested_paths.database_path.exists());
}

#[tokio::test]
async fn custom_seed_target_rejects_overlapping_owned_installation_from_another_module() {
    let fixture = Fixture::new();
    let paths = fixture.paths();
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    crate::initialize_database(&paths).await.unwrap();
    let pool = crate::storage_db::connect_pool(&paths).await.unwrap();
    let owned = fixture.root.join("custom-owned/runtime");
    sqlx::query("INSERT INTO modules (id,name,version) VALUES ('other','Other','1')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO instances (id,name,module_id,config_path,data_path,logs_path,saves_path) VALUES ('owner','Owner','other','config','data','logs','saves')")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO game_installs (module_id,install_root,scope,owner_instance_id) VALUES ('other',?1,'instance','owner')")
        .bind(owned.to_string_lossy().as_ref()).execute(&pool).await.unwrap();
    pool.close().await;
    for target in [
        owned.clone(),
        owned.join("nested-library"),
        fixture.root.join("custom-owned"),
    ] {
        let result =
            prepare_clean_library_seed_at(&paths, &fixture.descriptor, &target, None).await;
        assert!(
            matches!(result, Err(StorageError::InvalidInstancePath { message, .. }) if message.contains("overlaps"))
        );
        assert!(!target.exists());
    }
    assert!(!fixture.root.join("custom-owned").exists());
}

#[test]
fn unproven_source_and_old_runtime_snapshot_never_become_a_clean_baseline() {
    let fixture = Fixture::new();
    let source = fixture.source();
    put(
        &source,
        ".langame-package-baseline.json",
        br#"{"version":2,"files":{"user-mod.dll":"digest"}}"#,
    );
    put(&source, "mods/user-mod.dll", b"user mod");
    record_library_program_baseline(&source, &fixture.descriptor, false, None).unwrap();
    assert!(!source.join(CLEAN_PACKAGE).exists());
    assert!(read_clean_package_tree(&source, None).unwrap().is_none());
    let seed = fixture.seed(&source).unwrap();
    assert!(seed.requires_validation);
    assert!(seed.current_version.is_none());
    assert_eq!(fs::read_dir(&seed.install_root).unwrap().count(), 1);
    assert!(
        library_program_acquisition_is_trusted(&seed.install_root, &fixture.descriptor).unwrap()
    );
    assert_eq!(
        fs::read(source.join("mods/user-mod.dll")).unwrap(),
        b"user mod"
    );
    fixture.no_staging();
}

#[test]
fn updating_an_existing_program_revokes_the_old_allowlist_without_touching_payload() {
    let fixture = Fixture::new();
    let source = fixture.source();
    fixture.record(&source);
    // A new official version can add required files without changing old ones.
    // Its database version must never certify a seed made from the old list.
    put(
        &source,
        "assets/new-required.bin",
        b"official new version data",
    );
    put(
        &source,
        "extra-mod.dll",
        b"user data must survive revocation",
    );
    record_library_program_baseline(&source, &fixture.descriptor, false, None).unwrap();
    assert!(!source.join(CLEAN_PACKAGE).exists());
    assert!(!library_program_is_pristine(&source, &fixture.descriptor, None).unwrap());
    assert_eq!(
        fs::read(source.join("assets/new-required.bin")).unwrap(),
        b"official new version data"
    );
    assert_eq!(
        fs::read(source.join("extra-mod.dll")).unwrap(),
        b"user data must survive revocation"
    );
    let seed = fixture.seed(&source).unwrap();
    assert!(seed.requires_validation);
    assert!(seed.current_version.is_none());
    assert_eq!(fs::read_dir(&seed.install_root).unwrap().count(), 1);
    assert!(
        library_program_acquisition_is_trusted(&seed.install_root, &fixture.descriptor).unwrap()
    );
}

#[test]
fn pristine_check_requires_matching_module_and_original_bytes_but_does_not_trust_extra_files() {
    let fixture = Fixture::new();
    let source = fixture.source();
    assert!(!library_program_is_pristine(&source, &fixture.descriptor, None).unwrap());
    fixture.record(&source);
    put(&source, "extra-mod.dll", b"untrusted extra file");
    assert!(library_program_is_pristine(&source, &fixture.descriptor, None).unwrap());
    assert!(
        !read_clean_package_tree(&source, None)
            .unwrap()
            .unwrap()
            .files
            .contains_key("extra-mod.dll")
    );
    let mut other = fixture.descriptor.clone();
    other.summary.id = "another-module".into();
    assert!(!library_program_is_pristine(&source, &other, None).unwrap());
    fs::write(source.join("server.bin"), b"patched executable").unwrap();
    assert!(!library_program_is_pristine(&source, &fixture.descriptor, None).unwrap());
    fs::write(source.join("server.bin"), b"official executable").unwrap();
    fs::remove_file(source.join("assets/required.bin")).unwrap();
    assert!(!library_program_is_pristine(&source, &fixture.descriptor, None).unwrap());
    fs::write(source.join(CLEAN_PACKAGE), b"corrupt manifest").unwrap();
    assert!(library_program_is_pristine(&source, &fixture.descriptor, None).is_err());
}

#[tokio::test]
async fn pristine_check_cancels_during_hashing_without_changing_source() {
    let fixture = Fixture::new();
    let source = fixture.source();
    fixture.record(&source);
    let original = fs::read(source.join(CLEAN_PACKAGE)).unwrap();
    let cancellation = Arc::new(AtomicBool::new(false));
    let mut pause = pause_at(&cancellation, PausePoint::Hash);
    let worker_token = Arc::clone(&cancellation);
    let worker_source = source.clone();
    let descriptor = fixture.descriptor.clone();
    let worker = tokio::task::spawn_blocking(move || {
        library_program_is_pristine(&worker_source, &descriptor, Some(&worker_token))
    });
    pause.reached().await;
    cancellation.store(true, Ordering::Release);
    drop(pause);
    assert!(matches!(
        worker.await.unwrap(),
        Err(StorageError::InstanceCreationCancelled)
    ));
    assert_eq!(fs::read(source.join(CLEAN_PACKAGE)).unwrap(), original);
    assert_eq!(
        fs::read(source.join("server.bin")).unwrap(),
        b"official executable"
    );
}

#[test]
fn clean_seed_copies_only_original_matching_files_without_links() {
    let fixture = Fixture::new();
    let source = fixture.source();
    for path in [
        "Saved/world.sav",
        "mods/mod.dll",
        "user.cfg",
        "steamapps/workshop/123/mod.dll",
        ".langame-owned-private",
    ] {
        put(&source, path, b"must not become program payload");
    }
    fixture.record(&source);
    put(&source, "extra-mod.dll", b"added after official install");
    put(&source, "new-world/save.dat", b"added world");
    let manifest = read_manifest(&source).unwrap().unwrap();
    assert_eq!(manifest.files.len(), 2);
    let seed = fixture.seed(&source).unwrap();
    assert!(!seed.requires_validation);
    assert_eq!(seed.current_version.as_deref(), Some("official-build-1"));
    assert!(seed.install_root.join("assets/empty").is_dir());
    for path in [
        "Saved",
        "mods",
        "user.cfg",
        "steamapps/workshop",
        "extra-mod.dll",
        "new-world",
        ".langame-owned-private",
        STAGE_OWNER,
    ] {
        assert!(
            !seed.install_root.join(path).exists(),
            "copied unowned payload {path}"
        );
    }
    assert!(
        read_clean_package_tree(&seed.install_root, None)
            .unwrap()
            .is_some()
    );
    fs::write(seed.install_root.join("server.bin"), b"instance edit").unwrap();
    assert_eq!(
        fs::read(source.join("server.bin")).unwrap(),
        b"official executable"
    );
    fixture.no_staging();
}

#[test]
fn changed_or_missing_official_files_are_omitted_and_require_validation() {
    let fixture = Fixture::new();
    let source = fixture.source();
    put(&source, "assets/keep.bin", b"unchanged official content");
    fixture.record(&source);
    fs::write(source.join("server.bin"), b"loader patched executable").unwrap();
    #[cfg(windows)]
    {
        let mut permissions = fs::metadata(source.join("server.bin"))
            .unwrap()
            .permissions();
        permissions.set_readonly(true);
        fs::set_permissions(source.join("server.bin"), permissions).unwrap();
    }
    fs::remove_file(source.join("assets/required.bin")).unwrap();
    assert!(read_clean_package_tree(&source, None).unwrap().is_none());
    let seed = fixture.seed(&source).unwrap();
    assert!(seed.requires_validation);
    assert!(seed.current_version.is_none());
    assert!(!seed.install_root.join("server.bin").exists());
    assert!(!seed.install_root.join("assets/required.bin").exists());
    assert_eq!(
        fs::read(seed.install_root.join("assets/keep.bin")).unwrap(),
        b"unchanged official content"
    );
    assert_eq!(
        fs::read(source.join("server.bin")).unwrap(),
        b"loader patched executable"
    );
    // Keeping the expected allowlist prevents a partial seed from certifying itself.
    assert_eq!(
        read_manifest(&seed.install_root)
            .unwrap()
            .unwrap()
            .files
            .len(),
        3
    );
    assert!(
        read_clean_package_tree(&seed.install_root, None)
            .unwrap()
            .is_none()
    );
    fixture.no_staging();
}

#[test]
fn dst_seed_excludes_all_workshop_prefixes_and_native_save_prefix() {
    let mut fixture = Fixture::new();
    fixture.descriptor.summary.id = "dontstarve".into();
    fixture.descriptor.storage.runtime_copy_exclusions.clear();
    let source = fixture.source();
    for path in [
        "mods/workshop-123/modmain.lua",
        "mods/workshop-custom/modmain.lua",
        "Saved/first/world.sav",
    ] {
        put(&source, path, b"private data");
    }
    put(
        &source,
        "mods/INSTALLING_MODS.txt",
        b"official documentation",
    );
    fixture.record(&source);
    let seed = fixture.seed(&source).unwrap();
    assert!(!seed.install_root.join("Saved").exists());
    assert!(!seed.install_root.join("mods/workshop-123").exists());
    assert!(!seed.install_root.join("mods/workshop-custom").exists());
    assert!(seed.install_root.join("mods/INSTALLING_MODS.txt").is_file());
}

#[test]
fn corrupt_or_unsafe_clean_manifests_do_not_publish_a_seed() {
    let fixture = Fixture::new();
    let source = fixture.source();
    fixture.record(&source);
    let path = source.join(CLEAN_PACKAGE);
    let original: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for relative in [
        "../outside",
        "assets/../outside",
        "/outside",
        "C:/outside",
        "assets//file",
        ".langame-private-runtime",
        "assets/.langame-private-metadata",
    ] {
        let mut manifest = original.clone();
        manifest["files"][relative] = "a".repeat(64).into();
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(
            fixture.seed(&source).is_err(),
            "accepted unsafe entry {relative}"
        );
        assert!(!fixture.target().exists());
    }
    for bytes in [
        b"not json".to_vec(),
        br#"{"version":2,"files":{}}"#.to_vec(),
    ] {
        fs::write(&path, bytes).unwrap();
        assert!(fixture.seed(&source).is_err());
    }
    for (field, value) in [
        ("files", serde_json::json!({})),
        ("module_id", serde_json::json!("another-game")),
        ("version", serde_json::json!(2)),
    ] {
        let mut manifest = original.clone();
        manifest[field] = value;
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(fixture.seed(&source).is_err());
    }
    for key in ["SERVER.BIN", "ASSETS", "server.bin/child"] {
        let mut manifest = original.clone();
        manifest["files"][key] = "a".repeat(64).into();
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(
            fixture.seed(&source).is_err(),
            "accepted conflicting entry {key}"
        );
    }
    let mut manifest = original;
    manifest["source"] = "old_instance_snapshot".into();
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(fixture.seed(&source).is_err());
    assert!(!fixture.target().exists());
    assert_eq!(
        fs::read(source.join("server.bin")).unwrap(),
        b"official executable"
    );
    fixture.no_staging();
}

#[test]
fn oversized_clean_manifest_is_rejected_by_its_metadata() {
    let fixture = Fixture::new();
    let source = fixture.source();
    let file = fs::File::create(source.join(CLEAN_PACKAGE)).unwrap();
    file.set_len(MAX_MANIFEST_BYTES + 1).unwrap();
    drop(file);
    assert!(
        matches!(fixture.seed(&source), Err(StorageError::PrivateRuntimeRefresh { message, .. }) if message.contains("too large"))
    );
    assert!(!fixture.target().exists());
    fixture.no_staging();
}

#[test]
fn existing_target_and_unrelated_staging_are_never_replaced_or_removed() {
    let fixture = Fixture::new();
    let source = fixture.source();
    fixture.record(&source);
    put(&fixture.target(), ".langame-private-runtime", b"managed\n");
    put(&fixture.target(), "sentinel", b"existing installation");
    let unrelated = fixture.root.join("library/.langame-seed-unowned");
    put(&unrelated, "sentinel", b"unowned staging");
    assert!(fixture.seed(&source).is_err());
    assert_eq!(
        fs::read(fixture.target().join("sentinel")).unwrap(),
        b"existing installation"
    );
    assert_eq!(
        fs::read(unrelated.join("sentinel")).unwrap(),
        b"unowned staging"
    );
}

#[test]
fn cancellation_before_start_preserves_source_and_creates_nothing() {
    let fixture = Fixture::new();
    let source = fixture.source();
    fixture.record(&source);
    let original = fs::read(source.join(CLEAN_PACKAGE)).unwrap();
    let cancellation = AtomicBool::new(true);
    assert!(matches!(
        record_library_program_baseline(&source, &fixture.descriptor, true, Some(&cancellation)),
        Err(StorageError::InstanceCreationCancelled)
    ));
    assert_eq!(fs::read(source.join(CLEAN_PACKAGE)).unwrap(), original);
    assert!(matches!(
        prepare_seed_at(
            &fixture.target(),
            &fixture.descriptor,
            &[SeedSource {
                root: source.clone(),
                current_version: None
            }],
            Some(&cancellation)
        ),
        Err(StorageError::InstanceCreationCancelled)
    ));
    assert!(!fixture.target().exists());
    fixture.no_staging();
    assert!(matches!(
        library_program_is_pristine(&source, &fixture.descriptor, Some(&cancellation)),
        Err(StorageError::InstanceCreationCancelled)
    ));
    // This flag means the official updater has already finished mutating the
    // directory; late cancellation cannot retain trust in its previous list.
    assert!(matches!(
        record_library_program_baseline(&source, &fixture.descriptor, false, Some(&cancellation)),
        Err(StorageError::InstanceCreationCancelled)
    ));
    assert!(!source.join(CLEAN_PACKAGE).exists());
    assert_eq!(
        fs::read(source.join("server.bin")).unwrap(),
        b"official executable"
    );
}

#[tokio::test]
async fn cancellation_during_copy_cleans_only_owned_staging() {
    let fixture = Fixture::new();
    let source = fixture.source();
    fixture.record(&source);
    let target = fixture.target();
    let descriptor = fixture.descriptor.clone();
    let worker_source = source.clone();
    let cancellation = Arc::new(AtomicBool::new(false));
    let mut pause = pause_at(&cancellation, PausePoint::Copy);
    let worker_token = Arc::clone(&cancellation);
    let worker = tokio::task::spawn_blocking(move || {
        prepare_seed_at(
            &target,
            &descriptor,
            &[SeedSource {
                root: worker_source,
                current_version: None,
            }],
            Some(&worker_token),
        )
    });
    pause.reached().await;
    cancellation.store(true, Ordering::Release);
    drop(pause);
    assert!(matches!(
        worker.await.unwrap(),
        Err(StorageError::InstanceCreationCancelled)
    ));
    assert!(!fixture.target().exists());
    fixture.no_staging();
    assert_eq!(
        fs::read(source.join("server.bin")).unwrap(),
        b"official executable"
    );
}

#[tokio::test]
async fn target_created_during_copy_is_preserved_and_seed_is_not_published() {
    let fixture = Fixture::new();
    let source = fixture.source();
    fixture.record(&source);
    let target = fixture.target();
    let descriptor = fixture.descriptor.clone();
    let cancellation = Arc::new(AtomicBool::new(false));
    let mut pause = pause_at(&cancellation, PausePoint::Copy);
    let worker = tokio::task::spawn_blocking(move || {
        prepare_seed_at(
            &target,
            &descriptor,
            &[SeedSource {
                root: source,
                current_version: None,
            }],
            Some(&cancellation),
        )
    });
    pause.reached().await;
    put(&fixture.target(), "sentinel", b"concurrent installation");
    drop(pause);
    assert!(worker.await.unwrap().is_err());
    assert_eq!(
        fs::read(fixture.target().join("sentinel")).unwrap(),
        b"concurrent installation"
    );
    fixture.no_staging();
}

#[cfg(any(windows, unix))]
#[test]
fn linked_source_payload_is_rejected_without_reading_or_deleting_external_data() {
    let fixture = Fixture::new();
    let source = fixture.source();
    fixture.record(&source);
    let external = fixture.root.join("external-assets");
    fs::rename(source.join("assets"), &external).unwrap();
    let link = source.join("assets");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let result = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&link)
            .arg(&external)
            .creation_flags(0x0800_0000)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "junction fixture failed: {result:?}"
        );
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&external, &link).unwrap();
    let result = fixture.seed(&source);
    #[cfg(windows)]
    fs::remove_dir(&link).unwrap();
    #[cfg(unix)]
    fs::remove_file(&link).unwrap();
    assert!(result.is_err());
    assert!(!fixture.target().exists());
    fixture.no_staging();
    assert_eq!(
        fs::read(external.join("required.bin")).unwrap(),
        b"official assets"
    );
}

#[path = "program_acquisition_tests.rs"]
mod acquisition;

#[path = "program_validation_tests.rs"]
mod validation;

#[path = "program_seed_fallback_tests.rs"]
mod fallback;
