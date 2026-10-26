use super::*;
use app_core::INSTANCE_CREATION_DEFAULT_BIND_IP;

struct HumanitzFixture {
    root: PathBuf,
    config: PathBuf,
    install: PathBuf,
    module: app_modules::ModuleDescriptor,
}

impl HumanitzFixture {
    fn new() -> Self {
        let root = unique_test_root();
        let config = root.join("instances/humanitz/config");
        let install = root.join("games/humanitz");
        fs::create_dir_all(&config).unwrap();
        fs::create_dir_all(install.join("HumanitZServer")).unwrap();
        let module = app_modules::discover_modules(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules"),
        )
        .unwrap()
        .into_iter()
        .find(|module| module.summary.id == "humanitz")
        .unwrap();
        Self {
            root,
            config,
            install,
            module,
        }
    }

    fn native_path(&self) -> PathBuf {
        self.install.join("HumanitZServer/GameServerSettings.ini")
    }

    fn render(&self, settings: Value) -> Result<(), StorageError> {
        let settings = normalize_complete_instance_settings(
            Some(&self.module),
            settings.as_object().unwrap().clone(),
            "isolated-humanitz",
            "Isolated HumanitZ",
            INSTANCE_CREATION_DEFAULT_BIND_IP,
        )?;
        render_module_templates(
            &self.module.root.join("templates"),
            &ModuleTemplateRenderInput {
                config_dir: &self.config,
                install_root: &self.install,
                saves_dir: &self.root.join("saves"),
                instance_id: "isolated-humanitz",
                instance_name: "Isolated HumanitZ",
                module_id: "humanitz",
                bind_ip: INSTANCE_CREATION_DEFAULT_BIND_IP,
                autostart: false,
                settings: &settings,
                ports: &self.module.default_ports,
            },
        )?;
        materialize_module_support_files(&ModuleSupportMaterializationContext {
            storage_paths: &test_storage_paths(&self.root),
            module_id: "humanitz",
            install_root: &self.install,
            shared_install_root: &self.install,
            config_dir: &self.config,
            saves_dir: &self.root.join("saves"),
            instance_id: "isolated-humanitz",
            instance_running: false,
            settings: &settings,
        })
    }

    fn native(&self) -> String {
        fs::read_to_string(self.native_path()).unwrap()
    }
}

impl Drop for HumanitzFixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn assert_native_assignment(contents: &str, key: &str, expected: &str) {
    let values = contents
        .lines()
        .filter_map(|line| {
            let (name, value) = line.split_once('=')?;
            name.trim()
                .eq_ignore_ascii_case(key)
                .then_some(value.trim())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        values,
        vec![expected],
        "{key} must have one authoritative assignment"
    );
}

#[test]
fn humanitz_new_settings_preserve_native_values_when_unset() {
    let fixture = HumanitzFixture::new();
    fs::write(fixture.native_path(), "[World Settings]\nBackpackCleanup=7\nWeaponDurabilityMulti=0.75\nHeadshotOnlyMode=true\nUnknownFutureSetting=keep\n").unwrap();
    fixture.render(json!({})).unwrap();
    let native = fixture.native();
    assert_native_assignment(&native, "BackpackCleanup", "7");
    assert_native_assignment(&native, "WeaponDurabilityMulti", "0.75");
    assert_native_assignment(&native, "HeadshotOnlyMode", "true");
    assert_native_assignment(&native, "UnknownFutureSetting", "keep");
}

#[test]
fn humanitz_new_settings_write_explicit_values_and_clearing_keeps_last_native_values() {
    let fixture = HumanitzFixture::new();
    fs::write(fixture.native_path(), "[World Settings]\nBackpackCleanup=7\nWeaponDurabilityMulti=2\nHeadshotOnlyMode=true\n[World Settings]\nBackpackCleanup=9\n").unwrap();
    fixture
        .render(json!({
            "backpack_cleanup_days": 0,
            "weapon_durability_multiplier": 0.5,
            "headshot_only_mode": "disabled",
            "weapon_break": false,
            "recycle_car_days": 21
        }))
        .unwrap();
    for (key, value) in [
        ("BackpackCleanup", "0"),
        ("WeaponDurabilityMulti", "0.5"),
        ("HeadshotOnlyMode", "false"),
        ("WeaponBreak", "false"),
        ("RecycleCar", "21"),
    ] {
        assert_native_assignment(&fixture.native(), key, value);
    }
    fixture
        .render(json!({
            "backpack_cleanup_days": null,
            "weapon_durability_multiplier": null,
            "headshot_only_mode": "preserve"
        }))
        .unwrap();
    for (key, value) in [
        ("BackpackCleanup", "0"),
        ("WeaponDurabilityMulti", "0.5"),
        ("HeadshotOnlyMode", "false"),
    ] {
        assert_native_assignment(&fixture.native(), key, value);
    }
}

#[test]
fn humanitz_new_settings_allow_existing_extra_configuration_when_unset() {
    let fixture = HumanitzFixture::new();
    fixture.render(json!({ "settings_extra": "BackpackCleanup=8\nWeaponDurabilityMulti=0.25\nHeadshotOnlyMode=true" })).unwrap();
    for (key, value) in [
        ("BackpackCleanup", "8"),
        ("WeaponDurabilityMulti", "0.25"),
        ("HeadshotOnlyMode", "true"),
    ] {
        assert_native_assignment(&fixture.native(), key, value);
    }
}

#[test]
fn humanitz_new_settings_reject_conflicting_extra_assignments_without_writing() {
    let fixture = HumanitzFixture::new();
    let original = "[World Settings]\nBackpackCleanup=7\n";
    fs::write(fixture.native_path(), original).unwrap();
    for settings in [
        json!({ "backpack_cleanup_days": 0, "settings_extra": "backpackcleanup = 8" }),
        json!({ "weapon_durability_multiplier": 0.5, "settings_extra": "[World Settings]\nWeaponDurabilityMulti=2" }),
        json!({ "headshot_only_mode": "disabled", "settings_extra": "HeadshotOnlyMode=true" }),
        json!({ "settings_extra": "BackpackCleanup=4\n[World Settings]\nbackpackcleanup=5" }),
    ] {
        let error = fixture
            .render(settings)
            .expect_err("conflicting native ownership must fail");
        assert!(
            matches!(error, StorageError::InvalidModuleSetting { field, .. } if field == "settings_extra")
        );
        assert_eq!(fixture.native(), original);
        assert!(!fixture.config.join("GameServerSettings.ini").exists());
    }
}
