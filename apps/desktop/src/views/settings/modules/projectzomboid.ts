import { ProjectZomboidPolicyReview } from "../ProjectZomboidPolicyReview";
import type { TranslateFn } from "../../../i18n";
import type { SettingsModuleDefinition, SettingsModuleFieldGroup } from "../module-types";
import type { GuidedFieldCopy, GuidedSettingsField, GuidedSettingsSection, SettingsObject } from "../settings-schema";

const PROJECT_ZOMBOID_SECTIONS: GuidedSettingsSection[] = [
  {
    id: "join",
    parentId: "access",
    title: "Join & Trust",
    description: "Registration, login queues, Steam trust, and client integrity."
  },
  {
    id: "world",
    title: "World Rules",
    description: "Spawn, PvP, sleep, respawn, fire, travel, and simulation rules."
  },
  {
    id: "safehouses",
    title: "Safehouses",
    description: "Safehouse claiming, access, looting, fire, respawn, and removal policy."
  },
  {
    id: "loot",
    title: "Loot & Cleanup",
    description: "Loot respawn, cleanup, corpse, blood, trash, and ground-item removal policy."
  },
  {
    id: "services",
    title: "Host Services",
    description: "Java memory, process startup, simulation, and update tuning."
  },
  {
    id: "mods",
    title: "Workshop & Maps",
    description: "Map load order, Workshop downloads, and enabled Project Zomboid Mod IDs."
  },
  {
    id: "moderation",
    title: "Moderation & Logs",
    description: "Factions, Discord bridge, staff radio, logs, trading, and save-validation controls."
  },
  {
    id: "anticheat",
    title: "Anti-cheat",
    description: "Actions taken when each category of anti-cheat check fails."
  },
  {
    id: "raw",
    title: "Native Lua Files",
    description: "Raw SandboxVars, spawnpoints, and spawnregions Lua files for open-ended native configuration."
  }
];

interface ProjectZomboidFieldGroupSpec {
  id: string;
  title: string;
  description: string;
  layoutClass: string;
  keys: string[];
}

const PROJECT_ZOMBOID_GROUP_SPECS: Record<string, ProjectZomboidFieldGroupSpec[]> = {
  access: [
    {
      id: "operator-credentials",
      title: "Operator credentials",
      description: "Administrator account used when starting the server.",
      layoutClass: "access-operator-credentials",
      keys: ["admin_username", "admin_password"]
    }
  ],
  network: [
    {
      id: "voice",
      title: "Voice chat",
      description: "Voice transport and proximity range.",
      layoutClass: "services-voice",
      keys: ["voice_enable", "voice_min_distance", "voice_max_distance", "voice_3d"]
    },
    {
      id: "network-services",
      title: "Network services",
      description: "Advertised endpoint, RCON, ping checks, map transport, and UPnP.",
      layoutClass: "network-services",
      keys: ["announced_ip", "rcon_password", "ping_limit", "upnp", "login_queue_connect_timeout", "max_packets_per_second"]
    }
  ],
  join: [
    {
      id: "login-queue",
      title: "Login-Queue",
      description: "Dedicated Project Zomboid login-queue controls.",
      layoutClass: "services-login-queue",
      keys: ["login_queue_enabled"]
    },
    {
      id: "join-gate",
      title: "Join-Gate",
      description: "Dedicated Project Zomboid join-gate controls.",
      layoutClass: "join-join-gate",
      keys: ["open_server", "max_accounts_per_user", "allow_coop", "drop_off_whitelist_after_death", "allow_non_ascii_username"]
    },
    {
      id: "client-integrity",
      title: "Client-Integrity",
      description: "Dedicated Project Zomboid client-integrity controls.",
      layoutClass: "join-client-integrity",
      keys: ["do_lua_checksum", "deny_login_on_overloaded_server", "steam_vac", ]
    }
  ],
  world: [
    {
      id: "identity-visibility",
      title: "Identity-Visibility",
      description: "Dedicated Project Zomboid identity-visibility controls.",
      layoutClass: "join-identity-visibility",
      keys: ["display_user_name", "show_first_and_last_name", "steam_scoreboard", "mouse_over_to_see_display_name", "hide_players_behind_you", "username_disguises", "hide_disguised_user_name", "sneak_mode_hide_from_other_players", "disable_scoreboard", "hide_admins_in_player_list"]
    },
    {
      id: "spawn-and-conflict",
      title: "Spawn-And-Conflict",
      description: "Dedicated Project Zomboid spawn-and-conflict controls.",
      layoutClass: "world-spawn-and-conflict",
      keys: ["spawn_point", "spawn_items", "pvp", "safety_system", "show_safety", "safety_toggle_timer", "safety_cooldown_timer", "safety_disconnect_delay"]
    },
    {
      id: "pvp-damage",
      title: "PvP-Damage",
      description: "Dedicated Project Zomboid pvp-damage controls.",
      layoutClass: "world-pvp-damage",
      keys: ["pvp_melee_damage_modifier", "pvp_firearm_damage_modifier", "pvp_melee_while_hit_reaction", "player_bump_player", "knocked_down_allowed", "use_physics_hit_reaction"]
    },
    {
      id: "sleep-and-respawn",
      title: "Sleep-And-Respawn",
      description: "Dedicated Project Zomboid sleep-and-respawn controls.",
      layoutClass: "world-sleep-and-respawn",
      keys: ["sleep_allowed", "sleep_needed", "player_respawn_with_self", "player_respawn_with_other", "fast_forward_multiplier", "ultra_speed_doesnot_affect_to_animals"]
    },
    {
      id: "world-rules",
      title: "World-Rules",
      description: "Dedicated Project Zomboid world-rules controls.",
      layoutClass: "world-world-rules",
      keys: ["no_fire", "announce_death", "save_world_every_minutes", "car_engine_attraction_modifier", "map_remote_player_visibility", "speed_limit", "announce_animal_death", "disable_vehicle_towing", "disable_trailer_towing", "disable_burnt_towing", "show_coordinates", "world_seed"]
    }
  ],
  safehouses: [
    { id: "war", title: "Safehouse wars", description: "Safehouse wars rules.", layoutClass: "safehouses-war", keys: ["war", "war_start_delay", "war_duration", "war_safehouse_hit_points"] },
    {
      id: "safehouse-access",
      title: "Safehouse-Access",
      description: "Dedicated Project Zomboid safehouse-access controls.",
      layoutClass: "safehouses-safehouse-access",
      keys: ["player_safehouse", "admin_safehouse", "safehouse_allow_trepass", "safehouse_allow_non_residential", "safehouse_disable_disguises", "disable_safehouse_when_owner_connected"]
    },
    {
      id: "safehouse-rules",
      title: "Safehouse-Rules",
      description: "Dedicated Project Zomboid safehouse-rules controls.",
      layoutClass: "safehouses-safehouse-rules",
      keys: ["safehouse_allow_fire", "safehouse_allow_loot", "safehouse_allow_respawn", "safehouse_day_survived_to_claim", "safehouse_removal_time", "allow_destruction_by_sledgehammer", "sledgehammer_only_in_safehouse", "max_safezone_size"]
    }
  ],
  loot: [
    {
      id: "loot-respawn",
      title: "Loot-Respawn",
      description: "Dedicated Project Zomboid loot-respawn controls.",
      layoutClass: "loot-loot-respawn",
      keys: ["item_numbers_limit_per_container", "safehouse_prevents_loot_respawn"]
    },
    {
      id: "cleanup",
      title: "Cleanup",
      description: "Dedicated Project Zomboid cleanup controls.",
      layoutClass: "loot-cleanup",
      keys: ["blood_splat_lifespan_days", "remove_player_corpses_on_corpse_removal", "trash_delete_all", ]
    }
  ],
  services: [
    {
      id: "host-runtime",
      title: "Host runtime",
      description: "Java memory, empty-server simulation, and physics timing.",
      layoutClass: "services-host-runtime",
      keys: ["memory_gb", "pause_empty", "multiplayer_statistics_period"]
    },
    {
      id: "backups",
      title: "Backups",
      description: "Dedicated Project Zomboid backups controls.",
      layoutClass: "services-backups",
      keys: ["backups_count", "backups_on_start", "backups_on_version_change", "backups_period"]
    },
    {
      id: "zombie-network",
      title: "Zombie-Network",
      description: "Dedicated Project Zomboid zombie-network controls.",
      layoutClass: "services-zombie-network",
      keys: ["switch_zombies_ownership_each_update"]
    }
  ],
  mods: [
    {
      id: "workshop-downloads",
      title: "Workshop-Downloads",
      description: "Dedicated Project Zomboid workshop-downloads controls.",
      layoutClass: "mods-workshop-downloads",
      keys: ["workshop_items"]
    },
    {
      id: "enabled-mods",
      title: "Enabled-Mods",
      description: "Dedicated Project Zomboid enabled-mods controls.",
      layoutClass: "mods-enabled-mods",
      keys: ["mods"]
    }
  ],
  moderation: [
    { id: "word-filter", title: "Chat word filter", description: "Chat word filter rules.", layoutClass: "moderation-word-filter", keys: ["bad_word_list_file", "good_word_list_file", "bad_word_policy", "bad_word_replacement"] },
    {
      id: "chat",
      title: "Chat",
      description: "Global chat and enabled chat channels.",
      layoutClass: "moderation-chat",
      keys: ["global_chat", "chat_streams", "chat_message_character_limit", "chat_message_slow_mode_time"]
    },
    {
      id: "factions",
      title: "Factions",
      description: "Dedicated Project Zomboid factions controls.",
      layoutClass: "moderation-factions",
      keys: ["faction", "faction_day_survived_to_create", "faction_players_required_for_tag", ]
    },
    {
      id: "discord",
      title: "Discord",
      description: "Dedicated Project Zomboid discord controls.",
      layoutClass: "moderation-discord",
      keys: ["discord_enable", "discord_token", "discord_chat_channel", "discord_log_channel", "discord_command_channel", "webhook_address"]
    },
    {
      id: "staff-radio",
      title: "Staff-Radio",
      description: "Dedicated Project Zomboid staff-radio controls.",
      layoutClass: "moderation-staff-radio",
      keys: ["disable_radio_staff", "disable_radio_admin", "disable_radio_gm", "disable_radio_overseer", "disable_radio_moderator", "disable_radio_invisible"]
    },
    {
      id: "logging-filters",
      title: "Logging-Filters",
      description: "Dedicated Project Zomboid logging-filters controls.",
      layoutClass: "moderation-logging-filters",
      keys: ["client_command_filter", "client_action_logs", "perk_logs", "ban_kick_global_sound", "pvp_log_tool_chat", "pvp_log_tool_file"]
    }
  ],
  anticheat: [
    {
      id: "anti-cheat-types",
      title: "Anti-Cheat-Types",
      description: "Dedicated Project Zomboid anti-cheat-types controls.",
      layoutClass: "anticheat-anti-cheat-types",
      keys: ["anti_cheat_safety", "anti_cheat_speed", "anti_cheat_no_clip", "anti_cheat_hit", "anti_cheat_packet_exception", "anti_cheat_permission", "anti_cheat_xp", "anti_cheat_safe_house", "anti_cheat_player", "anti_cheat_checksum"]
    },
  ],
  raw: [
    {
      id: "native-lua-files",
      title: "Native-Lua-Files",
      description: "Dedicated Project Zomboid native-lua-files controls.",
      layoutClass: "raw-native-lua-files",
      keys: ["sandbox_vars_lua", "spawnpoints_lua", "spawnregions_lua"]
    }
  ]
};

function readCatalogText(t: TranslateFn, key: string): string | undefined {
  const value = t(key, undefined, "");
  return value.trim() ? value : undefined;
}

function buildProjectZomboidSections(t: TranslateFn): GuidedSettingsSection[] {
  return PROJECT_ZOMBOID_SECTIONS.map((section) => ({
    ...section,
    title: t(`projectzomboid.settings.sections.${section.id}`, undefined, section.title),
    description: t(
      `projectzomboid.settings.sections.${section.id}Description`,
      undefined,
      section.description ?? ""
    )
  }));
}

function buildProjectZomboidFieldCopy(key: string, t: TranslateFn): GuidedFieldCopy | undefined {
  const baseKey = `settings.schema.projectzomboid.${key}`;
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

function parseDelimitedList(value: unknown): string[] {
  if (typeof value !== "string" || value.trim().length === 0) {
    return [];
  }

  const seen = new Set<string>();
  const entries: string[] = [];

  for (const entry of value
    .replace(/\r\n/g, "\n")
    .replace(/\r/g, "\n")
    .split(/[\n;]+/)
    .map((item) => item.trim())
    .filter(Boolean)) {
    const normalizedKey = entry.toLocaleLowerCase();
    if (!seen.has(normalizedKey)) {
      seen.add(normalizedKey);
      entries.push(entry);
    }
  }

  return entries;
}

function readNumericSetting(settings: SettingsObject, key: string): number | null {
  const value = settings[key];
  if (typeof value === "number" && Number.isFinite(value)) {
    return value;
  }
  if (typeof value === "string" && value.trim().length > 0) {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : null;
  }
  return null;
}

function buildProjectZomboidFieldGroups(
  sectionId: string,
  fields: GuidedSettingsField[],
  t: TranslateFn
): SettingsModuleFieldGroup[] {
  const fieldsByKey = new Map(fields.map((field) => [field.key, field]));
  const claimedKeys = new Set<string>();
  const specs = PROJECT_ZOMBOID_GROUP_SPECS[sectionId] ?? [];

  const groups: SettingsModuleFieldGroup[] = [];

  for (const spec of specs) {
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
      title: t(`projectzomboid.settings.groups.${spec.id}.title`, undefined, spec.title),
      description: t(`projectzomboid.settings.groups.${spec.id}.description`, undefined, spec.description),
      layoutClass: spec.layoutClass,
      fields: groupFields
    });
  }

  const remainingFields = fields.filter((field) => !claimedKeys.has(field.key));
  if (remainingFields.length > 0) {
    groups.push({
      id: "additional",
      layoutClass: `${sectionId}-additional`,
      fields: remainingFields
    });
  }

  return groups.length > 0 ? groups : [{ id: "default", fields }];
}

function getPasswordWarning(value: unknown, placeholder: string, t: TranslateFn): string | undefined {
  if (typeof value !== "string") {
    return undefined;
  }

  const trimmed = value.trim();
  if (trimmed.length === 0) {
    return t("projectzomboid.settings.validation.passwordEmpty", undefined, "This password is currently empty.");
  }

  if (trimmed === placeholder) {
    return t(
      "projectzomboid.settings.validation.passwordPlaceholder",
      undefined,
      "This is still the default placeholder password. Change it before you open the server."
    );
  }

  return undefined;
}

export const projectZomboidSettingsDefinition: SettingsModuleDefinition = {
  id: "projectzomboid",
  getSections: buildProjectZomboidSections,
  fieldPresentationOverrides: { webhook_address: { behavior: "secret" } },
  specializedRenderers: {
    "projectzomboid-policy-review": { kind: "module-addon", sectionId: "anticheat", placement: "before-fields", Renderer: ProjectZomboidPolicyReview }
  },
  getEnumOptionLabel: (key, value, locale) => {
    if (!key.startsWith("anti_cheat_") && key !== "bad_word_policy") return undefined;
    const labels = locale.startsWith("zh") ? ["封禁", "踢出", "记录", "禁用"] : ["Ban", "Kick", "Log", "Disabled"];
    return typeof value === "number" ? labels[value - 1] : undefined;
  },
  buildFieldGroups: (sectionId, fields, _locale, t) =>
    buildProjectZomboidFieldGroups(sectionId, fields, t),
  getFieldCopy: (key, t) => buildProjectZomboidFieldCopy(key, t),
  resolveFieldEditorVariant: (key) => {
    if (key === "workshop_items") {
      return "workshop-id-list";
    }

    if (key === "map_name" || key === "mods") {
      return "string-list";
    }

    return undefined;
  },
  getFieldValidationMessage({ field, value, settings, t }) {
    switch (field.key) {
      case "map_name": {
        if (parseDelimitedList(value).length > 0) {
          return undefined;
        }
        return t(
          "projectzomboid.settings.validation.mapNameRequired",
          undefined,
          "Keep at least one map entry. The vanilla map still counts; the default is Muldraugh, KY."
        );
      }
      case "admin_password":
        return getPasswordWarning(value, "change-me-admin", t);
      case "rcon_password":
        return getPasswordWarning(value, "change-me-rcon", t);
      case "voice_min_distance":
      case "voice_max_distance": {
        const minDistance = readNumericSetting(settings, "voice_min_distance");
        const maxDistance = readNumericSetting(settings, "voice_max_distance");
        if (minDistance == null || maxDistance == null || maxDistance > minDistance) {
          return undefined;
        }
        return t(
          "projectzomboid.settings.validation.voiceDistance",
          undefined,
          "Voice max distance must be greater than the near voice distance."
        );
      }
      default:
        return undefined;
    }
  }
};
