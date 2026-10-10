export const VALHEIM_MODIFIER_NAMES = ["combat", "deathpenalty", "resources", "raids", "portals"] as const;
export type ValheimModifierName = (typeof VALHEIM_MODIFIER_NAMES)[number];
export const VALHEIM_WORLD_KEYS = ["nobuildcost", "playerevents", "passivemobs", "nomap"] as const;
export type ValheimWorldKey = (typeof VALHEIM_WORLD_KEYS)[number];

// The native schema decides which name/value pairs are valid. This list only
// supplies display order and translation vocabulary, not a second pair matrix.
const MODIFIER_VALUES = [
  "none", "casual", "veryeasy", "easy", "hard", "veryhard", "hardcore",
  "muchless", "less", "more", "muchmore", "most"
];

export interface ValheimWorldRuleContract {
  presets: string[];
  modifiers: Record<ValheimModifierName, string[]>;
  keys: ValheimWorldKey[];
}

function object(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("Invalid Valheim world rule schema");
  }
  return value as Record<string, unknown>;
}

export function readValheimWorldRuleContract(schemaJson: string): ValheimWorldRuleContract {
  const properties = object(object(JSON.parse(schemaJson) as unknown).properties);
  const presets = object(properties.world_preset).enum;
  const modifierPattern = object(properties.world_modifiers).pattern;
  const keyPattern = object(properties.world_set_keys).pattern;
  if (!Array.isArray(presets) || !presets.every((value): value is string => typeof value === "string") ||
      typeof modifierPattern !== "string" || typeof keyPattern !== "string") {
    throw new Error("Missing Valheim world rule choices");
  }
  const modifiers = {} as Record<ValheimModifierName, string[]>;
  const modifierRule = new RegExp(modifierPattern);
  for (const name of VALHEIM_MODIFIER_NAMES) {
    modifiers[name] = MODIFIER_VALUES.filter((value) => modifierRule.test(`${name} ${value}`));
    if (modifiers[name].length === 0) throw new Error(`Missing Valheim modifier choices: ${name}`);
  }
  const keyRule = new RegExp(keyPattern);
  return { presets, modifiers, keys: VALHEIM_WORLD_KEYS.filter((key) => keyRule.test(key)) };
}

export function parseValheimRuleEntries(value: unknown): string[] {
  if (value !== undefined && typeof value !== "string") {
    throw new Error("Valheim world rules must be stored as text");
  }
  return typeof value === "string"
    ? value.split(/[\r\n,;]+/).map((entry) => entry.trim()).filter(Boolean)
    : [];
}

export function readValheimModifier(value: unknown, name: ValheimModifierName): string {
  // Repeated native arguments apply in order. Reflect the final entry without
  // rewriting the stored list just because the editor was opened.
  const entries = parseValheimRuleEntries(value).filter((entry) => entry.split(/\s+/)[0] === name);
  return entries.length ? entries[entries.length - 1].split(/\s+/).slice(1).join(" ") : "";
}

export function replaceValheimModifier(value: unknown, name: ValheimModifierName, next: string): string {
  const entries = parseValheimRuleEntries(value).filter((entry) => entry.split(/\s+/)[0] !== name);
  if (next) entries.push(`${name} ${next}`);
  return entries.join("\n");
}

export function replaceValheimWorldKey(value: unknown, key: ValheimWorldKey, enabled: boolean): string {
  const entries = parseValheimRuleEntries(value).filter((entry) => entry !== key);
  if (enabled) entries.push(key);
  return entries.join("\n");
}

export function isValheimModifierEntry(entry: string, contract: ValheimWorldRuleContract): boolean {
  const [name, value, extra] = entry.split(/\s+/);
  return extra === undefined && VALHEIM_MODIFIER_NAMES.some((candidate) =>
    candidate === name && contract.modifiers[candidate].includes(value));
}
import nativeRules from "../../../../../modules/valheim/world-rules.json";


const PRESET_KEYS: Readonly<Record<string, readonly string[]>> = nativeRules.presets;
const MODIFIER_KEYS: Readonly<Record<ValheimModifierName, Readonly<Record<string, readonly string[]>>>> = nativeRules.modifiers;

export function valheimPresetKeys(preset: string): readonly string[] | null {
  return Object.prototype.hasOwnProperty.call(PRESET_KEYS, preset) ? PRESET_KEYS[preset] : null;
}

export function resolveSavedValheimKeys(keys: readonly string[]): string[] {
  const resolved = keys.map((key) => key.toLowerCase());
  const entries = resolved.filter((key) => key.startsWith("preset "));
  const preset = entries.length ? entries[entries.length - 1].slice(7) : "";
  if (!preset) return resolved;
  // ZoneSystem.SetStartingGlobalKeys also applies the serialized preset tag.
  // Some imported metadata carries this tag without expanded concrete keys.
  // Native KeyButton/KeySlider add or replace only the keys they define; they
  // do not clear unrelated keys or turn off existing checkbox rules.
  const apply = (values: readonly string[]) => {
    for (const value of values) {
      const name = value.split(" ")[0];
      for (let index = resolved.length - 1; index >= 0; index--) {
        if (resolved[index].split(" ")[0] === name) resolved.splice(index, 1);
      }
      resolved.push(value);
    }
  };
  const values = valheimPresetKeys(preset);
  if (values) apply(values);
  else for (const segment of preset.split(":")) {
    const [name, choice, extra] = segment.split("_");
    const modifier = VALHEIM_MODIFIER_NAMES.find((candidate) => candidate === name);
    if (extra !== undefined || !modifier) continue;
    const options = MODIFIER_KEYS[modifier];
    const values = options[choice === "default" ? "normal" : choice];
    if (values) apply(values);
  }
  return resolved;
}

export function readSavedValheimPreset(keys: readonly string[]): string {
  const rules = resolveSavedValheimKeys(keys).filter((key) => !key.startsWith("preset "));
  for (const [preset, values] of Object.entries(PRESET_KEYS)) {
    if (rules.length === values.length && values.every((key) => rules.includes(key))) return preset;
  }
  return "custom";
}

export function readSavedValheimModifier(keys: readonly string[], name: ValheimModifierName): string {
  const options = MODIFIER_KEYS[name];
  const names = new Set(Object.values(options).flatMap((values) => values.map((key) => key.split(" ")[0])));
  const rules = resolveSavedValheimKeys(keys).filter((key) => names.has(key.split(" ")[0]));
  for (const [value, keys] of Object.entries(options)) {
    if (rules.length === keys.length && keys.every((key) => rules.includes(key))) return value;
  }
  return "custom";
}
