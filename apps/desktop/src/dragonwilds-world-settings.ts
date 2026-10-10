import { invokeOrMock } from "./api-transport";
import catalog from "../../../modules/runescapedragonwilds/world-settings.json";

export type DragonwildsWorldMode = "Normal" | "Hard" | "Creative" | "Custom";

export interface DragonwildsWorldSettingDefinition {
  tag: string;
  kind: "boolean" | "number";
  player_adjustable: "Never" | "OnlyCustom" | "CustomAndCreative" | "AllModes";
  can_change_after_creation: boolean;
  minimum: number;
  maximum: number;
  decimal_places: number;
  gamepad_steps: number;
  postfix: string;
  preset_defaults: Record<Exclude<DragonwildsWorldMode, "Custom">, number>;
}

const permissions = ["Never", "OnlyCustom", "CustomAndCreative", "AllModes"] as const;
export const DRAGONWILDS_WORLD_SETTINGS: readonly DragonwildsWorldSettingDefinition[] = catalog.settings.map((setting) => {
  const permission = permissions.find((value) => value === setting.player_adjustable);
  if (!permission || (setting.kind !== "number" && setting.kind !== "boolean")) {
    throw new Error("The native Dragonwilds world catalog is invalid.");
  }
  return { ...setting, player_adjustable: permission, kind: setting.kind };
});

export const DRAGONWILDS_WORLD_CATEGORIES = [
  "survival", "player", "death", "magic", "building", "crafting", "progression", "creatures"
] as const;

export interface DragonwildsWorldSettingsSnapshot {
  instance_id: string;
  status: "ready" | "empty";
  world_file: string | null;
  world_name: string | null;
  world_mode: DragonwildsWorldMode | null;
  revision: string | null;
  values: Record<string, number>;
  overrides: Record<string, number>;
  definitions: DragonwildsWorldSettingDefinition[];
  writable: boolean;
  message: string | null;
  backup_id: string | null;
}

export interface WriteDragonwildsWorldSettingsInput {
  instance_id: string;
  world_file: string;
  expected_revision: string;
  world_mode: DragonwildsWorldMode;
  values: Record<string, number>;
}

export const readDragonwildsWorldSettings = (instanceId: string) =>
  invokeOrMock<DragonwildsWorldSettingsSnapshot>("read_dragonwilds_world_settings", { instanceId });

export const writeDragonwildsWorldSettings = (input: WriteDragonwildsWorldSettingsInput) =>
  invokeOrMock<DragonwildsWorldSettingsSnapshot>("write_dragonwilds_world_settings", { input });

export function isDragonwildsWorldSettingEditable(
  definition: DragonwildsWorldSettingDefinition, mode: DragonwildsWorldMode = "Custom"
): boolean {
  if (!definition.can_change_after_creation) return false;
  return definition.player_adjustable === "AllModes" ||
    (definition.player_adjustable === "OnlyCustom" && mode === "Custom") ||
    (definition.player_adjustable === "CustomAndCreative" && (mode === "Custom" || mode === "Creative"));
}

export function dragonwildsWorldSettingCopyKey(tag: string): string {
  const parts = tag.split(".");
  return parts[1] === "AI" && parts.length === 4 ? `ai.${parts[3]}` : parts[parts.length - 1] ?? tag;
}

export function dragonwildsWorldSettingGroup(tag: string): string {
  const parts = tag.split(".");
  if (parts[1] === "AI" && parts.length === 4) return `creatures.${parts[2]}`;
  if (parts[1] === "SurvivalCore") return "survival";
  const key = parts[parts.length - 1] ?? "";
  if (key === "FriendlyFire") return "player";
  if (/OnDeath|Gravestones/u.test(key)) return "death";
  if (/Spell|Teleportation/u.test(key)) return "magic";
  if (/Building/u.test(key)) return "building";
  if (/Crafting|Processing/u.test(key)) return "crafting";
  if (parts[1] === "Progression") return "progression";
  if (parts[1] === "Player") return "player";
  return "environment";
}

export function dragonwildsWorldSettingSection(tag: string): string {
  const category = dragonwildsWorldSettingGroup(tag).split(".")[0];
  return category === "environment" ? "world" : `world_${category}`;
}

export function dragonwildsWorldSettingValue(
  snapshot: DragonwildsWorldSettingsSnapshot,
  mode: DragonwildsWorldMode,
  definition: DragonwildsWorldSettingDefinition,
  draft: Readonly<Record<string, string>>
): string {
  const baseline = mode === "Custom" ? snapshot.values[definition.tag] :
    snapshot.overrides[definition.tag] ?? definition.preset_defaults[mode];
  return (isDragonwildsWorldSettingEditable(definition, mode) ? draft[definition.tag] : undefined) ??
    (baseline === undefined ? "" : String(Math.round(baseline * 10 ** definition.decimal_places) /
      10 ** definition.decimal_places));
}

export function isDragonwildsWorldSettingValueValid(
  definition: DragonwildsWorldSettingDefinition, raw: string
): boolean {
  if (raw.trim() === "") return false;
  const value = Number(raw);
  if (!Number.isFinite(value) || value < definition.minimum || value > definition.maximum) return false;
  if (definition.kind === "boolean") return value === 0 || value === 1;
  const scaled = value * 10 ** definition.decimal_places;
  return Math.abs(scaled - Math.round(scaled)) <= 1e-6;
}

// Only edited values are submitted. The native boundary preserves unknown settings
// and chooses the saved world's effective values when entering Custom mode.
export function buildDragonwildsWorldSettingsPatch(
  snapshot: DragonwildsWorldSettingsSnapshot,
  mode: DragonwildsWorldMode,
  draft: Readonly<Record<string, string>>
): Record<string, number> {
  const values: Record<string, number> = {};
  for (const definition of snapshot.definitions) {
    const raw = draft[definition.tag];
    if (raw === undefined || !isDragonwildsWorldSettingEditable(definition, mode)) continue;
    if (!isDragonwildsWorldSettingValueValid(definition, raw)) {
      throw new Error(`Invalid world setting: ${definition.tag}`);
    }
    const value = Number(raw);
    const baseline = mode === "Custom" ? snapshot.values[definition.tag] :
      snapshot.overrides[definition.tag] ?? definition.preset_defaults[mode];
    const precision = 10 ** definition.decimal_places;
    if (baseline === undefined || Math.round(value * precision) !== Math.round(baseline * precision)) {
      values[definition.tag] = value;
    }
  }
  return values;
}
