import type { TranslateFn } from "../../../i18n";
import type { SettingsModuleDefinition, SettingsModuleFieldGroup } from "../module-types";
import type { GuidedFieldCopy, GuidedSettingsField, GuidedSettingsSection } from "../settings-schema";

const SQUAD_SECTIONS: GuidedSettingsSection[] = [
  {
    id: "room",
    title: "Server Rules",
    description: "Server name, slots, messaging, and core team-balance rules."
  },
  {
    id: "network",
    title: "Visibility",
    description: "Server-list visibility and remote console access."
  },
  {
    id: "world",
    title: "Match Rules & Rotation",
    description: "Team rules, automatic moderation, rotation and voting behavior."
  },
  {
    id: "access",
    title: "Access & Admins",
    description: "Reserved slots and structured Admins.cfg entries."
  },
  {
    id: "advanced",
    title: "Advanced",
    description: "Demo recording and launch argument overrides."
  }
];

interface SquadGroupSpec {
  id: string;
  title: string;
  description: string;
  layoutClass: string;
  keys: string[];
}

const SQUAD_GROUP_SPECS: Record<string, SquadGroupSpec[]> = {
  network: [
    {
      id: "connection-admission",
      title: "Joining connections",
      description: "Time allowed for an incoming player to establish a connection.",
      layoutClass: "squad-connection-admission",
      keys: ["joining_player_timeout_seconds"]
    }
  ],
  world: [
    {
      id: "rules",
      title: "Team Rules",
      description: "Team-change and balance posture used in Server.cfg.",
      layoutClass: "squad-rules",
      keys: [
        "allow_team_changes",
        "prevent_team_change_if_unbalanced",
        "enforce_team_balance",
        "num_players_diff_for_team_changes",
        "alliance_enabled",
        "rejoin_squad_delay_after_kick",
        "tk_auto_kick_enabled",
        "auto_tk_ban_number_tks",
        "auto_tk_ban_time_seconds",
        "vehicle_kit_requirement_disabled",
        "vehicle_claiming_disabled"
      ]
    },
    {
      id: "rotation-mode",
      title: "Rotation Mode",
      description: "Map rotation mode, randomization, and voting posture from Server.cfg.",
      layoutClass: "squad-rotation-mode",
      keys: [
        "map_rotation_mode",
        "randomize_rotation_at_start",
        "use_vote_factions",
        "use_vote_level",
        "use_vote_layer",
        "allow_fireteam_layers_in_rotation",
        "time_between_matches_seconds",
        "time_before_vote_seconds",
        "prep_time_standard_seconds",
        "prep_time_small_scale_seconds"
      ]
    },
    {
      id: "rotation-files",
      title: "Voting configuration",
      description: "Voting pools, vote rules and additional match options.",
      layoutClass: "squad-rotation-files",
      keys: [
        "layer_voting",
        "layer_voting_low_players",
        "layer_voting_night",
        "vote_config",
        "custom_options",
        "excluded_factions",
        "excluded_layers",
        "excluded_levels"
      ]
    }
  ],
  access: [
    {
      id: "remote-lists",
      title: "Authorization and ban sources",
      description: "Remote sources used to grant administrator access and enforce bans.",
      layoutClass: "squad-remote-lists",
      keys: ["remote_ban_hosts", "remote_admin_hosts"]
    },
    {
      id: "operators",
      title: "Operator Access",
      description: "Administrator permissions, reserved slots and public queue capacity.",
      layoutClass: "squad-operators",
      keys: [
        "reserved_slots",
        "public_queue_limit",
        "allow_community_admin_access",
        "admin_steam_ids",
        "priority_join_steam_ids",
        "admin_permissions",
        "admins_cfg"
      ]
    }
  ],
  advanced: [
    {
      id: "runtime",
      title: "Runtime",
      description: "Demo capture and launch argument overrides.",
      layoutClass: "squad-runtime",
      keys: [
        "record_demos",
        "allow_public_clients_to_record",
        "allow_dev_profiling",
        "allow_qa",
        "extra_launch_args"
      ]
    }
  ]
};

function readCatalogText(t: TranslateFn, key: string): string | undefined {
  const value = t(key, undefined, "");
  return value.trim() ? value : undefined;
}

function buildSquadSections(t: TranslateFn): GuidedSettingsSection[] {
  return SQUAD_SECTIONS.map((section) => ({
    ...section,
    title: t(`squad.settings.sections.${section.id}`, undefined, section.title),
    description: t(
      `squad.settings.sections.${section.id}Description`,
      undefined,
      section.description ?? ""
    )
  }));
}

function buildSquadFieldCopy(key: string, t: TranslateFn): GuidedFieldCopy | undefined {
  const baseKey = `settings.schema.squad.${key}`;
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

function buildSquadFieldGroups(
  sectionId: string,
  fields: GuidedSettingsField[],
  t: TranslateFn
): SettingsModuleFieldGroup[] {
  const fieldsByKey = new Map(fields.map((field) => [field.key, field]));
  const claimedKeys = new Set<string>();
  const groups: SettingsModuleFieldGroup[] = [];

  for (const spec of SQUAD_GROUP_SPECS[sectionId] ?? []) {
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
      title: t(`squad.settings.groups.${spec.id}.title`, undefined, spec.title),
      description: t(`squad.settings.groups.${spec.id}.description`, undefined, spec.description),
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
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
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

export const squadSettingsDefinition: SettingsModuleDefinition = {
  id: "squad",
  getSections: buildSquadSections,
  buildFieldGroups: (sectionId, fields, _locale, t) => buildSquadFieldGroups(sectionId, fields, t),
  getFieldCopy: (key, t) => buildSquadFieldCopy(key, t),
  resolveFieldEditorVariant(key) {
    if (key === "map_rotation" || key === "admin_steam_ids" || key === "priority_join_steam_ids") {
      return "string-list";
    }
    if (key === "admin_permissions") {
      return "enum-check-list";
    }
    return undefined;
  },
  getFieldValidationMessage({ field, value, settings, t }) {
    if (field.key === "reserved_slots" || field.key === "max_players") {
      const reserved = readNumber(
        field.key === "reserved_slots" ? value : settings.reserved_slots
      );
      const maxPlayers = readNumber(field.key === "max_players" ? value : settings.max_players);
      if (reserved !== null && maxPlayers !== null && reserved > maxPlayers) {
        return t(
          "squad.settings.validation.reservedSlots",
          undefined,
          "Reserved slots cannot exceed max players."
        );
      }
    }

    if (field.key === "rcon_password" && readText(value).length === 0) {
      return t(
        "squad.settings.validation.rconPassword",
        undefined,
        "RCON password cannot be empty."
      );
    }

    if (field.key === "lan_only" || field.key === "advertise") {
      const lanOnly = readBoolean(field.key === "lan_only" ? value : settings.lan_only);
      const advertise = readBoolean(field.key === "advertise" ? value : settings.advertise);
      if (lanOnly && advertise) {
        return t(
          "squad.settings.validation.lanAdvertiseConflict",
          undefined,
          "LAN-only is enabled while advertisement is also enabled. Public listing usually needs LAN-only disabled."
        );
      }
    }

    return undefined;
  }
};
