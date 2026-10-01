use super::*;

#[cfg(test)]
static FAIL_NEXT_SETUP_WRITE: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

#[cfg(test)]
pub(crate) fn fail_next_dst_setup_write_for_test(install_root: &Path) {
    *FAIL_NEXT_SETUP_WRITE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(install_root.to_path_buf());
}

pub(super) fn sync_dst_mod_setup(
    context: &ModuleSupportMaterializationContext<'_>,
    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let instance_root = context.config_dir.parent().unwrap_or(context.config_dir);
    if crate::program_runtime::instance_program_mode(instance_root)?
        != crate::InstanceProgramMode::Independent
    {
        return Err(crate::program_runtime::invalid(
            instance_root,
            "DST Workshop configuration requires an independently owned program",
        ));
    }
    let runtime = crate::program_runtime::resolve_instance_runtime_root(instance_root)?;
    if runtime != context.install_root {
        return Err(StorageError::UnsafeManagedPath {
            path: context.install_root.to_path_buf(),
            root: runtime,
        });
    }
    write_dst_mod_setup(context.install_root, context.settings, files)
}

fn write_dst_mod_setup(
    install_root: &Path,
    settings: &Map<String, Value>,
    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let mods_root = install_root.join("mods");
    fs::create_dir_all(&mods_root).map_err(|source| StorageError::CreatePath {
        path: mods_root.clone(),
        source,
    })?;

    let setup_path = mods_root.join("dedicated_server_mods_setup.lua");
    let rendered = render_dst_mod_setup(settings);
    #[cfg(test)]
    {
        let mut fail_root = FAIL_NEXT_SETUP_WRITE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if fail_root.as_deref() == Some(install_root) {
            *fail_root = None;
            return Err(StorageError::WriteConfig {
                path: setup_path,
                source: std::io::Error::other("injected DST setup write failure"),
            });
        }
    }
    files.write(&setup_path, rendered.as_bytes())
}
