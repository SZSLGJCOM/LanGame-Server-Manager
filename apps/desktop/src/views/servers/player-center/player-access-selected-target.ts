import {
  canonicalPlayerAccessIdentity,
  normalizePlayerAccessMutationEntry,
  parsePlayerAccessCodec,
  playerAccessObjectIdentityKeys
} from "../../../domain/player-access";
import type { RuntimeLivePlayerEntry, RuntimePlayerIdentityKind } from "../../../types";
import type { PlayerAccessRosterCapability, RawSchemaProperty } from "./player-access-roster-model";

export interface SelectedRosterTarget {
  identity: string;
  rawValue: string | Record<string, unknown>;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function identityValue(player: RuntimeLivePlayerEntry, kinds: RuntimePlayerIdentityKind[]): string | null {
  const values = new Set(player.identifiers
    .filter((identifier) => kinds.includes(identifier.kind)
      && (identifier.stable || identifier.kind === "player_name"))
    .map((identifier) => identifier.value.trim())
    .filter(Boolean));
  // A conflicting pair must be resolved by the user, never by array order.
  return values.size === 1 ? Array.from(values)[0] : null;
}

export function isPlayerRosterCapability(field: PlayerAccessRosterCapability): boolean {
  const sourceKey = field.property["x-lsgm-source-key"];
  const codec = parsePlayerAccessCodec(field.property);
  return field.property.readOnly !== true && codec !== null && codec !== "minecraft_ip_csv"
    && !/(?:^|_)groups?(?:_|$)/i.test(field.key)
    && !(typeof sourceKey === "string" && /(?:^|\/)groups?$/i.test(sourceKey));
}

function objectIdentityTarget(player: RuntimeLivePlayerEntry, field: PlayerAccessRosterCapability): Record<string, unknown> | null {
  const properties = field.property.items?.properties;
  if (!properties || !isPlayerRosterCapability(field)) return null;
  const identityKeys = playerAccessObjectIdentityKeys(field.property);
  if (identityKeys.length === 0) return null;
  const result: Record<string, unknown> = {};

  for (const key of identityKeys) {
    const property: RawSchemaProperty | undefined = properties[key];
    const platformKey = property?.["x-lsgm-player-access-platform-field"];
    if (typeof platformKey !== "string" || !identityKeys.includes(platformKey)) continue;
    const platforms: [RuntimePlayerIdentityKind, string][] = [["steam_id", "Steam"], ["eos_id", "EOS"]];
    const allowed = properties[platformKey]?.enum;
    const candidates = platforms.flatMap(([kind, platform]) => {
      const value = identityValue(player, [kind]);
      return value && Array.isArray(allowed) && allowed.includes(platform) ? [{ platform, value }] : [];
    });
    if (candidates.length !== 1) return null;
    result[key] = candidates[0].value;
    result[platformKey] = candidates[0].platform;
  }

  const kindsByKey: Record<string, RuntimePlayerIdentityKind[]> = {
    steam_id: ["steam_id"], steamid: ["steam_id"], steam64_id: ["steam_id"], steam64: ["steam_id"],
    uuid: ["minecraft_uuid"], minecraft_uuid: ["minecraft_uuid"], eos_id: ["eos_id"],
    klei_user_id: ["klei_user_id"], name: ["player_name"], player_name: ["player_name"]
  };
  for (const key of identityKeys) {
    if (result[key] !== undefined) continue;
    const kinds = kindsByKey[key.toLowerCase()];
    const value = kinds ? identityValue(player, kinds) : null;
    if (!value) return null;
    result[key] = value;
  }

  return result;
}

function objectTarget(player: RuntimeLivePlayerEntry, field: PlayerAccessRosterCapability): Record<string, unknown> | null {
  const result = objectIdentityTarget(player, field);
  const properties = field.property.items?.properties;
  if (!result || !properties) return null;
  const required = field.property.items?.required;
  if (Array.isArray(required)) {
    for (const key of required) {
      if (typeof key !== "string") return null;
      if (result[key] === undefined) {
        if (properties[key]?.default === undefined) return null;
        result[key] = properties[key].default;
      }
      if (typeof result[key] === "string" && !result[key].trim()) return null;
    }
  }
  return result;
}

function delimitedTarget(
  player: RuntimeLivePlayerEntry,
  property: RawSchemaProperty,
  initialValue: string | null,
  delimiter: string
): string | null {
  const fields = property["x-lsgm-player-access-delimited-fields"];
  if (!initialValue || !Array.isArray(fields) || fields.length === 0) return null;
  const parts = [initialValue];
  for (const field of fields.slice(1)) {
    if (!isRecord(field)) return null;
    if (field.required !== true) break;
    // A display label is not evidence of a Minecraft account name.
    const value = field.format === "minecraft_name" ? identityValue(player, ["player_name"]) : null;
    if (!value || value.includes(delimiter)) return null;
    parts.push(value);
  }
  return parts.join(delimiter);
}

/** Resolve only identities whose namespace matches the roster's declared codec. */
export function resolveSelectedRosterTarget(
  player: RuntimeLivePlayerEntry | null,
  field: PlayerAccessRosterCapability
): SelectedRosterTarget | null {
  if (!player || !isPlayerRosterCapability(field)) return null;
  const codec = parsePlayerAccessCodec(field.property);
  if (!codec) return null;
  let candidate: string | Record<string, unknown> | null = null;
  switch (codec) {
    case "steam64":
      candidate = identityValue(player, ["steam_id"]);
      break;
    case "dst_klei_id":
      candidate = identityValue(player, ["klei_user_id"]);
      break;
    case "ark_account_id":
      candidate = identityValue(player, ["ark_account_id", "steam_id", "eos_id"]);
      break;
    case "pipe_steam64":
      candidate = delimitedTarget(player, field.property, identityValue(player, ["steam_id"]), "|");
      break;
    case "barotrauma_account":
      candidate = delimitedTarget(player, field.property, identityValue(player, ["steam_id"]), ",");
      break;
    case "csv_uuid_name":
      candidate = delimitedTarget(player, field.property, identityValue(player, ["minecraft_uuid"]), ",");
      break;
    case "terraria_banlist":
      candidate = identityValue(player, ["player_name"]);
      break;
    case "plain":
      // Existing name-only rosters have an explicit storage contract; plain text
      // by itself does not declare whether an account ID or player name is expected.
      if (field.key === "owner_name" && field.property["x-lsgm-source-key"] === "-owner") {
        candidate = identityValue(player, ["player_name"]);
      } else if (field.property["x-lsgm-source"] === "whitelist_config_json"
        && field.property["x-lsgm-source-key"] === "WhitelistedUsers") {
        candidate = identityValue(player, ["player_name"]);
      }
      break;
    case "object_identity":
      candidate = objectTarget(player, field);
      break;
    // Native account hashes, IPs and Valheim platform IDs have no live identity namespace.
    // HumanitZ info provides names only; an EOS PUID is not its composite NetID.
    case "humanitz_net_id":
    case "uint64":
    case "minecraft_ip_csv":
    case "valheim_platform_id":
      return null;
  }
  if (candidate === null) return null;
  const normalized = normalizePlayerAccessMutationEntry(codec, candidate, field.property, "add");
  return normalized && (typeof normalized.stored === "string" || isRecord(normalized.stored))
    ? { identity: normalized.identity, rawValue: normalized.stored }
    : null;
}

/** Resolve only proven identity fields; the editor must validate all remaining data before adding. */
export function resolveSelectedRosterIdentity(
  player: RuntimeLivePlayerEntry | null,
  field: PlayerAccessRosterCapability
): SelectedRosterTarget | null {
  if (!player || !isPlayerRosterCapability(field)) return null;
  if (parsePlayerAccessCodec(field.property) !== "object_identity") return resolveSelectedRosterTarget(player, field);
  const rawValue = objectIdentityTarget(player, field);
  const identity = rawValue ? canonicalPlayerAccessIdentity("object_identity", rawValue, field.property) : null;
  return rawValue && identity ? { identity, rawValue } : null;
}
