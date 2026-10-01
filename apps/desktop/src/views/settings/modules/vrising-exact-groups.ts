export const VRISING_VAMPIRE_STAT_KEYS = [
  "vampire_max_health_modifier",
  "vampire_physical_power_modifier",
  "vampire_spell_power_modifier",
  "vampire_resource_power_modifier",
  "vampire_siege_power_modifier",
  "vampire_damage_received_modifier",
  "vampire_revive_cancel_delay"
] as const;

export const VRISING_UNIT_STAT_KEYS = [
  "global_unit_max_health_modifier",
  "global_unit_power_modifier",
  "global_unit_level_increase",
  "vblood_unit_max_health_modifier",
  "vblood_unit_power_modifier",
  "vblood_unit_level_increase"
] as const;

export const VRISING_EQUIPMENT_STAT_KEYS = [
  "global_equipment_max_health_modifier",
  "global_equipment_resource_yield_modifier",
  "global_equipment_physical_power_modifier",
  "global_equipment_spell_power_modifier",
  "global_equipment_siege_power_modifier",
  "global_equipment_movement_speed_modifier"
] as const;

export const VRISING_CASTLE_HEART_LIMIT_KEYS = Array.from(
  { length: 5 },
  (_, index) => index + 1
).flatMap((level) => [
  `castle_heart_level_${level}_floor_limit`,
  `castle_heart_level_${level}_servant_limit`,
  `castle_heart_level_${level}_height_limit`
]);
