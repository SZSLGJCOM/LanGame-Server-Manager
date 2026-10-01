import vrisingSchema from "../../../../../../modules/vrising/schema.json";
import type { SettingsObject } from "../settings-schema";

type VRisingTypedFieldType = "boolean" | "integer" | "number" | "string";

interface VRisingTypedGameSettingDescriptor {
  fieldKey: string;
  jsonPath: string[];
  type: VRisingTypedFieldType;
}

const SCHEMA_PROPERTIES: Readonly<Record<string, {
  type: string;
  enum?: readonly unknown[];
  minimum?: number;
  maximum?: number;
  "x-lsgm-source"?: string;
  "x-lsgm-source-key"?: string;
}>> = vrisingSchema.properties;

const VRISING_GAME_DIFFICULTY_VALUE_BY_LABEL: Record<string, number> = {
  Easy: 0,
  Normal: 1,
  Brutal: 2
};

const VRISING_GAME_DIFFICULTY_LABEL_BY_VALUE: Record<number, string> = {
  0: "Easy",
  1: "Normal",
  2: "Brutal"
};

const VRISING_CASTLE_HEART_LIMIT_TYPE_LABEL_BY_VALUE: Record<number, string> = {
  0: "User",
  1: "Clan"
};

const VRISING_CASTLE_HEART_LIMIT_TYPE_VALUE_BY_LABEL: Record<string, number> = {
  User: 0,
  Clan: 1
};

const VRISING_PLAYER_INTERACTION_TIME_ZONE_LABEL_BY_VALUE: Record<number, string> = {
  0: "Local",
  1: "UTC"
};

const VRISING_PLAYER_INTERACTION_TIME_ZONE_VALUE_BY_LABEL: Record<string, number> = {
  Local: 0,
  UTC: 1
};

const VRISING_TYPED_GAME_SETTINGS: readonly VRisingTypedGameSettingDescriptor[] = Object.entries(SCHEMA_PROPERTIES)
  .flatMap<VRisingTypedGameSettingDescriptor>(([fieldKey, property]) => {
    const type = property.type;
    const sourceKey = property["x-lsgm-source-key"];
    if (fieldKey === "server_game_settings_json" || property["x-lsgm-source"] !== "server_game_settings_json" ||
      !sourceKey || (type !== "boolean" && type !== "integer" && type !== "number" && type !== "string")) return [];
    return [{ fieldKey, jsonPath: sourceKey.split("."), type }];
  });

export const VRISING_TYPED_GAME_SETTING_KEYS = new Set(
  VRISING_TYPED_GAME_SETTINGS.map((descriptor) => descriptor.fieldKey)
);

function parseVRisingServerGameSettings(settings: SettingsObject): Record<string, unknown> | null {
  const raw = settings.server_game_settings_json;
  if (typeof raw !== "string") {
    return {};
  }

  try {
    const parsed = JSON.parse(raw);
    if (parsed === null || Array.isArray(parsed) || typeof parsed !== "object") {
      return null;
    }

    return { ...(parsed as Record<string, unknown>) };
  } catch {
    return null;
  }
}

function normalizeVRisingGameDifficulty(value: unknown): string | undefined {
  if (typeof value === "string" && Object.prototype.hasOwnProperty.call(VRISING_GAME_DIFFICULTY_VALUE_BY_LABEL, value)) {
    return value;
  }

  if (typeof value === "number" && Object.prototype.hasOwnProperty.call(VRISING_GAME_DIFFICULTY_LABEL_BY_VALUE, value)) {
    return VRISING_GAME_DIFFICULTY_LABEL_BY_VALUE[value];
  }

  return undefined;
}

function serializeVRisingGameDifficulty(value: unknown): number | undefined {
  if (typeof value !== "string") {
    return undefined;
  }

  return VRISING_GAME_DIFFICULTY_VALUE_BY_LABEL[value];
}

function normalizeVRisingStringEnum(
  value: unknown,
  labelByValue: Record<number, string>,
  valueByLabel: Record<string, number>
): string | undefined {
  if (typeof value === "string" && Object.prototype.hasOwnProperty.call(valueByLabel, value)) {
    return value;
  }

  if (typeof value === "number" && Object.prototype.hasOwnProperty.call(labelByValue, value)) {
    return labelByValue[value];
  }

  return undefined;
}

function isValidTypedValue(value: unknown, type: string): boolean {
  if (type === "boolean") {
    return typeof value === "boolean";
  }

  if (type === "integer") {
    return typeof value === "number" && Number.isSafeInteger(value);
  }

  if (type === "number") {
    return typeof value === "number" && Number.isFinite(value);
  }

  return type === "string" && typeof value === "string";
}

function isValidManagedTypedValue(fieldKey: string, value: unknown): boolean {
  const property = SCHEMA_PROPERTIES[fieldKey];
  return isValidTypedValue(value, property.type) &&
    (!property.enum || property.enum.includes(value)) &&
    (typeof value !== "number" || (
      (property.minimum === undefined || value >= property.minimum) &&
      (property.maximum === undefined || value <= property.maximum)
    ));
}

function readNestedValue(root: Record<string, unknown>, path: string[]): unknown {
  let current: unknown = root;

  for (const segment of path) {
    if (!current || Array.isArray(current) || typeof current !== "object") {
      return undefined;
    }
    current = (current as Record<string, unknown>)[segment];
  }

  return current;
}

function readDescriptorValue(
  descriptor: VRisingTypedGameSettingDescriptor,
  root: Record<string, unknown>
): unknown {
  const value = readNestedValue(root, descriptor.jsonPath);
  if (descriptor.fieldKey === "game_difficulty") {
    return normalizeVRisingGameDifficulty(value);
  }

  if (descriptor.fieldKey === "castle_heart_limit_type") {
    return normalizeVRisingStringEnum(
      value,
      VRISING_CASTLE_HEART_LIMIT_TYPE_LABEL_BY_VALUE,
      VRISING_CASTLE_HEART_LIMIT_TYPE_VALUE_BY_LABEL
    );
  }

  if (descriptor.fieldKey === "player_interaction_time_zone") {
    return normalizeVRisingStringEnum(
      value,
      VRISING_PLAYER_INTERACTION_TIME_ZONE_LABEL_BY_VALUE,
      VRISING_PLAYER_INTERACTION_TIME_ZONE_VALUE_BY_LABEL
    );
  }

  return value;
}

export function findVRisingInvalidNativeValue(root: Record<string, unknown>): string | null {
  for (const descriptor of VRISING_TYPED_GAME_SETTINGS) {
    let current: unknown = root;
    let present = true;
    for (const [index, segment] of descriptor.jsonPath.entries()) {
      if (current === null || Array.isArray(current) || typeof current !== "object") {
        return descriptor.jsonPath.slice(0, index).join(".");
      }
      if (!Object.prototype.hasOwnProperty.call(current, segment)) {
        present = false;
        break;
      }
      current = (current as Record<string, unknown>)[segment];
    }
    if (!present) continue;
    const value = readDescriptorValue(descriptor, root);
    if (!isValidManagedTypedValue(descriptor.fieldKey, value)) {
      return descriptor.jsonPath.join(".");
    }
  }
  return null;
}

function writeNestedValue(root: Record<string, unknown>, path: string[], value: unknown) {
  let current = root;

  for (let index = 0; index < path.length - 1; index += 1) {
    const segment = path[index];
    const nextValue = current[segment];
    if (!nextValue || Array.isArray(nextValue) || typeof nextValue !== "object") {
      current[segment] = {};
    }
    current = current[segment] as Record<string, unknown>;
  }

  current[path[path.length - 1]] = value;
}

function writeDescriptorValue(
  root: Record<string, unknown>,
  descriptor: VRisingTypedGameSettingDescriptor,
  value: unknown
): boolean {
  if (descriptor.fieldKey === "game_difficulty") {
    const serialized = serializeVRisingGameDifficulty(value);
    if (serialized === undefined) {
      return false;
    }
    writeNestedValue(root, descriptor.jsonPath, serialized);
    return true;
  }

  writeNestedValue(root, descriptor.jsonPath, value);
  return true;
}

function deleteNestedValue(root: Record<string, unknown>, path: string[]): boolean {
  if (path.length === 0) {
    return Object.keys(root).length === 0;
  }

  const [segment, ...rest] = path;
  if (!Object.prototype.hasOwnProperty.call(root, segment)) {
    return Object.keys(root).length === 0;
  }

  if (rest.length === 0) {
    delete root[segment];
    return Object.keys(root).length === 0;
  }

  const nextValue = root[segment];
  if (!nextValue || Array.isArray(nextValue) || typeof nextValue !== "object") {
    delete root[segment];
    return Object.keys(root).length === 0;
  }

  const shouldDeleteChild = deleteNestedValue(nextValue as Record<string, unknown>, rest);
  if (shouldDeleteChild) {
    delete root[segment];
  }

  return Object.keys(root).length === 0;
}

export function applyVRisingTypedGameSettingsFromRawJson(settings: SettingsObject): SettingsObject {
  const parsed = parseVRisingServerGameSettings(settings);
  if (!parsed) {
    return settings;
  }

  const next: SettingsObject = { ...settings };

  for (const descriptor of VRISING_TYPED_GAME_SETTINGS) {
    const value = readDescriptorValue(descriptor, parsed);
    if (isValidTypedValue(value, descriptor.type)) {
      next[descriptor.fieldKey] = value;
    } else {
      delete next[descriptor.fieldKey];
    }
  }

  return next;
}

export function syncVRisingServerGameSettingsFromTypedFields(settings: SettingsObject): SettingsObject {
  const next: SettingsObject = { ...settings };
  const parsed = parseVRisingServerGameSettings(next) ?? {};

  for (const descriptor of VRISING_TYPED_GAME_SETTINGS) {
    const value = next[descriptor.fieldKey];
    // An invalid typed draft must leave the last valid native value available for recovery.
    if (value === undefined) {
      deleteNestedValue(parsed, descriptor.jsonPath);
    } else if (isValidManagedTypedValue(descriptor.fieldKey, value)) {
      writeDescriptorValue(parsed, descriptor, value);
    }
  }

  next.server_game_settings_json = JSON.stringify(parsed, null, 2);
  return next;
}
