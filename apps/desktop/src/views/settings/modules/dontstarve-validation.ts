import dontStarveSchema from "../../../../../../modules/dontstarve/schema.json";
import type { TranslateFn } from "../../../i18n";
import type { GuidedSettingsValidationIssue, SettingsObject } from "../settings-schema";
import { luaScalar, parseLuaDataTable } from "./dontstarve-lua-data";
import { getDontStarveWorldScriptMode } from "./dontstarve-world-lua";
import { DONTSTARVE_SHARD_NAMES, getDontStarveLayoutShards, isDontStarveShardActive } from "./dontstarve-shards";

type DstSchemaProperty = {
  default?: unknown;
};

const SCHEMA_PROPERTIES = dontStarveSchema.properties as Record<string, DstSchemaProperty>;

export function getDontStarveSettingsValidationIssues(
  settings: Readonly<SettingsObject>,
  t: TranslateFn
): GuidedSettingsValidationIssue[] {
  const issues: GuidedSettingsValidationIssue[] = [];
  const legacyPreset = settings.game_mode === "endless" ? "ENDLESS"
    : settings.game_mode === "wilderness" ? "WILDERNESS" : null;
  const masterPresets = explicitMasterPresets(settings);
  if (legacyPreset && masterPresets.some((preset) =>
    preset !== undefined && preset !== "SURVIVAL_TOGETHER" && preset !== legacyPreset)) {
    issues.push(issue("game_mode", "conflicting-playstyle", t, "dst.settings.validation.playstyleConflict",
      "Set the cluster game mode to Survival before choosing a different Master playstyle preset."));
  }

  for (const shard of getDontStarveLayoutShards(settings)) {
    if (!isDontStarveShardActive(settings, shard)) continue;
    const contract = { shard: DONTSTARVE_SHARD_NAMES[shard], enabledField: `${shard}_enabled_workshop_mod_ids`,
      configurationField: `${shard}_mod_configuration_options`, rawModField: `${shard}_modoverrides_lua` };
    const configurationIssue = validateModConfiguration(
      settings[contract.configurationField],
      contract.configurationField,
      contract.shard,
      t
    );
    if (configurationIssue) issues.push(configurationIssue);

    if (
      isCustomRawValue(settings, contract.rawModField) &&
      (hasListEntries(settings[contract.enabledField]) || hasRecordEntries(settings[contract.configurationField]))
    ) {
      issues.push(issue(
        contract.rawModField,
        "raw-structured-conflict",
        t,
        "dst.settings.validation.rawModConflict",
        `${contract.shard} raw modoverrides.lua cannot be combined with the enabled Mod list or structured Mod options.`
      ));
    }
  }

  return issues;
}

export function applyDontStarveOperationalPatch(
  settings: Readonly<SettingsObject>,
  patch: Readonly<SettingsObject>
): SettingsObject {
  const normalized = { ...patch };
  const nextSettings = { ...settings, ...patch };
  const rawChanged = Object.prototype.hasOwnProperty.call(patch, "master_worldgenoverride_lua") &&
    patch.master_worldgenoverride_lua !== settings.master_worldgenoverride_lua;
  if (!rawChanged && !getDontStarveWorldScriptMode(nextSettings, "master")) {
    if (patch.game_mode === "endless" || patch.game_mode === "wilderness") {
      normalized.master_settings_preset = patch.game_mode.toUpperCase();
      normalized.master_worldgen_preset = patch.game_mode.toUpperCase();
    } else if ("master_settings_preset" in patch || "master_worldgen_preset" in patch) {
      normalized.game_mode = "survival";
    }
  }
  if (patch.disable_data_collection === true) {
    normalized.offline_cluster = true;
  } else if (patch.offline_cluster === false) {
    normalized.disable_data_collection = false;
  }
  return normalized;
}

function explicitMasterPresets(settings: Readonly<SettingsObject>): (string | undefined)[] {
  if (!isCustomRawValue(settings, "master_worldgenoverride_lua")) {
    return [settings.master_settings_preset, settings.master_worldgen_preset]
      .map((value) => typeof value === "string" ? value : undefined);
  }
  if (getDontStarveWorldScriptMode(settings, "master")) return [];
  const raw = settings.master_worldgenoverride_lua;
  const table = parseLuaDataTable(typeof raw === "string" ? raw : "");
  const fallback = luaScalar(table, "preset");
  return ["settings_preset", "worldgen_preset"].map((key) => {
    const value = luaScalar(table, key) ?? fallback;
    return typeof value === "string" ? value : undefined;
  });
}

function validateModConfiguration(
  value: unknown,
  fieldKey: string,
  shard: string,
  t: TranslateFn
): GuidedSettingsValidationIssue | null {
  if (value === undefined) return null;
  if (!isRecord(value)) {
    return invalidModConfigurationIssue(fieldKey, shard, t);
  }
  for (const options of Object.values(value)) {
    if (!isRecord(options)) {
      return invalidModConfigurationIssue(fieldKey, shard, t);
    }
    for (const optionValue of Object.values(options)) {
      if (!isDstModPrimitive(optionValue)) {
        return invalidModConfigurationIssue(fieldKey, shard, t);
      }
    }
  }
  return null;
}

function invalidModConfigurationIssue(
  fieldKey: string,
  shard: string,
  t: TranslateFn
): GuidedSettingsValidationIssue {
  return issue(
    fieldKey,
    "invalid-mod-option-value",
    t,
    "dst.settings.validation.invalidModOptionValue",
    `${shard} Mod options must be a Workshop-ID map whose values are strings, finite numbers, or booleans.`
  );
}

export function isCustomRawValue(settings: Readonly<SettingsObject>, fieldKey: string): boolean {
  const value = settings[fieldKey];
  if (!hasText(value)) return false;
  return normalizeText(value) !== normalizeText(SCHEMA_PROPERTIES[fieldKey]?.default);
}

function hasListEntries(value: unknown): boolean {
  if (Array.isArray(value)) return value.length > 0;
  return typeof value === "string" && value
    .split(/[\s,;]+/u)
    .some((entry) => entry.trim().length > 0);
}

function hasRecordEntries(value: unknown): boolean {
  return isRecord(value) && Object.keys(value).length > 0;
}

function hasText(value: unknown): value is string {
  return typeof value === "string" && value.trim().length > 0;
}

function normalizeText(value: unknown): string {
  return typeof value === "string" ? value.replace(/\r\n?/gu, "\n").trim() : "";
}

function isDstModPrimitive(value: unknown): boolean {
  return typeof value === "string" || typeof value === "boolean" ||
    (typeof value === "number" && Number.isFinite(value));
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function issue(
  fieldKey: string,
  reason: string,
  t: TranslateFn,
  messageKey: string,
  fallback: string
): GuidedSettingsValidationIssue {
  return { fieldKey, reason, message: t(messageKey, undefined, fallback) };
}
