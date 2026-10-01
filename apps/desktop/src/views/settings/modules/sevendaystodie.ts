import type { TranslateFn } from "../../../i18n";
import { SevenDaysServerAdminPanel } from "../SevenDaysServerAdminPanel";
import type { SettingsModuleDefinition, SettingsModuleFieldGroup } from "../module-types";
import type { GuidedFieldCopy, GuidedSettingsField, GuidedSettingsSection } from "../settings-schema";

const SEVEN_DAYS_SECTIONS: GuidedSettingsSection[] = [
  {
    id: "world",
    title: "World Rules",
    description: "World identity, loot and death posture, day pacing, and the survival baseline for this host."
  },
  {
    id: "threat",
    title: "Zombie Pressure",
    description: "Spawn budgets, movement presets, blood-moon cadence, and the performance ceiling behind them."
  },
  {
    id: "claims",
    title: "Claims & Base Safety",
    description: "Land-claim protection and offline raid durability."
  }
];

interface SevenDaysFieldGroupSpec {
  id: string;
  title: string;
  description: string;
  layoutClass: string;
  keys: string[];
}

function buildSevenDaysSections(t: TranslateFn): GuidedSettingsSection[] {
  return SEVEN_DAYS_SECTIONS.map((section) => ({
    ...section,
    title: t(`settings.7dtd.sections.${section.id}`, undefined, section.title),
    description: t(`settings.7dtd.sections.${section.id}Description`, undefined, section.description ?? "")
  }));
}

function readCatalogText(t: TranslateFn, key: string): string | undefined {
  const value = t(key, undefined, "");
  const trimmed = value.trim();
  return trimmed ? trimmed : undefined;
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

function buildSevenDaysFieldCopy(key: string, t: TranslateFn): GuidedFieldCopy | undefined {
  const baseKey = `settings.schema.sevendaystodie.${key}`;
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

function getSevenDaysEnumOptionLabel(fieldKey: string, value: unknown, t: TranslateFn): string | undefined {
  return readCatalogText(t, `settings.schema.sevendaystodie.${fieldKey}.option.${buildSchemaEnumOptionKey(value)}`);
}

function readString(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function readNumber(value: unknown): number | null {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function buildGroupSpecMap(): Record<string, SevenDaysFieldGroupSpec[]> {
  return {
    world: [
      {
        id: "world-anchor",
        title: "World generation",
        description: "Choose the game mode, seed, and generated-world size.",
        layoutClass: "world-anchor",
        keys: ["game_mode", "world_seed", "world_size"]
      },
      {
        id: "sandbox",
        title: "Sandbox and player rules",
        description: "Version 3.0 groups difficulty, pacing, loot, zombie, and blood-moon options into the in-game sandbox code.",
        layoutClass: "world-difficulty",
        keys: [
          "sandbox_code",
          "build_create",
          "camera_restriction_mode",
          "player_killing_mode",
          "player_safe_zone_level",
          "player_safe_zone_hours",
          "party_shared_kill_range"
        ]
      },
      {
        id: "death-and-blocks",
        title: "Bedroll and first spawn",
        description: "Control bedroll protection, expiry, and where a first-time player may spawn near a friend.",
        layoutClass: "world-death-and-blocks",
        keys: [
          "bedroll_dead_zone_size",
          "bedroll_expiry_time",
          "allow_spawn_near_friend"
        ]
      },
      {
        id: "twitch",
        title: "Twitch integration",
        description: "Control whether Twitch interactions can run during blood moon.",
        layoutClass: "services-twitch",
        keys: ["twitch_blood_moon_allowed"]
      }
    ],
    threat: [
      {
        id: "spawn-budget",
        title: "Spawn budget",
        description: "Map-wide zombie and animal caps set the main CPU budget before blood moon even starts.",
        layoutClass: "threat-spawn-budget",
        keys: ["max_spawned_zombies", "max_spawned_animals"]
      }
    ],
    claims: [
      {
        id: "claim-core",
        title: "Claim footprint",
        description: "Claim count, claim size, spacing, and expiry time define the layout envelope for player bases.",
        layoutClass: "claims-core",
        keys: ["land_claim_count", "land_claim_size", "land_claim_dead_zone", "land_claim_expiry_time", "land_claim_decay_mode"]
      },
      {
        id: "offline-protection",
        title: "Offline durability",
        description: "Online and offline durability plus the handoff delay determine how exposed bases become after logout.",
        layoutClass: "claims-offline-protection",
        keys: [
          "land_claim_online_durability_modifier",
          "land_claim_offline_durability_modifier",
          "land_claim_offline_delay"
        ]
      },
    ],
    access: [
      {
        id: "join-policy", title: "Admission policy", description: "Anti-cheat and platform enforcement rules.",
        layoutClass: "access-join-policy", keys: ["ignore_eos_sanctions", "eac_enabled", "twitch_server_permission"]
      },
      {
        id: "seat-policy",
        title: "Seat policy",
        description: "Reserved slots and admin overflow seats decide how the host handles crowded public hours.",
        layoutClass: "access-seat-policy",
        keys: ["reserved_slots", "reserved_slots_permission", "admin_slots", "admin_slots_permission"]
      },
      {
        id: "player-identities",
        title: "Player identity",
        description: "Profile persistence controls whether players can freely swap character profiles when they reconnect.",
        layoutClass: "access-player-identities",
        keys: ["persistent_player_profiles"]
      },
    ],
    network: [
      {
        id: "connectivity", title: "Connectivity", description: "Client protocols, crossplay, and world transfer bandwidth.",
        layoutClass: "network-connectivity",
        keys: ["server_disabled_network_protocols", "server_allow_crossplay", "server_max_world_transfer_speed_kibs"]
      },
      {
        id: "remote-control",
        title: "Remote control",
        description: "The web panel and telnet are the host's direct remote-control surfaces.",
        layoutClass: "services-remote-control",
        keys: [
          "web_dashboard_enabled",
          "web_dashboard_url",
          "enable_map_rendering",
          "telnet_enabled",
          "telnet_password",
          "telnet_failed_login_limit",
          "telnet_failed_logins_blocktime"
        ]
      }
    ],
    runtime: [
      {
        id: "host-observability",
        title: "Host observability",
        description: "Console window visibility and command logging.",
        layoutClass: "services-observability",
        keys: ["terminal_window_enabled", "hide_command_execution_log"]
      },
      {
        id: "map-budget",
        title: "Map data budget",
        description: "Explored-map growth is a real save-data constraint on long-running worlds, especially with many regular players.",
        layoutClass: "world-map-budget",
        keys: ["max_uncovered_map_chunks_per_player", "max_chunk_age", "save_data_limit"]
      },
      {
        id: "dynamic-mesh",
        title: "Dynamic mesh",
        description: "Treat this cluster like a base-scale governor for builder-heavy servers.",
        layoutClass: "claims-dynamic-mesh",
        keys: [
          "dynamic_mesh_enabled",
          "dynamic_mesh_land_claim_only",
          "dynamic_mesh_land_claim_buffer",
          "dynamic_mesh_max_item_cache"
        ]
      },
      {
        id: "render-budget", title: "Rendering limits", description: "View distance and queued mesh layers.",
        layoutClass: "runtime-render-budget", keys: ["server_max_allowed_view_distance", "max_queued_mesh_layers"]
      }
    ]
  };
}

function buildSevenDaysFieldGroups(
  sectionId: string,
  fields: GuidedSettingsField[],
  locale: string,
  t: TranslateFn
): SettingsModuleFieldGroup[] {
  const fieldsByKey = new Map(fields.map((field) => [field.key, field]));
  const claimedKeys = new Set<string>();
  const specs = buildGroupSpecMap()[sectionId] ?? [];
  void locale;

  const groups = specs
    .map((spec) => {
      const groupFields = spec.keys
        .map((key) => fieldsByKey.get(key))
        .filter((field): field is GuidedSettingsField => Boolean(field));

      for (const field of groupFields) {
        claimedKeys.add(field.key);
      }

      return {
        id: spec.id,
        title: t(`settings.7dtd.groups.${spec.id}.title`, undefined, spec.title),
        description: t(`settings.7dtd.groups.${spec.id}.description`, undefined, spec.description),
        layoutClass: spec.layoutClass,
        fields: groupFields
      };
    })
    .filter((group) => group.fields.length > 0);

  const remainingFields = fields.filter((field) => !claimedKeys.has(field.key));
  if (remainingFields.length > 0) {
    groups.push({
      id: `${sectionId}-additional`,
      title: t("settings.7dtd.groups.additional.title", undefined, "Additional fields"),
      description: t(
        "settings.7dtd.groups.additional.description",
        undefined,
        "These fields still belong to this section but did not fit the main working groups above."
      ),
      layoutClass: `${sectionId}-additional`,
      fields: remainingFields
    });
  }

  return groups.length > 0
    ? groups
    : [{ id: `${sectionId}-default`, layoutClass: `${sectionId}-default`, fields }];
}

export const sevenDaysToDieSettingsDefinition: SettingsModuleDefinition = {
  id: "sevendaystodie",
  fieldPresentationOverrides: {
    command_permissions: {
      state: "specialized",
      owner: "configuration",
      sectionId: "access",
      rendererId: "seven-days-command-permissions"
    }
  },
  specializedRenderers: {
    "seven-days-command-permissions": {
      kind: "module-addon",
      sectionId: "access",
      fieldKey: "command_permissions",
      Renderer: SevenDaysServerAdminPanel
    }
  },
  getSections: buildSevenDaysSections,
  buildFieldGroups: buildSevenDaysFieldGroups,
  getFieldCopy: (key, t) => buildSevenDaysFieldCopy(key, t),
  getEnumOptionLabel: (fieldKey, value, _locale, t) => getSevenDaysEnumOptionLabel(fieldKey, value, t),
  getFieldValidationMessage({ field, value, settings, t }) {
    if (field.key === "world_size") {
      const worldSize = readNumber(value);
      if (worldSize !== null && (worldSize < 2048 || worldSize > 16384 || worldSize % 2048 !== 0)) {
        return t(
          "settings.7dtd.validation.worldSize",
          undefined,
          "RWG world size must stay between 2048 and 16384 and use a multiple of 2048."
        );
      }
    }

    if (field.key === "world_seed") {
      const worldType = readString(settings.game_world).trim().toUpperCase();
      if (worldType === "RWG" && readString(value).trim().length === 0) {
        return t(
          "settings.7dtd.validation.worldSeedRequired",
          undefined,
          "World seed is required when the world type is RWG."
        );
      }
    }

    if (field.key === "land_claim_size") {
      const size = readNumber(value);
      if (size !== null && size % 2 === 0) {
        return t(
          "settings.7dtd.validation.landClaimSize",
          undefined,
          "Use an odd land-claim size so the protected area stays centered on the claim block."
        );
      }
    }

    if (field.key === "web_dashboard_url") {
      const dashboardUrl = readString(value).trim();
      if (dashboardUrl.length > 0 && !/^https?:\/\//i.test(dashboardUrl)) {
        return t(
          "settings.7dtd.validation.webDashboardUrl",
          undefined,
          "Web dashboard URL must be a full http:// or https:// URL when you override it."
        );
      }
    }

    if (field.key === "reserved_slots") {
      const reservedSlots = readNumber(value);
      const maxPlayers = readNumber(settings.max_players);
      if (reservedSlots !== null && maxPlayers !== null && reservedSlots > maxPlayers) {
        return t(
          "settings.7dtd.validation.reservedSlots",
          undefined,
          "Reserved slots cannot exceed the total player cap."
        );
      }
    }

    if (field.key === "admin_slots") {
      const adminSlots = readNumber(value);
      const maxPlayers = readNumber(settings.max_players);
      if (adminSlots !== null && maxPlayers !== null && adminSlots > maxPlayers) {
        return t(
          "settings.7dtd.validation.adminSlots",
          undefined,
          "Admin overflow slots cannot exceed the total player cap."
        );
      }
    }

    return undefined;
  }
};
