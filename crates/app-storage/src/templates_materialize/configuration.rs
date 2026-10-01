use super::*;

pub(crate) async fn write_pending_instance_configuration_in_worker(
    templates_root: &Path,
    context: &ModuleSupportMaterializationContext<'_>,
    config_path: &Path,
    input: InstanceConfigInput<'_>,
    settings_lock: &crate::instance_settings_lock::InstanceSettingsLock,
    prepared: Option<PreparedWorkshopConfiguration>,
    native_permissions: Option<crate::instance_native_settings::MoriaPermissionsSnapshot>,
) -> Result<ManagedConfigMutation, StorageError> {
    let templates_root = templates_root.to_owned();
    let storage_paths = context.storage_paths.clone();
    let install_root = context.install_root.to_owned();
    let shared_install_root = context.shared_install_root.to_owned();
    let config_dir = context.config_dir.to_owned();
    let saves_dir = context.saves_dir.to_owned();
    let config_path = config_path.to_owned();
    let instance_id = input.instance_id.to_owned();
    let instance_name = input.instance_name.to_owned();
    let module_id = input.module_id.to_owned();
    let bind_ip = input.bind_ip.to_owned();
    let autostart = input.autostart;
    let settings = input.settings;
    let ports = input.ports.to_vec();
    let instance_running = context.instance_running;
    // Keep filesystem work off the executor and retain the per-instance lease.
    settings_lock
        .spawn_blocking(move || {
            let render_input = ModuleTemplateRenderInput {
                config_dir: &config_dir,
                install_root: &install_root,
                saves_dir: &saves_dir,
                instance_id: &instance_id,
                instance_name: &instance_name,
                module_id: &module_id,
                bind_ip: &bind_ip,
                autostart,
                settings: &settings,
                ports: &ports,
            };
            let context = ModuleSupportMaterializationContext {
                storage_paths: &storage_paths,
                module_id: &module_id,
                install_root: &install_root,
                shared_install_root: &shared_install_root,
                config_dir: &config_dir,
                saves_dir: &saves_dir,
                instance_id: &instance_id,
                instance_running,
                settings: &settings,
            };
            write_pending_configuration_with_native_snapshot(
                &templates_root,
                &render_input,
                &context,
                &config_path,
                InstanceConfigInput {
                    instance_id: &instance_id,
                    instance_name: &instance_name,
                    module_id: &module_id,
                    bind_ip: &bind_ip,
                    autostart,
                    settings: settings.clone(),
                    ports: &ports,
                },
                prepared,
                native_permissions,
            )
        })
        .await
        .map_err(|error| StorageError::BlockingTaskFailed {
            operation: "materializing instance configuration",
            message: error.to_string(),
        })?
}

pub(crate) fn write_pending_instance_configuration(
    templates_root: &Path,
    render_input: &ModuleTemplateRenderInput<'_>,
    context: &ModuleSupportMaterializationContext<'_>,
    config_path: &Path,
    input: InstanceConfigInput<'_>,
    prepared: Option<PreparedWorkshopConfiguration>,
) -> Result<ManagedConfigMutation, StorageError> {
    write_pending_configuration_with_native_snapshot(
        templates_root,
        render_input,
        context,
        config_path,
        input,
        prepared,
        None,
    )
}

fn write_pending_configuration_with_native_snapshot(
    templates_root: &Path,
    render_input: &ModuleTemplateRenderInput<'_>,
    context: &ModuleSupportMaterializationContext<'_>,
    config_path: &Path,
    input: InstanceConfigInput<'_>,
    prepared: Option<PreparedWorkshopConfiguration>,
    native_permissions: Option<crate::instance_native_settings::MoriaPermissionsSnapshot>,
) -> Result<ManagedConfigMutation, StorageError> {
    let mut files = ManagedConfigMutation::new(context.module_id);
    let result = (|| {
        ark_ini::capture_previous_ownership(context, config_path, &mut files)?;
        if !context.instance_running {
            if let Some(snapshot) = native_permissions {
                snapshot.verify_unchanged(context.instance_id)?;
                files.apply(vec![snapshot.into_write_plan(context.settings)])?;
            }
            render_module_templates_with_writer(
                templates_root,
                render_input,
                &mut |path, rendered| {
                    write_rendered_preserving_ark_ini(render_input, path, rendered, &mut files)
                },
            )?;
            if context.module_id == "soulmask" {
                combine_soulmask_profile_templates_pending(context.config_dir, &mut files)?;
            }
        }
        if !context.instance_running {
            materialize_module_support_files_pending(context, &mut files, prepared)?;
        }
        files.write(config_path, &render_instance_config(input)?)?;
        Ok(())
    })();
    match result {
        Ok(()) => Ok(files),
        Err(error) => Err(files.rollback_after(error)),
    }
}
