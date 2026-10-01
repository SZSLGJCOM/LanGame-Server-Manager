use std::collections::BTreeSet;
use std::path::Path;

use app_core::INSTANCE_CREATION_DEFAULT_BIND_IP;
use app_modules::ModuleDescriptor;

use super::config_acceptance_formats::{AcceptanceOutputRoots, verify_expected_file};
use super::config_acceptance_support::initial_files::{
    InitialFileRoots, materialize_initial_files,
};
use super::config_acceptance_support::{
    CONFIG_ACCEPTANCE_INSTANCE_NAME, ConfigAcceptanceFixture, config_acceptance_instance_id,
    discover_fixture_paths, read_acceptance_fixture, resolve_fixture_selection,
    unique_system_temp_root,
};
use super::{
    ModuleSupportMaterializationContext, ModuleTemplateRenderInput, StoragePaths,
    materialize_module_support_files, normalize_complete_instance_settings,
    render_module_templates,
};
use crate::save_paths::{InstanceSavePathContext, planned_instance_saves_dir};

pub(super) fn run_storage_acceptance_repository(
    modules_root: impl AsRef<Path>,
    requested: Option<&str>,
) -> Result<(), String> {
    let modules_root = modules_root.as_ref();
    let modules = app_modules::discover_modules(modules_root).map_err(|source| {
        format!(
            "failed to discover modules from {}: {source}",
            modules_root.display()
        )
    })?;
    let known = modules
        .iter()
        .map(|module| module.summary.id.clone())
        .collect::<BTreeSet<_>>();
    let fixtures = discover_fixture_paths(&modules)?;
    let selected = resolve_fixture_selection(&known, &fixtures, requested)?;

    for module_id in selected {
        let module = modules
            .iter()
            .find(|module| module.summary.id == module_id)
            .ok_or_else(|| format!("selected module disappeared: {module_id}"))?;
        for fixture_path in fixtures
            .get(&module_id)
            .ok_or_else(|| format!("module has no fixtures: {module_id}"))?
        {
            let fixture = read_acceptance_fixture(fixture_path)?;
            if fixture.module_id != module_id {
                return Err(format!(
                    "fixture module_id {:?} does not match module {module_id:?} at {}",
                    fixture.module_id,
                    fixture_path.display()
                ));
            }
            run_storage_acceptance_fixture(module, fixture_path, &fixture)?;
        }
    }

    Ok(())
}

fn run_storage_acceptance_fixture(
    module: &ModuleDescriptor,
    fixture_path: &Path,
    fixture: &ConfigAcceptanceFixture,
) -> Result<(), String> {
    let test_root = unique_system_temp_root(&format!("storage-{}", module.summary.id));
    let result = (|| {
        let instance_id = config_acceptance_instance_id(&module.summary.id);
        let instance_name = CONFIG_ACCEPTANCE_INSTANCE_NAME;
        let settings = normalize_complete_instance_settings(
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
        let instance_root = test_root.join("instances").join(&instance_id);
        let config_dir = instance_root.join("config");
        let install_directory = module
            .install
            .as_ref()
            .map(|install| install.shared_game_dir.as_str())
            .unwrap_or(&module.summary.id);
        let shared_install_root = test_root.join("games").join(install_directory);
        let install_root = instance_root.join("runtime");
        let saves_dir = planned_instance_saves_dir(
            Some(module),
            &InstanceSavePathContext {
                install_root: &install_root,
                instance_root: &instance_root,
                config_dir: &config_dir,
                instance_id: &instance_id,
                instance_name,
                module_id: &module.summary.id,
                settings: Some(&settings),
            },
        )
        .map_err(|source| {
            format!(
                "failed to plan isolated saves path for fixture {}: {source}",
                fixture_path.display()
            )
        })?;
        for path in [
            &instance_root,
            &config_dir,
            &saves_dir,
            &install_root,
            &shared_install_root,
        ] {
            std::fs::create_dir_all(path).map_err(|source| {
                format!(
                    "failed to create isolated acceptance path {}: {source}",
                    path.display()
                )
            })?;
        }

        let marker = install_root.join(crate::private_runtime::PRIVATE_RUNTIME_MARKER);
        std::fs::write(&marker, b"managed\n").map_err(|source| {
            format!(
                "failed to mark isolated acceptance runtime {}: {source}",
                marker.display()
            )
        })?;

        materialize_initial_files(
            &fixture.initial_files,
            &InitialFileRoots {
                config: &config_dir,
                install: &install_root,
                saves: &saves_dir,
                instance: &instance_root,
            },
            fixture_path,
        )?;

        let storage_paths = acceptance_storage_paths(&test_root, &module.root);
        render_module_templates(
            &module.root.join("templates"),
            &ModuleTemplateRenderInput {
                config_dir: &config_dir,
                install_root: &install_root,
                saves_dir: &saves_dir,
                instance_id: &instance_id,
                instance_name,
                module_id: &module.summary.id,
                bind_ip: INSTANCE_CREATION_DEFAULT_BIND_IP,
                autostart: false,
                settings: &settings,
                ports: &module.default_ports,
            },
        )
        .map_err(|source| {
            format!(
                "failed to render storage fixture {}: {source}",
                fixture_path.display()
            )
        })?;
        materialize_module_support_files(&ModuleSupportMaterializationContext {
            storage_paths: &storage_paths,
            module_id: &module.summary.id,
            install_root: &install_root,
            shared_install_root: &shared_install_root,
            config_dir: &config_dir,
            saves_dir: &saves_dir,
            instance_id: &instance_id,
            instance_running: false,
            settings: &settings,
        })
        .map_err(|source| {
            format!(
                "failed to materialize storage fixture {}: {source}",
                fixture_path.display()
            )
        })?;

        for expected in &fixture.expected_files {
            verify_expected_file(
                expected,
                &AcceptanceOutputRoots {
                    config: &config_dir,
                    install: &install_root,
                    saves: &saves_dir,
                    instance: &instance_root,
                },
                fixture_path,
            )?;
        }

        Ok(())
    })();

    let _ = std::fs::remove_dir_all(&test_root);
    result
}

fn acceptance_storage_paths(test_root: &Path, module_root: &Path) -> StoragePaths {
    let app_data_root = test_root.join("data");
    StoragePaths {
        app_data_root: app_data_root.clone(),
        settings_path: app_data_root.join("settings.json"),
        database_path: app_data_root.join("db").join("lgs.db"),
        logs_root: app_data_root.join("logs"),
        modules_root: module_root.parent().unwrap_or(module_root).to_path_buf(),
        migrations_root: test_root.join("migrations"),
        steamcmd_root: test_root.join("cmd").join("steamcmd"),
        games_root: test_root.join("games"),
        instances_root: test_root.join("instances"),
        archives_root: test_root.join("instances").join(".trash"),
    }
}
