use super::*;

struct SeedFixture {
    root: PathBuf,
    paths: StoragePaths,
    config_dir: PathBuf,
    runtime_ini: PathBuf,
}

impl SeedFixture {
    fn new(existing: Option<&str>) -> Self {
        let root = unique_test_root();
        let paths = test_storage_paths(&root);
        let config_dir = paths
            .instances_root
            .join("srv-projectzomboid")
            .join("config");
        let runtime_ini = config_dir.join("runtime-home/Zomboid/Server/srv-projectzomboid.ini");
        fs::create_dir_all(&config_dir).unwrap();
        if let Some(existing) = existing {
            fs::create_dir_all(runtime_ini.parent().unwrap()).unwrap();
            fs::write(&runtime_ini, existing).unwrap();
        }
        Self {
            root,
            paths,
            config_dir,
            runtime_ini,
        }
    }

    fn materialize(&self, seed: &str) -> String {
        let mut settings = collect_schema_defaults_from_schema_json(
            Some(include_str!("../../../modules/projectzomboid/schema.json")),
            SchemaDefaultContext {
                instance_id: Some("srv-projectzomboid"),
                instance_name: Some("Seed regression"),
            },
        )
        .unwrap();
        settings.insert("world_seed".into(), Value::String(seed.into()));
        settings.insert(
            "server_name".into(),
            Value::String("Seed regression".into()),
        );
        let ports = [
            PortBinding {
                name: "game".into(),
                protocol: "udp".into(),
                port: 16261,
            },
            PortBinding {
                name: "direct".into(),
                protocol: "udp".into(),
                port: 16262,
            },
            PortBinding {
                name: "rcon".into(),
                protocol: "tcp".into(),
                port: 27015,
            },
        ];
        let install = self.paths.games_root.join("projectzomboid");
        let saves = self.root.join("saves");
        let templates =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules/projectzomboid/templates");
        render_module_templates(
            &templates,
            &ModuleTemplateRenderInput {
                config_dir: &self.config_dir,
                install_root: &install,
                saves_dir: &saves,
                instance_id: "srv-projectzomboid",
                instance_name: "Seed regression",
                module_id: "projectzomboid",
                bind_ip: "127.0.0.1",
                autostart: false,
                settings: &settings,
                ports: &ports,
            },
        )
        .unwrap();
        materialize_module_support_files(&ModuleSupportMaterializationContext {
            storage_paths: &self.paths,
            module_id: "projectzomboid",
            install_root: &install,
            shared_install_root: &install,
            config_dir: &self.config_dir,
            saves_dir: &saves,
            instance_id: "srv-projectzomboid",
            instance_running: false,
            settings: &settings,
        })
        .unwrap();
        fs::read_to_string(&self.runtime_ini).unwrap()
    }
}

impl Drop for SeedFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn blank_field_preserves_native_seed_across_repeated_materialization() {
    let fixture = SeedFixture::new(Some(
        "Seed=existing-world-seed\r\nAntiCheatProtectionType1=false\r\n",
    ));
    for _ in 0..2 {
        let rendered = fixture.materialize("");
        assert_eq!(
            rendered
                .lines()
                .filter(|line| line.starts_with("Seed="))
                .collect::<Vec<_>>(),
            ["Seed=existing-world-seed"]
        );
        assert!(rendered.contains("PublicName=Seed regression"));
        assert!(!rendered.contains("AntiCheatProtectionType1="));
    }
}

#[test]
fn explicit_seed_replaces_existing_native_seed() {
    let fixture = SeedFixture::new(Some("Seed=previous-seed\n"));
    let rendered = fixture.materialize("chosen-seed");
    assert_eq!(
        rendered
            .lines()
            .filter(|line| line.starts_with("Seed="))
            .collect::<Vec<_>>(),
        ["Seed=chosen-seed"]
    );
    assert!(!rendered.contains("previous-seed"));
    assert!(fixture.materialize("").contains("Seed=chosen-seed"));
}

#[test]
fn new_instance_without_seed_leaves_native_generation_available() {
    let fixture = SeedFixture::new(None);
    let rendered = fixture.materialize("");
    assert!(!rendered.lines().any(|line| line.starts_with("Seed=")));
    assert!(rendered.contains("PublicName=Seed regression"));
}
