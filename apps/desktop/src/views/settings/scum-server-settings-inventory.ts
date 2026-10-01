import generalInventory from "../../../../../modules/scum/server-settings-v7/general.json";
import worldInventory from "../../../../../modules/scum/server-settings-v7/world.json";
import featuresInventory from "../../../../../modules/scum/server-settings-v7/features.json";
import respawnInventory from "../../../../../modules/scum/server-settings-v7/respawn.json";
import vehiclesInventory from "../../../../../modules/scum/server-settings-v7/vehicles.json";
import damageInventory from "../../../../../modules/scum/server-settings-v7/damage.json";
import economyDefaults from "../../../../../modules/scum/native-defaults/EconomyOverride.json";
import raidDefaults from "../../../../../modules/scum/native-defaults/RaidTimes.json";
import notificationDefaults from "../../../../../modules/scum/native-defaults/Notifications.json";
import type { TranslateFn } from "../../i18n";
import { scumNativeMessageKey } from "../../i18n/games/scum-native-messages";
import type {
  ConfigurationPresentationField,
  GuidedSettingsValidationIssue,
  SettingsObject
} from "./settings-schema";

export type ScumNativeSection =
  | "General"
  | "World"
  | "Features"
  | "Respawn"
  | "Vehicles"
  | "Damage";
export type ScumNativeValueType = "boolean" | "integer" | "number" | "string";

export interface ScumNativeSetting {
  section: ScumNativeSection;
  key: string;
  nativeKey: string;
  title: string;
  type: ScumNativeValueType;
  default: boolean | number | string;
  nativeDefault: string;
  presentation: "specialized" | "generated";
  minimum?: number;
  maximum?: number;
  risk?: "confirmation";
}

export const SCUM_SECTION_FIELDS: Readonly<Record<ScumNativeSection, string>> = {
  General: "server_general",
  World: "server_world",
  Features: "server_features",
  Respawn: "server_respawn",
  Vehicles: "server_vehicles",
  Damage: "server_damage"
};

const INVENTORIES = [
  generalInventory,
  worldInventory,
  featuresInventory,
  respawnInventory,
  vehiclesInventory,
  damageInventory
] as unknown as readonly ScumNativeSetting[][];

export const SCUM_NATIVE_SETTINGS: readonly ScumNativeSetting[] = INVENTORIES.flat();
export const SCUM_EDITABLE_SETTINGS = SCUM_NATIVE_SETTINGS.filter(
  (setting) => setting.presentation === "specialized"
);

export function scumInventoryForSection(section: ScumNativeSection): readonly ScumNativeSetting[] {
  return SCUM_NATIVE_SETTINGS.filter((setting) => setting.section === section);
}

export function scumSectionForField(fieldKey: string | undefined): ScumNativeSection | null {
  for (const [section, candidate] of Object.entries(SCUM_SECTION_FIELDS)) {
    if (candidate === fieldKey) return section as ScumNativeSection;
  }
  return null;
}

export function scumVirtualFieldKey(setting: ScumNativeSetting): string {
  return `${SCUM_SECTION_FIELDS[setting.section]}.${setting.key}`;
}

export const SCUM_CORE_SECTION_IDS = ["room", "network", "access", "runtime"] as const;
type ScumConfigurationSection = typeof SCUM_CORE_SECTION_IDS[number] | Lowercase<ScumNativeSection> | "maintenance";
export const SCUM_WIPE_KEYS = ["partial_wipe", "gold_wipe", "full_wipe"] as const;
export function isScumWipeKey(key: string): boolean {
  return SCUM_WIPE_KEYS.some((candidate) => candidate === key);
}

const NETWORK_KEYS = new Set([
  "max_ping_check_enabled", "max_ping", "max_number_of_consecutive_high_ping_readings",
  "master_server_update_send_interval"
]);
const RUNTIME_KEYS = new Set([
  "min_server_tick_rate", "max_server_tick_rate", "master_server_is_local_test",
  "rusty_locks_logging", "log_suicides",
  "delete_inactive_users", "days_since_last_login_to_become_inactive", "delete_banned_users",
  "maximum_time_for_chests_in_forbidden_zones", "log_chest_ownership",
  "delete_duplicate_chests_on_server_startup", "item_virtualization_relevancy_update_period",
  "item_virtualization_event_processing_time_budget", "item_virtualization_visitor_distance_travelled_for_update",
  "item_virtualization_visitor_bounds", "virtualized_item_bounds", "enable_net_watchdog",
  "enable_network_object_logging"
]);

export function scumPresentationSectionId(setting: ScumNativeSetting): ScumConfigurationSection {
  // Room navigation is independent of the native section that owns persistence.
  if (isScumWipeKey(setting.key)) return "maintenance";
  if (scumGroupId(setting) === "identity") return "room";
  if (NETWORK_KEYS.has(setting.key)) return "network";
  if (setting.key === "play_safe_id_protection") return "access";
  if (RUNTIME_KEYS.has(setting.key)) return "runtime";
  return setting.section.toLowerCase() as Lowercase<ScumNativeSection>;
}

export function scumRendererId(section: ScumNativeSection | ScumConfigurationSection): string {
  return `scum-server-${section.toLowerCase()}`;
}

export function buildScumNativePresentationFields(t?: TranslateFn): ConfigurationPresentationField[] {
  return SCUM_NATIVE_SETTINGS.map((setting, index) => ({
    key: scumVirtualFieldKey(setting),
    title: setting.presentation === "generated"
      ? t?.("scum.settings.generated.versionTitle", undefined, setting.title) ?? setting.title
      : t?.(scumNativeMessageKey(setting.key, "title"), undefined, setting.title) ?? setting.title,
    description: setting.presentation === "generated"
      ? t?.(
          "scum.settings.generated.versionDescription",
          undefined,
          "SCUM owns this generated configuration version marker."
        ) ?? "SCUM owns this generated configuration version marker."
      : t?.(
          scumNativeMessageKey(setting.key, "description"),
          undefined,
          ""
        ) ?? "",
    sectionId: scumPresentationSectionId(setting),
    sortWeight: index,
    sourceId: "generated_current_server_settings",
    sourceKey: setting.nativeKey,
    sourceSurface: "config_file",
    presentation: setting.presentation === "generated"
      ? {
          state: "generated",
          owner: "configuration",
          sectionId: scumPresentationSectionId(setting),
          reason: t?.(
            "scum.settings.generated.versionReason",
            undefined,
            "SCUM writes and migrates the ServerSettings version marker."
          ) ?? "SCUM writes and migrates the ServerSettings version marker."
        }
      : isScumWipeKey(setting.key)
      ? { state: "editable", owner: "maintenance", sectionId: "maintenance", restartScope: "server" }
      : {
          state: "specialized",
          owner: "configuration",
          sectionId: scumPresentationSectionId(setting),
          rendererId: scumRendererId(scumPresentationSectionId(setting)),
          rendererFieldKey: SCUM_SECTION_FIELDS[setting.section],
          restartScope: "server"
        }
  }));
}

function isRecord(value: unknown): value is SettingsObject {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function cloneJson<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

function defaultSectionSettings(section: ScumNativeSection): SettingsObject {
  return Object.fromEntries(
    scumInventoryForSection(section)
      .filter((setting) => setting.presentation === "specialized")
      .map((setting) => [setting.key, setting.default])
  );
}

export function initializeScumSettings(settings: Readonly<SettingsObject>): SettingsObject {
  const initialized: SettingsObject = { ...settings };
  for (const [section, fieldKey] of Object.entries(SCUM_SECTION_FIELDS)) {
    const current = isRecord(settings[fieldKey]) ? settings[fieldKey] : {};
    initialized[fieldKey] = {
      ...defaultSectionSettings(section as ScumNativeSection),
      ...current
    };
  }
  const economy = isRecord(settings.economy_override)
    ? settings.economy_override
    : (economyDefaults as SettingsObject)["economy-override"];
  initialized.economy_override = cloneJson(economy ?? {});
  initialized.raid_times = cloneJson(
    Array.isArray(settings.raid_times)
      ? settings.raid_times
      : (raidDefaults as SettingsObject)["raiding-times"] ?? []
  );
  initialized.notifications = cloneJson(
    Array.isArray(settings.notifications)
      ? settings.notifications
      : (notificationDefaults as SettingsObject).Notifications ?? []
  );
  return initialized;
}

export function readScumNativeValue(
  settings: Readonly<SettingsObject>,
  setting: ScumNativeSetting
): unknown {
  const fieldKey = SCUM_SECTION_FIELDS[setting.section];
  const section = isRecord(settings[fieldKey]) ? settings[fieldKey] : {};
  return section[setting.key] ?? setting.default;
}

export function patchScumNativeValue(
  settings: Readonly<SettingsObject>,
  setting: ScumNativeSetting,
  value: unknown
): SettingsObject {
  const fieldKey = SCUM_SECTION_FIELDS[setting.section];
  const section = isRecord(settings[fieldKey]) ? settings[fieldKey] : {};
  return { [fieldKey]: { ...section, [setting.key]: value } };
}

export function validateScumStructuredSettings(
  settings: Readonly<SettingsObject>,
  t: TranslateFn = (key, _params, fallback) => fallback ?? key
): GuidedSettingsValidationIssue[] {
  const issues: GuidedSettingsValidationIssue[] = [];
  for (const setting of SCUM_EDITABLE_SETTINGS) {
    const value = readScumNativeValue(settings, setting);
    const fieldKey = scumVirtualFieldKey(setting);
    const title = t(scumNativeMessageKey(setting.key, "title"), undefined, setting.title);
    if (!nativeValueMatches(setting.type, value)) {
      issues.push({
        fieldKey,
        reason: "native_type",
        message: t(
          "scum.settings.validation.nativeType",
          {
            title,
            type: t(`scum.settings.valueType.${setting.type}`, undefined, setting.type)
          },
          `${title} must be ${setting.type}.`
        )
      });
      continue;
    }
    if (typeof value === "number" && setting.minimum !== undefined && value < setting.minimum) {
      issues.push({ fieldKey, reason: "minimum", message: t(
        "scum.settings.validation.minimum",
        { title, minimum: setting.minimum },
        `${title} must be at least ${setting.minimum}.`
      ) });
    }
    if (typeof value === "number" && setting.maximum !== undefined && value > setting.maximum) {
      issues.push({ fieldKey, reason: "maximum", message: t(
        "scum.settings.validation.maximum",
        { title, maximum: setting.maximum },
        `${title} must be at most ${setting.maximum}.`
      ) });
    }
    if (typeof value === "string" && /[\r\n]/u.test(value)) {
      issues.push({ fieldKey, reason: "line_break", message: t(
        "scum.settings.validation.singleLine",
        { title },
        `${title} must stay on one INI line.`
      ) });
    }
  }
  return issues;
}

function nativeValueMatches(type: ScumNativeValueType, value: unknown): boolean {
  if (type === "boolean") return typeof value === "boolean";
  if (type === "integer") return typeof value === "number" && Number.isInteger(value);
  if (type === "number") return typeof value === "number" && Number.isFinite(value);
  return typeof value === "string";
}

export function scumGroupId(setting: ScumNativeSetting): string {
  const key = setting.nativeKey;
  if (setting.risk) return "maintenance-risk";
  if (setting.section === "General") {
    if (/ServerName|ServerDescription|ServerPassword|MaxPlayers|Welcome|Banner|Playstyle|MessageOfTheDay/u.test(key)) return "identity";
    if (/Chat|Voting|Vote|KillNotification/u.test(key)) return "communication";
    if (/TickRate|Ping|Virtualization|MasterServer/u.test(key)) return "performance";
    if (/Delete|Wipe|Inactive|Chest|Log/u.test(key)) return "retention-logging";
    return "gameplay-access";
  }
  if (setting.section === "World") {
    if (/Animal|Bear|Boar|Chicken|Deer|Donkey|Goat|Horse|Rabbit|Wolf|Fishing/u.test(key)) return "wildlife";
    if (/Puppet|NPC|Sentry|Dropship|Encounter/u.test(key)) return "encounters";
    if (/TimeOfDay|Nighttime|Sunrise|Sunset|Fog/u.test(key)) return "time-weather";
    if (/CargoDrop/u.test(key)) return "cargo-drops";
    if (/Bunker|Killbox|Keycard/u.test(key)) return "bunkers";
    return "world-rules";
  }
  if (setting.section === "Features") {
    if (/Flag|Base|Building|Raid|Chest|Rack|Well|Turret|Oven|Garden|Tombstone|Smoker/u.test(key)) return "building-raiding";
    if (/Water|Gasoline|Propane/u.test(key)) return "resources";
    if (/Spawner|ItemCooldown|Harvest/u.test(key)) return "items";
    if (/Squad/u.test(key)) return "squads";
    if (/SkillMultiplier|SelectedSkills/u.test(key)) return "skills";
    if (/Quest/u.test(key)) return "quests";
    if (/Network|Watchdog|Logging/u.test(key)) return "diagnostics";
    return "survival-features";
  }
  if (setting.section === "Respawn") {
    if (/Price/u.test(key)) return "prices";
    if (/Cooldown/u.test(key)) return "cooldowns";
    return "spawn-rules";
  }
  if (setting.section === "Vehicles") {
    if (/Battery|Fuel/u.test(key)) return "energy";
    if (/MaximumTime|Log/u.test(key)) return "lifecycle";
    return "fleet-limits";
  }
  if (/HumanToHuman/u.test(key)) return "pvp";
  if (/Decay|LockProtection/u.test(key)) return "decay";
  return "npc-structures";
}
