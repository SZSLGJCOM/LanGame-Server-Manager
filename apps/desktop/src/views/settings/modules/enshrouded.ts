import type { TranslateFn } from "../../../i18n";
import type { SettingsModuleDefinition, SettingsModuleFieldGroup } from "../module-types";
import type { GuidedFieldCopy, GuidedSettingsField, GuidedSettingsSection } from "../settings-schema";

function readCatalogText(t: TranslateFn, key: string): string | undefined {
  const value = t(key, undefined, "");
  return value.trim() ? value : undefined;
}

function buildSchemaEnumOptionKey(value: unknown): string {
  const raw = String(value).trim();
  const prefix = raw.startsWith("-") ? "minus_" : "";
  const normalized = raw
    .replace(/^-+/, "")
    .replace(/[^A-Za-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "")
    .toLowerCase();

  return `${prefix}${normalized || "empty"}`;
}

function buildEnshroudedFieldCopy(key: string, t: TranslateFn): GuidedFieldCopy | undefined {
  const baseKey = `settings.schema.enshrouded.${key}`;
  const title = readCatalogText(t, `${baseKey}.title`);
  const description = readCatalogText(t, `${baseKey}.description`);

  if (!title && !description) {
    return undefined;
  }

  return {
    title: title ?? key,
    description
  };
}

function getEnshroudedEnumOptionLabel(fieldKey: string, value: unknown, t: TranslateFn): string | undefined {
  if (typeof value !== "string" && typeof value !== "number") {
    return undefined;
  }

  if (fieldKey === "server_tags" && typeof value === "string") {
    return readCatalogText(t, `enshrouded.settings.tags.${value}`);
  }

  return readCatalogText(t, `settings.schema.enshrouded.${fieldKey}.option.${buildSchemaEnumOptionKey(value)}`);
}

function buildEnshroudedFieldGroups(
  sectionId: string,
  fields: GuidedSettingsField[],
  _locale: string,
  t: TranslateFn
): SettingsModuleFieldGroup[] {
  const fieldsByKey = new Map(fields.map((field) => [field.key, field]));
  const claimedKeys = new Set<string>();

  const sectionGroups: Record<
    string,
    Array<{ id: string; title: string; description: string; keys: string[] }>
  > = {
    communication: [
      {
        id: "communication",
        title: t("enshrouded.settings.sections.communication", undefined, "Communication"),
        description: t("enshrouded.settings.sections.communicationDescription", undefined, "Voice and text chat rules."),
        keys: ["voice_chat_mode", "enable_voice_chat", "enable_text_chat"]
      }
    ],
    access: [
      {
        id: "customRoles",
        title: t("enshrouded.settings.groups.customRoles", undefined, "Custom roles"),
        description: t("enshrouded.settings.groups.customRolesDescription", undefined, "Additional permission groups."),
        keys: ["custom_user_groups_json"]
      }
    ],
    players: [
      {
        id: "players",
        title: t("enshrouded.settings.groups.players", undefined, "Players"),
        description: t(
          "enshrouded.settings.groups.playersDescription",
          undefined,
          "Health, stamina, mana, and player survivability posture."
        ),
        keys: ["player_health_factor", "player_mana_factor", "player_stamina_factor", "player_body_heat_factor", "player_diving_time_factor", "shroud_time_factor"]
      }
    ],
    survival: [
      {
        id: "survival",
        title: t("enshrouded.settings.groups.survival", undefined, "Survival"),
        description: t(
          "enshrouded.settings.groups.survivalDescription",
          undefined,
          "Durability, starvation, food buff, and curse pressure."
        ),
        keys: ["enable_durability", "enable_starving_debuff", "food_buff_duration_factor", "hunger_to_starving_minutes", "tombstone_mode", "enable_glider_turbulences", "curse_modifier"]
      }
    ],
    world: [
      {
        id: "world",
        title: t("enshrouded.settings.groups.world", undefined, "World"),
        description: t(
          "enshrouded.settings.groups.worldDescription",
          undefined,
          "Weather and day-night cadence for the Enshrouded world."
        ),
        keys: ["weather_frequency", "fishing_difficulty", "day_time_minutes", "night_time_minutes", "game_settings_preset"]
      }
    ],
    progression: [
      {
        id: "progression",
        title: t("enshrouded.settings.groups.progression", undefined, "Progression"),
        description: t(
          "enshrouded.settings.groups.progressionDescription",
          undefined,
          "Gathering, production, rune, and XP multipliers."
        ),
        keys: ["mining_damage_factor", "plant_growth_speed_factor", "resource_drop_stack_amount_factor", "factory_production_speed_factor", "perk_upgrade_recycling_factor", "perk_cost_factor", "experience_combat_factor", "experience_mining_factor", "experience_exploration_quests_factor", "taming_startle_repercussion"]
      }
    ],
    combat: [
      {
        id: "combat",
        title: t("enshrouded.settings.groups.combat", undefined, "Combat"),
        description: t(
          "enshrouded.settings.groups.combatDescription",
          undefined,
          "Enemy pacing and aggro pressure."
        ),
        keys: ["random_spawner_amount", "aggro_pool_amount", "enemy_damage_factor", "enemy_health_factor", "enemy_stamina_factor", "enemy_perception_range_factor", "boss_damage_factor", "boss_health_factor", "threat_bonus", "pacify_all_enemies"]
      }
    ],
    admin_role: [
      {
        id: "adminRole",
        title: t("enshrouded.settings.groups.adminRole", undefined, "Admin role"),
        description: t(
          "enshrouded.settings.groups.adminRoleDescription",
          undefined,
          "Operator-level password and permissions."
        ),
        keys: ["admin_password", "admin_can_kick_ban", "admin_can_access_inventories", "admin_can_edit_world", "admin_can_edit_base", "admin_can_extend_base", "admin_reserved_slots"]
      }
    ],
    friend_role: [
      {
        id: "friendRole",
        title: t("enshrouded.settings.groups.friendRole", undefined, "Friend role"),
        description: t(
          "enshrouded.settings.groups.friendRoleDescription",
          undefined,
          "Collaborator access without full operator power."
        ),
        keys: ["friend_password", "friend_can_kick_ban", "friend_can_access_inventories", "friend_can_edit_world", "friend_can_edit_base", "friend_can_extend_base", "friend_reserved_slots"]
      }
    ],
    guest_role: [
      {
        id: "guestRole",
        title: t("enshrouded.settings.groups.guestRole", undefined, "Guest role"),
        description: t(
          "enshrouded.settings.groups.guestRoleDescription",
          undefined,
          "Temporary access with a smaller edit perimeter."
        ),
        keys: ["guest_password", "guest_can_kick_ban", "guest_can_access_inventories", "guest_can_edit_world", "guest_can_edit_base", "guest_can_extend_base", "guest_reserved_slots"]
      }
    ],
    visitor_role: [
      {
        id: "visitorRole",
        title: t("enshrouded.settings.groups.visitorRole", undefined, "Visitor role"),
        description: t(
          "enshrouded.settings.groups.visitorRoleDescription",
          undefined,
          "One-lane access for short event attendance."
        ),
        keys: ["visitor_password", "visitor_can_kick_ban", "visitor_can_access_inventories", "visitor_can_edit_world", "visitor_can_edit_base", "visitor_can_extend_base", "visitor_reserved_slots"]
      }
    ],
    moderation: [
      {
        id: "moderation",
        title: t("enshrouded.settings.groups.moderation", undefined, "Moderation"),
        description: t(
          "enshrouded.settings.groups.moderationDescription",
          undefined,
          "Player identities denied by the native bans array."
        ),
        keys: ["banned_player_ids"]
      }
    ]
  };

  const groups: SettingsModuleFieldGroup[] = (sectionGroups[sectionId] ?? []).map((spec) => {
    const groupFields = spec.keys
      .map((key) => fieldsByKey.get(key))
      .filter((field): field is GuidedSettingsField => Boolean(field));

    for (const field of groupFields) {
      claimedKeys.add(field.key);
    }

    return {
      id: spec.id,
      title: spec.title,
      description: spec.description,
      fields: groupFields
    };
  }).filter((group) => group.fields.length > 0);

  const remainingFields = fields.filter((field) => !claimedKeys.has(field.key));
  if (remainingFields.length > 0) {
    groups.push({
      id: "additional",
      fields: remainingFields
    });
  }

  return groups;
}

function buildEnshroudedSections(t: TranslateFn): GuidedSettingsSection[] {
  return [
    {
      id: "communication",
      title: t("enshrouded.settings.sections.communication", undefined, "Communication"),
      description: t("enshrouded.settings.sections.communicationDescription", undefined, "Voice and text chat rules.")
    },
    {
      id: "players",
      title: t("enshrouded.settings.sections.players", undefined, "Player Stats"),
      description: t(
        "enshrouded.settings.sections.playersDescription",
        undefined,
        "Health, mana, stamina, cold resistance, diving, and Shroud survivability for each player."
      )
    },
    {
      id: "survival",
      title: t("enshrouded.settings.sections.survival", undefined, "Survival Rules"),
      description: t(
        "enshrouded.settings.sections.survivalDescription",
        undefined,
        "Durability, starvation, food buffs, glider turbulence, death handling, and curse pressure."
      )
    },
    {
      id: "world",
      title: t("enshrouded.settings.sections.world", undefined, "World Pace"),
      description: t(
        "enshrouded.settings.sections.worldDescription",
        undefined,
        "Weather cadence, fishing friction, and the day-night loop that shape the feel of the world."
      )
    },
    {
      id: "progression",
      title: t("enshrouded.settings.sections.progression", undefined, "Progression"),
      description: t(
        "enshrouded.settings.sections.progressionDescription",
        undefined,
        "Gathering, crafting, rune economy, XP rates, and taming fallout."
      )
    },
    {
      id: "combat",
      title: t("enshrouded.settings.sections.combat", undefined, "Combat Pressure"),
      description: t(
        "enshrouded.settings.sections.combatDescription",
        undefined,
        "Enemy density, aggro pressure, damage, health, and boss tuning."
      )
    },
    {
      id: "admin_role",
      parentId: "access",
      title: t("enshrouded.settings.sections.admin_role", undefined, "Admin Role"),
      description: t(
        "enshrouded.settings.sections.admin_roleDescription",
        undefined,
        "The highest-trust password, permissions, and reserved slots for operators."
      )
    },
    {
      id: "friend_role",
      parentId: "access",
      title: t("enshrouded.settings.sections.friend_role", undefined, "Friend Role"),
      description: t(
        "enshrouded.settings.sections.friend_roleDescription",
        undefined,
        "Shared-building access for close collaborators without full operator powers."
      )
    },
    {
      id: "guest_role",
      parentId: "access",
      title: t("enshrouded.settings.sections.guest_role", undefined, "Guest Role"),
      description: t(
        "enshrouded.settings.sections.guest_roleDescription",
        undefined,
        "Low-trust access for people who can play and gather but should not reshape bases."
      )
    },
    {
      id: "visitor_role",
      parentId: "access",
      title: t("enshrouded.settings.sections.visitor_role", undefined, "Visitor Role"),
      description: t(
        "enshrouded.settings.sections.visitor_roleDescription",
        undefined,
        "Sightseeing or event access with the smallest edit surface."
      )
    },
    {
      id: "moderation",
      title: t("enshrouded.settings.sections.moderation", undefined, "Moderation"),
      description: t(
        "enshrouded.settings.sections.moderationDescription",
        undefined,
        "Player identities denied by the native bans array."
      )
    }
  ];
}

export const enshroudedSettingsDefinition: SettingsModuleDefinition = {
  id: "enshrouded",
  getSections: buildEnshroudedSections,
  getFieldCopy: (key, t) => buildEnshroudedFieldCopy(key, t),
  buildFieldGroups: (sectionId, fields, _locale, t) => buildEnshroudedFieldGroups(sectionId, fields, _locale, t),
  getEnumOptionLabel: (fieldKey, value, _locale, t) => getEnshroudedEnumOptionLabel(fieldKey, value, t),
  resolveFieldEditorVariant(key) {
    if (key === "server_tags") {
      return "enum-check-list";
    }

    return undefined;
  },
  getFieldValidationMessage({ field, value, t }) {
    if (field.key === "custom_user_groups_json") {
      if (typeof value !== "string" || value.trim().length === 0) {
        return undefined;
      }

      try {
        const parsed = JSON.parse(value);
        if (
          parsed &&
          (Array.isArray(parsed)
            ? parsed.every((entry) => entry && typeof entry === "object" && !Array.isArray(entry))
            : typeof parsed === "object" && !Array.isArray(parsed))
        ) {
          return undefined;
        }
      } catch {
        // Fall through to the shared validation message below.
      }

      return t(
        "enshrouded.settings.validation.customUserGroupsJson",
        undefined,
        "Use a JSON object or an array of JSON objects matching Enshrouded userGroups entries."
      );
    }

    return undefined;
  }
};
