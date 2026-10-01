use super::*;

fn fixture() -> (PathBuf, PathBuf, PathBuf) {
    let root =
        std::env::temp_dir().join(format!("langame-runtime-refresh-{}", uuid::Uuid::new_v4()));
    let shared = root.join("shared");
    let instance = root.join("instance");
    let runtime = instance.join("runtime");
    fs::create_dir_all(&shared).unwrap();
    fs::create_dir_all(&runtime).unwrap();
    (root, shared, instance)
}

fn create_baseline(shared: &Path, instance: &Path, exclusions: &[PathBuf]) {
    let runtime = instance.join("runtime");
    record_package_baseline(shared, &runtime, exclusions, None, None).unwrap();
    crate::private_runtime::record_projection_identity(shared, instance, &runtime).unwrap();
    fs::write(runtime.join(PRIVATE_RUNTIME_MARKER), b"managed\n").unwrap();
}

#[test]
fn refresh_replaces_package_file_and_preserves_instance_files() {
    let (root, shared, instance) = fixture();
    let runtime = instance.join("runtime");
    fs::write(shared.join("server.exe"), b"package one").unwrap();
    fs::copy(shared.join("server.exe"), runtime.join("server.exe")).unwrap();
    create_baseline(&shared, &instance, &[]);

    fs::create_dir(runtime.join("Mods")).unwrap();
    fs::write(runtime.join("Mods/local.dll"), b"local mod").unwrap();
    fs::write(runtime.join("operator.txt"), b"local setting").unwrap();
    fs::write(shared.join("server.exe"), b"package two").unwrap();
    fs::write(shared.join("new.asset"), b"new package file").unwrap();

    assert_eq!(
        refresh_private_runtime(&shared, &instance, None).unwrap(),
        PrivateRuntimeRefresh::Refreshed
    );
    assert_eq!(
        fs::read(runtime.join("server.exe")).unwrap(),
        b"package two"
    );
    assert_eq!(
        fs::read(runtime.join("new.asset")).unwrap(),
        b"new package file"
    );
    assert_eq!(
        fs::read(runtime.join("Mods/local.dll")).unwrap(),
        b"local mod"
    );
    assert_eq!(
        fs::read(runtime.join("operator.txt")).unwrap(),
        b"local setting"
    );
    assert!(!instance.join(ROLLBACK_NAME).exists());
    assert!(!instance.join(STAGING_NAME).exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unchanged_upstream_package_keeps_a_local_modification() {
    let (root, shared, instance) = fixture();
    let runtime = instance.join("runtime");
    fs::write(shared.join("server.cfg"), b"package default").unwrap();
    fs::copy(shared.join("server.cfg"), runtime.join("server.cfg")).unwrap();
    create_baseline(&shared, &instance, &[]);
    fs::write(runtime.join("server.cfg"), b"local configuration").unwrap();

    assert_eq!(
        refresh_private_runtime(&shared, &instance, None).unwrap(),
        PrivateRuntimeRefresh::Current
    );
    assert_eq!(
        fs::read(runtime.join("server.cfg")).unwrap(),
        b"local configuration"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn two_sided_change_refuses_refresh_without_touching_instance() {
    let (root, shared, instance) = fixture();
    let runtime = instance.join("runtime");
    fs::write(shared.join("server.cfg"), b"package default").unwrap();
    fs::copy(shared.join("server.cfg"), runtime.join("server.cfg")).unwrap();
    create_baseline(&shared, &instance, &[]);
    let original_manifest = fs::read(runtime.join(BASELINE_FILE)).unwrap();
    fs::write(shared.join("server.cfg"), b"new package default").unwrap();
    fs::write(runtime.join("server.cfg"), b"user choice").unwrap();

    let error = refresh_private_runtime(&shared, &instance, None).unwrap_err();
    assert!(error.to_string().contains("changed in both"));
    assert_eq!(
        fs::read(runtime.join("server.cfg")).unwrap(),
        b"user choice"
    );
    assert_eq!(
        fs::read(runtime.join(BASELINE_FILE)).unwrap(),
        original_manifest
    );
    assert!(!instance.join(STAGING_NAME).exists());
    assert!(!instance.join(ROLLBACK_NAME).exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn new_package_file_does_not_claim_an_equal_instance_file() {
    let (root, shared, instance) = fixture();
    let runtime = instance.join("runtime");
    fs::write(shared.join("server.exe"), b"package").unwrap();
    fs::copy(shared.join("server.exe"), runtime.join("server.exe")).unwrap();
    create_baseline(&shared, &instance, &[]);
    let baseline_before = fs::read(runtime.join(BASELINE_FILE)).unwrap();

    fs::write(runtime.join("owner.cfg"), b"same bytes").unwrap();
    fs::write(shared.join("owner.cfg"), b"same bytes").unwrap();
    let error = refresh_private_runtime(&shared, &instance, None).unwrap_err();
    assert!(error.to_string().contains("owner.cfg"));
    assert_eq!(fs::read(runtime.join("owner.cfg")).unwrap(), b"same bytes");
    assert_eq!(
        fs::read(runtime.join(BASELINE_FILE)).unwrap(),
        baseline_before
    );
    assert!(!instance.join(STAGING_NAME).exists());
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn refresh_copies_required_empty_package_directory() {
    let (root, shared, instance) = fixture();
    let runtime = instance.join("runtime");
    fs::write(shared.join("server.exe"), b"package").unwrap();
    fs::copy(shared.join("server.exe"), runtime.join("server.exe")).unwrap();
    create_baseline(&shared, &instance, &[]);

    fs::create_dir_all(shared.join("required/empty")).unwrap();
    assert_eq!(
        refresh_private_runtime(&shared, &instance, None).unwrap(),
        PrivateRuntimeRefresh::Refreshed
    );
    assert!(runtime.join("required/empty").is_dir());
    assert_eq!(
        refresh_private_runtime(&shared, &instance, None).unwrap(),
        PrivateRuntimeRefresh::Current
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn new_package_directory_does_not_claim_instance_directory() {
    let (root, shared, instance) = fixture();
    let runtime = instance.join("runtime");
    fs::write(shared.join("server.exe"), b"package").unwrap();
    fs::copy(shared.join("server.exe"), runtime.join("server.exe")).unwrap();
    create_baseline(&shared, &instance, &[]);
    fs::create_dir(runtime.join("Plugins")).unwrap();
    fs::write(runtime.join("Plugins/local.dll"), b"private mod").unwrap();
    fs::create_dir(shared.join("Plugins")).unwrap();

    let error = refresh_private_runtime(&shared, &instance, None).unwrap_err();
    assert!(error.to_string().contains("Plugins"));
    assert_eq!(
        fs::read(runtime.join("Plugins/local.dll")).unwrap(),
        b"private mod"
    );
    assert!(!instance.join(STAGING_NAME).exists());
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn missing_baseline_rejects_refresh_without_changing_runtime() {
    let (root, shared, instance) = fixture();
    let runtime = instance.join("runtime");
    fs::write(shared.join("server.exe"), b"new package").unwrap();
    fs::write(runtime.join("server.exe"), b"old package").unwrap();
    fs::write(runtime.join(PRIVATE_RUNTIME_MARKER), b"managed\n").unwrap();

    crate::private_runtime::record_projection_identity(&shared, &instance, &runtime).unwrap();
    let error = refresh_private_runtime(&shared, &instance, Some("2")).unwrap_err();
    assert!(error.to_string().contains("baseline is missing"), "{error}");
    assert_eq!(
        fs::read(runtime.join("server.exe")).unwrap(),
        b"old package"
    );
    assert!(!runtime.join(BASELINE_FILE).exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn interrupted_swap_restores_old_runtime_before_refresh() {
    let (root, shared, instance) = fixture();
    let runtime = instance.join("runtime");
    fs::write(shared.join("server.exe"), b"package").unwrap();
    fs::copy(shared.join("server.exe"), runtime.join("server.exe")).unwrap();
    create_baseline(&shared, &instance, &[]);

    fs::rename(&runtime, instance.join(ROLLBACK_NAME)).unwrap();
    let staging = instance.join(STAGING_NAME);
    fs::create_dir(&staging).unwrap();
    crate::private_runtime::record_projection_identity(&shared, &instance, &staging).unwrap();
    fs::write(staging.join(REFRESH_MARKER), b"managed\n").unwrap();
    assert_eq!(
        refresh_private_runtime(&shared, &instance, None).unwrap(),
        PrivateRuntimeRefresh::Current
    );
    assert_eq!(fs::read(runtime.join("server.exe")).unwrap(), b"package");
    assert!(!instance.join(ROLLBACK_NAME).exists());
    assert!(!staging.exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn shared_workshop_cache_is_excluded_from_baseline_and_refresh() {
    let (root, shared, instance) = fixture();
    let runtime = instance.join("runtime");
    let workshop = shared.join("steamapps/workshop");
    fs::create_dir_all(&workshop).unwrap();
    fs::write(workshop.join("cache.bin"), b"old cache").unwrap();
    fs::write(shared.join("server.exe"), b"package").unwrap();
    fs::copy(shared.join("server.exe"), runtime.join("server.exe")).unwrap();
    // Creation copies the parent package directory while omitting the cache.
    fs::create_dir(runtime.join("steamapps")).unwrap();
    create_baseline(&shared, &instance, std::slice::from_ref(&workshop));

    fs::write(workshop.join("cache.bin"), b"new cache").unwrap();
    assert_eq!(
        refresh_private_runtime(&shared, &instance, None).unwrap(),
        PrivateRuntimeRefresh::Current
    );
    assert!(!runtime.join("steamapps/workshop/cache.bin").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_baseline_still_blocks_refresh() {
    let (root, shared, instance) = fixture();
    let runtime = instance.join("runtime");
    fs::write(shared.join("server.exe"), b"new package").unwrap();
    fs::write(runtime.join("server.exe"), b"old package").unwrap();
    fs::write(runtime.join(PRIVATE_RUNTIME_MARKER), b"managed\n").unwrap();
    fs::write(runtime.join(BASELINE_FILE), b"invalid json").unwrap();
    crate::private_runtime::record_projection_identity(&shared, &instance, &runtime).unwrap();

    let error = refresh_private_runtime(&shared, &instance, Some("2")).unwrap_err();
    assert!(error.to_string().contains("baseline is invalid"));
    assert_eq!(
        fs::read(runtime.join("server.exe")).unwrap(),
        b"old package"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn dst_workshop_mod_added_after_creation_is_not_copied_to_instance() {
    let (root, shared, instance) = fixture();
    let runtime = instance.join("runtime");
    fs::write(shared.join("server.exe"), b"package one").unwrap();
    fs::copy(shared.join("server.exe"), runtime.join("server.exe")).unwrap();
    record_package_baseline_with_rules(&shared, &runtime, &[], Some("1"), true, None).unwrap();
    crate::private_runtime::record_projection_identity(&shared, &instance, &runtime).unwrap();
    fs::write(runtime.join(PRIVATE_RUNTIME_MARKER), b"managed\n").unwrap();

    let workshop = shared.join("mods/workshop-123456");
    fs::create_dir_all(&workshop).unwrap();
    fs::write(workshop.join("modmain.lua"), b"shared mod").unwrap();
    fs::create_dir_all(shared.join("mods/workshop-custom")).unwrap();
    fs::write(
        shared.join("mods/workshop-custom/modmain.lua"),
        b"package file",
    )
    .unwrap();
    fs::write(shared.join("server.exe"), b"package two").unwrap();

    assert_eq!(
        refresh_private_runtime(&shared, &instance, Some("2")).unwrap(),
        PrivateRuntimeRefresh::Refreshed
    );
    assert!(!runtime.join("mods/workshop-123456/modmain.lua").exists());
    assert_eq!(
        fs::read(runtime.join("mods/workshop-custom/modmain.lua")).unwrap(),
        b"package file"
    );
    assert_eq!(
        fs::read(runtime.join("server.exe")).unwrap(),
        b"package two"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn non_dst_package_keeps_new_numeric_workshop_directory() {
    let (root, shared, instance) = fixture();
    let runtime = instance.join("runtime");
    fs::write(shared.join("server.exe"), b"package").unwrap();
    fs::copy(shared.join("server.exe"), runtime.join("server.exe")).unwrap();
    create_baseline(&shared, &instance, &[]);

    let workshop = shared.join("mods/workshop-123456");
    fs::create_dir_all(&workshop).unwrap();
    fs::write(workshop.join("asset.bin"), b"package asset").unwrap();

    assert_eq!(
        refresh_private_runtime(&shared, &instance, None).unwrap(),
        PrivateRuntimeRefresh::Refreshed
    );
    assert_eq!(
        fs::read(runtime.join("mods/workshop-123456/asset.bin")).unwrap(),
        b"package asset"
    );
    fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[test]
fn case_variant_exclusion_does_not_copy_shared_workshop_cache() {
    let (root, shared, instance) = fixture();
    let runtime = instance.join("runtime");
    fs::write(shared.join("server.exe"), b"package one").unwrap();
    fs::copy(shared.join("server.exe"), runtime.join("server.exe")).unwrap();
    let workshop = shared.join("SteamApps/Workshop");
    fs::create_dir_all(&workshop).unwrap();
    fs::write(workshop.join("cache.bin"), b"shared cache").unwrap();
    create_baseline(&shared, &instance, &[shared.join("steamapps/workshop")]);
    fs::write(shared.join("server.exe"), b"package two").unwrap();

    assert_eq!(
        refresh_private_runtime(&shared, &instance, None).unwrap(),
        PrivateRuntimeRefresh::Refreshed
    );
    assert!(!runtime.join("SteamApps/Workshop/cache.bin").exists());
    fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[test]
fn case_colliding_baseline_paths_are_rejected() {
    let files = BTreeMap::from([
        (String::from("Mods/One"), String::from("first")),
        (String::from("mods/two"), String::from("second")),
    ]);
    let error = validate_case_unique_path_keys(files.keys(), Path::new("runtime")).unwrap_err();
    assert!(error.to_string().contains("differ only by ASCII case"));
}

#[test]
fn missing_required_runtime_never_refreshes_as_shared() {
    let (root, shared, instance) = fixture();
    fs::remove_dir(instance.join("runtime")).unwrap();
    fs::write(shared.join("server.exe"), b"shared package").unwrap();
    let error = refresh_private_runtime(&shared, &instance, None).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("required private runtime is missing")
    );
    assert!(!instance.join("runtime").exists());
    assert_eq!(
        fs::read(shared.join("server.exe")).unwrap(),
        b"shared package"
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn incomplete_baseline_never_guesses_workshop_ownership() {
    let (root, shared, instance) = fixture();
    let runtime = instance.join("runtime");
    fs::write(shared.join("server.exe"), b"package one").unwrap();
    fs::write(runtime.join("server.exe"), b"package one").unwrap();
    create_baseline(&shared, &instance, &[]);
    let baseline_path = runtime.join(BASELINE_FILE);
    let mut baseline: serde_json::Value =
        serde_json::from_slice(&fs::read(&baseline_path).unwrap()).unwrap();
    baseline
        .as_object_mut()
        .unwrap()
        .remove("exclude_dst_workshop_mods");
    fs::write(&baseline_path, serde_json::to_vec(&baseline).unwrap()).unwrap();
    fs::write(shared.join("server.exe"), b"package two").unwrap();
    let before = crate::test_file_snapshot::tree_snapshot(&runtime).unwrap();
    let result = refresh_private_runtime(&shared, &instance, None);
    assert!(
        matches!(result, Err(StorageError::PrivateRuntimeRefresh { .. })),
        "{result:?}"
    );
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&runtime).unwrap(),
        before
    );
    assert!(!instance.join(STAGING_NAME).exists());
    assert!(!instance.join(ROLLBACK_NAME).exists());
    fs::remove_dir_all(root).unwrap();
}
