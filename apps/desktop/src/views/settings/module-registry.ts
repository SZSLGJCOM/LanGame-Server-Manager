import type {
  ConfigurationSpecializedRendererRegistration,
  SettingsModuleDefinition
} from "./module-types";
import type { ConfigurationFieldBehavior } from "./settings-schema";
import { withConfigurationFieldGroups } from "./configuration-field-groups";
import { withMaintenanceSavePolicy } from "./save-policy";
import { abioticFactorSettingsDefinition } from "./modules/abioticfactor";
import { arkSurvivalAscendedSettingsDefinition } from "./modules/ark-asa";
import { arkSurvivalEvolvedSettingsDefinition } from "./modules/ark-ase";
import { astroneerSettingsDefinition } from "./modules/astroneer";
import { barotraumaSettingsDefinition } from "./modules/barotrauma";
import { conanExilesSettingsDefinition } from "./modules/conanexiles";
import { coreKeeperSettingsDefinition } from "./modules/corekeeper";
import { dontStarveSettingsDefinition } from "./modules/dontstarve";
import { enshroudedSettingsDefinition } from "./modules/enshrouded";
import { humanitzSettingsDefinition } from "./modules/humanitz";
import { minecraftSettingsDefinition } from "./modules/minecraft";
import { necesseSettingsDefinition } from "./modules/necesse";
import { nightingaleSettingsDefinition } from "./modules/nightingale";
import { palworldSettingsDefinition } from "./modules/palworld";
import { projectZomboidSettingsDefinition } from "./modules/projectzomboid";
import { returntomoriaSettingsDefinition } from "./modules/returntomoria";
import { rimworldSettingsDefinition } from "./modules/rimworld";
import { romesteadSettingsDefinition } from "./modules/romestead";
import { runescapedragonwildsSettingsDefinition } from "./modules/runescapedragonwilds";
import { rustSettingsDefinition } from "./modules/rust";
import { satisfactorySettingsDefinition } from "./modules/satisfactory";
import { scumSettingsDefinition } from "./modules/scum";
import { sevenDaysToDieSettingsDefinition } from "./modules/sevendaystodie";
import { soulmaskSettingsDefinition } from "./modules/soulmask";
import { sonsOfTheForestSettingsDefinition } from "./modules/sonsoftheforest";
import { squadSettingsDefinition } from "./modules/squad";
import { terrariaSettingsDefinition } from "./modules/terraria";
import { theforestSettingsDefinition } from "./modules/theforest";
import { unturnedSettingsDefinition } from "./modules/unturned";
import { valheimSettingsDefinition } from "./modules/valheim";
import { vrisingSettingsDefinition } from "./modules/vrising";
import { windroseSettingsDefinition } from "./modules/windrose";

const SECRET_FIELD_KEYS_BY_MODULE: Readonly<Record<string, readonly string[]>> = {
  abioticfactor: ["server_password", "admin_password"],
  arksurvivalascended: ["server_password", "admin_password"],
  arksurvivalevolved: ["spectator_password", "server_password", "admin_password"],
  astroneer: ["server_password", "console_password"],
  barotrauma: ["server_password"],
  conanexiles: ["server_password", "admin_password", "rcon_password"],
  corekeeper: ["join_password"],
  dontstarve: ["cluster_password", "cluster_token"],
  enshrouded: ["admin_password", "friend_password", "guest_password", "visitor_password"],
  humanitz: ["server_password", "admin_password", "rcon_password"],
  minecraft: ["rcon_password", "management_server_secret", "management_server_tls_keystore_password"],
  necesse: ["password"],
  nightingale: ["server_password", "admin_password"],
  palworld: ["server_password", "admin_password"],
  projectzomboid: ["server_password", "admin_password", "rcon_password", "discord_token"],
  returntomoria: ["server_password"],
  romestead: ["password"],
  runescapedragonwilds: ["admin_password", "world_password"],
  rust: ["rcon_password", "reports_server_endpoint_key"],
  sevendaystodie: ["server_password", "telnet_password"],
  sonsoftheforest: ["server_password"],
  soulmask: ["server_password", "admin_password", "rcon_password"],
  squad: ["rcon_password"],
  terraria: ["password"],
  theforest: ["server_password", "admin_password", "steam_account_token"],
  unturned: ["password", "game_server_login_token"],
  valheim: ["server_password"],
  vrising: ["server_password", "rcon_password"],
  windrose: ["server_password"]
};

const FIELD_BEHAVIOR_OVERRIDES: Readonly<Record<string, Readonly<Record<string, ConfigurationFieldBehavior>>>> = {
  arksurvivalascended: {
    custom_launch_flags: "raw",
    game_user_settings_extra: "raw",
    game_ini_extra: "raw"
  },
  arksurvivalevolved: {
    custom_launch_flags: "raw",
    game_user_settings_extra: "raw",
    game_ini_extra: "raw"
  },
  astroneer: { extra_launch_args: "raw" },
  barotrauma: { extra_launch_args: "raw" },
  conanexiles: {
    server_message_of_the_day: "multiline",
    server_settings_extra: "raw",
    game_ini_extra: "raw",
    engine_ini_extra: "raw",
    custom_launch_flags: "raw"
  },
  dontstarve: {
    master_world_overrides_extra: "raw",
    master_worldgenoverride_lua: "raw",
    caves_world_overrides_extra: "raw",
    caves_worldgenoverride_lua: "raw",
    master_modoverrides_lua: "raw",
    caves_modoverrides_lua: "raw"
  },
  enshrouded: { custom_user_groups_json: "raw" },
  humanitz: { settings_extra: "raw", extra_launch_args: "raw" },
  minecraft: {
    motd: "multiline",
    management_server_tls_keystore: "path",
    extra_properties: "raw"
  },
  necesse: { motd: "multiline", custom_launch_flags: "raw" },
  nightingale: { extra_launch_args: "raw" },
  projectzomboid: {
    server_description: "multiline",
    sandbox_vars_lua: "raw",
    spawnpoints_lua: "raw",
    spawnregions_lua: "raw"
  },
  returntomoria: { extra_launch_args: "raw" },
  rimworld: { extra_launch_args: "raw" },
  romestead: { extra_launch_args: "raw" },
  runescapedragonwilds: { extra_launch_args: "raw" },
  rust: {
    world_config_json: "raw",
    server_cfg_extra: "raw",
    users_cfg_extra: "raw",
    bans_cfg_extra: "raw",
    custom_launch_flags: "raw"
  },
  satisfactory: {
    engine_ini_extra: "raw",
    game_ini_extra: "raw",
    custom_launch_flags: "raw"
  },
  scum: { extra_launch_args: "raw" },
  sonsoftheforest: { extra_launch_args: "raw" },
  soulmask: { extra_launch_args: "raw" },
  squad: {
    admins_cfg: "raw",
    map_rotation: "raw",
    custom_options: "raw",
    layer_rotation: "raw",
    level_rotation: "raw",
    layer_voting: "raw",
    layer_voting_low_players: "raw",
    layer_voting_night: "raw",
    vote_config: "raw",
    excluded_factions: "raw",
    excluded_layers: "raw",
    excluded_levels: "raw",
    motd_cfg: "raw",
    remote_ban_hosts: "raw",
    remote_admin_hosts: "raw",
    server_message: "raw",
    extra_launch_args: "raw"
  },
  terraria: {
    motd: "multiline",
    tmodloader_runtime_dir: "path"
  },
  theforest: { extra_launch_args: "raw" },
  unturned: { custom_launch_flags: "raw" },
  valheim: { log_file: "path", custom_launch_flags: "raw" },
  vrising: { server_game_settings_json: "raw" }
};

// Official acquisition pages are product-owned metadata, never derived from a credential value.
const FIELD_RESOURCE_URLS: Readonly<Record<string, Readonly<Record<string, string>>>> = {
  dontstarve: { cluster_token: "https://accounts.klei.com/account/game/servers?game=DontStarveTogether" },
  unturned: { game_server_login_token: "https://steamcommunity.com/dev/managegameservers" },
  theforest: { steam_account_token: "https://steamcommunity.com/dev/managegameservers" },
  projectzomboid: { discord_token: "https://discord.com/developers/applications/select/bot" }
};

function withExplicitFieldPresentation(definition: SettingsModuleDefinition): SettingsModuleDefinition {
  const behaviors = new Map<string, ConfigurationFieldBehavior>();
  for (const fieldKey of SECRET_FIELD_KEYS_BY_MODULE[definition.id] ?? []) {
    behaviors.set(fieldKey, "secret");
  }
  for (const [fieldKey, behavior] of Object.entries(FIELD_BEHAVIOR_OVERRIDES[definition.id] ?? {})) {
    behaviors.set(fieldKey, behavior);
  }
  const resources = FIELD_RESOURCE_URLS[definition.id] ?? {};
  if (behaviors.size === 0 && Object.keys(resources).length === 0) {
    return definition;
  }

  const fieldPresentationOverrides = { ...definition.fieldPresentationOverrides };
  for (const [fieldKey, behavior] of behaviors) {
    fieldPresentationOverrides[fieldKey] = {
      ...fieldPresentationOverrides[fieldKey],
      behavior
    };
  }
  for (const [fieldKey, resourceUrl] of Object.entries(resources)) {
    fieldPresentationOverrides[fieldKey] = {
      ...fieldPresentationOverrides[fieldKey],
      resourceUrl
    };
  }
  return { ...definition, fieldPresentationOverrides };
}

function withWorkspaceRenderedField(
  definition: SettingsModuleDefinition,
  fieldKey: string,
  sectionId: string,
  rendererId: string,
  workspace: "mods" | "player_access"
): SettingsModuleDefinition {
  return {
    ...definition,
    fieldPresentationOverrides: {
      ...definition.fieldPresentationOverrides,
      [fieldKey]: {
        state: "specialized",
        owner: workspace,
        sectionId,
        rendererId
      }
    },
    specializedRenderers: {
      ...definition.specializedRenderers,
      [rendererId]: { kind: "workspace", workspace }
    }
  };
}

const arkSurvivalEvolvedPresentation = withWorkspaceRenderedField(
  arkSurvivalEvolvedSettingsDefinition,
  "active_mod_ids",
  "mods",
  "mod-workbench-workshop-ids",
  "mods"
);
const arkSurvivalAscendedPresentation = withWorkspaceRenderedField(
  arkSurvivalAscendedSettingsDefinition,
  "mod_ids_csv",
  "mods",
  "mod-workbench-asa-mod-ids",
  "mods"
);
const barotraumaPresentation = withWorkspaceRenderedField(
  barotraumaSettingsDefinition, "mod_workshop_ids", "runtime", "mod-workbench-workshop-ids", "mods"
);
const conanExilesPresentation = withWorkspaceRenderedField(
  conanExilesSettingsDefinition, "mod_workshop_ids", "mods", "mod-workbench-workshop-ids", "mods"
);
const soulmaskPresentation = withWorkspaceRenderedField(
  soulmaskSettingsDefinition, "mod_workshop_ids", "advanced", "mod-workbench-workshop-ids", "mods"
);
const projectZomboidPresentation = withWorkspaceRenderedField(
  withWorkspaceRenderedField(
    withWorkspaceRenderedField(
      projectZomboidSettingsDefinition, "map_name", "mods", "mod-workbench-map-order", "mods"
    ),
    "workshop_items", "mods", "mod-workbench-workshop-ids", "mods"
  ),
  "mods", "mods", "mod-workbench-mod-ids", "mods"
);
const palworldPresentation = withWorkspaceRenderedField(
  palworldSettingsDefinition,
  "mod_package_names",
  "services",
  "mod-workbench-package-names",
  "mods"
);
const terrariaPresentation = withWorkspaceRenderedField(
  terrariaSettingsDefinition,
  "tmodloader_workshop_item_ids",
  "mods",
  "mod-workbench-workshop-ids",
  "mods"
);
const unturnedPresentation = withWorkspaceRenderedField(
  unturnedSettingsDefinition,
  "workshop_file_ids",
  "advanced",
  "mod-workbench-workshop-ids",
  "mods"
);
const dontStarvePresentation = [
  "shared_workshop_mod_ids",
  "shared_workshop_collection_ids",
  "master_enabled_workshop_mod_ids",
  "caves_enabled_workshop_mod_ids",
  "islands_enabled_workshop_mod_ids",
  "volcano_enabled_workshop_mod_ids",
  "master_mod_configuration_options",
  "caves_mod_configuration_options",
  "islands_mod_configuration_options",
  "volcano_mod_configuration_options"
].reduce<SettingsModuleDefinition>((definition, fieldKey) => withWorkspaceRenderedField(
  definition, fieldKey, "mods", "mod-workbench-dst-mods", "mods"
), dontStarveSettingsDefinition);
const BASE_SETTINGS_MODULE_DEFINITIONS: Record<string, SettingsModuleDefinition> = {
  [abioticFactorSettingsDefinition.id]: abioticFactorSettingsDefinition,
  [arkSurvivalEvolvedPresentation.id]: arkSurvivalEvolvedPresentation,
  [arkSurvivalAscendedPresentation.id]: arkSurvivalAscendedPresentation,
  [astroneerSettingsDefinition.id]: astroneerSettingsDefinition,
  [barotraumaPresentation.id]: barotraumaPresentation,
  [conanExilesPresentation.id]: conanExilesPresentation,
  [coreKeeperSettingsDefinition.id]: coreKeeperSettingsDefinition,
  [dontStarvePresentation.id]: dontStarvePresentation,
  [enshroudedSettingsDefinition.id]: enshroudedSettingsDefinition,
  [humanitzSettingsDefinition.id]: humanitzSettingsDefinition,
  [minecraftSettingsDefinition.id]: minecraftSettingsDefinition,
  [necesseSettingsDefinition.id]: necesseSettingsDefinition,
  [nightingaleSettingsDefinition.id]: nightingaleSettingsDefinition,
  [palworldPresentation.id]: palworldPresentation,
  [projectZomboidPresentation.id]: projectZomboidPresentation,
  [returntomoriaSettingsDefinition.id]: {
    ...returntomoriaSettingsDefinition,
    fieldPresentationOverrides: {
      ...returntomoriaSettingsDefinition.fieldPresentationOverrides,
      permissions_lines: { owner: "player_access", behavior: "raw" },
      upgrade_optional_dlc_array: { owner: "maintenance" }
    }
  },
  [rimworldSettingsDefinition.id]: rimworldSettingsDefinition,
  [romesteadSettingsDefinition.id]: romesteadSettingsDefinition,
  [runescapedragonwildsSettingsDefinition.id]: runescapedragonwildsSettingsDefinition,
  [rustSettingsDefinition.id]: rustSettingsDefinition,
  [satisfactorySettingsDefinition.id]: satisfactorySettingsDefinition,
  [scumSettingsDefinition.id]: scumSettingsDefinition,
  [sevenDaysToDieSettingsDefinition.id]: sevenDaysToDieSettingsDefinition,
  [soulmaskPresentation.id]: soulmaskPresentation,
  [sonsOfTheForestSettingsDefinition.id]: sonsOfTheForestSettingsDefinition,
  [squadSettingsDefinition.id]: squadSettingsDefinition,
  [terrariaPresentation.id]: terrariaPresentation,
  [theforestSettingsDefinition.id]: theforestSettingsDefinition,
  [unturnedPresentation.id]: unturnedPresentation,
  [valheimSettingsDefinition.id]: valheimSettingsDefinition,
  [vrisingSettingsDefinition.id]: vrisingSettingsDefinition,
  [windroseSettingsDefinition.id]: windroseSettingsDefinition
};

const SETTINGS_MODULE_DEFINITIONS: Record<string, SettingsModuleDefinition> = Object.fromEntries(
  Object.entries(BASE_SETTINGS_MODULE_DEFINITIONS).map(([moduleId, definition]) => [
    moduleId,
    withConfigurationFieldGroups(withExplicitFieldPresentation(withMaintenanceSavePolicy(definition)))
  ])
);

export function resolveSettingsModuleDefinition(moduleId?: string | null): SettingsModuleDefinition | null {
  if (!moduleId) {
    return null;
  }

  return SETTINGS_MODULE_DEFINITIONS[moduleId] ?? null;
}

export function listSettingsModuleIds(): string[] {
  return Object.keys(SETTINGS_MODULE_DEFINITIONS).sort();
}

export interface ResolvedConfigurationSpecializedRenderer
  extends ConfigurationSpecializedRendererRegistration {
  id: string;
}

export function listConfigurationSpecializedRenderers(
  definition: SettingsModuleDefinition | null,
  sectionId: string
): ResolvedConfigurationSpecializedRenderer[] {
  return Object.entries(definition?.specializedRenderers ?? {}).flatMap(([id, registration]) => {
    if (
      registration.kind !== "module-addon" ||
      !("Renderer" in registration) ||
      typeof registration.Renderer !== "function" ||
      registration.sectionId !== sectionId
    ) {
      return [];
    }
    return [{ id, ...registration }];
  });
}
