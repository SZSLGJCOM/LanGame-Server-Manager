import { canonicalPlayerAccessIdentity, normalizePlayerAccessMutationEntry, parsePlayerAccessCodec, playerAccessObjectIdentityKeys } from "../../../domain/player-access";
import { isAuthoritativeLivePlayerSnapshot } from "../../../domain/live-player-state";
import type { RuntimeLivePlayerEntry, RuntimeLivePlayerIdentifier, RuntimeLivePlayerSnapshot, RuntimePlayerIdentityKind } from "../../../types";
import { isRosterRecord, type RosterEntry, type RosterField } from "./player-access-roster-model";
import { isPlayerRosterCapability, resolveSelectedRosterIdentity, type SelectedRosterTarget } from "./player-access-selected-target";

export interface SelectedRosterEntry { field: RosterField; entry: RosterEntry }

/** A roster identity can fill another roster, but never authorizes a runtime command. */
export function rosterEntryPlayer(selection: SelectedRosterEntry | null): RuntimeLivePlayerEntry | null {
  if (!selection || !isPlayerRosterCapability(selection.field)) return null;
  const { field, entry } = selection;
  const codec = parsePlayerAccessCodec(field.property);
  if (!codec || canonicalPlayerAccessIdentity(codec, entry.rawValue, field.property) !== entry.key) return null;
  const identifiers: RuntimeLivePlayerIdentifier[] = [];
  const add = (kind: RuntimePlayerIdentityKind, value: unknown) => {
    if (typeof value === "string" && value.trim()) identifiers.push({ kind, value: value.trim(), stable: true });
  };
  if (codec === "object_identity" && isRosterRecord(entry.rawValue)) {
    const values = entry.rawValue;
    const keys = playerAccessObjectIdentityKeys(field.property);
    const kinds: Record<string, RuntimePlayerIdentityKind> = { steam_id: "steam_id", steamid: "steam_id", steam64_id: "steam_id",
      steam64: "steam_id", eos_id: "eos_id", uuid: "minecraft_uuid", minecraft_uuid: "minecraft_uuid",
      klei_user_id: "klei_user_id", name: "player_name", player_name: "player_name" };
    for (const key of keys) {
      const platformKey = field.property.items?.properties?.[key]?.["x-lsgm-player-access-platform-field"];
      if (typeof platformKey === "string" && keys.includes(platformKey)) {
        if (values[platformKey] === "Steam") add("steam_id", values[key]);
        if (values[platformKey] === "EOS") add("eos_id", values[key]);
      } else if (kinds[key.toLowerCase()]) add(kinds[key.toLowerCase()], values[key]);
    }
  } else if (codec === "steam64" || codec === "pipe_steam64" || codec === "barotrauma_account") add("steam_id", entry.key);
  else if (codec === "dst_klei_id") add("klei_user_id", entry.key);
  else if (codec === "ark_account_id") add("ark_account_id", entry.key);
  else if (codec === "csv_uuid_name") {
    add("minecraft_uuid", entry.key);
    if (typeof entry.rawValue === "string") add("player_name", entry.rawValue.split(",")[1]);
  } else if (codec === "terraria_banlist") add("player_name", entry.key);
  else if (codec === "plain" && (field.property["x-lsgm-source-key"] === "-owner"
    || field.property["x-lsgm-source-key"] === "WhitelistedUsers")) add("player_name", entry.key);
  if (!identifiers.length) return null;
  return { player_key: `roster:${field.key}:${entry.key}`, display_name: entry.label, identifiers,
    available_action_ids: [], ping_ms: null, session_started_at_unix_ms: null, role: null, attributes: [] };
}

export function resolveRosterActionTarget(
  player: RuntimeLivePlayerEntry | null, selection: SelectedRosterEntry | null, field: RosterField
): SelectedRosterTarget | null {
  if (selection?.field.key === field.key) return typeof selection.entry.rawValue === "string" || isRosterRecord(selection.entry.rawValue)
    ? { identity: selection.entry.key, rawValue: selection.entry.rawValue } : null;
  const fromPlayer = resolveSelectedRosterIdentity(player, field);
  if (fromPlayer) return fromPlayer;
  // Shared explicit scalar codecs can carry identities not represented by the live model.
  const sourceCodec = selection && parsePlayerAccessCodec(selection.field.property);
  if (!selection || !sourceCodec || sourceCodec === "object_identity" || sourceCodec === "plain" || !isPlayerRosterCapability(selection.field)
    || !isPlayerRosterCapability(field) || sourceCodec !== parsePlayerAccessCodec(field.property)) return null;
  const normalized = normalizePlayerAccessMutationEntry(sourceCodec, selection.entry.rawValue, field.property, "add");
  return normalized && (typeof normalized.stored === "string" || isRosterRecord(normalized.stored))
    ? { identity: normalized.identity, rawValue: normalized.stored } : null;
}

/** Match against real fresh rows only; duplicate accounts cannot acquire runtime authority. */
export function matchRosterLivePlayer(selection: SelectedRosterEntry | null, snapshot: RuntimeLivePlayerSnapshot | null, now: number): RuntimeLivePlayerEntry | null {
  if (!selection || !isPlayerRosterCapability(selection.field) || !isAuthoritativeLivePlayerSnapshot(snapshot, now)) return null;
  const matches = snapshot.entries.filter((player) => resolveSelectedRosterIdentity(player, selection.field)?.identity === selection.entry.key);
  return matches.length === 1 ? matches[0] : null;
}
