use super::*;

use crate::instance_settings_lock::InstanceSettingsLock;

use super::templates_materialize::apply_pending_world_update;

#[path = "templates_materialize/barotrauma.rs"]
mod barotrauma;

pub(crate) async fn apply_module_prestart_support(
    context: &ModuleSupportMaterializationContext<'_>,
    settings_lock: &InstanceSettingsLock,
    descriptor: Option<&app_modules::ModuleDescriptor>,
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
        let root = context.install_root.to_owned();
        let requested_feature = [
            ("action_market_use_global_server", "The cross-server market"),
            ("action_rollback_is_enabled", "Player save rollback"),
        ]
        .into_iter()
        .find_map(|(key, name)| {
            (context.settings.get(key).and_then(Value::as_bool) == Some(true)).then_some(name)
        });
        let descriptor = descriptor.cloned();
        settings_lock
            .spawn_blocking(move || {
                super::rimworld::validate_release_before_start(
                    &root,
                    descriptor.as_ref(),
                    requested_feature,
                )
            })
            .await
            .map_err(|error| StorageError::BlockingTaskFailed {
                operation: "checking RimWorld release compatibility",
                message: error.to_string(),
            })??;
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
