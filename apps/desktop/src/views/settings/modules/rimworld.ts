import type { SettingsModuleDefinition } from "../module-types";
import { createSurvivalDedicatedSettingsDefinition } from "./survival-dedicated";
import { EN_US_RIMWORLD_MESSAGES } from "../../../i18n/games/rimworld.en";
import { ZH_CN_RIMWORLD_MESSAGES } from "../../../i18n/games/rimworld.zh-cn";

const RIMWORLD_FIELD_GROUPS = [
  { id: "player-feedback", sectionId: "room", keys: ["chat_login_notifications", "chat_disconnect_notifications"] },
  { id: "chat", sectionId: "room", keys: ["chat_enable_mo_td", "chat_message_of_the_day"] },
  { id: "world-policy", sectionId: "world", keys: ["difficulty_is_difficulty_enforced", "scenario_is_enforced", "scenario_name", "storyteller_is_enforced", "storyteller_def_name"] },
  { id: "difficulty-threats", sectionId: "world", keys: ["difficulty_threat_scale", "difficulty_allow_big_threats", "difficulty_allow_violent_quests", "difficulty_allow_intro_threats", "difficulty_predators_hunt_humanlikes", "difficulty_allow_extreme_weather_incidents", "difficulty_scaria_rot_chance", "difficulty_enemy_death_on_downed_chance_factor", "difficulty_manhunter_chance_on_damage_factor", "difficulty_deep_drill_infestation_chance_factor", "difficulty_friendly_fire_chance_factor", "difficulty_allow_instant_kill_chance", "difficulty_peaceful_temples", "difficulty_allow_cave_hives", "difficulty_allow_traps", "difficulty_allow_turrets", "difficulty_allow_mortars", "difficulty_classic_mortars", "difficulty_adaptation_effect_factor", "difficulty_adaptation_growth_rate_factor_over_zero", "difficulty_wastepack_infestation_chance_factor"] },
  { id: "difficulty-economy", sectionId: "world", keys: ["difficulty_crop_yield_factor", "difficulty_mine_yield_factor", "difficulty_butcher_yield_factor", "difficulty_research_speed_factor", "difficulty_quest_reward_value_factor", "difficulty_raid_loot_points_factor", "difficulty_trade_price_factor_loss", "difficulty_maintenance_cost_factor", "difficulty_fixed_wealth_mode", "difficulty_nomadic_mineable_resources_factor"] },
  { id: "difficulty-colonists", sectionId: "world", keys: ["difficulty_colonist_mood_offset", "difficulty_food_poison_chance_factor", "difficulty_player_pawn_infection_chance_factor", "difficulty_disease_interval_factor", "difficulty_enemy_reproduction_rate_factor", "difficulty_unwavering_prisoners", "difficulty_low_pop_conversion_boost", "difficulty_no_babies_or_children", "difficulty_babies_are_healthy", "difficulty_child_raiders_allowed", "difficulty_child_aging_rate", "difficulty_adult_aging_rate"] },
  { id: "action-aid", sectionId: "world", keys: ["action_aid_is_enabled", "action_aid_cooldown"] },
  { id: "action-caravan", sectionId: "world", keys: ["action_caravan_is_enabled", "action_caravan_cooldown"] },
  { id: "action-event", sectionId: "world", keys: ["action_event_is_enabled", "action_event_cooldown"] },
  { id: "action-guild", sectionId: "world", keys: ["action_guild_is_enabled", "action_guild_cooldown"] },
  { id: "action-leaderboard", sectionId: "world", keys: ["action_leaderboard_is_enabled", "action_leaderboard_cooldown"] },
  { id: "action-market", sectionId: "world", keys: ["action_market_is_enabled", "action_market_cooldown", "action_market_minimum_price", "action_market_price_multiplier", "action_market_max_entries_per_player"] },
  { id: "action-pollution", sectionId: "world", keys: ["action_pollution_is_enabled", "action_pollution_cooldown"] },
  { id: "action-raid", sectionId: "world", keys: ["action_raid_is_enabled", "action_raid_cooldown"] },
  { id: "action-river", sectionId: "world", keys: ["action_river_is_enabled", "action_river_cooldown"] },
  { id: "action-road", sectionId: "world", keys: ["action_road_is_enabled", "action_road_cooldown", "action_road_allow_dirt_path", "action_road_allow_dirt_road", "action_road_allow_stone_road", "action_road_allow_asphalt_path", "action_road_allow_asphalt_highway", "action_road_dirt_path_cost", "action_road_dirt_road_cost", "action_road_stone_road_cost", "action_road_asphalt_path_cost", "action_road_asphalt_highway_cost", "action_road_dirt_path_multiplier", "action_road_dirt_road_multiplier", "action_road_stone_road_multiplier", "action_road_asphalt_path_multiplier", "action_road_asphalt_highway_multiplier"] },
  { id: "action-scenario", sectionId: "world", keys: ["action_scenario_is_enabled", "action_scenario_cooldown"] },
  { id: "action-site", sectionId: "world", keys: ["action_site_is_enabled", "action_site_cooldown", "action_site_time_interval", "action_site_building_cost", "action_site_rewards_count"] },
  { id: "action-trade", sectionId: "world", keys: ["action_trade_is_enabled", "action_trade_cooldown"] },
  { id: "action-world_object", sectionId: "world", keys: ["action_world_object_is_enabled", "action_world_object_cooldown"] },
  { id: "action-zoom", sectionId: "world", keys: ["action_zoom_is_enabled", "action_zoom_cooldown"] },
  {
    id: "network",
    sectionId: "network",
    keys: ["bind_ip", "enable_server_telemetry", "use_upnp"]
  },
  { id: "console", sectionId: "runtime", keys: ["verbosity", "display_chat_in_console", "sync_local_save"] },
  { id: "advanced", sectionId: "advanced", keys: ["extra_launch_args"] }
] as const;

const rimworldBaseSettingsDefinition = createSurvivalDedicatedSettingsDefinition(
  "rimworld",
  ["network", "world", "runtime", "advanced"],
  RIMWORLD_FIELD_GROUPS
);

export const rimworldSettingsDefinition: SettingsModuleDefinition = {
  id: "rimworld",
  getSections: rimworldBaseSettingsDefinition.getSections,
  buildFieldGroups: rimworldBaseSettingsDefinition.buildFieldGroups,
  getFieldCopy: (key, _t, locale = "en-US") => {
    const catalog = locale.toLowerCase().startsWith("zh")
      ? ZH_CN_RIMWORLD_MESSAGES
      : EN_US_RIMWORLD_MESSAGES;
    const prefix = `settings.schema.rimworld.${key}`;
    const title = catalog[`${prefix}.title`];
    return title ? { title, description: catalog[`${prefix}.description`] ?? "" } : undefined;
  }
};
