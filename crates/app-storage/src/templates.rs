use super::*;
use crate::player_access_normalization::normalize_barotrauma_account;

#[path = "templates_materialize.rs"]
mod templates_materialize;
pub(crate) use templates_materialize::ManagedConfigMergePlan;
pub(crate) use templates_materialize::ManagedConfigMutation;
#[cfg(test)]
pub(crate) use templates_materialize::materialize_module_support_files;
pub(crate) use templates_materialize::{
    PreparedWorkshopConfiguration, prepare_workshop_configuration_in_worker,
    write_pending_instance_configuration, write_pending_instance_configuration_in_worker,
};
#[path = "templates_prestart.rs"]
mod templates_prestart;
#[cfg(test)]
pub(crate) use templates_materialize::fail_next_dst_setup_write_for_test;
pub(crate) use templates_prestart::apply_module_prestart_support;
#[path = "templates_rimworld.rs"]
mod rimworld;
pub(crate) use rimworld::preserve_retired_settings as preserve_rimworld_retired_settings;
#[path = "templates_projectzomboid_policy.rs"]
mod projectzomboid_policy;
pub(crate) use projectzomboid_policy::preserve_retired_settings as preserve_projectzomboid_retired_settings;

#[path = "templates_render_barotrauma.rs"]
mod templates_render_barotrauma;
use templates_render_barotrauma::render_barotrauma_config_player_xml;
#[path = "templates_render_common_games.rs"]
mod templates_render_common_games;
pub use templates_render_common_games::projectzomboid_mod_reorder_preserves_ids;
use templates_render_common_games::*;
#[path = "templates_render_ark.rs"]
mod templates_render_ark;
use templates_render_ark::*;
#[path = "templates_render_dst.rs"]
mod templates_render_dst;
use templates_render_dst::*;
#[path = "dst_world_settings.rs"]
pub(crate) mod dst_world_settings;
#[path = "templates_render_dst_inventory.rs"]
mod templates_render_dst_inventory;
#[path = "templates_render_palworld.rs"]
mod templates_render_palworld;
use templates_render_palworld::*;
#[path = "templates_render_scum.rs"]
mod templates_render_scum;
use templates_render_scum::*;
#[cfg(test)]
#[path = "templates_render_scum_tests.rs"]
mod templates_render_scum_tests;
#[path = "templates_render_soulmask.rs"]
mod templates_render_soulmask;
use templates_render_soulmask::*;
#[cfg(test)]
#[path = "templates_render_soulmask_tests.rs"]
mod templates_render_soulmask_tests;
#[path = "templates_render_vrising.rs"]
mod templates_render_vrising;
#[path = "templates_render_vrising_inventory.rs"]
mod templates_render_vrising_inventory;
use templates_render_vrising::*;
use templates_render_vrising_inventory::*;
#[path = "templates_render_unturned.rs"]
mod templates_render_unturned;
pub(crate) use templates_render_unturned::validate_unturned_native_settings;

#[derive(Clone, Copy, Default)]
pub(crate) struct SchemaDefaultContext<'a> {
    pub instance_id: Option<&'a str>,
    pub instance_name: Option<&'a str>,
}

#[derive(Clone, Copy)]
pub(crate) struct ModuleSupportMaterializationContext<'a> {
    pub storage_paths: &'a StoragePaths,
    pub module_id: &'a str,
    pub install_root: &'a Path,
    pub shared_install_root: &'a Path,
    pub config_dir: &'a Path,
    pub saves_dir: &'a Path,
    pub instance_id: &'a str,
    pub instance_running: bool,
    pub settings: &'a Map<String, Value>,
}

pub(crate) struct ModuleTemplateRenderInput<'a> {
    pub config_dir: &'a Path,
    pub install_root: &'a Path,
    pub saves_dir: &'a Path,
    pub instance_id: &'a str,
    pub instance_name: &'a str,
    pub module_id: &'a str,
    pub bind_ip: &'a str,
    pub autostart: bool,
    pub settings: &'a Map<String, Value>,
    pub ports: &'a [PortBinding],
}

pub(crate) struct InstanceConfigInput<'a> {
    pub instance_id: &'a str,
    pub instance_name: &'a str,
    pub module_id: &'a str,
    pub bind_ip: &'a str,
    pub autostart: bool,
    pub settings: Map<String, Value>,
    pub ports: &'a [PortBinding],
}

struct TemplateRenderContext<'a> {
    instance_root: &'a Path,
    config_dir: &'a Path,
    install_root: &'a Path,
    data_dir: &'a Path,
    logs_dir: &'a Path,
    saves_dir: &'a Path,
    instance_id: &'a str,
    instance_name: &'a str,
    module_id: &'a str,
    bind_ip: &'a str,
    autostart: bool,
    schema_defaults: &'a Map<String, Value>,
    settings: &'a Map<String, Value>,
    ports: &'a [PortBinding],
}

#[cfg(test)]
pub(crate) fn render_module_templates(
    templates_root: &Path,
    input: &ModuleTemplateRenderInput<'_>,
) -> Result<(), StorageError> {
    let mut files = ManagedConfigMutation::new(input.module_id);
    let result = (|| {
        render_module_templates_with_writer(templates_root, input, &mut |path, rendered| {
            templates_materialize::write_rendered_preserving_ark_ini(
                input, path, rendered, &mut files,
            )
        })?;
        if input.module_id == "soulmask" {
            combine_soulmask_profile_templates_pending(input.config_dir, &mut files)?;
        }
        Ok(())
    })();
    match result {
        Ok(()) => {
            files.commit();
            Ok(())
        }
        Err(error) => Err(files.rollback_after(error)),
    }
}

fn render_module_templates_with_writer(
    templates_root: &Path,
    input: &ModuleTemplateRenderInput<'_>,
    writer: &mut impl FnMut(&Path, String) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    if input.module_id == "humanitz" {
        crate::player_access_normalization::validate_humanitz_rosters(input.settings)?;
    }
    if input.module_id == "enshrouded" {
        crate::settings_validation::validate_enshrouded_settings(input.settings)?;
    }
    if !templates_root.exists() {
        return Ok(());
    }

    let instance_id = input.instance_id;
    let instance_name = input.instance_name;
    let schema_defaults = load_template_schema_defaults(
        templates_root,
        SchemaDefaultContext {
            instance_id: Some(instance_id),
            instance_name: Some(instance_name),
        },
    )?;

    let instance_root = input
        .config_dir
        .parent()
        .unwrap_or(input.config_dir)
        .to_path_buf();
    let data_dir = instance_root.join("data");
    let logs_dir = instance_root.join("logs");
    let context = TemplateRenderContext {
        instance_root: &instance_root,
        config_dir: input.config_dir,
        install_root: input.install_root,
        data_dir: &data_dir,
        logs_dir: &logs_dir,
        saves_dir: input.saves_dir,
        instance_id,
        instance_name,
        module_id: input.module_id,
        bind_ip: input.bind_ip,
        autostart: input.autostart,
        schema_defaults: &schema_defaults,
        settings: input.settings,
        ports: input.ports,
    };

    render_module_template_dir(templates_root, templates_root, &context, writer)?;
    Ok(())
}

const DEFAULT_DST_MODOVERRIDES_LUA: &str = "return {\n}\n";
pub(crate) const DEFAULT_DST_MASTER_WORLDGENOVERRIDE_LUA: &str = "return {\n  override_enabled = true,\n  settings_preset = \"SURVIVAL_TOGETHER\",\n  worldgen_preset = \"SURVIVAL_TOGETHER\",\n  overrides = {\n    world_size = \"default\",\n  }\n}\n";
const DEFAULT_DST_CAVES_WORLDGENOVERRIDE_LUA: &str = "return {\n  override_enabled = true,\n  settings_preset = \"DST_CAVE\",\n  worldgen_preset = \"DST_CAVE\",\n  overrides = {\n    world_size = \"default\",\n  }\n}\n";
const PALWORLD_GAME_USER_SETTINGS_FILE: &str = "GameUserSettings.ini";
const PALWORLD_WORLD_SETTINGS_FILE: &str = "PalWorldSettings.ini";
const ABIOTICFACTOR_SANDBOX_SETTINGS_FILE: &str = "SandboxSettings.ini";
const ABIOTICFACTOR_ADMIN_SETTINGS_FILE: &str = "Admin.ini";
const ABIOTICFACTOR_SANDBOX_SETTINGS_DIR: &str = "Config/WindowsServer/LanGame";
const ABIOTICFACTOR_ADMIN_SETTINGS_DIR: &str = "SaveGames/Server/LanGame";
const ARK_GAME_USER_SETTINGS_FILE: &str = "GameUserSettings.ini";
const ARK_GAME_INI_FILE: &str = "Game.ini";
const ARK_EVOLVED_ADMIN_IDS_FILE: &str = "AllowedCheaterSteamIDs.txt";
const ARK_ASCENDED_ADMIN_IDS_FILE: &str = "AllowedCheaterAccountIDs.txt";
const ARK_EXCLUSIVE_JOIN_FILE: &str = "PlayersExclusiveJoinList.txt";
const ARK_PRIORITY_JOIN_FILE: &str = "PlayersJoinNoCheckList.txt";
const BAROTRAUMA_CONFIG_PLAYER_FILE: &str = "config_player.xml";
const BAROTRAUMA_LOCAL_MODS_DIR: &str = "LocalMods";
const BAROTRAUMA_WORKSHOP_APP_ID: &str = "602960";
const TERRARIA_TMODLOADER_WORKSHOP_APP_ID: &str = "1281930";
const TERRARIA_TMODLOADER_WORKSHOP_ITEM_IDS_KEY: &str = "tmodloader_workshop_item_ids";
const TERRARIA_TMODLOADER_ENABLED_MOD_NAMES_KEY: &str = "tmodloader_enabled_mod_names";
const CONAN_ENGINE_FILE: &str = "Engine.ini";
const CONAN_GAME_FILE: &str = "Game.ini";
const CONAN_SERVER_SETTINGS_FILE: &str = "ServerSettings.ini";
const CONAN_MODLIST_FILE: &str = "modlist.txt";
const CONAN_WORKSHOP_APP_ID: &str = "440900";
const COREKEEPER_SERVER_CONFIG_FILE: &str = "ServerConfig.json";
const COREKEEPER_ADMINS_FILE: &str = "Admins.json";
const COREKEEPER_BANS_FILE: &str = "PlayerBans.json";
const ENSHROUDED_SERVER_CONFIG_FILE: &str = "enshrouded_server.json";
const HUMANITZ_GAME_SERVER_SETTINGS_FILE: &str = "GameServerSettings.ini";
const HUMANITZ_WELCOME_MESSAGE_FILE: &str = "WelcomeMessage.txt";
const HUMANITZ_ADMIN_LIST_FILE: &str = "AdminList.txt";
const HUMANITZ_RESERVED_SLOTS_FILE: &str = "F_ReservedSlots.txt";
const HUMANITZ_BANNED_PLAYERS_FILE: &str = "F_BannedPlayers.txt";
const MINECRAFT_EULA_FILE: &str = "eula.txt";
const MINECRAFT_SERVER_PROPERTIES_FILE: &str = "server.properties";
const MINECRAFT_OPS_FILE: &str = "ops.json";
const MINECRAFT_WHITELIST_FILE: &str = "whitelist.json";
const MINECRAFT_BANNED_PLAYERS_FILE: &str = "banned-players.json";
const MINECRAFT_BANNED_IPS_FILE: &str = "banned-ips.json";
const RUST_SERVER_CFG_FILE: &str = "server.cfg";
const RUST_USERS_CFG_FILE: &str = "users.cfg";
const RUST_BANS_CFG_FILE: &str = "bans.cfg";
const RUST_WORLD_CONFIG_FILE: &str = "world-config.json";
const SATISFACTORY_ENGINE_INI_FILE: &str = "Engine.ini";
const SATISFACTORY_GAME_INI_FILE: &str = "Game.ini";
const SEVENDAYSTODIE_SERVER_ADMIN_FILE: &str = "serveradmin.xml";
const UNTURNED_COMMANDS_FILE: &str = "Commands.dat";
const UNTURNED_GAMEPLAY_CONFIG_FILE: &str = "Config.txt";
const UNTURNED_WORKSHOP_FILE: &str = "WorkshopDownloadConfig.json";
const VALHEIM_ADMIN_LIST_FILE: &str = "adminlist.txt";
const VALHEIM_BANNED_LIST_FILE: &str = "bannedlist.txt";
const VALHEIM_PERMITTED_LIST_FILE: &str = "permittedlist.txt";
const PROJECT_ZOMBOID_SERVER_INI_FILE: &str = "server.ini";
const PROJECT_ZOMBOID_SANDBOX_VARS_FILE: &str = "SandboxVars.lua";
const PROJECT_ZOMBOID_SPAWNPOINTS_FILE: &str = "spawnpoints.lua";
const PROJECT_ZOMBOID_SPAWNREGIONS_FILE: &str = "spawnregions.lua";
const PROJECT_ZOMBOID_GENERATED_LAUNCH_SCRIPT: &str = "launch-projectzomboid.bat";
const PROJECT_ZOMBOID_LEGACY_GENERATED_SCRIPT: &str = "StartServer64.lgs.bat";
const PROJECT_ZOMBOID_LEGACY_PREPARE_SCRIPT: &str = "prepare-projectzomboid.ps1";
const VRISING_HOST_SETTINGS_FILE: &str = "ServerHostSettings.json";
const VRISING_GAME_SETTINGS_FILE: &str = "ServerGameSettings.json";
const VRISING_ADMIN_LIST_FILE: &str = "adminlist.txt";
const VRISING_BAN_LIST_FILE: &str = "banlist.txt";
const WINDROSE_SERVER_DESCRIPTION_FILE: &str = "ServerDescription.json";
const SQUAD_SERVER_CFG_FILE: &str = "Server.cfg";
const SQUAD_RCON_CFG_FILE: &str = "Rcon.cfg";
const SQUAD_ADMINS_CFG_FILE: &str = "Admins.cfg";
const SQUAD_CUSTOM_OPTIONS_CFG_FILE: &str = "CustomOptions.cfg";
const SQUAD_EXCLUDED_FACTIONS_CFG_FILE: &str = "ExcludedFactions.cfg";
const SQUAD_EXCLUDED_LAYERS_CFG_FILE: &str = "ExcludedLayers.cfg";
const SQUAD_EXCLUDED_LEVELS_CFG_FILE: &str = "ExcludedLevels.cfg";
const SQUAD_LAYER_ROTATION_CFG_FILE: &str = "LayerRotation.cfg";
const SQUAD_LAYER_VOTING_CFG_FILE: &str = "LayerVoting.cfg";
const SQUAD_LAYER_VOTING_LOW_PLAYERS_CFG_FILE: &str = "LayerVotingLowPlayers.cfg";
const SQUAD_LAYER_VOTING_NIGHT_CFG_FILE: &str = "LayerVotingNight.cfg";
const SQUAD_LEVEL_ROTATION_CFG_FILE: &str = "LevelRotation.cfg";
const SQUAD_MAP_ROTATION_CFG_FILE: &str = "MapRotation.cfg";
const SQUAD_SERVER_MESSAGES_CFG_FILE: &str = "ServerMessages.cfg";
const SQUAD_REMOTE_BAN_LIST_HOSTS_CFG_FILE: &str = "RemoteBanListHosts.cfg";
const SQUAD_REMOTE_ADMIN_LIST_HOSTS_CFG_FILE: &str = "RemoteAdminListHosts.cfg";
const SQUAD_VOTE_CONFIG_CFG_FILE: &str = "VoteConfig.cfg";
const SQUAD_MOTD_CFG_FILE: &str = "MOTD.cfg";
const SCUM_SERVER_SETTINGS_FILE: &str = "ServerSettings.ini";
const SCUM_ADMIN_USERS_FILE: &str = "ServerSettingsAdminUsers.ini";
const NIGHTINGALE_SERVER_SETTINGS_FILE: &str = "ServerSettings.ini";
const SOULMASK_GAME_XISHU_FILE: &str = "GameXishu.json";
const SONS_OF_THE_FOREST_OWNER_WHITELIST_FILE: &str = "ownerswhitelist.txt";
const RETURN_TO_MORIA_SERVER_CONFIG_FILE: &str = "MoriaServerConfig.ini";
const RETURN_TO_MORIA_SERVER_RULES_FILE: &str = "MoriaServerRules.txt";
const RETURN_TO_MORIA_SERVER_PERMISSIONS_FILE: &str = "MoriaServerPermissions.txt";
const ASTRONEER_SERVER_SETTINGS_FILE: &str = "AstroServerSettings.ini";
const ASTRONEER_ENGINE_FILE: &str = "Engine.ini";
const ASTRONEER_GAME_FILE: &str = "Game.ini";
const RUNESCAPE_DRAGONWILDS_DEDICATED_SERVER_FILE: &str = "DedicatedServer.ini";
const ROMESTEAD_CONFIG_FILE: &str = "config.json";
const ROMESTEAD_START_SCRIPT_FILE: &str = "start-romestead.bat";
fn parse_workshop_id_list(settings: &Map<String, Value>, key: &str) -> Vec<String> {
    let Some(raw) = lookup_setting_text(settings, key) else {
        return Vec::new();
    };

    let mut seen = HashSet::new();
    let mut ids = Vec::new();

    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.starts_with("--") {
            continue;
        }
        for entry in line.split(',') {
            if let Some(id) = normalize_workshop_id_line(entry)
                && seen.insert(id.clone())
            {
                ids.push(id);
            }
        }
    }

    ids
}

fn lookup_setting_text(settings: &Map<String, Value>, key: &str) -> Option<String> {
    settings.get(key).and_then(Value::as_str).map(String::from)
}

fn lookup_materialized_setting_text(settings: &Map<String, Value>, key: &str) -> Option<String> {
    let value = settings.get(key)?;
    match value {
        Value::Null => Some(String::new()),
        Value::Bool(boolean) => Some(boolean.to_string()),
        Value::Number(number) => Some(number.to_string()),
        Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    }
}

fn normalize_workshop_id_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("--") {
        return None;
    }

    let normalized = trimmed.strip_prefix("workshop-").unwrap_or(trimmed);
    if normalized.len() >= 6
        && normalized
            .chars()
            .all(|character| character.is_ascii_digit())
    {
        return Some(String::from(normalized));
    }

    if let Some(id_index) = trimmed.find("id=") {
        let digits = trimmed[id_index + 3..]
            .chars()
            .take_while(|character| character.is_ascii_digit())
            .collect::<String>();
        if digits.len() >= 6 {
            return Some(digits);
        }
    }

    extract_first_long_digit_group(trimmed)
}

fn extract_first_long_digit_group(text: &str) -> Option<String> {
    let mut current = String::new();

    for character in text.chars() {
        if character.is_ascii_digit() {
            current.push(character);
            continue;
        }

        if current.len() >= 6 {
            return Some(current);
        }
        current.clear();
    }

    if current.len() >= 6 {
        Some(current)
    } else {
        None
    }
}

fn ensure_normalized_trailing_newline(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    if normalized.ends_with('\n') {
        normalized
    } else {
        format!("{normalized}\n")
    }
}

fn strip_utf8_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

fn render_module_template_dir(
    templates_root: &Path,
    current_dir: &Path,
    context: &TemplateRenderContext<'_>,
    writer: &mut impl FnMut(&Path, String) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    for entry in fs::read_dir(current_dir).map_err(|source| StorageError::ReadDirectory {
        path: current_dir.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| StorageError::ReadDirectory {
            path: current_dir.to_path_buf(),
            source,
        })?;
        let entry_path = entry.path();

        if entry_path.is_dir() {
            render_module_template_dir(templates_root, &entry_path, context, writer)?;
            continue;
        }

        if entry_path.extension().and_then(|ext| ext.to_str()) != Some("hbs") {
            continue;
        }

        let relative_path =
            entry_path
                .strip_prefix(templates_root)
                .map_err(|_| StorageError::ReadDirectory {
                    path: entry_path.clone(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "template path escaped templates root",
                    ),
                })?;
        let output_relative = strip_hbs_extension(relative_path);
        let output_path = context.config_dir.join(output_relative);

        let template =
            fs::read_to_string(&entry_path).map_err(|source| StorageError::ReadConfig {
                path: entry_path.clone(),
                source,
            })?;
        let template = strip_utf8_bom(&template);
        let rendered =
            if context.module_id == "unturned" && template.trim() == "{{unturned.native_config}}" {
                templates_render_unturned::render_unturned_native_config(
                    context.settings,
                    context.schema_defaults,
                )?
            } else {
                render_template_text(template, context)?
            };

        writer(&output_path, rendered)?;
    }

    Ok(())
}

fn strip_hbs_extension(path: &Path) -> PathBuf {
    let as_text = path.to_string_lossy();
    let trimmed = as_text.strip_suffix(".hbs").unwrap_or(&as_text);
    PathBuf::from(trimmed)
}

fn render_template_text(
    template: &str,
    context: &TemplateRenderContext<'_>,
) -> Result<String, StorageError> {
    let mut rendered = String::new();
    let mut cursor = template;

    while let Some(open_index) = cursor.find("{{") {
        rendered.push_str(&cursor[..open_index]);
        let token_start = open_index + 2;

        if let Some(close_offset) = cursor[token_start..].find("}}") {
            let close_index = token_start + close_offset;
            let token = cursor[token_start..close_index].trim();
            let replacement = if token == "enshrouded.bans_json" {
                render_enshrouded_banned_accounts_json(context.settings)?
            } else {
                resolve_template_token(token, context).unwrap_or_default()
            };
            rendered.push_str(&replacement);
            cursor = &cursor[close_index + 2..];
        } else {
            rendered.push_str(&cursor[open_index..]);
            return Ok(rendered);
        }
    }

    rendered.push_str(cursor);
    Ok(rendered)
}

fn resolve_template_token(token: &str, context: &TemplateRenderContext<'_>) -> Option<String> {
    match token {
        "instance_id" | "instance.id" => Some(String::from(context.instance_id)),
        "instance_name" | "instance.name" => Some(String::from(context.instance_name)),
        "module_id" | "instance.module_id" | "module.id" => Some(String::from(context.module_id)),
        "bind_ip" | "instance.bind_ip" => Some(String::from(context.bind_ip)),
        "autostart" | "instance.autostart" => Some(context.autostart.to_string()),
        "paths.install_root" => Some(context.install_root.to_string_lossy().into_owned()),
        "paths.instance_root" => Some(context.instance_root.to_string_lossy().into_owned()),
        "paths.config_dir" => Some(context.config_dir.to_string_lossy().into_owned()),
        "paths.data_dir" => Some(context.data_dir.to_string_lossy().into_owned()),
        "paths.logs_dir" => Some(context.logs_dir.to_string_lossy().into_owned()),
        "paths.saves_dir" => Some(context.saves_dir.to_string_lossy().into_owned()),
        "runescapedragonwilds.platform_policy_line"
            if context.module_id == "runescapedragonwilds" =>
        {
            // An absent override must not replace an existing native restriction
            // with the new server's Crossplay default.
            Some(
                match context
                    .settings
                    .get("platform_policy")
                    .and_then(Value::as_str)
                {
                    Some(policy @ ("Crossplay" | "PC" | "PlayStation" | "Xbox" | "Nintendo")) => {
                        format!("PlatformPolicy={policy}")
                    }
                    _ => String::new(),
                },
            )
        }
        "runescapedragonwilds.owner_id" if context.module_id == "runescapedragonwilds" => {
            // Build 24574222 rejects an empty native OwnerId. Keep the user's
            // setting empty and express unassigned ownership only in the INI.
            Some(
                lookup_template_setting(context.settings, "owner_id")
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| String::from("LGSM_UNASSIGNED_OWNER")),
            )
        }
        _ => {
            if let Some(path) = token.strip_prefix("settings.") {
                lookup_template_setting(context.settings, path)
                    .or_else(|| lookup_template_setting(context.schema_defaults, path))
            } else if let Some(path) = token.strip_prefix("xml.settings.") {
                lookup_template_setting(context.settings, path)
                    .or_else(|| lookup_template_setting(context.schema_defaults, path))
                    .map(|value| escape_xml_attribute(&value))
            } else if let Some(path) = token.strip_prefix("json.settings.") {
                lookup_template_setting_json(context.settings, path)
                    .or_else(|| lookup_template_setting_json(context.schema_defaults, path))
            } else if let Some(path) = token.strip_prefix("json.paths.") {
                lookup_template_path_json(
                    context.instance_root,
                    context.install_root,
                    context.config_dir,
                    context.data_dir,
                    context.logs_dir,
                    context.saves_dir,
                    path,
                )
            } else if let Some(path) = token.strip_prefix("ports.") {
                lookup_template_port(context.ports, path)
            } else if let Some(path) = token.strip_prefix("astroneer.") {
                lookup_astroneer_template_token(context, path)
            } else if let Some(path) = token.strip_prefix("barotrauma.") {
                lookup_barotrauma_template_token(context.settings, path)
            } else if let Some(path) = token.strip_prefix("dst.") {
                lookup_dst_template_token_with_instance(context.settings, context.instance_id, path)
            } else if let Some(path) = token.strip_prefix("ark.") {
                lookup_ark_template_token(context.settings, path)
            } else if let Some(path) = token.strip_prefix("arkse.") {
                lookup_ark_evolved_template_token(context, path)
            } else if let Some(path) = token.strip_prefix("arksa.") {
                lookup_ark_ascended_template_token(context, path)
            } else if let Some(path) = token.strip_prefix("palworld.") {
                lookup_palworld_template_token(context.settings, context.ports, path)
            } else if let Some(path) = token.strip_prefix("enshrouded.") {
                lookup_enshrouded_template_token(context.settings, path)
            } else if let Some(path) = token.strip_prefix("abioticfactor.") {
                lookup_abioticfactor_template_token(context.settings, path)
            } else if let Some(path) = token.strip_prefix("projectzomboid.") {
                lookup_projectzomboid_template_token(context.settings, path)
            } else if let Some(path) = token.strip_prefix("terraria.") {
                lookup_terraria_template_token(context.settings, path)
            } else if let Some(path) = token.strip_prefix("sevendaystodie.") {
                lookup_sevendaystodie_template_token(context.settings, path)
            } else if let Some(path) = token.strip_prefix("squad.") {
                lookup_squad_template_token(context.settings, path)
            } else if let Some(path) = token.strip_prefix("scum.") {
                lookup_scum_template_token(context.settings, path)
            } else if let Some(path) = token.strip_prefix("windrose.") {
                lookup_windrose_template_token(context.settings, context.bind_ip, path)
            } else if let Some(path) = token.strip_prefix("corekeeper.") {
                lookup_corekeeper_template_token(context.settings, context.instance_id, path)
            } else if let Some(path) = token.strip_prefix("humanitz.") {
                lookup_humanitz_template_token(context.settings, path)
            } else if let Some(path) = token.strip_prefix("sonsoftheforest.") {
                lookup_sonsoftheforest_template_token(context.settings, path)
            } else if let Some(path) = token.strip_prefix("theforest.") {
                lookup_theforest_template_token(context.settings, context.schema_defaults, path)
            } else if let Some(path) = token.strip_prefix("minecraft.") {
                lookup_minecraft_template_token(context.settings, path)
            } else if let Some(path) = token.strip_prefix("rust.") {
                lookup_rust_template_token(context.settings, path)
            } else if let Some(path) = token.strip_prefix("romestead.") {
                lookup_romestead_template_token(context.settings, path)
            } else if let Some(path) = token.strip_prefix("satisfactory.") {
                lookup_satisfactory_template_token(context.settings, path)
            } else if let Some(path) = token.strip_prefix("unturned.") {
                lookup_unturned_template_token(context.settings, path)
            } else if let Some(path) = token.strip_prefix("valheim.") {
                lookup_valheim_template_token(context.settings, path)
            } else if let Some(path) = token.strip_prefix("vrising.") {
                lookup_vrising_template_token(context.settings, context.schema_defaults, path)
            } else {
                lookup_template_setting(context.settings, token)
            }
        }
    }
}

fn load_template_schema_defaults(
    templates_root: &Path,
    context: SchemaDefaultContext<'_>,
) -> Result<Map<String, Value>, StorageError> {
    let module_root = templates_root.parent().unwrap_or(templates_root);
    let schema_path = module_root.join("schema.json");
    if !schema_path.exists() {
        return Ok(Map::new());
    }

    let schema_json =
        fs::read_to_string(&schema_path).map_err(|source| StorageError::ReadConfig {
            path: schema_path,
            source,
        })?;

    collect_schema_defaults_from_schema_json(Some(strip_utf8_bom(&schema_json)), context)
}

pub(crate) fn collect_schema_defaults_from_schema_json(
    schema_json: Option<&str>,
    context: SchemaDefaultContext<'_>,
) -> Result<Map<String, Value>, StorageError> {
    let mut settings = Map::new();

    let Some(schema_json) = schema_json else {
        return Ok(settings);
    };

    let schema: Value = serde_json::from_str(schema_json)?;
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return Ok(settings);
    };

    for (key, property) in properties {
        let Some(property) = property.as_object() else {
            continue;
        };

        if let Some(default) = resolve_schema_default_value(property, context) {
            settings.insert(key.clone(), default);
        }
    }

    Ok(settings)
}

fn resolve_schema_default_value(
    property: &Map<String, Value>,
    context: SchemaDefaultContext<'_>,
) -> Option<Value> {
    match property
        .get("x-lsgm-default-source")
        .and_then(Value::as_str)
    {
        Some("instance_id") => context
            .instance_id
            .map(|value| Value::String(String::from(value))),
        Some("instance_name") => context
            .instance_name
            .map(|value| Value::String(String::from(value))),
        Some("generated_secret") => {
            let mut secret = uuid::Uuid::new_v4().simple().to_string();
            if let Some(length) = property
                .get("x-lsgm-generated-secret-length")
                .and_then(Value::as_u64)
                .filter(|length| *length > 0)
            {
                secret.truncate(length.min(secret.len() as u64) as usize);
            }
            if let Some(max_length) = property.get("maxLength").and_then(Value::as_u64) {
                secret.truncate(max_length.min(secret.len() as u64) as usize);
            }
            Some(Value::String(secret))
        }
        _ => property.get("default").cloned(),
    }
}

fn lookup_dst_template_token_with_instance(
    settings: &Map<String, Value>,
    instance_id: &str,
    path: &str,
) -> Option<String> {
    match path {
        "admin_list_lines" => Some(render_dst_klei_user_id_lines(settings, "admin_list")),
        "whitelist_lines" => Some(render_dst_klei_user_id_lines(settings, "whitelist")),
        "blocklist_lines" => Some(render_dst_klei_user_id_lines(settings, "blocklist")),
        "cluster_intention_line" => Some(render_dst_cluster_intention_line(settings)),
        "shard_enabled" => Some(
            (settings.get("shard_layout").and_then(Value::as_str) == Some("island_adventures")
                || settings.get("enable_caves").and_then(Value::as_bool) == Some(true))
            .to_string(),
        ),
        "master_worldgenoverride" => Some(render_dst_worldgenoverride(settings, "master")),
        "caves_worldgenoverride" => Some(render_dst_worldgenoverride(settings, "caves")),
        "islands_worldgenoverride" => Some(render_dst_worldgenoverride(settings, "islands")),
        "volcano_worldgenoverride" => Some(render_dst_worldgenoverride(settings, "volcano")),
        "master_modoverrides" => Some(render_dst_modoverrides(settings, "master")),
        "caves_modoverrides" => Some(render_dst_modoverrides(settings, "caves")),
        "islands_modoverrides" => Some(render_dst_modoverrides(settings, "islands")),
        "volcano_modoverrides" => Some(render_dst_modoverrides(settings, "volcano")),
        "caves_shard_id" => Some(
            if settings.get("shard_layout").and_then(Value::as_str) == Some("island_adventures") {
                String::from("2")
            } else {
                derive_dst_caves_shard_id(instance_id).to_string()
            },
        ),
        _ => None,
    }
}

fn lookup_ark_template_token(settings: &Map<String, Value>, path: &str) -> Option<String> {
    if let Some(key) = path.strip_prefix("steam64_lines ") {
        return Some(render_ark_steam64_lines(settings, key));
    }
    if let Some(key) = path.strip_prefix("account_id_lines ") {
        return Some(render_ark_account_id_lines(settings, key));
    }

    match path {
        "engram_entry_auto_unlock_lines" => Some(render_ark_prefixed_lines(
            settings,
            "engram_entry_auto_unlocks",
            "EngramEntryAutoUnlocks=",
        )),
        "override_named_engram_entries_lines" => Some(render_ark_prefixed_lines(
            settings,
            "override_named_engram_entries",
            "OverrideNamedEngramEntries=",
        )),
        "level_experience_ramp_overrides_lines" => Some(render_ark_prefixed_lines(
            settings,
            "level_experience_ramp_overrides",
            "LevelExperienceRampOverrides=",
        )),
        "override_player_level_engram_points_lines" => Some(render_ark_prefixed_lines(
            settings,
            "override_player_level_engram_points",
            "OverridePlayerLevelEngramPoints=",
        )),
        "npc_replacements_lines" => Some(render_ark_prefixed_lines(
            settings,
            "npc_replacements",
            "NPCReplacements=",
        )),
        "dino_spawn_weight_multipliers_lines" => Some(render_ark_prefixed_lines(
            settings,
            "dino_spawn_weight_multipliers",
            "DinoSpawnWeightMultipliers=",
        )),
        "config_add_npc_spawn_entries_container_lines" => Some(render_ark_prefixed_lines(
            settings,
            "config_add_npc_spawn_entries_container",
            "ConfigAddNPCSpawnEntriesContainer=",
        )),
        "config_subtract_npc_spawn_entries_container_lines" => Some(render_ark_prefixed_lines(
            settings,
            "config_subtract_npc_spawn_entries_container",
            "ConfigSubtractNPCSpawnEntriesContainer=",
        )),
        "config_override_npc_spawn_entries_container_lines" => Some(render_ark_prefixed_lines(
            settings,
            "config_override_npc_spawn_entries_container",
            "ConfigOverrideNPCSpawnEntriesContainer=",
        )),
        "config_override_supply_crate_items_lines" => Some(render_ark_prefixed_lines(
            settings,
            "config_override_supply_crate_items",
            "ConfigOverrideSupplyCrateItems=",
        )),
        "config_override_item_crafting_costs_lines" => Some(render_ark_prefixed_lines(
            settings,
            "config_override_item_crafting_costs",
            "ConfigOverrideItemCraftingCosts=",
        )),
        "config_override_item_max_quantity_lines" => Some(render_ark_prefixed_lines(
            settings,
            "config_override_item_max_quantity",
            "ConfigOverrideItemMaxQuantity=",
        )),
        "dino_class_damage_multipliers_lines" => Some(render_ark_prefixed_lines(
            settings,
            "dino_class_damage_multipliers",
            "DinoClassDamageMultipliers=",
        )),
        "dino_class_resistance_multipliers_lines" => Some(render_ark_prefixed_lines(
            settings,
            "dino_class_resistance_multipliers",
            "DinoClassResistanceMultipliers=",
        )),
        "tamed_dino_class_damage_multipliers_lines" => Some(render_ark_prefixed_lines(
            settings,
            "tamed_dino_class_damage_multipliers",
            "TamedDinoClassDamageMultipliers=",
        )),
        "tamed_dino_class_resistance_multipliers_lines" => Some(render_ark_prefixed_lines(
            settings,
            "tamed_dino_class_resistance_multipliers",
            "TamedDinoClassResistanceMultipliers=",
        )),
        "prevent_transfer_for_class_names_lines" => Some(render_ark_prefixed_lines(
            settings,
            "prevent_transfer_for_class_names",
            "PreventTransferForClassNames=",
        )),
        "auto_managed_mod_ids_lines" => Some(render_ark_prefixed_lines(
            settings,
            "auto_managed_mod_ids",
            "ModIDS=",
        )),
        _ => None,
    }
}

fn lookup_ark_evolved_template_token(
    context: &TemplateRenderContext<'_>,
    path: &str,
) -> Option<String> {
    let definitions = match path {
        "additional_gus_server_settings" => ARK_ASE_GUS_SERVER_SETTINGS,
        "additional_gus_session_settings" => ARK_ASE_GUS_SESSION_SETTINGS,
        "additional_gus_engine_session" => ARK_ASE_GUS_ENGINE_SESSION,
        "additional_gus_ragnarok" => ARK_ASE_GUS_RAGNAROK,
        "additional_gus_motd" => ARK_ASE_GUS_MOTD,
        "additional_game_ini" => ARK_ASE_GAME_INI,
        "multihome_ini_line" => {
            return Some(render_ark_multihome_ini_line(context.bind_ip));
        }
        "active_mods_ini_line" => {
            return Some(render_ark_active_mods_ini_line(
                context.settings,
                context.schema_defaults,
                "active_mod_ids",
            ));
        }
        "mod_installer_section" => {
            return Some(render_ark_mod_installer_section(
                context.settings,
                context.schema_defaults,
            ));
        }
        _ => return None,
    };
    Some(render_ark_native_ini_lines(
        context.settings,
        context.schema_defaults,
        definitions,
    ))
}

fn lookup_ark_ascended_template_token(
    context: &TemplateRenderContext<'_>,
    path: &str,
) -> Option<String> {
    let definitions = match path {
        "additional_gus_server_settings" => {
            let official = render_ark_native_ini_lines(
                context.settings,
                context.schema_defaults,
                ARK_ASA_GUS_SERVER_SETTINGS,
            );
            let patch = render_ark_native_ini_lines(
                context.settings,
                context.schema_defaults,
                ARK_ASA_PATCH_GUS_SERVER_SETTINGS,
            );
            return Some(format!("{official}{patch}"));
        }
        "additional_gus_session_settings" => ARK_ASA_GUS_SESSION_SETTINGS,
        "additional_gus_engine_session" => &[],
        "additional_gus_motd" => ARK_ASA_GUS_MOTD,
        "additional_game_ini" => {
            let official = render_ark_native_ini_lines(
                context.settings,
                context.schema_defaults,
                ARK_ASA_GAME_INI,
            );
            let advanced = render_ark_native_ini_lines(
                context.settings,
                context.schema_defaults,
                ARK_ASA_ADVANCED_GAME_INI,
            );
            return Some(format!("{official}{advanced}"));
        }
        "multihome_ini_line" => {
            return Some(render_ark_multihome_ini_line(context.bind_ip));
        }
        "active_mods_ini_line" => {
            return Some(render_ark_active_mods_ini_line(
                context.settings,
                context.schema_defaults,
                "mod_ids_csv",
            ));
        }
        _ => return None,
    };
    Some(render_ark_native_ini_lines(
        context.settings,
        context.schema_defaults,
        definitions,
    ))
}

fn lookup_palworld_template_token(
    settings: &Map<String, Value>,
    ports: &[PortBinding],
    path: &str,
) -> Option<String> {
    match path {
        "option_settings" => Some(render_palworld_option_settings(settings, ports)),
        _ => None,
    }
}

fn lookup_enshrouded_template_token(settings: &Map<String, Value>, path: &str) -> Option<String> {
    match path {
        "tags_json" => Some(render_enshrouded_tags_json(settings)),
        "extra_user_groups_json_entries" => {
            Some(render_enshrouded_extra_user_groups_json_entries(settings))
        }
        "hunger_to_starving_ns" => Some(render_enshrouded_minutes_as_ns(
            settings,
            "hunger_to_starving_minutes",
            10,
        )),
        "day_time_ns" => Some(render_enshrouded_minutes_as_ns(
            settings,
            "day_time_minutes",
            30,
        )),
        "night_time_ns" => Some(render_enshrouded_minutes_as_ns(
            settings,
            "night_time_minutes",
            12,
        )),
        _ => None,
    }
}

fn lookup_abioticfactor_template_token(
    settings: &Map<String, Value>,
    path: &str,
) -> Option<String> {
    if let Some(key) = path.strip_prefix("bool.") {
        return Some(lookup_ini_bool_text(settings, key, false));
    }

    let mut segments = path.split_whitespace();
    match segments.next()? {
        "moderator_lines" => Some(render_abioticfactor_moderator_lines(settings)),
        "ini_value" => {
            let setting_key = segments.next()?;
            let render_kind = match segments.next() {
                Some("bool") => AbioticFactorLineRenderKind::Bool,
                Some("scalar") | None => AbioticFactorLineRenderKind::Scalar,
                Some(_) => return None,
            };
            if segments.next().is_some() {
                return None;
            }
            settings.get(setting_key).map(|value| match render_kind {
                AbioticFactorLineRenderKind::Bool => {
                    stringify_abioticfactor_ini_bool(value).unwrap_or_default()
                }
                AbioticFactorLineRenderKind::Scalar => stringify_template_value(value),
            })
        }
        _ => None,
    }
}

#[derive(Clone, Copy)]
enum AbioticFactorLineRenderKind {
    Scalar,
    Bool,
}

fn stringify_abioticfactor_ini_bool(value: &Value) -> Option<String> {
    let boolean = template_value_as_bool(value)?;
    Some(if boolean {
        String::from("True")
    } else {
        String::from("False")
    })
}

fn template_value_as_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(boolean) => Some(*boolean),
        Value::Number(number) => number.as_i64().map(|numeric| numeric != 0),
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.eq_ignore_ascii_case("true")
                || trimmed.eq_ignore_ascii_case("1")
                || trimmed.eq_ignore_ascii_case("yes")
                || trimmed.eq_ignore_ascii_case("on")
            {
                Some(true)
            } else if trimmed.eq_ignore_ascii_case("false")
                || trimmed.eq_ignore_ascii_case("0")
                || trimmed.eq_ignore_ascii_case("no")
                || trimmed.eq_ignore_ascii_case("off")
            {
                Some(false)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn lookup_projectzomboid_template_token(
    settings: &Map<String, Value>,
    path: &str,
) -> Option<String> {
    match path {
        "welcome_message" => Some(render_projectzomboid_welcome_message(settings)),
        "seed_line" => Some(
            settings
                .get("world_seed")
                .and_then(Value::as_str)
                .filter(|seed| !seed.trim().is_empty())
                .map(|seed| format!("Seed={seed}"))
                .unwrap_or_default(),
        ),
        "map_list" => Some(render_projectzomboid_semicolon_list(settings, "map_name")),
        "workshop_items" => Some(render_projectzomboid_workshop_items(settings)),
        "mods" => Some(render_projectzomboid_semicolon_list(settings, "mods")),
        _ => None,
    }
}

fn lookup_terraria_template_token(settings: &Map<String, Value>, path: &str) -> Option<String> {
    match path {
        "banlist_lines" => Some(render_terraria_banlist_lines(settings)),
        "seed_line" => Some(render_terraria_seed_line(settings)),
        "special_seed_line" => Some(render_terraria_special_seed_line(settings)),
        "password_line" => Some(render_terraria_optional_text_line(
            settings, "password", "password",
        )),
        "secure_line" => Some(render_terraria_enabled_line(settings, "secure", "secure")),
        "upnp_line" => Some(render_terraria_enabled_line(settings, "upnp", "upnp")),
        "steam_line" => Some(render_terraria_enabled_line(settings, "steam", "steam")),
        "lobby_line" => Some(render_terraria_lobby_line(settings)),
        "disableannouncementbox_line" => Some(render_terraria_enabled_line(
            settings,
            "disableannouncementbox",
            "disableannouncementbox",
        )),
        "announcementboxrange_line" => Some(render_terraria_optional_i64_line(
            settings,
            "announcementboxrange",
            "announcementboxrange",
        )),
        "slowliquids_line" => Some(render_terraria_enabled_line(
            settings,
            "slowliquids",
            "slowliquids",
        )),
        _ => None,
    }
}

fn lookup_sevendaystodie_template_token(
    settings: &Map<String, Value>,
    path: &str,
) -> Option<String> {
    match path {
        "admin_user_lines" => Some(render_sevendaystodie_admin_user_lines(settings)),
        "admin_group_lines" => Some(render_sevendaystodie_admin_group_lines(settings)),
        "whitelist_user_lines" => Some(render_sevendaystodie_whitelist_user_lines(settings)),
        "whitelist_group_lines" => Some(render_sevendaystodie_whitelist_group_lines(settings)),
        "blacklist_lines" => Some(render_sevendaystodie_blacklist_lines(settings)),
        "permission_lines" => Some(render_sevendaystodie_permission_lines(settings)),
        _ => None,
    }
}

fn lookup_corekeeper_template_token(
    settings: &Map<String, Value>,
    instance_id: &str,
    path: &str,
) -> Option<String> {
    match path {
        "effective_game_id_json" => Some(render_corekeeper_effective_game_id_json(
            settings,
            instance_id,
        )),
        "admins_document_json" => Some(render_corekeeper_admins_document_json(settings)),
        "bans_document_json" => Some(render_corekeeper_bans_document_json(settings)),
        _ => None,
    }
}

fn lookup_astroneer_template_token(
    context: &TemplateRenderContext<'_>,
    path: &str,
) -> Option<String> {
    if path != "console_port" {
        return None;
    }
    let password = context
        .settings
        .get("console_password")
        .or_else(|| context.schema_defaults.get("console_password"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    // A blank password must disable the listener, including when it overrides a generated default.
    if password.trim().is_empty() {
        Some(String::from("0"))
    } else {
        lookup_template_port(context.ports, "console.port")
    }
}

fn lookup_barotrauma_template_token(settings: &Map<String, Value>, path: &str) -> Option<String> {
    match path {
        "client_permissions_xml" => Some(render_barotrauma_client_permissions_xml(settings)),
        _ => None,
    }
}

fn render_barotrauma_client_permissions_xml(settings: &Map<String, Value>) -> String {
    let mut seen = HashSet::new();
    let mut lines = vec![
        String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>"),
        String::from("<ClientPermissions>"),
    ];

    for entry in parse_config_lines(settings, "admin_entries") {
        let parts = split_delimited_entry(&entry, ',', 2);
        let Some(account_id) = parts
            .first()
            .and_then(|part| normalize_barotrauma_account(part))
        else {
            continue;
        };
        if !seen.insert(account_id.to_ascii_lowercase()) {
            continue;
        }

        let display_name = parts
            .get(1)
            .map(String::as_str)
            .unwrap_or(account_id.as_str())
            .trim();
        let display_name = if display_name.is_empty() {
            account_id.as_str()
        } else {
            display_name
        };
        lines.push(format!(
            "  <Client name=\"{}\" accountid=\"{}\" permissions=\"All\" />",
            escape_xml_attribute(display_name),
            escape_xml_attribute(&account_id)
        ));
    }

    lines.push(String::from("</ClientPermissions>"));
    lines.join("\n")
}

fn lookup_minecraft_template_token(settings: &Map<String, Value>, path: &str) -> Option<String> {
    match path {
        "server_ip" => Some(render_minecraft_server_ip(settings)),
        "ops_json" => Some(render_minecraft_ops_json(settings)),
        "whitelist_json" => Some(render_minecraft_named_uuid_json(
            settings,
            "whitelist_entries",
        )),
        "banned_players_json" => Some(render_minecraft_banned_players_json(settings)),
        "banned_ips_json" => Some(render_minecraft_banned_ips_json(settings)),
        "extra_properties_lines" => {
            Some(parse_config_lines(settings, "extra_properties").join("\n"))
        }
        _ => None,
    }
}

fn lookup_rust_template_token(settings: &Map<String, Value>, path: &str) -> Option<String> {
    match path {
        "owner_lines" => Some(render_rust_user_lines(settings, "owner_entries", "ownerid")),
        "moderator_lines" => Some(render_rust_user_lines(
            settings,
            "moderator_entries",
            "moderatorid",
        )),
        "skip_queue_lines" => Some(render_rust_skip_queue_lines(settings)),
        "ban_lines" => Some(render_rust_ban_lines(settings)),
        "users_cfg_extra_lines" => Some(render_non_roster_extra_lines(
            settings,
            "users_cfg_extra",
            &[
                "ownerid",
                "moderatorid",
                "skipqueueid",
                "global.skipqueueid",
            ],
        )),
        "bans_cfg_extra_lines" => Some(render_non_roster_extra_lines(
            settings,
            "bans_cfg_extra",
            &["banid"],
        )),
        "server_gamemode_line" => Some(render_rust_optional_quoted_setting_line(
            settings,
            "server_gamemode",
            "server.gamemode",
        )),
        "level_url_line" => Some(render_rust_optional_quoted_setting_line(
            settings,
            "level_url",
            "server.levelurl",
        )),
        "seed_line" => Some(render_rust_seed_line(settings)),
        "world_size_line" => Some(render_rust_world_size_line(settings)),
        "logo_image_line" => Some(render_rust_optional_quoted_setting_line(
            settings,
            "logo_image_url",
            "server.logoimage",
        )),
        "favorites_endpoint_line" => Some(render_rust_optional_quoted_setting_line(
            settings,
            "favorites_endpoint",
            "server.favoritesEndpoint",
        )),
        "wipe_timezone_line" => Some(render_rust_optional_quoted_setting_line(
            settings,
            "wipe_timezone",
            "wipetimer.wipeTimezone",
        )),
        "wipe_cron_override_line" => Some(render_rust_optional_quoted_setting_line(
            settings,
            "wipe_cron_override",
            "wipetimer.wipecronoverride",
        )),
        "wipe_unix_timestamp_override_line" => Some(render_rust_wipe_unix_override_line(settings)),
        "app_port_line" => Some(render_rust_optional_setting_line(
            settings, "app_port", "app.port",
        )),
        "app_public_ip_line" => Some(render_rust_optional_quoted_setting_line(
            settings,
            "app_public_ip",
            "app.publicip",
        )),
        "app_listen_ip_line" => Some(render_rust_optional_quoted_setting_line(
            settings,
            "app_listen_ip",
            "app.listenip",
        )),
        "bans_server_endpoint_line" => Some(render_rust_optional_quoted_setting_line(
            settings,
            "bans_server_endpoint",
            "server.bansServerEndpoint",
        )),
        "reports_server_endpoint_line" => Some(render_rust_optional_quoted_setting_line(
            settings,
            "reports_server_endpoint",
            "server.reportsServerEndpoint",
        )),
        "reports_server_endpoint_key_line" => Some(render_rust_optional_quoted_setting_line(
            settings,
            "reports_server_endpoint_key",
            "server.reportsServerEndpointKey",
        )),
        _ => None,
    }
}

fn lookup_satisfactory_template_token(settings: &Map<String, Value>, path: &str) -> Option<String> {
    match path {
        "crash_reporting_value" => Some(
            if settings
                .get("disable_crash_reporting")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                String::from("False")
            } else {
                String::from("True")
            },
        ),
        _ => None,
    }
}

fn lookup_romestead_template_token(settings: &Map<String, Value>, path: &str) -> Option<String> {
    match path {
        "world_seed_json" => {
            let seed = settings
                .get("auto_create_world_seed")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim();
            if seed.is_empty() {
                Some(String::from("null"))
            } else {
                serde_json::to_string(seed).ok()
            }
        }
        _ => None,
    }
}

fn lookup_unturned_template_token(settings: &Map<String, Value>, path: &str) -> Option<String> {
    match path {
        "password_line" => Some(render_unturned_optional_line(
            settings, "password", "Password",
        )),
        "owner_line" => Some(render_unturned_owner_line(settings)),
        "admin_lines" => Some(render_unturned_admin_lines(settings)),
        "pve_line" => Some(render_unturned_enabled_line(settings, "pve", "PVE")),
        "cheats_line" => Some(render_unturned_enabled_line(settings, "cheats", "Cheats")),
        "whitelist_line" => Some(render_unturned_enabled_line(
            settings,
            "whitelist_enabled",
            "Whitelist",
        )),
        "hide_admins_line" => Some(render_unturned_enabled_line(
            settings,
            "hide_admins",
            "Hide_Admins",
        )),
        "workshop_file_ids_json" => {
            Some(render_workshop_file_ids_json(settings, "workshop_file_ids"))
        }
        "workshop_ignore_children_file_ids_json" => Some(render_workshop_file_ids_json(
            settings,
            "workshop_ignore_children_file_ids",
        )),
        _ => None,
    }
}

fn lookup_humanitz_template_token(settings: &Map<String, Value>, path: &str) -> Option<String> {
    match path {
        "admin_list_lines" => render_humanitz_net_id_lines(settings, "admin_steam_ids"),
        "reserved_player_lines" => {
            render_humanitz_net_id_lines(settings, "reserved_player_steam_ids")
        }
        "banned_player_lines" => render_humanitz_net_id_lines(settings, "banned_player_steam_ids"),
        "settings_extra_lines" => Some(parse_config_lines(settings, "settings_extra").join("\n")),
        _ => None,
    }
}

fn render_humanitz_net_id_lines(settings: &Map<String, Value>, key: &str) -> Option<String> {
    crate::player_access_normalization::parse_humanitz_roster(settings, key)
        .ok()
        .map(|entries| entries.join("\n"))
}

fn lookup_valheim_template_token(settings: &Map<String, Value>, path: &str) -> Option<String> {
    match path {
        "admin_list_lines" => Some(render_valheim_platform_id_lines(settings, "admin_list")),
        "banned_list_lines" => Some(render_valheim_platform_id_lines(settings, "banned_list")),
        "permitted_list_lines" => {
            Some(render_valheim_platform_id_lines(settings, "permitted_list"))
        }
        _ => None,
    }
}

fn lookup_vrising_template_token(
    settings: &Map<String, Value>,
    schema_defaults: &Map<String, Value>,
    path: &str,
) -> Option<String> {
    match path {
        "optional_host_settings_members" => {
            Some(render_vrising_optional_host_settings_members(settings))
        }
        "admin_list_lines" => Some(render_vrising_steam64_lines(settings, "admin_list")),
        "ban_list_lines" => Some(render_vrising_steam64_lines(settings, "ban_list")),
        "server_game_settings_json" => Some(render_vrising_server_game_settings_json(
            settings,
            schema_defaults,
        )),
        _ => None,
    }
}

fn render_abioticfactor_moderator_lines(settings: &Map<String, Value>) -> String {
    let Some(raw) = lookup_setting_text(settings, "moderator_steam_ids") else {
        return String::new();
    };

    parse_steam64_values_from_text(&raw)
        .into_iter()
        .map(|steam_id| format!("Moderator={steam_id}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn normalize_corekeeper_identifier_list(raw: &str) -> Vec<String> {
    parse_steam64_values_from_text(raw)
}

fn normalize_corekeeper_game_id(raw: &str) -> Option<String> {
    let sanitized = raw
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>();

    if (15..=28).contains(&sanitized.len()) {
        Some(sanitized)
    } else {
        None
    }
}

fn derive_corekeeper_game_id(instance_id: &str) -> String {
    let sanitized_instance_id = instance_id
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_lowercase())
        .collect::<String>();

    let mut derived = format!("lgm{sanitized_instance_id}corekeeper");

    if derived.len() < 15 {
        derived.push_str("server");
    }

    if derived.len() < 15 {
        derived.extend(std::iter::repeat_n('0', 15 - derived.len()));
    }

    derived.truncate(28);
    derived
}

fn resolve_corekeeper_effective_game_id(
    settings: &Map<String, Value>,
    instance_id: &str,
) -> String {
    let configured = lookup_setting_text(settings, "game_id").unwrap_or_default();
    normalize_corekeeper_game_id(configured.trim())
        .unwrap_or_else(|| derive_corekeeper_game_id(instance_id))
}

fn render_corekeeper_effective_game_id_json(
    settings: &Map<String, Value>,
    instance_id: &str,
) -> String {
    serde_json::to_string(&resolve_corekeeper_effective_game_id(settings, instance_id))
        .unwrap_or_else(|_| String::from("\"lgmcorekeeperserver\""))
}

fn render_corekeeper_admins_document_json(settings: &Map<String, Value>) -> String {
    let raw = lookup_setting_text(settings, "admin_list").unwrap_or_default();
    let admins = normalize_corekeeper_identifier_list(&raw)
        .into_iter()
        .enumerate()
        .map(|(index, steam_id)| {
            json!({
                "index": index + 1,
                "privileges": 1,
                "name": "",
                "steamId": steam_id.parse::<u64>().unwrap_or(0_u64),
            })
        })
        .collect::<Vec<_>>();

    serde_json::to_string_pretty(&json!({ "adminList": admins }))
        .unwrap_or_else(|_| String::from("{\"adminList\":[]}"))
}

fn render_corekeeper_bans_document_json(settings: &Map<String, Value>) -> String {
    let raw = lookup_setting_text(settings, "ban_list").unwrap_or_default();
    let bans = normalize_corekeeper_identifier_list(&raw)
        .into_iter()
        .enumerate()
        .map(|(index, steam_id)| {
            json!({
                "index": index + 1,
                "name": "",
                "steamId": steam_id.parse::<u64>().unwrap_or(0_u64),
                "crossPlatformId": 0,
                "stringId": "",
            })
        })
        .collect::<Vec<_>>();

    let document = json!({
        "banList": bans,
    });

    serde_json::to_string_pretty(&document)
        .unwrap_or_else(|_| String::from("{\n  \"banList\": []\n}"))
}

fn escape_palworld_ini_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\"', "\\\"")
        .replace("\r\n", " ")
        .replace(['\n', '\r'], " ")
}

fn render_palworld_log_format(settings: &Map<String, Value>) -> String {
    match lookup_palworld_string_text(settings, "log_format", "text")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "json" => String::from("Json"),
        _ => String::from("Text"),
    }
}

fn render_palworld_crossplay_platforms(settings: &Map<String, Value>) -> String {
    let raw = lookup_palworld_string_text(settings, "crossplay_platforms", "(Steam,Xbox,PS5,Mac)");
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::from("(Steam,Xbox,PS5,Mac)");
    }
    if trimmed.starts_with('(') && trimmed.ends_with(')') {
        return String::from(trimmed);
    }

    let normalized = trimmed.replace("\r\n", "\n").replace('\r', "\n");
    let entries = normalized
        .split(['\n', ','])
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .collect::<Vec<_>>();

    if entries.is_empty() {
        String::from("(Steam,Xbox,PS5,Mac)")
    } else {
        format!("({})", entries.join(","))
    }
}

fn render_palworld_deny_technology_list(settings: &Map<String, Value>) -> String {
    let raw = lookup_palworld_string_text(settings, "deny_technology_list", "");
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.starts_with('(') && trimmed.ends_with(')') {
        return String::from(trimmed);
    }

    let normalized = trimmed.replace("\r\n", "\n").replace('\r', "\n");
    let entries = normalized
        .split(['\n', ','])
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let unquoted = entry.trim_matches('"');
            format!("\"{}\"", escape_palworld_ini_string(unquoted))
        })
        .collect::<Vec<_>>();

    if entries.is_empty() {
        String::new()
    } else {
        format!("({})", entries.join(","))
    }
}

fn lookup_port_number_text(ports: &[PortBinding], port_name: &str, default_value: &str) -> String {
    ports
        .iter()
        .find(|candidate| candidate.name == port_name)
        .map(|candidate| candidate.port.to_string())
        .unwrap_or_else(|| String::from(default_value))
}

fn lookup_template_setting_value<'a>(
    settings: &'a Map<String, Value>,
    path: &str,
) -> Option<&'a Value> {
    let mut segments = path.split('.');
    let first = segments.next()?;
    let mut current = settings.get(first)?;

    for segment in segments {
        current = current.get(segment)?;
    }

    Some(current)
}

fn lookup_template_setting(settings: &Map<String, Value>, path: &str) -> Option<String> {
    Some(stringify_template_value(lookup_template_setting_value(
        settings, path,
    )?))
}

fn lookup_sonsoftheforest_template_token(
    settings: &Map<String, Value>,
    path: &str,
) -> Option<String> {
    match path {
        "owner_whitelist_lines" => {
            Some(parse_steam64_lines(settings, "owner_whitelist_steam_ids").join("\n"))
        }
        _ => None,
    }
}

fn lookup_theforest_template_token(
    settings: &Map<String, Value>,
    schema_defaults: &Map<String, Value>,
    path: &str,
) -> Option<String> {
    let key = path.strip_suffix("_on_off")?;
    let enabled = lookup_template_setting_value(settings, key)
        .or_else(|| lookup_template_setting_value(schema_defaults, key))?
        .as_bool()?;
    Some(String::from(if enabled { "on" } else { "off" }))
}

fn lookup_scum_template_token(settings: &Map<String, Value>, path: &str) -> Option<String> {
    match path {
        "admin_steam_ids_lines" => Some(render_scum_admin_steam_ids_lines(settings)),
        "server_settings_ini" => render_scum_server_settings_ini(settings),
        "economy_override_json" => render_scum_economy_override(settings),
        "raid_times_json" => render_scum_raid_times(settings),
        "notifications_json" => render_scum_notifications(settings),
        _ => None,
    }
}

const SQUAD_ADMIN_GROUP_NAME: &str = "LanGameAdmin";
const SQUAD_RESERVED_GROUP_NAME: &str = "LanGameReserved";
const SQUAD_DEFAULT_ADMIN_PERMISSIONS: &[&str] = &[
    "changemap",
    "cheat",
    "private",
    "balance",
    "chat",
    "kick",
    "ban",
    "config",
    "cameraman",
    "debug",
    "pause",
    "immunity",
    "manageserver",
    "featuretest",
    "reserve",
    "teamchange",
    "forceteamchange",
    "canseeadminchat",
];

fn lookup_squad_template_token(settings: &Map<String, Value>, path: &str) -> Option<String> {
    match path {
        "admins_cfg" => Some(render_squad_admins_cfg(settings)),
        _ => None,
    }
}

fn normalize_squad_permission(value: &str) -> Option<String> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.is_empty()
        || !normalized.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        })
    {
        return None;
    }

    Some(normalized)
}

fn normalize_squad_group_name(value: &str) -> Option<String> {
    let normalized = value.trim();
    if normalized.is_empty()
        || !normalized.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        })
    {
        return None;
    }

    Some(String::from(normalized))
}

fn parse_squad_permissions(settings: &Map<String, Value>) -> Vec<String> {
    let mut seen = HashSet::new();
    let configured = parse_config_lines(settings, "admin_permissions")
        .into_iter()
        .filter_map(|entry| normalize_squad_permission(&entry))
        .filter(|entry| seen.insert(entry.clone()))
        .collect::<Vec<_>>();

    if !configured.is_empty() {
        return configured;
    }

    SQUAD_DEFAULT_ADMIN_PERMISSIONS
        .iter()
        .map(|entry| String::from(*entry))
        .collect()
}

fn render_squad_admin_assignment_lines(
    settings: &Map<String, Value>,
    key: &str,
    group_name: &str,
) -> Vec<String> {
    parse_steam64_lines(settings, key)
        .into_iter()
        .map(|steam_id| format!("Admin={steam_id}:{group_name}"))
        .collect()
}

fn normalize_squad_extra_admin_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if let Some(body) = trimmed.strip_prefix("Group=") {
        let (group_name, permission_blob) = body.split_once(':')?;
        let group_name = normalize_squad_group_name(group_name)?;
        let mut seen = HashSet::new();
        let permissions = permission_blob
            .split(',')
            .filter_map(normalize_squad_permission)
            .filter(|permission| seen.insert(permission.clone()))
            .collect::<Vec<_>>();

        if permissions.is_empty() {
            return None;
        }

        return Some(format!("Group={group_name}:{}", permissions.join(",")));
    }

    None
}

fn render_non_roster_extra_lines(
    settings: &Map<String, Value>,
    key: &str,
    managed_prefixes: &[&str],
) -> String {
    parse_config_lines(settings, key)
        .into_iter()
        .filter(|line| {
            let normalized = line.trim_start().to_ascii_lowercase();
            !managed_prefixes
                .iter()
                .any(|prefix| directive_has_prefix(&normalized, prefix))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn directive_has_prefix(line: &str, prefix: &str) -> bool {
    line.strip_prefix(prefix).is_some_and(|suffix| {
        suffix.is_empty() || suffix.chars().next().is_some_and(char::is_whitespace)
    })
}

fn parse_squad_extra_admin_lines(settings: &Map<String, Value>) -> Vec<String> {
    let mut seen = HashSet::new();
    parse_config_lines(settings, "admins_cfg")
        .into_iter()
        .filter_map(|line| normalize_squad_extra_admin_line(&line))
        .filter(|line| seen.insert(line.to_ascii_lowercase()))
        .collect()
}

fn render_squad_admins_cfg(settings: &Map<String, Value>) -> String {
    let mut lines = Vec::new();
    let permissions = parse_squad_permissions(settings);

    lines.push(String::from("// Generated by LanGame Server Manager."));
    lines.push(format!(
        "Group={SQUAD_ADMIN_GROUP_NAME}:{}",
        permissions.join(",")
    ));
    lines.push(format!("Group={SQUAD_RESERVED_GROUP_NAME}:reserve"));
    lines.extend(render_squad_admin_assignment_lines(
        settings,
        "admin_steam_ids",
        SQUAD_ADMIN_GROUP_NAME,
    ));
    lines.extend(render_squad_admin_assignment_lines(
        settings,
        "priority_join_steam_ids",
        SQUAD_RESERVED_GROUP_NAME,
    ));

    let extra_lines = parse_squad_extra_admin_lines(settings);
    if !extra_lines.is_empty() {
        lines.push(String::from(
            "// Additional Admins.cfg lines from instance settings.",
        ));
        lines.extend(extra_lines);
    }

    lines.join("\n")
}

fn render_scum_admin_steam_ids_lines(settings: &Map<String, Value>) -> String {
    parse_steam64_lines(settings, "admin_steam_ids").join("\n")
}

fn lookup_windrose_template_token(
    settings: &Map<String, Value>,
    bind_ip: &str,
    path: &str,
) -> Option<String> {
    match path {
        "is_password_protected" => {
            let password =
                lookup_materialized_setting_text(settings, "server_password").unwrap_or_default();
            Some((!password.trim().is_empty()).to_string())
        }
        "p2p_proxy_address_json" => Some(render_windrose_proxy_address_json(
            settings,
            "p2p_proxy_address",
            bind_ip,
        )),
        "direct_connection_proxy_address_json" => Some(render_windrose_proxy_address_json(
            settings,
            "direct_connection_proxy_address",
            bind_ip,
        )),
        _ => None,
    }
}

fn render_windrose_proxy_address_json(
    settings: &Map<String, Value>,
    key: &str,
    bind_ip: &str,
) -> String {
    let configured = lookup_materialized_setting_text(settings, key).unwrap_or_default();
    let resolved = if configured.trim().is_empty() {
        bind_ip.trim().to_string()
    } else {
        configured.trim().to_string()
    };

    serde_json::to_string(&resolved).unwrap_or_else(|_| String::from("\"\""))
}

fn lookup_template_setting_json(settings: &Map<String, Value>, path: &str) -> Option<String> {
    serde_json::to_string(lookup_template_setting_value(settings, path)?).ok()
}

fn lookup_template_port(ports: &[PortBinding], path: &str) -> Option<String> {
    let mut segments = path.split('.');
    let port_name = segments.next()?;
    let field = segments.next()?;
    let port = ports
        .iter()
        .find(|candidate| candidate.name == port_name)
        .or_else(|| match port_name {
            "caves" => ports.iter().find(|candidate| candidate.name == "backup"),
            "backup" => ports.iter().find(|candidate| candidate.name == "caves"),
            _ => None,
        })?;

    match field {
        "name" => Some(port.name.clone()),
        "protocol" => Some(port.protocol.clone()),
        "port" => Some(port.port.to_string()),
        _ => None,
    }
}

fn stringify_template_value(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(boolean) => boolean.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn render_instance_config(input: InstanceConfigInput<'_>) -> Result<Vec<u8>, StorageError> {
    let InstanceConfigInput {
        instance_id,
        instance_name,
        module_id,
        bind_ip,
        autostart,
        mut settings,
        ports,
    } = input;
    settings.insert(
        String::from("bind_ip"),
        Value::String(String::from(bind_ip)),
    );
    let generated_at_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);

    let document = json!({
        "instance_id": instance_id,
        "instance_name": instance_name,
        "module_id": module_id,
        "autostart": autostart,
        "generated_at_unix_ms": generated_at_unix_ms,
        "settings": settings,
        "ports": ports,
    });
    Ok(serde_json::to_vec_pretty(&document)?)
}

#[cfg(test)]
#[path = "templates_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "templates_dst_tests.rs"]
mod dst_tests;

#[cfg(test)]
#[path = "config_acceptance_tests.rs"]
mod config_acceptance_tests;
