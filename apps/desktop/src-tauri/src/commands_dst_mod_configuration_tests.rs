use super::*;

const MOD_ID: &str = "4999999999";

struct Fixture {
    root: PathBuf,
    storage: StorageBootstrap,
    instance_id: String,
    instance_root: PathBuf,
    program_root: PathBuf,
}

impl Fixture {
    async fn new(prefer_existing_install: bool) -> Result<Self, Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "langame-dst-mod-configuration-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(root.join("db"))?;
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let paths = app_storage::StoragePaths {
            app_data_root: root.clone(),
            settings_path: root.join("settings.json"),
            database_path: root.join("db/test.db"),
            logs_root: root.join("logs"),
            modules_root: workspace.join("modules"),
            migrations_root: workspace.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances/.trash"),
        };
        let storage = app_storage::bootstrap_storage_with_paths(paths.clone())?;
        initialize_database(&paths).await?;
        let descriptors = discover_modules(&paths.modules_root)?;
        let descriptor = find_descriptor(&descriptors, "dontstarve")?;
        sync_modules(&paths, std::slice::from_ref(descriptor)).await?;
        prepare_fake_registered_program(&paths, descriptor).await?;
        let created = app_storage::create_instance_with_options(
            &paths,
            descriptor,
            CreateInstanceInput {
                name: "DST Mod configuration".into(),
                module_id: "dontstarve".into(),
            },
            app_storage::InstanceCreationOptions {
                prefer_existing_install,
                require_clean_program: true,
                program_mode: Some(app_storage::InstanceProgramMode::Independent),
                ..Default::default()
            },
        )
        .await?;
        let instance_id = created.provisioning.summary.id;
        let instance_root = paths.instances_root.join(&instance_id);
        Ok(Self {
            root,
            storage,
            instance_id,
            instance_root,
            program_root: created.effective_install_root,
        })
    }

    fn instance_mod(&self) -> PathBuf {
        self.instance_root
            .join("data/ugc/Master/content/322330")
            .join(MOD_ID)
            .join("modinfo.lua")
    }

    fn write_mod(path: &Path, name: &str, option: &str) -> std::io::Result<()> {
        fs::create_dir_all(path.parent().expect("fixture Mod directory"))?;
        fs::write(
            path,
            format!(
                "name = {name:?}\nconfiguration_options = {{{{name = {option:?}, \
                 label = 'Fixture option', default = true, options = \
                 {{{{description = 'Enabled', data = true}}, \
                 {{description = 'Disabled', data = false}}}}}}}}\n"
            ),
        )
    }

    async fn read(&self) -> Result<Vec<DstModConfigurationSpec>, String> {
        let app = tauri::test::mock_builder()
            .manage(DesktopState::from_storage(&self.storage))
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .map_err(|error| error.to_string())?;
        read_dontstarve_mod_configuration_specs(
            app.state::<DesktopState>(),
            self.instance_id.clone(),
            vec![MOD_ID.into()],
            Some("zh-CN".into()),
        )
        .await
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn assert_instance_configuration(specs: &[DstModConfigurationSpec]) {
    assert_eq!(specs.len(), 1);
    let spec = &specs[0];
    assert_eq!(spec.mod_id, MOD_ID);
    assert_eq!(spec.status, "loaded", "{:?}", spec.message);
    assert_eq!(spec.mod_name.as_deref(), Some("Instance UGC Mod"));
    assert_eq!(spec.options.len(), 1);
    assert_eq!(spec.options[0].name, "instance_option");
    assert_eq!(spec.options[0].options.len(), 2);
}

#[tokio::test(flavor = "current_thread")]
async fn dst_mod_configuration_reads_exclusive_program_and_prefers_instance_ugc()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new(true).await?;
    let runtime = fixture.instance_root.join("runtime");
    let binding = runtime.join(".langame-exclusive-program.json");
    let binding_before = fs::read(&binding)?;
    assert!(!runtime.join(".langame-private-runtime").exists());
    assert!(app_storage::instance_uses_exclusive_program(
        &fixture.instance_root
    )?);
    Fixture::write_mod(
        &fixture
            .program_root
            .join(format!("mods/workshop-{MOD_ID}/modinfo.lua")),
        "Program fallback Mod",
        "fallback_option",
    )?;
    Fixture::write_mod(
        &fixture.instance_mod(),
        "Instance UGC Mod",
        "instance_option",
    )?;
    let mod_before = fs::read(fixture.instance_mod())?;

    assert_instance_configuration(&fixture.read().await?);

    assert_eq!(fs::read(&binding)?, binding_before);
    assert_eq!(fs::read(fixture.instance_mod())?, mod_before);
    assert!(!runtime.join(".langame-private-runtime").exists());
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn dst_mod_configuration_reads_private_copy_runtime() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = Fixture::new(false).await?;
    let marker = fixture
        .instance_root
        .join("runtime/.langame-private-runtime");
    assert_eq!(fs::read(&marker)?, b"managed\n");
    assert!(!app_storage::instance_uses_exclusive_program(
        &fixture.instance_root
    )?);
    Fixture::write_mod(
        &fixture.instance_mod(),
        "Instance UGC Mod",
        "instance_option",
    )?;

    assert_instance_configuration(&fixture.read().await?);

    assert_eq!(fs::read(&marker)?, b"managed\n");
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn dst_mod_configuration_rejects_missing_or_invalid_private_markers()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new(false).await?;
    let marker = fixture
        .instance_root
        .join("runtime/.langame-private-runtime");
    Fixture::write_mod(
        &fixture.instance_mod(),
        "Instance UGC Mod",
        "instance_option",
    )?;
    let mod_before = fs::read(fixture.instance_mod())?;
    fs::remove_file(&marker)?;
    let error = fixture.read().await.unwrap_err();
    assert!(error.contains("missing a valid private marker"), "{error}");
    assert!(!marker.exists());

    fs::write(&marker, b"invalid marker\n")?;
    let error = fixture.read().await.unwrap_err();
    assert!(error.contains("missing a valid private marker"), "{error}");
    assert_eq!(fs::read(&marker)?, b"invalid marker\n");
    assert_eq!(fs::read(fixture.instance_mod())?, mod_before);
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn dst_mod_configuration_rejects_changed_exclusive_program_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new(true).await?;
    Fixture::write_mod(
        &fixture.instance_mod(),
        "Instance UGC Mod",
        "instance_option",
    )?;
    let identity_path = fixture.program_root.join(".langame-program-identity.json");
    let mut identity: Value = serde_json::from_slice(&fs::read(&identity_path)?)?;
    identity["id"] = json!("different-program-identity");
    let changed = serde_json::to_vec(&identity)?;
    fs::write(&identity_path, &changed)?;

    let error = fixture.read().await.unwrap_err();

    assert!(error.contains("shared program identity changed"), "{error}");
    assert_eq!(fs::read(&identity_path)?, changed);
    assert!(fixture.instance_mod().exists());
    assert!(
        !fixture
            .instance_root
            .join("runtime/.langame-private-runtime")
            .exists()
    );
    Ok(())
}
