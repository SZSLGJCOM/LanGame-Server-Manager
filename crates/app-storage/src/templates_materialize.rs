use super::*;
#[path = "templates_materialize/ark_ini.rs"]
mod ark_ini;
pub(super) use ark_ini::write_rendered as write_rendered_preserving_ark_ini;
#[cfg(test)]
#[path = "templates_materialize/ark_ini_tests.rs"]
mod ark_ini_tests;
#[path = "templates_materialize/configuration.rs"]
mod configuration;
#[path = "templates_materialize/prepared_workshop.rs"]
mod prepared_workshop;
pub(crate) use configuration::{
    write_pending_instance_configuration, write_pending_instance_configuration_in_worker,
};
pub(crate) use prepared_workshop::{
    PreparedWorkshopConfiguration, prepare_workshop_configuration_in_worker,
};

#[path = "templates_materialize/dontstarve.rs"]
mod dontstarve;
#[path = "templates_materialize/managed_config_merge.rs"]
mod managed_config_merge;
#[path = "templates_materialize/package_staging.rs"]
pub(super) mod package_staging;
#[path = "templates_materialize/projectzomboid_seed.rs"]
mod projectzomboid_seed;
pub(crate) use managed_config_merge::ManagedConfigMergePlan;
pub(crate) use managed_config_merge::ManagedConfigMutation;
use managed_config_merge::{
    ManagedConfigFile, ManagedIniFile, merge_rendered_config_files, merge_rendered_ini_file,
    merge_rendered_ini_files, merge_rendered_json_object_file,
};
#[cfg(test)]
use managed_config_merge::{
    apply_managed_config_plans_with_writer, materialization_error, write_managed_config_plan,
};
#[cfg(test)]
#[path = "templates_materialize/managed_config_merge_tests.rs"]
mod managed_config_merge_tests;
#[cfg(test)]
pub(crate) use dontstarve::fail_next_dst_setup_write_for_test;
use dontstarve::sync_dst_mod_setup;
#[path = "templates_materialize/ue4ss_player_query.rs"]
mod ue4ss_player_query;
#[path = "templates_materialize/windrose.rs"]
mod windrose;
pub(super) use windrose::apply_pending_world_update;
use windrose::materialize_windrose_support_files;
#[path = "templates_materialize/necesse.rs"]
mod necesse;
#[path = "templates_materialize/palworld.rs"]
mod palworld;
#[cfg(test)]
#[path = "templates_materialize/palworld_tests.rs"]
mod palworld_tests;
#[path = "templates_materialize/satisfactory.rs"]
mod satisfactory;
#[path = "templates_materialize/unturned.rs"]
mod unturned;
use satisfactory::materialize_satisfactory_support_files;
#[path = "templates_materialize/scum.rs"]
mod scum;
use scum::materialize_scum_support_files;
#[path = "templates_materialize/unreal_large.rs"]
mod unreal_large;
#[cfg(test)]
use unreal_large::materialize_unreal_large_support_files;
#[cfg(test)]
#[path = "templates_materialize/unreal_large_tests.rs"]
mod unreal_large_tests;
#[path = "templates_materialize/workshop_packages.rs"]
mod workshop_packages;
use workshop_packages::{materialize_barotrauma_workshop_mods, materialize_conan_modlist};
#[cfg(test)]
#[path = "templates_materialize/workshop_package_tests.rs"]
mod workshop_package_tests;

fn materialize_module_support_files_pending(
    context: &ModuleSupportMaterializationContext<'_>,

    files: &mut ManagedConfigMutation,
    prepared: Option<PreparedWorkshopConfiguration>,
) -> Result<(), StorageError> {
    match context.module_id {
        "abioticfactor" | "conanexiles" | "humanitz" | "soulmask" => {
            unreal_large::materialize_with_packages(context, files, prepared)?
        }
        "arksurvivalascended" => materialize_ark_ascended_support_files(context, files)?,
        "arksurvivalevolved" => materialize_ark_evolved_support_files(context, files)?,
        "astroneer" => materialize_astroneer_support_files(context, files)?,
        "barotrauma" => match prepared {
            Some(prepared) => prepared.apply(context, files)?,
            None => materialize_barotrauma_workshop_mods(context, files)?,
        },
        "corekeeper" => materialize_corekeeper_support_files(context, files)?,
        "dontstarve" => sync_dst_mod_setup(context, files)?,
        "enshrouded" => materialize_enshrouded_support_files(context, files)?,
        "minecraft" => materialize_minecraft_support_files(context, files)?,
        "necesse" => materialize_necesse_support_dirs(context, files)?,
        "nightingale" => materialize_nightingale_support_files(context, files)?,
        "palworld" => materialize_palworld_support_files(context, files)?,
        "projectzomboid" => materialize_projectzomboid_support_files(context, files)?,
        "returntomoria" => materialize_returntomoria_support_files(context, files)?,
        "rimworld" => materialize_rimworld_support_files(context, files)?,
        "romestead" => materialize_romestead_support_files(context, files)?,
        "runescapedragonwilds" => materialize_runescapedragonwilds_support_files(context, files)?,
        "rust" => materialize_rust_support_files(context, files)?,
        "satisfactory" => materialize_satisfactory_support_files(context, files)?,
        "sevendaystodie" => materialize_sevendaystodie_support_files(context, files)?,
        "terraria" => materialize_terraria_support_files(context, files)?,
        "unturned" => materialize_unturned_support_files(context, files)?,
        "valheim" => materialize_valheim_support_files(context, files)?,
        "vrising" => materialize_vrising_support_files(context, files)?,
        "windrose" => materialize_windrose_support_files(context, files)?,
        "squad" => materialize_squad_support_files(context, files)?,
        "scum" => materialize_scum_support_files(context, files)?,
        "sonsoftheforest" => materialize_sonsoftheforest_support_files(context, files)?,
        _ => {}
    }

    Ok(())
}

#[cfg(test)]
pub(crate) fn materialize_module_support_files(
    context: &ModuleSupportMaterializationContext<'_>,
) -> Result<(), StorageError> {
    let mut files = ManagedConfigMutation::new(context.module_id);
    match materialize_module_support_files_pending(context, &mut files, None) {
        Ok(()) => {
            files.commit();
            Ok(())
        }
        Err(error) => Err(files.rollback_after(error)),
    }
}

fn materialize_ark_evolved_support_files(
    context: &ModuleSupportMaterializationContext<'_>,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    if !context.install_root.exists() {
        return Ok(());
    }

    let saved_root = context.install_root.join("ShooterGame").join("Saved");
    let binary_root = context
        .install_root
        .join("ShooterGame")
        .join("Binaries")
        .join("Win64");

    ark_ini::materialize(context, files)?;
    copy_required_module_support_file(
        &context.config_dir.join(ARK_EVOLVED_ADMIN_IDS_FILE),
        &saved_root.join(ARK_EVOLVED_ADMIN_IDS_FILE),
        "arksurvivalevolved",
        files,
    )?;
    copy_required_module_support_file(
        &context.config_dir.join(ARK_EXCLUSIVE_JOIN_FILE),
        &binary_root.join(ARK_EXCLUSIVE_JOIN_FILE),
        "arksurvivalevolved",
        files,
    )?;
    copy_required_module_support_file(
        &context.config_dir.join(ARK_PRIORITY_JOIN_FILE),
        &binary_root.join(ARK_PRIORITY_JOIN_FILE),
        "arksurvivalevolved",
        files,
    )?;

    ark_ini::prepare_cluster_directory(context)?;

    Ok(())
}

fn materialize_ark_ascended_support_files(
    context: &ModuleSupportMaterializationContext<'_>,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    if !context.install_root.exists() {
        return Ok(());
    }

    let saved_root = context.install_root.join("ShooterGame").join("Saved");
    let binary_root = context
        .install_root
        .join("ShooterGame")
        .join("Binaries")
        .join("Win64");

    ark_ini::materialize(context, files)?;
    copy_required_module_support_file(
        &context.config_dir.join(ARK_ASCENDED_ADMIN_IDS_FILE),
        &saved_root.join(ARK_ASCENDED_ADMIN_IDS_FILE),
        "arksurvivalascended",
        files,
    )?;
    copy_required_module_support_file(
        &context.config_dir.join(ARK_EXCLUSIVE_JOIN_FILE),
        &binary_root.join(ARK_EXCLUSIVE_JOIN_FILE),
        "arksurvivalascended",
        files,
    )?;
    copy_required_module_support_file(
        &context.config_dir.join(ARK_PRIORITY_JOIN_FILE),
        &binary_root.join(ARK_PRIORITY_JOIN_FILE),
        "arksurvivalascended",
        files,
    )?;

    ark_ini::prepare_cluster_directory(context)?;

    Ok(())
}

fn materialize_terraria_support_files(
    context: &ModuleSupportMaterializationContext<'_>,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let instance_root = context.config_dir.parent().unwrap_or(context.config_dir);
    let tmodloader_root = instance_root.join("tmodloader");
    let mods_root = tmodloader_root.join("Mods");
    let workshop_root = tmodloader_root
        .join("steamapps")
        .join("workshop")
        .join("content")
        .join(TERRARIA_TMODLOADER_WORKSHOP_APP_ID);

    fs::create_dir_all(&mods_root).map_err(|source| StorageError::CreatePath {
        path: mods_root.clone(),
        source,
    })?;
    fs::create_dir_all(&workshop_root).map_err(|source| StorageError::CreatePath {
        path: workshop_root,
        source,
    })?;

    let install_text = render_terraria_tmodloader_install_txt(context.settings);
    let install_path = mods_root.join("install.txt");
    files.write(&install_path, install_text.as_bytes())?;

    let enabled_text = render_terraria_tmodloader_enabled_json(context.settings)?;
    let enabled_path = mods_root.join("enabled.json");
    files.write(&enabled_path, enabled_text.as_bytes())
}

fn materialize_corekeeper_support_files(
    context: &ModuleSupportMaterializationContext<'_>,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let instance_root = instance_root_from_config_dir(context.config_dir);
    let data_root = instance_root.join("data");

    let server_config_source = context.config_dir.join(COREKEEPER_SERVER_CONFIG_FILE);
    let admins_source = context.config_dir.join(COREKEEPER_ADMINS_FILE);
    let bans_source = context.config_dir.join(COREKEEPER_BANS_FILE);
    let server_config_destination = data_root.join(COREKEEPER_SERVER_CONFIG_FILE);
    let admins_destination = data_root.join(COREKEEPER_ADMINS_FILE);
    let bans_destination = data_root.join(COREKEEPER_BANS_FILE);

    merge_rendered_config_files(
        &[
            ManagedConfigFile::JsonObject {
                source_path: &server_config_source,
                destination_path: &server_config_destination,
            },
            ManagedConfigFile::Text {
                source_path: &admins_source,
                destination_path: &admins_destination,
            },
            ManagedConfigFile::Text {
                source_path: &bans_source,
                destination_path: &bans_destination,
            },
        ],
        "corekeeper",
        files,
    )
}

fn materialize_enshrouded_support_files(
    context: &ModuleSupportMaterializationContext<'_>,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    if !context.install_root.exists() {
        return Ok(());
    }

    merge_rendered_json_object_file(
        &context.config_dir.join(ENSHROUDED_SERVER_CONFIG_FILE),
        &context.install_root.join(ENSHROUDED_SERVER_CONFIG_FILE),
        "enshrouded",
        files,
    )
}

fn materialize_minecraft_support_files(
    context: &ModuleSupportMaterializationContext<'_>,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let instance_root = instance_root_from_config_dir(context.config_dir);

    for file_name in [
        MINECRAFT_EULA_FILE,
        MINECRAFT_SERVER_PROPERTIES_FILE,
        MINECRAFT_OPS_FILE,
        MINECRAFT_WHITELIST_FILE,
        MINECRAFT_BANNED_PLAYERS_FILE,
        MINECRAFT_BANNED_IPS_FILE,
    ] {
        copy_required_module_support_file(
            &context.config_dir.join(file_name),
            &instance_root.join(file_name),
            "minecraft",
            files,
        )?;
    }

    Ok(())
}

fn materialize_sevendaystodie_support_files(
    context: &ModuleSupportMaterializationContext<'_>,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    fs::create_dir_all(context.saves_dir).map_err(|source| StorageError::CreatePath {
        path: context.saves_dir.to_path_buf(),
        source,
    })?;

    copy_required_module_support_file(
        &context.config_dir.join(SEVENDAYSTODIE_SERVER_ADMIN_FILE),
        &context.saves_dir.join(SEVENDAYSTODIE_SERVER_ADMIN_FILE),
        "sevendaystodie",
        files,
    )?;

    Ok(())
}

fn materialize_rust_support_files(
    context: &ModuleSupportMaterializationContext<'_>,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    if !context.install_root.exists() {
        return Ok(());
    }

    let target_identity_root = context
        .install_root
        .join("server")
        .join(context.instance_id);
    let target_config_root = target_identity_root.join("cfg");

    for file_name in [
        RUST_SERVER_CFG_FILE,
        RUST_USERS_CFG_FILE,
        RUST_BANS_CFG_FILE,
    ] {
        copy_required_module_support_file(
            &context.config_dir.join(file_name),
            &target_config_root.join(file_name),
            "rust",
            files,
        )?;
    }

    copy_required_module_support_file(
        &context.config_dir.join(RUST_WORLD_CONFIG_FILE),
        &target_identity_root.join(RUST_WORLD_CONFIG_FILE),
        "rust",
        files,
    )?;

    Ok(())
}

fn materialize_unturned_support_files(
    context: &ModuleSupportMaterializationContext<'_>,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    if !context.install_root.exists() {
        return Ok(());
    }

    let server_root = context
        .install_root
        .join("Servers")
        .join(context.instance_id);
    let commands_root = server_root.join("Server");

    copy_required_module_support_file(
        &context.config_dir.join(UNTURNED_COMMANDS_FILE),
        &commands_root.join(UNTURNED_COMMANDS_FILE),
        "unturned",
        files,
    )?;
    unturned::merge_gameplay_config(
        &context.config_dir.join(UNTURNED_GAMEPLAY_CONFIG_FILE),
        &server_root.join(UNTURNED_GAMEPLAY_CONFIG_FILE),
        files,
    )?;
    copy_required_module_support_file(
        &context.config_dir.join(UNTURNED_WORKSHOP_FILE),
        &server_root.join(UNTURNED_WORKSHOP_FILE),
        "unturned",
        files,
    )?;

    Ok(())
}

fn materialize_necesse_support_dirs(
    context: &ModuleSupportMaterializationContext<'_>,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let instance_root = instance_root_from_config_dir(context.config_dir);
    let data_dir = instance_root.join("data");
    let logs_dir = instance_root.join("logs");

    for path in [&data_dir, &logs_dir, context.saves_dir] {
        fs::create_dir_all(path).map_err(|source| StorageError::CreatePath {
            path: path.to_path_buf(),
            source,
        })?;
    }

    necesse::materialize_server_settings(context, files)
}

fn materialize_palworld_support_files(
    context: &ModuleSupportMaterializationContext<'_>,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let pal_root = context.install_root.join("Pal");
    if !pal_root.exists() {
        return Ok(());
    }

    let target_config_root = pal_root.join("Saved").join("Config").join("WindowsServer");
    let world_settings = palworld::plan_world_settings(
        &context.config_dir.join(PALWORLD_WORLD_SETTINGS_FILE),
        &target_config_root.join(PALWORLD_WORLD_SETTINGS_FILE),
    )?;
    fs::create_dir_all(&target_config_root).map_err(|source| StorageError::CreatePath {
        path: target_config_root.clone(),
        source,
    })?;

    copy_required_module_support_file(
        &context.config_dir.join(PALWORLD_GAME_USER_SETTINGS_FILE),
        &target_config_root.join(PALWORLD_GAME_USER_SETTINGS_FILE),
        "palworld",
        files,
    )?;
    files.apply(vec![world_settings])?;

    let mod_settings_root = pal_root.join("Binaries").join("Win64").join("Mods");
    fs::create_dir_all(&mod_settings_root).map_err(|source| StorageError::CreatePath {
        path: mod_settings_root.clone(),
        source,
    })?;
    let mod_settings_path = mod_settings_root.join("PalModSettings.ini");
    files.write(
        &mod_settings_path,
        render_palworld_mod_settings_ini(context.settings).as_bytes(),
    )?;

    Ok(())
}

fn materialize_valheim_support_files(
    context: &ModuleSupportMaterializationContext<'_>,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    fs::create_dir_all(context.saves_dir).map_err(|source| StorageError::CreatePath {
        path: context.saves_dir.to_path_buf(),
        source,
    })?;

    copy_required_module_support_file(
        &context.config_dir.join(VALHEIM_ADMIN_LIST_FILE),
        &context.saves_dir.join(VALHEIM_ADMIN_LIST_FILE),
        "valheim",
        files,
    )?;
    copy_required_module_support_file(
        &context.config_dir.join(VALHEIM_BANNED_LIST_FILE),
        &context.saves_dir.join(VALHEIM_BANNED_LIST_FILE),
        "valheim",
        files,
    )?;
    copy_required_module_support_file(
        &context.config_dir.join(VALHEIM_PERMITTED_LIST_FILE),
        &context.saves_dir.join(VALHEIM_PERMITTED_LIST_FILE),
        "valheim",
        files,
    )?;

    Ok(())
}

fn materialize_vrising_support_files(
    context: &ModuleSupportMaterializationContext<'_>,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let instance_root = instance_root_from_config_dir(context.config_dir);
    let settings_root = instance_root.join("Settings");

    fs::create_dir_all(context.saves_dir).map_err(|source| StorageError::CreatePath {
        path: context.saves_dir.to_path_buf(),
        source,
    })?;

    let rendered_settings_root = context.config_dir.join("Settings");
    let host_source = rendered_settings_root.join(VRISING_HOST_SETTINGS_FILE);
    let game_source = rendered_settings_root.join(VRISING_GAME_SETTINGS_FILE);
    let admins_source = rendered_settings_root.join(VRISING_ADMIN_LIST_FILE);
    let bans_source = rendered_settings_root.join(VRISING_BAN_LIST_FILE);
    let host_destination = settings_root.join(VRISING_HOST_SETTINGS_FILE);
    let game_destination = settings_root.join(VRISING_GAME_SETTINGS_FILE);
    let admins_destination = settings_root.join(VRISING_ADMIN_LIST_FILE);
    let bans_destination = settings_root.join(VRISING_BAN_LIST_FILE);

    merge_rendered_config_files(
        &[
            ManagedConfigFile::JsonObject {
                source_path: &host_source,
                destination_path: &host_destination,
            },
            ManagedConfigFile::JsonObject {
                source_path: &game_source,
                destination_path: &game_destination,
            },
        ],
        "vrising",
        files,
    )?;
    for (source, destination) in [
        (admins_source, admins_destination),
        (bans_source, bans_destination),
    ] {
        let roster = managed_config_merge::read_required_rendered_file(&source, "vrising")?;
        // The native FileUserList parser can throw on an empty existing file.
        // An absent override represents the same empty roster without that failure.
        if roster.trim().is_empty() {
            files.remove(&destination)?;
        } else {
            files.write(&destination, roster.as_bytes())?;
        }
    }
    Ok(())
}

fn materialize_squad_support_files(
    context: &ModuleSupportMaterializationContext<'_>,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    if !context.install_root.exists() {
        return Ok(());
    }

    let target_root = context.install_root.join("SquadGame").join("ServerConfig");
    for file_name in [
        SQUAD_SERVER_CFG_FILE,
        SQUAD_RCON_CFG_FILE,
        SQUAD_ADMINS_CFG_FILE,
        SQUAD_CUSTOM_OPTIONS_CFG_FILE,
        SQUAD_EXCLUDED_FACTIONS_CFG_FILE,
        SQUAD_EXCLUDED_LAYERS_CFG_FILE,
        SQUAD_EXCLUDED_LEVELS_CFG_FILE,
        SQUAD_LAYER_ROTATION_CFG_FILE,
        SQUAD_LAYER_VOTING_LOW_PLAYERS_CFG_FILE,
        SQUAD_LAYER_VOTING_NIGHT_CFG_FILE,
        SQUAD_LEVEL_ROTATION_CFG_FILE,
        SQUAD_MAP_ROTATION_CFG_FILE,
        SQUAD_SERVER_MESSAGES_CFG_FILE,
        SQUAD_REMOTE_BAN_LIST_HOSTS_CFG_FILE,
        SQUAD_VOTE_CONFIG_CFG_FILE,
        SQUAD_MOTD_CFG_FILE,
    ] {
        copy_required_module_support_file(
            &context.config_dir.join(file_name),
            &target_root.join(file_name),
            "squad",
            files,
        )?;
    }

    for (setting_key, file_name) in [
        ("layer_voting", SQUAD_LAYER_VOTING_CFG_FILE),
        ("remote_admin_hosts", SQUAD_REMOTE_ADMIN_LIST_HOSTS_CFG_FILE),
    ] {
        if lookup_materialized_setting_text(context.settings, setting_key)
            .is_some_and(|value| !value.trim().is_empty())
        {
            copy_required_module_support_file(
                &context.config_dir.join(file_name),
                &target_root.join(file_name),
                "squad",
                files,
            )?;
        }
    }

    Ok(())
}

fn materialize_nightingale_support_files(
    context: &ModuleSupportMaterializationContext<'_>,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    if !context.install_root.exists() {
        return Ok(());
    }

    let config_root = Path::new("NWX").join("Config");
    merge_rendered_ini_file(
        &context
            .config_dir
            .join(&config_root)
            .join(NIGHTINGALE_SERVER_SETTINGS_FILE),
        &context
            .install_root
            .join(&config_root)
            .join(NIGHTINGALE_SERVER_SETTINGS_FILE),
        "nightingale",
        &[],
        files,
    )
}

fn materialize_sonsoftheforest_support_files(
    context: &ModuleSupportMaterializationContext<'_>,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let user_data_root = context.saves_dir.parent().unwrap_or(context.saves_dir);

    // The packaged launcher creates this file before starting the native server.
    // Without it the server creates the file itself and stops at a restart prompt.
    if context.install_root.exists() {
        files.write(&context.install_root.join("steam_appid.txt"), b"1326470")?;
    }

    // The native server reads dedicatedserver.cfg from -userdatapath; it does
    // not support The Forest's separate -configfilepath argument.
    merge_rendered_config_files(
        &[ManagedConfigFile::JsonObject {
            source_path: &context.config_dir.join("dedicatedserver.cfg"),
            destination_path: &user_data_root.join("dedicatedserver.cfg"),
        }],
        "sonsoftheforest",
        files,
    )?;

    copy_required_module_support_file(
        &context
            .config_dir
            .join(SONS_OF_THE_FOREST_OWNER_WHITELIST_FILE),
        &user_data_root.join(SONS_OF_THE_FOREST_OWNER_WHITELIST_FILE),
        "sonsoftheforest",
        files,
    )
}

fn materialize_returntomoria_support_files(
    context: &ModuleSupportMaterializationContext<'_>,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    if !context.install_root.exists() {
        return Ok(());
    }

    merge_rendered_ini_file(
        &context.config_dir.join(RETURN_TO_MORIA_SERVER_CONFIG_FILE),
        &context
            .install_root
            .join(RETURN_TO_MORIA_SERVER_CONFIG_FILE),
        "returntomoria",
        &["Server"],
        files,
    )?;

    copy_required_module_support_file(
        &context.config_dir.join(RETURN_TO_MORIA_SERVER_RULES_FILE),
        &context.install_root.join(RETURN_TO_MORIA_SERVER_RULES_FILE),
        "returntomoria",
        files,
    )?;
    // Permissions include names written by the game. Preserve the exact text
    // projected from the native file, including its existing line endings.
    let permissions = context
        .settings
        .get("permissions_lines")
        .and_then(Value::as_str)
        .unwrap_or_default();
    files.write(
        &context
            .install_root
            .join(RETURN_TO_MORIA_SERVER_PERMISSIONS_FILE),
        permissions.as_bytes(),
    )?;

    Ok(())
}

fn materialize_astroneer_support_files(
    context: &ModuleSupportMaterializationContext<'_>,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    if !context.install_root.exists() {
        return Ok(());
    }

    let relative_config_root = Path::new("Astro")
        .join("Saved")
        .join("Config")
        .join("WindowsServer");
    let source_root = context.config_dir.join(&relative_config_root);
    let target_root = context.install_root.join(relative_config_root);

    let server_source = source_root.join(ASTRONEER_SERVER_SETTINGS_FILE);
    let server_destination = target_root.join(ASTRONEER_SERVER_SETTINGS_FILE);
    let engine_source = source_root.join(ASTRONEER_ENGINE_FILE);
    let engine_destination = target_root.join(ASTRONEER_ENGINE_FILE);
    let game_source = source_root.join(ASTRONEER_GAME_FILE);
    let game_destination = target_root.join(ASTRONEER_GAME_FILE);
    merge_rendered_ini_files(
        &[
            ManagedIniFile {
                source_path: &server_source,
                destination_path: &server_destination,
                removed_sections: &["AstroServerSettings"],
            },
            ManagedIniFile {
                source_path: &engine_source,
                destination_path: &engine_destination,
                removed_sections: &[],
            },
            ManagedIniFile {
                source_path: &game_source,
                destination_path: &game_destination,
                removed_sections: &[],
            },
        ],
        "astroneer",
        files,
    )?;

    Ok(())
}

fn materialize_romestead_support_files(
    context: &ModuleSupportMaterializationContext<'_>,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    if !context.install_root.exists() {
        return Ok(());
    }

    merge_rendered_json_object_file(
        &context.config_dir.join(ROMESTEAD_CONFIG_FILE),
        &context.install_root.join(ROMESTEAD_CONFIG_FILE),
        "romestead",
        files,
    )?;
    copy_required_module_support_file(
        &context.config_dir.join(ROMESTEAD_START_SCRIPT_FILE),
        &context.install_root.join(ROMESTEAD_START_SCRIPT_FILE),
        "romestead",
        files,
    )?;

    Ok(())
}

fn materialize_rimworld_support_files(
    context: &ModuleSupportMaterializationContext<'_>,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    if !context.install_root.exists() {
        return Ok(());
    }

    super::rimworld::materialize_configuration(context, files)?;
    let target_root = context.install_root.join("Configs");
    merge_rendered_json_object_file(
        &context.config_dir.join("ServerConfig.json"),
        &target_root.join("ServerConfig.json"),
        "rimworld",
        files,
    )?;
    Ok(())
}

fn materialize_runescapedragonwilds_support_files(
    context: &ModuleSupportMaterializationContext<'_>,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    if !context.install_root.exists() {
        return Ok(());
    }

    let config_root = Path::new("RSDragonwilds")
        .join("Saved")
        .join("Config")
        .join("WindowsServer");

    merge_rendered_ini_file(
        &context
            .config_dir
            .join(&config_root)
            .join(RUNESCAPE_DRAGONWILDS_DEDICATED_SERVER_FILE),
        &context
            .install_root
            .join(&config_root)
            .join(RUNESCAPE_DRAGONWILDS_DEDICATED_SERVER_FILE),
        "runescapedragonwilds",
        &[],
        files,
    )?;

    ue4ss_player_query::materialize(
        ue4ss_player_query::Game::Dragonwilds,
        context.install_root,
        context.instance_running,
        files,
    )
}

fn copy_required_module_support_file(
    source_path: &Path,
    destination_path: &Path,
    module_id: &str,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    if !source_path.exists() {
        return Err(StorageError::ModuleSupportMaterialization {
            module_id: String::from(module_id),
            path: source_path.to_path_buf(),
            message: String::from("rendered support file is missing"),
        });
    }

    let Some(parent) = destination_path.parent() else {
        return Err(StorageError::ModuleSupportMaterialization {
            module_id: String::from(module_id),
            path: destination_path.to_path_buf(),
            message: String::from("materialized support destination has no parent directory"),
        });
    };

    fs::create_dir_all(parent).map_err(|source| StorageError::CreatePath {
        path: parent.to_path_buf(),
        source,
    })?;

    files.copy(source_path, destination_path)?;

    Ok(())
}

fn instance_root_from_config_dir(config_dir: &Path) -> PathBuf {
    config_dir.parent().unwrap_or(config_dir).to_path_buf()
}

fn materialize_projectzomboid_support_files(
    context: &ModuleSupportMaterializationContext<'_>,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let runtime_home = context.config_dir.join("runtime-home");
    sync_projectzomboid_server_files(
        context.config_dir,
        &runtime_home,
        context.instance_id,
        files,
    )?;
    cleanup_projectzomboid_generated_files(context.config_dir, files)
}

fn sync_projectzomboid_server_files(
    config_dir: &Path,
    runtime_home: &Path,
    instance_id: &str,
    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let server_dir = runtime_home.join("Zomboid").join("Server");
    fs::create_dir_all(&server_dir).map_err(|source| StorageError::CreatePath {
        path: server_dir.clone(),
        source,
    })?;

    copy_projectzomboid_rendered_file(
        config_dir,
        &server_dir,
        PROJECT_ZOMBOID_SERVER_INI_FILE,
        &format!("{instance_id}.ini"),
        true,
        files,
    )?;
    copy_projectzomboid_rendered_file(
        config_dir,
        &server_dir,
        PROJECT_ZOMBOID_SANDBOX_VARS_FILE,
        &format!("{instance_id}_SandboxVars.lua"),
        false,
        files,
    )?;
    copy_projectzomboid_rendered_file(
        config_dir,
        &server_dir,
        PROJECT_ZOMBOID_SPAWNPOINTS_FILE,
        &format!("{instance_id}_spawnpoints.lua"),
        false,
        files,
    )?;
    copy_projectzomboid_rendered_file(
        config_dir,
        &server_dir,
        PROJECT_ZOMBOID_SPAWNREGIONS_FILE,
        &format!("{instance_id}_spawnregions.lua"),
        false,
        files,
    )?;

    Ok(())
}

fn copy_projectzomboid_rendered_file(
    config_dir: &Path,
    server_dir: &Path,
    rendered_name: &str,
    destination_name: &str,
    required: bool,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let source_path = config_dir.join(rendered_name);
    if !source_path.exists() {
        if required {
            return Err(StorageError::ModuleSupportMaterialization {
                module_id: String::from("projectzomboid"),
                path: source_path,
                message: format!("rendered {rendered_name} is missing"),
            });
        }
        return Ok(());
    }

    let destination = server_dir.join(destination_name);
    if rendered_name == PROJECT_ZOMBOID_SERVER_INI_FILE {
        projectzomboid_seed::copy_preserving_native_seed(&source_path, &destination, files)?;
    } else {
        files.copy(&source_path, &destination)?;
    }

    Ok(())
}

fn cleanup_projectzomboid_generated_files(
    config_dir: &Path,
    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    for name in [
        PROJECT_ZOMBOID_GENERATED_LAUNCH_SCRIPT,
        PROJECT_ZOMBOID_LEGACY_GENERATED_SCRIPT,
        PROJECT_ZOMBOID_LEGACY_PREPARE_SCRIPT,
    ] {
        files.remove(&config_dir.join(name))?;
    }

    Ok(())
}

fn render_dst_mod_setup(settings: &Map<String, Value>) -> String {
    let mut workshop_mod_ids = std::collections::BTreeSet::new();
    for key in [
        "shared_workshop_mod_ids",
        "master_enabled_workshop_mod_ids",
        "caves_enabled_workshop_mod_ids",
        "islands_enabled_workshop_mod_ids",
        "volcano_enabled_workshop_mod_ids",
    ] {
        workshop_mod_ids.extend(parse_workshop_id_list(settings, key));
    }
    let collection_ids = parse_workshop_id_list(settings, "shared_workshop_collection_ids")
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    let mut lines = vec![
        String::from("-- Generated by LanGame Server Manager."),
        String::from("-- Workshop downloads for this instance's shards."),
        String::new(),
    ];
    if workshop_mod_ids.is_empty() && collection_ids.is_empty() {
        lines.push(String::from("-- No Workshop mods configured."));
    } else {
        for mod_id in workshop_mod_ids {
            lines.push(format!("ServerModSetup(\"{mod_id}\")"));
        }
        for collection_id in collection_ids {
            lines.push(format!("ServerModCollectionSetup(\"{collection_id}\")"));
        }
    }
    format!("{}\n", lines.join("\n"))
}

fn render_terraria_tmodloader_install_txt(settings: &Map<String, Value>) -> String {
    let ids = parse_workshop_id_list(settings, TERRARIA_TMODLOADER_WORKSHOP_ITEM_IDS_KEY);
    if ids.is_empty() {
        String::new()
    } else {
        format!("{}\n", ids.join("\n"))
    }
}

fn render_terraria_tmodloader_enabled_json(
    settings: &Map<String, Value>,
) -> Result<String, StorageError> {
    let names = parse_terraria_tmodloader_enabled_mod_names(settings);
    serde_json::to_string_pretty(&names)
        .map(|json| format!("{json}\n"))
        .map_err(|source| StorageError::ModuleSupportMaterialization {
            module_id: String::from("terraria"),
            path: PathBuf::from("tmodloader/Mods/enabled.json"),
            message: format!("failed to render enabled.json: {source}"),
        })
}

fn parse_terraria_tmodloader_enabled_mod_names(settings: &Map<String, Value>) -> Vec<String> {
    let Some(raw) = lookup_setting_text(settings, TERRARIA_TMODLOADER_ENABLED_MOD_NAMES_KEY) else {
        return Vec::new();
    };

    let mut seen = HashSet::new();
    let mut names = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("--") {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            names.push(trimmed.to_string());
        }
    }
    names
}
