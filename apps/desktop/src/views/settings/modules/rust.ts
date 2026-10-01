import type { TranslateFn } from "../../../i18n";
import type { SettingsModuleDefinition, SettingsModuleFieldGroup } from "../module-types";
import type { GuidedFieldCopy, GuidedSettingsField, GuidedSettingsSection } from "../settings-schema";

const STEAM64_PATTERN = /^\d{17}$/;

const RUST_SECTIONS: GuidedSettingsSection[] = [
  {
    id: "room",
    title: "Server Listing",
    description: "Public identity and browser presentation for this Rust instance."
  },
  {
    id: "world",
    title: "World & Persistence",
    description: "World generation and save cadence tied to this server identity."
  },
  {
    id: "access",
    title: "Operators & Security",
    description: "Anti-cheat, operator permissions, central bans, and player-report handling."
  },
  {
    id: "gamemode",
    title: "Game Mode Rules",
    description: "Team-size, softcore recovery, and tutorial compatibility settings."
  },
  {
    id: "creative",
    title: "Creative Mode",
    description: "Built-in Creative Mode server variables."
  },
  {
    id: "wipe",
    title: "Wipe countdown",
    description: "Set the in-game countdown and endgame event timing. This does not schedule save deletion."
  },
  {
    id: "network",
    title: "Network Services",
    description: "Rust+ pairing, connection addresses, and remote-console transport."
  },
  {
    id: "advanced",
    title: "Advanced Overrides",
    description: "Raw cfg escape hatches and launch flag overrides."
  }
];

interface RustFieldGroupSpec {
  id: string;
  title: string;
  description: string;
  layoutClass: string;
  keys: string[];
}

const RUST_GROUP_SPECS: Record<string, RustFieldGroupSpec[]> = {
  world: [
    {
      id: "world-generation",
      title: "World generation",
      description: "Procedural generation seed and map size.",
      layoutClass: "rust-terrain",
      keys: ["seed", "world_size", "deep_sea_terrain_everywhere"]
    },
    {
      id: "world-runtime",
      title: "World runtime",
      description: "Save cadence and optional procedural world JSON are applied before the map is generated.",
      layoutClass: "rust-world-runtime",
      keys: ["save_interval_seconds", "world_config_json"]
    }
  ],
  access: [
    {
      id: "security",
      title: "Anti-cheat protection",
      description: "Anti-cheat protection for connected players.",
      layoutClass: "rust-security",
      keys: ["secure"]
    },
    {
      id: "operators",
      title: "Operators and bans",
      description: "Owners, moderators, skip queue entries, and bans materialize to users.cfg and bans.cfg.",
      layoutClass: "rust-access-lists",
      keys: ["owner_entries", "moderator_entries", "skip_queue_entries", "banned_entries"]
    },
    {
      id: "central-bans",
      title: "Central banning",
      description: "External ban backend endpoint with failure mode and timeout control.",
      layoutClass: "rust-central-bans",
      keys: ["bans_server_endpoint", "bans_server_failure_mode", "bans_server_timeout_seconds"]
    },
    {
      id: "player-reports",
      title: "Player reports",
      description: "Console summaries and optional HTTP forwarding for F7 report payloads.",
      layoutClass: "rust-player-reports",
      keys: ["reports_print_to_console", "reports_server_endpoint", "reports_server_endpoint_key"]
    }
  ],
  gamemode: [
    {
      id: "team-and-softcore",
      title: "Mode and team rules",
      description: "Game mode, PvE, team size, softcore recovery and tutorial rules.",
      layoutClass: "rust-team-softcore",
      keys: [
        "server_gamemode",
        "pve",
        "max_team_size",
        "softcore_reclaim_fraction_main",
        "softcore_reclaim_fraction_belt",
        "softcore_reclaim_fraction_wear",
        "tutorial_enabled"
      ]
    },
    {
      id: "group-upkeep",
      title: "Building upkeep",
      description: "Apply tiered upkeep increases for the number of players using a base.",
      layoutClass: "rust-group-upkeep",
      keys: [
        "upkeep_group_scaling",
        "upkeep_group_tier_0_playercount",
        "upkeep_group_tier_0_increase",
        "upkeep_group_tier_1_playercount",
        "upkeep_group_tier_1_increase",
        "upkeep_group_tier_2_increase",
        "upkeep_group_max_multiplier"
      ]
    },
    {
      id: "upkeep-membership",
      title: "Group membership counting",
      description: "Count cupboard and code-lock users, including recent authorization history.",
      layoutClass: "rust-upkeep-membership",
      keys: ["upkeep_group_window_hours", "upkeep_group_count_locks", "upkeep_lock_min_users", "upkeep_group_history_max"]
    }
  ],
  creative: [
    {
      id: "creative-server",
      title: "Creative server variables",
      description: "Enable Creative Mode capabilities for all players or specific building systems.",
      layoutClass: "rust-creative",
      keys: [
        "creative_all_users",
        "creative_always_on_enabled",
        "creative_bypass_hold_to_place_duration",
        "creative_free_build",
        "creative_free_placement",
        "creative_free_repair",
        "creative_unlimited_io"
      ]
    }
  ],
  wipe: [
    {
      id: "wipe-schedule",
      title: "Wipe countdown",
      description: "Match the countdown to your server's planned wipe using the weekday, time zone, or an exact date.",
      layoutClass: "rust-wipe-schedule",
      keys: [
        "wipe_day_of_week",
        "wipe_hour_of_day",
        "wipe_timezone",
        "wipe_cron_override",
        "wipe_unix_timestamp_override"
      ]
    }
  ],
  network: [
    {
      id: "connections",
      title: "Favorites endpoint",
      description: "The endpoint saved in player favorites.",
      layoutClass: "rust-connections",
      keys: ["favorites_endpoint"]
    },
    {
      id: "rust-plus",
      title: "Rust+",
      description: "Rust companion websocket endpoint and optional IP overrides.",
      layoutClass: "rust-plus",
      keys: ["app_port", "app_public_ip", "app_listen_ip"]
    }
  ],
  advanced: [
    {
      id: "npc-navigation",
      title: "NPC navigation",
      description: "Choose the navigation implementation when the server starts.",
      layoutClass: "rust-npc-navigation",
      keys: ["use_new_navmesh"]
    },
    {
      id: "overrides",
      title: "Raw overrides",
      description: "Escape hatches for unmanaged cfg lines and launch flags.",
      layoutClass: "rust-advanced-overrides",
      keys: ["server_cfg_extra", "users_cfg_extra", "bans_cfg_extra", "custom_launch_flags"]
    }
  ]
};

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

function buildRustSections(t: TranslateFn): GuidedSettingsSection[] {
  return RUST_SECTIONS.map((section) => ({
    ...section,
    title: t(`rust.settings.sections.${section.id}`, undefined, section.title),
    description: t(
      `rust.settings.sections.${section.id}Description`,
      undefined,
      section.description ?? ""
    )
  }));
}

function buildRustFieldCopy(key: string, t: TranslateFn): GuidedFieldCopy | undefined {
  const baseKey = `settings.schema.rust.${key}`;
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

function getRustEnumOptionLabel(fieldKey: string, value: unknown, t: TranslateFn): string | undefined {
  if (typeof value !== "string" && typeof value !== "number") {
    return undefined;
  }

  return readCatalogText(t, `settings.schema.rust.${fieldKey}.option.${buildSchemaEnumOptionKey(value)}`);
}

function buildRustFieldGroups(
  sectionId: string,
  fields: GuidedSettingsField[],
  t: TranslateFn
): SettingsModuleFieldGroup[] {
  const fieldsByKey = new Map(fields.map((field) => [field.key, field]));
  const claimedKeys = new Set<string>();
  const groups: SettingsModuleFieldGroup[] = [];

  for (const spec of RUST_GROUP_SPECS[sectionId] ?? []) {
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
      title: t(`rust.settings.groups.${spec.id}.title`, undefined, spec.title),
      description: t(`rust.settings.groups.${spec.id}.description`, undefined, spec.description),
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

function readText(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}

function readNumber(value: unknown): number | null {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function parseConfigLines(value: unknown): string[] {
  if (typeof value !== "string") {
    return [];
  }

  return value
    .replace(/\r\n/g, "\n")
    .replace(/\r/g, "\n")
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0 && !line.startsWith("//") && !line.startsWith("#"));
}

function findInvalidSteamIdLines(value: unknown): string[] {
  return parseConfigLines(value).filter((line) => {
    const [steamId] = line.split(/[|,\s]+/, 1);
    return !STEAM64_PATTERN.test(steamId ?? "");
  });
}

function buildSteamIdListValidationMessage(value: unknown, t: TranslateFn): string | undefined {
  const invalidLines = findInvalidSteamIdLines(value);
  if (invalidLines.length === 0) {
    return undefined;
  }

  const preview = invalidLines.slice(0, 3).join(", ");
  return t(
    "rust.settings.validation.steamIdListInvalid",
    { preview },
    `Only Steam64 IDs are valid at the start of each line. Fix: ${preview}`
  );
}

export const rustSettingsDefinition: SettingsModuleDefinition = {
  id: "rust",
  getSections: buildRustSections,
  buildFieldGroups: (sectionId, fields, _locale, t) =>
    buildRustFieldGroups(sectionId, fields, t),
  getFieldCopy: (key, t) => buildRustFieldCopy(key, t),
  getEnumOptionLabel: (fieldKey, value, _locale, t) => getRustEnumOptionLabel(fieldKey, value, t),
  resolveFieldEditorVariant(key) {
    if (key === "owner_entries" || key === "moderator_entries" || key === "skip_queue_entries" || key === "banned_entries") {
      return "string-list";
    }

    return undefined;
  },
  getFieldValidationMessage({ field, value, settings, t }) {
    if (field.key === "server_name" && readText(value).length === 0) {
      return t(
        "rust.settings.validation.serverNameRequired",
        undefined,
        "Give this Rust server a browser name before launch."
      );
    }

    if (field.key === "rcon_password") {
      const password = readText(value);
      if (password.length === 0 || password === "change-me-rcon") {
        return t(
          "rust.settings.validation.rconPassword",
          undefined,
          "Replace the default RCON password before daily use."
        );
      }
    }

    if (
      field.key === "owner_entries" ||
      field.key === "moderator_entries" ||
      field.key === "skip_queue_entries" ||
      field.key === "banned_entries"
    ) {
      return buildSteamIdListValidationMessage(value, t);
    }

    if (field.key === "app_port") {
      const appPort = readNumber(value);
      if (appPort !== null && appPort !== -1 && appPort >= 0 && appPort < 10000) {
        return t(
          "rust.settings.validation.appPortRange",
          undefined,
          "Rust+ port should be -1 or at least 10000."
        );
      }
    }

    if (field.key === "level_url" && readText(value).length > 0 && readText(settings.level).length === 0) {
      return t(
        "rust.settings.validation.levelRequiredWithCustomMap",
        undefined,
        "Keep a Rust level selected when using a custom map URL."
      );
    }

    return undefined;
  }
};
