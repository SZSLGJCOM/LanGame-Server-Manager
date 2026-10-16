//! Explicit real-game acceptance. The original installed package is read only;
//! only the newly copied package and this test's DesktopState may be mutated.
#![cfg(windows)]
use super::*;
use crate::commands::commands_file_mods::manage_manual_mod;

#[path = "commands_file_mods_acceptance_support.rs"]
mod support;

const MODULE: &str = "valheim";
const PACKAGE: &str = "thunderstore-JereKuusela-Server_devcommands";
const REFERENCE: &str =
    "https://thunderstore.io/c/valheim/p/JereKuusela/Server_devcommands/1.115.0/";
const ARCHIVE_SHA256: &str = "b2752c18342bc9cabf4734e3480cd2cc4aa9faa8955ba503d4f7cefcfa3e4b33";

#[tokio::test(flavor = "current_thread")]
#[ignore = "explicit opt-in: copies real Valheim, downloads reviewed BepInEx and Server_devcommands through production install, performs five private production start/stop cycles; no existing instance or firewall rules are modified"]
async fn isolated_valheim_thunderstore_file_mod_lifecycle() -> support::Result {
    let _serial = command_smoke_lock().lock().await;
    if std::env::var("LGSM_FILE_MODS_NATIVE").as_deref() != Ok("true") {
        return Err(
            "set LGSM_FILE_MODS_NATIVE=true for explicitly authorized native acceptance".into(),
        );
    }
    if WindowsPlatform::current_process_is_elevated()? {
        return Err(
            "native file Mod acceptance must run unelevated; firewall changes are not authorized"
                .into(),
        );
    }
    let source = support::input("LGSM_FILE_MODS_SOURCE_PACKAGE")?;
    let archive = support::input("LGSM_FILE_MODS_LOCAL_ARCHIVE")?;
    if support::hash(&archive)? != ARCHIVE_SHA256 {
        return Err(
            "local reimport archive does not match the reviewed Server_devcommands 1.115.0 ZIP"
                .into(),
        );
    }
    let evidence_parent = support::input("LGSM_FILE_MODS_EVIDENCE_ROOT")?;
    let evidence = evidence_parent.join(format!("run-{}", uuid::Uuid::new_v4().simple()));
    let repository = workspace_root().canonicalize()?;
    if source.starts_with(&repository)
        || evidence.starts_with(&repository)
        || repository.starts_with(&evidence)
        || evidence.starts_with(&source)
        || source.starts_with(&evidence)
    {
        return Err("native package and new evidence must be separate external paths".into());
    }
    fs::create_dir(&evidence)?;
    let source_before = support::source_hashes(&source)?;
    if !source_before.contains_key("valheim_server.exe") {
        return Err("source has no Valheim dedicated executable".into());
    }
    let copy_source = source.clone();
    let package = tokio::task::spawn_blocking(move || {
        package::copy_package(
            &copy_source,
            MODULE,
            None,
            16 * 1024 * 1024 * 1024,
            Duration::from_secs(600),
        )
    })
    .await??;
    let environment = environment::NativeEnvironment::set(&package.root)?;
    save_app_settings(AppSettings {
        archives_root: String::new(),
        servers_root: package.root.join("i").to_string_lossy().into_owned(),
        games_root: package.root.to_string_lossy().into_owned(),
        modules_root: workspace_root()
            .join("modules")
            .to_string_lossy()
            .into_owned(),
        steamcmd_root: package.root.join("steamcmd").to_string_lossy().into_owned(),
    })?;
    let storage = bootstrap_storage()?;
    initialize_database(&storage.paths).await?;
    let descriptors = discover_modules(&storage.paths.modules_root)?;
    sync_modules(&storage.paths, &descriptors).await?;
    sync_game_installs(
        &storage.paths,
        &[GameInstallSyncRecord {
            module_id: MODULE.into(),
            install_root: package.install_root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: None,
            mark_verified: true,
        }],
    )
    .await?;
    let descriptor = find_descriptor(&descriptors, MODULE)?;
    let desktop = DesktopState::default();
    desktop
        .app_state
        .write()
        .map_err(|_| "app settings lock poisoned")?
        .settings = storage.settings.clone();
    let app = tauri::test::mock_builder()
        .manage(desktop)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    let initial = creation::CreationMode {
        acquisition: creation::Acquisition::LocalPackage,
        instances: creation::InstanceMode::Default,
    }
    .create(
        state.clone(),
        &storage,
        descriptor,
        &package,
        // Mono's loader still has MAX_PATH limits. The managed scratch prefix
        // already carries the task identity, so keep the instance label short.
        "m",
    )
    .await?;
    let id = initial.summary.id.clone();
    let cleanup = NativeCleanup {
        state: &state,
        instance_id: id.clone(),
        cleanup_allowed: Arc::clone(&package.cleanup_allowed),
    };
    let mut settings: serde_json::Map<String, Value> =
        serde_json::from_str(&initial.settings_json)?;
    let smoke = readiness::SmokeManifest::load(descriptor)?;
    settings.extend(smoke.fixture.settings.clone());
    settings.insert("program_update".into(), json!({"policy":"pinned"}));
    settings.insert("public_server".into(), json!(0));
    settings.insert("crossplay_enabled".into(), json!(false));
    settings.insert(
        "world_name".into(),
        json!(format!("LGSMMod{}", uuid::Uuid::new_v4().simple())),
    );
    let details = app_storage::update_instance(
        &storage.paths,
        UpdateInstanceInput {
            id: id.clone(),
            bind_ip: "0.0.0.0".into(),
            auto_backup_on_stop: false,
            backup_retention_count: 2,
            settings_json: serde_json::to_string(&settings)?,
            ports: support::ports(&initial)?,
        },
    )
    .await?;
    let mut cycles = Vec::new();
    let verification: support::Result<Value> = async {
        install_manual_mod_references(state.clone(), id.clone(), vec![REFERENCE.into()]).await?;
        let runtime = support::plain(&creation::program_root(&read_instance_details(&storage.paths, &id).await?)?)?;
        if !runtime.starts_with(&support::plain(&package.root)?) || runtime == source {
            return Err("Mod installation escaped the isolated copied package".into());
        }
        let inventory = read_manual_mod_inventory(state.clone(), id.clone()).await?;
        let installed = inventory.items.iter().find(|item| item.management.as_ref().is_some_and(|state| state.package_id == PACKAGE))
            .ok_or("production installation did not record the expected Thunderstore identity")?;
        let plugin_root = support::plain(Path::new(&installed.path))?;
        let receipt: Value = serde_json::from_slice(&fs::read(runtime.join("BepInEx/plugins/.langame-online-mods.json"))?)?;
        let record = &receipt["packages"][PACKAGE];
        if record["version"] != "1.115.0" { return Err("online installer did not resolve the explicitly requested real package version".into()); }
        let dependencies = record["dependencies"].as_array().ok_or("online dependency declaration missing")?;
        if !dependencies.iter().any(|value| value.as_str().is_some_and(|value| value.starts_with("denikson-BepInExPack_Valheim-"))) {
            return Err("real package no longer declares the reviewed Valheim loader; review acceptance".into());
        }
        let cycle = support::Cycle { state: &state, storage: &storage, descriptor, smoke: &smoke,
            package: &package, runtime: &runtime, evidence: &evidence };
        cycles.push(Box::pin(support::run_cycle(&cycle, &details, "online-enabled", true)).await?);
        let plugin_config = runtime.join("BepInEx/config/server_devcommands.cfg");
        if fs::metadata(support::plain(&plugin_config)?)?.len() == 0 {
            return Err("real plugin initialization did not create a nonempty server_devcommands.cfg".into());
        }
        let global_config_before = support::hashes(&runtime.join("BepInEx/config"))?;
        let loader_before = support::hashes(&runtime.join("BepInEx/core"))?;
        let runtime_exe_before = support::hash(&runtime.join("valheim_server.exe"))?;
        fs::create_dir_all(plugin_root.join("config"))?;
        support::write_new(&plugin_root.join("config/operator.conf"), b"operator configuration")?;
        support::write_new(&plugin_root.join("operator-notes.md"), b"untracked operator notes")?;

        let consumer = package.root.join("dependency-consumer");
        fs::create_dir(&consumer)?;
        support::write_new(&consumer.join("manifest.json"), &serde_json::to_vec(&json!({
            "name":"LGSM_Consumer", "version_number":"1.0.0", "dependencies":["JereKuusela-Server_devcommands-1.115.0"],
        }))?)?;
        support::write_new(&consumer.join("readme.md"), b"metadata-only dependency consumer fixture; no executable files")?;
        let consumer_import = stage_manual_mod_files(state.clone(), id.clone(), vec![consumer.to_string_lossy().into_owned()]).await?;
        let consumer_inventory = read_manual_mod_inventory(state.clone(), id.clone()).await?;
        let consumer_key = consumer_inventory.items.iter().find_map(|item| {
            item.management.as_ref().filter(|management| management.dependencies.iter()
                .any(|dependency| dependency == "JereKuusela-Server_devcommands-1.115.0"))
                .map(|management| management.package_id.clone())
        }).ok_or("metadata-only dependency consumer did not retain its declaration")?;
        let before_reverse_checks = support::hashes(&runtime.join("BepInEx/plugins"))?;
        for action in ["disable", "delete"] {
            let error = manage_manual_mod(state.clone(), id.clone(), PACKAGE.into(), action.into()).await
                .expect_err("enabled managed consumer must protect its real package dependency");
            if !error.contains(&consumer_key) || before_reverse_checks != support::hashes(&runtime.join("BepInEx/plugins"))? {
                return Err("reverse dependency refusal changed published Mods".into());
            }
        }
        manage_manual_mod(state.clone(), id.clone(), consumer_key.clone(), "disable".into()).await?;
        let disabled_consumer_error = manage_manual_mod(state.clone(), id.clone(), PACKAGE.into(), "delete".into()).await
            .expect_err("a disabled managed consumer must still block deleting its dependency");
        if !disabled_consumer_error.contains(&consumer_key) { return Err(disabled_consumer_error.into()); }
        manage_manual_mod(state.clone(), id.clone(), consumer_key, "delete".into()).await?;

        // Only declared loader dependencies are external to the plugin. A user
        // cannot adopt BepInEx/core as an ordinary deletable Mod.
        let loader_rejection = manage_manual_mod(state.clone(), id.clone(), "core".into(), "adopt".into()).await
            .expect_err("loader/system roots cannot be adopted");
        if !loader_rejection.contains("protected") { return Err(loader_rejection.into()); }
        let missing = package.root.join("missing-dependency");
        fs::create_dir(&missing)?;
        support::write_new(&missing.join("manifest.json"), br#"{"name":"LGSM_MissingDependencyProbe","version_number":"1.0.0","dependencies":["LGSM-Missing-1.0.0"]}"#)?;
        support::write_new(&missing.join("readme.md"), b"metadata-only dependency refusal probe; no executable payload")?;
        let target_before = support::hashes(&runtime.join("BepInEx/plugins"))?;
        let missing_error = stage_manual_mod_files(state.clone(), id.clone(), vec![missing.to_string_lossy().into_owned()]).await
            .expect_err("explicit missing dependency must fail before publication");
        if !missing_error.contains("LGSM-Missing-1.0.0") || target_before != support::hashes(&runtime.join("BepInEx/plugins"))? {
            return Err("missing dependency rejection changed published Mods".into());
        }

        manage_manual_mod(state.clone(), id.clone(), PACKAGE.into(), "disable".into()).await?;
        cycles.push(Box::pin(support::run_cycle(&cycle, &details, "online-disabled", false)).await?);
        manage_manual_mod(state.clone(), id.clone(), PACKAGE.into(), "enable".into()).await?;
        cycles.push(Box::pin(support::run_cycle(&cycle, &details, "online-reenabled", true)).await?);
        let modified = support::hashes(&plugin_root)?.keys().find(|relative| {
            Path::new(relative).extension().and_then(|value| value.to_str()).is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "md" | "png"))
                && relative.as_str() != "operator-notes.md"
        }).cloned().ok_or("real package contains no non-payload file for modification-preservation probe")?;
        OpenOptions::new().append(true).open(plugin_root.join(&modified))?.write_all(b"\nLGSM operator preservation probe\n")?;
        let protected = ["config/operator.conf", "operator-notes.md", &modified].into_iter()
            .map(|relative| support::hash(&plugin_root.join(relative)).map(|hash| (relative.to_owned(), hash))).collect::<support::Result<std::collections::BTreeMap<_, _>>>()?;
        let deleted = manage_manual_mod(state.clone(), id.clone(), PACKAGE.into(), "delete".into()).await?;
        let preserved = support::plain(Path::new(deleted.preserved_path.as_deref().ok_or("deletion lost preserved operator files")?))?;
        if !preserved.starts_with(&package.root.canonicalize()?) || preserved.starts_with(runtime.join("BepInEx/plugins")) {
            return Err("preserved Mod data escaped the owned fixture or remains loadable".into());
        }
        for (relative, expected) in &protected {
            if support::hash(&preserved.join(relative))? != *expected { return Err(format!("protected Mod file changed: {relative}").into()); }
        }
        if deleted.removed_file_count == 0 || preserved.join("ServerDevcommands.dll").exists() || plugin_root.exists() {
            return Err("delete did not remove the real owned plugin payload from its active and retained roots".into());
        }
        cycles.push(Box::pin(support::run_cycle(&cycle, &details, "online-deleted", false)).await?);
        stage_manual_mod_files(state.clone(), id.clone(), vec![archive.to_string_lossy().into_owned()]).await?;
        cycles.push(Box::pin(support::run_cycle(&cycle, &details, "local-reimported", true)).await?);
        if global_config_before != support::hashes(&runtime.join("BepInEx/config"))?
            || loader_before != support::hashes(&runtime.join("BepInEx/core"))?
            || runtime_exe_before != support::hash(&runtime.join("valheim_server.exe"))? {
            return Err("Mod lifecycle changed global configuration, loader or native executable bytes".into());
        }
        let run_ids = cycles.iter().map(|cycle| cycle["run_id"].as_i64()).collect::<std::collections::BTreeSet<_>>();
        if run_ids.len() != 5 || run_ids.contains(&None) { return Err("native cycles reused a run identity".into()); }
        Ok(json!({"online_package":REFERENCE,"installed_version":record["version"],"declared_dependencies":dependencies,
            "local_reimport":"original reviewed Thunderstore ZIP through production local import",
            "local_archive_sha256":ARCHIVE_SHA256,"plugin_generated_configuration_sha256":support::hash(&plugin_config)?,
            "protected_file_sha256":protected,"preserved_fixture_path":preserved,
            "loader_root_adoption_rejected":true,"missing_dependency_prepublication_rejected":true,
            "reverse_dependency_enabled_disable_and_delete_rejected":true,
            "reverse_dependency_disabled_delete_rejected":true,
            "reverse_dependency_fixture":"production-imported metadata-only consumer of the real installed Server_devcommands; removed before game startup",
            "dependency_fixture_imported_files":consumer_import.copied_file_count,
            "synthetic_dependency_probe_contains_executable_files":false,
            "dependency_limit":"reviewed real loader, real installed dependency identity and metadata-only consumer; does not validate gameplay of another third-party dependency plugin",
            "global_bepinex_configuration_unchanged":true,"loader_core_unchanged":true,"runtime_executable_unchanged":true}))
    }.await;
    let source_unchanged = support::source_hashes(&source)? == source_before
        && support::hash(&archive)? == ARCHIVE_SHA256;
    let report = json!({"module":MODULE,"fixture":package.root,"source_package_unchanged":source_unchanged,
        "source_sha256":source_before,"cycles":cycles,"details":verification.as_ref().ok(),
        "error":verification.as_ref().err().map(ToString::to_string),"network":"private public=0, crossplay=false; native Valheim wildcard UDP on independently selected ports",
        "firewall_mutated":false,"scope":"production install/import/manage/start/readiness/stop; test AppHandle, no GUI assertions"});
    support::write_new(
        &evidence.join("file-mods-native-acceptance.json"),
        &serde_json::to_vec_pretty(&report)?,
    )?;
    println!(
        "FILE_MODS_NATIVE_ACCEPTANCE evidence={} completed_cycles={} source_unchanged={source_unchanged}",
        evidence.display(),
        cycles.len()
    );
    verification?;
    if !source_unchanged {
        return Err("read-only native source changed during acceptance".into());
    }
    drop(cleanup);
    drop(app);
    drop(environment);
    let disposable = package.root.clone();
    drop(package);
    if disposable.exists() {
        return Err("native processes passed but owned fixture cleanup did not finish".into());
    }
    Ok(())
}
