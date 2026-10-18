//! Per-map INIs are consumed through explicit native -GameINI/-GameUserSettingsINI paths.
use super::*;

pub(in crate::templates::templates_materialize) fn render_map_files(
    templates_root: &Path,
    input: &ModuleTemplateRenderInput<'_>,
    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let settings = Value::Object(input.settings.clone());
    if !app_core::ark_maps::is_ark(input.module_id)
        || !app_core::ark_mod_scopes::has_overrides(&settings)
    {
        return Ok(());
    }
    let invalid = |error| materialization_error(input.module_id, input.config_dir, error);
    app_core::ark_mod_scopes::validate_overrides(input.module_id, &settings).map_err(invalid)?;
    let maps = app_core::ark_maps::parse_additional_maps(&settings).map_err(invalid)?;
    let shared_history = Snapshot::load(
        input.config_dir,
        &input.config_dir.join("instance.json"),
        input.module_id,
    )?;
    for key in app_core::ark_mod_scopes::configured_process_keys(&settings).map_err(invalid)? {
        let effective =
            app_core::ark_mod_scopes::effective_settings(input.module_id, &settings, &key)
                .map_err(invalid)?;
        let mut effective = effective
            .as_object()
            .cloned()
            .ok_or(StorageError::InvalidSettingsRoot)?;
        if let Some(map) = maps
            .iter()
            .find(|map| app_core::ark_maps::process_key(&map.id) == key)
        {
            effective.insert("map_name".into(), Value::String(map.map_name.clone()));
            effective.insert("server_name".into(), Value::String(map.name.clone()));
        }
        let ports = map_ports(input.ports, &key);
        let directory = input.config_dir.join("ark-maps").join(&key);
        crate::instance_isolation::paths::normalize_path(&directory)?;
        for name in [
            ARK_GAME_INI_FILE,
            ARK_GAME_USER_SETTINGS_FILE,
            history::OWNERSHIP_FILE,
        ] {
            crate::instance_isolation::paths::normalize_resource_path(&directory.join(name))?;
        }
        let map_input = ModuleTemplateRenderInput {
            config_dir: &directory,
            settings: &effective,
            ports: &ports,
            ..*input
        };
        let mut history = Snapshot::load_map(&directory, input.module_id)?;
        let mut count = 0;
        render_module_templates_with_writer(templates_root, &map_input, &mut |path, rendered| {
            let Some(filename) = path.file_name().and_then(|name| name.to_str()) else {
                return Ok(());
            };
            if ![ARK_GAME_INI_FILE, ARK_GAME_USER_SETTINGS_FILE].contains(&filename) {
                return Ok(());
            }
            // Only fixed root-level native files belong to this map's input contract.
            if path.parent() != Some(directory.as_path()) {
                return Ok(());
            }
            write_map_file(
                input,
                &map_input,
                path,
                &rendered,
                &shared_history,
                &mut history,
                files,
            )?;
            count += 1;
            Ok(())
        })?;
        if count != 2 {
            return Err(materialization_error(
                input.module_id,
                &directory,
                "ARK map requires both native INI templates".into(),
            ));
        }
        files.apply(vec![history.plan()?])?;
    }
    Ok(())
}

fn map_ports(ports: &[PortBinding], key: &str) -> Vec<PortBinding> {
    if key == "main" {
        return ports
            .iter()
            .filter(|port| !port.name.starts_with("map-"))
            .cloned()
            .collect();
    }
    let prefix = format!("{key}-");
    ports
        .iter()
        .filter_map(|port| {
            let name = port.name.strip_prefix(&prefix)?;
            ["game", "peer", "query", "rcon"]
                .contains(&name)
                .then(|| PortBinding {
                    name: name.into(),
                    ..port.clone()
                })
        })
        .collect()
}

fn write_map_file(
    shared_input: &ModuleTemplateRenderInput<'_>,
    input: &ModuleTemplateRenderInput<'_>,
    path: &Path,
    rendered: &str,
    shared_history: &Snapshot,
    history: &mut Snapshot,
    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let game_ini = path
        .file_name()
        .is_some_and(|name| name == ARK_GAME_INI_FILE);
    let filename = if game_ini {
        ARK_GAME_INI_FILE
    } else {
        ARK_GAME_USER_SETTINGS_FILE
    };
    let (original, mut existing) = read_document(path, input.module_id)?;
    let mut exact = history.keys(filename, Target::Config);
    if original.is_none() {
        // Start with preserved native content, then replace every shared managed
        // key with this map's freshly rendered settings. Shared Mod fragments must
        // not survive into a map that disables that Mod or supplies another value.
        let live = input
            .install_root
            .join("ShooterGame/Saved/Config/WindowsServer")
            .join(filename);
        let (live_bytes, live_document) = read_document(&live, input.module_id)?;
        existing = if live_bytes.is_some() {
            live_document
        } else {
            read_document(&shared_input.config_dir.join(filename), input.module_id)?.1
        };
        exact.extend(shared_history.keys(filename, Target::Config));
        exact.extend(shared_history.keys(filename, Target::Live));
    }
    let source = prepare_rendered(rendered, input.settings, game_ini)
        .map_err(|error| materialization_error(input.module_id, path, error))?;
    let current = source.keys();
    exact.extend(current.clone());
    let replacement = merge(
        existing,
        source,
        &owned_keys(input.module_id, game_ini),
        &exact,
        false,
    )
    .render();
    history.record(filename, Target::Config, current);
    files.apply(vec![ManagedConfigMergePlan {
        destination_path: path.to_owned(),
        replacement,
        original,
    }])
}
