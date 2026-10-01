import type { SettingsObject } from "../settings/settings-schema";

function hasSetting(settings: SettingsObject, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(settings, key);
}

function normalizeValue(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map(normalizeValue);
  }
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>)
        .sort(([left], [right]) => left.localeCompare(right))
        .map(([key, entry]) => [key, normalizeValue(entry)])
    );
  }
  return value;
}

function settingEquals(left: SettingsObject, right: SettingsObject, key: string): boolean {
  if (hasSetting(left, key) !== hasSetting(right, key)) {
    return false;
  }
  if (!hasSetting(left, key)) {
    return true;
  }
  return JSON.stringify(normalizeValue(left[key])) === JSON.stringify(normalizeValue(right[key]));
}

export function changedSettingKeys(
  persistedSettings: SettingsObject,
  draftSettings: SettingsObject,
  candidateKeys: readonly string[]
): string[] {
  return Array.from(new Set(candidateKeys)).filter(
    (key) => !settingEquals(persistedSettings, draftSettings, key)
  );
}

export function mergeSettingPatch(
  latestSettings: SettingsObject,
  draftSettings: SettingsObject,
  dirtyKeys: readonly string[]
): SettingsObject {
  const mergedSettings = { ...latestSettings };
  for (const key of Array.from(new Set(dirtyKeys))) {
    if (hasSetting(draftSettings, key)) {
      mergedSettings[key] = draftSettings[key];
    } else {
      delete mergedSettings[key];
    }
  }
  return mergedSettings;
}
