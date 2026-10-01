import type { TranslateFn } from "../../../i18n";
import type { SettingsModuleDefinition, SettingsModuleFieldGroup } from "../module-types";
import type { GuidedFieldCopy, GuidedSettingsField, GuidedSettingsSection } from "../settings-schema";

const ACCESS_LIST_FIELDS = new Set(["admin_list", "banned_list", "permitted_list"]);

const VALHEIM_FIELD_COPY: Record<
  string,
  {
    titleKey: string;
    fallbackTitle: string;
    descriptionKey?: string;
    fallbackDescription?: string;
  }
> = {
  server_name: {
    titleKey: "settings.schema.valheim.server_name.title",
    fallbackTitle: "Server Name",
    descriptionKey: "settings.schema.valheim.server_name.description",
    fallbackDescription: "Shown in the Valheim browser and in the invite text you share with friends."
  },
  public_server: {
    titleKey: "settings.schema.valheim.public_server.title",
    fallbackTitle: "Browser Visibility",
    descriptionKey: "settings.schema.valheim.public_server.description",
    fallbackDescription: "Controls whether the dedicated server announces itself publicly or stays private or direct-share only."
  },
  crossplay_enabled: {
    titleKey: "settings.schema.valheim.crossplay_enabled.title",
    fallbackTitle: "Enable Crossplay",
    descriptionKey: "settings.schema.valheim.crossplay_enabled.description",
    fallbackDescription: "Uses Valheim's crossplay backend so Steam and non-Steam players can join through the same room."
  },
  instance_id: {
    titleKey: "settings.schema.valheim.instance_id.title",
    fallbackTitle: "PlayFab Instance ID",
    descriptionKey: "settings.schema.valheim.instance_id.description",
    fallbackDescription: "Optional Valheim -instanceid value for running multiple servers with the same public IP and port identity."
  },
  world_name: {
    titleKey: "settings.schema.valheim.world_name.title",
    fallbackTitle: "World Name",
    descriptionKey: "settings.schema.valheim.world_name.description",
    fallbackDescription: "This becomes the world save identity inside the instance-owned Valheim save root."
  },
  world_preset: {
    titleKey: "settings.schema.valheim.world_preset.title",
    fallbackTitle: "World Preset",
    descriptionKey: "settings.schema.valheim.world_preset.description",
    fallbackDescription: "Apply a preset at startup, replacing saved world modifiers. Leave empty to preserve the current world rules."
  },
  world_modifiers: {
    titleKey: "settings.schema.valheim.world_modifiers.title",
    fallbackTitle: "World Modifiers",
    descriptionKey: "settings.schema.valheim.world_modifiers.description",
    fallbackDescription: "One modifier and value per line, for example combat hard or resources most. Applied after the selected preset."
  },
  world_set_keys: {
    titleKey: "settings.schema.valheim.world_set_keys.title",
    fallbackTitle: "World Keys",
    descriptionKey: "settings.schema.valheim.world_set_keys.description",
    fallbackDescription: "One world key per line: nobuildcost, playerevents, passivemobs, or nomap."
  },
  save_interval_seconds: {
    titleKey: "settings.schema.valheim.save_interval_seconds.title",
    fallbackTitle: "Auto Save Interval (Seconds)",
    descriptionKey: "settings.schema.valheim.save_interval_seconds.description",
    fallbackDescription: "How often the dedicated server writes the live world state to disk."
  },
  backup_count: {
    titleKey: "settings.schema.valheim.backup_count.title",
    fallbackTitle: "Backup Count",
    descriptionKey: "settings.schema.valheim.backup_count.description",
    fallbackDescription: "How many rolling Valheim-native backup generations the server keeps."
  },
  backup_short_seconds: {
    titleKey: "settings.schema.valheim.backup_short_seconds.title",
    fallbackTitle: "Short Backup Interval (Seconds)",
    descriptionKey: "settings.schema.valheim.backup_short_seconds.description",
    fallbackDescription: "Cadence for short-interval native backup snapshots."
  },
  backup_long_seconds: {
    titleKey: "settings.schema.valheim.backup_long_seconds.title",
    fallbackTitle: "Long Backup Interval (Seconds)",
    descriptionKey: "settings.schema.valheim.backup_long_seconds.description",
    fallbackDescription: "Cadence for long-interval native backup snapshots."
  },
  custom_launch_flags: {
    titleKey: "settings.schema.valheim.custom_launch_flags.title",
    fallbackTitle: "Advanced: Custom Launch Flags",
    descriptionKey: "settings.schema.valheim.custom_launch_flags.description",
    fallbackDescription: "Additional Valheim launch flags appended to the launch plan."
  },
  log_file: {
    titleKey: "settings.schema.valheim.log_file.title",
    fallbackTitle: "Log File Path",
    descriptionKey: "settings.schema.valheim.log_file.description",
    fallbackDescription: "Optional path passed through -logFile. Leave blank to use LanGame managed logs only."
  },
  server_password: {
    titleKey: "settings.schema.valheim.server_password.title",
    fallbackTitle: "Join Password",
    descriptionKey: "settings.schema.valheim.server_password.description",
    fallbackDescription: "Written into the dedicated server launch command. Change the placeholder before you share the room."
  },
  permitted_list: {
    titleKey: "settings.schema.valheim.permitted_list.title",
    fallbackTitle: "Permitted Platform IDs",
    descriptionKey: "settings.schema.valheim.permitted_list.description",
    fallbackDescription: "One platform ID per line. Use this for private rooms or strict allow-list servers."
  },
  admin_list: {
    titleKey: "settings.schema.valheim.admin_list.title",
    fallbackTitle: "Admin Platform IDs",
    descriptionKey: "settings.schema.valheim.admin_list.description",
    fallbackDescription: "One platform ID per line. LanGame renders adminlist.txt and copies it into the active Valheim save root on launch."
  },
  banned_list: {
    titleKey: "settings.schema.valheim.banned_list.title",
    fallbackTitle: "Banned Platform IDs",
    descriptionKey: "settings.schema.valheim.banned_list.description",
    fallbackDescription: "One platform ID per line. LanGame renders bannedlist.txt and syncs it into the live Valheim save directory."
  }
};

interface ValheimFieldGroupSpec {
  id: string;
  titleKey: string;
  title: string;
  descriptionKey: string;
  description: string;
  layoutClass: string;
  keys: string[];
}

function readNumber(value: unknown): number | null {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function parseDelimitedEntries(value: unknown): string[] {
  if (typeof value !== "string" || value.trim().length === 0) {
    return [];
  }

  return value
    .replace(/\r\n/g, "\n")
    .replace(/\r/g, "\n")
    .split(/[\n;]+/)
    .map((entry) => entry.trim())
    .filter(Boolean);
}

function buildEntryPreview(entries: string[]): string {
  const preview = entries.slice(0, 3).join(", ");
  return entries.length > 3 ? `${preview} (+${entries.length - 3})` : preview;
}

function findDuplicateEntries(value: unknown): string[] {
  const seen = new Set<string>();
  const duplicateKeys = new Set<string>();
  const duplicates: string[] = [];

  for (const entry of parseDelimitedEntries(value)) {
    const normalizedEntry = entry.toLocaleLowerCase();
    if (seen.has(normalizedEntry)) {
      if (!duplicateKeys.has(normalizedEntry)) {
        duplicateKeys.add(normalizedEntry);
        duplicates.push(entry);
      }
      continue;
    }

    seen.add(normalizedEntry);
  }

  return duplicates;
}

function findOverlappingEntries(leftValue: unknown, rightValue: unknown): string[] {
  const rightEntries = new Set(parseDelimitedEntries(rightValue).map((entry) => entry.toLocaleLowerCase()));
  const seen = new Set<string>();
  const overlap: string[] = [];

  for (const entry of parseDelimitedEntries(leftValue)) {
    const normalizedEntry = entry.toLocaleLowerCase();
    if (!rightEntries.has(normalizedEntry) || seen.has(normalizedEntry)) {
      continue;
    }

    seen.add(normalizedEntry);
    overlap.push(entry);
  }

  return overlap;
}

function mergeUniqueEntries(entries: string[]): string[] {
  const seen = new Set<string>();
  const merged: string[] = [];

  for (const entry of entries) {
    const normalizedEntry = entry.toLocaleLowerCase();
    if (seen.has(normalizedEntry)) {
      continue;
    }

    seen.add(normalizedEntry);
    merged.push(entry);
  }

  return merged;
}

function buildFieldCopy(key: string, t: TranslateFn): GuidedFieldCopy | undefined {
  const entry = VALHEIM_FIELD_COPY[key];
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

function buildValheimSections(t: TranslateFn): GuidedSettingsSection[] {
  return [
    {
      id: "room",
      title: t("valheim.settings.sections.room", undefined, "Room & Discovery"),
      description: t(
        "valheim.settings.sections.roomDescription",
        undefined,
        "Server identity, browser exposure, and whether this room rides Steam direct-connect or the crossplay backend."
      )
    },
    {
      id: "world",
      title: t("valheim.settings.sections.world", undefined, "World & Saves"),
      description: t(
        "valheim.settings.sections.worldDescription",
        undefined,
        "World rules and native save or rolling-backup cadence."
      )
    },
    {
      id: "access",
      title: t("valheim.settings.sections.access", undefined, "Access & Lists"),
      description: t(
        "valheim.settings.sections.accessDescription",
        undefined,
        "Join password, allow-list, admin list, and bans that LanGame materializes into Valheim's native text files."
      )
    }
  ];
}

function buildGroupSpecMap(): Record<string, ValheimFieldGroupSpec[]> {
  return {
    network: [
      {
        id: "reachability",
        titleKey: "valheim.settings.groups.reachability.title",
        title: "Crossplay routing",
        descriptionKey: "valheim.settings.groups.reachability.description",
        description: "Crossplay backend and the instance identifier used for shared public endpoints.",
        layoutClass: "valheim-reachability",
        keys: ["crossplay_enabled", "instance_id"]
      }
    ],
    advanced: [
      {
        id: "world-launch-overrides",
        titleKey: "valheim.settings.groups.worldLaunchOverrides.title",
        title: "Launch overrides",
        descriptionKey: "valheim.settings.groups.worldLaunchOverrides.description",
        description: "Use custom launch flags only for fields not already covered by the typed controls above.",
        layoutClass: "world-launch-overrides",
        keys: ["log_file", "custom_launch_flags"]
      }
    ],
    world: [
      {
        id: "world-rules",
        titleKey: "valheim.settings.groups.worldRules.title",
        title: "World rule overlays",
        descriptionKey: "valheim.settings.groups.worldRules.description",
        description: "Preset, modifiers, and set keys are direct launch-level overlays for world behavior.",
        layoutClass: "world-rules",
        keys: ["world_preset", "world_modifiers", "world_set_keys"]
      },
      {
        id: "save-cadence",
        titleKey: "valheim.settings.groups.saveCadence.title",
        title: "Save cadence",
        descriptionKey: "valheim.settings.groups.saveCadence.description",
        description: "This sets how often the live world is flushed to disk, which directly affects how much progress can be lost after a crash or power cut.",
        layoutClass: "world-cadence",
        keys: ["save_interval_seconds"]
      },
      {
        id: "backup-ladder",
        titleKey: "valheim.settings.groups.backupLadder.title",
        title: "Native backup ladder",
        descriptionKey: "valheim.settings.groups.backupLadder.description",
        description: "Valheim ships with both short and long rolling backup ladders, which makes it useful for long-lived world rollback guardrails.",
        layoutClass: "world-backup-ladder",
        keys: ["backup_count", "backup_short_seconds", "backup_long_seconds"]
      }
    ],
    access: [
      {
        id: "rosters",
        titleKey: "valheim.settings.groups.rosters.title",
        title: "Admins and access lists",
        descriptionKey: "valheim.settings.groups.rosters.description",
        description: "Use one platform ID per line. LanGame writes these files into the instance config lane and copies them into the live save lane at launch time.",
        layoutClass: "access-rosters",
        keys: ["permitted_list", "admin_list", "banned_list"]
      }
    ]
  };
}

function buildValheimFieldGroups(
  sectionId: string,
  fields: GuidedSettingsField[],
  t: TranslateFn
): SettingsModuleFieldGroup[] {
  const fieldsByKey = new Map(fields.map((field) => [field.key, field]));
  const claimedKeys = new Set<string>();
  const groups: SettingsModuleFieldGroup[] = [];

  for (const group of buildGroupSpecMap()[sectionId] ?? []) {
    const groupFields = group.keys
      .map((key) => fieldsByKey.get(key))
      .filter((field): field is GuidedSettingsField => Boolean(field));

    if (groupFields.length === 0) {
      continue;
    }

    for (const field of groupFields) {
      claimedKeys.add(field.key);
    }

    groups.push({
      id: group.id,
      title: t(group.titleKey, undefined, group.title),
      description: t(group.descriptionKey, undefined, group.description),
      layoutClass: group.layoutClass,
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

  return groups.length > 0 ? groups : [{ id: "default", layoutClass: `${sectionId}-default`, fields }];
}

function getPublicServerLabel(value: unknown, t: TranslateFn): string | undefined {
  if (value === 1 || value === "1") {
    return t("settings.schema.valheim.public_server.option.1", undefined, "Public browser");
  }
  if (value === 0 || value === "0") {
    return t("settings.schema.valheim.public_server.option.0", undefined, "Private / direct only");
  }
  return undefined;
}

export const valheimSettingsDefinition: SettingsModuleDefinition = {
  id: "valheim",
  getSections: buildValheimSections,
  buildFieldGroups: (sectionId, fields, _locale, t) => buildValheimFieldGroups(sectionId, fields, t),
  getFieldCopy: (key, t) => buildFieldCopy(key, t),
  resolveFieldEditorVariant: (key) =>
    ACCESS_LIST_FIELDS.has(key) || key === "world_modifiers" || key === "world_set_keys"
      ? "string-list"
      : undefined,
  getEnumOptionLabel: (fieldKey, value, _locale, t) => {
    if (fieldKey !== "public_server") {
      return undefined;
    }
    return getPublicServerLabel(value, t);
  },
  getFieldValidationMessage: ({ field, value, settings, t }) => {
    if (ACCESS_LIST_FIELDS.has(field.key)) {
      const duplicateEntries = findDuplicateEntries(value);
      if (duplicateEntries.length > 0) {
        const preview = buildEntryPreview(duplicateEntries);
        return t(
          "valheim.settings.validation.listDuplicates",
          { preview },
          `Keep each platform ID only once in this list. Duplicates: ${preview}`
        );
      }

      const overlapEntries =
        field.key === "banned_list"
          ? mergeUniqueEntries([
              ...findOverlappingEntries(value, settings.permitted_list),
              ...findOverlappingEntries(value, settings.admin_list)
            ])
          : findOverlappingEntries(value, settings.banned_list);

      if (overlapEntries.length > 0) {
        const preview = buildEntryPreview(overlapEntries);
        return t(
          "valheim.settings.validation.accessConflict",
          { preview },
          `These platform IDs are currently on both sides of the gate: ${preview}. Remove them from either the banned list or the allow/admin lists.`
        );
      }
    }

    if (field.key === "backup_short_seconds" || field.key === "backup_long_seconds") {
      const shortInterval = readNumber(settings.backup_short_seconds);
      const longInterval = readNumber(settings.backup_long_seconds);
      if (shortInterval !== null && longInterval !== null && shortInterval >= longInterval) {
        return t(
          "valheim.settings.validation.backupIntervals",
          undefined,
          "Keep the short backup interval below the long backup interval so the rollback ladder stays meaningful."
        );
      }
    }

    if (field.key === "world_modifiers" || field.key === "world_set_keys") {
      const duplicates = findDuplicateEntries(value);
      if (duplicates.length > 0) {
        const preview = buildEntryPreview(duplicates);
        return t(
          "valheim.settings.validation.worldRuleDuplicates",
          { preview },
          `Keep each world rule entry only once. Duplicates: ${preview}`
        );
      }
    }

    if (field.key !== "server_password") {
      return undefined;
    }

    const password = typeof value === "string" ? value.trim() : "";
    if (password !== "change-me") {
      return undefined;
    }

    return t(
      "valheim.settings.validation.passwordPlaceholder",
      undefined,
      "Still using the placeholder join password. Change it before sharing this server."
    );
  }
};
