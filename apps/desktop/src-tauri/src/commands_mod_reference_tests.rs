use super::*;

#[test]
fn modrinth_reference_parser_rejects_unicode_invalid_references_without_panicking() {
    for reference in [
        "https://中.example/mod/test",
        "mr:😀😀",
        "https://😀.example/mod/test",
    ] {
        assert!(extract_modrinth_project_reference(reference).is_none());
    }
}

#[test]
fn minecraft_modrinth_does_not_guess_the_instance_loader_or_version() {
    let manifest = include_str!("../../../../modules/minecraft/module.toml");
    let source = module_mods_spec_from_manifest(manifest)
        .unwrap()
        .source
        .unwrap();
    assert!(source.loaders.is_empty());
    assert!(source.game_versions.is_empty());
}

#[test]
fn curseforge_reference_parser_reads_project_id_and_slug() {
    assert_eq!(
        extract_curseforge_project_id_from_reference("https://www.curseforge.com/projects/1346144")
            .as_deref(),
        Some("1346144")
    );
    assert_eq!(
        extract_curseforge_project_id_from_reference(
            "https://www.curseforge.com/ark-survival-ascended/mods/example?projectId=1346144"
        )
        .as_deref(),
        Some("1346144")
    );
    assert_eq!(
            extract_curseforge_mod_slug(
                "https://www.curseforge.com/ark-survival-ascended/mods/devkitlivemodtesting/files/6614383"
            )
            .as_deref(),
            Some("devkitlivemodtesting")
        );
}

#[test]
fn thunderstore_reference_parser_reads_package_urls() {
    let community_url = "https://thunderstore.io/c/valheim/p/ValheimModding/Jotunn/";
    let legacy_url = "https://thunderstore.io/package/denikson/BepInExPack_Valheim/";

    let community =
        extract_thunderstore_package_reference(community_url).expect("community package url");
    assert_eq!(community.namespace, "ValheimModding");
    assert_eq!(community.package, "Jotunn");
    assert_eq!(community.version, None);

    let legacy = extract_thunderstore_package_reference(legacy_url).expect("legacy package url");
    assert_eq!(legacy.namespace, "denikson");
    assert_eq!(legacy.package, "BepInExPack_Valheim");
    let pinned = extract_thunderstore_package_reference(
        "https://thunderstore.io/c/valheim/p/ValheimModding/Jotunn/2.30.1/",
    )
    .unwrap();
    assert_eq!(pinned.version.as_deref(), Some("2.30.1"));
    assert!(
        extract_thunderstore_package_reference("https://evilthunderstore.io/p/Author/Plugin/")
            .is_none()
    );
    assert!(
        extract_thunderstore_package_reference(
            "https://thunderstore.io/p/Author/Plugin/not-a-version/"
        )
        .is_none()
    );

    assert!(
        extract_thunderstore_package_reference("https://www.nexusmods.com/7daystodie/mods/123")
            .is_none()
    );
}

#[test]
fn thunderstore_payload_prefix_prefers_installable_plugin_content() {
    let names = vec![
        String::from("manifest.json"),
        String::from("README.md"),
        String::from("plugins/Jotunn.dll"),
    ];
    assert_eq!(select_thunderstore_payload_prefix(&names), Some("plugins/"));

    let bepinex_names = vec![
        String::from("manifest.json"),
        String::from("BepInEx/plugins/Example.dll"),
    ];
    assert_eq!(
        select_thunderstore_payload_prefix(&bepinex_names),
        Some("BepInEx/plugins/")
    );

    let root_names = vec![
        String::from("PlacementPlus.dll"),
        String::from("manifest.json"),
    ];
    assert_eq!(select_thunderstore_payload_prefix(&root_names), None);
}

#[test]
fn modrinth_reference_parser_reads_project_urls_and_filters() {
    let url = "https://modrinth.com/mod/fabric-api/version/0.128.2?loader=fabric&version=1.21.1";
    let parsed = extract_modrinth_project_reference(url).expect("modrinth project url");
    assert_eq!(parsed.project, "fabric-api");
    assert_eq!(parsed.loaders, vec![String::from("fabric")]);
    assert_eq!(parsed.game_versions, vec![String::from("1.21.1")]);

    let api =
        extract_modrinth_project_reference("https://api.modrinth.com/v2/project/P7dR8mSH/version")
            .expect("modrinth api project url");
    assert_eq!(api.project, "P7dR8mSH");

    let prefixed =
        extract_modrinth_project_reference("modrinth:fabric-api").expect("prefixed slug");
    assert_eq!(prefixed.project, "fabric-api");
    assert!(extract_modrinth_project_reference("https://thunderstore.io/c/valheim/").is_none());
}

#[test]
fn modrinth_download_selection_prefers_release_primary_file() {
    let versions = vec![
        ModrinthProjectVersion {
            dependencies: Vec::new(),
            project_id: String::from("P7dR8mSH"),
            name: String::from("Beta"),
            version_number: String::from("2.0.0-beta"),
            version_type: Some(String::from("beta")),
            status: Some(String::from("listed")),
            files: vec![ModrinthVersionFile {
                url: String::from("https://cdn.modrinth.com/beta.jar"),
                filename: String::from("beta.jar"),
                size: 0,
                hashes: HashMap::from([(
                    String::from("sha1"),
                    String::from("da39a3ee5e6b4b0d3255bfef95601890afd80709"),
                )]),
                primary: true,
                file_type: None,
            }],
        },
        ModrinthProjectVersion {
            dependencies: Vec::new(),
            project_id: String::from("P7dR8mSH"),
            name: String::from("Release"),
            version_number: String::from("1.0.0"),
            version_type: Some(String::from("release")),
            status: Some(String::from("listed")),
            files: vec![
                ModrinthVersionFile {
                    url: String::from("https://cdn.modrinth.com/release-sources.jar"),
                    filename: String::from("release-sources.jar"),
                    size: 0,
                    hashes: HashMap::from([(
                        String::from("sha1"),
                        String::from("da39a3ee5e6b4b0d3255bfef95601890afd80709"),
                    )]),
                    primary: true,
                    file_type: Some(String::from("sources-jar")),
                },
                ModrinthVersionFile {
                    url: String::from("https://cdn.modrinth.com/release.jar"),
                    filename: String::from("release.jar"),
                    size: 0,
                    hashes: HashMap::from([(
                        String::from("sha1"),
                        String::from("da39a3ee5e6b4b0d3255bfef95601890afd80709"),
                    )]),
                    primary: false,
                    file_type: None,
                },
            ],
        },
    ];

    let (version, file) = select_modrinth_download_file(&versions).expect("selected file");
    assert_eq!(version.version_number, "1.0.0");
    assert_eq!(file.filename, "release.jar");
}

#[tokio::test]
#[ignore]
async fn modrinth_provider_downloads_real_primary_file_smoke() {
    let client = reqwest::Client::builder()
        .user_agent(concat!("LanGameServerManager/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(45))
        .build()
        .expect("modrinth smoke client");
    let source = app_core::ModuleModSourceSpec {
        provider: String::from("modrinth"),
        label: String::from("Modrinth"),
        url: String::from("https://modrinth.com/mods"),
        loaders: vec![String::from("fabric")],
        game_versions: vec![String::from("1.21.1")],
        install_note: None,
    };
    let sources = download_modrinth_mod_sources(
        &client,
        &source,
        &[String::from("https://modrinth.com/mod/fabric-api")],
    )
    .await
    .expect("real Modrinth download");
    assert_eq!(sources.len(), 1);
    let source_path = &sources[0].path;
    assert!(source_path.exists());
    assert_eq!(
        source_path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("jar")
    );
    let stats = manual_mod_path_stats(source_path).expect("downloaded mod stats");
    assert_eq!(stats.file_count, 1);
    assert!(stats.total_bytes > 100 * 1024);
    cleanup_downloaded_mod_source(source_path);
}

#[tokio::test(flavor = "current_thread")]
#[ignore]
async fn modrinth_provider_stages_real_file_into_minecraft_instance_smoke()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = real_smoke_support::allocate_smoke_run_root("modrinth-minecraft-stage-smoke")
        .expect("modrinth stage smoke run root");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let workspace_root = workspace_root();
    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: run_root.join("instances").to_string_lossy().into_owned(),
        games_root: run_root.join("games").to_string_lossy().into_owned(),
        modules_root: workspace_root
            .join("modules")
            .to_string_lossy()
            .into_owned(),
        steamcmd_root: run_root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let minecraft_install_root = PathBuf::from(&settings.games_root).join("minecraft");
    fs::create_dir_all(&minecraft_install_root)?;
    fs::write(
        minecraft_install_root.join("server.jar"),
        "fake minecraft server jar",
    )?;
    save_app_settings(settings)?;

    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    let provisioning = command_result(
        create_instance_record_inner(
            app.state::<DesktopState>(),
            CreateInstanceInput {
                name: String::from("Modrinth Minecraft Smoke"),
                module_id: String::from("minecraft"),
            },
        )
        .await,
    )?;

    let client = reqwest::Client::builder()
        .user_agent(concat!("LanGameServerManager/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(45))
        .build()
        .expect("modrinth smoke client");
    let source = app_core::ModuleModSourceSpec {
        provider: String::from("modrinth"),
        label: String::from("Modrinth"),
        url: String::from("https://modrinth.com/mods"),
        loaders: vec![String::from("fabric")],
        game_versions: vec![String::from("1.21.1")],
        install_note: None,
    };
    let sources = download_modrinth_mod_sources(
        &client,
        &source,
        &[String::from("https://modrinth.com/mod/fabric-api")],
    )
    .await
    .expect("real Modrinth download");
    let source_paths = sources
        .iter()
        .map(|source| source.path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let stage_result = stage_manual_mod_files(
        app.state::<DesktopState>(),
        provisioning.summary.id.clone(),
        source_paths,
    )
    .await;
    for source in &sources {
        cleanup_downloaded_mod_source(&source.path);
    }
    let stage = command_result(stage_result)?;

    assert_eq!(stage.module_id, "minecraft");
    assert_eq!(stage.target_label, "mods");
    assert_eq!(stage.copied_file_count, 1);
    assert!(stage.target_path.ends_with("mods"));
    assert!(
        stage
            .items
            .iter()
            .any(|item| item.target_path.ends_with(".jar"))
    );
    let inventory = command_result(read_manual_mod_inventory_inner(provisioning.summary.id).await)?;
    assert_eq!(inventory.module_id, "minecraft");
    assert_eq!(inventory.items.len(), 1);
    assert!(inventory.items[0].name.ends_with(".jar"));
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore]
async fn install_manual_mod_references_command_rejects_unverified_minecraft_runtime_smoke()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = real_smoke_support::allocate_smoke_run_root("modrinth-reference-command-smoke")
        .expect("modrinth command smoke run root");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let workspace_root = workspace_root();
    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: run_root.join("instances").to_string_lossy().into_owned(),
        games_root: run_root.join("games").to_string_lossy().into_owned(),
        modules_root: workspace_root
            .join("modules")
            .to_string_lossy()
            .into_owned(),
        steamcmd_root: run_root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let minecraft_install_root = PathBuf::from(&settings.games_root).join("minecraft");
    fs::create_dir_all(&minecraft_install_root)?;
    fs::write(
        minecraft_install_root.join("server.jar"),
        "fake minecraft server jar",
    )?;
    save_app_settings(settings)?;

    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    let provisioning = command_result(
        create_instance_record_inner(
            app.state::<DesktopState>(),
            CreateInstanceInput {
                name: String::from("Modrinth Reference Command Smoke"),
                module_id: String::from("minecraft"),
            },
        )
        .await,
    )?;

    let error = command_result(
        install_manual_mod_references(
            app.state::<DesktopState>(),
            provisioning.summary.id.clone(),
            vec![String::from("mr:fabric-api")],
        )
        .await,
    )
    .unwrap_err();

    let error: Value = serde_json::from_str(&error.to_string())?;
    assert_eq!(error["code"], "mod_runtime_unverified");
    assert_eq!(error["reason"], "minecraft_loader");
    let inventory = command_result(read_manual_mod_inventory_inner(provisioning.summary.id).await)?;
    assert!(inventory.items.is_empty());
    Ok(())
}
