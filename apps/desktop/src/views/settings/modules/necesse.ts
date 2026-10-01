import {
  NECESSE_OWNER_PLACEHOLDER,
  NECESSE_PASSWORD_PLACEHOLDER,
  isNecesseOwnerNameRuntimeUnsafe
} from "../../../necesse-model";
import type { TranslateFn } from "../../../i18n";
import type { SettingsModuleDefinition, SettingsModuleFieldGroup } from "../module-types";
import type {
  GuidedFieldCopy,
  GuidedSettingsField,
  GuidedSettingsSection
} from "../settings-schema";

const FIELD_COPY: Record<
  string,
  {
    titleKey: string;
    fallbackTitle: string;
    descriptionKey?: string;
    fallbackDescription?: string;
  }
> = {
  world_name: {
    titleKey: "settings.schema.necesse.world_name.title",
    fallbackTitle: "World Name",
    descriptionKey: "settings.schema.necesse.world_name.description",
    fallbackDescription: "This becomes the live world save identity loaded by the dedicated server under this instance's managed Necesse saves root."
  },
  max_slots: {
    titleKey: "settings.schema.necesse.max_slots.title",
    fallbackTitle: "Max Players",
    descriptionKey: "settings.schema.necesse.max_slots.description",
    fallbackDescription: "Dedicated server slot count. Necesse currently accepts values from 1 to 250."
  },
  motd: {
    titleKey: "settings.schema.necesse.motd.title",
    fallbackTitle: "Message of the Day",
    descriptionKey: "settings.schema.necesse.motd.description",
    fallbackDescription: "Shown to joining players. Use \\n if you want line breaks in the dedicated server message."
  },
  owner_name: {
    titleKey: "settings.schema.necesse.owner_name.title",
    fallbackTitle: "Owner Player Name",
    descriptionKey: "settings.schema.necesse.owner_name.description",
    fallbackDescription:
      "Player name that should automatically receive owner permissions when joining this server. Current dedicated-server builds ignore owner names containing '-' characters, so use the real in-game name you actually join with and avoid hyphens."
  },
  password: {
    titleKey: "settings.schema.necesse.password.title",
    fallbackTitle: "Join Password",
    descriptionKey: "settings.schema.necesse.password.description",
    fallbackDescription: "Join gate for the room. Clear it only if you intentionally want an open server."
  },
  pause_when_empty: {
    titleKey: "settings.schema.necesse.pause_when_empty.title",
    fallbackTitle: "Pause When Empty",
    descriptionKey: "settings.schema.necesse.pause_when_empty.description",
    fallbackDescription: "Pause world simulation whenever no players are currently online."
  },
  strict_server_authority: {
    titleKey: "settings.schema.necesse.strict_server_authority.title",
    fallbackTitle: "Enable Server Authority Checks",
    descriptionKey: "settings.schema.necesse.strict_server_authority.description",
    fallbackDescription: "Makes the dedicated server validate client actions more strictly."
  },
  logging_enabled: {
    titleKey: "settings.schema.necesse.logging_enabled.title",
    fallbackTitle: "Write Session Logs",
    descriptionKey: "settings.schema.necesse.logging_enabled.description",
    fallbackDescription: "Create a native Necesse log file for each dedicated-server session under the instance data root."
  },
  zip_saves: {
    titleKey: "settings.schema.necesse.zip_saves.title",
    fallbackTitle: "Compress World Saves",
    descriptionKey: "settings.schema.necesse.zip_saves.description",
    fallbackDescription: "Keep the live world as a .zip save file under this instance's managed saves root."
  },
  language: {
    titleKey: "settings.schema.necesse.language.title",
    fallbackTitle: "Server Language",
    descriptionKey: "settings.schema.necesse.language.description",
    fallbackDescription: "Language code for occasional server-side messages and log text."
  },
  ignore_seasons: {
    titleKey: "settings.schema.necesse.ignore_seasons.title",
    fallbackTitle: "Disable Seasonal Content",
    descriptionKey: "settings.schema.necesse.ignore_seasons.description",
    fallbackDescription: "Turn off seasonal event content on this dedicated server."
  },
  custom_launch_flags: {
    titleKey: "settings.schema.necesse.custom_launch_flags.title",
    fallbackTitle: "Advanced: Custom Launch Flags",
    descriptionKey: "settings.schema.necesse.custom_launch_flags.description",
    fallbackDescription: "Additional Necesse launch flags appended to the launch plan."
  }
};

const WINDOWS_FILE_NAME_PATTERN = /[\\/:*?"<>|\r\n]/;
const WINDOWS_COMMAND_UNSAFE_PATTERN = /["\r\n]/;

function buildFieldCopy(key: string, t: TranslateFn): GuidedFieldCopy | undefined {
  const entry = FIELD_COPY[key];
  if (!entry) {
    const title = t(`settings.schema.necesse.${key}.title`, undefined, "");
    return title ? { title, description: t(`settings.schema.necesse.${key}.description`, undefined, "") } : undefined;
  }

  return {
    title: t(entry.titleKey, undefined, entry.fallbackTitle),
    description: entry.descriptionKey
      ? t(entry.descriptionKey, undefined, entry.fallbackDescription ?? "")
      : undefined
  };
}

function buildNecesseSections(t: TranslateFn): GuidedSettingsSection[] {
  return [
    {
      id: "access",
      title: t("necesse.settings.sections.access", undefined, "Join & Owner"),
      description: t(
        "necesse.settings.sections.accessDescription",
        undefined,
        "Server validation of client actions and permission policy."
      )
    },
    {
      id: "network",
      title: t("necesse.settings.sections.network", undefined, "Network"),
      description: t(
        "necesse.settings.sections.networkDescription",
        undefined,
        "Keep the host port and actual join route visible instead of hiding them behind default assumptions."
      ),
      showWhenEmpty: true
    },
    {
      id: "host",
      title: t("necesse.settings.sections.host", undefined, "Host Runtime"),
      description: t(
        "necesse.settings.sections.hostDescription",
        undefined,
        "Pause behavior, logs, save format, and launch overrides."
      )
    }
  ];
}

function buildNecesseFieldGroups(
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
    access: [
      {
        id: "access",
        title: t("necesse.settings.groups.access", undefined, "Authority checks"),
        description: t(
          "necesse.settings.groups.accessDescription",
          undefined,
          "Server validation of client actions."
        ),
        keys: ["owner_name", "strict_server_authority"]
      }
    ],
    network: [{ id: "latency", title: t("necesse.settings.groups.latency", undefined, "Client timeout"), description: "", keys: ["max_client_latency_seconds"] }],
    world: [
      { id: "world-capacity", title: t("necesse.settings.groups.worldCapacity", undefined, "World capacity"), description: "", keys: ["dropped_items_life_minutes", "max_settlements_per_player", "max_settlers_per_settlement", "world_border_size"] },
      {
        id: "seasons",
        title: t("necesse.settings.groups.seasons", undefined, "Seasonal content"),
        description: t("necesse.settings.groups.seasonsDescription", undefined, "Enable or disable seasonal event content."),
        keys: ["ignore_seasons"]
      }
    ],
    host: [
      {
        id: "host",
        title: t("necesse.settings.groups.host", undefined, "Runtime and diagnostics"),
        description: t(
          "necesse.settings.groups.hostDescription",
          undefined,
          "Pause behavior, logs, save format, and launch overrides."
        ),
        keys: [
          "unload_levels_cooldown",
          "unload_settlements",
          "pause_when_empty",
          "logging_enabled",
          "zip_saves",
          "custom_launch_flags"
        ]
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

export const necesseSettingsDefinition: SettingsModuleDefinition = {
  id: "necesse",
  getSections: buildNecesseSections,
  getFieldCopy: (key, t) => buildFieldCopy(key, t),
  buildFieldGroups: (sectionId, fields, _locale, t) => buildNecesseFieldGroups(sectionId, fields, _locale, t),
  getFieldValidationMessage: ({ field, value, t }) => {
    if (field.key === "world_name") {
      const worldName = typeof value === "string" ? value.trim() : "";
      if (!worldName) {
        return t(
          "necesse.settings.validation.worldNameMissing",
          undefined,
          "Necesse world name cannot be empty."
        );
      }
      if (WINDOWS_FILE_NAME_PATTERN.test(worldName)) {
        return t(
          "necesse.settings.validation.worldNameInvalid",
          undefined,
          "World name cannot contain Windows path characters like \\ / : * ? \" < > |."
        );
      }
      return undefined;
    }

    if (field.key === "max_slots") {
      const maxSlots = Number(value);
      if (!Number.isFinite(maxSlots)) {
        return undefined;
      }
      if (maxSlots < 1 || maxSlots > 250) {
        return t(
          "necesse.settings.validation.maxSlotsRange",
          undefined,
          "Necesse player slots must stay between 1 and 250."
        );
      }
      return undefined;
    }

    if (field.key === "owner_name") {
      const ownerName = typeof value === "string" ? value.trim() : "";
      if (WINDOWS_COMMAND_UNSAFE_PATTERN.test(ownerName)) {
        return t(
          "necesse.settings.validation.batchUnsafeText",
          undefined,
          "This field cannot contain double quotes or line breaks in the Windows launch command."
        );
      }
      if (isNecesseOwnerNameRuntimeUnsafe(ownerName)) {
        return t(
          "necesse.settings.validation.ownerHyphen",
          undefined,
          "Current Necesse dedicated-server builds ignore owner names containing '-'. Use the exact in-game player name without hyphens."
        );
      }
      if (ownerName === NECESSE_OWNER_PLACEHOLDER) {
        return t(
          "necesse.settings.validation.ownerPlaceholder",
          undefined,
          "Owner permissions still point at the placeholder name. Replace it with the real in-game player name you use to join."
        );
      }
      return undefined;
    }

    if (field.key === "password") {
      const password = typeof value === "string" ? value.trim() : "";
      if (WINDOWS_COMMAND_UNSAFE_PATTERN.test(password)) {
        return t(
          "necesse.settings.validation.batchUnsafeText",
          undefined,
          "This field cannot contain double quotes or line breaks in the Windows launch command."
        );
      }
      if (password === NECESSE_PASSWORD_PLACEHOLDER) {
        return t(
          "necesse.settings.validation.passwordPlaceholder",
          undefined,
          "Join password is still using the placeholder value. Replace it before daily use, or clear it if you intentionally want an open room."
        );
      }
      return undefined;
    }

    if (field.key === "motd" || field.key === "language") {
      const text = typeof value === "string" ? value : "";
      if (WINDOWS_COMMAND_UNSAFE_PATTERN.test(text)) {
        return t(
          "necesse.settings.validation.batchUnsafeText",
          undefined,
          "This field cannot contain double quotes or line breaks in the Windows launch command."
        );
      }
      return undefined;
    }

    return undefined;
  }
};
