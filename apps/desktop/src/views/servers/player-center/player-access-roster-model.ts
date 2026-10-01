import { decodePlayerAccessEntries, parsePlayerAccessCodec } from "../../../domain/player-access";
import type { LocaleCode, TranslateFn } from "../../../i18n";
import type { ModuleDetails } from "../../../types";
import { parseGuidedSettingsSchema } from "../../settings/guided-settings";
import type { GuidedSettingsField, SettingsObject } from "../../settings/settings-schema";

export type RosterLaneKind = "admin" | "allow" | "block" | "priority";
export type RosterFieldKind = "string-lines" | "string-scalar" | "string-list" | "object-list";

export interface RawSchemaProperty {
  type?: unknown;
  title?: unknown;
  description?: unknown;
  default?: unknown;
  enum?: unknown;
  format?: unknown;
  pattern?: unknown;
  readOnly?: unknown;
  required?: unknown;
  items?: RawSchemaProperty;
  properties?: Record<string, RawSchemaProperty>;
  [key: string]: unknown;
}

export interface RosterEntry {
  key: string;
  label: string;
  rawValue: unknown;
}

export interface RosterField {
  key: string;
  title: string;
  description: string | null;
  lane: RosterLaneKind;
  kind: RosterFieldKind;
  property: RawSchemaProperty;
  currentValue: unknown;
  entries: RosterEntry[];
  sortWeight: number;
}

export interface PlayerAccessRosterCapability {
  key: string;
  property: RawSchemaProperty;
}

const LANE_ORDER: RosterLaneKind[] = ["admin", "allow", "block", "priority"];
const PLAYER_ACCESS_KINDS = new Set<string>(LANE_ORDER);
const IDENTITY_PROPERTY_PRIORITY = [
  "steam_id",
  "steamid",
  "steam64_id",
  "steam64",
  "account_id",
  "user_id",
  "userid",
  "player_id",
  "playerid",
  "uuid",
  "xuid",
  "id",
  "name"
];

export function isRosterRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function hasSchemaType(property: RawSchemaProperty, expectedType: string): boolean {
  if (typeof property.type === "string") {
    return property.type === expectedType;
  }
  if (Array.isArray(property.type)) {
    return property.type.some((candidate) => candidate === expectedType);
  }
  return false;
}

export function readSchemaOrder(property: RawSchemaProperty): number {
  const rawOrder = property["x-lsgm-order"];
  return typeof rawOrder === "number" && Number.isFinite(rawOrder) ? rawOrder : 500;
}

export function humanizeRosterKey(key: string): string {
  return key
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .replace(/[_-]+/g, " ")
    .replace(/\s+/g, " ")
    .trim()
    .replace(/\b(id|ip|uuid|rcon|api|url)\b/gi, (match) => match.toUpperCase())
    .replace(/\b\w/g, (match) => match.toUpperCase());
}

function normalizeSimpleLines(rawValue: unknown): string[] {
  if (typeof rawValue !== "string" || rawValue.trim().length === 0) {
    return [];
  }

  const seen = new Set<string>();
  return rawValue
    .replace(/\r\n/g, "\n")
    .replace(/\r/g, "\n")
    .split("\n")
    .map((entry) => entry.trim())
    .filter((entry) => {
      if (!entry || entry.startsWith("#") || entry.startsWith("//")) {
        return false;
      }
      const identity = entry.toLowerCase();
      if (seen.has(identity)) {
        return false;
      }
      seen.add(identity);
      return true;
    });
}

function findIdentityKey(record: Record<string, unknown>): string | null {
  const keys = Object.keys(record);
  for (const preferredKey of IDENTITY_PROPERTY_PRIORITY) {
    const match = keys.find((key) => key.toLowerCase() === preferredKey);
    if (match && String(record[match] ?? "").trim().length > 0) {
      return match;
    }
  }
  return null;
}

export function stringifyRosterEntry(value: unknown): string {
  if (typeof value === "string") {
    return value.trim();
  }
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  if (isRosterRecord(value)) {
    const identityKey = findIdentityKey(value);
    const identity = identityKey ? String(value[identityKey] ?? "").trim() : "";
    const name = typeof value.name === "string" ? value.name.trim() : "";
    if (identity && name && identity !== name) {
      return `${identity} / ${name}`;
    }
    if (identity) {
      return identity;
    }
    return JSON.stringify(value);
  }
  return "";
}

function normalizeRosterIdentity(value: unknown): string {
  if (isRosterRecord(value)) {
    const identityKey = findIdentityKey(value);
    if (identityKey) {
      return String(value[identityKey] ?? "").trim().toLowerCase();
    }
  }
  return stringifyRosterEntry(value).toLowerCase();
}

function buildRosterEntry(value: unknown): RosterEntry | null {
  const label = stringifyRosterEntry(value);
  if (!label) {
    return null;
  }
  const key = normalizeRosterIdentity(value);
  return key ? { key, label, rawValue: value } : null;
}

function readRosterCurrentValue(field: RosterField, settings: SettingsObject): unknown {
  return Object.prototype.hasOwnProperty.call(settings, field.key)
    ? settings[field.key]
    : field.property.default ?? (hasSchemaType(field.property, "array") ? [] : "");
}

function readRosterEntries(field: RosterField, settings: SettingsObject): RosterEntry[] {
  const rawValue = readRosterCurrentValue(field, settings);
  if (parsePlayerAccessCodec(field.property)) {
    return decodePlayerAccessEntries(field.key, field.property, rawValue);
  }
  if (field.kind === "string-lines" || field.kind === "string-scalar") {
    return normalizeSimpleLines(rawValue)
      .map((entry) => buildRosterEntry(entry))
      .filter((entry): entry is RosterEntry => Boolean(entry));
  }
  if (Array.isArray(rawValue)) {
    const seen = new Set<string>();
    return rawValue
      .map((entry) => buildRosterEntry(entry))
      .filter((entry): entry is RosterEntry => Boolean(entry))
      .filter((entry) => {
        if (seen.has(entry.key)) return false;
        seen.add(entry.key);
        return true;
      });
  }
  return [];
}

function parseSchemaProperties(moduleDetails: ModuleDetails | null): Record<string, RawSchemaProperty> {
  if (!moduleDetails?.schema_json) {
    return {};
  }
  try {
    const parsed: unknown = JSON.parse(moduleDetails.schema_json);
    if (!isRosterRecord(parsed) || !isRosterRecord(parsed.properties)) {
      return {};
    }
    return Object.fromEntries(
      Object.entries(parsed.properties)
        .filter((entry): entry is [string, RawSchemaProperty] => isRosterRecord(entry[1]))
    );
  } catch {
    return {};
  }
}

function readPlayerAccessKind(property: RawSchemaProperty): RosterLaneKind | null {
  const rawKind = property["x-lsgm-player-access-kind"];
  return typeof rawKind === "string" && PLAYER_ACCESS_KINDS.has(rawKind)
    ? rawKind as RosterLaneKind
    : null;
}

export function readPlayerAccessRosterCapabilities(
  moduleDetails: ModuleDetails | null
): PlayerAccessRosterCapability[] {
  return Object.entries(parseSchemaProperties(moduleDetails))
    .filter(([, property]) => Boolean(readPlayerAccessKind(property)))
    .map(([key, property]) => ({ key, property }));
}

function inferRosterFieldKind(key: string, property: RawSchemaProperty): RosterFieldKind | null {
  if (hasSchemaType(property, "array")) {
    return property.items && hasSchemaType(property.items, "object") ? "object-list" : "string-list";
  }
  if (!hasSchemaType(property, "string")) {
    return null;
  }
  if (/(entries|ids|list|users|players|names|administrators|moderators|operators|banned|whitelist|blacklist|blocklist)/i.test(key)) {
    return "string-lines";
  }
  return /^(owner_name|owner_steam_id|owner_steam64_id)$/i.test(key) ? "string-scalar" : null;
}

export function buildRosterFields(
  moduleDetails: ModuleDetails | null,
  locale: LocaleCode,
  t: TranslateFn,
  settings: SettingsObject,
  options: { savedValuesOnly?: boolean } = {}
): RosterField[] {
  const properties = parseSchemaProperties(moduleDetails);
  const guidedSchema = parseGuidedSettingsSchema(moduleDetails, locale, t, { surface: "player_access" });
  const guidedTitles = new Map(guidedSchema.fields.map((field: GuidedSettingsField) => [field.key, field.title]));
  const moduleId = moduleDetails?.summary.id ?? "";

  return Object.entries(properties)
    .flatMap(([key, property]) => {
      if (options.savedValuesOnly && !Object.prototype.hasOwnProperty.call(settings, key)) return [];
      const lane = readPlayerAccessKind(property);
      const kind = inferRosterFieldKind(key, property);
      if (!lane || !kind) {
        return [];
      }
      const rawTitle = typeof property.title === "string" ? property.title : humanizeRosterKey(key);
      const title = guidedTitles.get(key) ?? t(`settings.schema.${moduleId}.${key}.title`, undefined, rawTitle);
      const rawDescription = typeof property.description === "string" ? property.description.trim() : "";
      const description = rawDescription
        ? t(`settings.schema.${moduleId}.${key}.description`, undefined, rawDescription)
        : null;
      const field: RosterField = {
        key,
        title,
        description,
        lane,
        kind,
        property,
        currentValue: "",
        entries: [],
        sortWeight: LANE_ORDER.indexOf(lane) * 10000 + readSchemaOrder(property)
      };
      return [{
        ...field,
        currentValue: readRosterCurrentValue(field, settings),
        entries: readRosterEntries(field, settings)
      }];
    })
    .sort((left, right) => left.sortWeight - right.sortWeight || left.title.localeCompare(right.title));
}
