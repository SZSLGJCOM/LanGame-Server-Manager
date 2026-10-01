import type { TranslateFn } from "../../../i18n";
import type { SettingsModuleDefinition, SettingsModuleFieldGroup } from "../module-types";
import type { GuidedFieldCopy, GuidedSettingsField, GuidedSettingsSection, SettingsObject } from "../settings-schema";

const MINECRAFT_SECTIONS: GuidedSettingsSection[] = [
  {
    id: "access",
    title: "Join & Trust",
    description: "EULA, authentication, code of conduct, whitelist, operators, and bans all live in the access-control lane."
  },
  {
    id: "world",
    title: "World Rules",
    description: "World generation, gameplay rules, data packs, and resource packs."
  },
  {
    id: "network",
    title: "Remote Services",
    description: "Query, RCON, status heartbeat, and the secured management endpoint."
  },
  {
    id: "advanced",
    title: "Host Performance",
    description: "Java memory, chunk processing, storage, and diagnostic settings."
  }
];

interface MinecraftFieldGroupSpec {
  id: string;
  title: string;
  description: string;
  layoutClass: string;
  keys: string[];
}

const MINECRAFT_GROUP_SPECS: Record<string, MinecraftFieldGroupSpec[]> = {
  access: [
    {
      id: "eula-auth",
      title: "EULA and account authentication",
      description: "Vanilla Minecraft exits until the EULA is accepted. Public servers should also keep online authentication enabled.",
      layoutClass: "minecraft-eula-auth",
      keys: [
        "eula_accepted",
        "enable_code_of_conduct",
        "online_mode",
        "enforce_secure_profile",
        "prevent_proxy_connections",
        "op_permission_level",
        "function_permission_level",
        "broadcast_console_to_ops",
      ]
    },
    {
      id: "access-lists",
      title: "Player lists",
      description: "Operators, whitelist players, and bans materialize into Minecraft's native JSON files.",
      layoutClass: "minecraft-access-lists",
      keys: [
        "enable_whitelist",
        "enforce_whitelist",
        "operator_entries",
        "whitelist_entries",
        "banned_player_entries",
        "banned_ip_entries"
      ]
    },
    {
      id: "spam-controls",
      title: "Chat and command moderation",
      description: "Spam thresholds and text filtering for player chat and commands.",
      layoutClass: "minecraft-spam-controls",
      keys: ["chat_spam_threshold_seconds", "command_spam_threshold_seconds", "text_filtering_config", "text_filtering_version"]
    },
    {
      id: "world-protection",
      title: "Flight checks",
      description: "Control whether the server disconnects players for flying.",
      layoutClass: "minecraft-world-protection",
      keys: ["allow_flight"]
    }
  ],
  world: [
    {
      id: "world-file",
      title: "World generation",
      description: "Seed, generation preset, structures, and world-size limits.",
      layoutClass: "minecraft-world-file",
      keys: [
        "level_seed",
        "level_type",
        "generator_settings",
        "generate_structures",
        "max_world_size"
      ]
    },
    {
      id: "game-rules",
      title: "Gameplay baseline",
      description: "Default mode, difficulty, hardcore, spawn protection, and idle behavior define the world players enter into.",
      layoutClass: "minecraft-game-rules",
      keys: [
        "gamemode",
        "force_gamemode",
        "difficulty",
        "hardcore",
        "spawn_protection",
        "player_idle_timeout",
        "pause_when_empty_seconds"
      ]
    },
    {
      id: "resource-packs",
      title: "Resource and data packs",
      description: "Resource-pack delivery and the data packs enabled when creating a world.",
      layoutClass: "minecraft-resource-packs",
      keys: ["initial_enabled_packs", "initial_disabled_packs", "resource_pack", "resource_pack_id", "resource_pack_sha1", "resource_pack_prompt", "require_resource_pack"]
    }
  ],
  network: [
    {
      id: "remote",
      title: "Discovery and transport",
      description: "Server queries, compression, packet limits, transfers, and status updates.",
      layoutClass: "minecraft-remote",
      keys: [
        "enable_query",
        "network_compression_threshold",
        "rate_limit",
        "use_native_transport",
        "accepts_transfers",
        "status_heartbeat_interval"
      ]
    },
    {
      id: "management",
      title: "Management endpoint",
      description: "Keep the WebSocket endpoint local or protect it with a strong secret, restricted origins, and TLS.",
      layoutClass: "minecraft-management",
      keys: [
        "management_server_enabled",
        "management_server_host",
        "management_server_port",
        "management_server_secret",
        "management_server_allowed_origins",
        "management_server_tls_enabled",
        "management_server_tls_keystore",
        "management_server_tls_keystore_password"
      ]
    }
  ],
  advanced: [
    {
      id: "performance",
      title: "Performance and diagnostics",
      description: "Java memory, chunk processing, storage compression, monitoring, and IP logging.",
      layoutClass: "minecraft-performance",
      keys: [
        "memory_min_mb",
        "memory_max_mb",
        "view_distance",
        "simulation_distance",
        "entity_broadcast_range_percentage",
        "max_tick_time",
        "sync_chunk_writes",
        "max_chained_neighbor_updates",
        "enable_jmx_monitoring",
        "region_file_compression",
        "log_ips"
      ]
    },
    {
      id: "overrides",
      title: "Additional properties",
      description: "Additional native server.properties entries, one key=value pair per line.",
      layoutClass: "minecraft-overrides",
      keys: ["extra_properties"]
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

function buildMinecraftSections(t: TranslateFn): GuidedSettingsSection[] {
  return MINECRAFT_SECTIONS.map((section) => ({
    ...section,
    title: t(`minecraft.settings.sections.${section.id}`, undefined, section.title),
    description: t(
      `minecraft.settings.sections.${section.id}Description`,
      undefined,
      section.description ?? ""
    )
  }));
}

function buildMinecraftFieldCopy(key: string, t: TranslateFn): GuidedFieldCopy | undefined {
  const baseKey = `settings.schema.minecraft.${key}`;
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

function getMinecraftEnumOptionLabel(fieldKey: string, value: unknown, t: TranslateFn): string | undefined {
  if (typeof value !== "string" && typeof value !== "number") {
    return undefined;
  }

  return readCatalogText(t, `settings.schema.minecraft.${fieldKey}.option.${buildSchemaEnumOptionKey(value)}`);
}

function buildMinecraftFieldGroups(
  sectionId: string,
  fields: GuidedSettingsField[],
  t: TranslateFn
): SettingsModuleFieldGroup[] {
  const fieldsByKey = new Map(fields.map((field) => [field.key, field]));
  const claimedKeys = new Set<string>();
  const groups: SettingsModuleFieldGroup[] = [];

  for (const spec of MINECRAFT_GROUP_SPECS[sectionId] ?? []) {
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
      title: t(`minecraft.settings.groups.${spec.id}.title`, undefined, spec.title),
      description: t(`minecraft.settings.groups.${spec.id}.description`, undefined, spec.description),
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

function readNumber(value: unknown): number | null {
  if (typeof value === "number" && Number.isFinite(value)) {
    return value;
  }

  if (typeof value === "string" && value.trim().length > 0) {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : null;
  }

  return null;
}

function readBoolean(value: unknown): boolean {
  if (typeof value === "boolean") {
    return value;
  }

  if (typeof value === "string") {
    return value.trim().toLowerCase() === "true";
  }

  return Boolean(value);
}

function readText(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}

function readSettingNumber(settings: SettingsObject, key: string): number | null {
  return readNumber(settings[key]);
}

function isWeakRconPassword(passwordValue: unknown): boolean {
  const password = readText(passwordValue);
  return password.length === 0 || password === "change-me-rcon";
}

export const minecraftSettingsDefinition: SettingsModuleDefinition = {
  id: "minecraft",
  getSections: buildMinecraftSections,
  buildFieldGroups: (sectionId, fields, _locale, t) =>
    buildMinecraftFieldGroups(sectionId, fields, t),
  getFieldCopy: (key, t) => buildMinecraftFieldCopy(key, t),
  getEnumOptionLabel: (fieldKey, value, _locale, t) => getMinecraftEnumOptionLabel(fieldKey, value, t),
  resolveFieldEditorVariant(key) {
    if (
      key === "operator_entries" ||
      key === "whitelist_entries" ||
      key === "banned_player_entries" ||
      key === "banned_ip_entries"
    ) {
      return "string-list";
    }

    return undefined;
  },
  getFieldValidationMessage({ field, value, settings, t }) {
    if (field.key === "eula_accepted" && !readBoolean(value)) {
      return t(
        "minecraft.settings.validation.eula",
        undefined,
        "Accept Mojang's Minecraft EULA before starting a vanilla server."
      );
    }

    if (field.key === "memory_min_mb" || field.key === "memory_max_mb") {
      const minMemory = field.key === "memory_min_mb"
        ? readNumber(value)
        : readSettingNumber(settings, "memory_min_mb");
      const maxMemory = field.key === "memory_max_mb"
        ? readNumber(value)
        : readSettingNumber(settings, "memory_max_mb");

      if (minMemory != null && maxMemory != null && maxMemory < minMemory) {
        return t(
          "minecraft.settings.validation.memoryOrder",
          undefined,
          "Maximum memory must be greater than or equal to minimum memory."
        );
      }
    }

    if (field.key === "enable_rcon" && readBoolean(value) && isWeakRconPassword(settings.rcon_password)) {
      return t(
        "minecraft.settings.validation.rconPassword",
        undefined,
        "Set a dedicated RCON password before enabling RCON."
      );
    }

    if (field.key === "rcon_password" && readBoolean(settings.enable_rcon) && isWeakRconPassword(value)) {
      return t(
        "minecraft.settings.validation.rconPassword",
        undefined,
        "Set a dedicated RCON password before enabling RCON."
      );
    }

    if (
      field.key === "management_server_enabled" ||
      field.key === "management_server_tls_enabled" ||
      field.key === "management_server_tls_keystore"
    ) {
      const managementEnabled = field.key === "management_server_enabled"
        ? readBoolean(value)
        : readBoolean(settings.management_server_enabled);
      const tlsEnabled = field.key === "management_server_tls_enabled"
        ? readBoolean(value)
        : readBoolean(settings.management_server_tls_enabled);
      const keystore = field.key === "management_server_tls_keystore"
        ? readText(value)
        : readText(settings.management_server_tls_keystore);

      if (managementEnabled && tlsEnabled && !keystore) {
        return t(
          "minecraft.settings.validation.managementTlsKeystore",
          undefined,
          "Choose a PKCS#12 keystore before enabling the TLS-protected management server."
        );
      }
    }

    return undefined;
  }
};
