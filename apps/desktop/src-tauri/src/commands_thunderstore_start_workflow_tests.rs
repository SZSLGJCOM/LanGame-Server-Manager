//! Real startup orchestration with isolated storage/state/program leases.
//! Only public-provider metadata and ZIP delivery are synthetic boundaries.
use super::*;
use std::collections::BTreeMap;
use tauri::Manager;

// Raw bytes provide an independent preservation oracle for this tiny fixture.
fn snapshot(root: &Path) -> std::io::Result<BTreeMap<PathBuf, Vec<u8>>> {
    let mut result = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path)?;
        assert!(!metadata.file_type().is_symlink());
        if metadata.is_dir() {
            for entry in fs::read_dir(&path)? {
                pending.push(entry?.path());
            }
        } else {
            result.insert(
                path.strip_prefix(root).unwrap().to_path_buf(),
                fs::read(&path)?,
            );
        }
    }
    Ok(result)
}

struct Fixture {
    root: PathBuf,
    storage: StorageBootstrap,
    descriptor: ModuleDescriptor,
    instance: InstanceDetails,
    program: PathBuf,
    target: ResolvedManualModTarget,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
impl Fixture {
    async fn new(policy: Option<&str>) -> Result<Self, Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "lgsm-ts-start-workflow-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap();
        let storage = app_storage::bootstrap_storage_with_paths(app_storage::StoragePaths {
            app_data_root: root.join("app-data"),
            settings_path: root.join("app-data/settings.json"),
            database_path: root.join("app-data/db/lgs.db"),
            logs_root: root.join("app-data/logs"),
            modules_root: workspace.join("modules"),
            migrations_root: workspace.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances/.trash"),
        })?;
        initialize_database(&storage.paths).await?;
        let descriptor = discover_modules(&storage.paths.modules_root)?
            .into_iter()
            .find(|module| module.summary.id == "corekeeper")
            .unwrap();
        sync_modules(&storage.paths, std::slice::from_ref(&descriptor)).await?;
        let library = storage.paths.games_root.join("corekeeper");
        fs::create_dir_all(library.join("CoreKeeperServer_Data/StreamingAssets/Mods"))?;
        fs::write(
            library.join("CoreKeeperServer.exe"),
            b"MZinert-server-fixture",
        )?;
        app_storage::record_library_program_baseline(&library, &descriptor, true, None)?;
        sync_game_installs(
            &storage.paths,
            &[GameInstallSyncRecord {
                module_id: "corekeeper".into(),
                install_root: library.to_string_lossy().into_owned(),
                install_state: InstallState::Installed,
                current_version: None,
                mark_verified: true,
            }],
        )
        .await?;
        let created = app_storage::create_instance_with_options(
            &storage.paths,
            &descriptor,
            CreateInstanceInput {
                name: "Thunderstore startup fixture".into(),
                module_id: "corekeeper".into(),
            },
            app_storage::InstanceCreationOptions {
                program_mode: Some(app_core::InstanceProgramMode::Independent),
                prefer_existing_install: false,
                require_clean_program: true,
                ..Default::default()
            },
        )
        .await?;
        let instance =
            read_instance_details(&storage.paths, &created.provisioning.summary.id).await?;
        let program = PathBuf::from(
            crate::commands::commands_runtime_lifecycle::private_runtime_install_root(&instance)?,
        );
        let target =
            resolve_manual_mod_target(&storage, instance.summary.id.clone(), false).await?;
        let fixture = Self {
            root,
            storage,
            descriptor,
            instance,
            program,
            target,
        };
        let reference = "https://thunderstore.io/package/Author/Plugin/1.0.0/".to_owned();
        let intent = if let Some(policy) = policy {
            Some(
                crate::commands::commands_mod_staging::root_thunderstore_intents_with_policy(
                    &[reference],
                    Some(policy),
                )?["thunderstore-Author-Plugin"]
                    .clone(),
            )
        } else {
            None
        };
        let root_source =
            fixture.source("Author-Plugin", "1.0.0", intent, &["Author-Library-1.0.0"])?;
        let dependency = fixture.source(
            "Author-Library",
            "1.0.0",
            Some(crate::commands::commands_mod_staging::ThunderstoreInstallIntent::Dependency),
            &[],
        )?;
        crate::commands::commands_mod_staging::stage_thunderstore_online_mod_sources(
            fixture.target.clone(),
            &[root_source, dependency],
            None,
        )?;
        fs::write(fixture.plugin().join("Config/user.ini"), b"operator config")?;
        fs::write(fixture.plugin().join("user-note.txt"), b"operator note")?;
        Ok(fixture)
    }
    fn plugin(&self) -> PathBuf {
        self.target.target_path.join("thunderstore-Author-Plugin")
    }
    fn source(
        &self,
        project: &str,
        version: &str,
        intent: Option<crate::commands::commands_mod_staging::ThunderstoreInstallIntent>,
        dependencies: &[&str],
    ) -> Result<DownloadedManualModSource, Box<dyn std::error::Error>> {
        let path = self
            .root
            .join(format!("download-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(path.join("Config"))?;
        fs::write(
            path.join("ModManifest.json"),
            format!(r#"{{"name":"{project}"}}"#),
        )?;
        fs::write(path.join("Plugin.dll"), format!("MZ{version}"))?;
        fs::write(path.join("Config/user.ini"), b"default")?;
        Ok(DownloadedManualModSource {
            path,
            label: project.into(),
            identity: OnlineModIdentity {
                provider: "thunderstore".into(),
                project: project.into(),
                historical_names: Vec::new(),
                version: version.into(),
                dependencies: dependencies.iter().map(|value| (*value).into()).collect(),
                thunderstore_intent: intent,
            },
        })
    }
    async fn guard(
        &self,
        state: &DesktopState,
    ) -> Result<app_steamcmd::GameInstallLifecycleGuard, String> {
        app_steamcmd::acquire_game_install_lifecycle(
            &state.install_locks,
            "corekeeper",
            std::slice::from_ref(&self.program),
        )
        .await
        .map_err(|error| error.to_string())
    }
}

fn metadata(project: &str, version: &str, dependencies: &[&str]) -> serde_json::Value {
    let (owner, name) = project.split_once('-').unwrap();
    json!({"full_name":project,"community_listings":[{"community":"core-keeper"}],"latest":{"version_number":version,"download_url":format!("https://thunderstore.io/package/download/{owner}/{name}/{version}/"),"dependencies":dependencies}})
}
fn archive(project: &str, version: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, bytes) in [
        ("ModManifest.json", format!(r#"{{"name":"{project}"}}"#)),
        ("Plugin.dll", format!("MZ{version}")),
        ("Config/user.ini", "new default".into()),
    ] {
        zip.start_file(name, options).unwrap();
        zip.write_all(bytes.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn provider(case: &str) -> mod_online::FixtureProvider {
    let mut provider = mod_online::FixtureProvider::default();
    let version = if case == "warm" { "1.0.0" } else { "2.0.0" };
    let library = if case == "warm" { "1.0.0" } else { "1.1.0" };
    let dependency = format!("Author-Library-{library}");
    let dependencies = if case == "orphan" {
        Vec::new()
    } else {
        vec![dependency.as_str()]
    };
    provider.packages.insert(
        "Author-Plugin".into(),
        metadata("Author-Plugin", version, &dependencies),
    );
    provider.packages.insert(
        "Author-Plugin-1.0.0".into(),
        metadata("Author-Plugin", "1.0.0", &["Author-Library-1.0.0"]),
    );
    for version in ["1.0.0", "1.1.0"] {
        provider.packages.insert(
            format!("Author-Library-{version}"),
            metadata("Author-Library", version, &[]),
        );
        provider.archives.insert(
            format!("Author-Library-{version}"),
            archive("Author-Library", version),
        );
    }
    provider.archives.insert(
        "Author-Plugin-2.0.0".into(),
        archive("Author-Plugin", "2.0.0"),
    );
    provider.fail_metadata = matches!(case, "network" | "unknown" | "corrupt" | "missing");
    provider.fail_download = case == "download";
    provider.cancel_metadata = case == "cancel";
    provider
}

async fn exercise(case: &str) -> Result<(), Box<dyn std::error::Error>> {
    let _serial = crate::commands::tests::command_smoke_lock().lock().await;
    let policy = if case == "unknown" {
        None
    } else if case == "pin" {
        Some("pinned")
    } else {
        Some("follow_latest")
    };
    let fixture = Fixture::new(policy).await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&fixture.storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    let reservation =
        match state.try_reserve_runtime_start(&fixture.instance.summary.id, "manual")? {
            RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
            other => panic!("unexpected reservation {other:?}"),
        };
    if case == "corrupt" {
        fs::write(fixture.plugin().join("Plugin.dll"), b"XX1.0.0")?;
    }
    if case == "missing" {
        fs::remove_file(fixture.plugin().join("Plugin.dll"))?;
    }
    let before = snapshot(&fixture.target.target_path)?;
    let mut provider = provider(case);
    if case == "data-change" {
        provider.mutate_target = Some(fixture.plugin().join("Config/late.ini"));
    }
    let outcome = update_before_start_inner(
        &state,
        &fixture.storage,
        ThunderstoreStartRequest {
            descriptor: &fixture.descriptor,
            instance: &fixture.instance,
            install_guard: fixture.guard(&state).await?,
            reservation: &reservation,
            context: crate::commands::commands_runtime_prestart_update::PrestartUpdateLogContext {
                source: "manual",
                console_log_path: None,
            },
        },
        Some(&provider),
    )
    .await;
    match case {
        "corrupt" | "missing" | "cancel" | "data-change" => {
            assert!(outcome.is_err(), "{case} was incorrectly allowed to start");
            if case == "data-change" {
                assert_eq!(
                    fs::read(fixture.plugin().join("Config/late.ini"))?,
                    b"operator wrote during network query"
                );
                assert_eq!(fs::read(fixture.plugin().join("Plugin.dll"))?, b"MZ1.0.0");
            } else {
                assert_eq!(snapshot(&fixture.target.target_path)?, before);
            }
            if matches!(case, "corrupt" | "missing") {
                assert!(provider.queries.lock().unwrap().is_empty());
            }
        }
        _ => {
            let outcome = outcome.map_err(|error| format!("{case}: {error}"))?;
            let warnings = &outcome.warnings;
            match case {
                "network" | "download" => {
                    assert_eq!(warnings.len(), 1);
                    assert_eq!(
                        warnings[0].code,
                        app_core::RuntimeStartWarningCode::ModUpdateUsingCache
                    );
                    assert_eq!(snapshot(&fixture.target.target_path)?, before);
                }
                "unknown" => {
                    assert_eq!(warnings.len(), 1);
                    assert_eq!(
                        warnings[0].code,
                        app_core::RuntimeStartWarningCode::ModUpdateSourceRequired
                    );
                    assert!(provider.queries.lock().unwrap().is_empty());
                }
                "orphan" => {
                    assert_eq!(warnings.len(), 1);
                    assert_eq!(
                        warnings[0].code,
                        app_core::RuntimeStartWarningCode::ModUpdateSourceRequired
                    );
                    assert_eq!(warnings[0].item_ids, ["Author-Library"]);
                    assert!(
                        fixture
                            .target
                            .target_path
                            .join("thunderstore-Author-Library/Plugin.dll")
                            .is_file()
                    );
                }
                _ => {
                    assert!(warnings.is_empty());
                }
            }
            if matches!(case, "latest" | "orphan") {
                assert_eq!(fs::read(fixture.plugin().join("Plugin.dll"))?, b"MZ2.0.0");
            }
            if matches!(case, "pin" | "warm") {
                assert!(provider.downloads.lock().unwrap().is_empty());
                assert_eq!(provider.queries.lock().unwrap().len(), 2);
                assert_eq!(
                    provider.queries.lock().unwrap()[0].1,
                    if case == "pin" {
                        Some("1.0.0".into())
                    } else {
                        None
                    }
                );
            }
            drop(outcome);
        }
    }
    assert_eq!(
        fs::read(fixture.plugin().join("Config/user.ini"))?,
        b"operator config"
    );
    assert_eq!(
        fs::read(fixture.plugin().join("user-note.txt"))?,
        b"operator note"
    );
    drop(reservation);
    let reacquired = tokio::time::timeout(Duration::from_secs(2), fixture.guard(&state)).await??;
    drop(reacquired);
    Ok(())
}

#[tokio::test]
async fn thunderstore_start_real_updater_checks_hot_cache_updates_roots_and_exact_dependencies_preserves_pins_and_data()
-> Result<(), Box<dyn std::error::Error>> {
    for case in ["latest", "warm", "pin", "orphan"] {
        exercise(case).await?;
    }
    Ok(())
}
#[tokio::test]
async fn thunderstore_start_real_updater_only_falls_back_after_provider_failure_and_complete_unchanged_proof()
-> Result<(), Box<dyn std::error::Error>> {
    for case in [
        "network",
        "download",
        "unknown",
        "corrupt",
        "missing",
        "data-change",
        "cancel",
    ] {
        exercise(case).await?;
    }
    Ok(())
}
