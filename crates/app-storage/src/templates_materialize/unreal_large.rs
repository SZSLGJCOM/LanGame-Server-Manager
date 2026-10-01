use super::*;

#[cfg(test)]
pub(super) fn materialize_unreal_large_support_files(
    context: &ModuleSupportMaterializationContext<'_>,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    materialize_with_packages(context, files, None)
}

pub(super) fn materialize_with_packages(
    context: &ModuleSupportMaterializationContext<'_>,
    files: &mut ManagedConfigMutation,
    prepared: Option<PreparedWorkshopConfiguration>,
) -> Result<(), StorageError> {
    if !context.install_root.exists() {
        return Ok(());
    }

    match context.module_id {
        "abioticfactor" => materialize_abioticfactor(context, files),
        "conanexiles" => materialize_conan(context, files, prepared),
        "humanitz" => materialize_humanitz(context, files),
        "soulmask" => materialize_soulmask(context, files),
        _ => Ok(()),
    }
}

fn materialize_abioticfactor(
    context: &ModuleSupportMaterializationContext<'_>,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let saved_root = context.install_root.join("AbioticFactor").join("Saved");
    let sandbox_source = context.config_dir.join(ABIOTICFACTOR_SANDBOX_SETTINGS_FILE);
    let admin_source = context.config_dir.join(ABIOTICFACTOR_ADMIN_SETTINGS_FILE);
    let sandbox_destination = saved_root
        .join(ABIOTICFACTOR_SANDBOX_SETTINGS_DIR)
        .join(format!("{}-SandboxSettings.ini", context.instance_id));
    let admin_destination = saved_root
        .join(ABIOTICFACTOR_ADMIN_SETTINGS_DIR)
        .join(format!("{}-Admin.ini", context.instance_id));

    merge_rendered_config_files(
        &[
            ManagedConfigFile::Ini {
                source_path: &sandbox_source,
                destination_path: &sandbox_destination,
                removed_sections: &[],
            },
            ManagedConfigFile::Ini {
                source_path: &admin_source,
                destination_path: &admin_destination,
                removed_sections: &[],
            },
        ],
        "abioticfactor",
        files,
    )
}

fn materialize_conan(
    context: &ModuleSupportMaterializationContext<'_>,

    files: &mut ManagedConfigMutation,
    prepared: Option<PreparedWorkshopConfiguration>,
) -> Result<(), StorageError> {
    let target_root = context
        .install_root
        .join("ConanSandbox")
        .join("Saved")
        .join("Config")
        .join("WindowsServer");
    let engine_source = context.config_dir.join(CONAN_ENGINE_FILE);
    let game_source = context.config_dir.join(CONAN_GAME_FILE);
    let settings_source = context.config_dir.join(CONAN_SERVER_SETTINGS_FILE);
    let engine_destination = target_root.join(CONAN_ENGINE_FILE);
    let game_destination = target_root.join(CONAN_GAME_FILE);
    let settings_destination = target_root.join(CONAN_SERVER_SETTINGS_FILE);

    merge_rendered_config_files(
        &[
            ManagedConfigFile::Ini {
                source_path: &engine_source,
                destination_path: &engine_destination,
                removed_sections: &[],
            },
            ManagedConfigFile::Ini {
                source_path: &game_source,
                destination_path: &game_destination,
                removed_sections: &[],
            },
            ManagedConfigFile::Ini {
                source_path: &settings_source,
                destination_path: &settings_destination,
                removed_sections: &[],
            },
        ],
        "conanexiles",
        files,
    )?;

    // Workshop deployment belongs to Mods. Configuration's three native INIs
    // and modlist share the configuration transaction; package deployment is separate.
    match prepared {
        Some(prepared) => prepared.apply(context, files),
        None => materialize_conan_modlist(context, files),
    }
}

fn materialize_humanitz(
    context: &ModuleSupportMaterializationContext<'_>,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let target_root = context.install_root.join("HumanitZServer");
    let settings_source = context.config_dir.join(HUMANITZ_GAME_SERVER_SETTINGS_FILE);
    let welcome_source = context.config_dir.join(HUMANITZ_WELCOME_MESSAGE_FILE);
    let admin_source = context.config_dir.join(HUMANITZ_ADMIN_LIST_FILE);
    let reserved_source = context.config_dir.join(HUMANITZ_RESERVED_SLOTS_FILE);
    let banned_source = context.config_dir.join(HUMANITZ_BANNED_PLAYERS_FILE);
    let settings_destination = target_root.join(HUMANITZ_GAME_SERVER_SETTINGS_FILE);
    let welcome_destination = target_root.join(HUMANITZ_WELCOME_MESSAGE_FILE);
    let admin_destination = target_root.join(HUMANITZ_ADMIN_LIST_FILE);
    let reserved_destination = target_root.join(HUMANITZ_RESERVED_SLOTS_FILE);
    let banned_destination = target_root.join(HUMANITZ_BANNED_PLAYERS_FILE);

    merge_rendered_config_files(
        &[
            ManagedConfigFile::Ini {
                source_path: &settings_source,
                destination_path: &settings_destination,
                removed_sections: &[],
            },
            ManagedConfigFile::Text {
                source_path: &welcome_source,
                destination_path: &welcome_destination,
            },
            ManagedConfigFile::Text {
                source_path: &admin_source,
                destination_path: &admin_destination,
            },
            ManagedConfigFile::Text {
                source_path: &reserved_source,
                destination_path: &reserved_destination,
            },
            ManagedConfigFile::Text {
                source_path: &banned_source,
                destination_path: &banned_destination,
            },
        ],
        "humanitz",
        files,
    )
}

fn materialize_soulmask(
    context: &ModuleSupportMaterializationContext<'_>,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let source = context.config_dir.join(SOULMASK_GAME_XISHU_FILE);
    let destination = context
        .install_root
        .join("WS")
        .join("Saved")
        .join("GameplaySettings")
        .join(SOULMASK_GAME_XISHU_FILE);
    merge_rendered_config_files(
        &[ManagedConfigFile::JsonObject {
            source_path: &source,
            destination_path: &destination,
        }],
        "soulmask",
        files,
    )
}
