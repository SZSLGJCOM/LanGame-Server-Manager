use super::*;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use app_core::INSTANCE_CREATION_DEFAULT_BIND_IP;

use crate::config_acceptance_test_support::paths::{
    expected_launch_argument_matches, resolve_expected_path_tokens,
};
use crate::config_acceptance_test_support::*;

#[test]
fn controlled_fixture_validates_real_launch_plan_arguments() {
    let repository = ControlledRepository::new();
    repository.write_module();
    repository.write_fixture(
        r#"{
  "fixture_version": 1,
  "module_id": "acceptancealpha",
  "settings": {"server_name": "LanGame 配置"},
  "expected": {
    "files": [],
    "launch": {
      "executable_suffix": "bin/server.exe",
      "arguments": ["--name", "LanGame 配置", "--config", "{{paths.config_dir}}", "--port", "27115"]
    }
  }
}"#,
    );

    run_runtime_acceptance_repository(&repository.modules_root(), None)
        .expect("controlled launch fixture should match build_launch_plan");
}

#[test]
fn controlled_fixture_rejects_malformed_json_and_argument_mismatch() {
    let malformed = ControlledRepository::new();
    malformed.write_module();
    malformed.write_fixture("{");
    assert!(
        run_runtime_acceptance_repository(&malformed.modules_root(), None)
            .unwrap_err()
            .contains("invalid fixture JSON")
    );

    let mismatch = ControlledRepository::new();
    mismatch.write_module();
    mismatch.write_fixture(
        r#"{
  "fixture_version": 1,
  "module_id": "acceptancealpha",
  "settings": {"server_name": "LanGame mismatch"},
  "expected": {
    "files": [],
    "launch": {"executable_suffix": "server.exe", "arguments": ["wrong"]}
  }
}"#,
    );
    assert!(
        run_runtime_acceptance_repository(&mismatch.modules_root(), None)
            .unwrap_err()
            .contains("launch arguments differ")
    );
}

#[test]
fn acceptance_argument_matching_is_path_token_scoped() {
    let instance_root = Path::new("acceptance").join("instance");
    let raw_path = "{{paths.logs_dir}}/CoreKeeperServer.log";
    let resolved_path = resolve_expected_path_tokens(
        raw_path,
        &instance_root,
        &instance_root.join("config"),
        &instance_root.join("install"),
        &instance_root.join("saves"),
    );
    let native_path = instance_root
        .join("logs")
        .join("CoreKeeperServer.log")
        .to_string_lossy()
        .into_owned();

    assert!(expected_launch_argument_matches(
        raw_path,
        &resolved_path,
        &native_path,
    ));
    let alternate_separators = native_path
        .chars()
        .map(|character| match character {
            '/' => '\\',
            '\\' => '/',
            other => other,
        })
        .collect::<String>();
    assert!(expected_launch_argument_matches(
        raw_path,
        &resolved_path,
        &alternate_separators,
    ));
    assert!(!expected_launch_argument_matches(
        "https://example.test/api",
        "https://example.test/api",
        r"https:\\example.test\api",
    ));
    assert!(!expected_launch_argument_matches(
        "secret/with/slashes",
        "secret/with/slashes",
        r"secret\with\slashes",
    ));
}

#[test]
fn controlled_fixture_rejects_settings_before_launch_rendering() {
    let repository = ControlledRepository::new();
    repository.write_module();
    repository.write_fixture(
        r#"{
  "fixture_version": 1,
  "module_id": "acceptancealpha",
  "settings": {"server_name": 42},
  "expected": {
    "files": [],
    "launch": {"executable_suffix": "bin/server.exe", "arguments": ["wrong"]}
  }
}"#,
    );

    assert!(
        run_runtime_acceptance_repository(&repository.modules_root(), None)
            .unwrap_err()
            .contains("invalid fixture settings")
    );
}

#[test]
fn controlled_fixture_rejects_unresolved_launch_tokens() {
    let repository = ControlledRepository::new();
    repository
        .write_module_with_args(r#"["--name", "{{settings.server_name}}", "{{settings.typo}}"]"#);
    repository.write_fixture(
        r#"{
  "fixture_version": 1,
  "module_id": "acceptancealpha",
  "settings": {"server_name": "LanGame"},
  "expected": {
    "files": [],
    "launch": {
      "executable_suffix": "bin/server.exe",
      "arguments": ["--name", "LanGame", "{{settings.typo}}"]
    }
  }
}"#,
    );

    assert!(
        run_runtime_acceptance_repository(&repository.modules_root(), None)
            .unwrap_err()
            .contains("unresolved arguments")
    );
}

#[test]
fn repository_native_launch_parameters_match_acceptance_fixtures() {
    let requested = std::env::var("GAME_CONFIG_ACCEPTANCE_MODULES").ok();
    run_runtime_acceptance_repository(&repository_root().join("modules"), requested.as_deref())
        .expect("repository runtime acceptance fixtures");
}

#[test]
fn astroneer_managed_logging_is_present_once_without_an_explicit_extra_log_flag() {
    let modules = app_modules::discover_modules(repository_root().join("modules")).unwrap();
    let module = modules
        .iter()
        .find(|module| module.summary.id == "astroneer")
        .unwrap();
    let fixture_path = module
        .root
        .join("config-fixtures/2026-09-08-native_console_probe_24411584.json");
    for extra_args in ["", "-NoSound"] {
        let mut fixture = read_acceptance_fixture(&fixture_path).unwrap();
        fixture.settings.insert(
            String::from("extra_launch_args"),
            Value::String(String::from(extra_args)),
        );
        fixture.expected_launch.arguments = ["-log", "-stdout", "-FullStdOutLogOutput"]
            .into_iter()
            .map(String::from)
            .collect();
        if !extra_args.is_empty() {
            fixture
                .expected_launch
                .arguments
                .push(String::from("-NoSound"));
        }
        let plan = validate_fixture_launch_plan(module, &fixture, &fixture_path)
            .expect("current managed logging and synthetic extra arguments");
        assert_eq!(
            plan.args
                .iter()
                .filter(|argument| argument.eq_ignore_ascii_case("-log"))
                .count(),
            1
        );
    }
}

#[test]
fn ark_ascended_launch_parameters_keep_native_ini_passwords_out_of_process_arguments() {
    let modules = app_modules::discover_modules(repository_root().join("modules")).unwrap();
    let module = modules
        .iter()
        .find(|module| module.summary.id == "arksurvivalascended")
        .unwrap();
    let fixture_path = module
        .root
        .join("config-fixtures/2026-07-13-steamcmd_anonymous_app_2430930.json");
    let fixture = read_acceptance_fixture(&fixture_path).unwrap();
    let plan = validate_fixture_launch_plan(module, &fixture, &fixture_path).unwrap();
    for (setting, native) in [
        ("admin_password", "ServerAdminPassword"),
        ("server_password", "ServerPassword"),
    ] {
        let password = fixture.settings[setting].as_str().unwrap();
        assert!(!password.is_empty());
        assert!(
            plan.args
                .iter()
                .all(|argument| !argument.contains(password) && !argument.contains(native))
        );
    }
}

fn run_runtime_acceptance_repository(
    modules_root: &Path,
    requested: Option<&str>,
) -> Result<(), String> {
    let modules = app_modules::discover_modules(modules_root)
        .map_err(|error| format!("failed to discover modules: {error}"))?;
    let known_modules = modules
        .iter()
        .map(|module| module.summary.id.clone())
        .collect::<BTreeSet<_>>();
    let fixture_paths = discover_fixture_paths(&modules)?;
    let selected = resolve_fixture_selection(&known_modules, &fixture_paths, requested)?;

    for module_id in selected {
        let module = modules
            .iter()
            .find(|module| module.summary.id == module_id)
            .ok_or_else(|| format!("selected module disappeared: {module_id}"))?;
        let paths = fixture_paths
            .get(&module_id)
            .ok_or_else(|| format!("module has no fixtures: {module_id}"))?;
        for fixture_path in paths {
            let fixture = read_acceptance_fixture(fixture_path)?;
            if fixture.module_id != module_id {
                return Err(format!(
                    "fixture module_id differs from directory: expected {module_id}, found {} at {}",
                    fixture.module_id,
                    fixture_path.display()
                ));
            }
            validate_fixture_launch_plan(module, &fixture, fixture_path)?;
        }
    }

    Ok(())
}

fn validate_fixture_launch_plan(
    module: &app_modules::ModuleDescriptor,
    fixture: &ConfigAcceptanceFixture,
    fixture_path: &Path,
) -> Result<LaunchPlan, String> {
    let temp_root = unique_system_temp_root(&format!("runtime-{}", module.summary.id));
    let result = (|| {
        let instance_id = config_acceptance_instance_id(&module.summary.id);
        let instance_name = CONFIG_ACCEPTANCE_INSTANCE_NAME;
        let instance_root = temp_root.join("instances").join(&instance_id);
        let config_dir = instance_root.join("config");
        let config_file_path = config_dir.join("instance.json");
        let games_root = temp_root.join("server-files");
        let install = module.install.as_ref().ok_or_else(|| {
            format!(
                "{} has no install contract for fixture {}",
                module.summary.id,
                fixture_path.display()
            )
        })?;
        let install_root = games_root.join(&install.shared_game_dir);
        let normalized_settings = app_storage::normalize_complete_instance_settings(
            Some(module),
            fixture.settings.clone(),
            &instance_id,
            instance_name,
            INSTANCE_CREATION_DEFAULT_BIND_IP,
        )
        .map_err(|source| {
            format!(
                "invalid fixture settings at {}: {source}",
                fixture_path.display()
            )
        })?;
        let saves_dir = app_storage::plan_instance_saves_dir(
            Some(module),
            &app_storage::InstanceSavePathContext {
                install_root: &install_root,
                instance_root: &instance_root,
                config_dir: &config_dir,
                instance_id: &instance_id,
                instance_name,
                module_id: &module.summary.id,
                settings: Some(&normalized_settings),
            },
        )
        .map_err(|source| {
            format!(
                "failed to plan fixture saves path at {}: {source}",
                fixture_path.display()
            )
        })?;

        for directory in [&config_dir, &saves_dir, &install_root] {
            std::fs::create_dir_all(directory).map_err(|source| {
                format!(
                    "failed to create acceptance path {}: {source}",
                    directory.display()
                )
            })?;
        }
        if let Some(suffix) = &fixture.expected_launch.executable_suffix {
            let executable = install_root.join(suffix);
            if let Some(parent) = executable.parent() {
                std::fs::create_dir_all(parent).map_err(|source| {
                    format!(
                        "failed to create executable parent {}: {source}",
                        parent.display()
                    )
                })?;
            }
            std::fs::write(&executable, []).map_err(|source| {
                format!(
                    "failed to write executable placeholder {}: {source}",
                    executable.display()
                )
            })?;
        }

        let settings = AppSettings {
            archives_root: String::new(),
            servers_root: temp_root.join("instances").to_string_lossy().into_owned(),
            games_root: games_root.to_string_lossy().into_owned(),
            modules_root: temp_root.join("modules").to_string_lossy().into_owned(),
            steamcmd_root: temp_root.join("steamcmd").to_string_lossy().into_owned(),
        };
        let module_details = ModuleDetails {
            summary: module.summary.clone(),
            schema_json: module.schema_json.clone(),
            default_ports: module.default_ports.clone(),
            install: module.install.clone(),
            process: module.process.clone(),
            workshop: module.workshop.clone(),
            mods: None,
            runtime: module.runtime.clone(),
        };
        let instance = InstanceDetails {
            summary: app_core::InstanceSummary {
                id: instance_id,
                name: String::from(instance_name),
                module_id: module.summary.id.clone(),
                status: app_core::InstanceStatus::Stopped,
                active_process_count: 0,
                bind_ip: String::from(INSTANCE_CREATION_DEFAULT_BIND_IP),
                port_count: module.default_ports.len(),
                autostart: false,
            },
            config_file_path: config_file_path.to_string_lossy().into_owned(),
            saves_path: saves_dir.to_string_lossy().into_owned(),
            backup_uses_declared_saves_path: true,
            auto_backup_on_stop: false,
            backup_retention_count: 0,
            settings_json: Value::Object(normalized_settings).to_string(),
            ports: module.default_ports.clone(),
            active_run: None,
        };
        let plan = build_launch_plan(&settings, &module_details, &instance).map_err(|error| {
            format!(
                "failed to build launch plan for {} at {}: {error}",
                module.summary.id,
                fixture_path.display()
            )
        })?;
        if let Some(issue) = plan
            .validation_issues
            .iter()
            .find(|issue| issue.code == "unresolved_launch_args")
        {
            return Err(format!(
                "launch plan contains unresolved arguments for {} at {}: {}",
                module.summary.id,
                fixture_path.display(),
                issue.message
            ));
        }

        if let Some(expected_suffix) = &fixture.expected_launch.executable_suffix {
            let actual = Path::new(&plan.executable_path);
            if !actual.ends_with(expected_suffix) {
                return Err(format!(
                    "launch executable differs for {} at {}: expected suffix {}, found {}",
                    module.summary.id,
                    fixture_path.display(),
                    expected_suffix.display(),
                    actual.display()
                ));
            }
        }

        let expected_arguments = fixture
            .expected_launch
            .arguments
            .iter()
            .map(|argument| {
                resolve_expected_path_tokens(
                    argument,
                    &instance_root,
                    &config_dir,
                    &install_root,
                    &saves_dir,
                )
            })
            .collect::<Vec<_>>();
        let arguments_match = plan.args.len() == expected_arguments.len()
            && fixture
                .expected_launch
                .arguments
                .iter()
                .zip(&expected_arguments)
                .zip(&plan.args)
                .all(|((raw_expected, resolved_expected), actual)| {
                    expected_launch_argument_matches(raw_expected, resolved_expected, actual)
                });
        if !arguments_match {
            return Err(format!(
                "launch arguments differ for {} at {}: expected {:?}, found {:?}",
                module.summary.id,
                fixture_path.display(),
                expected_arguments,
                plan.args
            ));
        }

        Ok(plan)
    })();
    let _ = std::fs::remove_dir_all(&temp_root);
    result
}

struct ControlledRepository {
    root: PathBuf,
}

impl ControlledRepository {
    fn new() -> Self {
        Self {
            root: unique_system_temp_root("runtime-controlled"),
        }
    }

    fn modules_root(&self) -> PathBuf {
        self.root.join("modules")
    }

    fn module_root(&self) -> PathBuf {
        self.modules_root().join("acceptancealpha")
    }

    fn write_module(&self) {
        self.write_module_with_args(
            r#"["--name", "{{settings.server_name}}", "--config", "{{paths.config_dir}}", "--port", "{{ports.game.port}}"]"#,
        );
    }

    fn write_module_with_args(&self, args_template: &str) {
        let module_root = self.module_root();
        std::fs::create_dir_all(&module_root).unwrap();
        std::fs::write(
            module_root.join("module.toml"),
            format!(
                r#"id = "acceptancealpha"
name = "Acceptance Alpha"
version = "1.0.0"

[[default_ports]]
name = "game"
protocol = "udp"
port = 27115

[install]
shared_game_dir = "acceptancealpha"

[process]
executable = "bin/server.exe"
args_template = {args_template}
"#
            ),
        )
        .unwrap();
        std::fs::write(
            module_root.join("schema.json"),
            r#"{"type":"object","properties":{"server_name":{"type":"string"}}}"#,
        )
        .unwrap();
    }

    fn write_fixture(&self, contents: &str) {
        let fixture_root = self.module_root().join("config-fixtures");
        std::fs::create_dir_all(&fixture_root).unwrap();
        std::fs::write(fixture_root.join("controlled.json"), contents).unwrap();
    }
}

impl Drop for ControlledRepository {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
