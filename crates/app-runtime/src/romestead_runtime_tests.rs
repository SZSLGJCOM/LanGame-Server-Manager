use super::*;
use crate::{build_launch_plan, build_launch_plan_with_override};
use app_core::{InstanceDetails, InstanceStatus, InstanceSummary, ModuleDetails};
use std::path::PathBuf;

struct Fixture {
    root: PathBuf,
    settings: AppSettings,
    runtime: PathBuf,
    install: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = crate::test_support::unique_test_root();
        let runtime = root.join("configured tools").join("dotnet");
        let settings = AppSettings {
            archives_root: String::new(),
            servers_root: root.join("instances").to_string_lossy().into_owned(),
            games_root: root.join("games").to_string_lossy().into_owned(),
            modules_root: root.join("modules").to_string_lossy().into_owned(),
            steamcmd_root: root
                .join("configured tools")
                .join("steamcmd")
                .to_string_lossy()
                .into_owned(),
        };
        let install = Path::new(&settings.games_root).join("romestead");
        fs::create_dir_all(&install).unwrap();
        write_runtime_config(&install, "8.0.0");
        let fixture = Self {
            root,
            settings,
            runtime,
            install,
        };
        fixture.install_runtime("8.0.28");
        fixture
    }

    fn install_runtime(&self, version: &str) {
        for relative in [
            String::from("dotnet.exe"),
            format!("host/fxr/{version}/hostfxr.dll"),
            format!("shared/Microsoft.NETCore.App/{version}/coreclr.dll"),
            format!("shared/Microsoft.NETCore.App/{version}/hostpolicy.dll"),
            format!("shared/Microsoft.NETCore.App/{version}/System.Private.CoreLib.dll"),
        ] {
            let path = self.runtime.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"runtime fixture").unwrap();
        }
    }

    fn environment(&self) -> BTreeMap<String, String> {
        let mut environment = BTreeMap::new();
        apply_portable_runtime(&self.settings, "romestead", &self.install, &mut environment);
        environment
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn write_runtime_config(install: &Path, version: &str) {
    fs::create_dir_all(install).unwrap();
    fs::write(install.join("Server.runtimeconfig.json"), serde_json::json!({
        "runtimeOptions": { "frameworks": [{ "name": "Microsoft.NETCore.App", "version": version }] }
    }).to_string()).unwrap();
}

#[test]
fn romestead_portable_runtime_follows_configured_tools_across_launch_roots() {
    let fixture = Fixture::new();
    let descriptor =
        app_modules::discover_modules(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules"))
            .unwrap()
            .into_iter()
            .find(|module| module.summary.id == "romestead")
            .unwrap();
    let module = ModuleDetails {
        summary: descriptor.summary,
        schema_json: descriptor.schema_json,
        default_ports: descriptor.default_ports.clone(),
        install: descriptor.install,
        process: descriptor.process,
        workshop: descriptor.workshop,
        mods: None,
        runtime: descriptor.runtime,
    };
    let instance_root = Path::new(&fixture.settings.servers_root).join("second");
    fs::create_dir_all(instance_root.join("config")).unwrap();
    let instance = InstanceDetails {
        summary: InstanceSummary {
            id: String::from("second"),
            name: String::from("Second"),
            module_id: String::from("romestead"),
            status: InstanceStatus::Stopped,
            active_process_count: 0,
            bind_ip: String::from("0.0.0.0"),
            port_count: 1,
            autostart: false,
        },
        config_file_path: instance_root
            .join("config/instance.json")
            .to_string_lossy()
            .into_owned(),
        saves_path: instance_root
            .join("runtime/saved_worlds")
            .to_string_lossy()
            .into_owned(),
        backup_uses_declared_saves_path: true,
        auto_backup_on_stop: false,
        backup_retention_count: 0,
        settings_json: String::from("{}"),
        ports: descriptor.default_ports,
        active_run: None,
    };
    let private = instance_root.join("runtime");
    let isolated = fixture.root.join("isolated-copy");
    for root in [&fixture.install, &private, &isolated] {
        write_runtime_config(root, "8.0.0");
        fs::write(root.join("start-romestead.bat"), b"fixture").unwrap();
        let plan = if root == &fixture.install {
            build_launch_plan(&fixture.settings, &module, &instance).unwrap()
        } else {
            build_launch_plan_with_override(&fixture.settings, &module, &instance, root.to_str())
                .unwrap()
        };
        assert_eq!(Path::new(&plan.install_root), root);
        assert_eq!(
            plan.environment.get("DOTNET_ROOT_X64"),
            Some(&fixture.runtime.to_string_lossy().into_owned())
        );
        assert_eq!(
            plan.environment.get("DOTNET_ROOT"),
            plan.environment.get("DOTNET_ROOT_X64")
        );
        assert!(!plan.environment.contains_key("PATH"));
    }
}

#[test]
fn romestead_missing_or_incompatible_runtime_preserves_system_discovery() {
    let fixture = Fixture::new();
    for relative in [
        "dotnet.exe",
        "host/fxr/8.0.28/hostfxr.dll",
        "shared/Microsoft.NETCore.App/8.0.28/coreclr.dll",
        "shared/Microsoft.NETCore.App/8.0.28/hostpolicy.dll",
        "shared/Microsoft.NETCore.App/8.0.28/System.Private.CoreLib.dll",
    ] {
        fs::remove_file(fixture.runtime.join(relative)).unwrap();
        assert!(fixture.environment().is_empty(), "incomplete: {relative}");
        fixture.install_runtime("8.0.28");
    }
    for version in ["8.0.29", "8.1.0", "9.0.0"] {
        write_runtime_config(&fixture.install, version);
        assert!(fixture.environment().is_empty(), "incompatible: {version}");
    }
    fs::write(
        fixture.install.join("Server.runtimeconfig.json"),
        "invalid JSON",
    )
    .unwrap();
    assert!(fixture.environment().is_empty());
    fs::remove_file(fixture.install.join("Server.runtimeconfig.json")).unwrap();
    assert!(fixture.environment().is_empty());
}

#[test]
fn romestead_runtime_does_not_override_other_modules_or_exact_patch_policy() {
    let fixture = Fixture::new();
    let mut environment =
        BTreeMap::from([(String::from("DOTNET_ROOT_X64"), String::from("existing"))]);
    apply_portable_runtime(
        &fixture.settings,
        "barotrauma",
        &fixture.install,
        &mut environment,
    );
    assert_eq!(
        environment.get("DOTNET_ROOT_X64").map(String::as_str),
        Some("existing")
    );
    fs::write(fixture.install.join("Server.runtimeconfig.json"), serde_json::json!({
        "runtimeOptions": { "framework": { "name": "Microsoft.NETCore.App", "version": "8.0.0" }, "rollForward": "Disable" }
    }).to_string()).unwrap();
    assert!(fixture.environment().is_empty());
    fixture.install_runtime("8.0.0");
    fs::remove_file(fixture.runtime.join("host/fxr/8.0.0/hostfxr.dll")).unwrap();
    assert!(fixture.environment().contains_key("DOTNET_ROOT_X64"));
}
