use super::*;

const SCUM_ECONOMY_OVERRIDE_FILE: &str = "EconomyOverride.json";
const SCUM_RAID_TIMES_FILE: &str = "RaidTimes.json";
const SCUM_NOTIFICATIONS_FILE: &str = "Notifications.json";

pub(super) fn materialize_scum_support_files(
    context: &ModuleSupportMaterializationContext<'_>,

    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    if !context.install_root.exists() {
        return Ok(());
    }

    let target_root = context
        .install_root
        .join("SCUM")
        .join("Saved")
        .join("Config")
        .join("WindowsServer");
    let server_settings_source = context.config_dir.join(SCUM_SERVER_SETTINGS_FILE);
    let server_settings_destination = target_root.join(SCUM_SERVER_SETTINGS_FILE);
    let economy_source = context.config_dir.join(SCUM_ECONOMY_OVERRIDE_FILE);
    let economy_destination = target_root.join(SCUM_ECONOMY_OVERRIDE_FILE);
    let raid_source = context.config_dir.join(SCUM_RAID_TIMES_FILE);
    let raid_destination = target_root.join(SCUM_RAID_TIMES_FILE);
    let notifications_source = context.config_dir.join(SCUM_NOTIFICATIONS_FILE);
    let notifications_destination = target_root.join(SCUM_NOTIFICATIONS_FILE);
    let admin_source = context.config_dir.join(SCUM_ADMIN_USERS_FILE);
    let admin_destination = target_root.join(SCUM_ADMIN_USERS_FILE);

    merge_rendered_config_files(
        &[
            ManagedConfigFile::Ini {
                source_path: &server_settings_source,
                destination_path: &server_settings_destination,
                removed_sections: &[],
            },
            ManagedConfigFile::JsonObject {
                source_path: &economy_source,
                destination_path: &economy_destination,
            },
            ManagedConfigFile::JsonObject {
                source_path: &raid_source,
                destination_path: &raid_destination,
            },
            ManagedConfigFile::JsonObject {
                source_path: &notifications_source,
                destination_path: &notifications_destination,
            },
            ManagedConfigFile::Text {
                source_path: &admin_source,
                destination_path: &admin_destination,
            },
        ],
        "scum",
        files,
    )?;
    ue4ss_player_query::materialize(
        ue4ss_player_query::Game::Scum,
        context.install_root,
        context.instance_running,
        files,
    )
}
