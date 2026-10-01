import { appendArkLine, arkLines, joinArkLines } from "./ark-native-ast";

export const ARK_STAT_FIELDS: Readonly<Record<string, string>> = {
  per_level_stats_multiplier_player_integer: "PerLevelStatsMultiplier_Player",
  per_level_stats_multiplier_dino_wild_integer: "PerLevelStatsMultiplier_DinoWild",
  per_level_stats_multiplier_dino_tamed_type_integer: "PerLevelStatsMultiplier_DinoTamed",
  player_base_stat_multipliers_attribute: "PlayerBaseStatMultipliers",
  item_stat_clamps_attribute: "ItemStatClamps",
  mutagen_level_boost_stat_id: "MutagenLevelBoost",
  mutagen_level_boost_bred_stat_id: "MutagenLevelBoost_Bred"
};
export const ARK_STAT_NAMES = ["health", "stamina", "torpidity", "oxygen", "food", "water", "temperature", "weight", "melee", "speed", "fortitude", "crafting"] as const;
export interface ArkStatEntry { index: number; variant: string; value: string; lineIndex: number; prefix: string; suffix: string }

export function readArkStats(raw: string, fieldKey: string): { entries: ArkStatEntry[] } {
  const nativeKey = ARK_STAT_FIELDS[fieldKey];
  if (!nativeKey) throw new Error("Unknown stat field.");
  const entries: ArkStatEntry[] = [];
  const seen = new Set<string>();
  for (const [lineIndex, line] of arkLines(raw).entries()) {
    if (!line.text.trim()) continue;
    const leading = line.text.match(/^\s*/)?.[0] ?? "";
    const body = line.text.trimStart().replace(new RegExp(`^${nativeKey}`), "");
    const match = body.match(/^(_Add|_Affinity)?(?:\[(\d+)\]|(\d+))\s*=\s*(.*?)\s*$/);
    if (!match) throw new Error(`Line ${lineIndex + 1}: expected [index]=number.`);
    const index = Number(match[2] ?? match[3]);
    const variant = match[1] ?? "";
    if (variant && fieldKey !== "per_level_stats_multiplier_dino_tamed_type_integer") throw new Error("Add and affinity are only available for tamed creatures.");
    if (!Number.isSafeInteger(index)) throw new Error("Invalid stat index.");
    if (seen.has(`${variant}:${index}`)) throw new Error(`Duplicate stat index ${variant}[${index}].`);
    seen.add(`${variant}:${index}`);
    const value = match[4];
    if (!value.trim() || !Number.isFinite(Number(value)) || Number(value) < 0) throw new Error(`Line ${lineIndex + 1}: use a finite, non-negative multiplier.`);
    const equalAt = line.text.indexOf("=") + 1;
    const afterEquals = line.text.slice(equalAt).match(/^\s*/)?.[0] ?? "";
    const suffix = line.text.match(/\s*$/)?.[0] ?? "";
    entries.push({ index, variant, value, lineIndex, prefix: line.text.slice(0, equalAt) + afterEquals || leading, suffix });
  }
  return { entries };
}

export function patchArkStat(raw: string, fieldKey: string, index: number, variant: string, value: string): string {
  const entries = readArkStats(raw, fieldKey).entries;
  const existing = entries.find((entry) => entry.index === index && entry.variant === variant);
  if (!existing) return value === "" ? raw : appendArkLine(raw, `${variant}[${index}]=${value}`);
  const lines = arkLines(raw);
  if (value === "") lines.splice(existing.lineIndex, 1);
  else lines[existing.lineIndex].text = existing.prefix + value + existing.suffix;
  return joinArkLines(lines);
}
