import {
  formatCoreKeeperAllowedPlatform,
  formatCoreKeeperSeasonOverride,
  formatCoreKeeperWorldMode,
  normalizeCoreKeeperGameId,
  parseCoreKeeperSteamIdList
} from "../../../corekeeper-model";
import type { TranslateFn } from "../../../i18n";
import type { SettingsModuleDefinition, SettingsModuleFieldGroup } from "../module-types";
import type { GuidedEditorVariant, GuidedFieldCopy, GuidedSettingsField, GuidedSettingsSection } from "../settings-schema";

const FIELD_COPY: Record<
  string,
  {
    titleKey: string;
    fallbackTitle: string;
    descriptionKey?: string;
    fallbackDescription?: string;
  }
> = {
  server_name: {
    titleKey: "settings.schema.corekeeper.server_name.title",
    fallbackTitle: "Server Name",
    descriptionKey: "settings.schema.corekeeper.server_name.description",
    fallbackDescription: "Shown to players when they browse or join this Core Keeper world."
  },
  game_id: {
    titleKey: "settings.schema.corekeeper.game_id.title",
    fallbackTitle: "Steam Game ID",
    descriptionKey: "settings.schema.corekeeper.game_id.description",
    fallbackDescription: "Stable Steam relay join code. Leave blank to let LanGame derive one from this instance."
  },
  max_players: {
    titleKey: "settings.schema.corekeeper.max_players.title",
    fallbackTitle: "Max Players",
    descriptionKey: "settings.schema.corekeeper.max_players.description",
    fallbackDescription: "Maximum number of players allowed in this world."
  },
  world_index: {
    titleKey: "settings.schema.corekeeper.world_index.title",
    fallbackTitle: "World Slot",
    descriptionKey: "settings.schema.corekeeper.world_index.description",
    fallbackDescription: "Select which managed world slot this instance should load."
  },
  world_seed: {
    titleKey: "settings.schema.corekeeper.world_seed.title",
    fallbackTitle: "World Seed",
    descriptionKey: "settings.schema.corekeeper.world_seed.description",
    fallbackDescription: "Seed text used when a fresh world is created."
  },
  hashed_world_seed: {
    titleKey: "settings.schema.corekeeper.hashed_world_seed.title",
    fallbackTitle: "Hashed World Seed",
    descriptionKey: "settings.schema.corekeeper.hashed_world_seed.description",
    fallbackDescription: "Unsigned 32-bit seed hash used only when World Seed is empty."
  },
  world_mode: {
    titleKey: "settings.schema.corekeeper.world_mode.title",
    fallbackTitle: "World Mode",
    descriptionKey: "settings.schema.corekeeper.world_mode.description",
    fallbackDescription: "Core Keeper world difficulty and pacing mode."
  },
  season_override: {
    titleKey: "settings.schema.corekeeper.season_override.title",
    fallbackTitle: "Season Override",
    descriptionKey: "settings.schema.corekeeper.season_override.description",
    fallbackDescription: "Force one seasonal event, or stay on the game's automatic calendar."
  },
  max_packets_per_frame: {
    titleKey: "settings.schema.corekeeper.max_packets_per_frame.title",
    fallbackTitle: "Network Packet Limit",
    descriptionKey: "settings.schema.corekeeper.max_packets_per_frame.description",
    fallbackDescription: "Upper bound for packets the dedicated server sends each frame."
  },
  network_send_rate: {
    titleKey: "settings.schema.corekeeper.network_send_rate.title",
    fallbackTitle: "Network Send Rate",
    descriptionKey: "settings.schema.corekeeper.network_send_rate.description",
    fallbackDescription: "Packet send cadence used by the dedicated server networking loop."
  },
  direct_connection_enabled: {
    titleKey: "settings.schema.corekeeper.direct_connection_enabled.title",
    fallbackTitle: "Use Direct Connection Mode",
    descriptionKey: "settings.schema.corekeeper.direct_connection_enabled.description",
    fallbackDescription: "Switch from Steam relay to direct IP:port joins for this instance."
  },
  join_password: {
    titleKey: "settings.schema.corekeeper.join_password.title",
    fallbackTitle: "Join Password",
    descriptionKey: "settings.schema.corekeeper.join_password.description",
    fallbackDescription: "Required for direct IP sharing. Replace the placeholder before daily use."
  },
  allowed_platform_code: {
    titleKey: "settings.schema.corekeeper.allowed_platform_code.title",
    fallbackTitle: "Allowed Platform",
    descriptionKey: "settings.schema.corekeeper.allowed_platform_code.description",
    fallbackDescription: "Optional storefront restriction applied only to direct IP joins."
  },
  admin_list: {
    titleKey: "settings.schema.corekeeper.admin_list.title",
    fallbackTitle: "Admin Steam64 IDs",
    descriptionKey: "settings.schema.corekeeper.admin_list.description",
    fallbackDescription: "One Steam64 ID per line. LanGame materializes Admins.json for this instance."
  },
  ban_list: {
    titleKey: "settings.schema.corekeeper.ban_list.title",
    fallbackTitle: "Banned Steam64 IDs",
    descriptionKey: "settings.schema.corekeeper.ban_list.description",
    fallbackDescription: "One Steam64 ID per line. LanGame materializes PlayerBans.json for this instance."
  }
};

function readBoolean(value: unknown): boolean {
  if (typeof value === "boolean") {
    return value;
  }
  if (typeof value === "number") {
    return value !== 0;
  }
  if (typeof value === "string") {
    const normalized = value.trim().toLowerCase();
    if (["true", "1", "yes", "on"].includes(normalized)) {
      return true;
    }
    if (["false", "0", "no", "off", ""].includes(normalized)) {
      return false;
    }
  }
  return Boolean(value);
}

function readNumber(value: unknown): number | null {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function buildFieldCopy(key: string, t: TranslateFn): GuidedFieldCopy | undefined {
  const entry = FIELD_COPY[key];
  if (!entry) {
    return undefined;
  }

  return {
    title: t(entry.titleKey, undefined, entry.fallbackTitle),
    description: entry.descriptionKey
      ? t(entry.descriptionKey, undefined, entry.fallbackDescription ?? "")
      : undefined
  };
}

function buildCoreKeeperSections(t: TranslateFn): GuidedSettingsSection[] {
  return [
    {
      id: "world",
      title: t("corekeeper.settings.sections.world", undefined, "World"),
      description: t(
        "corekeeper.settings.sections.worldDescription",
        undefined,
        "Choose which managed world slot to boot and what seasonal or difficulty posture it should carry."
      )
    },
    {
      id: "network",
      title: t("corekeeper.settings.sections.network", undefined, "Network"),
      description: t(
        "corekeeper.settings.sections.networkDescription",
        undefined,
        "Keep relay vs direct join mode, host port, and networking cadence in one explicit Windows-first lane."
      )
    },
    {
      id: "access",
      title: t("corekeeper.settings.sections.access", undefined, "Access"),
      description: t(
        "corekeeper.settings.sections.accessDescription",
        undefined,
        "Manage the Steam64 lists that materialize into the JSON files Core Keeper actually reads."
      )
    }
  ];
}

function buildIdentifierValidationMessage(
  value: unknown,
  t: TranslateFn
): string | undefined {
  const { invalidEntries } = parseCoreKeeperSteamIdList(value);
  if (invalidEntries.length === 0) {
    return undefined;
  }

  return t(
    "corekeeper.settings.validation.identifierListInvalid",
    {
      preview: invalidEntries.slice(0, 3).join(", ")
    },
    `Only Steam64 IDs are valid here. Remove or fix: ${invalidEntries.slice(0, 3).join(", ")}`
  );
}

function resolveEnumLabel(
  fieldKey: string,
  value: unknown,
  t: TranslateFn
): string | undefined {
  if (fieldKey === "world_mode") {
    return formatCoreKeeperWorldMode(value, t);
  }

  if (fieldKey === "season_override") {
    return formatCoreKeeperSeasonOverride(value, t);
  }

  if (fieldKey === "allowed_platform_code") {
    return formatCoreKeeperAllowedPlatform(value, t);
  }

  return undefined;
}

function buildCoreKeeperFieldGroups(
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
    world: [
      {
        id: "world",
        title: t("corekeeper.settings.groups.world", undefined, "World"),
        description: t(
          "corekeeper.settings.groups.worldDescription",
          undefined,
          "Which world slot to load and how seasonal difficulty should apply."
        ),
        keys: ["world_seed", "hashed_world_seed", "world_mode", "season_override"]
      }
    ],
    network: [
      {
        id: "network",
        title: t("corekeeper.settings.groups.network", undefined, "Network"),
        description: t(
          "corekeeper.settings.groups.networkDescription",
          undefined,
          "Direct join posture and packet cadence controls."
        ),
        keys: ["max_packets_per_frame", "network_send_rate", "direct_connection_enabled"]
      }
    ],
    access: [
      {
        id: "access",
        title: t("corekeeper.settings.groups.access", undefined, "Platform admission"),
        description: t(
          "corekeeper.settings.groups.accessDescription",
          undefined,
          "Direct-connection credentials. Administrator and ban lists are managed in Player Access."
        ),
        keys: ["admin_list", "ban_list", "allowed_platform_code"]
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

export const coreKeeperSettingsDefinition: SettingsModuleDefinition = {
  id: "corekeeper",
  getSections: buildCoreKeeperSections,
  getFieldCopy: (key, t) => buildFieldCopy(key, t),
  buildFieldGroups: (sectionId, fields, _locale, t) => buildCoreKeeperFieldGroups(sectionId, fields, _locale, t),
  getEnumOptionLabel: (fieldKey, value, _locale, t) => resolveEnumLabel(fieldKey, value, t),
  resolveFieldEditorVariant: (key): GuidedEditorVariant | undefined =>
    key === "admin_list" || key === "ban_list" ? "string-list" : undefined,
  getFieldValidationMessage: ({ field, value, settings, t }) => {
    if (field.key === "max_players") {
      const maxPlayers = readNumber(value);
      if (maxPlayers === null) {
        return undefined;
      }

      if (maxPlayers < 1 || maxPlayers > 100) {
        return t(
          "corekeeper.settings.validation.maxPlayersRange",
          undefined,
          "Keep the Core Keeper player cap between 1 and 100."
        );
      }
      return undefined;
    }

    if (field.key === "game_id") {
      const raw = typeof value === "string" ? value.trim() : "";
      if (!raw) {
        return undefined;
      }
      if (!normalizeCoreKeeperGameId(raw)) {
        return t(
          "corekeeper.settings.validation.gameIdFormat",
          undefined,
          "Steam relay Game ID must stay alphanumeric and land between 15 and 28 characters after cleanup."
        );
      }
      return undefined;
    }

    if (field.key === "hashed_world_seed") {
      const worldSeed = typeof settings.world_seed === "string" ? settings.world_seed.trim() : "";
      if (worldSeed) {
        return t(
          "corekeeper.settings.validation.hashedSeedIgnored",
          undefined,
          "Core Keeper ignores the hashed seed while World Seed contains text. Clear World Seed to use this value."
        );
      }
      return undefined;
    }

    if (field.key === "join_password") {
      if (!readBoolean(settings.direct_connection_enabled)) {
        return undefined;
      }

      const password = typeof value === "string" ? value.trim() : "";
      if (!password) {
        return t(
          "corekeeper.settings.validation.joinPasswordMissing",
          undefined,
          "Direct connection mode is on, so this instance needs a real join password before you share the route."
        );
      }
      if (password === "change-me-corekeeper") {
        return t(
          "corekeeper.settings.validation.joinPasswordPlaceholder",
          undefined,
          "Direct connection mode is still using the placeholder password. Replace it before daily use."
        );
      }
      return undefined;
    }

    if (field.key === "admin_list" || field.key === "ban_list") {
      return buildIdentifierValidationMessage(value, t);
    }

    return undefined;
  }
};
