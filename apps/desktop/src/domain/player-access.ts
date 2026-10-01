export type PlayerAccessCodec =
  | "plain"
  | "steam64"
  | "humanitz_net_id"
  | "uint64"
  | "pipe_steam64"
  | "csv_uuid_name"
  | "minecraft_ip_csv"
  | "terraria_banlist"
  | "ark_account_id"
  | "barotrauma_account"
  | "dst_klei_id"
  | "valheim_platform_id"
  | "object_identity";

export type PlayerAccessSyncMode = "direct" | "reload" | "restart";

export interface PlayerAccessSchemaProperty {
  type?: unknown;
  default?: unknown;
  items?: PlayerAccessSchemaProperty;
  properties?: Record<string, PlayerAccessSchemaProperty>;
  pattern?: unknown;
  [key: string]: unknown;
}

export interface PlayerAccessSync {
  mode: PlayerAccessSyncMode;
  addActionId: string | null;
  removeActionId: string | null;
  actionId: string | null;
  verifyActionId: string | null;
  consumeActionIds: string[];
}

export interface PlayerAccessRuntimeActionLike {
  id: string;
}

export interface PlayerAccessFieldSource {
  key: string;
  property: PlayerAccessSchemaProperty;
}

export interface DecodedPlayerAccessEntry {
  key: string;
  label: string;
  rawValue: unknown;
}

export interface PlayerAccessBinding {
  fieldKey: string;
  sync: PlayerAccessSync;
  consumedRuntimeActionIds: string[];
}

const PLAYER_ACCESS_CODECS = new Set<PlayerAccessCodec>([
  "plain",
  "steam64",
  "humanitz_net_id",
  "uint64",
  "pipe_steam64",
  "csv_uuid_name",
  "minecraft_ip_csv",
  "terraria_banlist",
  "ark_account_id",
  "barotrauma_account",
  "dst_klei_id",
  "valheim_platform_id",
  "object_identity"
]);

const PLAYER_ACCESS_SYNC_MODES = new Set<PlayerAccessSyncMode>(["direct", "reload", "restart"]);

const OBJECT_IDENTITY_KEYS = [
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

const MINECRAFT_UUID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const MINECRAFT_COMPACT_UUID_PATTERN = /^[0-9a-f]{32}$/i;
const MINECRAFT_PLAYER_NAME_PATTERN = /^[A-Za-z0-9_]{1,16}$/;
const HUMANITZ_NET_ID_PATTERN = /^(?:[0-9a-fA-F]{32})?\|[0-9a-fA-F]{32}$/;
export function isHumanitzNetId(value: string): boolean {
  return HUMANITZ_NET_ID_PATTERN.test(value) && !Array.from(value).some((character) => UNICODE_CONTROL_PATTERN.test(character));
}
const STEAM64_PATTERN = /^\d{17}$/;
const STEAM64_BASE = 76561197960265728n;
const UINT64_MAX_DECIMAL = "18446744073709551615";
const UNICODE_CONTROL_PATTERN = /\p{Cc}/u;
const UNSUPPORTED_RUNTIME_ACTION_SYNTAX = [";", "&&", "||", "`", "$("] as const;

export function hasUnsupportedRuntimeActionSyntax(value: string): boolean {
  return UNICODE_CONTROL_PATTERN.test(value)
    || value.includes("{{")
    || value.includes("}}")
    || UNSUPPORTED_RUNTIME_ACTION_SYNTAX.some((fragment) => value.includes(fragment));
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function readNonEmptyString(value: unknown): string | null {
  if (typeof value !== "string") {
    return null;
  }
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

function readOptionalId(record: Record<string, unknown>, key: string): string | null {
  const value = record[key];
  return value === undefined ? null : readNonEmptyString(value);
}

function hasInvalidOptionalId(record: Record<string, unknown>, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(record, key) && !readNonEmptyString(record[key]);
}

function readStringArray(value: unknown): string[] | null {
  if (value === undefined) {
    return [];
  }
  if (!Array.isArray(value)) {
    return null;
  }

  const ids: string[] = [];
  const seen = new Set<string>();
  for (const candidate of value) {
    const id = readNonEmptyString(candidate);
    if (!id) {
      return null;
    }
    if (!seen.has(id)) {
      seen.add(id);
      ids.push(id);
    }
  }
  return ids;
}

function uniqueIds(ids: readonly (string | null)[]): string[] {
  const seen = new Set<string>();
  const result: string[] = [];
  for (const id of ids) {
    if (id && !seen.has(id)) {
      seen.add(id);
      result.push(id);
    }
  }
  return result;
}

function canonicalText(value: string): string {
  return value.trim().toLowerCase();
}

function isUint64Decimal(value: string): boolean {
  if (!/^[0-9]+$/.test(value)) return false;
  const magnitude = value.replace(/^0+/, "") || "0";
  return magnitude.length < UINT64_MAX_DECIMAL.length
    || (magnitude.length === UINT64_MAX_DECIMAL.length && magnitude <= UINT64_MAX_DECIMAL);
}

function splitLines(value: string): string[] {
  return value
    .replace(/\r\n/g, "\n")
    .replace(/\r/g, "\n")
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0 && !line.startsWith("#") && !line.startsWith("//"));
}

function rawEntries(
  codec: PlayerAccessCodec,
  value: unknown,
  property: PlayerAccessSchemaProperty
): unknown[] {
  if (codec === "object_identity") {
    return Array.isArray(value) ? value : [];
  }
  if (typeof value === "string") {
    if (property.format !== "textarea") {
      const scalar = value.trim();
      return scalar ? [scalar] : [];
    }
    const entries = splitLines(value);
    if ((codec === "steam64" || codec === "uint64") && playerAccessTextAcceptsComma(property)) {
      return entries.flatMap((entry) => entry.split(",").map((part) => part.trim()).filter(Boolean));
    }
    return entries;
  }
  if (Array.isArray(value)) {
    return value;
  }
  return value === undefined || value === null ? [] : [value];
}

export function playerAccessObjectIdentityKeys(property: PlayerAccessSchemaProperty): string[] {
  const properties = property.items?.properties;
  if (!properties) {
    return [];
  }

  const keys = Object.keys(properties);
  const declaredKeys = keys.filter(
    (key) => properties[key]?.["x-lsgm-player-access-identity"] === true
  );
  if (declaredKeys.length > 0) {
    return declaredKeys;
  }

  const inferredKey = OBJECT_IDENTITY_KEYS
    .map((candidate) => keys.find((key) => key.toLocaleLowerCase() === candidate))
    .find((key): key is string => Boolean(key))
    ?? keys.find((key) => {
      const type = properties[key]?.type;
      return type === "string" || (Array.isArray(type) && type.includes("string"));
    })
    ?? null;
  return inferredKey ? [inferredKey] : [];
}

function canonicalObjectIdentityValue(value: unknown): string | null {
  if (typeof value === "string") {
    return readNonEmptyString(value)?.toLowerCase() ?? null;
  }
  if (typeof value === "number" && Number.isFinite(value)) {
    return String(value);
  }
  if (typeof value === "boolean") {
    return String(value);
  }
  return null;
}

export function matchesPlayerAccessDateFormat(value: string, format: unknown): boolean {
  if (format !== "date" && format !== "date-or-local-datetime") return true;
  const match = /^(\d{4})-(\d{2})-(\d{2})(?: (\d{2}):(\d{2}):(\d{2}))?$/.exec(value);
  if (!match || (format === "date" && match[4] !== undefined)) return false;
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const leap = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
  const days = [31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  return year > 0 && month >= 1 && month <= 12 && day >= 1 && day <= days[month - 1]
    && (match[4] === undefined || (Number(match[4]) <= 23 && Number(match[5]) <= 59 && Number(match[6]) <= 59));
}

function matchesStringProperty(value: string, property: PlayerAccessSchemaProperty): boolean {
  if (!matchesPlayerAccessDateFormat(value, property.format)) return false;
  const enumValues = Array.isArray(property.enum) ? property.enum : [];
  if (enumValues.length > 0 && !enumValues.some((candidate) => candidate === value)) {
    return false;
  }
  if (typeof property.minLength === "number" && Array.from(value).length < property.minLength) {
    return false;
  }
  if (typeof property.maxLength === "number" && Array.from(value).length > property.maxLength) {
    return false;
  }
  const pattern = readNonEmptyString(property.pattern);
  if (pattern) {
    try {
      if (!new RegExp(pattern).test(value)) {
        return false;
      }
    } catch {
      return false;
    }
  }
  return true;
}

function objectIdentity(
  rawValue: unknown,
  property: PlayerAccessSchemaProperty | undefined
): { key: string; values: string[]; name: string | null } | null {
  if (!isRecord(rawValue) || !property) {
    return null;
  }
  const objectProperties = property.items?.properties ?? {};
  for (const [key, schema] of Object.entries(objectProperties)) {
    if (schema.type !== "string" || rawValue[key] === undefined) {
      continue;
    }
    if (typeof rawValue[key] !== "string") {
      return null;
    }
    const value = String(rawValue[key]).trim();
    if (Array.from(value).some((character) => UNICODE_CONTROL_PATTERN.test(character))) {
      return null;
    }
    if (!matchesStringProperty(value, schema)) {
      return null;
    }
  }
  const identityKeys = playerAccessObjectIdentityKeys(property);
  if (identityKeys.length === 0) {
    return null;
  }
  const values: string[] = [];
  const canonicalValues: string[] = [];
  for (const identityKey of identityKeys) {
    const canonicalIdentity = canonicalObjectIdentityValue(rawValue[identityKey]);
    if (!canonicalIdentity) {
      return null;
    }
    const identity = String(rawValue[identityKey]).trim();
    const identityProperty = property.items?.properties?.[identityKey];
    const platformField = identityProperty?.["x-lsgm-player-access-platform-field"];
    if (typeof platformField === "string") {
      const platform = String(rawValue[platformField] ?? "").trim().toLowerCase();
      const validAccount = platform === "steam" ? /^\d{17}$/.test(identity)
        : platform === "eos" ? /^[0-9a-fA-F]{8,32}$/.test(identity)
          : (platform === "xbl" || platform === "psn") && /^[A-Za-z0-9._:-]{1,64}$/.test(identity);
      if (!validAccount) return null;
    }
    const enumValues = Array.isArray(identityProperty?.enum) ? identityProperty.enum : [];
    if (enumValues.length > 0 && !enumValues.some((candidate) => candidate === identity)) {
      return null;
    }
    const pattern = readNonEmptyString(identityProperty?.pattern);
    if (pattern) {
      try {
        if (!new RegExp(pattern).test(String(rawValue[identityKey] ?? ""))) {
          return null;
        }
      } catch {
        return null;
      }
    }
    values.push(identity);
    canonicalValues.push(canonicalIdentity);
  }

  const name = readNonEmptyString(rawValue.name);
  const key = canonicalValues.length === 1
    ? canonicalValues[0]
    : identityKeys.map((identityKey, index) => `${identityKey}=${JSON.stringify(canonicalValues[index])}`).join("|");
  return { key, values, name };
}

export type DelimitedEntryRequirement = "stored" | "add" | "remove";

export interface NormalizedDelimitedEntry {
  parts: string[];
  identity: string;
  stored: string;
}

function playerAccessTextAcceptsComma(property: PlayerAccessSchemaProperty): boolean {
  const separators = property["x-lsgm-player-access-entry-separators"];
  return Array.isArray(separators) && separators.includes("comma");
}

export function normalizePlayerAccessDelimitedEntry(
  codec: PlayerAccessCodec,
  rawValue: unknown,
  property: PlayerAccessSchemaProperty | undefined,
  requirement: DelimitedEntryRequirement
): NormalizedDelimitedEntry | null {
  if (!property) {
    return null;
  }
  const delimiter = codec === "pipe_steam64"
    ? "|"
    : codec === "csv_uuid_name" || codec === "minecraft_ip_csv" || codec === "barotrauma_account"
      ? ","
      : null;
  const fields = property["x-lsgm-player-access-delimited-fields"];
  const value = readNonEmptyString(rawValue);
  if (!delimiter || !Array.isArray(fields) || fields.length === 0 || !value
    || !validDelimitedContract(codec, fields)) {
    return null;
  }
  const lastField = fields[fields.length - 1];
  const consumeRest = isRecord(lastField) && lastField.consumeRest === true;
  const rawParts = consumeRest
    ? splitDelimitedEntry(value, delimiter, fields.length)
    : value.split(delimiter).map((part) => part.trim());
  if (rawParts.length > fields.length || rawParts.some((part) => part.length === 0)) {
    return null;
  }

  const parts: string[] = [];
  for (let index = 0; index < rawParts.length; index += 1) {
    const field = fields[index];
    if (!isRecord(field) || !readNonEmptyString(field.name)) {
      return null;
    }
    const normalized = normalizeDelimitedSegment(rawParts[index], field);
    if (!normalized) {
      return null;
    }
    parts.push(normalized);
  }
  if (requirement !== "remove") {
    for (let index = 0; index < fields.length; index += 1) {
      const field = fields[index];
      if (!isRecord(field) || (field.required === true && index >= parts.length)) {
        return null;
      }
    }
  }
  return parts[0] ? { parts, identity: parts[0], stored: parts.join(delimiter) } : null;
}

function validDelimitedContract(codec: PlayerAccessCodec, fields: unknown[]): boolean {
  const expectedIdentityFormat = codec === "pipe_steam64"
    ? "steam64"
    : codec === "csv_uuid_name"
      ? "minecraft_uuid"
      : codec === "minecraft_ip_csv"
        ? "ip"
        : codec === "barotrauma_account"
          ? "barotrauma_account"
          : null;
  const supportedFormats = new Set([
    "steam64", "minecraft_uuid", "minecraft_name", "ip", "barotrauma_account",
    "text", "integer", "boolean"
  ]);
  if (!expectedIdentityFormat) {
    return false;
  }
  for (let index = 0; index < fields.length; index += 1) {
    const field = fields[index];
    if (!isRecord(field) || !readNonEmptyString(field.name)
      || !supportedFormats.has(String(field.format ?? ""))
      || typeof field.required !== "boolean") {
      return false;
    }
    if (field.consumeRest === true && (
      index + 1 !== fields.length || field.format !== "text"
    )) {
      return false;
    }
  }
  const first = fields[0];
  return isRecord(first)
    && first.format === expectedIdentityFormat
    && first.required === true;
}

function splitDelimitedEntry(value: string, delimiter: string, fieldCount: number): string[] {
  const parts: string[] = [];
  let remainder = value;
  for (let index = 1; index < fieldCount; index += 1) {
    const delimiterIndex = remainder.indexOf(delimiter);
    if (delimiterIndex < 0) {
      break;
    }
    parts.push(remainder.slice(0, delimiterIndex).trim());
    remainder = remainder.slice(delimiterIndex + delimiter.length);
  }
  parts.push(remainder.trim());
  return parts;
}

function normalizeDelimitedSegment(rawValue: string, field: Record<string, unknown>): string | null {
  const format = readNonEmptyString(field.format);
  if (format === "steam64") {
    return STEAM64_PATTERN.test(rawValue) ? rawValue : null;
  }
  if (format === "minecraft_uuid") {
    return normalizeMinecraftUuid(rawValue);
  }
  if (format === "minecraft_name") {
    return normalizeMinecraftPlayerName(rawValue);
  }
  if (format === "ip") {
    return normalizeIpAddress(rawValue);
  }
  if (format === "barotrauma_account") {
    return normalizeBarotraumaSteam2Id(rawValue);
  }
  if (format === "text") {
    const maxLength = typeof field.maxLength === "number" && Number.isInteger(field.maxLength)
      ? field.maxLength
      : null;
    return rawValue.includes("{{") || rawValue.includes("}}")
      || Array.from(rawValue).some((character) => UNICODE_CONTROL_PATTERN.test(character))
      || (maxLength !== null && Array.from(rawValue).length > maxLength)
      ? null
      : rawValue;
  }
  if (format === "integer") {
    if (!/^-?\d+$/.test(rawValue)) {
      return null;
    }
    const value = Number(rawValue);
    if (!Number.isSafeInteger(value)
      || (typeof field.minimum === "number" && value < field.minimum)
      || (typeof field.maximum === "number" && value > field.maximum)) {
      return null;
    }
    return String(value);
  }
  if (format === "boolean") {
    const value = rawValue.toLowerCase();
    if (["1", "true", "yes", "on"].includes(value)) return "true";
    if (["0", "false", "no", "off"].includes(value)) return "false";
  }
  return null;
}

function normalizeMinecraftUuid(value: string): string | null {
  const trimmed = value.trim();
  if (MINECRAFT_UUID_PATTERN.test(trimmed)) {
    return trimmed.toLowerCase();
  }
  if (!MINECRAFT_COMPACT_UUID_PATTERN.test(trimmed)) {
    return null;
  }
  const compact = trimmed.toLowerCase();
  return `${compact.slice(0, 8)}-${compact.slice(8, 12)}-${compact.slice(12, 16)}-${compact.slice(16, 20)}-${compact.slice(20)}`;
}

function normalizeMinecraftPlayerName(value: string): string | null {
  const trimmed = value.trim();
  return MINECRAFT_PLAYER_NAME_PATTERN.test(trimmed) ? trimmed : null;
}

function normalizeIpv4Address(value: string): string | null {
  const ipv4Parts = value.split(".");
  if (ipv4Parts.length === 4 && ipv4Parts.every((part) => (
    /^\d{1,3}$/.test(part)
    && (part === "0" || !part.startsWith("0"))
    && Number(part) >= 0
    && Number(part) <= 255
  ))) {
    return ipv4Parts.map((part) => String(Number(part))).join(".");
  }
  return null;
}

function normalizeIpAddress(value: string): string | null {
  const trimmed = value.trim().toLowerCase();
  const ipv4 = normalizeIpv4Address(trimmed);
  if (ipv4) {
    return ipv4;
  }

  if (trimmed.length > 45 || !trimmed.includes(":") || !/^[0-9a-f:.]+$/.test(trimmed)) {
    return null;
  }

  try {
    const hostname = new URL(`http://[${trimmed}]/`).hostname;
    return hostname.startsWith("[") && hostname.endsWith("]")
      ? hostname.slice(1, -1).toLowerCase()
      : null;
  } catch {
    return null;
  }
}

function utf8ByteLength(value: string): number {
  let length = 0;
  for (let index = 0; index < value.length; index += 1) {
    const codeUnit = value.charCodeAt(index);
    if (codeUnit <= 0x7f) {
      length += 1;
    } else if (codeUnit <= 0x7ff) {
      length += 2;
    } else if (codeUnit >= 0xd800 && codeUnit <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (next >= 0xdc00 && next <= 0xdfff) {
        length += 4;
        index += 1;
      } else {
        length += 3;
      }
    } else {
      length += 3;
    }
  }
  return length;
}

function normalizeArkAccountId(rawValue: unknown): string | null {
  const value = readNonEmptyString(rawValue);
  if (!value || value.length > 128 || value.includes("{{") || value.includes("}}")
    || /[,|;\s]/u.test(value) || Array.from(value).some((character) => (
      UNICODE_CONTROL_PATTERN.test(character) || !/[A-Za-z0-9_:-]/.test(character)
    ))) {
    return null;
  }
  return value;
}

function normalizeBarotraumaSteam2Id(rawValue: unknown): string | null {
  const value = readNonEmptyString(rawValue) ?? "";
  if (!value) {
    return null;
  }
  const upper = value.toUpperCase();
  if (upper.startsWith("STEAM_")) {
    const parts = upper.slice(6).split(":");
    if (parts.length === 3 && /^[01]$/.test(parts[1] ?? "") && /^\d+$/.test(parts[2] ?? "")) {
      return `STEAM_1:${parts[1]}:${parts[2]}`;
    }
  }
  if (/^\[U:1:\d+\]$/i.test(value)) {
    const accountId = BigInt(value.slice(5, -1));
    return `STEAM_1:${accountId % 2n}:${accountId / 2n}`;
  }
  if (STEAM64_PATTERN.test(value)) {
    const steam64 = BigInt(value);
    if (steam64 > STEAM64_BASE) {
      const accountId = steam64 - STEAM64_BASE;
      return `STEAM_1:${accountId % 2n}:${accountId / 2n}`;
    }
  }
  return null;
}

function normalizeDstKleiId(rawValue: unknown): string | null {
  const value = readNonEmptyString(rawValue);
  if (!value || value.length < 4 || value.length > 64 || !value.toUpperCase().startsWith("KU_")
    || value.includes("{{") || value.includes("}}") || !/^[A-Za-z0-9_-]+$/.test(value)) {
    return null;
  }
  return `KU_${value.slice(3)}`;
}

function normalizeValheimPlatformId(rawValue: unknown): string | null {
  const value = readNonEmptyString(rawValue);
  if (!value || value.includes(",") || value.includes("|")
    || Array.from(value).some((character) => /\s/u.test(character) || UNICODE_CONTROL_PATTERN.test(character))) {
    return null;
  }
  return value;
}

export function parsePlayerAccessCodec(property: PlayerAccessSchemaProperty): PlayerAccessCodec | null {
  const value = property["x-lsgm-player-access-codec"];
  return typeof value === "string" && PLAYER_ACCESS_CODECS.has(value as PlayerAccessCodec)
    ? value as PlayerAccessCodec
    : null;
}

export function parsePlayerAccessSync(property: PlayerAccessSchemaProperty): PlayerAccessSync | null {
  const rawSync = property["x-lsgm-player-access-sync"];
  if (!isRecord(rawSync)) {
    return null;
  }

  const mode = rawSync.mode;
  if (typeof mode !== "string" || !PLAYER_ACCESS_SYNC_MODES.has(mode as PlayerAccessSyncMode)) {
    return null;
  }

  if (["add_action_id", "remove_action_id", "action_id", "verify_action_id"]
    .some((key) => hasInvalidOptionalId(rawSync, key))) {
    return null;
  }

  const addActionId = readOptionalId(rawSync, "add_action_id");
  const removeActionId = readOptionalId(rawSync, "remove_action_id");
  const actionId = readOptionalId(rawSync, "action_id");
  const verifyActionId = readOptionalId(rawSync, "verify_action_id");
  const consumeActionIds = readStringArray(rawSync.consume_action_ids);
  if (consumeActionIds === null) {
    return null;
  }

  if (mode === "direct" && ((!addActionId && !removeActionId) || actionId)) {
    return null;
  }
  if (mode === "reload" && (!actionId || addActionId || removeActionId)) {
    return null;
  }
  if (mode === "restart" && (addActionId || removeActionId || actionId || verifyActionId)) {
    return null;
  }

  return {
    mode: mode as PlayerAccessSyncMode,
    addActionId,
    removeActionId,
    actionId,
    verifyActionId,
    consumeActionIds
  };
}

export function playerAccessMutationActionId(sync: PlayerAccessSync, operation: PlayerAccessMutationOperation): string | null {
  if (sync.mode === "direct") return operation === "add" ? sync.addActionId : sync.removeActionId;
  return sync.mode === "reload" ? sync.actionId : null;
}

export function canonicalPlayerAccessIdentity(
  codec: PlayerAccessCodec,
  rawValue: unknown,
  property?: PlayerAccessSchemaProperty
): string | null {
  if (codec === "object_identity") {
    return objectIdentity(rawValue, property)?.key ?? null;
  }

  const stringValue = readNonEmptyString(rawValue);
  if (!stringValue || Array.from(stringValue).some((character) => UNICODE_CONTROL_PATTERN.test(character))) {
    return null;
  }
  if (property && !matchesStringProperty(stringValue, property)) {
    return null;
  }

  if (codec === "pipe_steam64" || codec === "csv_uuid_name"
    || codec === "minecraft_ip_csv" || codec === "barotrauma_account") {
    return normalizePlayerAccessDelimitedEntry(codec, rawValue, property, "remove")?.identity.toLowerCase() ?? null;
  }

  if (codec === "steam64") {
    return STEAM64_PATTERN.test(stringValue) ? stringValue : null;
  }
  if (codec === "humanitz_net_id") {
    return HUMANITZ_NET_ID_PATTERN.test(stringValue) || STEAM64_PATTERN.test(stringValue) ? stringValue : null;
  }

  if (codec === "uint64") {
    return isUint64Decimal(stringValue) ? stringValue : null;
  }

  if (codec === "ark_account_id") {
    const value = normalizeArkAccountId(rawValue);
    return value ? canonicalText(value) : null;
  }

  if (codec === "dst_klei_id") {
    return normalizeDstKleiId(rawValue)?.toLowerCase() ?? null;
  }

  if (codec === "valheim_platform_id") {
    const value = normalizeValheimPlatformId(rawValue);
    return value ? canonicalText(value) : null;
  }

  if (codec === "terraria_banlist" && (
    utf8ByteLength(stringValue) > 128
    || stringValue.includes("{{")
    || stringValue.includes("}}")
    || Array.from(stringValue).some((character) => UNICODE_CONTROL_PATTERN.test(character))
  )) {
    return null;
  }
  return canonicalText(stringValue);
}

export function canonicalPlayerAccessAdditionIdentity(
  codec: PlayerAccessCodec,
  rawValue: unknown,
  property?: PlayerAccessSchemaProperty
): string | null {
  if (codec === "pipe_steam64" || codec === "csv_uuid_name"
    || codec === "minecraft_ip_csv" || codec === "barotrauma_account") {
    return normalizePlayerAccessDelimitedEntry(codec, rawValue, property, "add")?.identity.toLowerCase() ?? null;
  }

  if (codec === "object_identity") {
    return objectIdentity(rawValue, property)?.key ?? null;
  }

  if (codec === "humanitz_net_id") {
    return normalizePlayerAccessMutationEntry(codec, rawValue, property ?? {}, "add")?.identity ?? null;
  }
  return canonicalPlayerAccessIdentity(codec, rawValue, property);
}

export function findPlayerAccessMutationConflict(
  fieldKey: string,
  property: PlayerAccessSchemaProperty,
  properties: Record<string, PlayerAccessSchemaProperty>,
  settings: Record<string, unknown>,
  operation: PlayerAccessMutationOperation,
  rawValue: unknown
): string | null {
  if (operation !== "add") {
    return null;
  }
  const codec = parsePlayerAccessCodec(property);
  const identity = codec
    ? canonicalPlayerAccessAdditionIdentity(codec, rawValue, property)
    : null;
  if (!identity) {
    return null;
  }
  const conflicts = property["x-lsgm-player-access-conflicts-with"];
  if (!Array.isArray(conflicts)) {
    return null;
  }
  for (const conflictField of conflicts) {
    if (typeof conflictField !== "string" || !conflictField.trim()) {
      throw new Error("Player-access conflict metadata must contain field names.");
    }
    const conflictKey = conflictField.trim();
    const conflictProperty = properties[conflictKey];
    if (!conflictProperty) {
      throw new Error(`Player-access conflict metadata references unknown field ${conflictKey}.`);
    }
    const conflictValue = settings[conflictKey] ?? conflictProperty.default
      ?? (conflictProperty.type === "array" ? [] : "");
    if (decodePlayerAccessEntries(conflictKey, conflictProperty, conflictValue)
      .some((entry) => entry.key === identity)) {
      return `Identity conflicts with ${conflictKey}; remove it there before adding it to ${fieldKey}.`;
    }
  }
  return null;
}

export type PlayerAccessMutationOperation = "add" | "remove";

export interface NormalizedPlayerAccessMutationEntry {
  identity: string;
  stored: unknown;
}

export interface PlayerAccessMutationPatch {
  changed: boolean;
  value: unknown;
}

function defaultObjectPlayerAccessValue(
  key: string,
  property: PlayerAccessSchemaProperty,
  accessKind: string
): unknown {
  if (property.default !== undefined) {
    return property.default;
  }
  if (property.type === "integer" || property.type === "number") {
    return key.endsWith("permission_level") && accessKind !== "admin" ? 1000 : 0;
  }
  if (property.type === "boolean") {
    return false;
  }
  return key.toLowerCase().includes("reason") ? "LanGame" : "";
}

function objectPlayerAccessLiveTarget(
  property: PlayerAccessSchemaProperty,
  rawValue: Record<string, unknown>,
  stored: Record<string, unknown>
): string | null {
  const identityKeys = playerAccessObjectIdentityKeys(property);
  const format = property["x-lsgm-player-access-live-target"];
  if (format !== undefined) {
    if (format !== "platform_userid" || !identityKeys.includes("platform") || !identityKeys.includes("userid")
      || property.items?.properties?.userid?.["x-lsgm-player-access-platform-field"] !== "platform"
      || typeof rawValue.platform !== "string" || !rawValue.platform.trim()
      || typeof rawValue.userid !== "string" || !rawValue.userid.trim()) return null;
    const platform = stored.platform;
    const userid = stored.userid;
    if (typeof platform !== "string" || typeof userid !== "string"
      || !/^[A-Za-z][A-Za-z0-9]*$/.test(platform) || !/^[A-Za-z0-9._:-]{1,64}$/.test(userid)) return null;
    return `${platform}_${userid}`;
  }
  const key = identityKeys.find((identityKey) => (
    typeof property.items?.properties?.[identityKey]?.["x-lsgm-player-access-platform-field"] === "string"
  )) ?? identityKeys[identityKeys.length - 1];
  return key && typeof stored[key] === "string" ? stored[key] : null;
}

function normalizeObjectPlayerAccessEntry(
  rawValue: unknown,
  property: PlayerAccessSchemaProperty
): NormalizedPlayerAccessMutationEntry | null {
  if (!isRecord(rawValue) || property.type !== "array" || property.items?.type !== "object"
    || !property.items.properties) {
    return null;
  }

  const accessKind = readNonEmptyString(property["x-lsgm-player-access-kind"]) ?? "";
  const stored: Record<string, unknown> = {};
  for (const [key, itemProperty] of Object.entries(property.items.properties)) {
    const candidate = Object.prototype.hasOwnProperty.call(rawValue, key)
      ? rawValue[key]
      : defaultObjectPlayerAccessValue(key, itemProperty, accessKind);
    if (itemProperty.type === "string") {
      if (typeof candidate !== "string") {
        return null;
      }
      const value = candidate.trim();
      if (Array.from(value).some((character) => UNICODE_CONTROL_PATTERN.test(character))
        || !matchesStringProperty(value, itemProperty)) {
        return null;
      }
      stored[key] = value;
      continue;
    }
    if (itemProperty.type === "integer") {
      if (!Number.isSafeInteger(candidate)
        || (typeof itemProperty.minimum === "number" && Number(candidate) < itemProperty.minimum)
        || (typeof itemProperty.maximum === "number" && Number(candidate) > itemProperty.maximum)) {
        return null;
      }
      stored[key] = candidate;
      continue;
    }
    if (itemProperty.type === "number") {
      if (typeof candidate !== "number" || !Number.isFinite(candidate)
        || (typeof itemProperty.minimum === "number" && candidate < itemProperty.minimum)
        || (typeof itemProperty.maximum === "number" && candidate > itemProperty.maximum)) {
        return null;
      }
      stored[key] = candidate;
      continue;
    }
    if (itemProperty.type === "boolean") {
      if (typeof candidate !== "boolean") {
        return null;
      }
      stored[key] = candidate;
      continue;
    }
    return null;
  }

  const identity = objectIdentity(stored, property)?.key ?? null;
  return identity && objectPlayerAccessLiveTarget(property, rawValue, stored) !== null ? { identity, stored } : null;
}

export function normalizePlayerAccessMutationEntry(
  codec: PlayerAccessCodec,
  rawValue: unknown,
  property: PlayerAccessSchemaProperty,
  requirement: DelimitedEntryRequirement
): NormalizedPlayerAccessMutationEntry | null {
  if (property["x-lsgm-player-access-live-target"] !== undefined && codec !== "object_identity") return null;
  if (codec === "object_identity") {
    return normalizeObjectPlayerAccessEntry(rawValue, property);
  }
  if (codec === "pipe_steam64" || codec === "csv_uuid_name"
    || codec === "minecraft_ip_csv" || codec === "barotrauma_account") {
    const normalized = normalizePlayerAccessDelimitedEntry(codec, rawValue, property, requirement);
    return normalized ? { identity: normalized.identity.toLowerCase(), stored: normalized.stored } : null;
  }

  const value = readNonEmptyString(rawValue);
  if (!value || Array.from(value).some((character) => UNICODE_CONTROL_PATTERN.test(character))
    || !matchesStringProperty(value, property)) {
    return null;
  }
  if (codec === "humanitz_net_id") {
    if (typeof rawValue !== "string" || Array.from(rawValue).some((character) => UNICODE_CONTROL_PATTERN.test(character))) return null;
    const valid = HUMANITZ_NET_ID_PATTERN.test(value) || (requirement !== "add" && STEAM64_PATTERN.test(value));
    return valid ? { identity: value, stored: value } : null;
  }

  let stored: string | null = value;
  if (codec === "steam64") {
    stored = STEAM64_PATTERN.test(value) ? value : null;
  } else if (codec === "uint64") {
    stored = isUint64Decimal(value) ? value : null;
  } else if (codec === "terraria_banlist") {
    stored = utf8ByteLength(value) <= 128 && !value.includes("{{") && !value.includes("}}")
      ? value
      : null;
  } else if (codec === "ark_account_id") {
    stored = normalizeArkAccountId(value);
  } else if (codec === "dst_klei_id") {
    stored = normalizeDstKleiId(value);
  } else if (codec === "valheim_platform_id") {
    stored = normalizeValheimPlatformId(value);
  }
  return stored ? { identity: canonicalText(stored), stored } : null;
}

function playerAccessValuesEqual(left: unknown, right: unknown): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

export function playerAccessLiveTarget(
  property: PlayerAccessSchemaProperty,
  rawValue: unknown,
  operation: PlayerAccessMutationOperation
): string | null {
  const codec = parsePlayerAccessCodec(property);
  if (!codec || (property["x-lsgm-player-access-live-target"] !== undefined && codec !== "object_identity")) return null;
  const normalized = normalizePlayerAccessMutationEntry(codec, rawValue, property, operation);
  if (!normalized) return null;
  if (codec === "object_identity") {
    return isRecord(rawValue) && isRecord(normalized.stored)
      ? objectPlayerAccessLiveTarget(property, rawValue, normalized.stored) : null;
  }
  if (codec === "pipe_steam64" || codec === "csv_uuid_name" || codec === "minecraft_ip_csv" || codec === "barotrauma_account") {
    const entry = normalizePlayerAccessDelimitedEntry(codec, rawValue, property, operation);
    return entry ? codec === "csv_uuid_name" ? entry.parts[1] ?? entry.identity : entry.identity : null;
  }
  return typeof normalized.stored === "string" ? normalized.stored : null;
}

function normalizedScalarComparisonValue(
  codec: PlayerAccessCodec,
  property: PlayerAccessSchemaProperty,
  value: unknown
): unknown {
  if (typeof value === "string" && value.trim().length === 0) {
    return "";
  }
  return normalizePlayerAccessMutationEntry(codec, value, property, "stored")?.stored
    ?? (typeof value === "string" ? value.trim() : value);
}

function storedTextEntries(
  codec: PlayerAccessCodec,
  property: PlayerAccessSchemaProperty,
  current: string
): { canonicalized: string | null; entries: string[] } {
  if (codec === "uint64" || codec === "humanitz_net_id") {
    const validated = rawEntries(codec, current, property).map((entry) => {
      const normalized = normalizePlayerAccessMutationEntry(codec, entry, property, "stored");
      if (!normalized || typeof normalized.stored !== "string") {
        throw new Error("Stored player-access roster does not match the field codec.");
      }
      return normalized.stored;
    });
    const normalizeSeparators = playerAccessTextAcceptsComma(property);
    const entries = normalizeSeparators ? [...new Set(validated)] : validated;
    return { canonicalized: normalizeSeparators ? entries.join("\n") : null, entries };
  }
  if (codec === "steam64" && playerAccessTextAcceptsComma(property)) {
    const seen = new Set<string>();
    const entries = rawEntries(codec, current, property)
      .flatMap((entry) => {
        const normalized = normalizePlayerAccessMutationEntry(codec, entry, property, "stored");
        if (!normalized || seen.has(normalized.identity) || typeof normalized.stored !== "string") {
          return [];
        }
        seen.add(normalized.identity);
        return [normalized.stored];
      });
    return { canonicalized: entries.join("\n"), entries };
  }
  return {
    canonicalized: null,
    entries: current.replace(/\r\n?/g, "\n").split("\n").map((entry) => entry.trim()).filter(Boolean)
  };
}

export function applyPlayerAccessMutation(
  property: PlayerAccessSchemaProperty,
  currentValue: unknown,
  operation: PlayerAccessMutationOperation,
  rawValue: unknown,
  expectedValue?: unknown
): PlayerAccessMutationPatch {
  const kind = readNonEmptyString(property["x-lsgm-player-access-kind"]);
  const codec = parsePlayerAccessCodec(property);
  const sync = parsePlayerAccessSync(property);
  if (!kind || !["admin", "allow", "block", "priority"].includes(kind) || !codec || !sync) {
    throw new Error("Field is not an authorized player-access roster.");
  }
  if (operation !== "add" && operation !== "remove") {
    throw new Error("Unsupported player-access mutation operation.");
  }

  const scalar = property.type === "string" && property.format !== "textarea";
  if (scalar && expectedValue === undefined) {
    throw new Error("expectedValue is required for scalar player-access fields.");
  }
  if (!scalar && expectedValue !== undefined) {
    throw new Error("expectedValue is only valid for scalar player-access fields.");
  }
  if (scalar) {
    const currentComparison = normalizedScalarComparisonValue(codec, property, currentValue);
    const expectedComparison = normalizedScalarComparisonValue(codec, property, expectedValue);
    if (!playerAccessValuesEqual(currentComparison, expectedComparison)) {
      throw new Error("Player-access value changed while this edit was pending. Reload and retry.");
    }
  }

  const normalized = normalizePlayerAccessMutationEntry(codec, rawValue, property, operation);
  if (!normalized) {
    throw new Error("Player-access value does not match the field codec.");
  }

  if (property.type === "string" && property.format === "textarea") {
    if (typeof currentValue !== "string" || typeof normalized.stored !== "string") {
      throw new Error("Stored player-access roster must be a string.");
    }
    const { canonicalized, entries } = storedTextEntries(codec, property, currentValue);
    const matching = entries.some((entry) => (
      normalizePlayerAccessMutationEntry(codec, entry, property, "stored")?.identity === normalized.identity
    ));
    if (operation === "add" && !matching) {
      return { changed: true, value: [...entries, normalized.stored].join("\n") };
    }
    if (operation === "remove" && matching) {
      const value = entries.filter((entry) => (
        normalizePlayerAccessMutationEntry(codec, entry, property, "stored")?.identity !== normalized.identity
      )).join("\n");
      return { changed: true, value };
    }
    const value = canonicalized ?? currentValue;
    return { changed: value !== currentValue, value };
  }

  if (scalar) {
    if (typeof currentValue !== "string" || typeof normalized.stored !== "string") {
      throw new Error("Stored scalar player-access value must be a string.");
    }
    const existing = currentValue.trim().length > 0
      ? normalizePlayerAccessMutationEntry(codec, currentValue, property, "stored")
      : null;
    const matches = existing?.identity === normalized.identity;
    const value = operation === "add"
      ? (matches ? currentValue : normalized.stored)
      : (matches ? "" : currentValue);
    return { changed: value !== currentValue, value };
  }

  if (property.type === "array") {
    if (!Array.isArray(currentValue)) {
      throw new Error("Stored player-access roster must be an array.");
    }
    const matches = (entry: unknown) => (
      normalizePlayerAccessMutationEntry(codec, entry, property, "stored")?.identity === normalized.identity
    );
    const hasMatch = currentValue.some(matches);
    if (operation === "add" && codec === "object_identity" && hasMatch) {
      let replaced = false;
      const value = currentValue.flatMap((entry) => {
        if (!matches(entry)) return [entry];
        if (replaced) return [];
        replaced = true;
        return [normalized.stored];
      });
      return { changed: !playerAccessValuesEqual(value, currentValue), value };
    }
    if (operation === "add") {
      return hasMatch
        ? { changed: false, value: currentValue }
        : { changed: true, value: [...currentValue, normalized.stored] };
    }
    return hasMatch
      ? { changed: true, value: currentValue.filter((entry) => !matches(entry)) }
      : { changed: false, value: currentValue };
  }

  throw new Error("Unsupported player-access field storage type.");
}

function playerAccessEntryLabel(
  codec: PlayerAccessCodec,
  rawValue: unknown,
  property: PlayerAccessSchemaProperty
): string | null {
  if (codec === "object_identity") {
    const identity = objectIdentity(rawValue, property);
    if (!identity) {
      return null;
    }
    const identityLabel = identity.values.join(" / ");
    return identity.name && !identity.values.some((value) => canonicalText(value) === canonicalText(identity.name ?? ""))
      ? `${identityLabel} / ${identity.name}`
      : identityLabel;
  }

  if (codec === "csv_uuid_name") {
    const normalized = normalizePlayerAccessDelimitedEntry(codec, rawValue, property, "stored");
    if (!normalized) {
      return null;
    }
    return `${normalized.parts[0]} / ${normalized.parts[1]}`;
  }

  return readNonEmptyString(rawValue);
}

export function decodePlayerAccessEntries(
  fieldKey: string,
  property: PlayerAccessSchemaProperty,
  value: unknown
): DecodedPlayerAccessEntry[] {
  void fieldKey;
  const codec = parsePlayerAccessCodec(property);
  if (!codec) {
    return [];
  }

  const entries: DecodedPlayerAccessEntry[] = [];
  const seen = new Set<string>();
  for (const rawValue of rawEntries(codec, value, property)) {
    const key = normalizePlayerAccessMutationEntry(codec, rawValue, property, "stored")?.identity ?? null;
    const label = playerAccessEntryLabel(codec, rawValue, property);
    if (!key || !label || seen.has(key)) {
      continue;
    }
    seen.add(key);
    entries.push({ key, label, rawValue });
  }
  return entries;
}

export function buildPlayerAccessBindings<TAction extends PlayerAccessRuntimeActionLike>(
  fields: readonly PlayerAccessFieldSource[],
  runtimeActions: readonly TAction[]
): PlayerAccessBinding[] {
  const actionIds = new Set(runtimeActions.map((action) => action.id));

  return fields.flatMap((field) => {
    const codec = parsePlayerAccessCodec(field.property);
    const sync = parsePlayerAccessSync(field.property);
    if (!codec || !sync) {
      return [];
    }

    const consumedRuntimeActionIds = uniqueIds(sync.consumeActionIds)
      .filter((id) => actionIds.has(id));

    return [{
      fieldKey: field.key,
      sync,
      consumedRuntimeActionIds
    }];
  });
}

export function filterConsumedPlayerAccessActions<TAction extends PlayerAccessRuntimeActionLike>(
  runtimeActions: readonly TAction[],
  bindings: readonly PlayerAccessBinding[]
): TAction[] {
  const consumedIds = new Set(bindings.flatMap((binding) => binding.consumedRuntimeActionIds));
  return runtimeActions.filter((action) => !consumedIds.has(action.id));
}
