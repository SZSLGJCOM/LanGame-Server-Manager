use super::*;

use crate::instance_settings_lock::InstanceSettingsLock;

use super::templates_materialize::apply_pending_world_update;

#[path = "templates_materialize/barotrauma.rs"]
mod barotrauma;

pub(crate) async fn apply_module_prestart_support(
    context: &ModuleSupportMaterializationContext<'_>,
    settings_lock: &InstanceSettingsLock,
) -> Result<(), StorageError> {
    if context.instance_running {
        return Err(StorageError::ModuleSupportMaterialization {
            module_id: String::from(context.module_id),
            path: context.install_root.to_path_buf(),
            message: String::from("pre-start support requires a stopped instance"),
        });
    }
    if context.module_id == "rimworld" {
        super::rimworld::validate_before_start(context)?;
    }
    if context.module_id == "projectzomboid" {
        super::projectzomboid_policy::validate_before_start(context.settings)?;
    }
    if context.module_id == "barotrauma" {
        let install = context.install_root.to_owned();
        let instance = context.config_dir.to_owned();
        // Program/content preparation belongs to stopped startup, not ordinary
        // configuration saves. Its worker owns the lock through cancellation.
        settings_lock
            .spawn_blocking(move || barotrauma::materialize_runtime(&install, &instance))
            .await
            .map_err(|error| StorageError::BlockingTaskFailed {
                operation: "preparing the Barotrauma instance runtime",
                message: error.to_string(),
            })??;
    }
    if context.module_id == "windrose" {
        apply_pending_world_update(context.install_root, context.config_dir, context.settings)
            .await
            .map_err(|error| StorageError::ModuleSupportMaterialization {
                module_id: String::from("windrose"),
                path: context.install_root.to_path_buf(),
                message: error.to_string(),
            })?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "templates_prestart_tests.rs"]
mod tests;
