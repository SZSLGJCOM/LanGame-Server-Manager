import { isChineseLocale, type TranslateFn } from "../../../i18n";
import { EN_US_BAROTRAUMA_MESSAGES } from "../../../i18n/games/barotrauma.en";
import { ZH_CN_BAROTRAUMA_MESSAGES } from "../../../i18n/games/barotrauma.zh-cn";
import type { SettingsModuleDefinition, SettingsModuleFieldGroup } from "../module-types";
import type { GuidedFieldCopy, GuidedSettingsField, GuidedSettingsSection } from "../settings-schema";

const BAROTRAUMA_STEAM2_PATTERN = /^STEAM_[01]:[01]:\d+$/i;
const BAROTRAUMA_STEAM3_PATTERN = /^\[U:1:\d+\]$/i;
const STEAM64_PATTERN = /^\d{17}$/;
const STEAM_WORKSHOP_ID_PATTERN = /^\d+$/;

const BAROTRAUMA_SECTIONS: GuidedSettingsSection[] = [
  { id: "round", title: "Round and respawning", description: "Level selection, missions, round starts and character respawning." },
  { id: "voting", title: "Voting", description: "Player voting thresholds and time limits." },
  { id: "pvp", title: "PvP and traitors", description: "Team combat, scoring and traitor events." },
  { id: "bots", title: "Bots and NPCs", description: "Crew population and non-player character behavior." },
  { id: "gameplay", title: "Gameplay", description: "Visibility, interactions, karma and starting perks." },
  { id: "campaign", title: "Campaign", description: "Campaign creation, economy, rewards and difficulty." },
];

interface BarotraumaGroupSpec {
  id: string;
  title: string;
  description: string;
  keys: string[];
}

const BAROTRAUMA_GROUP_SPECS: Record<string, BarotraumaGroupSpec[]> = {
  network: [
    { id: "connectivity", title: "Connectivity", description: "Port mapping and delayed weapon-input compensation.", keys: ["enable_upnp", "max_lag_compensation"] },
    { id: "voice-transfer", title: "Voice and content transfer", description: "Voice chat and files delivered to joining players.", keys: ["voice_chat_enabled", "allow_file_transfers", "allow_mod_downloads"] },
    { id: "traffic", title: "Traffic limits", description: "Protection against excessive client messages.", keys: ["enable_dos_protection", "max_packet_amount"] },
    { id: "synchronization", title: "Synchronization and timeouts", description: "Connection, event delivery and synchronization time budgets.", keys: ["minimum_mid_round_sync_timeout", "round_start_sync_duration", "event_removal_time", "old_received_event_kick_time", "old_event_kick_time", "timeout_threshold_not_in_game", "timeout_threshold_in_game"] },
  ],
  access: [
    { id: "authentication", title: "Joining requirements", description: "Account authentication and permitted player names.", keys: ["require_authentication", "allowed_client_name_chars"] },
    { id: "inactivity", title: "Away status and inactivity", description: "Away status and automatic inactivity removal.", keys: ["allow_afk", "kick_afk_time"] },
    { id: "wrong-password", title: "Automatic bans", description: "Password attempt limits and automatic ban durations.", keys: ["ban_after_wrong_password", "max_password_retries_before_ban", "auto_ban_time", "max_auto_ban_time"] },
  ],
  runtime: [
    { id: "simulation", title: "Simulation", description: "Server simulation updates per second.", keys: ["tick_rate"] },
    { id: "logs", title: "Server logs", description: "Native game log output and file rotation.", keys: ["save_server_logs", "lines_per_log_file"] },
    { id: "launch", title: "Launch arguments", description: "Additional arguments for DedicatedServer.", keys: ["extra_launch_args"] },
  ],
  round: [
    { id: "scenario", title: "Level and game mode", description: "Mode, level seed, biome and difficulty selection.", keys: ["game_mode_identifier", "randomize_seed", "biome", "selected_outpost_name", "selected_level_difficulty", "mode_selection_mode"] },
    { id: "submarines", title: "Submarines", description: "Main submarine, respawn shuttle and selection rules.", keys: ["selected_submarine", "selected_shuttle", "sub_selection_mode", "hidden_subs"] },
    { id: "missions", title: "Mission selection", description: "Selected mission types and the random mission pool.", keys: ["mission_types", "allowed_random_mission_types"] },
    { id: "auto-start", title: "Round start", description: "Automatic lobby countdown and player-ready checks.", keys: ["auto_restart", "auto_restart_interval", "start_when_clients_ready", "start_when_clients_ready_ratio"] },
    { id: "respawn", title: "Respawn timing and transport", description: "Respawn mode, shuttle timing and required player ratio.", keys: ["respawn_mode", "use_respawn_shuttle", "respawn_interval", "max_transport_time", "min_respawn_ratio"] },
    { id: "death", title: "Death penalties", description: "Skill loss, replacement characters and permanent death.", keys: ["skill_loss_percentage_on_death", "skill_loss_percentage_on_immediate_respawn", "replace_cost_percentage", "allow_bot_takeover_on_permadeath", "ironman_mode"] },
    { id: "disconnected", title: "Disconnected characters", description: "How long disconnected characters remain in the world.", keys: ["kill_disconnected_time", "despawn_disconnected_permadeath_time"] },
  ],
  voting: [
    { id: "end-round", title: "End-round votes", description: "Enable round-ending votes and choose the required support.", keys: ["allow_end_voting", "end_vote_required_ratio"] },
    { id: "kick", title: "Kick votes", description: "Player-removal votes and new-voter eligibility.", keys: ["allow_vote_kick", "kick_vote_required_ratio", "disallow_kick_vote_time"] },
    { id: "general-votes", title: "Other votes", description: "Support threshold and duration for other supported votes.", keys: ["vote_required_ratio", "vote_timeout"] },
  ],
  pvp: [
    { id: "teams", title: "Teams and combat", description: "Team assignment, balance, friendly fire and stun resistance.", keys: ["pvp_team_selection_mode", "pvp_auto_balance_threshold", "allow_friendly_fire", "pvp_stun_resist"] },
    { id: "battlefield", title: "PvP level and scoring", description: "Level content, sonar tracking and the winning score.", keys: ["pvp_spawn_monsters", "pvp_spawn_wrecks", "track_opponent_in_pvp", "win_score_pvp"] },
    { id: "traitors", title: "Traitor events", description: "Probability, danger, player requirements and accusations.", keys: ["traitor_probability", "traitor_danger_level", "traitors_min_player_count", "min_percentage_of_players_for_traitor_accusation"] },
  ],
  bots: [
    { id: "crew-bots", title: "Crew population", description: "Bot count, population limit and filling behavior.", keys: ["bot_count", "max_bot_count", "bot_spawn_mode"] },
    { id: "npc-behavior", title: "NPC behavior", description: "Crew conversations and NPC vulnerability.", keys: ["disable_bot_conversations", "killable_npcs"] },
  ],
  gameplay: [
    { id: "visibility", title: "Visibility and spectating", description: "Spectating, obstruction and enemy health information.", keys: ["allow_spectating", "los_mode", "show_enemy_health_bars"] },
    { id: "interactions", title: "Player interactions", description: "Identity, wiring, item transfers and outpost damage.", keys: ["allow_disguises", "allow_rewiring", "lock_all_default_wires", "allow_linking_wifi_to_chat", "allow_drag_and_drop_give", "destructible_outposts"] },
    { id: "karma", title: "Karma", description: "Player-conduct tracking and rules.", keys: ["karma_enabled", "karma_preset"] },
    { id: "perks", title: "Starting perks", description: "Perk point budget and selected faction perks.", keys: ["disembark_point_allowance", "selected_coalition_perks", "selected_separatists_perks"] },
    { id: "creatures", title: "Creature spawning", description: "Creature species excluded from spawning.", keys: ["disabled_monsters"] },
  ],
  campaign: [
    { id: "campaign-creation", title: "Campaign setup", description: "Preset, starting supplies, funds, mission capacity and world pressure.", keys: ["campaign_preset_name", "campaign_tutorial_enabled", "campaign_start_item_set", "campaign_starting_balance_amount", "campaign_max_mission_count", "campaign_world_hostility", "campaign_radiation_enabled"] },
    { id: "campaign-money", title: "Money and salaries", description: "Loot destinations, transfer requests and starting salaries.", keys: ["looted_money_destination", "maximum_money_transfer_request", "new_campaign_default_salary"] },
    { id: "campaign-trading", title: "Trading and deliveries", description: "Shop access, deliveries and price multipliers.", keys: ["allow_remote_campaign_interactions", "allow_immediate_item_delivery", "campaign_shop_price_multiplier", "campaign_shipyard_price_multiplier"] },
    { id: "campaign-survival", title: "Survival and equipment", description: "Vitality, oxygen capacity, fuel duration and repair injury.", keys: ["campaign_crew_vitality_multiplier", "campaign_non_crew_vitality_multiplier", "campaign_oxygen_multiplier", "campaign_fuel_multiplier", "campaign_repair_fail_multiplier"] },
    { id: "campaign-rewards", title: "Rewards", description: "Mission money and character experience.", keys: ["campaign_mission_reward_multiplier", "campaign_experience_reward_multiplier"] },
    { id: "campaign-security", title: "Warnings and security", description: "Infection warnings and outpost searches.", keys: ["campaign_show_husk_warning", "campaign_patdown_probability"] },
  ],
};

function catalog(locale: string) {
  return isChineseLocale(locale) ? ZH_CN_BAROTRAUMA_MESSAGES : EN_US_BAROTRAUMA_MESSAGES;
}

function buildBarotraumaSections(t: TranslateFn, locale = "en-US"): GuidedSettingsSection[] {
  const messages = catalog(locale);
  return BAROTRAUMA_SECTIONS.map((section) => ({
    ...section,
    title: messages[`barotrauma.settings.sections.${section.id}`] ?? t(`barotrauma.settings.sections.${section.id}`, undefined, section.title),
    description: messages[`barotrauma.settings.sections.${section.id}Description`] ?? t(
      `barotrauma.settings.sections.${section.id}Description`,
      undefined,
      section.description ?? ""
    )
  }));
}

function buildBarotraumaFieldCopy(key: string, locale: string): GuidedFieldCopy | undefined {
  const messages = catalog(locale);
  const baseKey = `settings.schema.barotrauma.${key}`;
  const title = messages[`${baseKey}.title`];
  return title ? { title, description: messages[`${baseKey}.description`] } : undefined;
}

function buildBarotraumaFieldGroups(
  sectionId: string,
  fields: GuidedSettingsField[],
  t: TranslateFn,
  locale: string
): SettingsModuleFieldGroup[] {
  const messages = catalog(locale);
  const fieldsByKey = new Map(fields.map((field) => [field.key, field]));
  const claimedKeys = new Set<string>();
  const groups: SettingsModuleFieldGroup[] = [];

  for (const spec of BAROTRAUMA_GROUP_SPECS[sectionId] ?? []) {
    const groupFields = spec.keys
      .map((key) => fieldsByKey.get(key))
      .filter((field): field is GuidedSettingsField => Boolean(field));

    if (groupFields.length === 0) {
      continue;
    }

    for (const field of groupFields) {
      claimedKeys.add(field.key);
    }

    groups.push({
      id: spec.id,
      title: messages[`barotrauma.settings.groups.${spec.id}.title`] ?? t(`barotrauma.settings.groups.${spec.id}.title`, undefined, spec.title),
      description: messages[`barotrauma.settings.groups.${spec.id}.description`] ?? t(
        `barotrauma.settings.groups.${spec.id}.description`,
        undefined,
        spec.description
      ),
      layoutClass: `barotrauma-${spec.id}`,
      fields: groupFields
    });
  }

  const remainingFields = fields.filter((field) => !claimedKeys.has(field.key));
  if (remainingFields.length > 0) {
    groups.push({
      id: "additional",
      title: messages["barotrauma.settings.groups.additional.title"] ?? t("barotrauma.settings.groups.additional.title", undefined, "Other settings"),
      layoutClass: `${sectionId}-additional`,
      fields: remainingFields
    });
  }

  return groups.length > 0 ? groups : [{ id: "default", fields }];
}

function parseBarotraumaAdminIds(value: unknown): string[] {
  if (typeof value !== "string") {
    return [];
  }

  return value
    .replace(/\r\n/g, "\n")
    .replace(/\r/g, "\n")
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0 && !line.startsWith("#") && !line.startsWith("//"))
    .map((line) => line.split(/[,|]/)[0]?.trim() ?? "")
    .filter(Boolean);
}

function isBarotraumaAdminId(value: string): boolean {
  return (
    BAROTRAUMA_STEAM2_PATTERN.test(value) ||
    BAROTRAUMA_STEAM3_PATTERN.test(value) ||
    STEAM64_PATTERN.test(value)
  );
}

function parseBarotraumaWorkshopIds(value: unknown): string[] {
  if (typeof value !== "string") {
    return [];
  }

  return value
    .replace(/\r\n/g, "\n")
    .replace(/\r/g, "\n")
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0 && !line.startsWith("#") && !line.startsWith("//"));
}

export const barotraumaSettingsDefinition: SettingsModuleDefinition = {
  id: "barotrauma",
  getSections: buildBarotraumaSections,
  buildFieldGroups: (sectionId, fields, locale, t) =>
    buildBarotraumaFieldGroups(sectionId, fields, t, locale),
  getFieldCopy: (key, _t, locale = "en-US") => buildBarotraumaFieldCopy(key, locale),
  getEnumOptionLabel: (key, value, locale) =>
    catalog(locale)[`settings.schema.barotrauma.${key}.option.${String(value).toLowerCase()}`],
  resolveFieldEditorVariant(key) {
    if (key === "admin_entries" || key === "mod_workshop_ids") {
      return "string-list";
    }

    return undefined;
   },
  getFieldValidationMessage({ field, value, t }) {
    if (field.key === "admin_entries") {
      const invalid = parseBarotraumaAdminIds(value).filter((entry) => !isBarotraumaAdminId(entry));
      if (invalid.length > 0) {
        const preview = invalid.slice(0, 3).join(", ");
        return t(
          "barotrauma.settings.validation.adminEntries",
          { preview  },
          `Use Steam2, Steam3, or SteamID64 values for Barotrauma admins. Fix: ${preview}`
        );
      }
    }
    if (field.key === "mod_workshop_ids") {
      const invalid = parseBarotraumaWorkshopIds(value).filter((entry) => !STEAM_WORKSHOP_ID_PATTERN.test(entry));
      if (invalid.length > 0) {
        const preview = invalid.slice(0, 3).join(", ");
        return t(
          "barotrauma.settings.validation.workshopIds",
          { preview  },
          `Workshop mod IDs must be numeric Steam Workshop item IDs. Fix: ${preview}`
        );
      }
    }

    return undefined;
  }
};
