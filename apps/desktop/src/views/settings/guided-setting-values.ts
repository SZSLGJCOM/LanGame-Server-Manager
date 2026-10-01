import type { GuidedSettingsField } from "./settings-schema";

function compareValue(left: unknown, right: unknown): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

export function findEnumOptionIndex(field: GuidedSettingsField, value: unknown): number {
  return field.enumOptions?.findIndex((option) => compareValue(option.value, value)) ?? -1;
}

export function isOptionalBooleanOverride(field: GuidedSettingsField): boolean {
  return field.type === "boolean" && !field.required &&
    (field.preserveNativeWhenUnset === true ||
      field.defaultValue === undefined && field.defaultSource === undefined);
}

export function parseWorkshopIdList(rawValue: unknown): string[] {
  if (typeof rawValue !== "string" || rawValue.trim().length === 0) {
    return [];
  }

  const seen = new Set<string>();
  const ids: string[] = [];
  const lines = rawValue
    .replace(/\r\n/g, "\n")
    .replace(/\r/g, "\n")
    .split("\n");

  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#") || trimmed.startsWith("--")) {
      continue;
    }

    for (const entry of trimmed.split(",")) {
      const value = entry.trim();
      if (!value || value.startsWith("#") || value.startsWith("--")) {
        continue;
      }
      const match = value.match(/\d{6,}/);
      if (!match) {
        continue;
      }

      const id = match[0];
      if (seen.has(id)) {
        continue;
      }

      seen.add(id);
      ids.push(id);
    }
  }

  return ids;
}
export function serializeWorkshopIdList(ids: string[]): string {
  return ids.join("\n");
}
