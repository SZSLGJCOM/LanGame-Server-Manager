import { arkLines, parseArkRule, type ArkNode } from "./ark-native-ast";
import { readArkEngramPoints, readArkLevels } from "./ark-level-model";
import { ARK_STAT_FIELDS, readArkStats } from "./ark-stat-model";
import { ARK_RULE_FIELDS } from "./ark-rule-fields";

const NON_NEGATIVE = new Set(["SpawnWeightMultiplier", "SpawnLimitPercentage", "EntryWeight", "MaxPercentageOfDesiredNumToAllow", "LevelToAutoUnlock", "EngramPointsCost", "EngramLevelRequirement", "EngramIndex", "MinItemSets", "MaxItemSets", "MinNumItems", "MaxNumItems", "SetWeight", "MinQuantity", "MaxQuantity", "MinQuality", "MaxQuality", "BaseResourceRequirement", "MaxItemQuantity", "NumItemSetsPower", "NumItemsPower", "Weight", "ResourceItemAmount", "Multiplier", "ChanceToBeBlueprintOverride"]);
const INTEGERS = new Set(["LevelToAutoUnlock", "EngramPointsCost", "EngramLevelRequirement", "EngramIndex", "MinItemSets", "MaxItemSets", "MinNumItems", "MaxNumItems", "MaxItemQuantity"]);
const PROBABILITIES = new Set(["SpawnLimitPercentage", "MaxPercentageOfDesiredNumToAllow", "ChanceToBeBlueprintOverride"]);
const NATIVE_CONTAINERS = new Set(["ItemSets", "ItemEntries", "BaseCraftingResourceRequirements", "Quantity", "NPCSpawnEntries", "NPCSpawnLimits"]);
function checkNumbers(node: ArkNode): void {
  if (node.kind !== "group") return;
  for (const entry of node.entries) {
    if (entry.key && NON_NEGATIVE.has(entry.key)) {
      const value = entry.value.kind === "scalar" ? Number(entry.value.raw) : NaN;
      if (!Number.isFinite(value) || value < 0) throw new Error(`${entry.key} must be a finite, non-negative number.`);
      if (INTEGERS.has(entry.key) && !Number.isSafeInteger(value)) throw new Error(`${entry.key} must be an integer.`);
      if (PROBABILITIES.has(entry.key) && value > 1) throw new Error(`${entry.key} must be at most 1.`);
    }
    if (entry.key === "ItemsWeights" || entry.key === "ItemWeights") {
      if (entry.value.kind !== "group" || entry.value.entries.some((item) => item.value.kind !== "scalar" || !Number.isFinite(Number(item.value.raw)) || Number(item.value.raw) < 0)) throw new Error("Item weights must be non-negative numbers.");
    }
    if (!entry.key || NATIVE_CONTAINERS.has(entry.key)) checkNumbers(entry.value);
  }
}

export function validateArkComplexField(key: string, value: unknown): string | undefined {
  if (typeof value !== "string" || !value.trim()) return undefined;
  try {
    if (ARK_STAT_FIELDS[key]) readArkStats(value, key);
    else if (key === "level_experience_ramp_overrides") {
      if (readArkLevels(value).curves.some((curve) => curve.levels.length === 0)) throw new Error("Each experience curve must define at least one level.");
    }
    else if (key === "override_player_level_engram_points") readArkEngramPoints(value);
    else if (ARK_RULE_FIELDS[key]) {
      for (const line of arkLines(value)) if (line.text.trim()) checkNumbers(parseArkRule(line.text, ARK_RULE_FIELDS[key].nativeKey));
    }
  } catch (error) { return error instanceof Error ? error.message : String(error); }
  return undefined;
}
