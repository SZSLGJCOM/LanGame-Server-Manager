import {
  canonicalPlayerAccessAdditionIdentity,
  parsePlayerAccessCodec,
  matchesPlayerAccessDateFormat,
  playerAccessObjectIdentityKeys
} from "../../../domain/player-access";
import { selectLocaleText, type LocaleCode } from "../../../i18n";
import {
  hasSchemaType,
  humanizeRosterKey,
  isRosterRecord,
  readSchemaOrder,
  type RawSchemaProperty,
  type RosterField
} from "./player-access-roster-model";

export type ObjectRosterDraft = Record<string, unknown>;
export type ObjectRosterPropertyKind = "string" | "integer" | "boolean";

export interface ObjectRosterProperty {
  key: string;
  title: string;
  kind: ObjectRosterPropertyKind;
  property: RawSchemaProperty;
  enumValues: Array<string | number>;
  identity: boolean;
  required: boolean;
}

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

function findWritableIdentityKey(properties: Record<string, RawSchemaProperty>): string | null {
  const keys = Object.keys(properties);
  for (const preferredKey of IDENTITY_PROPERTY_PRIORITY) {
    const match = keys.find((key) => key.toLowerCase() === preferredKey);
    if (match) {
      return match;
    }
  }
  return keys.find((key) => hasSchemaType(properties[key], "string")) ?? null;
}

function readObjectRosterIdentityProperty(field: RosterField): [string, RawSchemaProperty] | null {
  if (field.kind !== "object-list") {
    return null;
  }
  const itemProperties = field.property.items?.properties ?? {};
  const identityKey = playerAccessObjectIdentityKeys(field.property)[0]
    ?? findWritableIdentityKey(itemProperties);
  if (!identityKey) {
    return null;
  }
  return [identityKey, itemProperties[identityKey]];
}

function describeRosterPattern(property: RawSchemaProperty, locale: LocaleCode): string {
  const title = typeof property.title === "string" ? property.title : "";
  return /steam/i.test(title)
    ? selectLocaleText(locale, "17 位 Steam64 ID", "a 17-digit Steam64 ID")
    : selectLocaleText(locale, "符合该名单字段格式的值", "a value matching this roster field");
}

function rosterFieldCopy(field: RosterField): string {
  return [
    field.key,
    field.title,
    field.description ?? "",
    typeof field.property.title === "string" ? field.property.title : "",
    typeof field.property.description === "string" ? field.property.description : ""
  ].join(" ");
}

function validateSimpleRosterEntryInput(
  field: RosterField,
  entry: string,
  locale: LocaleCode
): string | null {
  if (field.kind !== "string-lines" && field.kind !== "string-list" && field.kind !== "string-scalar") {
    return null;
  }

  const copy = rosterFieldCopy(field);
  const compactEntry = entry.trim();
  if (/Steam64ID\|/i.test(copy) && !/^\d{17}($|[|,])/.test(compactEntry)) {
    return selectLocaleText(
      locale,
      "请输入以 17 位 Steam64 ID 开头的条目，例如 76561198000000000|Name|Reason；否则服务器读取的名单文件会忽略它。",
      "Enter an entry that starts with a 17-digit Steam64 ID, such as 76561198000000000|Name|Reason; otherwise the server-read roster file will ignore it."
    );
  }
  if (/\buuid\s*,\s*name\b/i.test(copy) && !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\s*,\s*[^,\s]/i.test(compactEntry)) {
    return selectLocaleText(
      locale,
      "请输入 UUID,name 条目；Minecraft 原生 JSON 名单不会读取只有玩家名的行。",
      "Enter a UUID,name entry; Minecraft's native JSON roster will not read a player-name-only row."
    );
  }
  const requiresPlainSteam64 = /\b(?:Steam64|SteamID64)\s+(?:account\s+)?ID\b/i.test(copy)
    && /\b(?:one|optional|17-digit)\b/i.test(copy)
    && !/\bSteam2\b/i.test(copy)
    && !/\bSteam64ID\|/i.test(copy)
    && !/\bor exact player name\b/i.test(copy)
    && !/\bor player name\b/i.test(copy);
  if (requiresPlainSteam64 && !/^\d{17}$/.test(compactEntry)) {
    return selectLocaleText(
      locale,
      "请输入 17 位 Steam64 ID；这个服务器名单不会在这里读取玩家名或其他文本。",
      "Enter a 17-digit Steam64 ID; this server roster does not read player names or other text here."
    );
  }
  return null;
}

export function validateRosterEntryInput(
  field: RosterField,
  entry: string,
  locale: LocaleCode
): string | null {
  const codec = parsePlayerAccessCodec(field.property);
  if (codec) {
    if (canonicalPlayerAccessAdditionIdentity(codec, entry, field.property)) {
      return null;
    }
    if (codec === "csv_uuid_name") {
      return selectLocaleText(
        locale,
        "请输入 UUID,name 条目；UUID 可带或不带连字符，玩家名必须为 1–16 位英文字母、数字或下划线。",
        "Enter UUID,name; the UUID may be compact or hyphenated, and the player name must contain 1–16 ASCII letters, digits, or underscores."
      );
    }
    if (codec === "minecraft_ip_csv") {
      return selectLocaleText(
        locale,
        "请输入有效的 IPv4 或 IPv6 地址；可在逗号后附加封禁原因。",
        "Enter a valid IPv4 or IPv6 address, optionally followed by a comma and ban reason."
      );
    }
    if (codec === "terraria_banlist") {
      return selectLocaleText(
        locale,
        "请输入不超过 128 个 UTF-8 字节的 Terraria 封禁条目，且不要包含模板或控制字符。",
        "Enter a Terraria ban entry of at most 128 UTF-8 bytes without template or control characters."
      );
    }
    if (codec === "steam64" || codec === "pipe_steam64") {
      return selectLocaleText(locale, "请输入 17 位 Steam64 ID。", "Enter a 17-digit Steam64 ID.");
    }
    if (codec === "humanitz_net_id") {
      return selectLocaleText(locale,
        "请输入完整 NetID：EpicAccountId|ProductUserId 或 |ProductUserId，每个非空部分均为 32 位十六进制。旧 Steam64 ID 只能保留或删除，不能作为新的 NetID 添加。",
        "Enter the complete NetID: EpicAccountId|ProductUserId or |ProductUserId, with 32 hexadecimal digits per nonempty part. Existing Steam64 IDs can be retained or removed, not added as new NetIDs.");
    }
    if (codec === "uint64") {
      return selectLocaleText(
        locale,
        "请输入 0 至 18446744073709551615 范围内的十进制账户哈希，仅使用数字 0–9。",
        "Enter a decimal account hash from 0 to 18446744073709551615 using only digits 0–9."
      );
    }
    if (codec === "object_identity") {
      const objectIdentity = readObjectRosterIdentityProperty(field);
      return objectIdentity && /^(steam_id|steamid|steam64_id|steam64)$/i.test(objectIdentity[0])
        ? selectLocaleText(locale, "请输入 17 位 Steam64 ID。", "Enter a 17-digit Steam64 ID.")
        : selectLocaleText(locale, "请输入符合该名单身份格式的值。", "Enter a value matching this roster identity format.");
    }
    return selectLocaleText(locale, "请输入有效的名单条目。", "Enter a valid roster entry.");
  }

  const simpleValidationError = validateSimpleRosterEntryInput(field, entry, locale);
  if (simpleValidationError) {
    return simpleValidationError;
  }
  const objectIdentity = readObjectRosterIdentityProperty(field);
  if (!objectIdentity) {
    return null;
  }
  const [identityKey, identityProperty] = objectIdentity;
  const pattern = typeof identityProperty.pattern === "string" ? identityProperty.pattern : "";
  if (pattern) {
    try {
      if (!new RegExp(pattern).test(entry)) {
        const expectation = describeRosterPattern(identityProperty, locale);
        return selectLocaleText(
          locale,
          `请输入${expectation}；否则服务器读取的名单文件会忽略该条目。`,
          `Enter ${expectation}; otherwise the server-read roster file will ignore this entry.`
        );
      }
    } catch {
      return null;
    }
  }
  if (/^(steam_id|steamid|steam64_id|steam64)$/i.test(identityKey) && !/^\d{17}$/.test(entry)) {
    return selectLocaleText(
      locale,
      "请输入 17 位 Steam64 ID；这个服务器名单不会在这里读取玩家名。",
      "Enter a 17-digit Steam64 ID; this server roster does not read player names here."
    );
  }
  return null;
}

export function readObjectRosterProperties(field: RosterField): ObjectRosterProperty[] {
  if (field.kind !== "object-list") {
    return [];
  }
  const itemSchema = field.property.items;
  const properties = itemSchema?.properties ?? {};
  const requiredKeys = new Set(
    Array.isArray(itemSchema?.required)
      ? itemSchema.required.filter((value): value is string => typeof value === "string")
      : []
  );
  const identityKeys = new Set(playerAccessObjectIdentityKeys(field.property));

  return Object.entries(properties)
    .flatMap(([key, property]) => {
      if (property.readOnly === true) {
        return [];
      }
      const enumValues = Array.isArray(property.enum)
        ? property.enum.filter((value): value is string | number => (
          typeof value === "string" || (typeof value === "number" && Number.isFinite(value))
        ))
        : [];
      const kind: ObjectRosterPropertyKind | null = hasSchemaType(property, "boolean")
        ? "boolean"
        : hasSchemaType(property, "integer")
          ? "integer"
          : hasSchemaType(property, "string") || enumValues.length > 0
            ? "string"
            : null;
      if (!kind) {
        return [];
      }
      return [{
        key,
        title: typeof property.title === "string" ? property.title : humanizeRosterKey(key),
        kind,
        property,
        enumValues,
        identity: identityKeys.has(key),
        required: requiredKeys.has(key) || identityKeys.has(key)
      }];
    })
    .sort((left, right) => readSchemaOrder(left.property) - readSchemaOrder(right.property));
}

export function defaultObjectRosterPropertyValue(property: ObjectRosterProperty): unknown {
  if (property.property.default !== undefined) {
    return property.property.default;
  }
  if (property.enumValues.length > 0) {
    return property.enumValues[0];
  }
  if (property.kind === "boolean") {
    return false;
  }
  return property.kind === "integer" ? 0 : "";
}

export function buildObjectRosterDraft(field: RosterField, rawValue?: unknown): ObjectRosterDraft {
  const source = isRosterRecord(rawValue) ? rawValue : {};
  const draft: ObjectRosterDraft = { ...source };
  for (const property of readObjectRosterProperties(field)) {
    if (!Object.prototype.hasOwnProperty.call(draft, property.key)) {
      draft[property.key] = defaultObjectRosterPropertyValue(property);
    }
  }
  return draft;
}

export function normalizeObjectRosterDraft(
  field: RosterField,
  draft: ObjectRosterDraft
): ObjectRosterDraft {
  const value: ObjectRosterDraft = { ...draft };
  for (const property of readObjectRosterProperties(field)) {
    const candidate = draft[property.key];
    if (property.kind === "string") {
      value[property.key] = typeof candidate === "string" ? candidate.trim() : String(candidate ?? "").trim();
    } else if (property.kind === "integer") {
      value[property.key] = typeof candidate === "number" ? candidate : Number(candidate);
    } else {
      value[property.key] = candidate === true;
    }
  }
  return value;
}

export function validateObjectRosterDraft(
  field: RosterField,
  value: ObjectRosterDraft,
  locale: LocaleCode
): string | null {
  for (const property of readObjectRosterProperties(field)) {
    const candidate = value[property.key];
    if (property.kind === "string") {
      const text = typeof candidate === "string" ? candidate : "";
      if (property.required && text.length === 0) {
        return selectLocaleText(locale, `请填写${property.title}。`, `Enter ${property.title}.`);
      }
      if (text.length > 0 && property.enumValues.length > 0 && !property.enumValues.includes(text)) {
        return selectLocaleText(locale, `${property.title}不是允许的选项。`, `${property.title} is not an allowed option.`);
      }
      if (!matchesPlayerAccessDateFormat(text, property.property.format)) {
        return selectLocaleText(locale, `${property.title}日期或时间无效。`, `${property.title} has an invalid date or time.`);
      }
      const pattern = typeof property.property.pattern === "string" ? property.property.pattern : "";
      if (text.length > 0 && pattern) {
        try {
          if (!new RegExp(pattern).test(text)) {
            return selectLocaleText(locale, `${property.title}格式无效。`, `${property.title} has an invalid format.`);
          }
        } catch {
          return selectLocaleText(locale, `${property.title}的模块校验规则无效。`, `${property.title} has an invalid module validation rule.`);
        }
      }
    } else if (property.kind === "integer" && !Number.isInteger(candidate)) {
      return selectLocaleText(locale, `${property.title}必须是整数。`, `${property.title} must be an integer.`);
    } else if (property.kind === "boolean" && typeof candidate !== "boolean") {
      return selectLocaleText(locale, `${property.title}必须为开或关。`, `${property.title} must be on or off.`);
    }
  }

  const codec = parsePlayerAccessCodec(field.property);
  if (codec === "object_identity" && !canonicalPlayerAccessAdditionIdentity(codec, value, field.property)) {
    return selectLocaleText(locale, "请完整填写有效的身份字段。", "Complete all identity fields with valid values.");
  }
  return null;
}
