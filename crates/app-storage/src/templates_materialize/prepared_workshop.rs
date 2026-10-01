use super::*;

/// Package payloads have their own durable publication/ownership records. Prepare
/// them while holding the instance lease, before reserving SQLite's writer. Only
/// their small activation file belongs to the configuration/database transaction.
pub(crate) struct PreparedWorkshopConfiguration {
    module_id: String,
    install_root: PathBuf,
    config_dir: PathBuf,
    ids: Vec<String>,
    plan: Option<ManagedConfigMergePlan>,
}

impl PreparedWorkshopConfiguration {
    pub(super) fn apply(
        self,
        context: &ModuleSupportMaterializationContext<'_>,
        files: &mut ManagedConfigMutation,
    ) -> Result<(), StorageError> {
        if self.module_id != context.module_id
            || self.install_root != context.install_root
            || self.config_dir != context.config_dir
            || self.ids != parse_workshop_id_list(context.settings, "mod_workshop_ids")
        {
            return Err(StorageError::ModuleSupportMaterialization {
                module_id: context.module_id.to_owned(),
                path: context.config_dir.to_owned(),
                message: "Workshop configuration changed after package preparation".to_owned(),
            });
        }
        if let Some(plan) = self.plan {
            files.apply(vec![plan])?;
        }
        Ok(())
    }
}

pub(crate) async fn prepare_workshop_configuration_in_worker(
    context: &ModuleSupportMaterializationContext<'_>,
    lease: &crate::instance_settings_lock::InstanceSettingsLock,
) -> Result<Option<PreparedWorkshopConfiguration>, StorageError> {
    if context.instance_running || !matches!(context.module_id, "barotrauma" | "conanexiles") {
        return Ok(None);
    }
    let storage_paths = context.storage_paths.clone();
    let module_id = context.module_id.to_owned();
    let install_root = context.install_root.to_owned();
    let shared_install_root = context.shared_install_root.to_owned();
    let config_dir = context.config_dir.to_owned();
    let saves_dir = context.saves_dir.to_owned();
    let instance_id = context.instance_id.to_owned();
    let settings = context.settings.clone();
    lease
        .spawn_blocking(move || {
            let context = ModuleSupportMaterializationContext {
                storage_paths: &storage_paths,
                module_id: &module_id,
                install_root: &install_root,
                shared_install_root: &shared_install_root,
                config_dir: &config_dir,
                saves_dir: &saves_dir,
                instance_id: &instance_id,
                instance_running: false,
                settings: &settings,
            };
            let plan = if !install_root.exists() {
                None
            } else if module_id == "barotrauma" {
                workshop_packages::prepare_barotrauma_workshop_mods(&context)?
            } else {
                Some(workshop_packages::prepare_conan_modlist(&context)?)
            };
            #[cfg(test)]
            crate::instance_archive::test_gate::pause(
                &storage_paths.database_path,
                crate::instance_archive::test_gate::Point::PackagesReady,
            );
            Ok(Some(PreparedWorkshopConfiguration {
                ids: parse_workshop_id_list(&settings, "mod_workshop_ids"),
                module_id,
                install_root,
                config_dir,
                plan,
            }))
        })
        .await
        .map_err(|error| StorageError::BlockingTaskFailed {
            operation: "preparing instance Workshop packages",
            message: error.to_string(),
        })?
}
