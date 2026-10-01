use super::*;

async fn register_clean_creation_fixture_package(
    paths: &StoragePaths,
    descriptor: &app_modules::ModuleDescriptor,
    root: &Path,
) {
    // The fixture calls this only before adding operator data, modelling the
    // installer's successful acquisition into a clean package directory.
    crate::record_library_program_baseline(root, descriptor, true, None).unwrap();
    register_creation_fixture_install(paths, descriptor, root).await;
}

async fn register_creation_fixture_install(
    paths: &StoragePaths,
    descriptor: &app_modules::ModuleDescriptor,
    root: &Path,
) {
    crate::sync_game_installs(
        paths,
        &[GameInstallSyncRecord {
            module_id: descriptor.summary.id.clone(),
            install_root: root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some("fixture-original-package".into()),
            mark_verified: true,
        }],
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn every_new_instance_owns_a_private_runtime_from_the_first_creation() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let shared_root = paths.games_root.join(&descriptor.summary.id);
    fs::write(shared_root.join("package.fixture"), b"installed package").unwrap();
    register_clean_creation_fixture_package(&paths, &descriptor, &shared_root).await;
    let create = |name: &str| CreateInstanceInput {
        name: name.to_owned(),
        module_id: descriptor.summary.id.clone(),
    };
    let first = create_instance_with_options(
        &paths,
        &descriptor,
        create("First"),
        InstanceCreationOptions::default(),
    )
    .await
    .unwrap();
    let second = create_instance_with_options(
        &paths,
        &descriptor,
        create("Second"),
        InstanceCreationOptions::default(),
    )
    .await
    .unwrap();

    for created in [&first, &second] {
        assert_ne!(created.effective_install_root, shared_root);
        assert!(
            created
                .effective_install_root
                .join(".langame-private-runtime")
                .is_file()
        );
        assert_eq!(
            fs::read(created.effective_install_root.join("package.fixture")).unwrap(),
            b"installed package"
        );
    }
    fs::write(
        first.effective_install_root.join("package.fixture"),
        b"first writes",
    )
    .unwrap();
    assert_eq!(
        fs::read(second.effective_install_root.join("package.fixture")).unwrap(),
        b"installed package"
    );
    assert_eq!(
        fs::read(shared_root.join("package.fixture")).unwrap(),
        b"installed package"
    );
    cleanup_root(&root);
}

#[tokio::test]
async fn dst_creation_does_not_inherit_case_variant_workshop_mods() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let shared_root = paths.games_root.join("dontstarve");
    fs::create_dir_all(shared_root.join("bin64")).unwrap();
    fs::write(
        shared_root.join("bin64/dontstarve_dedicated_server_nullrenderer_x64.exe"),
        b"synthetic official server program",
    )
    .unwrap();
    register_clean_creation_fixture_package(&paths, &descriptor, &shared_root).await;
    let shared_mod = shared_root.join("mods/Workshop-123456/modmain.lua");
    fs::create_dir_all(shared_mod.parent().unwrap()).unwrap();
    fs::write(&shared_mod, b"shared user mod").unwrap();
    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Clean DST world"),
            module_id: String::from("dontstarve"),
        },
    )
    .await
    .unwrap();

    let private_root = instance_private_runtime_root(&created);
    assert_eq!(
        fs::read(private_root.join("bin64/dontstarve_dedicated_server_nullrenderer_x64.exe"))
            .unwrap(),
        b"synthetic official server program"
    );
    assert!(
        !private_root
            .join("mods/Workshop-123456/modmain.lua")
            .exists()
    );
    assert_eq!(fs::read(&shared_mod).unwrap(), b"shared user mod");
    cleanup_root(&root);
}
async fn create_after_retained_world(delete_original: bool) {
    let root = unique_test_root();
    let mut paths = test_paths(&root);
    paths.modules_root = fs::canonicalize(repo_root().join("modules")).unwrap();
    let descriptor = app_modules::discover_modules(&paths.modules_root)
        .unwrap()
        .into_iter()
        .find(|descriptor| descriptor.summary.id == "rimworld")
        .unwrap();
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    let shared_root = paths.games_root.join("rimworld");
    fs::create_dir_all(shared_root.join("Assets")).unwrap();
    fs::write(shared_root.join("RTServer.exe"), b"package fixture").unwrap();
    register_clean_creation_fixture_package(&paths, &descriptor, &shared_root).await;
    let create = |name: &str| CreateInstanceInput {
        name: name.to_owned(),
        module_id: "rimworld".to_owned(),
    };
    let original = if delete_original {
        Some(
            create_instance(&paths, &descriptor, create("Deleted world"))
                .await
                .unwrap(),
        )
    } else {
        None
    };
    assert!(
        shared_root.is_dir(),
        "creating an instance must retain its source library"
    );
    let sentinel = shared_root.join("Assets/retained-world.sentinel");
    fs::write(&sentinel, b"retained world requires explicit import").unwrap();
    if let Some(original) = original {
        delete_instance(&paths, &original.summary.id).await.unwrap();
    }
    assert!(list_instances(&paths).await.unwrap().is_empty());
    let created = create_instance(&paths, &descriptor, create("Fresh world"))
        .await
        .unwrap();
    let details = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let inherited = Path::new(&details.saves_path)
        .join("retained-world.sentinel")
        .exists();
    assert_eq!(
        fs::read(&sentinel).unwrap(),
        b"retained world requires explicit import"
    );
    cleanup_root(&root);
    assert!(
        !inherited,
        "a first registered instance inherited an unowned world"
    );
}

#[tokio::test]
async fn first_instance_does_not_inherit_unregistered_install_save_data() {
    create_after_retained_world(false).await;
}

#[tokio::test]
async fn replacement_instance_does_not_inherit_deleted_instance_save_data() {
    create_after_retained_world(true).await;
}

#[tokio::test]
async fn second_instance_does_not_inherit_shared_install_save_data() {
    let root = unique_test_root();
    let mut paths = test_paths(&root);
    paths.modules_root = repo_root().join("modules");
    let descriptor = app_modules::discover_modules(&paths.modules_root)
        .unwrap()
        .into_iter()
        .find(|descriptor| descriptor.summary.id == "rimworld")
        .unwrap();
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    let shared_root = paths.games_root.join("rimworld");
    fs::create_dir_all(&shared_root).unwrap();
    fs::write(shared_root.join("RTServer.exe"), b"package fixture").unwrap();
    register_clean_creation_fixture_package(&paths, &descriptor, &shared_root).await;
    let create = |name: &str| CreateInstanceInput {
        name: name.to_owned(),
        module_id: "rimworld".to_owned(),
    };
    let first = create_instance(&paths, &descriptor, create("First world"))
        .await
        .unwrap();
    let first_details = read_instance_details(&paths, &first.summary.id)
        .await
        .unwrap();
    let first_save = Path::new(&first_details.saves_path).join("first-world.sentinel");
    fs::write(&first_save, b"only the first instance owns this world").unwrap();
    let first_config = fs::read(&first.config_file_path).unwrap();
    let second = create_instance(&paths, &descriptor, create("Second world"))
        .await
        .unwrap();
    let second_details = read_instance_details(&paths, &second.summary.id)
        .await
        .unwrap();
    let inherited = Path::new(&second_details.saves_path)
        .join("first-world.sentinel")
        .exists();
    let private_root = Path::new(&second.config_file_path)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("runtime");
    assert_eq!(
        fs::read(private_root.join("RTServer.exe")).unwrap(),
        b"package fixture"
    );
    assert_eq!(
        fs::read(&first_save).unwrap(),
        b"only the first instance owns this world"
    );
    assert_eq!(fs::read(&first.config_file_path).unwrap(), first_config);
    assert_ne!(first_details.saves_path, second_details.saves_path);
    cleanup_root(&root);
    assert!(
        !inherited,
        "new private runtime copied the first instance's world"
    );
}

async fn assert_dragonwilds_creation_preserves_instance_identity(register_first: bool) {
    let root = unique_test_root();
    let mut paths = test_paths(&root);
    paths.modules_root = repo_root().join("modules");
    let descriptor = app_modules::discover_modules(&paths.modules_root)
        .unwrap()
        .into_iter()
        .find(|descriptor| descriptor.summary.id == "runescapedragonwilds")
        .unwrap();
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    let shared_root = paths.games_root.join("runescapedragonwilds");
    fs::create_dir_all(&shared_root).unwrap();
    fs::write(
        shared_root.join("RSDragonwildsServer.exe"),
        b"package fixture",
    )
    .unwrap();
    register_clean_creation_fixture_package(&paths, &descriptor, &shared_root).await;
    let create = |name: &str| CreateInstanceInput {
        name: name.to_owned(),
        module_id: descriptor.summary.id.clone(),
    };
    if register_first {
        create_instance(&paths, &descriptor, create("Original server"))
            .await
            .unwrap();
    }
    let relative_config = "RSDragonwilds/Saved/Config/WindowsServer/DedicatedServer.ini";
    let original_config = shared_root.join(relative_config);
    fs::create_dir_all(original_config.parent().unwrap()).unwrap();
    let original_native = b"[/Script/Dominion.DedicatedServerSettings]\nServerGuid=original-guid\nAdminUsers=original-admin\n";
    fs::write(&original_config, original_native).unwrap();
    assert!(
        !shared_root.join("RSDragonwilds/Saved/SaveGames").exists()
            || fs::read_dir(shared_root.join("RSDragonwilds/Saved/SaveGames"))
                .unwrap()
                .next()
                .is_none(),
        "the fixture must require isolation without any saved world"
    );

    let created = create_instance(&paths, &descriptor, create("Fresh server"))
        .await
        .unwrap();
    let private_root = Path::new(&created.config_file_path)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("runtime");
    let native_path = private_root.join(relative_config);
    // Both creation and the production startup materializer must leave identity
    // generation to the new native server, even when no save data exists yet.
    let created_native = fs::read_to_string(&native_path).ok();
    materialize_instance_configuration_for_start(&paths, &created.summary.id)
        .await
        .unwrap();
    let fresh_native = fs::read_to_string(&native_path).ok();
    let copied_program = fs::read(private_root.join("RSDragonwildsServer.exe")).ok();
    if let Some(mut native) = fresh_native.clone() {
        native.push_str("ServerGuid=fresh-guid\nAdminUsers=fresh-admin\n");
        fs::write(&native_path, native).unwrap();
        materialize_instance_configuration_for_start(&paths, &created.summary.id)
            .await
            .unwrap();
    }
    let restarted_native = fs::read_to_string(&native_path).ok();
    let retained_original = fs::read(&original_config).unwrap();
    cleanup_root(&root);

    assert_eq!(
        copied_program.as_deref(),
        Some(b"package fixture".as_slice())
    );
    assert_eq!(retained_original, original_native);
    for native in [created_native, fresh_native] {
        let native = native.expect("new server must have a private native configuration");
        assert!(
            !native.contains("ServerGuid=") && !native.contains("AdminUsers="),
            "new server inherited another server's native identity: {native}"
        );
        assert!(native.contains("OwnerId=LGSM_UNASSIGNED_OWNER"));
    }
    let restarted_native = restarted_native.unwrap();
    assert!(restarted_native.contains("ServerGuid=fresh-guid"));
    assert!(restarted_native.contains("AdminUsers=fresh-admin"));
}

#[tokio::test]
async fn dragonwilds_creation_does_not_inherit_existing_instance_identity() {
    assert_dragonwilds_creation_preserves_instance_identity(true).await;
}

#[tokio::test]
async fn dragonwilds_creation_isolates_unregistered_native_identity_without_saves() {
    assert_dragonwilds_creation_preserves_instance_identity(false).await;
}

#[tokio::test]
async fn creation_uses_current_save_boundary_instead_of_stale_recorded_root() {
    let root = unique_test_root();
    let mut paths = test_paths(&root);
    paths.modules_root = repo_root().join("modules");
    let descriptor = app_modules::discover_modules(&paths.modules_root)
        .unwrap()
        .into_iter()
        .find(|descriptor| descriptor.summary.id == "romestead")
        .unwrap();
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    let shared_root = paths.games_root.join("romestead");
    fs::create_dir_all(&shared_root).unwrap();
    fs::write(shared_root.join("Server.exe"), b"package fixture").unwrap();
    register_clean_creation_fixture_package(&paths, &descriptor, &shared_root).await;
    let create = |name: &str| CreateInstanceInput {
        name: name.to_owned(),
        module_id: "romestead".to_owned(),
    };
    let first = create_instance(&paths, &descriptor, create("Existing world"))
        .await
        .unwrap();
    let save = shared_root.join("saved_worlds/existing-world.sentinel");
    fs::create_dir_all(save.parent().unwrap()).unwrap();
    fs::write(&save, b"existing world").unwrap();
    // The previous declaration recorded the whole package as the save directory.
    let pool = crate::storage_db::connect_pool(&paths).await.unwrap();
    sqlx::query("UPDATE instances SET saves_path = ?1 WHERE id = ?2")
        .bind(shared_root.to_string_lossy().as_ref())
        .bind(&first.summary.id)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let details = read_instance_details(&paths, &first.summary.id)
        .await
        .unwrap();
    assert_eq!(
        Path::new(&details.saves_path),
        instance_private_runtime_root(&first).join("saved_worlds")
    );

    let second = create_instance(&paths, &descriptor, create("Clean world")).await;
    let second = match second {
        Ok(instance) => instance,
        Err(error) => {
            cleanup_root(&root);
            panic!("stale recorded root prevented a clean instance: {error}");
        }
    };
    let details = read_instance_details(&paths, &second.summary.id)
        .await
        .unwrap();
    let private_root = Path::new(&details.saves_path).parent().unwrap();
    assert_ne!(private_root, shared_root);
    assert_eq!(
        fs::read(private_root.join("Server.exe")).unwrap(),
        b"package fixture"
    );
    assert!(
        !Path::new(&details.saves_path)
            .join("existing-world.sentinel")
            .exists()
    );
    assert_eq!(fs::read(&save).unwrap(), b"existing world");
    cleanup_root(&root);
}

#[tokio::test]
async fn new_enshrouded_instances_do_not_inherit_shared_mods_or_native_config() {
    let root = unique_test_root();
    let mut paths = test_paths(&root);
    paths.modules_root = repo_root().join("modules");
    let descriptor = app_modules::discover_modules(&paths.modules_root)
        .unwrap()
        .into_iter()
        .find(|descriptor| descriptor.summary.id == "enshrouded")
        .unwrap();
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let shared_root = paths.games_root.join("enshrouded");
    let shared_mod = shared_root.join("mods/legacy-mod.dll");
    let shared_native = shared_root.join("enshrouded_server.json");
    fs::create_dir_all(shared_mod.parent().unwrap()).unwrap();
    fs::write(
        shared_root.join("enshrouded_server.exe"),
        b"package fixture",
    )
    .unwrap();
    register_clean_creation_fixture_package(&paths, &descriptor, &shared_root).await;
    fs::write(&shared_mod, b"legacy mod").unwrap();
    fs::write(&shared_native, br#"{"legacyOwner":"shared"}"#).unwrap();

    let create = |name: &str| CreateInstanceInput {
        name: name.to_owned(),
        module_id: descriptor.summary.id.clone(),
    };
    let first = create_instance_with_options(
        &paths,
        &descriptor,
        create("First"),
        InstanceCreationOptions::default(),
    )
    .await
    .unwrap();
    let second = create_instance_with_options(
        &paths,
        &descriptor,
        create("Second"),
        InstanceCreationOptions::default(),
    )
    .await
    .unwrap();

    for created in [&first, &second] {
        let runtime = &created.effective_install_root;
        assert_ne!(runtime, &shared_root);
        assert_eq!(
            fs::read(runtime.join("enshrouded_server.exe")).unwrap(),
            b"package fixture"
        );
        assert!(!runtime.join("mods/legacy-mod.dll").exists());
        let native = fs::read_to_string(runtime.join("enshrouded_server.json")).unwrap();
        assert!(!native.contains("legacyOwner"), "{native}");
    }

    fs::create_dir_all(first.effective_install_root.join("mods")).unwrap();
    fs::write(
        first.effective_install_root.join("mods/only-first.dll"),
        b"first mod",
    )
    .unwrap();
    assert!(
        !second
            .effective_install_root
            .join("mods/only-first.dll")
            .exists()
    );
    assert!(!shared_root.join("mods/only-first.dll").exists());
    assert_eq!(fs::read(&shared_mod).unwrap(), b"legacy mod");
    assert_eq!(
        fs::read(&shared_native).unwrap(),
        br#"{"legacyOwner":"shared"}"#
    );
    cleanup_root(&root);
}
